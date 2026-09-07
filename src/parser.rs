use crate::ast::{
    BinOp, Binding, Expr, ExprKind, Function, Param, Program, Stmt, StmtKind, Type, UnaryOp,
};
use crate::diagnostic::{Diagnostic, DiagnosticStage, SourceId, SourceSpan};

#[derive(Debug, Clone)]
struct Line {
    number: usize,
    indent: usize,
    text: String,
}

impl Line {
    fn span(&self) -> SourceSpan {
        SourceSpan::new(self.number, self.indent + 1, self.text.len().max(1))
    }
}

#[derive(Debug, Clone)]
pub struct ParseSnapshot {
    pub source_id: SourceId,
    pub fingerprint: u64,
    pub program: Option<Program>,
    pub diagnostics: Vec<Diagnostic>,
}

impl ParseSnapshot {
    pub fn is_valid(&self) -> bool {
        self.diagnostics.is_empty() && self.program.is_some()
    }
}

pub fn parse_snapshot(source: &str, source_id: SourceId) -> ParseSnapshot {
    let fingerprint = source_fingerprint(source);
    match parse_all_with_source(source, source_id) {
        Ok(program) => ParseSnapshot {
            source_id,
            fingerprint,
            program: Some(program),
            diagnostics: Vec::new(),
        },
        Err(diagnostics) => ParseSnapshot {
            source_id,
            fingerprint,
            program: None,
            diagnostics,
        },
    }
}

pub fn reparse_snapshot(previous: &ParseSnapshot, source: &str) -> ParseSnapshot {
    let fingerprint = source_fingerprint(source);
    if fingerprint == previous.fingerprint {
        return previous.clone();
    }
    parse_snapshot(source, previous.source_id)
}

pub fn parse(source: &str) -> Result<Program, Diagnostic> {
    parse_all(source).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .next()
            .expect("parse_all always returns at least one diagnostic on failure")
    })
}

pub fn parse_with_source(source: &str, source_id: SourceId) -> Result<Program, Diagnostic> {
    parse_all_with_source(source, source_id).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .next()
            .expect("parse_all_with_source always returns diagnostics on failure")
    })
}

pub fn parse_all_with_source(
    source: &str,
    source_id: SourceId,
) -> Result<Program, Vec<Diagnostic>> {
    match parse_all(source) {
        Ok(mut program) => {
            attach_program_source(&mut program, source_id);
            Ok(program)
        }
        Err(diagnostics) => Err(diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.with_source(source_id))
            .collect()),
    }
}

pub fn parse_all(source: &str) -> Result<Program, Vec<Diagnostic>> {
    let (lines, mut diagnostics) = preprocess(source);
    let mut functions = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = &lines[index];
        if line.indent != 0 {
            diagnostics.push(diag(
                line.number,
                "top-level declarations must not be indented",
            ));
            index += 1;
            continue;
        }

        let header = match parse_function_header(&line.text, line.number) {
            Ok(header) => header,
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                index = recover_after_malformed_function(&lines, index + 1);
                continue;
            }
        };
        let FunctionHeader {
            name,
            name_span,
            params,
            returns,
            return_span,
            return_type_spans,
        } = header;
        let function_line = line.number;
        let function_span = line.span();
        index += 1;

        let body = if index < lines.len() && lines[index].indent > 0 {
            let body_indent = lines[index].indent;
            match parse_block(&lines, &mut index, body_indent) {
                Ok(body) => body,
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    index = recover_after_malformed_function(&lines, index);
                    continue;
                }
            }
        } else {
            Vec::new()
        };

        if index >= lines.len() || lines[index].indent != 0 || lines[index].text != "}" {
            diagnostics.push(diag(
                function_line,
                "function body must end with a top-level '}'",
            ));
            index = recover_after_malformed_function(&lines, index);
            continue;
        }
        index += 1;

        functions.push(Function {
            name,
            name_span,
            keyword_span: SourceSpan::new(function_line, 1, 2),
            params,
            returns,
            return_span,
            return_type_spans,
            body,
            line: function_line,
            span: function_span,
        });
    }

    if functions.is_empty() && diagnostics.is_empty() {
        diagnostics.push(Diagnostic::global(
            DiagnosticStage::Parse,
            "Flux source contains no functions",
        ));
    }

    if diagnostics.is_empty() {
        Ok(Program { functions })
    } else {
        Err(diagnostics)
    }
}

