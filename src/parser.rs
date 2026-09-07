use crate::ast::{
    BinOp, Binding, Expr, ExprKind, Function, Param, Program, Stmt, StmtKind, Type, UnaryOp,
};

#[derive(Debug, Clone)]
struct Line {
    number: usize,
    indent: usize,
    text: String,
}

pub fn parse(source: &str) -> Result<Program, String> {
    let lines = preprocess(source)?;
    let mut functions = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = &lines[index];
        if line.indent != 0 {
            return Err(diag(
                line.number,
                "top-level declarations must not be indented",
            ));
        }

        let (name, params, returns) = parse_function_header(&line.text, line.number)?;
        let function_line = line.number;
        index += 1;

        let body = if index < lines.len() && lines[index].indent > 0 {
            let body_indent = lines[index].indent;
            parse_block(&lines, &mut index, body_indent)?
        } else {
            Vec::new()
        };

        if index >= lines.len() || lines[index].indent != 0 || lines[index].text != "}" {
            return Err(diag(
                function_line,
                "function body must end with a top-level '}'",
            ));
        }
        index += 1;

        functions.push(Function {
            name,
            params,
            returns,
            body,
            line: function_line,
        });
    }

    if functions.is_empty() {
        return Err("Flux source contains no functions".to_string());
    }

    Ok(Program { functions })
}

fn preprocess(source: &str) -> Result<Vec<Line>, String> {
    let mut lines = Vec::new();
    for (index, raw) in source.lines().enumerate() {
        let number = index + 1;
        if raw.contains('\t') {
            return Err(diag(
                number,
                "tabs are not allowed for indentation; use spaces",
            ));
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
    Ok(lines)
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

fn parse_function_header(
    input: &str,
    line: usize,
) -> Result<(String, Vec<Param>, Vec<Type>), String> {
    let Some(rest) = input.strip_prefix("fn ") else {
        return Err(diag(
            line,
            "expected function declaration starting with 'fn'",
        ));
    };
    let Some(open) = rest.find('(') else {
        return Err(diag(line, "expected '(' after function name"));
    };
    let name = rest[..open].trim();
    validate_identifier(name, line)?;

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
        return Err(diag(line, "functions must open their body with '{'"));
    };
    let returns = parse_return_types(ret_src.trim(), line)?;

    let mut params = Vec::new();
    if !params_src.trim().is_empty() {
        for raw_param in params_src.split(',') {
            let Some((raw_name, raw_ty)) = raw_param.split_once(':') else {
                return Err(diag(line, "parameters use 'name: type' syntax"));
            };
            let param_name = raw_name.trim();
            validate_identifier(param_name, line)?;
            let ty = parse_type(raw_ty.trim(), line)?;
            if ty == Type::Void {
                return Err(diag(line, "parameters cannot have type void"));
            }
            if params.iter().any(|param: &Param| param.name == param_name) {
                return Err(diag(line, &format!("duplicate parameter '{param_name}'")));
            }
            params.push(Param {
                name: param_name.to_string(),
                ty,
            });
        }
    }

    Ok((name.to_string(), params, returns))
}

fn parse_block(lines: &[Line], index: &mut usize, indent: usize) -> Result<Vec<Stmt>, String> {
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
            let cond_src = line.text[3..line.text.len() - 1].trim();
            if cond_src.is_empty() {
                return Err(diag(line.number, "if requires a condition"));
            }
            let cond = parse_expression(cond_src, line.number)?;
            let stmt_line = line.number;
            *index += 1;
            let nested = parse_nested_block(lines, index, indent, stmt_line, "if")?;
            Stmt {
                line: stmt_line,
                kind: StmtKind::If { cond, body: nested },
            }
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
            let start = parse_expression(start_src.trim(), stmt_line)?;
            let end = parse_expression(end_src.trim(), stmt_line)?;
            *index += 1;
            let nested = parse_nested_block(lines, index, indent, stmt_line, "for")?;
            Stmt {
                line: stmt_line,
                kind: StmtKind::ForRange {
                    name: name.to_string(),
                    start,
                    end,
                    body: nested,
                },
            }
        } else {
            let stmt = parse_simple_statement(&line.text, line.number)?;
            *index += 1;
            stmt
        };

        body.push(stmt);
    }

    Ok(body)
}

fn parse_nested_block(
    lines: &[Line],
    index: &mut usize,
    parent_indent: usize,
    parent_line: usize,
    kind: &str,
) -> Result<Vec<Stmt>, String> {
    if *index >= lines.len() || lines[*index].indent <= parent_indent {
        return Err(diag(
            parent_line,
            &format!("{kind} requires an indented body"),
        ));
    }
    let nested_indent = lines[*index].indent;
    parse_block(lines, index, nested_indent)
}

