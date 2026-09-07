use std::collections::HashMap;

use crate::ast::{BinOp, Expr, ExprKind, Function, Stmt, StmtKind, Type, UnaryOp};
use crate::diagnostic::Diagnostic;
use crate::parser;

pub fn format_source(source: &str) -> Result<String, Vec<Diagnostic>> {
    let program = parser::parse_all(source)?;
    let mut formatted = HashMap::new();
    for function in &program.functions {
        format_function(function, &mut formatted);
    }

    let raw_lines = source.lines().collect::<Vec<_>>();
    let mut output = Vec::with_capacity(raw_lines.len());
    let mut blank_pending = false;

    for (index, raw) in raw_lines.iter().enumerate() {
        let line_number = index + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            blank_pending = !output.is_empty();
            continue;
        }

        if blank_pending {
            output.push(String::new());
            blank_pending = false;
        }

        if let Some(code) = formatted.get(&line_number) {
            let comment = comment_suffix(raw);
            if let Some(comment) = comment {
                output.push(format!("{code} {comment}"));
            } else {
                output.push(code.clone());
            }
            continue;
        }

        if trimmed == "}" {
            output.push("}".to_string());
            continue;
        }

        if trimmed.starts_with('#') {
            output.push(trimmed.to_string());
            continue;
        }

        output.push(raw.trim_end().to_string());
    }

    while output.last().is_some_and(String::is_empty) {
        output.pop();
    }

    Ok(format!("{}\n", output.join("\n")))
}

fn format_function(function: &Function, lines: &mut HashMap<usize, String>) {
    let params = function
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, param.ty.name()))
        .collect::<Vec<_>>()
        .join(", ");
    lines.insert(
        function.line,
        format!(
            "fn {}({params}) -> {} {{",
            function.name,
            format_return_types(&function.returns)
        ),
    );
    format_block(&function.body, 1, lines);
}

fn format_block(body: &[Stmt], depth: usize, lines: &mut HashMap<usize, String>) {
    for stmt in body {
        let pad = "    ".repeat(depth);
        match &stmt.kind {
            StmtKind::Let { name, ty, expr, .. } => {
                lines.insert(
                    stmt.line,
                    format!("{pad}let {name}: {} = {}", ty.name(), format_expr(expr, 0)),
                );
            }
            StmtKind::LetDestructure {
                bindings,
                expr,
                else_return,
            } => {
                let bindings = bindings
                    .iter()
                    .map(|binding| format!("{}: {}", binding.name, binding.ty.name()))
                    .collect::<Vec<_>>()
                    .join(", ");
                let suffix = if *else_return { " else return" } else { "" };
                lines.insert(
                    stmt.line,
                    format!("{pad}let {bindings} = {}{suffix}", format_expr(expr, 0)),
                );
            }
            StmtKind::Return(values) => {
                if values.is_empty() {
                    lines.insert(stmt.line, format!("{pad}return"));
                } else {
                    let values = values
                        .iter()
                        .map(|expr| format_expr(expr, 0))
                        .collect::<Vec<_>>()
                        .join(", ");
                    lines.insert(stmt.line, format!("{pad}return {values}"));
                }
            }
            StmtKind::Expr(expr) => {
                lines.insert(stmt.line, format!("{pad}{}", format_expr(expr, 0)));
            }
            StmtKind::If {
                cond,
                body,
                else_body,
                else_keyword_span,
            } => {
                let keyword = if stmt.keyword_span.length == 4 {
                    "elif"
                } else {
                    "if"
                };
                lines.insert(
                    stmt.line,
                    format!("{pad}{keyword} {}:", format_expr(cond, 0)),
                );
                format_block(body, depth + 1, lines);
                if !else_body.is_empty() {
                    let nested_elif = else_keyword_span.is_some_and(|span| {
                        else_body.first().is_some_and(|stmt| {
                            stmt.line == span.line && stmt.keyword_span.length == 4
                        })
                    });
                    if let Some(span) = else_keyword_span
                        && !nested_elif
                    {
                        lines.insert(span.line, format!("{pad}else:"));
                    }
                    format_block(
                        else_body,
                        if nested_elif { depth } else { depth + 1 },
                        lines,
                    );
                }
            }
            StmtKind::ForRange {
                name,
                start,
                end,
                body,
                ..
            } => {
                lines.insert(
                    stmt.line,
                    format!(
                        "{pad}for {name} in {}..{}:",
                        format_expr(start, 0),
                        format_expr(end, 0)
                    ),
                );
                format_block(body, depth + 1, lines);
            }
        }
    }
}

fn format_return_types(types: &[Type]) -> String {
    match types {
        [] => "void".to_string(),
        [ty] => ty.name().to_string(),
        _ => format!(
            "({})",
            types.iter().map(Type::name).collect::<Vec<_>>().join(", ")
        ),
    }
}

fn format_expr(expr: &Expr, parent_precedence: u8) -> String {
    match &expr.kind {
        ExprKind::Int(value) => value.to_string(),
        ExprKind::Bool(value) => value.to_string(),
        ExprKind::Str(value) => format_string(value),
        ExprKind::Nil => "nil".to_string(),
        ExprKind::Var(name) => name.clone(),
        ExprKind::Call { name, args } => format!(
            "{name}({})",
            args.iter()
                .map(|arg| format_expr(arg, 0))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExprKind::Unary { op, expr } => {
            let operator = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            };
            format!("{operator}{}", format_expr(expr, 7))
        }
        ExprKind::Binary { left, op, right } => {
            let precedence = binary_precedence(*op);
            let text = format!(
                "{} {} {}",
                format_expr(left, precedence),
                binary_text(*op),
                format_expr(right, precedence + 1)
            );
            if precedence < parent_precedence {
                format!("({text})")
            } else {
                text
            }
        }
    }
}

fn binary_precedence(op: BinOp) -> u8 {
    match op {
        BinOp::Or => 1,
        BinOp::And => 2,
        BinOp::Eq | BinOp::Ne => 3,
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 4,
        BinOp::Add | BinOp::Sub => 5,
        BinOp::Mul | BinOp::Div => 6,
    }
}

fn binary_text(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}

fn format_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn comment_suffix(line: &str) -> Option<&str> {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
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
        } else if ch == '#' && !in_string {
            return Some(line[index..].trim_end());
        }
    }
    None
}