fn source_fingerprint(source: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in source.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn attach_program_source(program: &mut Program, source_id: SourceId) {
    for function in &mut program.functions {
        function.span = function.span.with_source(source_id);
        function.keyword_span = function.keyword_span.with_source(source_id);
        function.name_span = function.name_span.with_source(source_id);
        function.return_span = function.return_span.with_source(source_id);
        for span in &mut function.return_type_spans {
            *span = span.with_source(source_id);
        }
        for param in &mut function.params {
            param.name_span = param.name_span.with_source(source_id);
            param.type_span = param.type_span.with_source(source_id);
        }
        attach_block_source(&mut function.body, source_id);
    }
}

fn attach_block_source(body: &mut [Stmt], source_id: SourceId) {
    for stmt in body {
        stmt.span = stmt.span.with_source(source_id);
        stmt.keyword_span = stmt.keyword_span.with_source(source_id);
        match &mut stmt.kind {
            StmtKind::Let {
                name_span,
                type_span,
                expr,
                ..
            } => {
                *name_span = name_span.with_source(source_id);
                *type_span = type_span.with_source(source_id);
                attach_expr_source(expr, source_id);
            }
            StmtKind::LetDestructure { bindings, expr, .. } => {
                for binding in bindings {
                    binding.name_span = binding.name_span.with_source(source_id);
                    binding.type_span = binding.type_span.with_source(source_id);
                }
                attach_expr_source(expr, source_id);
            }
            StmtKind::Return(expressions) => {
                for expr in expressions {
                    attach_expr_source(expr, source_id);
                }
            }
            StmtKind::Expr(expr) => attach_expr_source(expr, source_id),
            StmtKind::If {
                cond,
                body,
                else_body,
                else_keyword_span,
            } => {
                if let Some(span) = else_keyword_span {
                    *span = span.with_source(source_id);
                }
                attach_expr_source(cond, source_id);
                attach_block_source(body, source_id);
                attach_block_source(else_body, source_id);
            }
            StmtKind::ForRange {
                name_span,
                start,
                end,
                body,
                ..
            } => {
                *name_span = name_span.with_source(source_id);
                attach_expr_source(start, source_id);
                attach_expr_source(end, source_id);
                attach_block_source(body, source_id);
            }
        }
    }
}

fn attach_expr_source(expr: &mut Expr, source_id: SourceId) {
    expr.span = expr.span.with_source(source_id);
    match &mut expr.kind {
        ExprKind::Call { args, .. } => {
            for arg in args {
                attach_expr_source(arg, source_id);
            }
        }
        ExprKind::Unary { expr, .. } => attach_expr_source(expr, source_id),
        ExprKind::Binary { left, right, .. } => {
            attach_expr_source(left, source_id);
            attach_expr_source(right, source_id);
        }
        ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::Var(_) => {}
    }
}

fn preprocess(source: &str) -> (Vec<Line>, Vec<Diagnostic>) {
    let mut lines = Vec::new();
    let mut diagnostics = Vec::new();
    for (index, raw) in source.lines().enumerate() {
        let number = index + 1;
        if raw.contains('\t') {
            diagnostics.push(diag(
                number,
                "tabs are not allowed for indentation; use spaces",
            ));
            continue;
        }

        let indent = raw.bytes().take_while(|byte| *byte == b' ').count();
        let without_comment = strip_comment(&raw[indent..]);
        let text = without_comment.trim_end();
        if text.trim().is_empty() {
            continue;
        }

        lines.push(Line {
            number,
            indent,
            text: text.to_string(),
        });
    }
    (lines, diagnostics)
}

fn recover_after_malformed_function(lines: &[Line], mut index: usize) -> usize {
    while index < lines.len() {
        let line = &lines[index];
        if line.indent == 0 {
            if line.text == "}" {
                return index + 1;
            }
            if line.text.starts_with("fn ") {
                return index;
            }
        }
        index += 1;
    }
    index
}

fn strip_comment(input: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if ch == '#' && !in_string {
            return &input[..index];
        }
    }
    input
}

struct FunctionHeader {
    name: String,
    name_span: SourceSpan,
    params: Vec<Param>,
    returns: Vec<Type>,
    return_span: SourceSpan,
    return_type_spans: Vec<SourceSpan>,
}

