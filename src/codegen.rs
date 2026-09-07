use std::collections::HashMap;

use crate::ast::{BinOp, Expr, ExprKind, Function, Program, Stmt, StmtKind, Type, UnaryOp};
use crate::typecheck::{Signatures, type_of_expr};

pub fn emit_c(program: &Program, signatures: &Signatures) -> Result<String, String> {
    let mut out = String::new();
    out.push_str("#include <stdbool.h>\n");
    out.push_str("#include <stdint.h>\n");
    out.push_str("#include <stdio.h>\n");
    out.push_str("#include <stdlib.h>\n");
    out.push_str("#include <string.h>\n\n");
    out.push_str("static inline void flux_print_i64(int64_t value) { printf(\"%lld\\n\", (long long)value); }\n");
    out.push_str(
        "static inline void flux_print_bool(bool value) { puts(value ? \"true\" : \"false\"); }\n",
    );
    out.push_str("static inline void flux_print_str(const char *value) { puts(value); }\n");
    out.push_str("static inline int64_t flux_div_i64(int64_t a, int64_t b) {\n");
    out.push_str("    if (b == 0 || (a == INT64_MIN && b == -1)) { fputs(\"Flux runtime error: invalid integer division\\n\", stderr); abort(); }\n");
    out.push_str("    return a / b;\n");
    out.push_str("}\n\n");

    for function in &program.functions {
        if function.returns.len() > 1 {
            let tag = multi_return_struct_name(&function.name);
            out.push_str(&format!("struct {tag} {{\n"));
            for (index, ty) in function.returns.iter().enumerate() {
                out.push_str(&format!("    {} v{index};\n", c_type(ty)));
            }
            out.push_str("};\n");
        }
    }
    if program
        .functions
        .iter()
        .any(|function| function.returns.len() > 1)
    {
        out.push('\n');
    }

    for function in &program.functions {
        out.push_str(&function_prototype(function));
        out.push_str(";\n");
    }
    out.push('\n');

    let mut temp_counter = 0usize;
    for function in &program.functions {
        emit_function(&mut out, function, signatures, &mut temp_counter)?;
        out.push('\n');
    }

    Ok(out)
}

