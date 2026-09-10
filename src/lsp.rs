use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use crate::diagnostic::{Diagnostic, SourceId, SourceSpan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PositionEncoding {
    Utf8,
    Utf16,
}

struct LspRequestContext<'a> {
    encoding: PositionEncoding,
    cache: Option<&'a mut crate::project::ProjectAnalysisCache>,
}

#[derive(Debug, Clone, PartialEq)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(i64),
    RawNumber(String),
    String(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}

impl JsonValue {
    fn get(&self, key: &str) -> Option<&Self> {
        let Self::Object(values) = self else {
            return None;
        };
        values.get(key)
    }

    fn as_str(&self) -> Option<&str> {
        let Self::String(value) = self else {
            return None;
        };
        Some(value)
    }

    fn as_array(&self) -> Option<&[Self]> {
        let Self::Array(values) = self else {
            return None;
        };
        Some(values)
    }

    fn as_usize(&self) -> Option<usize> {
        let Self::Number(value) = self else {
            return None;
        };
        usize::try_from(*value).ok()
    }

    fn to_json(&self) -> String {
        match self {
            Self::Null => "null".to_string(),
            Self::Bool(value) => value.to_string(),
            Self::Number(value) => value.to_string(),
            Self::RawNumber(value) => value.clone(),
            Self::String(value) => json_string(value),
            Self::Array(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(Self::to_json)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Object(values) => format!(
                "{{{}}}",
                values
                    .iter()
                    .map(|(key, value)| format!("{}:{}", json_string(key), value.to_json()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
}

pub fn run_stdio() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    run_server(stdin.lock(), stdout.lock())
}

fn run_server<R: BufRead, W: Write>(mut reader: R, mut writer: W) -> io::Result<()> {
    let mut documents = HashMap::<String, String>::new();
    let mut analysis_cache = crate::project::ProjectAnalysisCache::default();
    let mut encoding = PositionEncoding::Utf16;
    let mut shutdown = false;

    while let Some(payload) = read_message(&mut reader)? {
        let message = match parse_json(&payload) {
            Ok(message) => message,
            Err(message) => {
                write_message(
                    &mut writer,
                    &jsonrpc_error(JsonValue::Null, -32700, &message).to_json(),
                )?;
                continue;
            }
        };
        let method = message.get("method").and_then(JsonValue::as_str);
        let id = message.get("id").cloned();
        if shutdown && method != Some("exit") {
            if let Some(id) = id {
                write_message(
                    &mut writer,
                    &jsonrpc_error(id, -32600, "server has already shut down").to_json(),
                )?;
            }
            continue;
        }

        match method {
            Some("initialize") => {
                encoding = negotiate_position_encoding(message.get("params"));
                if let Some(id) = id {
                    write_message(&mut writer, &initialize_response(id, encoding).to_json())?;
                }
            }
            Some("initialized") => {}
            Some("shutdown") => {
                shutdown = true;
                if let Some(id) = id {
                    write_message(&mut writer, &jsonrpc_result(id, JsonValue::Null).to_json())?;
                }
            }
            Some("exit") => return Ok(()),
            Some("textDocument/didOpen") => {
                if let Some(params) = message.get("params")
                    && let (Some(uri), Some(text)) = (
                        params
                            .get("textDocument")
                            .and_then(|doc| doc.get("uri"))
                            .and_then(JsonValue::as_str),
                        params
                            .get("textDocument")
                            .and_then(|doc| doc.get("text"))
                            .and_then(JsonValue::as_str),
                    )
                {
                    invalidate_lsp_analysis_path(&mut analysis_cache, uri);
                    documents.insert(uri.to_string(), text.to_string());
                    publish_workspace_diagnostics(&mut writer, &documents, encoding)?;
                }
            }
            Some("textDocument/didChange") => {
                if let Some(params) = message.get("params")
                    && let Some(uri) = params
                        .get("textDocument")
                        .and_then(|doc| doc.get("uri"))
                        .and_then(JsonValue::as_str)
                    && documents.contains_key(uri)
                    && let Some(text) = params
                        .get("contentChanges")
                        .and_then(JsonValue::as_array)
                        .and_then(|changes| changes.last())
                        .and_then(|change| change.get("text"))
                        .and_then(JsonValue::as_str)
                {
                    invalidate_lsp_analysis_path(&mut analysis_cache, uri);
                    documents.insert(uri.to_string(), text.to_string());
                    publish_workspace_diagnostics(&mut writer, &documents, encoding)?;
                }
            }
            Some("textDocument/didClose") => {
                if let Some(uri) = message
                    .get("params")
                    .and_then(|params| params.get("textDocument"))
                    .and_then(|doc| doc.get("uri"))
                    .and_then(JsonValue::as_str)
                {
                    invalidate_lsp_analysis_path(&mut analysis_cache, uri);
                    documents.remove(uri);
                    publish_empty_diagnostics(&mut writer, uri)?;
                    publish_workspace_diagnostics(&mut writer, &documents, encoding)?;
                }
            }
            Some("textDocument/formatting") => {
                if let Some(id) = id {
                    let uri = message
                        .get("params")
                        .and_then(|params| params.get("textDocument"))
                        .and_then(|doc| doc.get("uri"))
                        .and_then(JsonValue::as_str);
                    let edits = uri
                        .and_then(|uri| documents.get(uri))
                        .and_then(|source| format_document(source, encoding));
                    write_message(
                        &mut writer,
                        &jsonrpc_result(id, JsonValue::Array(edits.unwrap_or_default())).to_json(),
                    )?;
                }
            }
            Some("textDocument/codeAction") => {
                if let Some(id) = id {
                    let uri = message
                        .get("params")
                        .and_then(|params| params.get("textDocument"))
                        .and_then(|doc| doc.get("uri"))
                        .and_then(JsonValue::as_str);
                    let actions = uri
                        .and_then(|uri| documents.get(uri).map(|source| (uri, source)))
                        .map(|(uri, source)| code_actions(uri, source, &documents, encoding))
                        .unwrap_or_default();
                    write_message(
                        &mut writer,
                        &jsonrpc_result(id, JsonValue::Array(actions)).to_json(),
                    )?;
                }
            }
            Some("textDocument/hover") => {
                if let Some(id) = id {
                    let params = message.get("params");
                    let uri = params
                        .and_then(|params| params.get("textDocument"))
                        .and_then(|doc| doc.get("uri"))
                        .and_then(JsonValue::as_str);
                    let line = params
                        .and_then(|params| params.get("position"))
                        .and_then(|position| position.get("line"))
                        .and_then(JsonValue::as_usize);
                    let character = params
                        .and_then(|params| params.get("position"))
                        .and_then(|position| position.get("character"))
                        .and_then(JsonValue::as_usize);
                    let hover = match (uri, line, character) {
                        (Some(uri), Some(line), Some(character)) => {
                            documents.get(uri).and_then(|source| {
                                hover_for_document_cached(
                                    uri,
                                    source,
                                    &documents,
                                    line,
                                    character,
                                    encoding,
                                    Some(&mut analysis_cache),
                                )
                            })
                        }
                        _ => None,
                    };
                    write_message(
                        &mut writer,
                        &jsonrpc_result(id, hover.unwrap_or(JsonValue::Null)).to_json(),
                    )?;
                }
            }
            Some("textDocument/inlayHint") => {
                if let Some(id) = id {
                    let params = message.get("params");
                    let uri = params
                        .and_then(|params| params.get("textDocument"))
                        .and_then(|doc| doc.get("uri"))
                        .and_then(JsonValue::as_str);
                    let start_line = params
                        .and_then(|params| params.get("range"))
                        .and_then(|range| range.get("start"))
                        .and_then(|start| start.get("line"))
                        .and_then(JsonValue::as_usize)
                        .unwrap_or(0);
                    let end_line = params
                        .and_then(|params| params.get("range"))
                        .and_then(|range| range.get("end"))
                        .and_then(|end| end.get("line"))
                        .and_then(JsonValue::as_usize)
                        .unwrap_or(usize::MAX);
                    let hints = uri
                        .and_then(|uri| documents.get(uri).map(|source| (uri, source)))
                        .map(|(uri, source)| {
                            inlay_hints_for_document(
                                uri, source, &documents, start_line, end_line, encoding,
                            )
                        })
                        .unwrap_or_default();
                    write_message(
                        &mut writer,
                        &jsonrpc_result(id, JsonValue::Array(hints)).to_json(),
                    )?;
                }
            }
            Some("textDocument/semanticTokens/full") => {
                if let Some(id) = id {
                    let uri = message
                        .get("params")
                        .and_then(|params| params.get("textDocument"))
                        .and_then(|doc| doc.get("uri"))
                        .and_then(JsonValue::as_str);
                    let data = uri
                        .and_then(|uri| {
                            documents
                                .get(uri)
                                .map(|source| semantic_tokens(uri, source, encoding))
                        })
                        .unwrap_or_default();
                    write_message(
                        &mut writer,
                        &jsonrpc_result(id, object([("data", JsonValue::Array(data))])).to_json(),
                    )?;
                }
            }
            Some("textDocument/definition") => {
                if let Some(id) = id {
                    let result = request_position(message.get("params")).and_then(
                        |(uri, line, character)| {
                            documents.get(uri).and_then(|source| {
                                definition_for_document_cached(
                                    uri,
                                    source,
                                    &documents,
                                    line,
                                    character,
                                    encoding,
                                    Some(&mut analysis_cache),
                                )
                            })
                        },
                    );
                    write_message(
                        &mut writer,
                        &jsonrpc_result(id, result.unwrap_or(JsonValue::Null)).to_json(),
                    )?;
                }
            }
            Some("textDocument/references") => {
                if let Some(id) = id {
                    let result = request_position(message.get("params"))
                        .and_then(|(uri, line, character)| {
                            documents.get(uri).map(|source| {
                                references_for_document_cached(
                                    uri,
                                    source,
                                    &documents,
                                    line,
                                    character,
                                    encoding,
                                    Some(&mut analysis_cache),
                                )
                            })
                        })
                        .unwrap_or_default();
                    write_message(
                        &mut writer,
                        &jsonrpc_result(id, JsonValue::Array(result)).to_json(),
                    )?;
                }
            }
            Some("textDocument/rename") => {
                if let Some(id) = id {
                    let params = message.get("params");
                    let new_name = params
                        .and_then(|params| params.get("newName"))
                        .and_then(JsonValue::as_str);
                    let result = request_position(params).and_then(|(uri, line, character)| {
                        let new_name = new_name?;
                        documents.get(uri).and_then(|source| {
                            rename_for_document_cached(
                                uri,
                                source,
                                &documents,
                                line,
                                character,
                                new_name,
                                LspRequestContext {
                                    encoding,
                                    cache: Some(&mut analysis_cache),
                                },
                            )
                        })
                    });
                    write_message(
                        &mut writer,
                        &jsonrpc_result(id, result.unwrap_or(JsonValue::Null)).to_json(),
                    )?;
                }
            }
            Some("textDocument/completion") => {
                if let Some(id) = id {
                    let params = message.get("params");
                    let uri = params
                        .and_then(|params| params.get("textDocument"))
                        .and_then(|doc| doc.get("uri"))
                        .and_then(JsonValue::as_str);
                    let line = params
                        .and_then(|params| params.get("position"))
                        .and_then(|position| position.get("line"))
                        .and_then(JsonValue::as_usize);
                    let character = params
                        .and_then(|params| params.get("position"))
                        .and_then(|position| position.get("character"))
                        .and_then(JsonValue::as_usize);
                    let items = uri
                        .and_then(|uri| documents.get(uri).map(|source| (uri, source)))
                        .map(|(uri, source)| {
                            completion_items_at_cursor_cached(
                                uri,
                                source,
                                &documents,
                                line,
                                character,
                                encoding,
                                Some(&mut analysis_cache),
                            )
                        })
                        .unwrap_or_else(|| completion_items(""));
                    write_message(
                        &mut writer,
                        &jsonrpc_result(id, JsonValue::Array(items)).to_json(),
                    )?;
                }
            }
            Some("flux/hotReloadStatus") => {
                if let Some(id) = id {
                    let uri = message
                        .get("params")
                        .and_then(|params| params.get("textDocument"))
                        .and_then(|doc| doc.get("uri"))
                        .and_then(JsonValue::as_str);
                    let status = uri
                        .and_then(|uri| hot_reload_status(uri, &documents))
                        .unwrap_or(JsonValue::Null);
                    write_message(&mut writer, &jsonrpc_result(id, status).to_json())?;
                }
            }
            Some("textDocument/signatureHelp") => {
                if let Some(id) = id {
                    let result = request_position(message.get("params")).and_then(
                        |(uri, line, character)| {
                            documents.get(uri).and_then(|source| {
                                signature_help_for_document_cached(
                                    uri,
                                    source,
                                    &documents,
                                    line,
                                    character,
                                    encoding,
                                    Some(&mut analysis_cache),
                                )
                            })
                        },
                    );
                    write_message(
                        &mut writer,
                        &jsonrpc_result(id, result.unwrap_or(JsonValue::Null)).to_json(),
                    )?;
                }
            }
            Some(_) if id.is_some() => {
                write_message(
                    &mut writer,
                    &jsonrpc_error(id.expect("checked request id"), -32601, "method not found")
                        .to_json(),
                )?;
            }
            None if id.is_some() => {
                write_message(
                    &mut writer,
                    &jsonrpc_error(id.expect("checked request id"), -32600, "invalid request")
                        .to_json(),
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn initialize_response(id: JsonValue, encoding: PositionEncoding) -> JsonValue {
    let position_encoding = match encoding {
        PositionEncoding::Utf8 => "utf-8",
        PositionEncoding::Utf16 => "utf-16",
    };
    jsonrpc_result(
        id,
        object([
            (
                "capabilities",
                object([
                    (
                        "positionEncoding",
                        JsonValue::String(position_encoding.to_string()),
                    ),
                    ("codeActionProvider", JsonValue::Bool(true)),
                    (
                        "completionProvider",
                        object([
                            ("resolveProvider", JsonValue::Bool(false)),
                            (
                                "triggerCharacters",
                                JsonValue::Array(vec![
                                    JsonValue::String(".".to_string()),
                                    JsonValue::String(":".to_string()),
                                ]),
                            ),
                        ]),
                    ),
                    ("documentFormattingProvider", JsonValue::Bool(true)),
                    ("hoverProvider", JsonValue::Bool(true)),
                    (
                        "experimental",
                        object([("fluxHotReloadStatus", JsonValue::Bool(true))]),
                    ),
                    ("inlayHintProvider", JsonValue::Bool(true)),
                    ("definitionProvider", JsonValue::Bool(true)),
                    ("referencesProvider", JsonValue::Bool(true)),
                    ("renameProvider", JsonValue::Bool(true)),
                    (
                        "signatureHelpProvider",
                        object([(
                            "triggerCharacters",
                            JsonValue::Array(vec![
                                JsonValue::String("(".to_string()),
                                JsonValue::String(",".to_string()),
                            ]),
                        )]),
                    ),
                    (
                        "semanticTokensProvider",
                        object([
                            (
                                "legend",
                                object([
                                    (
                                        "tokenTypes",
                                        JsonValue::Array(
                                            SEMANTIC_TOKEN_TYPES
                                                .iter()
                                                .map(|token| {
                                                    JsonValue::String((*token).to_string())
                                                })
                                                .collect(),
                                        ),
                                    ),
                                    ("tokenModifiers", JsonValue::Array(Vec::new())),
                                ]),
                            ),
                            ("full", JsonValue::Bool(true)),
                        ]),
                    ),
                    (
                        "textDocumentSync",
                        object([
                            ("openClose", JsonValue::Bool(true)),
                            ("change", JsonValue::Number(1)),
                        ]),
                    ),
                ]),
            ),
            (
                "serverInfo",
                object([
                    (
                        "name",
                        JsonValue::String("Flux Language Server".to_string()),
                    ),
                    (
                        "version",
                        JsonValue::String(env!("CARGO_PKG_VERSION").to_string()),
                    ),
                ]),
            ),
        ]),
    )
}

fn negotiate_position_encoding(params: Option<&JsonValue>) -> PositionEncoding {
    let encodings = params
        .and_then(|params| params.get("capabilities"))
        .and_then(|capabilities| capabilities.get("general"))
        .and_then(|general| general.get("positionEncodings"))
        .and_then(JsonValue::as_array);
    if encodings.is_some_and(|values| values.iter().any(|value| value.as_str() == Some("utf-8"))) {
        PositionEncoding::Utf8
    } else {
        PositionEncoding::Utf16
    }
}

fn format_document(source: &str, encoding: PositionEncoding) -> Option<Vec<JsonValue>> {
    let formatted = crate::formatter::format_source(source).ok()?;
    if formatted == source {
        return Some(Vec::new());
    }
    Some(vec![object([
        ("range", whole_document_range(source, encoding)),
        ("newText", JsonValue::String(formatted)),
    ])])
}

fn whole_document_range(source: &str, encoding: PositionEncoding) -> JsonValue {
    let line_count = source.lines().count().max(1);
    let final_line = source.lines().last().unwrap_or("");
    let end_line = if source.ends_with('\n') {
        line_count
    } else {
        line_count.saturating_sub(1)
    };
    let end_character = if source.ends_with('\n') {
        0
    } else {
        encoded_column(final_line, final_line.len(), encoding)
    };
    object([
        (
            "start",
            object([
                ("line", JsonValue::Number(0)),
                ("character", JsonValue::Number(0)),
            ]),
        ),
        (
            "end",
            object([
                ("line", JsonValue::Number(end_line as i64)),
                ("character", JsonValue::Number(end_character as i64)),
            ]),
        ),
    ])
}

const COMPLETION_KEYWORDS: &[&str] = &[
    "fn",
    "let",
    "var",
    "return",
    "if",
    "elif",
    "else",
    "for",
    "while",
    "in",
    "break",
    "continue",
    "match",
    "struct",
    "enum",
    "interface",
    "impl",
    "type",
    "const",
    "pub",
    "import",
    "view",
    "app",
    "state",
    "derived",
    "true",
    "false",
    "nil",
];

fn completion_items(source: &str) -> Vec<JsonValue> {
    let mut items = Vec::new();
    let mut seen = HashSet::<String>::new();
    for keyword in COMPLETION_KEYWORDS {
        push_completion_item(&mut items, &mut seen, keyword, 14, "Flux keyword");
    }
    for builtin in ["i64", "bool", "str", "error", "void"] {
        push_completion_item(&mut items, &mut seen, builtin, 22, "built-in Flux type");
    }
    push_completion_item(&mut items, &mut seen, "print", 3, "fn print(value) -> void");
    push_completion_item(
        &mut items,
        &mut seen,
        "take",
        3,
        "fn take(list: T[], count: i64) -> T[]",
    );
    push_completion_item(
        &mut items,
        &mut seen,
        "skip",
        3,
        "fn skip(list: T[], count: i64) -> T[]",
    );
    push_completion_item(
        &mut items,
        &mut seen,
        "any",
        3,
        "fn any(list: bool[]) -> bool",
    );
    push_completion_item(
        &mut items,
        &mut seen,
        "every",
        3,
        "fn every(list: bool[]) -> bool",
    );
    push_completion_item(
        &mut items,
        &mut seen,
        "fold",
        3,
        "fn fold(list: T[], initial: A, reducer: fn(A, T) -> A) -> A",
    );
    push_completion_item(
        &mut items,
        &mut seen,
        "reduce",
        3,
        "fn reduce(list: T[], reducer: fn(T, T) -> T) -> T",
    );
    push_completion_item(
        &mut items,
        &mut seen,
        "map",
        3,
        "fn map(list: T[], callback: fn(T) -> U) -> U[]",
    );
    push_completion_item(
        &mut items,
        &mut seen,
        "filter",
        3,
        "fn filter(list: T[], predicate: fn(T) -> bool) -> T[]",
    );
    push_completion_item(
        &mut items,
        &mut seen,
        "where",
        3,
        "fn where(list: T[], predicate: fn(T) -> bool) -> T[]",
    );
    push_completion_item(
        &mut items,
        &mut seen,
        "concat",
        3,
        "fn concat(left: T[], right: T[]) -> T[]",
    );
    push_completion_item(
        &mut items,
        &mut seen,
        "distinct",
        3,
        "fn distinct(list: scalar[]) -> scalar[]",
    );
    push_completion_item(
        &mut items,
        &mut seen,
        "flatten",
        3,
        "fn flatten(list: T[][]) -> T[]",
    );
    push_completion_item(
        &mut items,
        &mut seen,
        "sorted",
        3,
        "fn sorted(list: ordered[]) -> same ordered list type",
    );
    push_completion_item(
        &mut items,
        &mut seen,
        "chunked",
        3,
        "fn chunked(list: T[], size: i64) -> T[][]",
    );

    let Ok(program) = crate::parser::parse_all(source) else {
        return items;
    };
    for alias in &program.aliases {
        push_completion_item(
            &mut items,
            &mut seen,
            &alias.name,
            18,
            &format!("type {} = {}", alias.name, alias.target.name()),
        );
    }
    for interface in &program.interfaces {
        push_completion_item(
            &mut items,
            &mut seen,
            &interface.name,
            8,
            &format!("interface {}", interface.name),
        );
    }
    for definition in &program.structs {
        push_completion_item(
            &mut items,
            &mut seen,
            &definition.name,
            22,
            &format!("struct {}", definition.name),
        );
    }
    for definition in &program.enums {
        push_completion_item(
            &mut items,
            &mut seen,
            &definition.name,
            13,
            &format!("enum {}", definition.name),
        );
    }
    for constant in &program.constants {
        push_completion_item(
            &mut items,
            &mut seen,
            &constant.name,
            21,
            &format!("const {}: {}", constant.name, constant.ty.name()),
        );
    }
    for view in &program.views {
        push_completion_item(
            &mut items,
            &mut seen,
            &view.name,
            22,
            &format!("view {}", view.name),
        );
    }
    for function in &program.functions {
        push_completion_item(
            &mut items,
            &mut seen,
            &function.name,
            3,
            &format_ast_function_signature(function),
        );
    }
    items
}

#[cfg(test)]
fn completion_items_at_cursor(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: Option<usize>,
    character: Option<usize>,
    encoding: PositionEncoding,
) -> Vec<JsonValue> {
    completion_items_at_cursor_cached(
        uri, source, documents, line_index, character, encoding, None,
    )
}

fn completion_items_at_cursor_cached(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: Option<usize>,
    character: Option<usize>,
    encoding: PositionEncoding,
    cache: Option<&mut crate::project::ProjectAnalysisCache>,
) -> Vec<JsonValue> {
    let mut items = completion_items_at_position(uri, source, documents, line_index);
    let mut seen = items
        .iter()
        .filter_map(|item| item.get("label").and_then(JsonValue::as_str))
        .map(str::to_string)
        .collect::<HashSet<_>>();
    let (Some(line_index), Some(character)) = (line_index, character) else {
        return items;
    };
    let Some(receiver) = member_receiver_at_cursor(source, line_index, character, encoding) else {
        return items;
    };
    if let Some(program) =
        completion_contract_program_cached(uri, source, documents, line_index, cache)
    {
        let namespace = is_valid_identifier(&receiver).then_some(receiver.as_str());
        let namespace_matched = namespace.is_some_and(|namespace| {
            add_qualified_namespace_completions(&mut items, &mut seen, namespace, &program)
        });
        if !namespace_matched
            && let Some(type_name) =
                member_receiver_type_name(source, line_index, &receiver, &program)
        {
            if matches!(
                crate::ast::Type::parse(&type_name),
                Some(crate::ast::Type::List(_))
            ) {
                add_list_property_completions(&mut items, &mut seen, &type_name);
            } else {
                add_struct_field_completions(&mut items, &mut seen, &type_name, &program);
            }
        }
    }
    items
}

fn completion_items_at_position(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: Option<usize>,
) -> Vec<JsonValue> {
    let mut items = completion_items(source);
    let mut seen = items
        .iter()
        .filter_map(|item| item.get("label").and_then(JsonValue::as_str))
        .map(str::to_string)
        .collect::<HashSet<_>>();
    if let Some(line_index) = line_index {
        add_builtin_ui_context_completions(&mut items, &mut seen, source, line_index);
        add_recovered_view_property_completions(
            &mut items, &mut seen, uri, source, documents, line_index,
        );
    }
    if let Some(line_index) = line_index
        && let Some(database) = analyzed_document(uri, source)
    {
        add_position_local_completions(
            &mut items,
            &mut seen,
            source,
            source_id_for_uri(uri),
            line_index + 1,
            database.program(),
            &database,
        );
    }
    let Some(path) = file_uri_path(uri).and_then(|path| std::fs::canonicalize(path).ok()) else {
        return items;
    };
    let overlays = document_overlays(documents);
    let Ok((program, _)) = crate::project::load_with_overlays(&path, &overlays) else {
        return items;
    };
    let current_id = SourceId::from_name(path.to_string_lossy().as_ref());
    for alias in &program.aliases {
        if alias.name_span.source_id == current_id || alias.public {
            push_completion_item(
                &mut items,
                &mut seen,
                &alias.name,
                18,
                &format!("type {} = {}", alias.name, alias.target.name()),
            );
        }
    }
    for interface in &program.interfaces {
        if interface.name_span.source_id == current_id || interface.public {
            push_completion_item(
                &mut items,
                &mut seen,
                &interface.name,
                8,
                &format!("interface {}", interface.name),
            );
        }
    }
    for definition in &program.structs {
        if definition.name_span.source_id == current_id || definition.public {
            push_completion_item(
                &mut items,
                &mut seen,
                &definition.name,
                22,
                &format!("struct {}", definition.name),
            );
        }
    }
    for definition in &program.enums {
        if definition.name_span.source_id == current_id || definition.public {
            push_completion_item(
                &mut items,
                &mut seen,
                &definition.name,
                13,
                &format!("enum {}", definition.name),
            );
        }
    }
    for constant in &program.constants {
        if constant.name_span.source_id == current_id || constant.public {
            push_completion_item(
                &mut items,
                &mut seen,
                &constant.name,
                21,
                &format!("const {}: {}", constant.name, constant.ty.name()),
            );
        }
    }
    for view in &program.views {
        if view.name_span.source_id == current_id || view.public {
            push_completion_item(
                &mut items,
                &mut seen,
                &view.name,
                22,
                &format!("view {}", view.name),
            );
        }
    }
    for function in &program.functions {
        if function.name_span.source_id == current_id || function.public {
            push_completion_item(
                &mut items,
                &mut seen,
                &function.name,
                3,
                &format_ast_function_signature(function),
            );
        }
    }
    if let Some(line_index) = line_index {
        add_custom_view_property_completions(&mut items, &mut seen, source, line_index, &program);
    }
    if let Some(line_index) = line_index
        && let Ok(signatures) = crate::typecheck::check_all(&program)
    {
        let database =
            crate::semantic::SemanticDatabase::from_analyzed(program.clone(), signatures);
        add_position_local_completions(
            &mut items,
            &mut seen,
            source,
            current_id,
            line_index + 1,
            &program,
            &database,
        );
    }
    items
}

fn member_receiver_at_cursor(
    source: &str,
    line_index: usize,
    character: usize,
    encoding: PositionEncoding,
) -> Option<String> {
    let line = source.lines().nth(line_index)?;
    let cursor = byte_offset_for_encoded_column(line, character, encoding).min(line.len());
    let prefix = line.get(..cursor)?;
    let bytes = prefix.as_bytes();
    let mut member_start = bytes.len();
    while member_start > 0 && is_identifier_byte(bytes[member_start - 1]) {
        member_start -= 1;
    }
    if member_start == 0 || bytes[member_start - 1] != b'.' {
        return None;
    }
    let receiver_end = member_start - 1;
    let receiver_prefix = prefix[..receiver_end].trim_end();
    if receiver_prefix.is_empty() {
        return None;
    }
    if receiver_prefix.ends_with(')') {
        let bytes = receiver_prefix.as_bytes();
        let mut depth = 0usize;
        let mut open = None;
        for index in (0..bytes.len()).rev() {
            match bytes[index] {
                b')' => depth += 1,
                b'(' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        open = Some(index);
                        break;
                    }
                }
                _ => {}
            }
        }
        let open = open?;
        let mut start = open;
        while start > 0 && is_identifier_byte(bytes[start - 1]) {
            start -= 1;
        }
        if start == open {
            return None;
        }
        return Some(receiver_prefix[start..].to_string());
    }
    let bytes = receiver_prefix.as_bytes();
    let mut start = bytes.len();
    while start > 0 && (is_identifier_byte(bytes[start - 1]) || bytes[start - 1] == b'.') {
        start -= 1;
    }
    (start < bytes.len()).then(|| receiver_prefix[start..].to_string())
}

fn completion_contract_program_cached(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
    cache: Option<&mut crate::project::ProjectAnalysisCache>,
) -> Option<crate::ast::Program> {
    if let Some((database, _)) = analyzed_project_document_cached(uri, documents, cache) {
        return Some(database.program().clone());
    }
    let prefix = source_before_active_top_level_declaration(source, line_index);
    let probe = format!("{prefix}fn __flux_lsp_completion_probe() -> i64 {{ 0 }}\n");
    if let Some(path) = file_uri_path(uri).and_then(|path| std::fs::canonicalize(path).ok()) {
        let mut overlays = document_overlays(documents);
        overlays.insert(path.clone(), probe.clone());
        if let Ok((program, _)) = crate::project::load_with_overlays(&path, &overlays) {
            return Some(program);
        }
    }
    crate::parser::parse_all(&probe).ok()
}

fn source_before_active_top_level_declaration(source: &str, line_index: usize) -> &str {
    let mut byte_offset = 0usize;
    let mut declaration_start = None;
    for (index, segment) in source.split_inclusive('\n').enumerate() {
        if index > line_index {
            break;
        }
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        if leading_spaces(line) == 0 {
            let trimmed = line.trim_start();
            if trimmed.starts_with("fn ")
                || trimmed.starts_with("pub fn ")
                || trimmed.starts_with("view ")
                || trimmed.starts_with("pub view ")
            {
                declaration_start = Some(byte_offset);
            }
        }
        byte_offset += segment.len();
    }
    &source[..declaration_start.unwrap_or(byte_offset)]
}

fn add_qualified_namespace_completions(
    items: &mut Vec<JsonValue>,
    seen: &mut HashSet<String>,
    namespace: &str,
    program: &crate::ast::Program,
) -> bool {
    if namespace == "process" {
        push_completion_item(items, seen, "pid", 3, "fn process.pid() -> i64");
        push_completion_item(items, seen, "parentPid", 3, "fn process.parentPid() -> i64");
        push_completion_item(
            items,
            seen,
            "terminationRequested",
            3,
            "fn process.terminationRequested() -> bool",
        );
        push_completion_item(items, seen, "exit", 3, "fn process.exit(code: i64) -> void");
        push_completion_item(
            items,
            seen,
            "hasEnv",
            3,
            "fn process.hasEnv(name: str) -> bool",
        );
        push_completion_item(
            items,
            seen,
            "env",
            3,
            "fn process.env(name: str, fallback: str) -> str",
        );
        return true;
    }
    if namespace == "locale" {
        push_completion_item(items, seen, "language", 3, "fn locale.language() -> str");
        push_completion_item(items, seen, "region", 3, "fn locale.region() -> str");
        return true;
    }
    if namespace == "time" {
        push_completion_item(items, seen, "unixMillis", 3, "fn time.unixMillis() -> i64");
        push_completion_item(
            items,
            seen,
            "monotonicMillis",
            3,
            "fn time.monotonicMillis() -> i64",
        );
        push_completion_item(
            items,
            seen,
            "sleepMillis",
            3,
            "fn time.sleepMillis(durationMs: i64) -> void",
        );
        return true;
    }
    if namespace == "fs" {
        for (label, detail) in [
            ("exists", "fn fs.exists(path: str) -> bool"),
            ("isFile", "fn fs.isFile(path: str) -> bool"),
            ("isDirectory", "fn fs.isDirectory(path: str) -> bool"),
            (
                "createDirectory",
                "fn fs.createDirectory(path: str) -> error",
            ),
            ("removeFile", "fn fs.removeFile(path: str) -> error"),
            (
                "removeDirectory",
                "fn fs.removeDirectory(path: str) -> error",
            ),
            (
                "writeText",
                "fn fs.writeText(path: str, text: str) -> error",
            ),
            (
                "appendText",
                "fn fs.appendText(path: str, text: str) -> error",
            ),
            (
                "rename",
                "fn fs.rename(source: str, destination: str) -> error",
            ),
            (
                "copyFile",
                "fn fs.copyFile(source: str, destination: str) -> error",
            ),
        ] {
            push_completion_item(items, seen, label, 3, detail);
        }
        return true;
    }
    if namespace == "android" {
        push_completion_item(items, seen, "sdkInt", 3, "fn android.sdkInt() -> i64");
        push_completion_item(
            items,
            seen,
            "vibrate",
            3,
            "fn android.vibrate(durationMs: i64) -> void",
        );
        push_completion_item(
            items,
            seen,
            "openUrl",
            3,
            "fn android.openUrl(url: str) -> void",
        );
        push_completion_item(
            items,
            seen,
            "share",
            3,
            "fn android.share(text: str) -> void",
        );
        push_completion_item(
            items,
            seen,
            "showKeyboard",
            3,
            "fn android.showKeyboard() -> void",
        );
        push_completion_item(
            items,
            seen,
            "hideKeyboard",
            3,
            "fn android.hideKeyboard() -> void",
        );
        push_completion_item(
            items,
            seen,
            "focusNext",
            3,
            "fn android.focusNext(wrap: bool = false) -> void",
        );
        push_completion_item(
            items,
            seen,
            "focusPrevious",
            3,
            "fn android.focusPrevious(wrap: bool = false) -> void",
        );
        push_completion_item(
            items,
            seen,
            "focusFirst",
            3,
            "fn android.focusFirst() -> void",
        );
        push_completion_item(
            items,
            seen,
            "focusLast",
            3,
            "fn android.focusLast() -> void",
        );
        push_completion_item(
            items,
            seen,
            "clearFocus",
            3,
            "fn android.clearFocus() -> void",
        );
        push_completion_item(
            items,
            seen,
            "selectionStart",
            3,
            "fn android.selectionStart() -> i64",
        );
        push_completion_item(
            items,
            seen,
            "selectionEnd",
            3,
            "fn android.selectionEnd() -> i64",
        );
        push_completion_item(
            items,
            seen,
            "setCaret",
            3,
            "fn android.setCaret(position: i64) -> bool",
        );
        push_completion_item(
            items,
            seen,
            "setSelection",
            3,
            "fn android.setSelection(start: i64, end: i64) -> bool",
        );
        push_completion_item(
            items,
            seen,
            "createNotificationChannel",
            3,
            "fn android.createNotificationChannel(id: str, name: str, description: str) -> void",
        );
        push_completion_item(
            items,
            seen,
            "permissionGranted",
            3,
            "fn android.permissionGranted(permission: str) -> bool",
        );
        push_completion_item(
            items,
            seen,
            "requestPermission",
            3,
            "fn android.requestPermission(permission: str) -> void",
        );
        push_completion_item(
            items,
            seen,
            "notificationPermissionGranted",
            3,
            "fn android.notificationPermissionGranted() -> bool",
        );
        push_completion_item(
            items,
            seen,
            "requestNotificationPermission",
            3,
            "fn android.requestNotificationPermission() -> void",
        );
        push_completion_item(
            items,
            seen,
            "notify",
            3,
            "fn android.notify(channelId: str, notificationId: i64, title: str, body: str) -> void",
        );
        push_completion_item(
            items,
            seen,
            "notifyUrlAction",
            3,
            "fn android.notifyUrlAction(channelId: str, notificationId: i64, title: str, body: str, actionLabel: str, url: str) -> void",
        );
        push_completion_item(
            items,
            seen,
            "cancelNotification",
            3,
            "fn android.cancelNotification(notificationId: i64) -> void",
        );
        return true;
    }
    if let Some(definition) = program
        .enums
        .iter()
        .find(|definition| definition.name == namespace)
    {
        for variant in &definition.variants {
            let payloads = variant
                .payloads
                .iter()
                .map(|payload| payload.ty.name())
                .collect::<Vec<_>>()
                .join(", ");
            push_completion_item(
                items,
                seen,
                &variant.name,
                20,
                &format!("{namespace}.{}({payloads}) -> {namespace}", variant.name),
            );
        }
        return true;
    }
    if let Some(interface) = program
        .interfaces
        .iter()
        .find(|interface| interface.name == namespace)
    {
        for function in &interface.functions {
            let params = function
                .params
                .iter()
                .map(|param| format!("{}: {}", param.name, param.ty.name()))
                .collect::<Vec<_>>()
                .join(", ");
            push_completion_item(
                items,
                seen,
                &function.name,
                3,
                &format!(
                    "fn {namespace}.{}(receiver: {namespace}{}) -> {}",
                    function.name,
                    if params.is_empty() {
                        String::new()
                    } else {
                        format!(", {params}")
                    },
                    format_return_types_for_lsp(&function.returns)
                ),
            );
        }
        return true;
    }
    false
}

fn visible_value_type_name(source: &str, line_index: usize, value_name: &str) -> Option<String> {
    let lines = source.lines().collect::<Vec<_>>();
    let cursor_line = lines.get(line_index)?;
    let cursor_indent = leading_spaces(cursor_line);
    for line in lines.iter().take(line_index + 1).rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = leading_spaces(line);
        if let Some(rest) = trimmed
            .strip_prefix("let ")
            .or_else(|| trimmed.strip_prefix("var "))
            && indent <= cursor_indent
            && let Some((name, after_name)) = rest.split_once(':')
            && name.trim() == value_name
        {
            let type_name = after_name
                .split(['=', ','])
                .next()
                .map(str::trim)
                .filter(|name| crate::ast::Type::parse(name).is_some())?;
            return Some(type_name.to_string());
        }
        if indent == 0 && (trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ")) {
            let open = trimmed.find('(')?;
            let close = trimmed[open + 1..].find(')')? + open + 1;
            for parameter in trimmed[open + 1..close].split(',') {
                let Some((name, ty)) = parameter.split_once(':') else {
                    continue;
                };
                if name.trim() == value_name {
                    let type_name = ty
                        .split('=')
                        .next()
                        .map(str::trim)
                        .filter(|name| crate::ast::Type::parse(name).is_some())?;
                    return Some(type_name.to_string());
                }
            }
            break;
        }
    }
    None
}

fn struct_field_for_position<'a>(
    source: &str,
    line_index: usize,
    character: usize,
    encoding: PositionEncoding,
    program: &'a crate::ast::Program,
) -> Option<&'a crate::ast::StructField> {
    let line = source.lines().nth(line_index)?;
    let byte = byte_offset_for_encoded_column(line, character, encoding);
    let field_name = identifier_at(line, byte)?;
    let receiver = member_receiver_at_cursor(source, line_index, character, encoding)?;
    let type_name = member_receiver_type_name(source, line_index, &receiver, program)?;
    struct_definition_for_type(&type_name, program)?
        .fields
        .iter()
        .find(|field| field.name == field_name)
}

fn list_property_for_position(
    source: &str,
    line_index: usize,
    character: usize,
    encoding: PositionEncoding,
    program: &crate::ast::Program,
) -> Option<(String, crate::ast::Type)> {
    let line = source.lines().nth(line_index)?;
    let byte = byte_offset_for_encoded_column(line, character, encoding);
    let property = identifier_at(line, byte)?;
    let receiver = member_receiver_at_cursor(source, line_index, character, encoding)?;
    let type_name = member_receiver_type_name(source, line_index, &receiver, program)?;
    let crate::ast::Type::List(element) = crate::ast::Type::parse(&type_name)? else {
        return None;
    };
    let ty = match property {
        "length" => crate::ast::Type::I64,
        "isEmpty" | "isNotEmpty" => crate::ast::Type::Bool,
        "first" | "last" | "single" => *element,
        _ => return None,
    };
    Some((property.to_string(), ty))
}

fn member_receiver_type_name(
    source: &str,
    line_index: usize,
    receiver: &str,
    program: &crate::ast::Program,
) -> Option<String> {
    if receiver.ends_with(')') {
        let open = receiver.find('(')?;
        let function_name = receiver[..open].trim();
        if !is_valid_identifier(function_name) {
            return None;
        }
        let function = program
            .functions
            .iter()
            .find(|function| function.name == function_name)?;
        let [return_type] = function.returns.as_slice() else {
            return None;
        };
        return named_type_name(return_type, program);
    }

    let mut parts = receiver.split('.');
    let root = parts.next()?;
    if !is_valid_identifier(root) {
        return None;
    }
    let source_type = crate::ast::Type::parse(&visible_value_type_name(source, line_index, root)?)?;
    let mut type_name = named_type_name(&source_type, program)?;
    for field_name in parts {
        if !is_valid_identifier(field_name) {
            return None;
        }
        let definition = struct_definition_for_type(&type_name, program)?;
        let field = definition
            .fields
            .iter()
            .find(|field| field.name == field_name)?;
        type_name = named_type_name(&field.ty, program)?;
    }
    Some(type_name)
}

fn named_type_name(ty: &crate::ast::Type, program: &crate::ast::Program) -> Option<String> {
    let mut current = ty.clone();
    let mut visited = HashSet::new();
    loop {
        let crate::ast::Type::Named(name) = current else {
            return Some(current.name());
        };
        if !visited.insert(name.clone()) {
            return None;
        }
        let Some(alias) = program.aliases.iter().find(|alias| alias.name == name) else {
            return Some(name);
        };
        current = alias.target.clone();
    }
}

fn struct_definition_for_type<'a>(
    type_name: &str,
    program: &'a crate::ast::Program,
) -> Option<&'a crate::ast::StructDef> {
    let concrete = named_type_name(&crate::ast::Type::Named(type_name.to_string()), program)?;
    program
        .structs
        .iter()
        .find(|definition| definition.name == concrete)
}

fn add_struct_field_completions(
    items: &mut Vec<JsonValue>,
    seen: &mut HashSet<String>,
    type_name: &str,
    program: &crate::ast::Program,
) {
    let Some(definition) = struct_definition_for_type(type_name, program) else {
        return;
    };
    for field in &definition.fields {
        push_completion_item(
            items,
            seen,
            &field.name,
            5,
            &format!(
                "field {}.{}: {}",
                definition.name,
                field.name,
                field.ty.name()
            ),
        );
    }
}

fn add_list_property_completions(
    items: &mut Vec<JsonValue>,
    seen: &mut HashSet<String>,
    type_name: &str,
) {
    let element_type = crate::ast::Type::parse(type_name)
        .and_then(|ty| match ty {
            crate::ast::Type::List(element) => Some(element.name()),
            _ => None,
        })
        .unwrap_or_else(|| "value".to_string());
    for (name, ty) in [
        ("length", "i64".to_string()),
        ("isEmpty", "bool".to_string()),
        ("isNotEmpty", "bool".to_string()),
        ("first", element_type.clone()),
        ("last", element_type.clone()),
        ("single", element_type),
    ] {
        push_completion_item(
            items,
            seen,
            name,
            10,
            &format!("property {type_name}.{name}: {ty}"),
        );
    }
}

fn format_return_types_for_lsp(returns: &[crate::ast::Type]) -> String {
    match returns {
        [] => "void".to_string(),
        [ty] => ty.name(),
        values => format!(
            "({})",
            values
                .iter()
                .map(crate::ast::Type::name)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn add_builtin_ui_context_completions(
    items: &mut Vec<JsonValue>,
    seen: &mut HashSet<String>,
    source: &str,
    line_index: usize,
) {
    let lines = source.lines().collect::<Vec<_>>();
    let Some(current) = lines.get(line_index) else {
        return;
    };
    let indent = leading_spaces(current);
    if inside_view_block(&lines, line_index) {
        for (name, ty) in crate::typecheck::VIEW_ENVIRONMENT_BINDINGS {
            push_completion_item(
                items,
                seen,
                name,
                6,
                &format!("read-only view environment {name}: {}", ty.name()),
            );
        }
    }
    if indent >= 8
        && let Some((kind, element_line)) = enclosing_view_element(&lines, line_index)
    {
        if let Some((property, _)) = current.trim().split_once(':')
            && matches!(
                property.trim(),
                "color"
                    | "backgroundColor"
                    | "borderColor"
                    | "borderTopColor"
                    | "borderBottomColor"
                    | "borderStartColor"
                    | "borderEndColor"
                    | "shadowColor"
            )
        {
            for token in crate::typecheck::SEMANTIC_UI_COLOR_TOKENS {
                push_completion_item(
                    items,
                    seen,
                    &format!("\"{token}\""),
                    12,
                    "semantic Flux UI color",
                );
            }
        }
        let existing = view_properties_before_cursor(&lines, element_line, line_index);
        for property in crate::typecheck::view_property_names(kind) {
            if existing.contains(&property) {
                continue;
            }
            let detail = crate::typecheck::view_property_type(kind, &property)
                .map(|ty| format!("{kind}.{property}: {}", ty.name()))
                .unwrap_or_else(|| format!("{kind}.{property}"));
            push_completion_item(items, seen, &property, 10, &detail);
        }
        return;
    }
    if indent == 4 && inside_view_block(&lines, line_index) {
        for kind in crate::typecheck::BUILTIN_VIEW_ELEMENT_KINDS {
            push_completion_item(items, seen, kind, 22, "built-in Flux view element");
        }
        for grid in ["grid columns:", "grid rows:", "grid gap:"] {
            push_completion_item(items, seen, grid, 14, "flat-grid layout declaration");
        }
        push_completion_item(items, seen, "state", 14, "mutable primitive view state");
        push_completion_item(items, seen, "derived", 14, "read-only derived view value");
    }
}

fn view_contract_program(source: &str, line_index: usize) -> Option<crate::ast::Program> {
    if let Ok(program) = crate::parser::parse_all(source) {
        return Some(program);
    }
    let mut byte_offset = 0usize;
    let mut current_view_start = None;
    for (index, segment) in source.split_inclusive('\n').enumerate() {
        if index > line_index {
            break;
        }
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        if leading_spaces(line) == 0 {
            let trimmed = line.trim();
            if trimmed.starts_with("view ") || trimmed.starts_with("pub view ") {
                current_view_start = Some(byte_offset);
            }
        }
        byte_offset += segment.len();
    }
    crate::parser::parse_all(source.get(..current_view_start?)?).ok()
}

fn recovered_view_contract(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
    kind: &str,
) -> Option<(crate::ast::ViewDef, Option<crate::project::ProjectSource>)> {
    let prefix_program = view_contract_program(source, line_index)?;
    if let Some(view) = prefix_program.views.iter().find(|view| view.name == kind) {
        return Some((view.clone(), None));
    }
    let current_path = std::fs::canonicalize(file_uri_path(uri)?).ok()?;
    let parent = current_path.parent()?;
    let package_root = nearest_package_manifest(&current_path)
        .and_then(|manifest| manifest.parent().map(std::path::Path::to_path_buf));
    let overlays = document_overlays(documents);
    for import in &prefix_program.imports {
        let import_path = std::path::Path::new(&import.path);
        if import_path.is_absolute()
            || import_path.extension().and_then(|value| value.to_str()) != Some("flux")
            || import_path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::CurDir | std::path::Component::ParentDir
                )
            })
        {
            continue;
        }
        let Ok(resolved) = std::fs::canonicalize(parent.join(import_path)) else {
            continue;
        };
        if package_root
            .as_ref()
            .is_some_and(|root| !resolved.starts_with(root))
        {
            continue;
        }
        let Ok((program, sources)) = crate::project::load_with_overlays(&resolved, &overlays)
        else {
            continue;
        };
        let Some(view) = program
            .views
            .iter()
            .find(|view| view.name == kind && view.public)
        else {
            continue;
        };
        let source = sources
            .iter()
            .find(|source| source.source_id == view.name_span.source_id)
            .cloned();
        return Some((view.clone(), source));
    }
    None
}

fn add_recovered_view_property_completions(
    items: &mut Vec<JsonValue>,
    seen: &mut HashSet<String>,
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
) {
    let lines = source.lines().collect::<Vec<_>>();
    let Some((kind, element_line)) = enclosing_view_element(&lines, line_index) else {
        return;
    };
    if !crate::typecheck::view_property_names(kind).is_empty() {
        return;
    }
    let Some((view, _)) = recovered_view_contract(uri, source, documents, line_index, kind) else {
        return;
    };
    let existing = view_properties_before_cursor(&lines, element_line, line_index);
    add_view_parameter_completions(items, seen, kind, &view, &existing);
}

fn add_view_parameter_completions(
    items: &mut Vec<JsonValue>,
    seen: &mut HashSet<String>,
    kind: &str,
    view: &crate::ast::ViewDef,
    existing: &HashSet<String>,
) {
    for param in &view.params {
        if existing.contains(param.name.as_str()) {
            continue;
        }
        push_completion_item(
            items,
            seen,
            &param.name,
            10,
            &format!("{kind}.{}: {}", param.name, param.ty.name()),
        );
    }
}

fn add_custom_view_property_completions(
    items: &mut Vec<JsonValue>,
    seen: &mut HashSet<String>,
    source: &str,
    line_index: usize,
    program: &crate::ast::Program,
) {
    let lines = source.lines().collect::<Vec<_>>();
    let Some((kind, element_line)) = enclosing_view_element(&lines, line_index) else {
        return;
    };
    if !crate::typecheck::view_property_names(kind).is_empty() {
        return;
    }
    let Some(view) = program.views.iter().find(|view| view.name == kind) else {
        return;
    };
    let existing = view_properties_before_cursor(&lines, element_line, line_index);
    add_view_parameter_completions(items, seen, kind, view, &existing);
}

fn enclosing_view_element<'a>(lines: &'a [&str], line_index: usize) -> Option<(&'a str, usize)> {
    for index in (0..line_index).rev() {
        let line = lines.get(index)?;
        if line.trim().is_empty() {
            continue;
        }
        let indent = leading_spaces(line);
        if indent == 0 {
            return None;
        }
        if indent != 4 {
            continue;
        }
        let tokens = line.split_whitespace().collect::<Vec<_>>();
        if tokens.len() >= 4 && tokens[2] == "at" {
            return Some((tokens[0], index));
        }
        return None;
    }
    None
}

fn view_properties_before_cursor(
    lines: &[&str],
    element_line: usize,
    line_index: usize,
) -> HashSet<String> {
    lines
        .iter()
        .take(line_index)
        .skip(element_line + 1)
        .filter_map(|line| {
            let trimmed = line.trim();
            (leading_spaces(line) >= 8)
                .then(|| {
                    trimmed
                        .split_once(':')
                        .map(|(name, _)| name.trim().to_string())
                })
                .flatten()
        })
        .collect()
}

fn inside_view_block(lines: &[&str], line_index: usize) -> bool {
    for line in lines.iter().take(line_index).rev() {
        if line.trim().is_empty() || leading_spaces(line) > 0 {
            continue;
        }
        let trimmed = line.trim();
        return trimmed.starts_with("view ") && trimmed.ends_with('{');
    }
    false
}

fn add_position_local_completions(
    items: &mut Vec<JsonValue>,
    seen: &mut HashSet<String>,
    source: &str,
    source_id: SourceId,
    line: usize,
    program: &crate::ast::Program,
    database: &crate::semantic::SemanticDatabase,
) {
    let Some(function) = program.functions.iter().find(|function| {
        function.name_span.source_id == source_id
            && function.line <= line
            && function_contains_line(source, function.line, line)
    }) else {
        return;
    };
    use crate::semantic::SymbolKind;
    for symbol in database.symbols().iter().filter(|symbol| {
        symbol.span.source_id == source_id
            && symbol.span.line >= function.line
            && symbol.span.line <= line
            && matches!(
                symbol.kind,
                SymbolKind::Parameter
                    | SymbolKind::Binding
                    | SymbolKind::MutableBinding
                    | SymbolKind::PatternBinding
                    | SymbolKind::LoopVariable
            )
            && local_symbol_visible_at_line(source, symbol, line)
    }) {
        let prefix = match symbol.kind {
            SymbolKind::Parameter => "parameter",
            SymbolKind::MutableBinding => "var",
            SymbolKind::Binding => "let",
            SymbolKind::PatternBinding => "pattern",
            SymbolKind::LoopVariable => "loop",
            _ => unreachable!("filtered local completion symbol kind"),
        };
        let detail = symbol
            .ty
            .as_ref()
            .map(|ty| format!("{prefix} {}: {}", symbol.name, ty.name()))
            .unwrap_or_else(|| format!("{prefix} {}", symbol.name));
        push_completion_item(items, seen, &symbol.name, 6, &detail);
    }
}

fn local_symbol_visible_at_line(
    source: &str,
    symbol: &crate::semantic::SemanticSymbol,
    cursor_line: usize,
) -> bool {
    use crate::semantic::SymbolKind;
    if symbol.kind == SymbolKind::Parameter {
        return true;
    }
    if symbol.span.line > cursor_line {
        return false;
    }
    let lines = source.lines().collect::<Vec<_>>();
    let Some(declaration_line) = lines.get(symbol.span.line.saturating_sub(1)) else {
        return false;
    };
    let Some(cursor_source) = lines.get(cursor_line.saturating_sub(1)) else {
        return false;
    };
    let declaration_indent = leading_spaces(declaration_line);
    let cursor_indent = leading_spaces(cursor_source);
    let scoped_header_binding = symbol.kind == SymbolKind::LoopVariable
        || (symbol.kind == SymbolKind::PatternBinding
            && declaration_line.trim_end().ends_with(':'));
    if scoped_header_binding {
        if cursor_line == symbol.span.line || cursor_indent <= declaration_indent {
            return false;
        }
        let start = symbol.span.line;
        let end = cursor_line.saturating_sub(1);
        if start >= end {
            return true;
        }
        return !lines[start..end]
            .iter()
            .filter(|line| !line.trim().is_empty())
            .any(|line| leading_spaces(line) <= declaration_indent);
    }
    if declaration_indent > cursor_indent {
        return false;
    }
    let start = symbol.span.line;
    let end = cursor_line.saturating_sub(1);
    if start >= end {
        return true;
    }
    !lines[start..end]
        .iter()
        .filter(|line| !line.trim().is_empty())
        .any(|line| leading_spaces(line) < declaration_indent)
}

fn leading_spaces(line: &str) -> usize {
    line.bytes().take_while(|byte| *byte == b' ').count()
}

fn function_contains_line(source: &str, start_line: usize, line: usize) -> bool {
    if line < start_line {
        return false;
    }
    let Some(header) = source.lines().nth(start_line.saturating_sub(1)) else {
        return false;
    };
    if header.trim_end().ends_with('}') {
        return line == start_line;
    }
    source
        .lines()
        .enumerate()
        .skip(start_line)
        .find(|(_, candidate)| {
            candidate.len() == candidate.trim_start().len() && candidate.trim() == "}"
        })
        .is_some_and(|(end_index, _)| line <= end_index + 1)
}

#[cfg(test)]
fn signature_help_for_document(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
    character: usize,
    encoding: PositionEncoding,
) -> Option<JsonValue> {
    signature_help_for_document_cached(
        uri, source, documents, line_index, character, encoding, None,
    )
}

fn signature_help_for_document_cached(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
    character: usize,
    encoding: PositionEncoding,
    cache: Option<&mut crate::project::ProjectAnalysisCache>,
) -> Option<JsonValue> {
    let line = source.lines().nth(line_index).unwrap_or("");
    let byte_in_line = byte_offset_for_encoded_column(line, character, encoding);
    let absolute = source
        .lines()
        .take(line_index)
        .map(|line| line.len() + 1)
        .sum::<usize>()
        + byte_in_line;
    let prefix = source.get(..absolute.min(source.len()))?;
    let (call_name, active_parameter) = active_call(prefix)?;
    let project_analysis = analyzed_project_document_cached(uri, documents, cache);
    let standalone_database;
    let database = if let Some((database, _)) = project_analysis.as_ref() {
        database
    } else {
        standalone_database = analyzed_document(uri, source)?;
        &standalone_database
    };
    if call_name == "print" {
        return Some(signature_help_for_builtin(
            "print",
            &["value: i64 | bool | str | error"],
            "void",
            active_parameter,
        ));
    }
    if call_name == "error" {
        return Some(signature_help_for_builtin(
            "error",
            &["message: str"],
            "error",
            active_parameter,
        ));
    }
    if call_name == "take" || call_name == "skip" {
        return Some(signature_help_for_builtin(
            call_name,
            &["list: T[]", "count: i64"],
            "T[]",
            active_parameter,
        ));
    }
    if call_name == "any" || call_name == "every" {
        return Some(signature_help_for_builtin(
            call_name,
            &["list: bool[]"],
            "bool",
            active_parameter,
        ));
    }
    if call_name == "fold" {
        return Some(signature_help_for_builtin(
            "fold",
            &["list: T[]", "initial: A", "reducer: fn(A, T) -> A"],
            "A",
            active_parameter,
        ));
    }
    if call_name == "reduce" {
        return Some(signature_help_for_builtin(
            "reduce",
            &["list: T[]", "reducer: fn(T, T) -> T"],
            "T",
            active_parameter,
        ));
    }
    if call_name == "map" {
        return Some(signature_help_for_builtin(
            "map",
            &["list: T[]", "callback: fn(T) -> U"],
            "U[]",
            active_parameter,
        ));
    }
    if call_name == "filter" || call_name == "where" {
        return Some(signature_help_for_builtin(
            call_name,
            &["list: T[]", "predicate: fn(T) -> bool"],
            "T[]",
            active_parameter,
        ));
    }
    if call_name == "concat" {
        return Some(signature_help_for_builtin(
            "concat",
            &["left: T[]", "right: T[]"],
            "T[]",
            active_parameter,
        ));
    }
    if call_name == "distinct" {
        return Some(signature_help_for_builtin(
            "distinct",
            &["list: scalar[]"],
            "same scalar list type",
            active_parameter,
        ));
    }
    if call_name == "flatten" {
        return Some(signature_help_for_builtin(
            "flatten",
            &["list: T[][]"],
            "T[]",
            active_parameter,
        ));
    }
    if call_name == "sorted" {
        return Some(signature_help_for_builtin(
            "sorted",
            &["list: ordered[]"],
            "same ordered list type",
            active_parameter,
        ));
    }
    if call_name == "chunked" {
        return Some(signature_help_for_builtin(
            "chunked",
            &["list: T[]", "size: i64"],
            "T[][]",
            active_parameter,
        ));
    }
    if let Some((namespace, member)) = call_name.split_once('.') {
        if namespace == "process" {
            match member {
                "pid" | "parentPid" => {
                    return Some(signature_help_for_builtin(
                        &format!("process.{member}"),
                        &[],
                        "i64",
                        active_parameter,
                    ));
                }
                "terminationRequested" => {
                    return Some(signature_help_for_builtin(
                        "process.terminationRequested",
                        &[],
                        "bool",
                        active_parameter,
                    ));
                }
                "exit" => {
                    return Some(signature_help_for_builtin(
                        "process.exit",
                        &["code: i64"],
                        "void",
                        active_parameter,
                    ));
                }
                "hasEnv" => {
                    return Some(signature_help_for_builtin(
                        "process.hasEnv",
                        &["name: str"],
                        "bool",
                        active_parameter,
                    ));
                }
                "env" => {
                    return Some(signature_help_for_builtin(
                        "process.env",
                        &["name: str", "fallback: str"],
                        "str",
                        active_parameter,
                    ));
                }
                _ => {}
            }
        }
        if namespace == "locale" {
            match member {
                "language" | "region" => {
                    return Some(signature_help_for_builtin(
                        &format!("locale.{member}"),
                        &[],
                        "str",
                        active_parameter,
                    ));
                }
                _ => {}
            }
        }
        if namespace == "time" {
            match member {
                "unixMillis" | "monotonicMillis" => {
                    return Some(signature_help_for_builtin(
                        &format!("time.{member}"),
                        &[],
                        "i64",
                        active_parameter,
                    ));
                }
                "sleepMillis" => {
                    return Some(signature_help_for_builtin(
                        "time.sleepMillis",
                        &["durationMs: i64"],
                        "void",
                        active_parameter,
                    ));
                }
                _ => {}
            }
        }
        if namespace == "fs" {
            match member {
                "exists" | "isFile" | "isDirectory" => {
                    return Some(signature_help_for_builtin(
                        &format!("fs.{member}"),
                        &["path: str"],
                        "bool",
                        active_parameter,
                    ));
                }
                "createDirectory" | "removeFile" | "removeDirectory" => {
                    return Some(signature_help_for_builtin(
                        &format!("fs.{member}"),
                        &["path: str"],
                        "error",
                        active_parameter,
                    ));
                }
                "writeText" | "appendText" => {
                    return Some(signature_help_for_builtin(
                        &format!("fs.{member}"),
                        &["path: str", "text: str"],
                        "error",
                        active_parameter,
                    ));
                }
                "rename" | "copyFile" => {
                    return Some(signature_help_for_builtin(
                        &format!("fs.{member}"),
                        &["source: str", "destination: str"],
                        "error",
                        active_parameter,
                    ));
                }
                _ => {}
            }
        }
        if namespace == "android" {
            match member {
                "sdkInt" => {
                    return Some(signature_help_for_builtin(
                        "android.sdkInt",
                        &[],
                        "i64",
                        active_parameter,
                    ));
                }
                "vibrate" => {
                    return Some(signature_help_for_builtin(
                        "android.vibrate",
                        &["durationMs: i64"],
                        "void",
                        active_parameter,
                    ));
                }
                "openUrl" => {
                    return Some(signature_help_for_builtin(
                        "android.openUrl",
                        &["url: str"],
                        "void",
                        active_parameter,
                    ));
                }
                "share" => {
                    return Some(signature_help_for_builtin(
                        "android.share",
                        &["text: str"],
                        "void",
                        active_parameter,
                    ));
                }
                "focusNext" | "focusPrevious" => {
                    return Some(signature_help_for_builtin(
                        &format!("android.{member}"),
                        &["wrap: bool = false"],
                        "void",
                        active_parameter,
                    ));
                }
                "showKeyboard" | "hideKeyboard" | "focusFirst" | "focusLast" | "clearFocus" => {
                    return Some(signature_help_for_builtin(
                        &format!("android.{member}"),
                        &[],
                        "void",
                        active_parameter,
                    ));
                }
                "selectionStart" | "selectionEnd" => {
                    return Some(signature_help_for_builtin(
                        &format!("android.{member}"),
                        &[],
                        "i64",
                        active_parameter,
                    ));
                }
                "setCaret" => {
                    return Some(signature_help_for_builtin(
                        "android.setCaret",
                        &["position: i64"],
                        "bool",
                        active_parameter,
                    ));
                }
                "setSelection" => {
                    return Some(signature_help_for_builtin(
                        "android.setSelection",
                        &["start: i64", "end: i64"],
                        "bool",
                        active_parameter,
                    ));
                }
                "createNotificationChannel" => {
                    return Some(signature_help_for_builtin(
                        "android.createNotificationChannel",
                        &["id: str", "name: str", "description: str"],
                        "void",
                        active_parameter,
                    ));
                }
                "permissionGranted" => {
                    return Some(signature_help_for_builtin(
                        "android.permissionGranted",
                        &["permission: str"],
                        "bool",
                        active_parameter,
                    ));
                }
                "requestPermission" => {
                    return Some(signature_help_for_builtin(
                        "android.requestPermission",
                        &["permission: str"],
                        "void",
                        active_parameter,
                    ));
                }
                "notificationPermissionGranted" => {
                    return Some(signature_help_for_builtin(
                        "android.notificationPermissionGranted",
                        &[],
                        "bool",
                        active_parameter,
                    ));
                }
                "requestNotificationPermission" => {
                    return Some(signature_help_for_builtin(
                        "android.requestNotificationPermission",
                        &[],
                        "void",
                        active_parameter,
                    ));
                }
                "notify" => {
                    return Some(signature_help_for_builtin(
                        "android.notify",
                        &[
                            "channelId: str",
                            "notificationId: i64",
                            "title: str",
                            "body: str",
                        ],
                        "void",
                        active_parameter,
                    ));
                }
                "notifyUrlAction" => {
                    return Some(signature_help_for_builtin(
                        "android.notifyUrlAction",
                        &[
                            "channelId: str",
                            "notificationId: i64",
                            "title: str",
                            "body: str",
                            "actionLabel: str",
                            "url: str",
                        ],
                        "void",
                        active_parameter,
                    ));
                }
                "cancelNotification" => {
                    return Some(signature_help_for_builtin(
                        "android.cancelNotification",
                        &["notificationId: i64"],
                        "void",
                        active_parameter,
                    ));
                }
                _ => {}
            }
        }
        if let Some(definition) = database.signatures().enum_type(namespace)
            && let Some(variant) = definition.variant(member)
        {
            return Some(signature_help_for_enum_variant(
                namespace,
                member,
                &variant.payloads,
                active_parameter,
            ));
        }
        let interface = database.signatures().interface(namespace)?;
        let signature = interface.functions.get(member)?;
        return Some(signature_help_for_interface_capability(
            namespace,
            member,
            signature,
            active_parameter,
        ));
    }
    if database.signatures().interface(call_name).is_some() {
        return Some(signature_help_for_interface_pack(
            call_name,
            active_parameter,
        ));
    }
    if let Some(ty) = visible_local_callable_type(database, uri, source, line_index + 1, call_name)
    {
        return Some(signature_help_for_function_value(
            call_name,
            &ty,
            active_parameter,
        ));
    }
    let function = database
        .program()
        .functions
        .iter()
        .find(|function| function.name == call_name)?;
    let label = format_ast_function_signature(function);
    let parameters = function
        .params
        .iter()
        .map(|param| {
            object([(
                "label",
                JsonValue::String(format!("{}: {}", param.name, param.ty.name())),
            )])
        })
        .collect::<Vec<_>>();
    Some(object([
        (
            "signatures",
            JsonValue::Array(vec![object([
                ("label", JsonValue::String(label)),
                ("parameters", JsonValue::Array(parameters)),
            ])]),
        ),
        ("activeSignature", JsonValue::Number(0)),
        (
            "activeParameter",
            JsonValue::Number(active_parameter.min(function.params.len().saturating_sub(1)) as i64),
        ),
    ]))
}

fn signature_help_for_builtin(
    name: &str,
    labels: &[&str],
    returns: &str,
    active_parameter: usize,
) -> JsonValue {
    signature_help_from_labels(name, labels, returns, active_parameter)
}

fn signature_help_for_enum_variant(
    enum_name: &str,
    variant_name: &str,
    payloads: &[crate::ast::Type],
    active_parameter: usize,
) -> JsonValue {
    let labels = payloads
        .iter()
        .map(crate::ast::Type::name)
        .collect::<Vec<_>>();
    signature_help_from_owned_labels(
        &format!("{enum_name}.{variant_name}"),
        labels,
        enum_name,
        active_parameter,
    )
}

fn signature_help_for_interface_pack(interface_name: &str, active_parameter: usize) -> JsonValue {
    signature_help_from_labels(
        interface_name,
        &["value: implementing concrete value"],
        interface_name,
        active_parameter,
    )
}

fn signature_help_from_labels(
    name: &str,
    labels: &[&str],
    returns: &str,
    active_parameter: usize,
) -> JsonValue {
    signature_help_from_owned_labels(
        name,
        labels.iter().map(|label| (*label).to_string()).collect(),
        returns,
        active_parameter,
    )
}

fn signature_help_from_owned_labels(
    name: &str,
    labels: Vec<String>,
    returns: &str,
    active_parameter: usize,
) -> JsonValue {
    let label = format!("fn {name}({}) -> {returns}", labels.join(", "));
    let parameter_count = labels.len();
    let parameters = labels
        .into_iter()
        .map(|label| object([("label", JsonValue::String(label))]))
        .collect::<Vec<_>>();
    object([
        (
            "signatures",
            JsonValue::Array(vec![object([
                ("label", JsonValue::String(label)),
                ("parameters", JsonValue::Array(parameters)),
            ])]),
        ),
        ("activeSignature", JsonValue::Number(0)),
        (
            "activeParameter",
            JsonValue::Number(active_parameter.min(parameter_count.saturating_sub(1)) as i64),
        ),
    ])
}

fn visible_local_callable_type(
    database: &crate::semantic::SemanticDatabase,
    uri: &str,
    source: &str,
    line: usize,
    name: &str,
) -> Option<crate::ast::Type> {
    use crate::semantic::SymbolKind;
    let source_id = source_id_for_uri(uri);
    let symbol = database
        .symbols_named(name)
        .filter(|symbol| {
            symbol.span.source_id == source_id
                && symbol.span.line <= line
                && matches!(
                    symbol.kind,
                    SymbolKind::Parameter | SymbolKind::Binding | SymbolKind::MutableBinding
                )
                && local_symbol_visible_at_line(source, symbol, line)
        })
        .max_by_key(|symbol| symbol.span.line)?;
    let ty = database.signatures().canonical_type(symbol.ty.as_ref()?);
    matches!(ty, crate::ast::Type::Function { .. }).then_some(ty)
}

fn signature_help_for_function_value(
    name: &str,
    ty: &crate::ast::Type,
    active_parameter: usize,
) -> JsonValue {
    let crate::ast::Type::Function { params, returns } = ty else {
        unreachable!("function-value signature help requires a function type")
    };
    let param_labels = params
        .iter()
        .map(crate::ast::Type::name)
        .collect::<Vec<_>>();
    let returns = match returns.as_slice() {
        [] => "void".to_string(),
        [ty] => ty.name(),
        values => format!(
            "({})",
            values
                .iter()
                .map(crate::ast::Type::name)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let label = format!("fn {name}({}) -> {returns}", param_labels.join(", "));
    let parameters = param_labels
        .into_iter()
        .map(|label| object([("label", JsonValue::String(label))]))
        .collect::<Vec<_>>();
    object([
        (
            "signatures",
            JsonValue::Array(vec![object([
                ("label", JsonValue::String(label)),
                ("parameters", JsonValue::Array(parameters)),
            ])]),
        ),
        ("activeSignature", JsonValue::Number(0)),
        (
            "activeParameter",
            JsonValue::Number(active_parameter.min(params.len().saturating_sub(1)) as i64),
        ),
    ])
}

fn signature_help_for_interface_capability(
    interface_name: &str,
    member_name: &str,
    signature: &crate::typecheck::Signature,
    active_parameter: usize,
) -> JsonValue {
    let mut labels = vec![format!("receiver: {interface_name}")];
    labels.extend(
        signature
            .param_details
            .iter()
            .map(|param| format!("{}: {}", param.name, param.ty.name())),
    );
    let returns = match signature.returns.as_slice() {
        [] => "void".to_string(),
        [ty] => ty.name(),
        values => format!(
            "({})",
            values
                .iter()
                .map(crate::ast::Type::name)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let label = format!(
        "fn {interface_name}.{member_name}({}) -> {returns}",
        labels.join(", ")
    );
    let parameters = labels
        .into_iter()
        .map(|label| object([("label", JsonValue::String(label))]))
        .collect::<Vec<_>>();
    object([
        (
            "signatures",
            JsonValue::Array(vec![object([
                ("label", JsonValue::String(label)),
                ("parameters", JsonValue::Array(parameters)),
            ])]),
        ),
        ("activeSignature", JsonValue::Number(0)),
        (
            "activeParameter",
            JsonValue::Number(active_parameter.min(signature.param_details.len()) as i64),
        ),
    ])
}

fn active_call(prefix: &str) -> Option<(&str, usize)> {
    let bytes = prefix.as_bytes();
    let mut stack = Vec::<usize>::new();
    let mut index = 0usize;
    let mut in_string = false;
    let mut raw_string = false;
    let mut multiline_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if multiline_string {
                if bytes[index..].starts_with(b"\"\"\"") {
                    in_string = false;
                    multiline_string = false;
                    index += 3;
                    continue;
                }
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                }
            } else if raw_string {
                if byte == b'"' {
                    in_string = false;
                    raw_string = false;
                }
            } else if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"\"\"\"") {
            in_string = true;
            multiline_string = true;
            index += 3;
            continue;
        }
        if byte == b'r' && bytes.get(index + 1) == Some(&b'"') {
            in_string = true;
            raw_string = true;
            index += 2;
            continue;
        }
        match byte {
            b'"' => {
                in_string = true;
                raw_string = false;
            }
            b'(' => stack.push(index),
            b')' => {
                stack.pop();
            }
            _ => {}
        }
        index += 1;
    }
    let open = *stack.last()?;
    let mut name_end = open;
    while name_end > 0 && bytes[name_end - 1].is_ascii_whitespace() {
        name_end -= 1;
    }
    let mut name_start = name_end;
    while name_start > 0
        && (is_identifier_byte(bytes[name_start - 1]) || bytes[name_start - 1] == b'.')
    {
        name_start -= 1;
    }
    if name_start == name_end {
        return None;
    }
    let name = std::str::from_utf8(&bytes[name_start..name_end]).ok()?;
    let mut depth = 0usize;
    let mut commas = 0usize;
    let mut index = open + 1;
    let mut in_string = false;
    let mut raw_string = false;
    let mut multiline_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if multiline_string {
                if bytes[index..].starts_with(b"\"\"\"") {
                    in_string = false;
                    multiline_string = false;
                    index += 3;
                    continue;
                }
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                }
            } else if raw_string {
                if byte == b'"' {
                    in_string = false;
                    raw_string = false;
                }
            } else if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"\"\"\"") {
            in_string = true;
            multiline_string = true;
            index += 3;
            continue;
        }
        if byte == b'r' && bytes.get(index + 1) == Some(&b'"') {
            in_string = true;
            raw_string = true;
            index += 2;
            continue;
        }
        match byte {
            b'"' => {
                in_string = true;
                raw_string = false;
            }
            b'(' => depth += 1,
            b')' if depth > 0 => depth -= 1,
            b',' if depth == 0 => commas += 1,
            _ => {}
        }
        index += 1;
    }
    Some((name, commas))
}