fn parse_function_header(input: &str, line: usize) -> Result<FunctionHeader, Diagnostic> {
    let Some(rest) = input.strip_prefix("fn ") else {
        return Err(diag(
            line,
            "expected function declaration starting with 'fn'",
        ));
    };
    let Some(open) = rest.find('(') else {
        return Err(diag(line, "expected '(' after function name"));
    };
    let raw_name = &rest[..open];
    let name = raw_name.trim();
    validate_identifier(name, line)?;
    let name_span = SourceSpan::new(line, 4 + raw_name.find(name).unwrap_or(0), name.len());

    let Some(close_rel) = rest[open + 1..].find(')') else {
        return Err(diag(line, "expected ')' after function parameters"));
    };
    let close = open + 1 + close_rel;
    let params_src = &rest[open + 1..close];
    let suffix = rest[close + 1..].trim();
    let Some(ret_src) = suffix.strip_prefix("->") else {
        return Err(diag(
            line,
            "functions require an explicit return type with '->'",
        ));
    };
    let Some(ret_src) = ret_src.trim().strip_suffix('{') else {
        let insertion = SourceSpan::new(line, input.len() + 1, 0);
        return Err(Diagnostic::new(
            DiagnosticStage::Parse,
            SourceSpan::line(line),
            "functions must open their body with '{'",
        )
        .with_fix(insertion, " {", "insert the function body opener"));
    };
    let return_text = ret_src.trim();
    let returns = parse_return_types(return_text, line)?;
    let return_span = SourceSpan::new(
        line,
        input
            .find(return_text)
            .map(|offset| offset + 1)
            .unwrap_or(1),
        return_text.len(),
    );
    let return_type_spans = return_type_spans(return_text, return_span);

    let mut params = Vec::new();
    if !params_src.trim().is_empty() {
        let params_base = 4 + open + 1;
        let mut param_offset = 0usize;
        for raw_param in params_src.split(',') {
            let Some((raw_name, raw_ty)) = raw_param.split_once(':') else {
                return Err(diag(line, "parameters use 'name: type' syntax"));
            };
            let param_name = raw_name.trim();
            validate_identifier(param_name, line)?;
            let type_text = raw_ty.trim();
            let ty = parse_type(type_text, line)?;
            let name_offset = raw_param.find(param_name).unwrap_or(0);
            let type_offset = raw_param.find(type_text).unwrap_or(raw_param.len());
            if ty == Type::Void {
                return Err(diag(line, "parameters cannot have type void"));
            }
            if params.iter().any(|param: &Param| param.name == param_name) {
                return Err(diag(line, &format!("duplicate parameter '{param_name}'")));
            }
            params.push(Param {
                name: param_name.to_string(),
                name_span: SourceSpan::new(
                    line,
                    params_base + param_offset + name_offset,
                    param_name.len(),
                ),
                ty,
                type_span: SourceSpan::new(
                    line,
                    params_base + param_offset + type_offset,
                    type_text.len(),
                ),
            });
            param_offset += raw_param.len() + 1;
        }
    }

    Ok(FunctionHeader {
        name: name.to_string(),
        name_span,
        params,
        returns,
        return_span,
        return_type_spans,
    })
}

fn return_type_spans(return_text: &str, return_span: SourceSpan) -> Vec<SourceSpan> {
    let Some(inner) = return_text
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
    else {
        return vec![return_span];
    };

    let mut spans = Vec::new();
    let mut search_from = 0usize;
    for raw in split_top_level_commas(inner) {
        let ty = raw.trim();
        let relative = inner[search_from..]
            .find(ty)
            .map(|offset| search_from + offset)
            .unwrap_or(search_from);
        spans.push(SourceSpan::new(
            return_span.line,
            return_span.column + 1 + relative,
            ty.len(),
        ));
        search_from = relative + ty.len();
    }
    spans
}