fn function_prototype(function: &Function) -> String {
    let ret = if function.name == "main" {
        "int".to_string()
    } else {
        c_function_return_type(function)
    };
    let params = if function.params.is_empty() {
        "void".to_string()
    } else {
        function
            .params
            .iter()
            .map(|param| format!("{} {}", c_type(&param.ty), param.name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!("{ret} {}({params})", function.name)
}

fn emit_function(
    out: &mut String,
    function: &Function,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<(), String> {
    out.push_str(&function_prototype(function));
    out.push_str(" {\n");
    let mut env = HashMap::new();
    for param in &function.params {
        env.insert(param.name.clone(), param.ty.clone());
    }
    emit_block(
        out,
        &function.body,
        1,
        &mut env,
        signatures,
        temp_counter,
        function,
    )?;
    out.push_str("}\n");
    Ok(())
}

fn emit_block(
    out: &mut String,
    body: &[Stmt],
    depth: usize,
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
    current_function: &Function,
) -> Result<(), String> {
    for stmt in body {
        let pad = "    ".repeat(depth);
        match &stmt.kind {
            StmtKind::Let { name, ty, expr } => {
                let value = emit_expr(expr, env, signatures)?;
                out.push_str(&format!("{pad}{} {name} = {};\n", c_type(ty), value.code));
                env.insert(name.clone(), ty.clone());
            }
            StmtKind::LetDestructure { bindings, expr } => {
                let (value, tag) = emit_multi_expr(expr, env, signatures)?;
                let temp = format!("flux__multi_{}", *temp_counter);
                *temp_counter += 1;
                out.push_str(&format!("{pad}struct {tag} {temp} = {value};\n"));
                for (index, binding) in bindings.iter().enumerate() {
                    out.push_str(&format!(
                        "{pad}{} {} = {temp}.v{index};\n",
                        c_type(&binding.ty),
                        binding.name
                    ));
                    env.insert(binding.name.clone(), binding.ty.clone());
                }
            }
            StmtKind::Return(values) if values.is_empty() => {
                out.push_str(&format!("{pad}return;\n"));
            }
            StmtKind::Return(values) if values.len() == 1 => {
                let value = emit_expr(&values[0], env, signatures)?;
                out.push_str(&format!("{pad}return {};\n", value.code));
            }
            StmtKind::Return(values) => {
                let tag = multi_return_struct_name(&current_function.name);
                let temp = format!("flux__return_{}", *temp_counter);
                *temp_counter += 1;
                out.push_str(&format!("{pad}struct {tag} {temp};\n"));
                for (index, expr) in values.iter().enumerate() {
                    let value = emit_expr(expr, env, signatures)?;
                    out.push_str(&format!("{pad}{temp}.v{index} = {};\n", value.code));
                }
                out.push_str(&format!("{pad}return {temp};\n"));
            }
            StmtKind::Expr(expr) => {
                let value = emit_expr(expr, env, signatures)?;
                out.push_str(&format!("{pad}{};\n", value.code));
            }
            StmtKind::If { cond, body } => {
                let cond = emit_expr(cond, env, signatures)?;
                out.push_str(&format!("{pad}if {} {{\n", c_condition(&cond.code)));
                let mut nested = env.clone();
                emit_block(
                    out,
                    body,
                    depth + 1,
                    &mut nested,
                    signatures,
                    temp_counter,
                    current_function,
                )?;
                out.push_str(&format!("{pad}}}\n"));
            }
            StmtKind::ForRange {
                name,
                start,
                end,
                body,
            } => {
                let start = emit_expr(start, env, signatures)?;
                let end = emit_expr(end, env, signatures)?;
                let temp = format!("flux__end_{}", *temp_counter);
                *temp_counter += 1;
                out.push_str(&format!(
                    "{pad}for (int64_t {name} = {}, {temp} = {}; {name} < {temp}; ++{name}) {{\n",
                    start.code, end.code
                ));
                let mut nested = env.clone();
                nested.insert(name.clone(), Type::I64);
                emit_block(
                    out,
                    body,
                    depth + 1,
                    &mut nested,
                    signatures,
                    temp_counter,
                    current_function,
                )?;
                out.push_str(&format!("{pad}}}\n"));
            }
        }
    }
    Ok(())
}

struct EmittedExpr {
    code: String,
    ty: Type,
}

fn emit_expr(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<EmittedExpr, String> {
    let emitted = match &expr.kind {
        ExprKind::Int(value) => EmittedExpr {
            code: format!("INT64_C({value})"),
            ty: Type::I64,
        },
        ExprKind::Bool(value) => EmittedExpr {
            code: if *value { "true" } else { "false" }.to_string(),
            ty: Type::Bool,
        },
        ExprKind::Str(value) => EmittedExpr {
            code: c_string(value),
            ty: Type::Str,
        },
        ExprKind::Var(name) => EmittedExpr {
            code: name.clone(),
            ty: env.get(name).cloned().ok_or_else(|| {
                format!(
                    "line {}: unknown binding '{name}' during code generation",
                    expr.line
                )
            })?,
        },
        ExprKind::Call { name, args } if name == "print" => {
            let arg = emit_expr(&args[0], env, signatures)?;
            let helper = match arg.ty {
                Type::I64 => "flux_print_i64",
                Type::Bool => "flux_print_bool",
                Type::Str => "flux_print_str",
                Type::Void => return Err(format!("line {}: cannot print void", expr.line)),
            };
            EmittedExpr {
                code: format!("{helper}({})", arg.code),
                ty: Type::Void,
            }
        }
        ExprKind::Call { name, args } => {
            let signature = signatures.get(name).ok_or_else(|| {
                format!(
                    "line {}: unknown function '{name}' during code generation",
                    expr.line
                )
            })?;
            let mut rendered = Vec::with_capacity(args.len());
            for arg in args {
                rendered.push(emit_expr(arg, env, signatures)?.code);
            }
            let ty = match signature.returns.as_slice() {
                [] => Type::Void,
                [ty] => ty.clone(),
                _ => {
                    return Err(format!(
                        "line {}: multi-value call '{name}' requires destructuring during code generation",
                        expr.line
                    ));
                }
            };
            EmittedExpr {
                code: format!("{name}({})", rendered.join(", ")),
                ty,
            }
        }
        ExprKind::Unary { op, expr: inner } => {
            let inner = emit_expr(inner, env, signatures)?;
            EmittedExpr {
                code: format!(
                    "({}{})",
                    match op {
                        UnaryOp::Neg => "-",
                        UnaryOp::Not => "!",
                    },
                    inner.code
                ),
                ty: match op {
                    UnaryOp::Neg => Type::I64,
                    UnaryOp::Not => Type::Bool,
                },
            }
        }
        ExprKind::Binary { left, op, right } => {
            let left = emit_expr(left, env, signatures)?;
            let right = emit_expr(right, env, signatures)?;
            let result_ty = type_of_expr(expr, env, signatures)?;
            let code = if matches!(op, BinOp::Div) {
                format!("flux_div_i64({}, {})", left.code, right.code)
            } else if matches!(op, BinOp::Eq | BinOp::Ne) && left.ty == Type::Str {
                let comparator = if matches!(op, BinOp::Eq) { "==" } else { "!=" };
                format!("(strcmp({}, {}) {comparator} 0)", left.code, right.code)
            } else {
                format!("({} {} {})", left.code, c_operator(*op), right.code)
            };
            EmittedExpr {
                code,
                ty: result_ty,
            }
        }
    };
    Ok(emitted)
}

fn emit_multi_expr(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<(String, String), String> {
    let ExprKind::Call { name, args } = &expr.kind else {
        return Err(format!(
            "line {}: only multi-value function calls can be destructured",
            expr.line
        ));
    };
    let signature = signatures.get(name).ok_or_else(|| {
        format!(
            "line {}: unknown function '{name}' during code generation",
            expr.line
        )
    })?;
    if signature.returns.len() < 2 {
        return Err(format!(
            "line {}: function '{name}' does not return multiple values",
            expr.line
        ));
    }
    let mut rendered = Vec::with_capacity(args.len());
    for arg in args {
        rendered.push(emit_expr(arg, env, signatures)?.code);
    }
    Ok((
        format!("{name}({})", rendered.join(", ")),
        multi_return_struct_name(name),
    ))
}

fn c_function_return_type(function: &Function) -> String {
    match function.returns.as_slice() {
        [] => "void".to_string(),
        [ty] => c_type(ty).to_string(),
        _ => format!("struct {}", multi_return_struct_name(&function.name)),
    }
}

fn multi_return_struct_name(function_name: &str) -> String {
    format!("flux__ret_{function_name}")
}

fn c_type(ty: &Type) -> &'static str {
    match ty {
        Type::I64 => "int64_t",
        Type::Bool => "bool",
        Type::Str => "const char *",
        Type::Void => "void",
    }
}

fn c_condition(code: &str) -> String {
    if code.starts_with('(') && code.ends_with(')') {
        code.to_string()
    } else {
        format!("({code})")
    }
}

fn c_operator(op: BinOp) -> &'static str {
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

fn c_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_ascii_control() => out.push_str(&format!("\\x{:02x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}