fn push_completion_item(
    items: &mut Vec<JsonValue>,
    seen: &mut HashSet<String>,
    label: &str,
    kind: i64,
    detail: &str,
) {
    if !seen.insert(label.to_string()) {
        return;
    }
    items.push(object([
        ("label", JsonValue::String(label.to_string())),
        ("kind", JsonValue::Number(kind)),
        ("detail", JsonValue::String(detail.to_string())),
    ]));
}

fn format_ast_function_signature(function: &crate::ast::Function) -> String {
    let mut params = Vec::new();
    let mut emitted_named_marker = false;
    for param in &function.params {
        if param.named_only && !emitted_named_marker {
            params.push("*".to_string());
            emitted_named_marker = true;
        }
        let default = if param.default.is_some() {
            " = …"
        } else {
            ""
        };
        params.push(format!("{}: {}{default}", param.name, param.ty.name()));
    }
    let returns = match function.returns.as_slice() {
        [] => "void".to_string(),
        [ty] => ty.name(),
        values => format!(
            "({})",
            values
                .iter()
                .map(|ty| ty.name())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    format!("fn {}({}) -> {returns}", function.name, params.join(", "))
}

const SEMANTIC_TOKEN_TYPES: &[&str] = &[
    "keyword",
    "string",
    "number",
    "type",
    "enum",
    "interface",
    "struct",
    "function",
    "parameter",
    "variable",
    "property",
    "enumMember",
    "operator",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SemanticTokenKind {
    Keyword = 0,
    String = 1,
    Number = 2,
    Type = 3,
    Enum = 4,
    Interface = 5,
    Struct = 6,
    Function = 7,
    Parameter = 8,
    Variable = 9,
    Property = 10,
    EnumMember = 11,
    Operator = 12,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SemanticToken {
    line: usize,
    start: usize,
    length: usize,
    kind: SemanticTokenKind,
}

fn inlay_hints_for_document(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    start_line: usize,
    end_line: usize,
    encoding: PositionEncoding,
) -> Vec<JsonValue> {
    let project_database = analyzed_project_document(uri, documents).map(|(database, _)| database);
    let standalone_database;
    let database = if let Some(database) = project_database.as_ref() {
        database
    } else {
        let Some(database) = analyzed_document(uri, source) else {
            return Vec::new();
        };
        standalone_database = database;
        &standalone_database
    };
    let source_id = source_id_for_uri(uri);
    use crate::semantic::SymbolKind;
    database
        .symbols()
        .iter()
        .filter(|symbol| {
            symbol.span.source_id == source_id
                && symbol.span.line.saturating_sub(1) >= start_line
                && symbol.span.line.saturating_sub(1) <= end_line
                && matches!(
                    symbol.kind,
                    SymbolKind::PatternBinding | SymbolKind::LoopVariable
                )
                && symbol.ty.is_some()
        })
        .filter_map(|symbol| {
            let range = lsp_range(symbol.span, source, encoding);
            let position = range.get("end")?.clone();
            Some(object([
                ("position", position),
                (
                    "label",
                    JsonValue::String(format!(": {}", symbol.ty.as_ref()?.name())),
                ),
                ("kind", JsonValue::Number(1)),
                ("paddingLeft", JsonValue::Bool(false)),
                ("paddingRight", JsonValue::Bool(false)),
            ]))
        })
        .collect()
}

fn semantic_tokens(uri: &str, source: &str, encoding: PositionEncoding) -> Vec<JsonValue> {
    let source_id = SourceId::from_name(uri);
    let database = crate::parser::parse_all_with_source(source, source_id)
        .ok()
        .filter(|program| program.imports.is_empty())
        .and_then(|program| {
            crate::typecheck::check_all(&program)
                .ok()
                .map(|signatures| {
                    crate::semantic::SemanticDatabase::from_analyzed(program, signatures)
                })
        });
    let mut tokens = Vec::new();
    let mut in_multiline_string = false;
    for (line_index, line) in source.lines().enumerate() {
        tokenize_semantic_line(
            &mut tokens,
            line,
            line_index,
            source_id,
            database.as_ref(),
            encoding,
            &mut in_multiline_string,
        );
    }
    encode_semantic_tokens(&tokens)
}

fn tokenize_semantic_line(
    tokens: &mut Vec<SemanticToken>,
    line: &str,
    line_index: usize,
    source_id: SourceId,
    database: Option<&crate::semantic::SemanticDatabase>,
    encoding: PositionEncoding,
    in_multiline_string: &mut bool,
) {
    let bytes = line.as_bytes();
    let mut index = 0usize;

    if *in_multiline_string {
        if let Some(end) = multiline_string_end(bytes, 0) {
            push_semantic_token(
                tokens,
                line,
                line_index,
                0,
                end,
                SemanticTokenKind::String,
                encoding,
            );
            *in_multiline_string = false;
            index = end;
        } else {
            push_semantic_token(
                tokens,
                line,
                line_index,
                0,
                bytes.len(),
                SemanticTokenKind::String,
                encoding,
            );
            return;
        }
    }
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"\"\"\"") {
            let start = index;
            if let Some(end) = multiline_string_end(bytes, index + 3) {
                index = end;
            } else {
                index = bytes.len();
                *in_multiline_string = true;
            }
            push_semantic_token(
                tokens,
                line,
                line_index,
                start,
                index,
                SemanticTokenKind::String,
                encoding,
            );
            continue;
        }
        if byte == b'r' && bytes.get(index + 1) == Some(&b'"') {
            let start = index;
            index += 2;
            while index < bytes.len() {
                let current = bytes[index];
                index += 1;
                if current == b'"' {
                    break;
                }
            }
            push_semantic_token(
                tokens,
                line,
                line_index,
                start,
                index,
                SemanticTokenKind::String,
                encoding,
            );
            continue;
        }
        if byte == b'"' {
            let start = index;
            index += 1;
            let mut escaped = false;
            while index < bytes.len() {
                let current = bytes[index];
                index += 1;
                if escaped {
                    escaped = false;
                } else if current == b'\\' {
                    escaped = true;
                } else if current == b'"' {
                    break;
                }
            }
            push_semantic_token(
                tokens,
                line,
                line_index,
                start,
                index,
                SemanticTokenKind::String,
                encoding,
            );
            continue;
        }
        if byte.is_ascii_digit() {
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            push_semantic_token(
                tokens,
                line,
                line_index,
                start,
                index,
                SemanticTokenKind::Number,
                encoding,
            );
            continue;
        }
        if byte == b'_' || byte.is_ascii_alphabetic() {
            let start = index;
            index += 1;
            while index < bytes.len() && is_identifier_byte(bytes[index]) {
                index += 1;
            }
            let word = &line[start..index];
            let kind =
                semantic_identifier_kind(word, source_id, line_index + 1, start + 1, database);
            push_semantic_token(tokens, line, line_index, start, index, kind, encoding);
            continue;
        }
        if is_operator_byte(byte) {
            let start = index;
            index += 1;
            while index < bytes.len() && is_operator_byte(bytes[index]) {
                index += 1;
            }
            push_semantic_token(
                tokens,
                line,
                line_index,
                start,
                index,
                SemanticTokenKind::Operator,
                encoding,
            );
            continue;
        }
        index += 1;
    }
}

fn semantic_identifier_kind(
    word: &str,
    source_id: SourceId,
    line: usize,
    column: usize,
    database: Option<&crate::semantic::SemanticDatabase>,
) -> SemanticTokenKind {
    if is_flux_keyword(word) {
        return SemanticTokenKind::Keyword;
    }
    if matches!(word, "i64" | "bool" | "str" | "error" | "void") {
        return SemanticTokenKind::Type;
    }
    if word == "print" {
        return SemanticTokenKind::Function;
    }
    let Some(database) = database else {
        return SemanticTokenKind::Variable;
    };
    let symbol = database.symbol_at(source_id, line, column).or_else(|| {
        let mut matches = database.symbols_named(word);
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    });
    symbol
        .map(|symbol| semantic_symbol_kind(symbol.kind))
        .unwrap_or(SemanticTokenKind::Variable)
}

fn semantic_symbol_kind(kind: crate::semantic::SymbolKind) -> SemanticTokenKind {
    use crate::semantic::SymbolKind;
    match kind {
        SymbolKind::TypeAlias | SymbolKind::View => SemanticTokenKind::Type,
        SymbolKind::Interface | SymbolKind::InterfaceImplementation => SemanticTokenKind::Interface,
        SymbolKind::InterfaceFunction | SymbolKind::InterfaceImplementationMapping => {
            SemanticTokenKind::Function
        }
        SymbolKind::Constant
        | SymbolKind::Binding
        | SymbolKind::MutableBinding
        | SymbolKind::PatternBinding
        | SymbolKind::LoopVariable
        | SymbolKind::ViewState
        | SymbolKind::ViewDerived
        | SymbolKind::ViewElement => SemanticTokenKind::Variable,
        SymbolKind::Enum => SemanticTokenKind::Enum,
        SymbolKind::EnumVariant => SemanticTokenKind::EnumMember,
        SymbolKind::Struct => SemanticTokenKind::Struct,
        SymbolKind::StructField | SymbolKind::ViewProperty => SemanticTokenKind::Property,
        SymbolKind::Function => SemanticTokenKind::Function,
        SymbolKind::Parameter => SemanticTokenKind::Parameter,
    }
}

fn is_flux_keyword(word: &str) -> bool {
    matches!(
        word,
        "fn" | "let"
            | "var"
            | "return"
            | "if"
            | "elif"
            | "else"
            | "for"
            | "while"
            | "in"
            | "break"
            | "continue"
            | "match"
            | "struct"
            | "enum"
            | "interface"
            | "impl"
            | "type"
            | "const"
            | "pub"
            | "import"
            | "view"
            | "app"
            | "state"
            | "derived"
            | "grid"
            | "at"
            | "span"
            | "rows"
            | "columns"
            | "true"
            | "false"
            | "nil"
            | "auto"
    )
}

fn is_operator_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'+' | b'-' | b'*' | b'/' | b'%' | b'=' | b'<' | b'>' | b'!' | b'&' | b'|' | b'.'
    )
}