fn parse_block(lines: &[Line], index: &mut usize, indent: usize) -> Result<Vec<Stmt>, Diagnostic> {
    let mut body = Vec::new();

    while *index < lines.len() {
        let line = &lines[*index];
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            return Err(diag(line.number, "unexpected indentation"));
        }
        if line.text == "}" {
            break;
        }

        let stmt = if line.text.starts_with("if ") && line.text.ends_with(':') {
            parse_if_statement(lines, index, indent)?
        } else if line.text.starts_with("elif ") {
            return Err(diag(
                line.number,
                "elif must immediately follow an if or elif block",
            ));
        } else if line.text == "else" || line.text == "else:" || line.text.starts_with("else ") {
            return Err(diag(
                line.number,
                "else must immediately follow an if or elif block",
            ));
        } else if line.text.starts_with("for ") && line.text.ends_with(':') {
            let stmt_line = line.number;
            let inner = line.text[4..line.text.len() - 1].trim();
            let Some((name_src, range_src)) = inner.split_once(" in ") else {
                return Err(diag(stmt_line, "for loops use 'for name in start..end:'"));
            };
            let name = name_src.trim();
            validate_identifier(name, stmt_line)?;
            let Some((start_src, end_src)) = split_range(range_src) else {
                return Err(diag(
                    stmt_line,
                    "for loops currently require an exclusive range 'start..end'",
                ));
            };
            let start_src = start_src.trim();
            let end_src = end_src.trim();
            let start = parse_expression_at(
                start_src,
                stmt_line,
                expression_column(&line.text, start_src, line.indent + 1),
            )?;
            let end = parse_expression_at(
                end_src,
                stmt_line,
                expression_column(&line.text, end_src, line.indent + 1),
            )?;
            *index += 1;
            let nested = parse_nested_block(lines, index, indent, stmt_line, "for")?;
            Stmt {
                line: stmt_line,
                span: line.span(),
                keyword_span: SourceSpan::new(stmt_line, line.indent + 1, 3),
                kind: StmtKind::ForRange {
                    name: name.to_string(),
                    name_span: SourceSpan::new(
                        stmt_line,
                        expression_column(&line.text, name, line.indent + 1),
                        name.len(),
                    ),
                    start,
                    end,
                    body: nested,
                },
            }
        } else {
            let stmt = parse_simple_statement(&line.text, line.span())?;
            *index += 1;
            stmt
        };

        body.push(stmt);
    }

    Ok(body)
}

fn parse_if_statement(
    lines: &[Line],
    index: &mut usize,
    indent: usize,
) -> Result<Stmt, Diagnostic> {
    let line = &lines[*index];
    let cond_src = line.text[3..line.text.len() - 1].trim();
    if cond_src.is_empty() {
        return Err(diag(line.number, "if requires a condition"));
    }
    let cond = parse_expression_at(
        cond_src,
        line.number,
        expression_column(&line.text, cond_src, line.indent + 1),
    )?;
    let stmt_line = line.number;
    let stmt_span = line.span();
    *index += 1;
    let body = parse_nested_block(lines, index, indent, stmt_line, "if")?;
    let (else_body, else_keyword_span) = parse_if_tail(lines, index, indent)?;

    Ok(Stmt {
        line: stmt_line,
        span: stmt_span,
        keyword_span: SourceSpan::new(stmt_line, line.indent + 1, 2),
        kind: StmtKind::If {
            cond,
            body,
            else_body,
            else_keyword_span,
        },
    })
}

fn parse_if_tail(
    lines: &[Line],
    index: &mut usize,
    indent: usize,
) -> Result<(Vec<Stmt>, Option<SourceSpan>), Diagnostic> {
    if *index >= lines.len() || lines[*index].indent != indent {
        return Ok((Vec::new(), None));
    }

    let line = &lines[*index];
    if line.text.starts_with("elif ") && line.text.ends_with(':') {
        let cond_src = line.text[5..line.text.len() - 1].trim();
        if cond_src.is_empty() {
            return Err(diag(line.number, "elif requires a condition"));
        }
        let cond = parse_expression_at(
            cond_src,
            line.number,
            expression_column(&line.text, cond_src, line.indent + 1),
        )?;
        let stmt_line = line.number;
        let stmt_span = line.span();
        *index += 1;
        let body = parse_nested_block(lines, index, indent, stmt_line, "elif")?;
        let (else_body, else_keyword_span) = parse_if_tail(lines, index, indent)?;
        return Ok((
            vec![Stmt {
                line: stmt_line,
                span: stmt_span,
                keyword_span: SourceSpan::new(stmt_line, line.indent + 1, 4),
                kind: StmtKind::If {
                    cond,
                    body,
                    else_body,
                    else_keyword_span,
                },
            }],
            Some(SourceSpan::new(stmt_line, line.indent + 1, 4)),
        ));
    }

    if line.text == "else:" {
        let stmt_line = line.number;
        let keyword_span = SourceSpan::new(stmt_line, line.indent + 1, 4);
        *index += 1;
        let body = parse_nested_block(lines, index, indent, stmt_line, "else")?;
        return Ok((body, Some(keyword_span)));
    }

    Ok((Vec::new(), None))
}

fn parse_nested_block(
    lines: &[Line],
    index: &mut usize,
    parent_indent: usize,
    parent_line: usize,
    kind: &str,
) -> Result<Vec<Stmt>, Diagnostic> {
    if *index >= lines.len() || lines[*index].indent <= parent_indent {
        return Err(diag(
            parent_line,
            &format!("{kind} requires an indented body"),
        ));
    }
    let nested_indent = lines[*index].indent;
    parse_block(lines, index, nested_indent)
}