fn parse_simple_statement(input: &str, line: usize) -> Result<Stmt, String> {
    if let Some(rest) = input.strip_prefix("let ") {
        let Some((binding_src, expr_src)) = rest.split_once('=') else {
            return Err(diag(line, "let bindings require '= expression'"));
        };
        let raw_bindings = split_top_level_commas(binding_src);
        if raw_bindings.len() == 1 {
            let binding = parse_binding(raw_bindings[0], line)?;
            let expr = parse_expression(expr_src.trim(), line)?;
            return Ok(Stmt {
                line,
                kind: StmtKind::Let {
                    name: binding.name,
                    ty: binding.ty,
                    expr,
                },
            });
        }

        let mut bindings = Vec::with_capacity(raw_bindings.len());
        for raw_binding in raw_bindings {
            let binding = parse_binding(raw_binding, line)?;
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
        let expr = parse_expression(expr_src.trim(), line)?;
        return Ok(Stmt {
            line,
            kind: StmtKind::LetDestructure { bindings, expr },
        });
    }

    if input == "return" {
        return Ok(Stmt {
            line,
            kind: StmtKind::Return(Vec::new()),
        });
    }
    if let Some(expr_src) = input.strip_prefix("return ") {
        let expressions = split_top_level_commas(expr_src)
            .into_iter()
            .map(|part| parse_expression(part.trim(), line))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Stmt {
            line,
            kind: StmtKind::Return(expressions),
        });
    }

    Ok(Stmt {
        line,
        kind: StmtKind::Expr(parse_expression(input, line)?),
    })
}

fn parse_binding(input: &str, line: usize) -> Result<Binding, String> {
    let Some((name_src, ty_src)) = input.split_once(':') else {
        return Err(diag(
            line,
            "strict bindings require an explicit type: 'let name: type = value'",
        ));
    };
    let name = name_src.trim();
    validate_identifier(name, line)?;
    let ty = parse_type(ty_src.trim(), line)?;
    if ty == Type::Void {
        return Err(diag(line, "variables cannot have type void"));
    }
    Ok(Binding {
        name: name.to_string(),
        ty,
    })
}

fn split_top_level_commas(input: &str) -> Vec<&str> {
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
                parts.push(&input[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&input[start..]);
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

fn parse_return_types(input: &str, line: usize) -> Result<Vec<Type>, String> {
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

fn parse_type(input: &str, line: usize) -> Result<Type, String> {
    Type::parse(input).ok_or_else(|| diag(line, &format!("unknown type '{input}'")))
}

fn validate_identifier(input: &str, line: usize) -> Result<(), String> {
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
        "fn" | "let" | "return" | "if" | "for" | "in" | "true" | "false"
    ) {
        return Err(diag(line, &format!("'{input}' is reserved")));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Int(i64),
    Str(String),
    Ident(String),
    True,
    False,
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

fn parse_expression(input: &str, line: usize) -> Result<Expr, String> {
    let tokens = lex_expression(input, line)?;
    let mut parser = ExprParser {
        tokens: &tokens,
        index: 0,
        line,
    };
    let expr = parser.parse_binary(1)?;
    if parser.index != tokens.len() {
        return Err(diag(line, "unexpected token after expression"));
    }
    Ok(expr)
}

fn lex_expression(input: &str, line: usize) -> Result<Vec<Token>, String> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0usize;

    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }

        if byte.is_ascii_digit() {
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            let value = input[start..index]
                .parse::<i64>()
                .map_err(|_| diag(line, "integer literal is outside i64 range"))?;
            tokens.push(Token::Int(value));
            continue;
        }

        if byte == b'"' {
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
                                return Err(diag(
                                    line,
                                    &format!("unsupported escape '\\{}'", other as char),
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
                return Err(diag(line, "unterminated string literal"));
            }
            tokens.push(Token::Str(value));
            continue;
        }

        if byte == b'_' || byte.is_ascii_alphabetic() {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index] == b'_' || bytes[index].is_ascii_alphanumeric())
            {
                index += 1;
            }
            let ident = &input[start..index];
            tokens.push(match ident {
                "true" => Token::True,
                "false" => Token::False,
                _ => Token::Ident(ident.to_string()),
            });
            continue;
        }

        let (token, width) = match byte {
            b'+' => (Token::Plus, 1),
            b'-' => (Token::Minus, 1),
            b'*' => (Token::Star, 1),
            b'/' => (Token::Slash, 1),
            b'(' => (Token::LParen, 1),
            b')' => (Token::RParen, 1),
            b',' => (Token::Comma, 1),
            b'!' if bytes.get(index + 1) == Some(&b'=') => (Token::NotEq, 2),
            b'!' => (Token::Bang, 1),
            b'=' if bytes.get(index + 1) == Some(&b'=') => (Token::EqEq, 2),
            b'<' if bytes.get(index + 1) == Some(&b'=') => (Token::Le, 2),
            b'<' => (Token::Lt, 1),
            b'>' if bytes.get(index + 1) == Some(&b'=') => (Token::Ge, 2),
            b'>' => (Token::Gt, 1),
            b'&' if bytes.get(index + 1) == Some(&b'&') => (Token::AndAnd, 2),
            b'|' if bytes.get(index + 1) == Some(&b'|') => (Token::OrOr, 2),
            other => {
                return Err(diag(
                    line,
                    &format!("unexpected character '{}' in expression", other as char),
                ));
            }
        };
        tokens.push(token);
        index += width;
    }

    if tokens.is_empty() {
        return Err(diag(line, "expected expression"));
    }
    Ok(tokens)
}