fn multiline_string_end(bytes: &[u8], mut index: usize) -> Option<usize> {
    let mut escaped = false;
    while index < bytes.len() {
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if bytes[index] == b'\\' {
            escaped = true;
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"\"\"\"") {
            return Some(index + 3);
        }
        index += 1;
    }
    None
}

fn push_semantic_token(
    tokens: &mut Vec<SemanticToken>,
    line: &str,
    line_index: usize,
    start_byte: usize,
    end_byte: usize,
    kind: SemanticTokenKind,
    encoding: PositionEncoding,
) {
    let start = encoded_column(line, start_byte, encoding);
    let end = encoded_column(line, end_byte, encoding);
    if end <= start {
        return;
    }
    tokens.push(SemanticToken {
        line: line_index,
        start,
        length: end - start,
        kind,
    });
}

fn encode_semantic_tokens(tokens: &[SemanticToken]) -> Vec<JsonValue> {
    let mut data = Vec::with_capacity(tokens.len() * 5);
    let mut previous_line = 0usize;
    let mut previous_start = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        let delta_line = if index == 0 {
            token.line
        } else {
            token.line - previous_line
        };
        let delta_start = if index == 0 || delta_line > 0 {
            token.start
        } else {
            token.start - previous_start
        };
        data.extend([
            JsonValue::Number(delta_line as i64),
            JsonValue::Number(delta_start as i64),
            JsonValue::Number(token.length as i64),
            JsonValue::Number(token.kind as i64),
            JsonValue::Number(0),
        ]);
        previous_line = token.line;
        previous_start = token.start;
    }
    data
}