fn parse_simple_statement(input: &str, span: SourceSpan) -> Result<Stmt, Diagnostic> {
    let line = span.line;
    if let Some(rest) = input.strip_prefix("let ") {
        let Some((binding_src, raw_expr_src)) = rest.split_once('=') else {
            return Err(diag(line, "let bindings require '= expression'"));
        };
        let raw_expr_src = raw_expr_src.trim();
        let (expr_src, else_return) =
            if let Some(expr_src) = raw_expr_src.strip_suffix(" else return") {
                (expr_src.trim_end(), true)
            } else {
                (raw_expr_src, false)
            };
        let raw_bindings = split_top_level_commas(binding_src);
        if raw_bindings.len() == 1 {
            if else_return {
                return Err(diag(
                    line,
                    "'else return' requires a multi-value destructuring binding",
                ));
            }
            let raw_binding = raw_bindings[0].trim();
            let binding = parse_binding(
                raw_binding,
                line,
                expression_column(input, raw_binding, span.column),
            )?;
            let expr = parse_expression_at(
                expr_src,
                line,
                expression_column(input, expr_src, span.column),
            )?;
            return Ok(Stmt {
                line,
                span,
                keyword_span: SourceSpan::new(line, span.column, 3),
                kind: StmtKind::Let {
                    name: binding.name,
                    name_span: binding.name_span,
                    ty: binding.ty,
                    type_span: binding.type_span,
                    expr,
                },
            });
        }

        let mut bindings = Vec::with_capacity(raw_bindings.len());
        for raw_binding in raw_bindings {
            let raw_binding = raw_binding.trim();
            let binding = parse_binding(
                raw_binding,
                line,
                expression_column(input, raw_binding, span.column),
            )?;
            if bindings
                .iter()
                .any(|existing: &Binding| existing.name == binding.name)
            {
                return Err(diag(
                    line,
                    &format!("duplicate destructured binding '{}'", binding.name),
                ));
            }
            bindings.push(binding);
        }
        let expr = parse_expression_at(
            expr_src,
            line,
            expression_column(input, expr_src, span.column),
        )?;
        return Ok(Stmt {
            line,
            span,
            keyword_span: SourceSpan::new(line, span.column, 3),
            kind: StmtKind::LetDestructure {
                bindings,
                expr,
                else_return,
            },
        });
    }

    if input == "return" {
        return Ok(Stmt {
            line,
            span,
            keyword_span: SourceSpan::new(line, span.column, 6),
            kind: StmtKind::Return(Vec::new()),
        });
    }
    if let Some(expr_src) = input.strip_prefix("return ") {
        let expressions = split_top_level_commas(expr_src)
            .into_iter()
            .map(|part| {
                let part = part.trim();
                parse_expression_at(part, line, expression_column(input, part, span.column))
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Stmt {
            line,
            span,
            keyword_span: SourceSpan::new(line, span.column, 6),
            kind: StmtKind::Return(expressions),
        });
    }

    Ok(Stmt {
        line,
        span,
        keyword_span: SourceSpan::new(line, span.column, 0),
        kind: StmtKind::Expr(parse_expression_at(input, line, span.column)?),
    })
}

fn parse_binding(input: &str, line: usize, column: usize) -> Result<Binding, Diagnostic> {
    let Some((name_src, ty_src)) = input.split_once(':') else {
        return Err(diag(
            line,
            "strict bindings require an explicit type: 'let name: type = value'",
        ));
    };
    let name = name_src.trim();
    validate_identifier(name, line)?;
    let type_text = ty_src.trim();
    let ty = parse_type(type_text, line)?;
    if ty == Type::Void {
        return Err(diag(line, "variables cannot have type void"));
    }
    Ok(Binding {
        name: name.to_string(),
        name_span: SourceSpan::new(line, column + name_src.find(name).unwrap_or(0), name.len()),
        ty,
        type_span: SourceSpan::new(
            line,
            column + input.find(type_text).unwrap_or(0),
            type_text.len(),
        ),
    })
}

fn split_top_level_commas(input: &str) -> Vec<&str> {
    split_top_level_commas_with_offsets(input)
        .into_iter()
        .map(|(part, _)| part)
        .collect()
}

fn split_top_level_commas_with_offsets(input: &str) -> Vec<(&str, usize)> {
    let bytes = input.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut depth = 0usize;

    for (index, byte) in bytes.iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' && in_string {
            escaped = true;
            continue;
        }
        if byte == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match byte {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                parts.push((&input[start..index], start));
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push((&input[start..], start));
    parts
}

fn split_range(input: &str) -> Option<(&str, &str)> {
    let bytes = input.as_bytes();
    let mut in_string = false;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut index = 0usize;
    while index + 1 < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if byte == b'\\' && in_string {
            escaped = true;
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = !in_string;
            index += 1;
            continue;
        }
        if !in_string {
            match byte {
                b'(' => depth += 1,
                b')' => depth = depth.saturating_sub(1),
                b'.' if depth == 0 && bytes[index + 1] == b'.' => {
                    return Some((&input[..index], &input[index + 2..]));
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

fn parse_return_types(input: &str, line: usize) -> Result<Vec<Type>, Diagnostic> {
    let input = input.trim();
    if input == "void" {
        return Ok(Vec::new());
    }
    if let Some(inner) = input
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
    {
        let parts = split_top_level_commas(inner);
        if parts.len() < 2 {
            return Err(diag(
                line,
                "multi-value return types require at least two types",
            ));
        }
        let mut returns = Vec::with_capacity(parts.len());
        for part in parts {
            let ty = parse_type(part.trim(), line)?;
            if ty == Type::Void {
                return Err(diag(line, "void cannot appear in a multi-value return"));
            }
            returns.push(ty);
        }
        return Ok(returns);
    }

    let ty = parse_type(input, line)?;
    if ty == Type::Void {
        Ok(Vec::new())
    } else {
        Ok(vec![ty])
    }
}

fn parse_type(input: &str, line: usize) -> Result<Type, Diagnostic> {
    Type::parse(input).ok_or_else(|| diag(line, &format!("unknown type '{input}'")))
}

fn validate_identifier(input: &str, line: usize) -> Result<(), Diagnostic> {
    if input.starts_with("flux__") {
        return Err(diag(
            line,
            "identifiers starting with 'flux__' are reserved for the compiler",
        ));
    }
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return Err(diag(line, "identifier cannot be empty"));
    };
    if !(first == '_' || first.is_ascii_alphabetic())
        || !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    {
        return Err(diag(line, &format!("invalid identifier '{input}'")));
    }
    if matches!(
        input,
        "fn" | "let"
            | "return"
            | "if"
            | "elif"
            | "else"
            | "for"
            | "in"
            | "true"
            | "false"
            | "nil"
            | "error"
    ) {
        return Err(diag(line, &format!("'{input}' is reserved")));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Int(i64),
    Str(String),
    Ident(String),
    True,
    False,
    Nil,
    Plus,
    Minus,
    Star,
    Slash,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    LParen,
    RParen,
    Comma,
}

#[derive(Debug, Clone, PartialEq)]
struct Token {
    kind: TokenKind,
    span: SourceSpan,
}

fn expression_column(haystack: &str, needle: &str, base_column: usize) -> usize {
    haystack
        .find(needle)
        .map(|offset| base_column + offset)
        .unwrap_or(base_column)
}

fn parse_expression_at(input: &str, line: usize, column: usize) -> Result<Expr, Diagnostic> {
    let tokens = lex_expression(input, line, column)?;
    let mut parser = ExprParser {
        tokens: &tokens,
        index: 0,
        line,
    };
    let expr = parser.parse_binary(1)?;
    if parser.index != tokens.len() {
        let span = tokens[parser.index].span;
        return Err(Diagnostic::new(
            DiagnosticStage::Parse,
            span,
            "unexpected token after expression",
        ));
    }
    Ok(expr)
}

fn lex_expression(input: &str, line: usize, column: usize) -> Result<Vec<Token>, Diagnostic> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0usize;

    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }

        let start = index;
        let kind = if byte.is_ascii_digit() {
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            let value = input[start..index].parse::<i64>().map_err(|_| {
                Diagnostic::new(
                    DiagnosticStage::Parse,
                    SourceSpan::new(line, column + start, index - start),
                    "integer literal is outside i64 range",
                )
            })?;
            TokenKind::Int(value)
        } else if byte == b'"' {
            index += 1;
            let mut value = String::new();
            let mut closed = false;
            while index < bytes.len() {
                match bytes[index] {
                    b'"' => {
                        index += 1;
                        closed = true;
                        break;
                    }
                    b'\\' => {
                        index += 1;
                        if index >= bytes.len() {
                            break;
                        }
                        let escaped = match bytes[index] {
                            b'n' => '\n',
                            b'r' => '\r',
                            b't' => '\t',
                            b'"' => '"',
                            b'\\' => '\\',
                            other => {
                                return Err(Diagnostic::new(
                                    DiagnosticStage::Parse,
                                    SourceSpan::new(line, column + index, 1),
                                    format!("unsupported escape '\\{}'", other as char),
                                ));
                            }
                        };
                        value.push(escaped);
                        index += 1;
                    }
                    other if other.is_ascii() => {
                        value.push(other as char);
                        index += 1;
                    }
                    _ => {
                        let rest = &input[index..];
                        let ch = rest.chars().next().expect("non-empty unicode tail");
                        value.push(ch);
                        index += ch.len_utf8();
                    }
                }
            }
            if !closed {
                return Err(Diagnostic::new(
                    DiagnosticStage::Parse,
                    SourceSpan::new(line, column + start, input.len() - start),
                    "unterminated string literal",
                ));
            }
            TokenKind::Str(value)
        } else if byte == b'_' || byte.is_ascii_alphabetic() {
            index += 1;
            while index < bytes.len()
                && (bytes[index] == b'_' || bytes[index].is_ascii_alphanumeric())
            {
                index += 1;
            }
            match &input[start..index] {
                "true" => TokenKind::True,
                "false" => TokenKind::False,
                "nil" => TokenKind::Nil,
                ident => TokenKind::Ident(ident.to_string()),
            }
        } else {
            let (kind, width) = match byte {
                b'+' => (TokenKind::Plus, 1),
                b'-' => (TokenKind::Minus, 1),
                b'*' => (TokenKind::Star, 1),
                b'/' => (TokenKind::Slash, 1),
                b'(' => (TokenKind::LParen, 1),
                b')' => (TokenKind::RParen, 1),
                b',' => (TokenKind::Comma, 1),
                b'!' if bytes.get(index + 1) == Some(&b'=') => (TokenKind::NotEq, 2),
                b'!' => (TokenKind::Bang, 1),
                b'=' if bytes.get(index + 1) == Some(&b'=') => (TokenKind::EqEq, 2),
                b'<' if bytes.get(index + 1) == Some(&b'=') => (TokenKind::Le, 2),
                b'<' => (TokenKind::Lt, 1),
                b'>' if bytes.get(index + 1) == Some(&b'=') => (TokenKind::Ge, 2),
                b'>' => (TokenKind::Gt, 1),
                b'&' if bytes.get(index + 1) == Some(&b'&') => (TokenKind::AndAnd, 2),
                b'|' if bytes.get(index + 1) == Some(&b'|') => (TokenKind::OrOr, 2),
                other => {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        SourceSpan::new(line, column + index, 1),
                        format!("unexpected character '{}' in expression", other as char),
                    ));
                }
            };
            index += width;
            kind
        };

        tokens.push(Token {
            kind,
            span: SourceSpan::new(line, column + start, index - start),
        });
    }

    if tokens.is_empty() {
        return Err(Diagnostic::new(
            DiagnosticStage::Parse,
            SourceSpan::new(line, column, 1),
            "expected expression",
        ));
    }
    Ok(tokens)
}