struct ExprParser<'a> {
    tokens: &'a [Token],
    index: usize,
    line: usize,
}

impl ExprParser<'_> {
    fn parse_binary(&mut self, min_precedence: u8) -> Result<Expr, String> {
        let mut left = self.parse_unary()?;
        while let Some((op, precedence)) = self.peek_binary() {
            if precedence < min_precedence {
                break;
            }
            self.index += 1;
            let right = self.parse_binary(precedence + 1)?;
            left = Expr {
                line: self.line,
                kind: ExprKind::Binary {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if matches!(self.tokens.get(self.index), Some(Token::Minus)) {
            self.index += 1;
            let expr = self.parse_unary()?;
            return Ok(Expr {
                line: self.line,
                kind: ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                },
            });
        }
        if matches!(self.tokens.get(self.index), Some(Token::Bang)) {
            self.index += 1;
            let expr = self.parse_unary()?;
            return Ok(Expr {
                line: self.line,
                kind: ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                },
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        let Some(token) = self.tokens.get(self.index).cloned() else {
            return Err(diag(self.line, "expected expression"));
        };
        self.index += 1;

        match token {
            Token::Int(value) => Ok(Expr {
                line: self.line,
                kind: ExprKind::Int(value),
            }),
            Token::Str(value) => Ok(Expr {
                line: self.line,
                kind: ExprKind::Str(value),
            }),
            Token::True => Ok(Expr {
                line: self.line,
                kind: ExprKind::Bool(true),
            }),
            Token::False => Ok(Expr {
                line: self.line,
                kind: ExprKind::Bool(false),
            }),
            Token::Ident(name) => {
                if !matches!(self.tokens.get(self.index), Some(Token::LParen)) {
                    return Ok(Expr {
                        line: self.line,
                        kind: ExprKind::Var(name),
                    });
                }
                self.index += 1;
                let mut args = Vec::new();
                if !matches!(self.tokens.get(self.index), Some(Token::RParen)) {
                    loop {
                        args.push(self.parse_binary(1)?);
                        match self.tokens.get(self.index) {
                            Some(Token::Comma) => self.index += 1,
                            Some(Token::RParen) => break,
                            _ => {
                                return Err(diag(
                                    self.line,
                                    "expected ',' or ')' in call arguments",
                                ));
                            }
                        }
                    }
                }
                if !matches!(self.tokens.get(self.index), Some(Token::RParen)) {
                    return Err(diag(self.line, "expected ')' after call arguments"));
                }
                self.index += 1;
                Ok(Expr {
                    line: self.line,
                    kind: ExprKind::Call { name, args },
                })
            }
            Token::LParen => {
                let expr = self.parse_binary(1)?;
                if !matches!(self.tokens.get(self.index), Some(Token::RParen)) {
                    return Err(diag(self.line, "expected ')'"));
                }
                self.index += 1;
                Ok(expr)
            }
            _ => Err(diag(self.line, "expected expression")),
        }
    }

    fn peek_binary(&self) -> Option<(BinOp, u8)> {
        Some(match self.tokens.get(self.index)? {
            Token::OrOr => (BinOp::Or, 1),
            Token::AndAnd => (BinOp::And, 2),
            Token::EqEq => (BinOp::Eq, 3),
            Token::NotEq => (BinOp::Ne, 3),
            Token::Lt => (BinOp::Lt, 4),
            Token::Le => (BinOp::Le, 4),
            Token::Gt => (BinOp::Gt, 4),
            Token::Ge => (BinOp::Ge, 4),
            Token::Plus => (BinOp::Add, 5),
            Token::Minus => (BinOp::Sub, 5),
            Token::Star => (BinOp::Mul, 6),
            Token::Slash => (BinOp::Div, 6),
            _ => return None,
        })
    }
}

fn diag(line: usize, message: &str) -> String {
    format!("line {line}: {message}")
}
