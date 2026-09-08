use std::collections::{BTreeMap, HashMap};
use std::io::{self, BufRead, Write};

use crate::diagnostic::{Diagnostic, SourceId, SourceSpan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PositionEncoding {
    Utf8,
    Utf16,
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
                    documents.insert(uri.to_string(), text.to_string());
                    publish_document_diagnostics(&mut writer, uri, text, encoding)?;
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
                    documents.insert(uri.to_string(), text.to_string());
                    publish_document_diagnostics(&mut writer, uri, text, encoding)?;
                }
            }
            Some("textDocument/didClose") => {
                if let Some(uri) = message
                    .get("params")
                    .and_then(|params| params.get("textDocument"))
                    .and_then(|doc| doc.get("uri"))
                    .and_then(JsonValue::as_str)
                {
                    documents.remove(uri);
                    publish_empty_diagnostics(&mut writer, uri)?;
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

fn publish_document_diagnostics<W: Write>(
    writer: &mut W,
    uri: &str,
    source: &str,
    encoding: PositionEncoding,
) -> io::Result<()> {
    let source_id = SourceId::from_name(uri);
    let diagnostics = match crate::parser::parse_all_with_source(source, source_id) {
        Err(diagnostics) => diagnostics,
        Ok(program) if program.imports.is_empty() => crate::typecheck::check_all(&program)
            .err()
            .unwrap_or_default(),
        Ok(_) => Vec::new(),
    };
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
}