struct ExprParser<'a> {
    tokens: &'a [Token],
    index: usize,
    line: usize,
}

impl ExprParser<'_> {
    fn parse_binary(&mut self, min_precedence: u8) -> Result<Expr, Diagnostic> {
        let mut left = self.parse_unary()?;
        while let Some((op, precedence)) = self.peek_binary() {
            if precedence < min_precedence {
                break;
            }
            self.index += 1;
            let right = self.parse_binary(precedence + 1)?;
            let span = SourceSpan::new(
                self.line,
                left.span.column,
                right.span.column + right.span.length - left.span.column,
            );
            left = Expr {
                line: self.line,
                span,
                kind: ExprKind::Binary {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, Diagnostic> {
        if matches!(
            self.tokens.get(self.index).map(|token| &token.kind),
            Some(TokenKind::Minus)
        ) {
            let op_span = self.tokens[self.index].span;
            self.index += 1;
            let expr = self.parse_unary()?;
            let span = SourceSpan::new(
                self.line,
                op_span.column,
                expr.span.column + expr.span.length - op_span.column,
            );
            return Ok(Expr {
                line: self.line,
                span,
                kind: ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                },
            });
        }
        if matches!(
            self.tokens.get(self.index).map(|token| &token.kind),
            Some(TokenKind::Bang)
        ) {
            let op_span = self.tokens[self.index].span;
            self.index += 1;
            let expr = self.parse_unary()?;
            let span = SourceSpan::new(
                self.line,
                op_span.column,
                expr.span.column + expr.span.length - op_span.column,
            );
            return Ok(Expr {
                line: self.line,
                span,
                kind: ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                },
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, Diagnostic> {
        let Some(token) = self.tokens.get(self.index).cloned() else {
            return Err(diag(self.line, "expected expression"));
        };
        self.index += 1;
        let token_span = token.span;

        match token.kind {
            TokenKind::Int(value) => Ok(Expr {
                line: self.line,
                span: token_span,
                kind: ExprKind::Int(value),
            }),
            TokenKind::Str(value) => Ok(Expr {
                line: self.line,
                span: token_span,
                kind: ExprKind::Str(value),
            }),
            TokenKind::True => Ok(Expr {
                line: self.line,
                span: token_span,
                kind: ExprKind::Bool(true),
            }),
            TokenKind::False => Ok(Expr {
                line: self.line,
                span: token_span,
                kind: ExprKind::Bool(false),
            }),
            TokenKind::Nil => Ok(Expr {
                line: self.line,
                span: token_span,
                kind: ExprKind::Nil,
            }),
            TokenKind::Ident(name) => {
                if !matches!(
                    self.tokens.get(self.index).map(|token| &token.kind),
                    Some(TokenKind::LParen)
                ) {
                    return Ok(Expr {
                        line: self.line,
                        span: token_span,
                        kind: ExprKind::Var(name),
                    });
                }
                self.index += 1;
                let mut args = Vec::new();
                if !matches!(
                    self.tokens.get(self.index).map(|token| &token.kind),
                    Some(TokenKind::RParen)
                ) {
                    loop {
                        args.push(self.parse_binary(1)?);
                        match self.tokens.get(self.index).map(|token| &token.kind) {
                            Some(TokenKind::Comma) => self.index += 1,
                            Some(TokenKind::RParen) => break,
                            _ => {
                                return Err(diag(
                                    self.line,
                                    "expected ',' or ')' in call arguments",
                                ));
                            }
                        }
                    }
                }
                let Some(close) = self.tokens.get(self.index) else {
                    return Err(diag(self.line, "expected ')' after call arguments"));
                };
                if !matches!(close.kind, TokenKind::RParen) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        close.span,
                        "expected ')' after call arguments",
                    ));
                }
                let close_span = close.span;
                self.index += 1;
                Ok(Expr {
                    line: self.line,
                    span: SourceSpan::new(
                        self.line,
                        token_span.column,
                        close_span.column + close_span.length - token_span.column,
                    ),
                    kind: ExprKind::Call { name, args },
                })
            }
            TokenKind::LParen => {
                let mut expr = self.parse_binary(1)?;
                let Some(close) = self.tokens.get(self.index) else {
                    return Err(diag(self.line, "expected ')'"));
                };
                if !matches!(close.kind, TokenKind::RParen) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        close.span,
                        "expected ')'",
                    ));
                }
                let close_span = close.span;
                self.index += 1;
                expr.span = SourceSpan::new(
                    self.line,
                    token_span.column,
                    close_span.column + close_span.length - token_span.column,
                );
                Ok(expr)
            }
            _ => Err(Diagnostic::new(
                DiagnosticStage::Parse,
                token_span,
                "expected expression",
            )),
        }
    }

    fn peek_binary(&self) -> Option<(BinOp, u8)> {
        Some(match &self.tokens.get(self.index)?.kind {
            TokenKind::OrOr => (BinOp::Or, 1),
            TokenKind::AndAnd => (BinOp::And, 2),
            TokenKind::EqEq => (BinOp::Eq, 3),
            TokenKind::NotEq => (BinOp::Ne, 3),
            TokenKind::Lt => (BinOp::Lt, 4),
            TokenKind::Le => (BinOp::Le, 4),
            TokenKind::Gt => (BinOp::Gt, 4),
            TokenKind::Ge => (BinOp::Ge, 4),
            TokenKind::Plus => (BinOp::Add, 5),
            TokenKind::Minus => (BinOp::Sub, 5),
            TokenKind::Star => (BinOp::Mul, 6),
            TokenKind::Slash => (BinOp::Div, 6),
            _ => return None,
        })
    }
}

fn diag(line: usize, message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticStage::Parse, SourceSpan::line(line), message)
}
