use crate::ast::{
    BinOp, Binding, ConstantDef, EnumDef, EnumPayload, EnumVariant, Expr, ExprKind, Function,
    MatchArm, Param, PatternBinding, Program, Stmt, StmtKind, StructDef, StructField,
    StructLiteralField, StructPatternField, Type, TypeAlias, UnaryOp,
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
    let mut aliases = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut constants = Vec::new();
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

        if line.text.starts_with("type ") {
            match parse_type_alias(line) {
                Ok(alias) => aliases.push(alias),
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
            index += 1;
            continue;
        }

        if line.text.starts_with("const ") {
            match parse_constant(line) {
                Ok(constant) => constants.push(constant),
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
            index += 1;
            continue;
        }

        if line.text.starts_with("struct ") {
            match parse_struct_declaration(&lines, &mut index) {
                Ok(definition) => structs.push(definition),
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    index = recover_after_malformed_declaration(&lines, index.saturating_add(1));
                }
            }
            continue;
        }

        if line.text.starts_with("enum ") {
            match parse_enum_declaration(&lines, &mut index) {
                Ok(definition) => enums.push(definition),
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    index = recover_after_malformed_declaration(&lines, index.saturating_add(1));
                }
            }
            continue;
        }

        let header = match parse_function_header(&line.text, line.number) {
            Ok(header) => header,
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                index = recover_after_malformed_declaration(&lines, index + 1);
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
                    index = recover_after_malformed_declaration(&lines, index);
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
            index = recover_after_malformed_declaration(&lines, index);
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
        Ok(Program {
            aliases,
            structs,
            enums,
            constants,
            functions,
        })
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
    for alias in &mut program.aliases {
        alias.span = alias.span.with_source(source_id);
        alias.name_span = alias.name_span.with_source(source_id);
        alias.target_span = alias.target_span.with_source(source_id);
    }
    for definition in &mut program.enums {
        definition.span = definition.span.with_source(source_id);
        definition.keyword_span = definition.keyword_span.with_source(source_id);
        definition.name_span = definition.name_span.with_source(source_id);
        for variant in &mut definition.variants {
            variant.name_span = variant.name_span.with_source(source_id);
            for payload in &mut variant.payloads {
                payload.type_span = payload.type_span.with_source(source_id);
            }
        }
    }
    for constant in &mut program.constants {
        constant.span = constant.span.with_source(source_id);
        constant.name_span = constant.name_span.with_source(source_id);
        constant.type_span = constant.type_span.with_source(source_id);
        attach_expr_source(&mut constant.value, source_id);
    }
    for definition in &mut program.structs {
        definition.span = definition.span.with_source(source_id);
        definition.keyword_span = definition.keyword_span.with_source(source_id);
        definition.name_span = definition.name_span.with_source(source_id);
        for field in &mut definition.fields {
            field.name_span = field.name_span.with_source(source_id);
            field.type_span = field.type_span.with_source(source_id);
        }
    }
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
            StmtKind::LetStructDestructure {
                struct_span,
                fields,
                expr,
                ..
            } => {
                *struct_span = struct_span.with_source(source_id);
                for field in fields {
                    field.field_span = field.field_span.with_source(source_id);
                    field.binding.span = field.binding.span.with_source(source_id);
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
            StmtKind::Match { value, arms } => {
                attach_expr_source(value, source_id);
                for arm in arms {
                    arm.span = arm.span.with_source(source_id);
                    arm.enum_span = arm.enum_span.with_source(source_id);
                    arm.variant_span = arm.variant_span.with_source(source_id);
                    for binding in &mut arm.bindings {
                        binding.span = binding.span.with_source(source_id);
                    }
                    attach_block_source(&mut arm.body, source_id);
                }
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
        ExprKind::StructLiteral {
            name_span,
            base,
            fields,
            ..
        } => {
            *name_span = name_span.with_source(source_id);
            if let Some(base) = base {
                attach_expr_source(base, source_id);
            }
            for field in fields {
                field.name_span = field.name_span.with_source(source_id);
                attach_expr_source(&mut field.value, source_id);
            }
        }
        ExprKind::EnumVariant {
            enum_span,
            variant_span,
            args,
            ..
        } => {
            *enum_span = enum_span.with_source(source_id);
            *variant_span = variant_span.with_source(source_id);
            for arg in args {
                attach_expr_source(arg, source_id);
            }
        }
        ExprKind::Field {
            base, name_span, ..
        } => {
            *name_span = name_span.with_source(source_id);
            attach_expr_source(base, source_id);
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

fn recover_after_malformed_declaration(lines: &[Line], mut index: usize) -> usize {
    while index < lines.len() {
        let line = &lines[index];
        if line.indent == 0 {
            if line.text == "}" {
                return index + 1;
            }
            if line.text.starts_with("fn ")
                || line.text.starts_with("struct ")
                || line.text.starts_with("enum ")
                || line.text.starts_with("type ")
                || line.text.starts_with("const ")
            {
                return index;
            }
        }
        index += 1;
    }
    index
}

fn parse_type_alias(line: &Line) -> Result<TypeAlias, Diagnostic> {
    let Some(rest) = line.text.strip_prefix("type ") else {
        return Err(diag(line.number, "expected type alias declaration"));
    };
    let Some(eq_offset) = rest.find('=') else {
        return Err(diag(
            line.number,
            "type aliases use 'type Name = Target' syntax",
        ));
    };
    let raw_name = &rest[..eq_offset];
    let raw_target = &rest[eq_offset + 1..];
    let (name, name_column) = trim_with_column(raw_name, 6);
    validate_identifier(name, line.number)?;
    let (target_text, target_column) = trim_with_column(raw_target, 6 + eq_offset + 1);
    let target = parse_type(target_text, line.number)?;
    if target == Type::Void {
        return Err(diag(line.number, "type aliases cannot target void"));
    }
    Ok(TypeAlias {
        name: name.to_string(),
        name_span: SourceSpan::new(line.number, name_column, name.len()),
        target,
        target_span: SourceSpan::new(line.number, target_column, target_text.len()),
        line: line.number,
        span: line.span(),
    })
}

fn parse_constant(line: &Line) -> Result<ConstantDef, Diagnostic> {
    let Some(rest) = line.text.strip_prefix("const ") else {
        return Err(diag(line.number, "expected constant declaration"));
    };
    let Some(eq_offset) = rest.find('=') else {
        return Err(diag(
            line.number,
            "constants use 'const name: type = expression' syntax",
        ));
    };
    let binding_src = &rest[..eq_offset];
    let raw_value = &rest[eq_offset + 1..];
    let binding = parse_binding(binding_src.trim(), line.number, 7)?;
    let raw_value_column = 7 + eq_offset + 1;
    let (value_src, value_column) = trim_with_column(raw_value, raw_value_column);
    let value = parse_expression_at(value_src, line.number, value_column)?;
    Ok(ConstantDef {
        name: binding.name,
        name_span: binding.name_span,
        ty: binding.ty,
        type_span: binding.type_span,
        value,
        line: line.number,
        span: line.span(),
    })
}

fn parse_enum_declaration(lines: &[Line], index: &mut usize) -> Result<EnumDef, Diagnostic> {
    let header = &lines[*index];
    let Some(rest) = header.text.strip_prefix("enum ") else {
        return Err(diag(header.number, "expected enum declaration"));
    };
    let Some(raw_name) = rest.strip_suffix('{') else {
        return Err(diag(
            header.number,
            "enum declarations must open their body with '{'",
        ));
    };
    let name = raw_name.trim();
    validate_identifier(name, header.number)?;
    let leading = raw_name.len() - raw_name.trim_start().len();
    let name_span = SourceSpan::new(header.number, 6 + leading, name.len());
    let definition_span = header.span();
    *index += 1;

    let mut variants = Vec::new();
    if *index < lines.len() && lines[*index].indent > 0 {
        let variant_indent = lines[*index].indent;
        while *index < lines.len() {
            let line = &lines[*index];
            if line.indent < variant_indent {
                break;
            }
            if line.indent > variant_indent {
                return Err(diag(line.number, "unexpected indentation in enum body"));
            }
            if line.text == "}" {
                break;
            }

            let text = line.text.trim();
            let (variant_name, payload_source, payload_column) = if let Some(open) = text.find('(')
            {
                let Some(inner) = text.strip_suffix(')') else {
                    return Err(diag(line.number, "enum variant payload must end with ')'"));
                };
                let variant_name = text[..open].trim();
                let payload_source = &inner[open + 1..];
                let payload_column = line.indent + 1 + open + 1;
                (variant_name, Some(payload_source), payload_column)
            } else {
                (text, None, line.indent + 1 + text.len())
            };
            validate_identifier(variant_name, line.number)?;
            if variants
                .iter()
                .any(|variant: &EnumVariant| variant.name == variant_name)
            {
                return Err(diag(
                    line.number,
                    &format!("duplicate enum variant '{variant_name}'"),
                ));
            }
            let variant_offset = line.text.find(variant_name).unwrap_or(0);
            let mut payloads = Vec::new();
            if let Some(payload_source) = payload_source
                && !payload_source.trim().is_empty()
            {
                for (raw_payload, offset) in split_top_level_commas_with_offsets(payload_source) {
                    let (type_text, type_column) =
                        trim_with_column(raw_payload, payload_column + offset);
                    let ty = parse_type(type_text, line.number)?;
                    if ty == Type::Void {
                        return Err(diag(line.number, "enum payloads cannot have type void"));
                    }
                    payloads.push(EnumPayload {
                        ty,
                        type_span: SourceSpan::new(line.number, type_column, type_text.len()),
                    });
                }
            }
            variants.push(EnumVariant {
                name: variant_name.to_string(),
                name_span: SourceSpan::new(
                    line.number,
                    line.indent + 1 + variant_offset,
                    variant_name.len(),
                ),
                payloads,
            });
            *index += 1;
        }
    }

    if variants.is_empty() {
        return Err(diag(
            header.number,
            "enum declarations require at least one variant",
        ));
    }
    if *index >= lines.len() || lines[*index].indent != 0 || lines[*index].text != "}" {
        return Err(diag(
            header.number,
            "enum body must end with a top-level '}'",
        ));
    }
    *index += 1;

    Ok(EnumDef {
        name: name.to_string(),
        name_span,
        keyword_span: SourceSpan::new(header.number, 1, 4),
        variants,
        line: header.number,
        span: definition_span,
    })
}

fn parse_struct_declaration(lines: &[Line], index: &mut usize) -> Result<StructDef, Diagnostic> {
    let header = &lines[*index];
    let Some(rest) = header.text.strip_prefix("struct ") else {
        return Err(diag(header.number, "expected struct declaration"));
    };
    let Some(raw_name) = rest.strip_suffix('{') else {
        return Err(diag(
            header.number,
            "struct declarations must open their body with '{'",
        ));
    };
    let name = raw_name.trim();
    validate_identifier(name, header.number)?;
    let leading = raw_name.len() - raw_name.trim_start().len();
    let name_span = SourceSpan::new(header.number, 8 + leading, name.len());
    let definition_span = header.span();
    *index += 1;

    let mut fields = Vec::new();
    if *index < lines.len() && lines[*index].indent > 0 {
        let field_indent = lines[*index].indent;
        while *index < lines.len() {
            let line = &lines[*index];
            if line.indent < field_indent {
                break;
            }
            if line.indent > field_indent {
                return Err(diag(line.number, "unexpected indentation in struct body"));
            }
            let Some(colon_offset) = line.text.find(':') else {
                return Err(diag(line.number, "struct fields use 'name: type' syntax"));
            };
            let raw_field_name = &line.text[..colon_offset];
            let raw_type = &line.text[colon_offset + 1..];
            let (field_name, name_column) = trim_with_column(raw_field_name, line.indent + 1);
            validate_identifier(field_name, line.number)?;
            let (type_text, type_column) =
                trim_with_column(raw_type, line.indent + 1 + colon_offset + 1);
            let ty = parse_type(type_text, line.number)?;
            if ty == Type::Void {
                return Err(diag(line.number, "struct fields cannot have type void"));
            }
            if fields
                .iter()
                .any(|field: &StructField| field.name == field_name)
            {
                return Err(diag(
                    line.number,
                    &format!("duplicate struct field '{field_name}'"),
                ));
            }
            fields.push(StructField {
                name: field_name.to_string(),
                name_span: SourceSpan::new(line.number, name_column, field_name.len()),
                ty,
                type_span: SourceSpan::new(line.number, type_column, type_text.len()),
            });
            *index += 1;
        }
    }

    if *index >= lines.len() || lines[*index].indent != 0 || lines[*index].text != "}" {
        return Err(diag(
            header.number,
            "struct body must end with a top-level '}'",
        ));
    }
    *index += 1;

    Ok(StructDef {
        name: name.to_string(),
        name_span,
        keyword_span: SourceSpan::new(header.number, 1, 6),
        fields,
        line: header.number,
        span: definition_span,
    })
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
    let arrow_offset = input
        .find("->")
        .expect("function return arrow was already parsed");
    let after_arrow = &input[arrow_offset + 2..];
    let return_leading = after_arrow.len() - after_arrow.trim_start().len();
    let return_column = arrow_offset + 2 + return_leading + 1;
    let return_span = SourceSpan::new(line, return_column, return_text.len());
    let return_type_spans = return_type_spans(return_text, return_span);

    let mut params = Vec::new();
    if !params_src.trim().is_empty() {
        let params_base_column = 4 + open + 1;
        for (raw_param, param_offset) in split_top_level_commas_with_offsets(params_src) {
            let Some(colon_offset) = raw_param.find(':') else {
                return Err(diag(line, "parameters use 'name: type' syntax"));
            };
            let raw_name = &raw_param[..colon_offset];
            let raw_ty = &raw_param[colon_offset + 1..];
            let (param_name, name_column) =
                trim_with_column(raw_name, params_base_column + param_offset);
            validate_identifier(param_name, line)?;
            let (type_text, type_column) =
                trim_with_column(raw_ty, params_base_column + param_offset + colon_offset + 1);
            let ty = parse_type(type_text, line)?;
            if ty == Type::Void {
                return Err(diag(line, "parameters cannot have type void"));
            }
            if params.iter().any(|param: &Param| param.name == param_name) {
                return Err(diag(line, &format!("duplicate parameter '{param_name}'")));
            }
            params.push(Param {
                name: param_name.to_string(),
                name_span: SourceSpan::new(line, name_column, param_name.len()),
                ty,
                type_span: SourceSpan::new(line, type_column, type_text.len()),
            });
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

    split_top_level_commas_with_offsets(inner)
        .into_iter()
        .map(|(raw, offset)| {
            let (ty, column) = trim_with_column(raw, return_span.column + 1 + offset);
            SourceSpan::new(return_span.line, column, ty.len())
        })
        .collect()
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

        let stmt = if line.text.starts_with("match ") && line.text.ends_with(':') {
            parse_match_statement(lines, index, indent)?
        } else if line.text.starts_with("if ") && line.text.ends_with(':') {
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
            let raw_inner = &line.text[4..line.text.len() - 1];
            let inner_leading = raw_inner.len() - raw_inner.trim_start().len();
            let inner = raw_inner.trim();
            let inner_column = line.indent + 1 + 4 + inner_leading;
            let Some(in_offset) = inner.find(" in ") else {
                return Err(diag(stmt_line, "for loops use 'for name in start..end:'"));
            };
            let raw_name = &inner[..in_offset];
            let raw_range = &inner[in_offset + 4..];
            let (name, name_column) = trim_with_column(raw_name, inner_column);
            validate_identifier(name, stmt_line)?;
            let (range_src, range_column) =
                trim_with_column(raw_range, inner_column + in_offset + 4);
            let Some((start_raw, end_raw, range_split)) = split_range_with_offset(range_src) else {
                return Err(diag(
                    stmt_line,
                    "for loops currently require an exclusive range 'start..end'",
                ));
            };
            let (start_src, start_column) = trim_with_column(start_raw, range_column);
            let (end_src, end_column) = trim_with_column(end_raw, range_column + range_split + 2);
            let start = parse_expression_at(start_src, stmt_line, start_column)?;
            let end = parse_expression_at(end_src, stmt_line, end_column)?;
            *index += 1;
            let nested = parse_nested_block(lines, index, indent, stmt_line, "for")?;
            Stmt {
                line: stmt_line,
                span: line.span(),
                keyword_span: SourceSpan::new(stmt_line, line.indent + 1, 3),
                kind: StmtKind::ForRange {
                    name: name.to_string(),
                    name_span: SourceSpan::new(stmt_line, name_column, name.len()),
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

fn parse_match_statement(
    lines: &[Line],
    index: &mut usize,
    indent: usize,
) -> Result<Stmt, Diagnostic> {
    let line = &lines[*index];
    let raw_value = &line.text[6..line.text.len() - 1];
    let (value_src, value_column) = trim_with_column(raw_value, line.indent + 1 + 6);
    if value_src.is_empty() {
        return Err(diag(line.number, "match requires a value"));
    }
    let value = parse_expression_at(value_src, line.number, value_column)?;
    let stmt_line = line.number;
    let stmt_span = line.span();
    *index += 1;

    if *index >= lines.len() || lines[*index].indent <= indent {
        return Err(diag(stmt_line, "match requires at least one indented arm"));
    }
    let arm_indent = lines[*index].indent;
    let mut arms = Vec::new();
    while *index < lines.len() && lines[*index].indent == arm_indent {
        let arm_line = &lines[*index];
        let mut arm = parse_match_arm_header(arm_line)?;
        *index += 1;
        arm.body = parse_nested_block(lines, index, arm_indent, arm.line, "match arm")?;
        arms.push(arm);
    }
    if arms.is_empty() {
        return Err(diag(stmt_line, "match requires at least one arm"));
    }

    Ok(Stmt {
        line: stmt_line,
        span: stmt_span,
        keyword_span: SourceSpan::new(stmt_line, line.indent + 1, 5),
        kind: StmtKind::Match { value, arms },
    })
}

fn parse_match_arm_header(line: &Line) -> Result<MatchArm, Diagnostic> {
    let Some(pattern) = line.text.strip_suffix(':') else {
        return Err(diag(line.number, "match arms must end with ':'"));
    };
    let pattern = pattern.trim();
    let Some(dot_offset) = pattern.find('.') else {
        return Err(diag(
            line.number,
            "enum match arms use 'Enum.Variant(bindings):' syntax",
        ));
    };
    let enum_name = pattern[..dot_offset].trim();
    validate_identifier(enum_name, line.number)?;
    let variant_source = &pattern[dot_offset + 1..];
    let Some(open_offset) = variant_source.find('(') else {
        return Err(diag(
            line.number,
            "enum match variants require parentheses, including payloadless variants",
        ));
    };
    let Some(inner) = variant_source.strip_suffix(')') else {
        return Err(diag(line.number, "match arm pattern must end with ')'"));
    };
    let variant = variant_source[..open_offset].trim();
    validate_identifier(variant, line.number)?;
    let bindings_source = &inner[open_offset + 1..];
    let enum_column = line.indent + 1 + pattern.find(enum_name).unwrap_or(0);
    let variant_column =
        line.indent + 1 + dot_offset + 1 + variant_source[..open_offset].find(variant).unwrap_or(0);
    let binding_base_column = line.indent + 1 + dot_offset + 1 + open_offset + 1;
    let mut bindings = Vec::new();
    if !bindings_source.trim().is_empty() {
        for (raw_binding, offset) in split_top_level_commas_with_offsets(bindings_source) {
            let (binding, column) = trim_with_column(raw_binding, binding_base_column + offset);
            validate_identifier(binding, line.number)?;
            if binding != "_"
                && bindings
                    .iter()
                    .any(|existing: &PatternBinding| existing.name == binding)
            {
                return Err(diag(
                    line.number,
                    &format!("duplicate match binding '{binding}'"),
                ));
            }
            bindings.push(PatternBinding {
                name: binding.to_string(),
                span: SourceSpan::new(line.number, column, binding.len()),
            });
        }
    }

    Ok(MatchArm {
        enum_name: enum_name.to_string(),
        enum_span: SourceSpan::new(line.number, enum_column, enum_name.len()),
        variant: variant.to_string(),
        variant_span: SourceSpan::new(line.number, variant_column, variant.len()),
        bindings,
        body: Vec::new(),
        line: line.number,
        span: line.span(),
    })
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
        let Some(eq_offset) = rest.find('=') else {
            return Err(diag(line, "let bindings require '= expression'"));
        };
        let binding_src = &rest[..eq_offset];
        let raw_expr_src = &rest[eq_offset + 1..];
        let raw_expr_column = span.column + 4 + eq_offset + 1;
        let (trimmed_expr, expr_column) = trim_with_column(raw_expr_src, raw_expr_column);
        let (expr_src, else_return) =
            if let Some(expr_src) = trimmed_expr.strip_suffix(" else return") {
                (expr_src.trim_end(), true)
            } else {
                (trimmed_expr, false)
            };
        if let Some((struct_name, struct_span, fields)) =
            parse_struct_destructure_pattern(binding_src, line, span.column + 4)?
        {
            if else_return {
                return Err(diag(
                    line,
                    "'else return' is only valid on multi-value call destructuring",
                ));
            }
            let expr = parse_expression_at(expr_src, line, expr_column)?;
            return Ok(Stmt {
                line,
                span,
                keyword_span: SourceSpan::new(line, span.column, 3),
                kind: StmtKind::LetStructDestructure {
                    struct_name,
                    struct_span,
                    fields,
                    expr,
                },
            });
        }

        let raw_bindings = split_top_level_commas_with_offsets(binding_src);
        if raw_bindings.len() == 1 {
            if else_return {
                return Err(diag(
                    line,
                    "'else return' requires a multi-value destructuring binding",
                ));
            }
            let (raw_binding, binding_offset) = raw_bindings[0];
            let (raw_binding, binding_column) =
                trim_with_column(raw_binding, span.column + 4 + binding_offset);
            let binding = parse_binding(raw_binding, line, binding_column)?;
            let expr = parse_expression_at(expr_src, line, expr_column)?;
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
        for (raw_binding, binding_offset) in raw_bindings {
            let (raw_binding, binding_column) =
                trim_with_column(raw_binding, span.column + 4 + binding_offset);
            let binding = parse_binding(raw_binding, line, binding_column)?;
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
        let expr = parse_expression_at(expr_src, line, expr_column)?;
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
        let expression_base_column = span.column + 7;
        let expressions = split_top_level_commas_with_offsets(expr_src)
            .into_iter()
            .map(|(raw_part, offset)| {
                let (part, column) = trim_with_column(raw_part, expression_base_column + offset);
                parse_expression_at(part, line, column)
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Stmt {
            line,
            span,
            keyword_span: SourceSpan::new(line, span.column, 6),
            kind: StmtKind::Return(expressions),
        });
    }

    let expr = parse_expression_at(input, line, span.column)?;
    Ok(Stmt {
        line,
        span,
        keyword_span: expr.span,
        kind: StmtKind::Expr(expr),
    })
}

fn parse_struct_destructure_pattern(
    input: &str,
    line: usize,
    column: usize,
) -> Result<Option<(String, SourceSpan, Vec<StructPatternField>)>, Diagnostic> {
    let trimmed = input.trim();
    let leading = input.len() - input.trim_start().len();
    let pattern_column = column + leading;
    let Some(open_offset) = trimmed.find('{') else {
        return Ok(None);
    };
    let Some(inner) = trimmed.strip_suffix('}') else {
        return Ok(None);
    };
    let struct_name = trimmed[..open_offset].trim();
    if struct_name.is_empty() {
        return Ok(None);
    }
    validate_identifier(struct_name, line)?;
    let struct_offset = trimmed[..open_offset]
        .find(struct_name)
        .expect("trimmed struct pattern contains its name");
    let struct_span = SourceSpan::new(line, pattern_column + struct_offset, struct_name.len());
    let inner = &inner[open_offset + 1..];
    if inner.trim().is_empty() {
        return Err(diag(
            line,
            "struct destructuring requires at least one field",
        ));
    }

    let mut fields = Vec::new();
    for (raw_field, offset) in split_top_level_commas_with_offsets(inner) {
        let field_column = pattern_column + open_offset + 1 + offset;
        let (entry, entry_column) = trim_with_column(raw_field, field_column);
        if entry.is_empty() {
            return Err(diag(
                line,
                "struct destructuring contains an empty field pattern",
            ));
        }
        let (field_name, field_name_column, binding_name, binding_column) =
            if let Some(colon_offset) = entry.find(':') {
                let (field_name, field_name_column) =
                    trim_with_column(&entry[..colon_offset], entry_column);
                let (binding_name, binding_column) =
                    trim_with_column(&entry[colon_offset + 1..], entry_column + colon_offset + 1);
                (field_name, field_name_column, binding_name, binding_column)
            } else {
                (entry, entry_column, entry, entry_column)
            };
        validate_identifier(field_name, line)?;
        if binding_name != "_" {
            validate_identifier(binding_name, line)?;
        }
        if fields
            .iter()
            .any(|existing: &StructPatternField| existing.field == field_name)
        {
            return Err(diag(
                line,
                &format!("duplicate struct pattern field '{field_name}'"),
            ));
        }
        if binding_name != "_"
            && fields
                .iter()
                .any(|existing: &StructPatternField| existing.binding.name == binding_name)
        {
            return Err(diag(
                line,
                &format!("duplicate struct pattern binding '{binding_name}'"),
            ));
        }
        fields.push(StructPatternField {
            field: field_name.to_string(),
            field_span: SourceSpan::new(line, field_name_column, field_name.len()),
            binding: PatternBinding {
                name: binding_name.to_string(),
                span: SourceSpan::new(line, binding_column, binding_name.len()),
            },
        });
    }

    Ok(Some((struct_name.to_string(), struct_span, fields)))
}

fn parse_binding(input: &str, line: usize, column: usize) -> Result<Binding, Diagnostic> {
    let Some(colon_offset) = input.find(':') else {
        return Err(diag(
            line,
            "strict bindings require an explicit type: 'let name: type = value'",
        ));
    };
    let name_src = &input[..colon_offset];
    let ty_src = &input[colon_offset + 1..];
    let (name, name_column) = trim_with_column(name_src, column);
    validate_identifier(name, line)?;
    let (type_text, type_column) = trim_with_column(ty_src, column + colon_offset + 1);
    let ty = parse_type(type_text, line)?;
    if ty == Type::Void {
        return Err(diag(line, "variables cannot have type void"));
    }
    Ok(Binding {
        name: name.to_string(),
        name_span: SourceSpan::new(line, name_column, name.len()),
        ty,
        type_span: SourceSpan::new(line, type_column, type_text.len()),
    })
}

fn split_top_level_commas(input: &str) -> Vec<&str> {
    split_top_level_commas_with_offsets(input)
        .into_iter()
        .map(|(part, _)| part)
        .collect()
}

fn trim_with_column(input: &str, column: usize) -> (&str, usize) {
    let leading = input.len() - input.trim_start().len();
    (input.trim(), column + leading)
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
            b'(' | b'{' => depth += 1,
            b')' | b'}' => depth = depth.saturating_sub(1),
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

fn split_range_with_offset(input: &str) -> Option<(&str, &str, usize)> {
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
                    return Some((&input[..index], &input[index + 2..], index));
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
        "fn" | "struct"
            | "enum"
            | "type"
            | "const"
            | "let"
            | "match"
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
    LBrace,
    RBrace,
    Colon,
    Dot,
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
                b'{' => (TokenKind::LBrace, 1),
                b'}' => (TokenKind::RBrace, 1),
                b':' => (TokenKind::Colon, 1),
                b'.' => (TokenKind::Dot, 1),
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
        let mut expr = self.parse_atom()?;
        while matches!(
            self.tokens.get(self.index).map(|token| &token.kind),
            Some(TokenKind::Dot)
        ) {
            self.index += 1;
            let Some(field) = self.tokens.get(self.index).cloned() else {
                return Err(diag(self.line, "expected field name after '.'"));
            };
            let TokenKind::Ident(name) = field.kind else {
                return Err(Diagnostic::new(
                    DiagnosticStage::Parse,
                    field.span,
                    "expected field name after '.'",
                ));
            };
            self.index += 1;
            if matches!(
                self.tokens.get(self.index).map(|token| &token.kind),
                Some(TokenKind::LParen)
            ) {
                let ExprKind::Var(enum_name) = &expr.kind else {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        field.span,
                        "only enum variants may be called through '.'",
                    ));
                };
                let enum_name = enum_name.clone();
                let enum_span = expr.span;
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
                                    "expected ',' or ')' in enum variant payload",
                                ));
                            }
                        }
                    }
                }
                let Some(close) = self.tokens.get(self.index) else {
                    return Err(diag(self.line, "expected ')' after enum variant payload"));
                };
                if !matches!(close.kind, TokenKind::RParen) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        close.span,
                        "expected ')' after enum variant payload",
                    ));
                }
                let close_span = close.span;
                self.index += 1;
                expr = Expr {
                    line: self.line,
                    span: SourceSpan::new(
                        self.line,
                        enum_span.column,
                        close_span.column + close_span.length - enum_span.column,
                    ),
                    kind: ExprKind::EnumVariant {
                        enum_name,
                        enum_span,
                        variant: name,
                        variant_span: field.span,
                        args,
                    },
                };
                continue;
            }
            let span = SourceSpan::new(
                self.line,
                expr.span.column,
                field.span.column + field.span.length - expr.span.column,
            );
            expr = Expr {
                line: self.line,
                span,
                kind: ExprKind::Field {
                    base: Box::new(expr),
                    name,
                    name_span: field.span,
                },
            };
        }
        Ok(expr)
    }

    fn parse_atom(&mut self) -> Result<Expr, Diagnostic> {
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
                if matches!(
                    self.tokens.get(self.index).map(|token| &token.kind),
                    Some(TokenKind::LBrace)
                ) {
                    return self.parse_struct_literal(name, token_span);
                }
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

    fn parse_struct_literal(
        &mut self,
        name: String,
        name_span: SourceSpan,
    ) -> Result<Expr, Diagnostic> {
        self.index += 1;
        let mut base = None;
        let mut fields = Vec::new();
        if matches!(
            (
                self.tokens.get(self.index).map(|token| &token.kind),
                self.tokens.get(self.index + 1).map(|token| &token.kind),
            ),
            (Some(TokenKind::Dot), Some(TokenKind::Dot))
        ) {
            self.index += 2;
            base = Some(Box::new(self.parse_binary(1)?));
            match self.tokens.get(self.index).map(|token| &token.kind) {
                Some(TokenKind::Comma) => self.index += 1,
                Some(TokenKind::RBrace) => {}
                _ => {
                    return Err(diag(
                        self.line,
                        "expected ',' or '}' after struct update base",
                    ));
                }
            }
        }
        if !matches!(
            self.tokens.get(self.index).map(|token| &token.kind),
            Some(TokenKind::RBrace)
        ) {
            loop {
                let Some(field_token) = self.tokens.get(self.index).cloned() else {
                    return Err(diag(self.line, "expected struct field name"));
                };
                let TokenKind::Ident(field_name) = field_token.kind else {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        field_token.span,
                        "expected struct field name",
                    ));
                };
                self.index += 1;
                let Some(colon) = self.tokens.get(self.index) else {
                    return Err(diag(self.line, "expected ':' after struct field name"));
                };
                if !matches!(colon.kind, TokenKind::Colon) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        colon.span,
                        "expected ':' after struct field name",
                    ));
                }
                self.index += 1;
                let value = self.parse_binary(1)?;
                fields.push(StructLiteralField {
                    name: field_name,
                    name_span: field_token.span,
                    value,
                });
                match self.tokens.get(self.index).map(|token| &token.kind) {
                    Some(TokenKind::Comma) => {
                        self.index += 1;
                        if matches!(
                            self.tokens.get(self.index).map(|token| &token.kind),
                            Some(TokenKind::RBrace)
                        ) {
                            break;
                        }
                    }
                    Some(TokenKind::RBrace) => break,
                    _ => return Err(diag(self.line, "expected ',' or '}' in struct literal")),
                }
            }
        }
        let Some(close) = self.tokens.get(self.index) else {
            return Err(diag(self.line, "expected '}' after struct literal"));
        };
        if !matches!(close.kind, TokenKind::RBrace) {
            return Err(Diagnostic::new(
                DiagnosticStage::Parse,
                close.span,
                "expected '}' after struct literal",
            ));
        }
        let close_span = close.span;
        self.index += 1;
        Ok(Expr {
            line: self.line,
            span: SourceSpan::new(
                self.line,
                name_span.column,
                close_span.column + close_span.length - name_span.column,
            ),
            kind: ExprKind::StructLiteral {
                name,
                name_span,
                base,
                fields,
            },
        })
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