fn hot_reload_status(uri: &str, documents: &HashMap<String, String>) -> Option<JsonValue> {
    let current_path = std::fs::canonicalize(file_uri_path(uri)?).ok()?;
    let mut candidates = workspace_project_targets(documents);
    if !candidates
        .iter()
        .any(|candidate| candidate == &current_path)
    {
        candidates.push(current_path);
    }
    candidates.sort();
    candidates.dedup();

    candidates
        .into_iter()
        .filter_map(|candidate| {
            let status_path = crate::project::development_status_path(&candidate).ok()?;
            let text = std::fs::read_to_string(status_path).ok()?;
            let value = parse_json(&text).ok()?;
            let updated = value.get("updated_unix_ms")?.as_usize()?;
            Some((updated, value))
        })
        .max_by_key(|(updated, _)| *updated)
        .map(|(_, value)| value)
}

fn request_position(params: Option<&JsonValue>) -> Option<(&str, usize, usize)> {
    let params = params?;
    Some((
        params.get("textDocument")?.get("uri")?.as_str()?,
        params.get("position")?.get("line")?.as_usize()?,
        params.get("position")?.get("character")?.as_usize()?,
    ))
}

fn source_id_for_uri(uri: &str) -> SourceId {
    file_uri_path(uri)
        .as_deref()
        .and_then(canonical_source_id)
        .unwrap_or_else(|| SourceId::from_name(uri))
}

fn analyzed_document(uri: &str, source: &str) -> Option<crate::semantic::SemanticDatabase> {
    let source_id = source_id_for_uri(uri);
    let program = crate::parser::parse_all_with_source(source, source_id).ok()?;
    if !program.imports.is_empty() {
        return None;
    }
    let signatures = crate::typecheck::check_all(&program).ok()?;
    Some(crate::semantic::SemanticDatabase::from_analyzed(
        program, signatures,
    ))
}

fn symbol_for_position<'a>(
    database: &'a crate::semantic::SemanticDatabase,
    uri: &str,
    source: &str,
    line_index: usize,
    character: usize,
    encoding: PositionEncoding,
) -> Option<&'a crate::semantic::SemanticSymbol> {
    let source_id = source_id_for_uri(uri);
    let line = source.lines().nth(line_index).unwrap_or("");
    let byte = byte_offset_for_encoded_column(line, character, encoding);
    database
        .symbol_at(source_id, line_index + 1, byte + 1)
        .or_else(|| {
            let name = identifier_at(line, byte)?;
            if let Some(local) =
                visible_local_symbol_for_position(database, source, source_id, line_index + 1, name)
            {
                return Some(local);
            }
            let mut matches = database.symbols_named(name);
            let first = matches.next()?;
            matches.next().is_none().then_some(first)
        })
}

fn is_local_symbol_kind(kind: crate::semantic::SymbolKind) -> bool {
    use crate::semantic::SymbolKind;
    matches!(
        kind,
        SymbolKind::Parameter
            | SymbolKind::Binding
            | SymbolKind::MutableBinding
            | SymbolKind::PatternBinding
            | SymbolKind::LoopVariable
    )
}

fn visible_local_symbol_for_position<'a>(
    database: &'a crate::semantic::SemanticDatabase,
    source: &str,
    source_id: SourceId,
    line: usize,
    name: &str,
) -> Option<&'a crate::semantic::SemanticSymbol> {
    let function = database.program().functions.iter().find(|function| {
        function.name_span.source_id == source_id
            && function.line <= line
            && function_contains_line(source, function.line, line)
    })?;
    database
        .symbols()
        .iter()
        .filter(|symbol| {
            symbol.name == name
                && symbol.span.source_id == source_id
                && symbol.span.line >= function.line
                && symbol.span.line <= line
                && is_local_symbol_kind(symbol.kind)
                && local_symbol_visible_at_line(source, symbol, line)
        })
        .max_by_key(|symbol| (symbol.span.line, symbol.span.column))
}

#[cfg(test)]
fn definition_for_document(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
    character: usize,
    encoding: PositionEncoding,
) -> Option<JsonValue> {
    definition_for_document_cached(
        uri, source, documents, line_index, character, encoding, None,
    )
}

fn definition_for_document_cached(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
    character: usize,
    encoding: PositionEncoding,
    cache: Option<&mut crate::project::ProjectAnalysisCache>,
) -> Option<JsonValue> {
    if let Some((database, sources)) = analyzed_project_document_cached(uri, documents, cache) {
        if let Some(definition) = ui_property_definition(
            uri, source, line_index, character, encoding, &database, &sources,
        ) {
            return Some(definition);
        }
        if let Some(field) =
            struct_field_for_position(source, line_index, character, encoding, database.program())
        {
            let target = sources
                .iter()
                .find(|candidate| candidate.source_id == field.name_span.source_id)?;
            return Some(object([
                ("uri", JsonValue::String(file_uri_from_path(&target.path))),
                ("range", lsp_range(field.name_span, &target.text, encoding)),
            ]));
        }
        let symbol = symbol_for_position(&database, uri, source, line_index, character, encoding)?;
        let target = sources
            .iter()
            .find(|candidate| candidate.source_id == symbol.span.source_id)?;
        return Some(object([
            ("uri", JsonValue::String(file_uri_from_path(&target.path))),
            ("range", lsp_range(symbol.span, &target.text, encoding)),
        ]));
    }
    let database = analyzed_document(uri, source);
    if let Some(database) = database.as_ref()
        && let Some(definition) =
            ui_property_definition(uri, source, line_index, character, encoding, database, &[])
    {
        return Some(definition);
    }
    if database.is_none()
        && let Some(definition) = recovered_ui_property_definition(
            uri, source, documents, line_index, character, encoding,
        )
    {
        return Some(definition);
    }
    let database = database?;
    if let Some(field) =
        struct_field_for_position(source, line_index, character, encoding, database.program())
    {
        return Some(object([
            ("uri", JsonValue::String(uri.to_string())),
            ("range", lsp_range(field.name_span, source, encoding)),
        ]));
    }
    let symbol = symbol_for_position(&database, uri, source, line_index, character, encoding)?;
    Some(object([
        ("uri", JsonValue::String(uri.to_string())),
        ("range", lsp_range(symbol.span, source, encoding)),
    ]))
}

fn recovered_ui_property_definition(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
    character: usize,
    encoding: PositionEncoding,
) -> Option<JsonValue> {
    let lines = source.lines().collect::<Vec<_>>();
    let line = *lines.get(line_index)?;
    if leading_spaces(line) < 8 {
        return None;
    }
    let byte = byte_offset_for_encoded_column(line, character, encoding);
    let word = identifier_at(line, byte)?;
    let property = line.trim().split_once(':')?.0.trim();
    if property != word {
        return None;
    }
    let (kind, _) = enclosing_view_element(&lines, line_index)?;
    if !crate::typecheck::view_property_names(kind).is_empty() {
        return None;
    }
    let (view, target_source) = recovered_view_contract(uri, source, documents, line_index, kind)?;
    let param = view.params.iter().find(|param| param.name == property)?;
    if let Some(target) = target_source {
        return Some(object([
            ("uri", JsonValue::String(file_uri_from_path(&target.path))),
            ("range", lsp_range(param.name_span, &target.text, encoding)),
        ]));
    }
    Some(object([
        ("uri", JsonValue::String(uri.to_string())),
        ("range", lsp_range(param.name_span, source, encoding)),
    ]))
}

fn ui_property_definition(
    uri: &str,
    source: &str,
    line_index: usize,
    character: usize,
    encoding: PositionEncoding,
    database: &crate::semantic::SemanticDatabase,
    sources: &[crate::project::ProjectSource],
) -> Option<JsonValue> {
    let lines = source.lines().collect::<Vec<_>>();
    let line = *lines.get(line_index)?;
    if leading_spaces(line) < 8 {
        return None;
    }
    let byte = byte_offset_for_encoded_column(line, character, encoding);
    let word = identifier_at(line, byte)?;
    let property = line.trim().split_once(':')?.0.trim();
    if property != word {
        return None;
    }
    let (kind, _) = enclosing_view_element(&lines, line_index)?;
    if !crate::typecheck::view_property_names(kind).is_empty() {
        return None;
    }
    let view = database
        .program()
        .views
        .iter()
        .find(|view| view.name == kind)?;
    let param = view.params.iter().find(|param| param.name == property)?;
    if let Some(target) = sources
        .iter()
        .find(|source| source.source_id == param.name_span.source_id)
    {
        return Some(object([
            ("uri", JsonValue::String(file_uri_from_path(&target.path))),
            ("range", lsp_range(param.name_span, &target.text, encoding)),
        ]));
    }
    if param.name_span.source_id == source_id_for_uri(uri)
        || param.name_span.source_id == SourceId::UNKNOWN
    {
        return Some(object([
            ("uri", JsonValue::String(uri.to_string())),
            ("range", lsp_range(param.name_span, source, encoding)),
        ]));
    }
    None
}

fn analyzed_project_document(
    uri: &str,
    documents: &HashMap<String, String>,
) -> Option<(
    crate::semantic::SemanticDatabase,
    Vec<crate::project::ProjectSource>,
)> {
    analyzed_project_document_cached(uri, documents, None)
}

fn analyzed_project_document_cached(
    uri: &str,
    documents: &HashMap<String, String>,
    cache: Option<&mut crate::project::ProjectAnalysisCache>,
) -> Option<(
    crate::semantic::SemanticDatabase,
    Vec<crate::project::ProjectSource>,
)> {
    let current_path = std::fs::canonicalize(file_uri_path(uri)?).ok()?;
    let overlays = document_overlays(documents);
    let mut candidates = workspace_project_targets(documents);
    if !candidates
        .iter()
        .any(|candidate| candidate == &current_path)
    {
        candidates.push(current_path.clone());
    }
    candidates.sort();
    candidates.dedup();

    let mut best: Option<(usize, bool, crate::project::ProjectAnalysis)> = None;
    let mut cache = cache;
    for candidate in candidates {
        let analysis = if let Some(cache) = cache.as_deref_mut() {
            cache.analyze_with_overlays(&candidate, &overlays)
        } else {
            crate::project::analyze_with_overlays(&candidate, &overlays)
        };
        let Ok(analysis) = analysis else {
            continue;
        };
        if !analysis
            .sources
            .iter()
            .any(|source| source.path == current_path)
        {
            continue;
        }
        let manifest_backed = candidate
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "flux.toml");
        let score = (analysis.sources.len(), manifest_backed);
        if best
            .as_ref()
            .is_none_or(|(count, manifest, _)| score > (*count, *manifest))
        {
            best = Some((analysis.sources.len(), manifest_backed, analysis));
        }
    }
    let (_, _, analysis) = best?;
    let database =
        crate::semantic::SemanticDatabase::from_analyzed(analysis.program, analysis.signatures);
    Some((database, analysis.sources))
}

#[cfg(test)]
fn references_for_document(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
    character: usize,
    encoding: PositionEncoding,
) -> Vec<JsonValue> {
    references_for_document_cached(
        uri, source, documents, line_index, character, encoding, None,
    )
}

fn local_symbol_occurrences(
    source: &str,
    source_id: SourceId,
    target: &crate::semantic::SemanticSymbol,
    database: &crate::semantic::SemanticDatabase,
) -> Vec<SourceSpan> {
    identifier_occurrences(source, &target.name)
        .into_iter()
        .filter(|span| {
            visible_local_symbol_for_position(database, source, source_id, span.line, &target.name)
                .is_some_and(|symbol| symbol.span == target.span)
        })
        .collect()
}

fn local_symbol_name_conflicts(
    source: &str,
    target: &crate::semantic::SemanticSymbol,
    new_name: &str,
    database: &crate::semantic::SemanticDatabase,
) -> bool {
    let source_id = target.span.source_id;
    let Some(function) = database.program().functions.iter().find(|function| {
        function.name_span.source_id == source_id
            && function.line <= target.span.line
            && function_contains_line(source, function.line, target.span.line)
    }) else {
        return true;
    };
    database.symbols().iter().any(|symbol| {
        symbol.span != target.span
            && symbol.name == new_name
            && symbol.span.source_id == source_id
            && symbol.span.line >= function.line
            && function_contains_line(source, function.line, symbol.span.line)
            && is_local_symbol_kind(symbol.kind)
    })
}

fn references_for_document_cached(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
    character: usize,
    encoding: PositionEncoding,
    cache: Option<&mut crate::project::ProjectAnalysisCache>,
) -> Vec<JsonValue> {
    if let Some((database, sources)) = analyzed_project_document_cached(uri, documents, cache) {
        let Some(symbol) =
            symbol_for_position(&database, uri, source, line_index, character, encoding)
        else {
            return Vec::new();
        };
        if is_local_symbol_kind(symbol.kind) {
            let Some(project_source) = sources
                .iter()
                .find(|source| source.source_id == symbol.span.source_id)
            else {
                return Vec::new();
            };
            return local_symbol_occurrences(
                &project_source.text,
                project_source.source_id,
                symbol,
                &database,
            )
            .into_iter()
            .map(|span| {
                object([
                    ("uri", JsonValue::String(project_source_uri(project_source))),
                    ("range", lsp_range(span, &project_source.text, encoding)),
                ])
            })
            .collect();
        }
        return sources
            .iter()
            .flat_map(|project_source| {
                identifier_occurrences(&project_source.text, &symbol.name)
                    .into_iter()
                    .map(|span| {
                        object([
                            ("uri", JsonValue::String(project_source_uri(project_source))),
                            ("range", lsp_range(span, &project_source.text, encoding)),
                        ])
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
    }
    let Some(database) = analyzed_document(uri, source) else {
        return Vec::new();
    };
    let Some(symbol) = symbol_for_position(&database, uri, source, line_index, character, encoding)
    else {
        return Vec::new();
    };
    let occurrences = if is_local_symbol_kind(symbol.kind) {
        local_symbol_occurrences(source, source_id_for_uri(uri), symbol, &database)
    } else {
        identifier_occurrences(source, &symbol.name)
    };
    occurrences
        .into_iter()
        .map(|span| {
            object([
                ("uri", JsonValue::String(uri.to_string())),
                ("range", lsp_range(span, source, encoding)),
            ])
        })
        .collect()
}

#[cfg(test)]
fn rename_for_document(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
    character: usize,
    new_name: &str,
    encoding: PositionEncoding,
) -> Option<JsonValue> {
    rename_for_document_cached(
        uri,
        source,
        documents,
        line_index,
        character,
        new_name,
        LspRequestContext {
            encoding,
            cache: None,
        },
    )
}

fn rename_for_document_cached(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
    character: usize,
    new_name: &str,
    context: LspRequestContext<'_>,
) -> Option<JsonValue> {
    let LspRequestContext { encoding, cache } = context;
    if !is_valid_identifier(new_name) || is_flux_keyword(new_name) {
        return None;
    }
    if let Some((database, sources)) = analyzed_project_document_cached(uri, documents, cache) {
        let symbol = symbol_for_position(&database, uri, source, line_index, character, encoding)?;
        if is_local_symbol_kind(symbol.kind) {
            let project_source = sources
                .iter()
                .find(|source| source.source_id == symbol.span.source_id)?;
            if local_symbol_name_conflicts(&project_source.text, symbol, new_name, &database) {
                return None;
            }
            let edits = local_symbol_occurrences(
                &project_source.text,
                project_source.source_id,
                symbol,
                &database,
            )
            .into_iter()
            .map(|span| {
                object([
                    ("range", lsp_range(span, &project_source.text, encoding)),
                    ("newText", JsonValue::String(new_name.to_string())),
                ])
            })
            .collect::<Vec<_>>();
            let mut changes = BTreeMap::new();
            changes.insert(project_source_uri(project_source), JsonValue::Array(edits));
            return Some(object([("changes", JsonValue::Object(changes))]));
        }
        if database.symbols_named(new_name).next().is_some() {
            return None;
        }
        let mut changes = BTreeMap::new();
        for project_source in &sources {
            let edits = identifier_occurrences(&project_source.text, &symbol.name)
                .into_iter()
                .map(|span| {
                    object([
                        ("range", lsp_range(span, &project_source.text, encoding)),
                        ("newText", JsonValue::String(new_name.to_string())),
                    ])
                })
                .collect::<Vec<_>>();
            if !edits.is_empty() {
                changes.insert(project_source_uri(project_source), JsonValue::Array(edits));
            }
        }
        return Some(object([("changes", JsonValue::Object(changes))]));
    }
    let database = analyzed_document(uri, source)?;
    let symbol = symbol_for_position(&database, uri, source, line_index, character, encoding)?;
    if is_local_symbol_kind(symbol.kind) {
        if local_symbol_name_conflicts(source, symbol, new_name, &database) {
            return None;
        }
        let edits = local_symbol_occurrences(source, source_id_for_uri(uri), symbol, &database)
            .into_iter()
            .map(|span| {
                object([
                    ("range", lsp_range(span, source, encoding)),
                    ("newText", JsonValue::String(new_name.to_string())),
                ])
            })
            .collect::<Vec<_>>();
        let mut changes = BTreeMap::new();
        changes.insert(uri.to_string(), JsonValue::Array(edits));
        return Some(object([("changes", JsonValue::Object(changes))]));
    }
    if database.symbols_named(new_name).next().is_some() {
        return None;
    }
    let edits = identifier_occurrences(source, &symbol.name)
        .into_iter()
        .map(|span| {
            object([
                ("range", lsp_range(span, source, encoding)),
                ("newText", JsonValue::String(new_name.to_string())),
            ])
        })
        .collect::<Vec<_>>();
    let mut changes = BTreeMap::new();
    changes.insert(uri.to_string(), JsonValue::Array(edits));
    Some(object([("changes", JsonValue::Object(changes))]))
}

fn project_source_uri(source: &crate::project::ProjectSource) -> String {
    file_uri_from_path(&source.path)
}

fn identifier_occurrences(source: &str, name: &str) -> Vec<SourceSpan> {
    let mut occurrences = Vec::new();
    for (line_index, line) in source.lines().enumerate() {
        let bytes = line.as_bytes();
        let mut index = 0usize;
        while index < bytes.len() {
            match bytes[index] {
                b'r' if bytes.get(index + 1) == Some(&b'"') => {
                    index += 2;
                    while index < bytes.len() {
                        let byte = bytes[index];
                        index += 1;
                        if byte == b'"' {
                            break;
                        }
                    }
                }
                b'"' => {
                    index += 1;
                    let mut escaped = false;
                    while index < bytes.len() {
                        let byte = bytes[index];
                        index += 1;
                        if escaped {
                            escaped = false;
                        } else if byte == b'\\' {
                            escaped = true;
                        } else if byte == b'"' {
                            break;
                        }
                    }
                }
                byte if byte == b'_' || byte.is_ascii_alphabetic() => {
                    let start = index;
                    index += 1;
                    while index < bytes.len() && is_identifier_byte(bytes[index]) {
                        index += 1;
                    }
                    if &line[start..index] == name {
                        occurrences.push(SourceSpan::new(line_index + 1, start + 1, name.len()));
                    }
                }
                _ => index += 1,
            }
        }
    }
    occurrences
}

fn is_valid_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

#[cfg(test)]
fn hover_for_document(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
    character: usize,
    encoding: PositionEncoding,
) -> Option<JsonValue> {
    hover_for_document_cached(
        uri, source, documents, line_index, character, encoding, None,
    )
}

fn hover_for_document_cached(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
    character: usize,
    encoding: PositionEncoding,
    cache: Option<&mut crate::project::ProjectAnalysisCache>,
) -> Option<JsonValue> {
    if let Some(hover) = ui_contract_hover(uri, source, documents, line_index, character, encoding)
    {
        return Some(hover);
    }
    let project_database =
        analyzed_project_document_cached(uri, documents, cache).map(|(database, _)| database);
    let standalone_database;
    let database = if let Some(database) = project_database.as_ref() {
        database
    } else {
        standalone_database = analyzed_document(uri, source)?;
        &standalone_database
    };
    let line = source.lines().nth(line_index).unwrap_or("");
    let byte = byte_offset_for_encoded_column(line, character, encoding);
    if let Some((property, ty)) =
        list_property_for_position(source, line_index, character, encoding, database.program())
    {
        let hovered_name = identifier_at(line, byte).unwrap_or(&property);
        let hovered_start = identifier_start_at(line, byte).unwrap_or(byte);
        let hovered_span = SourceSpan::new(line_index + 1, hovered_start + 1, hovered_name.len());
        return Some(object([
            (
                "contents",
                object([
                    ("kind", JsonValue::String("markdown".to_string())),
                    (
                        "value",
                        JsonValue::String(format!(
                            "```flux\nproperty {property}: {}\n```",
                            ty.name()
                        )),
                    ),
                ]),
            ),
            ("range", lsp_range(hovered_span, source, encoding)),
        ]));
    }
    if let Some(field) =
        struct_field_for_position(source, line_index, character, encoding, database.program())
    {
        let hovered_name = identifier_at(line, byte).unwrap_or(&field.name);
        let hovered_start = identifier_start_at(line, byte).unwrap_or(byte);
        let hovered_span = SourceSpan::new(line_index + 1, hovered_start + 1, hovered_name.len());
        return Some(object([
            (
                "contents",
                object([
                    ("kind", JsonValue::String("markdown".to_string())),
                    (
                        "value",
                        JsonValue::String(format!(
                            "```flux\nfield {}: {}\n```",
                            field.name,
                            field.ty.name()
                        )),
                    ),
                ]),
            ),
            ("range", lsp_range(hovered_span, source, encoding)),
        ]));
    }
    let symbol = symbol_for_position(database, uri, source, line_index, character, encoding)?;
    let description = hover_description(symbol, database);
    let hovered_name = identifier_at(line, byte).unwrap_or(&symbol.name);
    let hovered_start = identifier_start_at(line, byte).unwrap_or(byte);
    let hovered_span = SourceSpan::new(line_index + 1, hovered_start + 1, hovered_name.len());
    Some(object([
        (
            "contents",
            object([
                ("kind", JsonValue::String("markdown".to_string())),
                (
                    "value",
                    JsonValue::String(format!("```flux\n{description}\n```")),
                ),
            ]),
        ),
        ("range", lsp_range(hovered_span, source, encoding)),
    ]))
}

fn ui_contract_hover(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
    character: usize,
    encoding: PositionEncoding,
) -> Option<JsonValue> {
    let lines = source.lines().collect::<Vec<_>>();
    let line = *lines.get(line_index)?;
    let byte = byte_offset_for_encoded_column(line, character, encoding);
    let word = identifier_at(line, byte)?;
    let start = identifier_start_at(line, byte)?;
    let indent = leading_spaces(line);
    let description = if indent == 4 {
        let tokens = line.split_whitespace().collect::<Vec<_>>();
        if tokens.len() < 4 || tokens[2] != "at" || tokens[0] != word {
            return None;
        }
        view_element_hover_description(word, uri, source, documents, line_index)?
    } else if indent >= 8 {
        let (kind, _) = enclosing_view_element(&lines, line_index)?;
        let property_name = line.trim().split_once(':')?.0.trim();
        if property_name != word {
            return None;
        }
        view_property_hover_description(kind, property_name, uri, source, documents, line_index)?
    } else {
        return None;
    };
    let span = SourceSpan::new(line_index + 1, start + 1, word.len());
    Some(object([
        (
            "contents",
            object([
                ("kind", JsonValue::String("markdown".to_string())),
                (
                    "value",
                    JsonValue::String(format!("```flux\n{description}\n```")),
                ),
            ]),
        ),
        ("range", lsp_range(span, source, encoding)),
    ]))
}

fn view_element_hover_description(
    kind: &str,
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
) -> Option<String> {
    let properties = crate::typecheck::view_property_names(kind);
    if !properties.is_empty() {
        let rendered = properties
            .iter()
            .filter_map(|property| {
                crate::typecheck::view_property_type(kind, property)
                    .map(|ty| format!("{property}: {}", ty.name()))
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Some(format!("element {kind} {{ {rendered} }}"));
    }
    let (view, _) = recovered_view_contract(uri, source, documents, line_index, kind)?;
    let params = view
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, param.ty.name()))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!("view {}({params})", view.name))
}

fn view_property_hover_description(
    kind: &str,
    property: &str,
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    line_index: usize,
) -> Option<String> {
    if let Some(ty) = crate::typecheck::view_property_type(kind, property) {
        return Some(format!("property {kind}.{property}: {}", ty.name()));
    }
    let (view, _) = recovered_view_contract(uri, source, documents, line_index, kind)?;
    let param = view.params.iter().find(|param| param.name == property)?;
    Some(format!("property {kind}.{property}: {}", param.ty.name()))
}

fn hover_description(
    symbol: &crate::semantic::SemanticSymbol,
    database: &crate::semantic::SemanticDatabase,
) -> String {
    use crate::semantic::SymbolKind;
    match symbol.kind {
        SymbolKind::Function => database
            .signature(&symbol.name)
            .map(|signature| format_signature(&symbol.name, signature))
            .unwrap_or_else(|| format!("fn {}", symbol.name)),
        SymbolKind::MutableBinding => typed_symbol("var", symbol),
        SymbolKind::Binding => typed_symbol("let", symbol),
        SymbolKind::Constant => typed_symbol("const", symbol),
        SymbolKind::Parameter => typed_symbol("parameter", symbol),
        SymbolKind::PatternBinding => typed_symbol("pattern", symbol),
        SymbolKind::LoopVariable => typed_symbol("loop", symbol),
        SymbolKind::StructField => typed_symbol("field", symbol),
        SymbolKind::ViewProperty => typed_symbol("property", symbol),
        SymbolKind::ViewState => typed_symbol("state", symbol),
        SymbolKind::ViewDerived => typed_symbol("derived", symbol),
        SymbolKind::InterfaceFunction | SymbolKind::InterfaceImplementationMapping => {
            typed_symbol("fn", symbol)
        }
        SymbolKind::TypeAlias => symbol
            .ty
            .as_ref()
            .map(|ty| format!("type {} = {}", symbol.name, ty.name()))
            .unwrap_or_else(|| format!("type {}", symbol.name)),
        SymbolKind::Interface => format!("interface {}", symbol.name),
        SymbolKind::InterfaceImplementation => format!("impl {}", symbol.name),
        SymbolKind::Enum => format!("enum {}", symbol.name),
        SymbolKind::EnumVariant => typed_symbol("variant", symbol),
        SymbolKind::Struct => format!("struct {}", symbol.name),
        SymbolKind::View => format!("view {}", symbol.name),
        SymbolKind::ViewElement => format!("element {}", symbol.name),
    }
}

fn typed_symbol(prefix: &str, symbol: &crate::semantic::SemanticSymbol) -> String {
    symbol
        .ty
        .as_ref()
        .map(|ty| format!("{prefix} {}: {}", symbol.name, ty.name()))
        .unwrap_or_else(|| format!("{prefix} {}", symbol.name))
}

fn format_signature(name: &str, signature: &crate::typecheck::Signature) -> String {
    let mut params = Vec::new();
    let mut emitted_named_marker = false;
    for param in &signature.param_details {
        if param.named_only && !emitted_named_marker {
            params.push("*".to_string());
            emitted_named_marker = true;
        }
        params.push(format!("{}: {}", param.name, param.ty.name()));
    }
    let returns = match signature.returns.as_slice() {
        [] => "void".to_string(),
        [ty] => ty.name(),
        values => format!(
            "({})",
            values
                .iter()
                .map(|ty| ty.name())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    format!("fn {name}({}) -> {returns}", params.join(", "))
}

fn identifier_start_at(line: &str, byte_offset: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut cursor = byte_offset.min(bytes.len().saturating_sub(1));
    if !is_identifier_byte(bytes[cursor]) && cursor > 0 && is_identifier_byte(bytes[cursor - 1]) {
        cursor -= 1;
    }
    if !is_identifier_byte(bytes[cursor]) {
        return None;
    }
    while cursor > 0 && is_identifier_byte(bytes[cursor - 1]) {
        cursor -= 1;
    }
    Some(cursor)
}

fn identifier_at(line: &str, byte_offset: usize) -> Option<&str> {
    let bytes = line.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut cursor = byte_offset.min(bytes.len().saturating_sub(1));
    if !is_identifier_byte(bytes[cursor]) && cursor > 0 && is_identifier_byte(bytes[cursor - 1]) {
        cursor -= 1;
    }
    if !is_identifier_byte(bytes[cursor]) {
        return None;
    }
    let mut start = cursor;
    while start > 0 && is_identifier_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = cursor + 1;
    while end < bytes.len() && is_identifier_byte(bytes[end]) {
        end += 1;
    }
    std::str::from_utf8(&bytes[start..end]).ok()
}

fn is_identifier_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn byte_offset_for_encoded_column(
    line: &str,
    character: usize,
    encoding: PositionEncoding,
) -> usize {
    match encoding {
        PositionEncoding::Utf8 => character.min(line.len()),
        PositionEncoding::Utf16 => {
            let mut units = 0usize;
            for (byte, ch) in line.char_indices() {
                if units >= character {
                    return byte;
                }
                let next = units + ch.len_utf16();
                if next > character {
                    return byte;
                }
                units = next;
            }
            line.len()
        }
    }
}

fn document_diagnostics_with_overlays(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
) -> Vec<Diagnostic> {
    if let Some(path) = file_uri_path(uri)
        && let Ok(current_path) = std::fs::canonicalize(&path)
    {
        let overlays = document_overlays(documents);
        let current_id = SourceId::from_name(current_path.to_string_lossy().as_ref());
        if let Some((diagnostics, entry_path)) =
            best_workspace_analysis(&current_path, documents, &overlays)
        {
            let is_entry = entry_path
                .as_ref()
                .is_some_and(|entry| entry == &current_path);
            return diagnostics
                .into_iter()
                .filter(|diagnostic| match diagnostic.span {
                    Some(span) => {
                        span.source_id == current_id || span.source_id == SourceId::UNKNOWN
                    }
                    None => is_entry,
                })
                .collect();
        }
    }

    let source_id = SourceId::from_name(uri);
    match crate::parser::parse_all_with_source(source, source_id) {
        Err(diagnostics) => diagnostics,
        Ok(program) if program.imports.is_empty() => crate::typecheck::check_all(&program)
            .err()
            .unwrap_or_default(),
        Ok(_) => Vec::new(),
    }
}

fn best_workspace_analysis(
    current_path: &std::path::Path,
    documents: &HashMap<String, String>,
    overlays: &HashMap<PathBuf, String>,
) -> Option<(Vec<Diagnostic>, Option<PathBuf>)> {
    let mut candidates = workspace_project_targets(documents);
    if !candidates.iter().any(|candidate| candidate == current_path) {
        candidates.push(current_path.to_path_buf());
    }
    candidates.sort();
    candidates.dedup();

    let mut best: Option<(usize, bool, Vec<Diagnostic>, Option<PathBuf>)> = None;
    for candidate in candidates {
        let (diagnostics, sources) = crate::project::check_with_overlays(&candidate, overlays);
        if !sources.iter().any(|source| source.path == current_path) {
            continue;
        }
        let entry_path = crate::project::resolve_entry(&candidate)
            .ok()
            .and_then(|entry| std::fs::canonicalize(entry).ok());
        let manifest_backed = candidate
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "flux.toml");
        let score = (sources.len(), manifest_backed);
        if best
            .as_ref()
            .is_none_or(|(count, manifest, _, _)| score > (*count, *manifest))
        {
            best = Some((sources.len(), manifest_backed, diagnostics, entry_path));
        }
    }
    best.map(|(_, _, diagnostics, entry_path)| (diagnostics, entry_path))
}

fn workspace_project_targets(documents: &HashMap<String, String>) -> Vec<PathBuf> {
    let mut targets = Vec::new();
    for uri in documents.keys() {
        let Some(path) = file_uri_path(uri) else {
            continue;
        };
        let Ok(canonical) = std::fs::canonicalize(path) else {
            continue;
        };
        targets.push(canonical.clone());
        if let Some(manifest) = nearest_package_manifest(&canonical) {
            targets.push(manifest);
        }
    }
    targets
}

fn nearest_package_manifest(path: &std::path::Path) -> Option<PathBuf> {
    let mut directory = path.parent();
    while let Some(current) = directory {
        let manifest = current.join("flux.toml");
        if manifest.is_file() {
            return std::fs::canonicalize(manifest).ok();
        }
        directory = current.parent();
    }
    None
}

fn invalidate_lsp_analysis_path(cache: &mut crate::project::ProjectAnalysisCache, uri: &str) {
    if let Some(path) = file_uri_path(uri) {
        cache.invalidate_path(&path);
    }
}

fn document_overlays(documents: &HashMap<String, String>) -> HashMap<PathBuf, String> {
    documents
        .iter()
        .filter_map(|(uri, source)| {
            let path = file_uri_path(uri)?;
            let canonical = std::fs::canonicalize(path).ok()?;
            Some((canonical, source.clone()))
        })
        .collect()
}

fn canonical_source_id(path: &std::path::Path) -> Option<SourceId> {
    std::fs::canonicalize(path)
        .ok()
        .map(|path| SourceId::from_name(path.to_string_lossy().as_ref()))
}

fn file_uri_from_path(path: &std::path::Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let mut uri = String::from("file://");
    if cfg!(windows) && !normalized.starts_with('/') {
        uri.push('/');
    }
    for byte in normalized.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'.' | b'_' | b'~') {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

fn file_uri_path(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?;
    let mut bytes = Vec::with_capacity(encoded.len());
    let input = encoded.as_bytes();
    let mut index = 0usize;
    while index < input.len() {
        if input[index] == b'%' && index + 2 < input.len() {
            let hex = std::str::from_utf8(&input[index + 1..index + 3]).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            bytes.push(input[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(bytes).ok()?;
    #[cfg(windows)]
    let decoded = decoded
        .strip_prefix('/')
        .filter(|path| path.as_bytes().get(1) == Some(&b':'))
        .unwrap_or(&decoded)
        .to_string();
    Some(PathBuf::from(decoded))
}

fn code_actions(
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    encoding: PositionEncoding,
) -> Vec<JsonValue> {
    let source_id = file_uri_path(uri)
        .as_deref()
        .and_then(canonical_source_id)
        .unwrap_or_else(|| SourceId::from_name(uri));
    document_diagnostics_with_overlays(uri, source, documents)
        .into_iter()
        .flat_map(|diagnostic| {
            let rendered = lsp_diagnostic(&diagnostic, source, source_id, uri, encoding);
            diagnostic
                .fixes
                .iter()
                .filter(|fix| {
                    fix.span.source_id == source_id || fix.span.source_id == SourceId::UNKNOWN
                })
                .map(|fix| {
                    let edit = object([
                        ("range", lsp_range(fix.span, source, encoding)),
                        ("newText", JsonValue::String(fix.replacement.clone())),
                    ]);
                    let mut changes = BTreeMap::new();
                    changes.insert(uri.to_string(), JsonValue::Array(vec![edit]));
                    object([
                        ("title", JsonValue::String(fix.message.clone())),
                        ("kind", JsonValue::String("quickfix".to_string())),
                        ("isPreferred", JsonValue::Bool(true)),
                        ("diagnostics", JsonValue::Array(vec![rendered.clone()])),
                        ("edit", object([("changes", JsonValue::Object(changes))])),
                    ])
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn publish_workspace_diagnostics<W: Write>(
    writer: &mut W,
    documents: &HashMap<String, String>,
    encoding: PositionEncoding,
) -> io::Result<()> {
    let mut open_documents = documents.iter().collect::<Vec<_>>();
    open_documents.sort_by_key(|(uri, _)| *uri);
    for (uri, source) in open_documents {
        publish_document_diagnostics(writer, uri, source, documents, encoding)?;
    }
    Ok(())
}

fn publish_document_diagnostics<W: Write>(
    writer: &mut W,
    uri: &str,
    source: &str,
    documents: &HashMap<String, String>,
    encoding: PositionEncoding,
) -> io::Result<()> {
    let source_id = file_uri_path(uri)
        .as_deref()
        .and_then(canonical_source_id)
        .unwrap_or_else(|| SourceId::from_name(uri));
    let diagnostics = document_diagnostics_with_overlays(uri, source, documents);
    let rendered = diagnostics
        .iter()
        .map(|diagnostic| lsp_diagnostic(diagnostic, source, source_id, uri, encoding))
        .collect::<Vec<_>>();
    let notification = object([
        ("jsonrpc", JsonValue::String("2.0".to_string())),
        (
            "method",
            JsonValue::String("textDocument/publishDiagnostics".to_string()),
        ),
        (
            "params",
            object([
                ("uri", JsonValue::String(uri.to_string())),
                ("diagnostics", JsonValue::Array(rendered)),
            ]),
        ),
    ]);
    write_message(writer, &notification.to_json())
}

fn publish_empty_diagnostics<W: Write>(writer: &mut W, uri: &str) -> io::Result<()> {
    let notification = object([
        ("jsonrpc", JsonValue::String("2.0".to_string())),
        (
            "method",
            JsonValue::String("textDocument/publishDiagnostics".to_string()),
        ),
        (
            "params",
            object([
                ("uri", JsonValue::String(uri.to_string())),
                ("diagnostics", JsonValue::Array(Vec::new())),
            ]),
        ),
    ]);
    write_message(writer, &notification.to_json())
}

fn lsp_diagnostic(
    diagnostic: &Diagnostic,
    source: &str,
    source_id: SourceId,
    uri: &str,
    encoding: PositionEncoding,
) -> JsonValue {
    let span = diagnostic.span.unwrap_or(SourceSpan::new(1, 1, 1));
    let mut values = BTreeMap::from([
        ("range".to_string(), lsp_range(span, source, encoding)),
        ("severity".to_string(), JsonValue::Number(1)),
        ("source".to_string(), JsonValue::String("flux".to_string())),
        (
            "code".to_string(),
            JsonValue::String(diagnostic.stage.name().to_string()),
        ),
        (
            "message".to_string(),
            JsonValue::String(diagnostic.message.clone()),
        ),
    ]);

    let related = diagnostic
        .labels
        .iter()
        .filter(|label| {
            label.span.source_id == source_id || label.span.source_id == SourceId::UNKNOWN
        })
        .map(|label| {
            object([
                (
                    "location",
                    object([
                        ("uri", JsonValue::String(uri.to_string())),
                        ("range", lsp_range(label.span, source, encoding)),
                    ]),
                ),
                ("message", JsonValue::String(label.message.clone())),
            ])
        })
        .collect::<Vec<_>>();
    if !related.is_empty() {
        values.insert("relatedInformation".to_string(), JsonValue::Array(related));
    }

    let fixes = diagnostic
        .fixes
        .iter()
        .filter(|fix| fix.span.source_id == source_id || fix.span.source_id == SourceId::UNKNOWN)
        .map(|fix| {
            object([
                ("range", lsp_range(fix.span, source, encoding)),
                ("replacement", JsonValue::String(fix.replacement.clone())),
                ("message", JsonValue::String(fix.message.clone())),
            ])
        })
        .collect::<Vec<_>>();
    values.insert(
        "data".to_string(),
        object([
            (
                "stage",
                JsonValue::String(diagnostic.stage.name().to_string()),
            ),
            ("fixes", JsonValue::Array(fixes)),
            (
                "notes",
                JsonValue::Array(
                    diagnostic
                        .notes
                        .iter()
                        .cloned()
                        .map(JsonValue::String)
                        .collect(),
                ),
            ),
        ]),
    );
    JsonValue::Object(values)
}

fn lsp_range(span: SourceSpan, source: &str, encoding: PositionEncoding) -> JsonValue {
    let line_index = span.line.saturating_sub(1);
    let line = source.lines().nth(line_index).unwrap_or("");
    let start_byte = span.column.saturating_sub(1).min(line.len());
    let end_byte = start_byte.saturating_add(span.length).min(line.len());
    let start = encoded_column(line, start_byte, encoding);
    let end = encoded_column(line, end_byte, encoding);
    object([
        (
            "start",
            object([
                ("line", JsonValue::Number(line_index as i64)),
                ("character", JsonValue::Number(start as i64)),
            ]),
        ),
        (
            "end",
            object([
                ("line", JsonValue::Number(line_index as i64)),
                ("character", JsonValue::Number(end as i64)),
            ]),
        ),
    ])
}

fn encoded_column(line: &str, byte_offset: usize, encoding: PositionEncoding) -> usize {
    let mut boundary = byte_offset.min(line.len());
    while boundary > 0 && !line.is_char_boundary(boundary) {
        boundary -= 1;
    }
    match encoding {
        PositionEncoding::Utf8 => boundary,
        PositionEncoding::Utf16 => line[..boundary].encode_utf16().count(),
    }
}

fn jsonrpc_result(id: JsonValue, result: JsonValue) -> JsonValue {
    object([
        ("jsonrpc", JsonValue::String("2.0".to_string())),
        ("id", id),
        ("result", result),
    ])
}

fn jsonrpc_error(id: JsonValue, code: i64, message: &str) -> JsonValue {
    object([
        ("jsonrpc", JsonValue::String("2.0".to_string())),
        ("id", id),
        (
            "error",
            object([
                ("code", JsonValue::Number(code)),
                ("message", JsonValue::String(message.to_string())),
            ]),
        ),
    ])
}

fn object<const N: usize>(entries: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn read_message<R: BufRead>(reader: &mut R) -> io::Result<Option<String>> {
    let mut content_length = None;
    loop {
        let mut header = String::new();
        let bytes = reader.read_line(&mut header)?;
        if bytes == 0 {
            return Ok(None);
        }
        if header == "\r\n" || header == "\n" {
            break;
        }
        if let Some(value) = header
            .trim_end()
            .strip_prefix("Content-Length:")
            .map(str::trim)
        {
            content_length = value.parse::<usize>().ok();
        }
    }
    let Some(length) = content_length else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "LSP message is missing Content-Length",
        ));
    };
    let mut bytes = vec![0u8; length];
    reader.read_exact(&mut bytes)?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn write_message<W: Write>(writer: &mut W, payload: &str) -> io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n{payload}", payload.len())?;
    writer.flush()
}

fn parse_json(input: &str) -> Result<JsonValue, String> {
    let mut parser = JsonParser {
        bytes: input.as_bytes(),
        index: 0,
    };
    let value = parser.parse_value()?;
    parser.skip_whitespace();
    if parser.index != parser.bytes.len() {
        return Err("unexpected trailing JSON content".to_string());
    }
    Ok(value)
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl JsonParser<'_> {
    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_whitespace();
        match self.bytes.get(self.index).copied() {
            Some(b'n') => {
                self.expect_keyword(b"null")?;
                Ok(JsonValue::Null)
            }
            Some(b't') => {
                self.expect_keyword(b"true")?;
                Ok(JsonValue::Bool(true))
            }
            Some(b'f') => {
                self.expect_keyword(b"false")?;
                Ok(JsonValue::Bool(false))
            }
            Some(b'"') => self.parse_string().map(JsonValue::String),
            Some(b'[') => self.parse_array(),
            Some(b'{') => self.parse_object(),
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            _ => Err("expected JSON value".to_string()),
        }
    }

    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.index += 1;
        let mut values = Vec::new();
        self.skip_whitespace();
        if self.consume(b']') {
            return Ok(JsonValue::Array(values));
        }
        loop {
            values.push(self.parse_value()?);
            self.skip_whitespace();
            if self.consume(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(JsonValue::Array(values))
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.index += 1;
        let mut values = BTreeMap::new();
        self.skip_whitespace();
        if self.consume(b'}') {
            return Ok(JsonValue::Object(values));
        }
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            let value = self.parse_value()?;
            values.insert(key, value);
            self.skip_whitespace();
            if self.consume(b'}') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(JsonValue::Object(values))
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut value = String::new();
        while let Some(byte) = self.bytes.get(self.index).copied() {
            self.index += 1;
            match byte {
                b'"' => return Ok(value),
                b'\\' => {
                    let escaped = self
                        .bytes
                        .get(self.index)
                        .copied()
                        .ok_or_else(|| "unterminated JSON escape".to_string())?;
                    self.index += 1;
                    match escaped {
                        b'"' => value.push('"'),
                        b'\\' => value.push('\\'),
                        b'/' => value.push('/'),
                        b'b' => value.push('\u{0008}'),
                        b'f' => value.push('\u{000c}'),
                        b'n' => value.push('\n'),
                        b'r' => value.push('\r'),
                        b't' => value.push('\t'),
                        b'u' => value.push(self.parse_unicode_escape()?),
                        _ => return Err("unsupported JSON escape".to_string()),
                    }
                }
                0x00..=0x1f => return Err("control character in JSON string".to_string()),
                ascii if ascii.is_ascii() => value.push(ascii as char),
                _ => {
                    self.index -= 1;
                    let rest = std::str::from_utf8(&self.bytes[self.index..])
                        .map_err(|_| "invalid UTF-8 in JSON string".to_string())?;
                    let ch = rest
                        .chars()
                        .next()
                        .ok_or_else(|| "invalid UTF-8 in JSON string".to_string())?;
                    value.push(ch);
                    self.index += ch.len_utf8();
                }
            }
        }
        Err("unterminated JSON string".to_string())
    }

    fn parse_unicode_escape(&mut self) -> Result<char, String> {
        let first = self.parse_hex_quad()?;
        if (0xd800..=0xdbff).contains(&first) {
            if self.bytes.get(self.index..self.index + 2) != Some(b"\\u") {
                return Err("unpaired high surrogate in JSON string".to_string());
            }
            self.index += 2;
            let second = self.parse_hex_quad()?;
            if !(0xdc00..=0xdfff).contains(&second) {
                return Err("invalid low surrogate in JSON string".to_string());
            }
            let codepoint = 0x10000 + (((first - 0xd800) as u32) << 10) + (second - 0xdc00) as u32;
            char::from_u32(codepoint).ok_or_else(|| "invalid JSON unicode escape".to_string())
        } else {
            char::from_u32(first as u32).ok_or_else(|| "invalid JSON unicode escape".to_string())
        }
    }

    fn parse_hex_quad(&mut self) -> Result<u16, String> {
        let end = self.index + 4;
        let bytes = self
            .bytes
            .get(self.index..end)
            .ok_or_else(|| "short JSON unicode escape".to_string())?;
        let text =
            std::str::from_utf8(bytes).map_err(|_| "invalid JSON unicode escape".to_string())?;
        self.index = end;
        u16::from_str_radix(text, 16).map_err(|_| "invalid JSON unicode escape".to_string())
    }

    fn parse_number(&mut self) -> Result<JsonValue, String> {
        let start = self.index;
        self.consume(b'-');
        let integer_start = self.index;
        if self.consume(b'0') {
            if self
                .bytes
                .get(self.index)
                .is_some_and(|byte| byte.is_ascii_digit())
            {
                return Err("leading zero in JSON number".to_string());
            }
        } else {
            while self
                .bytes
                .get(self.index)
                .is_some_and(|byte| byte.is_ascii_digit())
            {
                self.index += 1;
            }
            if self.index == integer_start {
                return Err("invalid JSON number".to_string());
            }
        }
        let mut integral = true;
        if self.consume(b'.') {
            integral = false;
            let fraction_start = self.index;
            while self
                .bytes
                .get(self.index)
                .is_some_and(|byte| byte.is_ascii_digit())
            {
                self.index += 1;
            }
            if self.index == fraction_start {
                return Err("JSON fraction requires digits".to_string());
            }
        }
        if self
            .bytes
            .get(self.index)
            .is_some_and(|byte| matches!(byte, b'e' | b'E'))
        {
            integral = false;
            self.index += 1;
            if self
                .bytes
                .get(self.index)
                .is_some_and(|byte| matches!(byte, b'+' | b'-'))
            {
                self.index += 1;
            }
            let exponent_start = self.index;
            while self
                .bytes
                .get(self.index)
                .is_some_and(|byte| byte.is_ascii_digit())
            {
                self.index += 1;
            }
            if self.index == exponent_start {
                return Err("JSON exponent requires digits".to_string());
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index])
            .map_err(|_| "invalid JSON number".to_string())?;
        if integral && let Ok(value) = text.parse::<i64>() {
            Ok(JsonValue::Number(value))
        } else {
            Ok(JsonValue::RawNumber(text.to_string()))
        }
    }

    fn expect_keyword(&mut self, keyword: &[u8]) -> Result<(), String> {
        if self.bytes.get(self.index..self.index + keyword.len()) == Some(keyword) {
            self.index += keyword.len();
            Ok(())
        } else {
            Err("invalid JSON keyword".to_string())
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        self.skip_whitespace();
        if self.consume(byte) {
            Ok(())
        } else {
            Err(format!("expected '{}' in JSON", byte as char))
        }
    }

    fn consume(&mut self, byte: u8) -> bool {
        if self.bytes.get(self.index) == Some(&byte) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn skip_whitespace(&mut self) {
        while self
            .bytes
            .get(self.index)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            self.index += 1;
        }
    }
}

fn json_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch < ' ' => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_handles_unicode_and_nested_values() {
        let input = r#"{"text":"hello 😀","values":[1,1.25e2,true,null,{"x":"\\n"}]}"#;
        let parsed = parse_json(input).expect("JSON should parse");
        let reparsed = parse_json(&parsed.to_json()).expect("serialized JSON should parse");
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn utf16_positions_account_for_non_bmp_characters() {
        let source = "😀value";
        assert_eq!(encoded_column(source, 4, PositionEncoding::Utf8), 4);
        assert_eq!(encoded_column(source, 4, PositionEncoding::Utf16), 2);
    }

    #[test]
    fn file_uri_round_trip_encodes_reserved_path_bytes() {
        let path = PathBuf::from("/tmp/flux project/100%#example.flux");
        let uri = file_uri_from_path(&path);
        assert!(uri.contains("flux%20project"));
        assert!(uri.contains("100%25%23example.flux"));
        assert_eq!(file_uri_path(&uri), Some(path));
    }

    #[test]
    fn formatting_returns_one_whole_document_edit() {
        let source = "fn main()->i64 {\n  return 0\n}\n";
        let edits =
            format_document(source, PositionEncoding::Utf8).expect("valid source should format");
        assert_eq!(edits.len(), 1);
        let json = edits[0].to_json();
        assert!(json.contains("fn main() -> i64"));
        assert!(json.contains("\\n    return 0\\n"));
    }

    #[test]
    fn code_actions_reuse_machine_applicable_compiler_fixes() {
        let source = "fn main() -> i64\n    return 0\n}\n";
        let documents = HashMap::from([("file:///tmp/fix.flux".to_string(), source.to_string())]);
        let actions = code_actions(
            "file:///tmp/fix.flux",
            source,
            &documents,
            PositionEncoding::Utf8,
        );
        assert_eq!(actions.len(), 1);
        let json = actions[0].to_json();
        assert!(json.contains("insert the function body opener"));
        assert!(json.contains("quickfix"));
        assert!(json.contains("newText"));
    }

    #[test]
    fn ui_hover_uses_contracts_even_when_builtin_view_source_is_incomplete() {
        let uri = "file:///tmp/ui-hover.flux";
        let source = "view Screen {\n    grid columns: 1fr\n    grid rows: auto\n    Text title at 1,1\n        selectable: true\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let element = hover_for_document(uri, source, &documents, 3, 5, PositionEncoding::Utf8)
            .expect("built-in element hover should survive incomplete source")
            .to_json();
        assert!(
            element.contains("element Text { text: str, variant: str, selectable: bool, size: i64")
        );
        assert!(element.contains("accessibilityLabel: str"));
        assert!(element.contains("accessibilityOrder: i64"));
        assert!(element.contains("onHover: fn() -> void"));
        assert!(element.contains("alignX: str"));
        assert!(element.contains("marginStart: i64"));
        assert!(element.contains("minHeight: i64"));

        let property = hover_for_document(uri, source, &documents, 4, 10, PositionEncoding::Utf8)
            .expect("built-in property hover should survive incomplete source")
            .to_json();
        assert!(property.contains("property Text.selectable: bool"));
    }

    #[test]
    fn ui_hover_describes_custom_view_parameters_when_source_is_valid() {
        let uri = "file:///tmp/custom-ui-hover.flux";
        let source = "view Badge(label: str) {\n    grid columns: 1fr\n    grid rows: auto\n    Text title at 1,1\n        text: label\n}\nview Screen {\n    grid columns: 1fr\n    grid rows: auto\n    Badge badge at 1,1\n        label: \"Flux\"\n}\nfn main() -> i64 { 0 }\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let element = hover_for_document(uri, source, &documents, 9, 5, PositionEncoding::Utf8)
            .expect("custom view kind should hover")
            .to_json();
        assert!(element.contains("view Badge(label: str)"));
        let property = hover_for_document(uri, source, &documents, 10, 10, PositionEncoding::Utf8)
            .expect("custom view property should hover")
            .to_json();
        assert!(property.contains("property Badge.label: str"));
    }

    #[test]
    fn hover_reports_declaration_and_unambiguous_usage_types() {
        let source =
            "fn double(value: i64) -> i64 { value * 2 }\nfn main() -> i64 { double(21) }\n";
        let uri = "file:///tmp/hover.flux";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let declaration = hover_for_document(uri, source, &documents, 0, 4, PositionEncoding::Utf8)
            .expect("function declaration should hover")
            .to_json();
        assert!(declaration.contains("fn double(value: i64) -> i64"));

        let usage = hover_for_document(uri, source, &documents, 1, 19, PositionEncoding::Utf8)
            .expect("unambiguous function usage should hover")
            .to_json();
        assert!(usage.contains("fn double(value: i64) -> i64"));
    }

    #[test]
    fn qualified_completion_survives_incomplete_enum_and_interface_members() {
        let uri = "file:///tmp/qualified-completion.flux";
        let source = "enum Outcome {\n    Ok(i64)\n    Failed(error)\n}\ninterface Storage {\n    fn load(path: str) -> (str, error)\n    fn save(path: str, data: str) -> error\n}\nfn main() -> i64 {\n    let result: Outcome = Outcome.\n    Storage.\n    process.\n    locale.\n    time.\n    fs.\n    android.\n    return 0\n}\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let enum_line = source
            .lines()
            .position(|line| line.contains("Outcome."))
            .expect("enum completion line should exist");
        let enum_source = source.lines().nth(enum_line).unwrap();
        let enum_items = JsonValue::Array(completion_items_at_cursor(
            uri,
            source,
            &documents,
            Some(enum_line),
            Some(enum_source.len()),
            PositionEncoding::Utf8,
        ))
        .to_json();
        assert!(enum_items.contains("\"label\":\"Ok\""));
        assert!(enum_items.contains("Outcome.Ok(i64) -> Outcome"));
        assert!(enum_items.contains("\"label\":\"Failed\""));

        let interface_line = source
            .lines()
            .position(|line| line.trim() == "Storage.")
            .expect("interface completion line should exist");
        let interface_source = source.lines().nth(interface_line).unwrap();
        let interface_items = JsonValue::Array(completion_items_at_cursor(
            uri,
            source,
            &documents,
            Some(interface_line),
            Some(interface_source.len()),
            PositionEncoding::Utf8,
        ))
        .to_json();
        assert!(interface_items.contains("\"label\":\"load\""));
        assert!(interface_items.contains("fn Storage.load(receiver: Storage, path: str)"));
        assert!(interface_items.contains("\"label\":\"save\""));

        let process_line = source
            .lines()
            .position(|line| line.trim() == "process.")
            .expect("process completion line should exist");
        let process_source = source.lines().nth(process_line).unwrap();
        let process_items = JsonValue::Array(completion_items_at_cursor(
            uri,
            source,
            &documents,
            Some(process_line),
            Some(process_source.len()),
            PositionEncoding::Utf8,
        ))
        .to_json();
        assert!(process_items.contains("fn process.pid() -> i64"));
        assert!(process_items.contains("fn process.parentPid() -> i64"));
        assert!(process_items.contains("fn process.terminationRequested() -> bool"));
        assert!(process_items.contains("fn process.exit(code: i64) -> void"));
        assert!(process_items.contains("fn process.hasEnv(name: str) -> bool"));
        assert!(process_items.contains("fn process.env(name: str, fallback: str) -> str"));

        let locale_line = source
            .lines()
            .position(|line| line.trim() == "locale.")
            .expect("locale completion line should exist");
        let locale_source = source.lines().nth(locale_line).unwrap();
        let locale_items = JsonValue::Array(completion_items_at_cursor(
            uri,
            source,
            &documents,
            Some(locale_line),
            Some(locale_source.len()),
            PositionEncoding::Utf8,
        ))
        .to_json();
        assert!(locale_items.contains("fn locale.language() -> str"));
        assert!(locale_items.contains("fn locale.region() -> str"));

        let time_line = source
            .lines()
            .position(|line| line.trim() == "time.")
            .expect("time completion line should exist");
        let time_source = source.lines().nth(time_line).unwrap();
        let time_items = JsonValue::Array(completion_items_at_cursor(
            uri,
            source,
            &documents,
            Some(time_line),
            Some(time_source.len()),
            PositionEncoding::Utf8,
        ))
        .to_json();
        assert!(time_items.contains("fn time.unixMillis() -> i64"));
        assert!(time_items.contains("fn time.monotonicMillis() -> i64"));
        assert!(time_items.contains("fn time.sleepMillis(durationMs: i64) -> void"));

        let fs_line = source
            .lines()
            .position(|line| line.trim() == "fs.")
            .expect("filesystem completion line should exist");
        let fs_source = source.lines().nth(fs_line).unwrap();
        let fs_items = JsonValue::Array(completion_items_at_cursor(
            uri,
            source,
            &documents,
            Some(fs_line),
            Some(fs_source.len()),
            PositionEncoding::Utf8,
        ))
        .to_json();
        assert!(fs_items.contains("fn fs.exists(path: str) -> bool"));
        assert!(fs_items.contains("fn fs.writeText(path: str, text: str) -> error"));
        assert!(fs_items.contains("fn fs.rename(source: str, destination: str) -> error"));
        assert!(fs_items.contains("fn fs.copyFile(source: str, destination: str) -> error"));

        let android_line = source
            .lines()
            .position(|line| line.trim() == "android.")
            .expect("Android completion line should exist");
        let android_source = source.lines().nth(android_line).unwrap();
        let android_items = JsonValue::Array(completion_items_at_cursor(
            uri,
            source,
            &documents,
            Some(android_line),
            Some(android_source.len()),
            PositionEncoding::Utf8,
        ))
        .to_json();
        assert!(android_items.contains("\"label\":\"vibrate\""));
        assert!(android_items.contains("fn android.sdkInt() -> i64"));
        assert!(android_items.contains("fn android.vibrate(durationMs: i64) -> void"));
        assert!(android_items.contains("\"label\":\"openUrl\""));
        assert!(android_items.contains("fn android.openUrl(url: str) -> void"));
        assert!(android_items.contains("fn android.share(text: str) -> void"));
        assert!(android_items.contains("fn android.showKeyboard() -> void"));
        assert!(android_items.contains("fn android.hideKeyboard() -> void"));
        assert!(android_items.contains("fn android.focusNext(wrap: bool = false) -> void"));
        assert!(android_items.contains("fn android.focusPrevious(wrap: bool = false) -> void"));
        assert!(android_items.contains("fn android.focusFirst() -> void"));
        assert!(android_items.contains("fn android.focusLast() -> void"));
        assert!(android_items.contains("fn android.clearFocus() -> void"));
        assert!(android_items.contains("fn android.selectionStart() -> i64"));
        assert!(android_items.contains("fn android.selectionEnd() -> i64"));
        assert!(android_items.contains("fn android.setCaret(position: i64) -> bool"));
        assert!(android_items.contains("fn android.setSelection(start: i64, end: i64) -> bool"));
        assert!(android_items.contains(
            "fn android.createNotificationChannel(id: str, name: str, description: str) -> void"
        ));
        assert!(android_items.contains("fn android.permissionGranted(permission: str) -> bool"));
        assert!(android_items.contains("fn android.requestPermission(permission: str) -> void"));
        assert!(android_items.contains("fn android.notificationPermissionGranted() -> bool"));
        assert!(android_items.contains("fn android.requestNotificationPermission() -> void"));
        assert!(android_items.contains(
            "fn android.notify(channelId: str, notificationId: i64, title: str, body: str) -> void"
        ));
        assert!(android_items.contains("fn android.notifyUrlAction(channelId: str, notificationId: i64, title: str, body: str, actionLabel: str, url: str) -> void"));
        assert!(
            android_items.contains("fn android.cancelNotification(notificationId: i64) -> void")
        );
    }

    #[test]
    fn struct_field_completion_uses_visible_typed_values_during_incomplete_edit() {
        let uri = "file:///tmp/struct-field-completion.flux";
        let source = "struct User {\n    name: str\n    age: i64\n}\ntype Person = User\nfn describe(user: User) -> i64 {\n    let person: Person = user\n    user.\n    person.\n    return 0\n}\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);

        for (line_index, value) in [(7usize, "user"), (8usize, "person")] {
            let line = source.lines().nth(line_index).unwrap();
            assert!(line.trim_start().starts_with(value));
            let items = JsonValue::Array(completion_items_at_cursor(
                uri,
                source,
                &documents,
                Some(line_index),
                Some(line.len()),
                PositionEncoding::Utf8,
            ))
            .to_json();
            assert!(items.contains("\"label\":\"name\""));
            assert!(items.contains("field User.name: str"));
            assert!(items.contains("\"label\":\"age\""));
            assert!(items.contains("field User.age: i64"));
        }
    }

    #[test]
    fn struct_field_completion_follows_nested_fields_and_function_results() {
        let uri = "file:///tmp/expression-member-completion.flux";
        let source = "struct Profile {\n    display_name: str\n    score: i64\n}\nstruct User {\n    profile: Profile\n}\nfn load_user() -> User {\n    return User { profile: Profile { display_name: \"Ada\", score: 42 } }\n}\nfn describe(user: User) -> i64 {\n    user.profile.\n    load_user().\n    return 0\n}\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);

        let nested_line = source
            .lines()
            .position(|line| line.trim() == "user.profile.")
            .expect("nested member line should exist");
        let nested_source = source.lines().nth(nested_line).unwrap();
        let nested = JsonValue::Array(completion_items_at_cursor(
            uri,
            source,
            &documents,
            Some(nested_line),
            Some(nested_source.len()),
            PositionEncoding::Utf8,
        ))
        .to_json();
        assert!(nested.contains("\"label\":\"display_name\""));
        assert!(nested.contains("field Profile.display_name: str"));
        assert!(nested.contains("\"label\":\"score\""));

        let call_line = source
            .lines()
            .position(|line| line.trim() == "load_user().")
            .expect("call-result member line should exist");
        let call_source = source.lines().nth(call_line).unwrap();
        let call = JsonValue::Array(completion_items_at_cursor(
            uri,
            source,
            &documents,
            Some(call_line),
            Some(call_source.len()),
            PositionEncoding::Utf8,
        ))
        .to_json();
        assert!(call.contains("\"label\":\"profile\""));
        assert!(call.contains("field User.profile: Profile"));
    }

    #[test]
    fn list_property_completion_and_hover_use_compiler_known_contracts() {
        let completion_uri = "file:///tmp/list-property-completion.flux";
        let completion_source = "type Numbers = i64[]\nfn main() -> i64 {\n    let values: Numbers = [1, 2, 3]\n    values.\n    return 0\n}\n";
        let completion_documents =
            HashMap::from([(completion_uri.to_string(), completion_source.to_string())]);
        let line_index = completion_source
            .lines()
            .position(|line| line.trim() == "values.")
            .expect("list completion line should exist");
        let line = completion_source.lines().nth(line_index).unwrap();
        let items = JsonValue::Array(completion_items_at_cursor(
            completion_uri,
            completion_source,
            &completion_documents,
            Some(line_index),
            Some(line.len()),
            PositionEncoding::Utf8,
        ))
        .to_json();
        assert!(items.contains("\"label\":\"length\""));
        assert!(items.contains("property i64[].length: i64"));
        assert!(items.contains("\"label\":\"isEmpty\""));
        assert!(items.contains("\"label\":\"isNotEmpty\""));
        assert!(items.contains("\"label\":\"first\""));
        assert!(items.contains("property i64[].first: i64"));
        assert!(items.contains("\"label\":\"last\""));
        assert!(items.contains("\"label\":\"single\""));

        let hover_uri = "file:///tmp/list-property-hover.flux";
        let hover_source = "fn main() -> i64 {\n    let values: i64[] = [1, 2, 3]\n    print(values.length)\n    return 0\n}\n";
        let hover_documents = HashMap::from([(hover_uri.to_string(), hover_source.to_string())]);
        let hover_line = hover_source
            .lines()
            .position(|line| line.contains("values.length"))
            .expect("list hover line should exist");
        let hover_text = hover_source.lines().nth(hover_line).unwrap();
        let character = hover_text.find("length").unwrap() + 2;
        let hover = hover_for_document(
            hover_uri,
            hover_source,
            &hover_documents,
            hover_line,
            character,
            PositionEncoding::Utf8,
        )
        .expect("list property should have hover")
        .to_json();
        assert!(hover.contains("property length: i64"));

        let first_source = "fn main() -> i64 {\n    let values: i64[] = [1, 2, 3]\n    print(values.first)\n    return 0\n}\n";
        let first_documents = HashMap::from([(hover_uri.to_string(), first_source.to_string())]);
        let first_line = first_source
            .lines()
            .position(|line| line.contains("values.first"))
            .expect("first hover line should exist");
        let first_text = first_source.lines().nth(first_line).unwrap();
        let first_character = first_text.find("first").unwrap() + 2;
        let first_hover = hover_for_document(
            hover_uri,
            first_source,
            &first_documents,
            first_line,
            first_character,
            PositionEncoding::Utf8,
        )
        .expect("list first property should have hover")
        .to_json();
        assert!(first_hover.contains("property first: i64"));
    }

    #[test]
    fn imported_qualified_completion_uses_forward_import_contracts_during_incomplete_edit() {
        let root =
            std::env::temp_dir().join(format!("flux-qualified-completion-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temporary completion project should be writable");
        let dependency = root.join("api.flux");
        let main = root.join("main.flux");
        std::fs::write(
            &dependency,
            "pub enum Outcome {\n    Ok(i64)\n    Failed(error)\n}\npub interface Storage {\n    fn load(path: str) -> (str, error)\n}\n",
        )
        .expect("completion dependency should be writable");
        let source = "import \"api.flux\"\nfn main() -> i64 {\n    Outcome.\n    Storage.\n    return 0\n}\n";
        std::fs::write(&main, source).expect("completion entry should be writable");
        let main = std::fs::canonicalize(main).unwrap();
        let uri = file_uri_from_path(&main);
        let documents = HashMap::from([(uri.clone(), source.to_string())]);

        let outcome_line = 2usize;
        let outcome_source = source.lines().nth(outcome_line).unwrap();
        let enum_items = JsonValue::Array(completion_items_at_cursor(
            &uri,
            source,
            &documents,
            Some(outcome_line),
            Some(outcome_source.len()),
            PositionEncoding::Utf8,
        ))
        .to_json();
        assert!(enum_items.contains("Outcome.Ok(i64) -> Outcome"));

        let storage_line = 3usize;
        let storage_source = source.lines().nth(storage_line).unwrap();
        let interface_items = JsonValue::Array(completion_items_at_cursor(
            &uri,
            source,
            &documents,
            Some(storage_line),
            Some(storage_source.len()),
            PositionEncoding::Utf8,
        ))
        .to_json();
        assert!(interface_items.contains("fn Storage.load(receiver: Storage, path: str)"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn text_input_completion_exposes_multiline_submit_policy() {
        let source = "view Form {\n    grid columns: 1fr\n    grid rows: auto\n    TextInput input at 1,1\n        \n}\nfn main() -> i64 { 0 }\n";
        let uri = "file:///tmp/text-input-completion.flux";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let properties = JsonValue::Array(completion_items_at_position(
            uri,
            source,
            &documents,
            Some(4),
        ))
        .to_json();
        assert!(properties.contains("\"label\":\"multiline\""));
        assert!(properties.contains("TextInput.multiline: bool"));
        assert!(properties.contains("\"label\":\"submitOnEnter\""));
        assert!(properties.contains("TextInput.submitOnEnter: bool"));
    }

    #[test]
    fn custom_view_completion_recovers_contract_before_malformed_current_view() {
        let uri = "file:///tmp/malformed-custom-view.flux";
        let source = "view Badge(label: str, count: i64 = 1) {\n    grid columns: 1fr\n    grid rows: auto\n    Text title at 1,1\n        text: label\n}\nview Screen {\n    grid columns: 1fr\n    grid rows: auto\n    Badge badge at 1,1\n        lab\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let line_index = source.lines().count().saturating_sub(1);
        let items = JsonValue::Array(completion_items_at_position(
            uri,
            source,
            &documents,
            Some(line_index),
        ))
        .to_json();
        assert!(items.contains("\"label\":\"label\""));
        assert!(items.contains("Badge.label: str"));
        assert!(items.contains("\"label\":\"count\""));
        assert!(items.contains("Badge.count: i64"));
    }

    #[test]
    fn completion_suggests_semantic_ui_color_values() {
        let source = "view Screen {\n    grid columns: 1fr\n    grid rows: auto\n    Text title at 1,1\n        text: \"Flux\"\n        color: \n}\n";
        let uri = "file:///tmp/ui-color-completion.flux";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let items = completion_items_at_position(uri, source, &documents, Some(5));
        for token in crate::typecheck::SEMANTIC_UI_COLOR_TOKENS {
            let expected = format!("\"{token}\"");
            assert!(
                items
                    .iter()
                    .any(|item| item.get("label").and_then(JsonValue::as_str)
                        == Some(expected.as_str())),
                "semantic color completion should include {token}"
            );
        }
        assert!(items.iter().any(|item| {
            item.get("detail").and_then(JsonValue::as_str) == Some("semantic Flux UI color")
        }));
    }

    #[test]
    fn completion_uses_compiler_view_contracts_inside_flat_ui_blocks() {
        let source = "view Badge(label: str) {\n    grid columns: 1fr\n    grid rows: auto\n    Text title at 1,1\n        text: label\n        \n}\nview Screen {\n    grid columns: 1fr\n    grid rows: auto\n    Badge badge at 1,1\n        \n}\nfn main() -> i64 { 0 }\n";
        let uri = "file:///tmp/ui-completion.flux";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let text_properties = JsonValue::Array(completion_items_at_position(
            uri,
            source,
            &documents,
            Some(5),
        ))
        .to_json();
        assert!(text_properties.contains("\"label\":\"selectable\""));
        assert!(text_properties.contains("Text.selectable: bool"));
        assert!(text_properties.contains("\"label\":\"translateX\""));
        assert!(text_properties.contains("Text.translateX: i64"));
        assert!(text_properties.contains("\"label\":\"scaleXPercent\""));
        assert!(text_properties.contains("Text.scaleXPercent: i64"));
        assert!(text_properties.contains("\"label\":\"fontFamily\""));
        assert!(text_properties.contains("Text.fontFamily: str"));
        assert!(text_properties.contains("\"label\":\"lineHeightPercent\""));
        assert!(text_properties.contains("Text.lineHeightPercent: i64"));
        assert!(text_properties.contains("\"label\":\"wrapMode\""));
        assert!(text_properties.contains("Text.wrapMode: str"));
        assert!(text_properties.contains("\"label\":\"paddingStart\""));
        assert!(text_properties.contains("Text.paddingStart: i64"));
        assert!(text_properties.contains("\"label\":\"borderStyle\""));
        assert!(text_properties.contains("Text.borderStyle: str"));
        assert!(text_properties.contains("\"label\":\"transitionEasing\""));
        assert!(text_properties.contains("Text.transitionEasing: str"));
        assert!(text_properties.contains("\"label\":\"windowWidth\""));
        assert!(text_properties.contains("read-only view environment windowWidth: i64"));
        assert!(text_properties.contains("\"label\":\"windowIsLandscape\""));
        assert!(text_properties.contains("read-only view environment windowIsLandscape: bool"));
        assert!(text_properties.contains("\"label\":\"displayScale\""));
        assert!(!text_properties.contains("\"label\":\"text\",\"kind\":10"));

        let custom_properties = JsonValue::Array(completion_items_at_position(
            uri,
            source,
            &documents,
            Some(11),
        ))
        .to_json();
        assert!(custom_properties.contains("\"label\":\"label\""));
        assert!(custom_properties.contains("Badge.label: str"));

        let layout = JsonValue::Array(completion_items_at_position(
            uri,
            source,
            &documents,
            Some(8),
        ))
        .to_json();
        assert!(layout.contains("\"label\":\"Button\""));
        assert!(layout.contains("\"label\":\"grid columns:\""));
        assert!(layout.contains("\"label\":\"state\""));
        assert!(layout.contains("\"label\":\"derived\""));
    }

    #[test]
    fn custom_view_contracts_survive_an_incomplete_later_view() {
        let uri = "file:///tmp/incomplete-ui-contract.flux";
        let source = "view Badge(label: str) {\n    grid columns: 1fr\n    grid rows: auto\n    Text title at 1,1\n        text: label\n}\nview Screen {\n    grid columns: 1fr\n    grid rows: auto\n    Badge badge at 1,1\n        \n        label: \"Flux\"\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let property_line = source
            .lines()
            .position(|line| line.contains("label: \"Flux\""))
            .expect("property line should exist");
        let blank_line = property_line - 1;

        let completion = JsonValue::Array(completion_items_at_position(
            uri,
            source,
            &documents,
            Some(blank_line),
        ))
        .to_json();
        assert!(completion.contains("Badge.label: str"));

        let hover = hover_for_document(
            uri,
            source,
            &documents,
            property_line,
            10,
            PositionEncoding::Utf8,
        )
        .expect("custom property hover should recover an earlier complete view")
        .to_json();
        assert!(hover.contains("property Badge.label: str"));

        let definition = definition_for_document(
            uri,
            source,
            &documents,
            property_line,
            10,
            PositionEncoding::Utf8,
        )
        .expect("custom property definition should recover an earlier complete view")
        .to_json();
        assert!(definition.contains("\"line\":0"));
        assert!(definition.contains("\"character\":11"));
    }

    #[test]
    fn imported_custom_view_contracts_survive_a_malformed_importer_with_overlays() {
        let root =
            std::env::temp_dir().join(format!("flux-incomplete-import-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temporary UI project should be writable");
        let dependency = root.join("badge.flux");
        let main = root.join("main.flux");
        std::fs::write(
            &dependency,
            "pub view Badge(label: str) {\n    grid columns: 1fr\n    grid rows: auto\n    Text title at 1,1\n        text: label\n}\n",
        )
        .expect("view dependency should be writable");
        let source = "import \"badge.flux\"\nview Screen {\n    grid columns: 1fr\n    grid rows: auto\n    Badge badge at 1,1\n        \n        label: true\n";
        std::fs::write(&main, source).expect("malformed importer should still exist on disk");
        let dependency = std::fs::canonicalize(dependency).unwrap();
        let main = std::fs::canonicalize(main).unwrap();
        let uri = file_uri_from_path(&main);
        let dependency_uri = file_uri_from_path(&dependency);
        let dependency_overlay = "pub view Badge(label: bool) {\n    grid columns: 1fr\n    grid rows: auto\n    Text title at 1,1\n        text: \"overlay\"\n}\n";
        let documents = HashMap::from([
            (uri.clone(), source.to_string()),
            (dependency_uri.clone(), dependency_overlay.to_string()),
        ]);
        let property_line = source
            .lines()
            .position(|line| line.contains("label: true"))
            .expect("property line should exist");
        let blank_line = property_line - 1;

        let completion = JsonValue::Array(completion_items_at_position(
            &uri,
            source,
            &documents,
            Some(blank_line),
        ))
        .to_json();
        assert!(completion.contains("Badge.label: bool"));

        let hover = hover_for_document(
            &uri,
            source,
            &documents,
            property_line,
            10,
            PositionEncoding::Utf8,
        )
        .expect("imported overlay property should hover through malformed importer")
        .to_json();
        assert!(hover.contains("property Badge.label: bool"));

        let definition = definition_for_document(
            &uri,
            source,
            &documents,
            property_line,
            10,
            PositionEncoding::Utf8,
        )
        .expect("imported overlay property should navigate through malformed importer")
        .to_json();
        assert!(definition.contains("badge.flux"));
        assert!(definition.contains("\"line\":0"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn completion_adds_only_lexically_visible_current_function_locals() {
        let uri = "file:///tmp/local-completion.flux";
        let source = "fn first(other: i64) -> i64 { other }\nfn compute(input: i64) -> i64 {\n    if input > 0:\n        let hidden: i64 = input\n        print(hidden)\n    let count: i64 = input\n    var total: i64 = count\n    return total\n}\nfn main() -> i64 { compute(1) }\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let items = completion_items_at_position(uri, source, &documents, Some(6));
        let json = JsonValue::Array(items).to_json();
        assert!(json.contains("\"label\":\"input\""));
        assert!(json.contains("parameter input: i64"));
        assert!(json.contains("\"label\":\"count\""));
        assert!(json.contains("let count: i64"));
        assert!(json.contains("\"label\":\"total\""));
        assert!(json.contains("var total: i64"));
        assert!(!json.contains("\"label\":\"hidden\""));
        assert!(!json.contains("\"label\":\"other\""));
    }

    #[test]
    fn signature_help_supports_builtins_enum_variants_and_interface_packing() {
        let uri = "file:///tmp/call-shapes.flux";
        let source = "enum Outcome {\n    Ok(i64, str)\n}\ninterface Readable {\n    fn read() -> str\n}\nstruct Memory {\n    value: str\n}\nfn memory_read(memory: Memory) -> str { memory.value }\nfn add(left: i64, right: i64) -> i64 { left + right }\nimpl Readable for Memory {\n    read: memory_read\n}\nfn main() -> i64 {\n    let _outcome: Outcome = Outcome.Ok(42, \"Flux\")\n    let memory: Memory = Memory { value: \"x\" }\n    let _readable: Readable = Readable(memory)\n    let values: i64[] = [1, 2]\n    let _folded: i64 = fold(values, 0, add)\n    let _reduced: i64 = reduce(values, add)\n    let checks: bool[] = [true, false]\n    let _has: bool = any(checks)\n    print(error(\"boom\"))\n    return 0\n}\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let help_for = |needle: &str| {
            let line_index = source
                .lines()
                .position(|line| line.contains(needle))
                .expect("call line should exist");
            let line = source.lines().nth(line_index).unwrap();
            let cursor = if needle == "error(" {
                line.find("error(").unwrap() + "error(\"boom\"".len()
            } else if needle == "any(" {
                line.find("any(").unwrap() + "any(".len()
            } else if needle == "fold(" {
                line.find("fold(").unwrap() + "fold(values, 0, ".len()
            } else if needle == "reduce(" {
                line.find("reduce(").unwrap() + "reduce(values, ".len()
            } else {
                line.len().saturating_sub(1)
            };
            signature_help_for_document(
                uri,
                source,
                &documents,
                line_index,
                cursor,
                PositionEncoding::Utf8,
            )
            .expect("call should have signature help")
            .to_json()
        };

        let enum_help = help_for("Outcome.Ok");
        assert!(enum_help.contains("fn Outcome.Ok(i64, str) -> Outcome"));
        let pack_help = help_for("Readable(memory");
        assert!(pack_help.contains("fn Readable(value: implementing concrete value) -> Readable"));
        let print_help = help_for("print(");
        assert!(print_help.contains("fn print(value: i64 | bool | str | error) -> void"));
        let error_help = help_for("error(");
        assert!(error_help.contains("fn error(message: str) -> error"));
        let any_help = help_for("any(");
        assert!(any_help.contains("fn any(list: bool[]) -> bool"));
        let fold_help = help_for("fold(");
        assert!(fold_help.contains("fn fold(list: T[], initial: A, reducer: fn(A, T) -> A) -> A"));
        let reduce_help = help_for("reduce(");
        assert!(reduce_help.contains("fn reduce(list: T[], reducer: fn(T, T) -> T) -> T"));
    }

    #[test]
    fn signature_help_supports_process_capabilities() {
        let uri = "file:///tmp/process-signatures.flux";
        let source = "fn main() -> i64 {\n    print(process.pid())\n    print(process.parentPid())\n    print(process.terminationRequested())\n    process.exit(0)\n    print(process.hasEnv(\"HOME\"))\n    print(process.env(\"HOME\", \"missing\"))\n    return 0\n}\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        for (needle, expected) in [
            ("process.pid(", "fn process.pid() -> i64"),
            ("process.parentPid(", "fn process.parentPid() -> i64"),
            (
                "process.terminationRequested(",
                "fn process.terminationRequested() -> bool",
            ),
            ("process.exit(", "fn process.exit(code: i64) -> void"),
            ("process.hasEnv(", "fn process.hasEnv(name: str) -> bool"),
            (
                "process.env(",
                "fn process.env(name: str, fallback: str) -> str",
            ),
        ] {
            let line_index = source
                .lines()
                .position(|line| line.contains(needle))
                .expect("process call line should exist");
            let line = source.lines().nth(line_index).unwrap();
            let cursor = line.find(needle).unwrap() + needle.len();
            let help = signature_help_for_document(
                uri,
                source,
                &documents,
                line_index,
                cursor,
                PositionEncoding::Utf8,
            )
            .expect("process call should have signature help")
            .to_json();
            assert!(help.contains(expected));
        }
    }

    #[test]
    fn signature_help_supports_locale_capabilities() {
        let uri = "file:///tmp/locale-signatures.flux";
        let source = "fn main() -> i64 {\n    print(locale.language())\n    print(locale.region())\n    return 0\n}\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        for (needle, expected) in [
            ("locale.language(", "fn locale.language() -> str"),
            ("locale.region(", "fn locale.region() -> str"),
        ] {
            let line_index = source
                .lines()
                .position(|line| line.contains(needle))
                .expect("locale call line should exist");
            let line = source.lines().nth(line_index).unwrap();
            let cursor = line.find(needle).unwrap() + needle.len();
            let help = signature_help_for_document(
                uri,
                source,
                &documents,
                line_index,
                cursor,
                PositionEncoding::Utf8,
            )
            .expect("locale call should have signature help")
            .to_json();
            assert!(help.contains(expected));
        }
    }

    #[test]
    fn signature_help_supports_time_capabilities() {
        let uri = "file:///tmp/time-signatures.flux";
        let source = "fn main() -> i64 {\n    print(time.unixMillis())\n    print(time.monotonicMillis())\n    time.sleepMillis(10)\n    return 0\n}\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        for (needle, expected) in [
            ("time.unixMillis(", "fn time.unixMillis() -> i64"),
            ("time.monotonicMillis(", "fn time.monotonicMillis() -> i64"),
            (
                "time.sleepMillis(",
                "fn time.sleepMillis(durationMs: i64) -> void",
            ),
        ] {
            let line_index = source
                .lines()
                .position(|line| line.contains(needle))
                .expect("time call line should exist");
            let line = source.lines().nth(line_index).unwrap();
            let cursor = line.find(needle).unwrap() + needle.len();
            let help = signature_help_for_document(
                uri,
                source,
                &documents,
                line_index,
                cursor,
                PositionEncoding::Utf8,
            )
            .expect("time call should have signature help")
            .to_json();
            assert!(help.contains(expected));
        }
    }

    #[test]
    fn signature_help_supports_filesystem_capabilities() {
        let uri = "file:///tmp/filesystem-signatures.flux";
        let source = "fn main() -> i64 {\n    print(fs.exists(\"a\"))\n    print(fs.isFile(\"a\"))\n    print(fs.isDirectory(\"a\"))\n    print(fs.createDirectory(\"a\"))\n    print(fs.removeFile(\"a\"))\n    print(fs.removeDirectory(\"a\"))\n    print(fs.writeText(\"a\", \"x\"))\n    print(fs.appendText(\"a\", \"x\"))\n    print(fs.rename(\"a\", \"b\"))\n    print(fs.copyFile(\"a\", \"b\"))\n    return 0\n}\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        for (needle, expected) in [
            ("fs.exists(", "fn fs.exists(path: str) -> bool"),
            ("fs.isFile(", "fn fs.isFile(path: str) -> bool"),
            ("fs.isDirectory(", "fn fs.isDirectory(path: str) -> bool"),
            (
                "fs.createDirectory(",
                "fn fs.createDirectory(path: str) -> error",
            ),
            ("fs.removeFile(", "fn fs.removeFile(path: str) -> error"),
            (
                "fs.removeDirectory(",
                "fn fs.removeDirectory(path: str) -> error",
            ),
            (
                "fs.writeText(",
                "fn fs.writeText(path: str, text: str) -> error",
            ),
            (
                "fs.appendText(",
                "fn fs.appendText(path: str, text: str) -> error",
            ),
            (
                "fs.rename(",
                "fn fs.rename(source: str, destination: str) -> error",
            ),
            (
                "fs.copyFile(",
                "fn fs.copyFile(source: str, destination: str) -> error",
            ),
        ] {
            let line_index = source
                .lines()
                .position(|line| line.contains(needle))
                .expect("filesystem call line should exist");
            let line = source.lines().nth(line_index).unwrap();
            let cursor = line.find(needle).unwrap() + needle.len();
            let help = signature_help_for_document(
                uri,
                source,
                &documents,
                line_index,
                cursor,
                PositionEncoding::Utf8,
            )
            .expect("filesystem call should have signature help")
            .to_json();
            assert!(help.contains(expected));
        }
    }

    #[test]
    fn signature_help_supports_android_platform_calls() {
        let uri = "file:///tmp/android-platform-signatures.flux";
        let source = "fn main() -> i64 {\n    print(android.sdkInt())\n    android.vibrate(25)\n    android.openUrl(\"https://example.com\")\n    android.share(\"hello\")\n    print(android.permissionGranted(\"android.permission.CAMERA\"))\n    android.requestPermission(\"android.permission.CAMERA\")\n    android.createNotificationChannel(\"updates\", \"Updates\", \"Flux updates\")\n    print(android.notificationPermissionGranted())\n    android.requestNotificationPermission()\n    android.notify(\"updates\", 1, \"Hello\", \"from Flux\")\n    android.notifyUrlAction(\"updates\", 2, \"Hello\", \"Open site\", \"Open\", \"https://example.com\")\n    android.cancelNotification(1)\n    return 0\n}\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        for (needle, expected) in [
            ("android.sdkInt(", "fn android.sdkInt() -> i64"),
            (
                "android.vibrate(",
                "fn android.vibrate(durationMs: i64) -> void",
            ),
            ("android.openUrl(", "fn android.openUrl(url: str) -> void"),
            ("android.share(", "fn android.share(text: str) -> void"),
            (
                "android.permissionGranted(",
                "fn android.permissionGranted(permission: str) -> bool",
            ),
            (
                "android.requestPermission(",
                "fn android.requestPermission(permission: str) -> void",
            ),
            (
                "android.createNotificationChannel(",
                "fn android.createNotificationChannel(id: str, name: str, description: str) -> void",
            ),
            (
                "android.notificationPermissionGranted(",
                "fn android.notificationPermissionGranted() -> bool",
            ),
            (
                "android.requestNotificationPermission(",
                "fn android.requestNotificationPermission() -> void",
            ),
            (
                "android.notify(",
                "fn android.notify(channelId: str, notificationId: i64, title: str, body: str) -> void",
            ),
            (
                "android.notifyUrlAction(",
                "fn android.notifyUrlAction(channelId: str, notificationId: i64, title: str, body: str, actionLabel: str, url: str) -> void",
            ),
            (
                "android.cancelNotification(",
                "fn android.cancelNotification(notificationId: i64) -> void",
            ),
        ] {
            let line_index = source
                .lines()
                .position(|line| line.contains(needle))
                .expect("platform call line should exist");
            let line = source.lines().nth(line_index).unwrap();
            let cursor = line.find(needle).unwrap() + needle.len();
            let help = signature_help_for_document(
                uri,
                source,
                &documents,
                line_index,
                cursor,
                PositionEncoding::Utf8,
            )
            .expect("platform call should have signature help")
            .to_json();
            assert!(help.contains(expected));
        }
    }

    #[test]
    fn signature_help_supports_android_focus_and_selection() {
        let uri = "file:///tmp/android-focus-signatures.flux";
        let source = "fn main() -> i64 {\n    android.focusNext()\n    android.focusPrevious()\n    android.focusFirst()\n    android.focusLast()\n    android.clearFocus()\n    print(android.selectionStart())\n    print(android.selectionEnd())\n    print(android.setCaret(1))\n    print(android.setSelection(0, 1))\n    return 0\n}\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        for (needle, expected) in [
            (
                "android.focusNext(",
                "fn android.focusNext(wrap: bool = false) -> void",
            ),
            (
                "android.focusPrevious(",
                "fn android.focusPrevious(wrap: bool = false) -> void",
            ),
            ("android.focusFirst(", "fn android.focusFirst() -> void"),
            ("android.focusLast(", "fn android.focusLast() -> void"),
            ("android.clearFocus(", "fn android.clearFocus() -> void"),
            (
                "android.selectionStart(",
                "fn android.selectionStart() -> i64",
            ),
            ("android.selectionEnd(", "fn android.selectionEnd() -> i64"),
            (
                "android.setCaret(",
                "fn android.setCaret(position: i64) -> bool",
            ),
            (
                "android.setSelection(",
                "fn android.setSelection(start: i64, end: i64) -> bool",
            ),
        ] {
            let line_index = source
                .lines()
                .position(|line| line.contains(needle))
                .expect("Android input call line should exist");
            let line = source.lines().nth(line_index).unwrap();
            let cursor = line.find(needle).unwrap() + needle.len();
            let help = signature_help_for_document(
                uri,
                source,
                &documents,
                line_index,
                cursor,
                PositionEncoding::Utf8,
            )
            .expect("Android input call should have signature help")
            .to_json();
            assert!(help.contains(expected));
        }
    }

    #[test]
    fn signature_help_supports_sequence_transforms() {
        let uri = "file:///tmp/sequence-transform-signatures.flux";
        let source = "fn double(value: i64) -> i64 { value * 2 }\nfn keep(value: i64) -> bool { value > 0 }\nfn main() -> i64 {\n    let values: i64[] = [1, 2]\n    let other: i64[] = [3, 4]\n    let nested: i64[][] = [values, other]\n    let _mapped: i64[] = map(values, double)\n    let _filtered: i64[] = filter(values, keep)\n    let _selected: i64[] = where(values, keep)\n    let _joined: i64[] = concat(values, other)\n    let _unique: i64[] = distinct(values)\n    let _flat: i64[] = flatten(nested)\n    let _sorted: i64[] = sorted(values)\n    let _chunks: i64[][] = chunked(values, 1)\n    return 0\n}\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let help_for = |needle: &str| {
            let line_index = source
                .lines()
                .position(|line| line.contains(needle))
                .expect("sequence transform call line should exist");
            let line = source.lines().nth(line_index).unwrap();
            let cursor = line.find(needle).unwrap() + needle.len();
            signature_help_for_document(
                uri,
                source,
                &documents,
                line_index,
                cursor,
                PositionEncoding::Utf8,
            )
            .expect("sequence transform should have signature help")
            .to_json()
        };

        let map_help = help_for("map(values, ");
        assert!(map_help.contains("fn map(list: T[], callback: fn(T) -> U) -> U[]"));
        let filter_help = help_for("filter(values, ");
        assert!(filter_help.contains("fn filter(list: T[], predicate: fn(T) -> bool) -> T[]"));
        let where_help = help_for("where(values, ");
        assert!(where_help.contains("fn where(list: T[], predicate: fn(T) -> bool) -> T[]"));
        let concat_help = help_for("concat(values, ");
        assert!(concat_help.contains("fn concat(left: T[], right: T[]) -> T[]"));
        let distinct_help = help_for("distinct(");
        assert!(distinct_help.contains("fn distinct(list: scalar[]) -> same scalar list type"));
        let flatten_help = help_for("flatten(");
        assert!(flatten_help.contains("fn flatten(list: T[][]) -> T[]"));
        let sorted_help = help_for("sorted(");
        assert!(sorted_help.contains("fn sorted(list: ordered[]) -> same ordered list type"));
        let chunked_help = help_for("chunked(values, ");
        assert!(chunked_help.contains("fn chunked(list: T[], size: i64) -> T[][]"));
    }

    #[test]
    fn local_hover_and_definition_resolve_same_named_parameters_by_function_scope() {
        let uri = "file:///tmp/local-shadow-navigation.flux";
        let source = "fn first(value: i64) -> i64 {\n    return value\n}\nfn second(value: str) -> str {\n    return value\n}\nfn main() -> i64 { 0 }\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let line_index = 4usize;
        let line = source.lines().nth(line_index).unwrap();
        let character = line.find("value").expect("usage should exist") + 1;

        let hover = hover_for_document(
            uri,
            source,
            &documents,
            line_index,
            character,
            PositionEncoding::Utf8,
        )
        .expect("same-named parameter usage should resolve in its function")
        .to_json();
        assert!(hover.contains("value: str"));

        let definition = definition_for_document(
            uri,
            source,
            &documents,
            line_index,
            character,
            PositionEncoding::Utf8,
        )
        .expect("same-named parameter usage should navigate in its function")
        .to_json();
        assert!(definition.contains("\"line\":3"));
        assert!(definition.contains("\"character\":10"));
    }

    #[test]
    fn struct_member_hover_and_definition_use_receiver_type() {
        let uri = "file:///tmp/struct-member-navigation.flux";
        let source = "struct Profile {\n    name: str\n}\nstruct Other {\n    name: i64\n}\nstruct User {\n    profile: Profile\n}\nfn describe(user: User) -> str {\n    return user.profile.name\n}\nfn main() -> i64 { 0 }\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let line_index = source
            .lines()
            .position(|line| line.contains("user.profile.name"))
            .expect("member usage line should exist");
        let line = source.lines().nth(line_index).unwrap();
        let character = line.find("name").expect("member name should exist") + 1;
        let database = crate::semantic::SemanticDatabase::analyze(source, source_id_for_uri(uri))
            .expect("member navigation source should analyze");
        assert!(
            struct_field_for_position(
                source,
                line_index,
                character,
                PositionEncoding::Utf8,
                database.program(),
            )
            .is_some(),
            "receiver-aware field lookup should resolve the nested field"
        );

        let hover = hover_for_document(
            uri,
            source,
            &documents,
            line_index,
            character,
            PositionEncoding::Utf8,
        )
        .expect("typed struct member should hover")
        .to_json();
        assert!(hover.contains("field name: str"));

        let definition = definition_for_document(
            uri,
            source,
            &documents,
            line_index,
            character,
            PositionEncoding::Utf8,
        )
        .expect("typed struct member should navigate")
        .to_json();
        assert!(definition.contains("\"line\":1"));
        assert!(definition.contains("\"character\":4"));
    }

    #[test]
    fn signature_help_supports_first_class_function_values() {
        let uri = "file:///tmp/function-value-signature.flux";
        let source = "type Mapper = fn(i64, str) -> bool\nfn apply(transform: Mapper) -> bool {\n    return transform(42, \"Flux\")\n}\nfn always_true(value: i64, label: str) -> bool {\n    print(value)\n    print(label)\n    return true\n}\nfn main() -> i64 {\n    let result: bool = apply(always_true)\n    print(result)\n    return 0\n}\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let call_line_index = source
            .lines()
            .position(|line| line.contains("transform(42"))
            .expect("function-value call line should exist");
        let call_line = source.lines().nth(call_line_index).unwrap();
        let help = signature_help_for_document(
            uri,
            source,
            &documents,
            call_line_index,
            call_line.len().saturating_sub(1),
            PositionEncoding::Utf8,
        )
        .expect("first-class function call should have signature help")
        .to_json();
        assert!(help.contains("fn transform(i64, str) -> bool"));
        assert!(help.contains("\"label\":\"i64\""));
        assert!(help.contains("\"label\":\"str\""));
        assert!(help.contains("\"activeParameter\":1"));
    }

    #[test]
    fn signature_help_supports_interface_capability_call_shape() {
        let uri = "file:///tmp/interface-signature.flux";
        let source = "interface Storage {\n    fn load(path: str) -> (str, error)\n}\nstruct Memory {\n    value: str\n}\nfn memory_load(storage: Memory, path: str) -> (str, error) {\n    print(storage.value)\n    return path, nil\n}\nimpl Storage for Memory {\n    load: memory_load\n}\nfn main() -> i64 {\n    let memory: Memory = Memory { value: \"x\" }\n    let storage: Storage = Storage(memory)\n    let data: str, err: error = Storage.load(storage, \"settings\")\n    print(data)\n    print(err)\n    return 0\n}\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let call_line_index = source
            .lines()
            .position(|line| line.contains("Storage.load"))
            .expect("capability call line should exist");
        let call_line = source.lines().nth(call_line_index).unwrap();
        let help = signature_help_for_document(
            uri,
            source,
            &documents,
            call_line_index,
            call_line.len().saturating_sub(1),
            PositionEncoding::Utf8,
        )
        .expect("interface capability call should have signature help")
        .to_json();
        assert!(
            help.contains("fn Storage.load(receiver: Storage, path: str) -&gt; (str, error)")
                || help.contains("fn Storage.load(receiver: Storage, path: str) -> (str, error)")
        );
        assert!(help.contains("\"label\":\"receiver: Storage\""));
        assert!(help.contains("\"label\":\"path: str\""));
        assert!(help.contains("\"activeParameter\":1"));
    }

    #[test]
    fn signature_help_tracks_the_active_argument() {
        let source = "fn add(left: i64, right: i64) -> i64 { left + right }\nfn main() -> i64 { add(1, 2) }\n";
        let uri = "file:///tmp/signature.flux";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let help =
            signature_help_for_document(uri, source, &documents, 1, 25, PositionEncoding::Utf8)
                .expect("call should have signature help")
                .to_json();
        assert!(help.contains("fn add(left: i64, right: i64) -> i64"));
        assert!(help.contains("\"activeParameter\":1"));
        assert!(help.contains("\"label\":\"left: i64\""));
    }

    #[test]
    fn custom_view_property_definition_targets_the_declared_parameter() {
        let uri = "file:///tmp/ui-definition.flux";
        let source = "view Badge(label: str) {\n    grid columns: 1fr\n    grid rows: auto\n    Text title at 1,1\n        text: label\n}\nview Screen {\n    grid columns: 1fr\n    grid rows: auto\n    Badge badge at 1,1\n        label: \"Flux\"\n}\nfn main() -> i64 { 0 }\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let definition =
            definition_for_document(uri, source, &documents, 10, 10, PositionEncoding::Utf8)
                .expect("composed view property should resolve to its parameter")
                .to_json();
        assert!(definition.contains("\"line\":0"));
        assert!(definition.contains("\"character\":11"));
    }

    #[test]
    fn imported_custom_view_property_definition_targets_dependency_parameter() {
        let root = std::env::temp_dir().join(format!("flux-ui-definition-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temporary UI project should be writable");
        let dependency = root.join("badge.flux");
        let main = root.join("main.flux");
        std::fs::write(
            &dependency,
            "pub view Badge(label: str) {\n    grid columns: 1fr\n    grid rows: auto\n    Text title at 1,1\n        text: label\n}\n",
        )
        .expect("view dependency should be writable");
        let source = "import \"badge.flux\"\nview Screen {\n    grid columns: 1fr\n    grid rows: auto\n    Badge badge at 1,1\n        label: \"Flux\"\n}\nfn main() -> i64 { 0 }\n";
        std::fs::write(&main, source).expect("entry should be writable");
        let main = std::fs::canonicalize(main).unwrap();
        let uri = format!("file://{}", main.display());
        let documents = HashMap::from([(uri.clone(), source.to_string())]);
        let property_line = source
            .lines()
            .position(|line| line.contains("label: \"Flux\""))
            .expect("property line should exist");
        let definition = definition_for_document(
            &uri,
            source,
            &documents,
            property_line,
            10,
            PositionEncoding::Utf8,
        )
        .expect("imported composed-view property should resolve")
        .to_json();
        assert!(definition.contains("badge.flux"));
        assert!(definition.contains("\"line\":0"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn navigation_and_rename_are_safe_for_unambiguous_symbols() {
        let uri = "file:///tmp/navigation.flux";
        let source =
            "fn double(value: i64) -> i64 { value * 2 }\nfn main() -> i64 { double(21) }\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let definition =
            definition_for_document(uri, source, &documents, 1, 19, PositionEncoding::Utf8)
                .expect("function usage should resolve")
                .to_json();
        assert!(definition.contains("\"line\":0"));
        assert!(definition.contains("\"character\":3"));

        let references =
            references_for_document(uri, source, &documents, 1, 19, PositionEncoding::Utf8);
        assert_eq!(references.len(), 2);

        let rename = rename_for_document(
            uri,
            source,
            &documents,
            1,
            19,
            "twice",
            PositionEncoding::Utf8,
        )
        .expect("safe rename should produce edits")
        .to_json();
        assert_eq!(rename.matches("newText").count(), 2);
        assert!(rename.contains("twice"));
        assert!(
            rename_for_document(uri, source, &documents, 1, 19, "fn", PositionEncoding::Utf8,)
                .is_none()
        );
    }

    #[test]
    fn local_references_and_rename_follow_function_scope() {
        let uri = "file:///tmp/local-references.flux";
        let source = "fn first(value: i64) -> i64 {\n    let item: i64 = value\n    return item\n}\nfn second(value: i64) -> i64 {\n    let item: i64 = value + 1\n    return item\n}\nfn main() -> i64 { 0 }\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let line = 2usize;
        let character = source
            .lines()
            .nth(line)
            .and_then(|line| line.find("item"))
            .expect("local usage should exist")
            + 1;

        let database =
            analyzed_document(uri, source).expect("standalone local source should analyze");
        let target = visible_local_symbol_for_position(
            &database,
            source,
            source_id_for_uri(uri),
            line + 1,
            "item",
        )
        .expect("local binding should resolve at its usage");
        assert!(is_local_symbol_kind(target.kind));
        assert_eq!(target.span.line, 2);
        assert_eq!(
            local_symbol_occurrences(source, source_id_for_uri(uri), target, &database).len(),
            2
        );

        let references = references_for_document(
            uri,
            source,
            &documents,
            line,
            character,
            PositionEncoding::Utf8,
        );
        assert_eq!(references.len(), 2);
        let references_json = JsonValue::Array(references).to_json();
        assert!(references_json.contains("\"line\":1"));
        assert!(references_json.contains("\"line\":2"));
        assert!(!references_json.contains("\"line\":5"));
        assert!(!references_json.contains("\"line\":6"));

        let rename = rename_for_document(
            uri,
            source,
            &documents,
            line,
            character,
            "result",
            PositionEncoding::Utf8,
        )
        .expect("local rename should stay inside the resolved function scope")
        .to_json();
        assert_eq!(rename.matches("newText").count(), 2);
        assert!(rename.contains("result"));
        assert!(rename.contains("\"line\":1"));
        assert!(rename.contains("\"line\":2"));
        assert!(!rename.contains("\"line\":5"));
        assert!(!rename.contains("\"line\":6"));

        assert!(
            rename_for_document(
                uri,
                source,
                &documents,
                line,
                character,
                "value",
                PositionEncoding::Utf8,
            )
            .is_none(),
            "renaming a local onto another symbol in the same function must be rejected"
        );
    }

    #[test]
    fn navigation_refuses_ambiguous_shadowed_names() {
        let uri = "file:///tmp/ambiguous.flux";
        let source =
            "fn first(value: i64) -> i64 { value }\nfn second(value: i64) -> i64 { value }\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        assert!(
            definition_for_document(uri, source, &documents, 0, 31, PositionEncoding::Utf8,)
                .is_none()
        );
        assert!(
            references_for_document(uri, source, &documents, 0, 31, PositionEncoding::Utf8,)
                .is_empty()
        );
        assert!(
            rename_for_document(
                uri,
                source,
                &documents,
                0,
                31,
                "item",
                PositionEncoding::Utf8,
            )
            .is_none()
        );
    }

    #[test]
    fn completion_includes_keywords_builtins_and_buffer_declarations() {
        let source = "type Count = i64\nfn main() -> i64 { 0 }\n";
        let json = JsonValue::Array(completion_items(source)).to_json();
        assert!(json.contains("\"label\":\"while\""));
        assert!(json.contains("\"label\":\"i64\""));
        assert!(json.contains("\"label\":\"Count\""));
        assert!(json.contains("\"label\":\"main\""));
        assert!(json.contains("fn main() -> i64"));
        assert!(json.contains("\"label\":\"take\""));
        assert!(json.contains("fn take(list: T[], count: i64) -> T[]"));
        assert!(json.contains("\"label\":\"skip\""));
        assert!(!json.contains("\"label\":\"firstOrDefault\""));
        assert!(!json.contains("\"label\":\"lastOrDefault\""));
        assert!(json.contains("\"label\":\"any\""));
        assert!(json.contains("fn any(list: bool[]) -> bool"));
        assert!(json.contains("\"label\":\"every\""));
        assert!(json.contains("\"label\":\"fold\""));
        assert!(json.contains("fn fold(list: T[], initial: A, reducer: fn(A, T) -> A) -> A"));
        assert!(json.contains("\"label\":\"reduce\""));
        assert!(json.contains("fn reduce(list: T[], reducer: fn(T, T) -> T) -> T"));
        assert!(json.contains("\"label\":\"map\""));
        assert!(json.contains("fn map(list: T[], callback: fn(T) -> U) -> U[]"));
        assert!(json.contains("\"label\":\"filter\""));
        assert!(json.contains("fn filter(list: T[], predicate: fn(T) -> bool) -> T[]"));
        assert!(json.contains("\"label\":\"where\""));
        assert!(json.contains("\"label\":\"concat\""));
        assert!(json.contains("fn concat(left: T[], right: T[]) -> T[]"));
        assert!(json.contains("\"label\":\"distinct\""));
        assert!(json.contains("fn distinct(list: scalar[]) -> scalar[]"));
        assert!(json.contains("\"label\":\"flatten\""));
        assert!(json.contains("fn flatten(list: T[][]) -> T[]"));
        assert!(json.contains("\"label\":\"sorted\""));
        assert!(json.contains("fn sorted(list: ordered[]) -> same ordered list type"));
        assert!(json.contains("\"label\":\"chunked\""));
        assert!(json.contains("fn chunked(list: T[], size: i64) -> T[][]"));
    }

    #[test]
    fn go_to_definition_resolves_imported_public_symbols() {
        let root = std::env::temp_dir().join(format!("flux-lsp-definition-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temporary LSP project should be writable");
        let dependency = root.join("dep.flux");
        let main = root.join("main.flux");
        std::fs::write(&dependency, "pub fn value() -> i64 { 42 }\n")
            .expect("dependency should be writable");
        let main_source = "import \"dep.flux\"\nfn main() -> i64 { value() }\n";
        std::fs::write(&main, main_source).expect("entry should be writable");
        let main = std::fs::canonicalize(main).unwrap();
        let main_uri = format!("file://{}", main.display());
        let documents = HashMap::from([(main_uri.clone(), main_source.to_string())]);
        let definition = definition_for_document(
            &main_uri,
            main_source,
            &documents,
            1,
            20,
            PositionEncoding::Utf8,
        )
        .expect("imported public function should resolve")
        .to_json();
        assert!(definition.contains("dep.flux"));
        assert!(definition.contains("\"line\":0"));
        assert!(definition.contains("\"character\":7"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn imported_references_and_rename_cover_the_loaded_project_graph() {
        let root =
            std::env::temp_dir().join(format!("flux-lsp-project-rename-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temporary LSP project should be writable");
        let dependency = root.join("dep.flux");
        let main = root.join("main.flux");
        std::fs::write(&dependency, "pub fn value() -> i64 { 42 }\n")
            .expect("dependency should be writable");
        let main_source = "import \"dep.flux\"\nfn main() -> i64 { value() }\n";
        std::fs::write(&main, main_source).expect("entry should be writable");
        let main = std::fs::canonicalize(main).unwrap();
        let main_uri = format!("file://{}", main.display());
        let documents = HashMap::from([(main_uri.clone(), main_source.to_string())]);

        let references = references_for_document(
            &main_uri,
            main_source,
            &documents,
            1,
            20,
            PositionEncoding::Utf8,
        );
        assert_eq!(references.len(), 2);
        let references_json = JsonValue::Array(references).to_json();
        assert!(references_json.contains("dep.flux"));
        assert!(references_json.contains("main.flux"));

        let rename = rename_for_document(
            &main_uri,
            main_source,
            &documents,
            1,
            20,
            "answer",
            PositionEncoding::Utf8,
        )
        .expect("imported function should rename within its loaded project graph")
        .to_json();
        assert_eq!(rename.matches("newText").count(), 2);
        assert!(rename.contains("dep.flux"));
        assert!(rename.contains("main.flux"));
        assert!(rename.contains("answer"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_analysis_cache_is_used_by_repeated_lsp_queries() {
        let root = std::env::temp_dir().join(format!("flux-lsp-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temporary LSP cache project should be writable");
        let dependency = root.join("dep.flux");
        let main = root.join("main.flux");
        std::fs::write(&dependency, "pub fn value() -> i64 { 1 }\n")
            .expect("dependency should be writable");
        let source = "import \"dep.flux\"\nfn main() -> i64 { value() }\n";
        std::fs::write(&main, source).expect("entry should be writable");
        let main = std::fs::canonicalize(main).unwrap();
        let uri = file_uri_from_path(&main);
        let documents = HashMap::from([(uri.clone(), source.to_string())]);
        let mut cache = crate::project::ProjectAnalysisCache::default();

        analyzed_project_document_cached(&uri, &documents, Some(&mut cache))
            .expect("first LSP analysis should succeed");
        analyzed_project_document_cached(&uri, &documents, Some(&mut cache))
            .expect("second LSP analysis should reuse the cache");
        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().misses, 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn imported_document_diagnostics_use_open_buffer_overlays() {
        let root = std::env::temp_dir().join(format!("flux-lsp-overlays-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temporary LSP project should be writable");
        let dependency = root.join("dep.flux");
        let main = root.join("main.flux");
        std::fs::write(&dependency, "pub fn value() -> i64 { 1 }\n")
            .expect("dependency should be writable");
        let main_source = "import \"dep.flux\"\nfn main() -> i64 { value() }\n";
        std::fs::write(&main, main_source).expect("entry should be writable");
        let dependency = std::fs::canonicalize(dependency).unwrap();
        let main = std::fs::canonicalize(main).unwrap();
        let dependency_uri = format!("file://{}", dependency.display());
        let main_uri = format!("file://{}", main.display());
        let documents = HashMap::from([
            (main_uri.clone(), main_source.to_string()),
            (
                dependency_uri,
                "pub fn value() -> str { \"unsaved\" }\n".to_string(),
            ),
        ]);
        let diagnostics = document_diagnostics_with_overlays(&main_uri, main_source, &documents);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.message.contains("expected i64, got str") })
        );
        let dependency_uri = documents
            .keys()
            .find(|uri| *uri != &main_uri)
            .expect("dependency URI should remain open");
        let dependency_diagnostics = document_diagnostics_with_overlays(
            dependency_uri,
            "pub fn value() -> str { \"unsaved\" }\n",
            &documents,
        );
        assert!(
            !dependency_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("program requires fn main")),
            "imported modules must not be diagnosed as executable entry points"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn imported_completion_hover_and_signature_help_use_unsaved_overlays() {
        let root =
            std::env::temp_dir().join(format!("flux-lsp-symbol-overlays-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temporary LSP project should be writable");
        let dependency = root.join("dep.flux");
        let main = root.join("main.flux");
        std::fs::write(&dependency, "pub fn value(input: i64) -> i64 { input }\n")
            .expect("dependency should be writable");
        let main_source = "import \"dep.flux\"\nfn main() -> i64 { value(1, 2) }\n";
        std::fs::write(&main, main_source).expect("entry should be writable");
        let dependency = std::fs::canonicalize(dependency).unwrap();
        let main = std::fs::canonicalize(main).unwrap();
        let dependency_uri = format!("file://{}", dependency.display());
        let main_uri = format!("file://{}", main.display());
        let dependency_overlay =
            "pub fn value(amount: i64, scale: i64 = 1) -> i64 { amount * scale }\n";
        let documents = HashMap::from([
            (main_uri.clone(), main_source.to_string()),
            (dependency_uri, dependency_overlay.to_string()),
        ]);

        let completion = JsonValue::Array(completion_items_at_position(
            &main_uri,
            main_source,
            &documents,
            Some(1),
        ))
        .to_json();
        assert!(completion.contains("\"label\":\"value\""));
        assert!(completion.contains("scale: i64 = …"));

        let call_line = main_source.lines().nth(1).unwrap();
        let value_character = call_line.find("value").unwrap() + 1;
        let hover = hover_for_document(
            &main_uri,
            main_source,
            &documents,
            1,
            value_character,
            PositionEncoding::Utf8,
        )
        .expect("imported function should hover through the overlay project")
        .to_json();
        assert!(hover.contains("fn value(amount: i64, scale: i64) -> i64"));

        let signature_character = call_line.find("2)").unwrap() + 1;
        let signature = signature_help_for_document(
            &main_uri,
            main_source,
            &documents,
            1,
            signature_character,
            PositionEncoding::Utf8,
        )
        .expect("imported function should provide signature help")
        .to_json();
        assert!(signature.contains("fn value(amount: i64, scale: i64 = …) -> i64"));
        assert!(signature.contains("\"activeParameter\":1"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn inlay_hints_show_only_genuinely_inferred_binding_types() {
        let uri = "file:///tmp/inlay.flux";
        let source = "struct User {\n    name: str\n}\nfn load() -> User { User { name: \"Flux\" } }\nfn main() -> i64 {\n    let User { name } = load()\n    for i in 0..1:\n        print(name)\n        print(i)\n    return 0\n}\n";
        let documents = HashMap::from([(uri.to_string(), source.to_string())]);
        let hints = inlay_hints_for_document(
            uri,
            source,
            &documents,
            0,
            usize::MAX,
            PositionEncoding::Utf8,
        );
        let json = JsonValue::Array(hints).to_json();
        assert!(json.contains("\"label\":\": str\""));
        assert!(json.contains("\"label\":\": i64\""));
        assert_eq!(json.matches("\"kind\":1").count(), 2);
        assert!(!json.contains("load: fn"));
    }

    #[test]
    fn hot_reload_status_reads_the_latest_project_runner_state() {
        let root = std::env::temp_dir().join(format!("flux-lsp-run-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temporary run-status project should be writable");
        let main = root.join("main.flux");
        std::fs::write(&main, "fn main() -> i64 { 0 }\n").expect("entry should be writable");
        let main = std::fs::canonicalize(main).unwrap();
        let uri = file_uri_from_path(&main);
        let status_path = crate::project::development_status_path(&main).unwrap();
        std::fs::write(
            &status_path,
            "{\"version\":1,\"state\":\"restarted\",\"generation\":3,\"mode\":\"debug\",\"runner_pid\":42,\"updated_unix_ms\":1234,\"message\":\"rebuilt\"}\n",
        )
        .expect("status should be writable");
        let documents = HashMap::from([(uri.clone(), "fn main() -> i64 { 0 }\n".to_string())]);
        let status = hot_reload_status(&uri, &documents)
            .expect("LSP should discover the current runner status")
            .to_json();
        assert!(status.contains("\"state\":\"restarted\""));
        assert!(status.contains("\"generation\":3"));
        let _ = std::fs::remove_file(status_path);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn multiline_strings_stay_strings_for_lsp_scanners() {
        let mut tokens = Vec::new();
        let mut in_multiline = false;
        tokenize_semantic_line(
            &mut tokens,
            "    let text: str = \"\"\"alpha",
            0,
            SourceId::new(1),
            None,
            PositionEncoding::Utf8,
            &mut in_multiline,
        );
        assert!(in_multiline);
        tokenize_semantic_line(
            &mut tokens,
            "beta \" quote, (still string)",
            1,
            SourceId::new(1),
            None,
            PositionEncoding::Utf8,
            &mut in_multiline,
        );
        assert!(in_multiline);
        assert_eq!(
            tokens.last().map(|token| token.kind),
            Some(SemanticTokenKind::String)
        );
        tokenize_semantic_line(
            &mut tokens,
            "gamma\"\"\"",
            2,
            SourceId::new(1),
            None,
            PositionEncoding::Utf8,
            &mut in_multiline,
        );
        assert!(!in_multiline);
        assert_eq!(
            active_call("print(\"\"\"hello (x, y)\nworld\"\"\", "),
            Some(("print", 1))
        );
    }

    #[test]
    fn semantic_tokens_mix_lexical_and_typed_categories() {
        let source = r#"fn double(value: i64) -> i64 { value * 2 }
fn main() -> i64 {
    let path: str = r"C:\Flux\"
    print(path)
    return double(21)
}
"#;
        crate::semantic::SemanticDatabase::analyze(source, SourceId::new(1))
            .expect("semantic-token fixture should analyze");
        let data = semantic_tokens("file:///tmp/tokens.flux", source, PositionEncoding::Utf8);
        assert!(!data.is_empty());
        let numbers = data
            .iter()
            .map(|value| match value {
                JsonValue::Number(number) => *number,
                _ => panic!("semantic token data must be numeric"),
            })
            .collect::<Vec<_>>();
        let (chunks, remainder) = numbers.as_chunks::<5>();
        assert!(remainder.is_empty());
        let kinds = chunks.iter().map(|token| token[3]).collect::<Vec<_>>();
        assert!(kinds.contains(&(SemanticTokenKind::Keyword as i64)));
        assert!(
            kinds.contains(&(SemanticTokenKind::Function as i64)),
            "semantic token kinds were {kinds:?}"
        );
        assert!(kinds.contains(&(SemanticTokenKind::Parameter as i64)));
        assert!(kinds.contains(&(SemanticTokenKind::Type as i64)));
        assert!(kinds.contains(&(SemanticTokenKind::Number as i64)));
        assert!(kinds.contains(&(SemanticTokenKind::String as i64)));
        assert!(kinds.contains(&(SemanticTokenKind::Operator as i64)));
    }
}
