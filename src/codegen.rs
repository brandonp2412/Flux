use std::collections::{HashMap, HashSet};

use crate::ast::{
    BinOp, Expr, ExprKind, Function, Program, Stmt, StmtKind, StructDef, Type, UnaryOp,
};
use crate::diagnostic::{Diagnostic, DiagnosticStage, SourceSpan};
use crate::typecheck::{Signatures, type_of_expr};

pub fn emit_c(program: &Program, signatures: &Signatures) -> Result<String, Diagnostic> {
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
    out.push_str("static inline void flux_print_error(const char *value) { puts(value ? value : \"nil\"); }\n");
    out.push_str("static inline bool flux_error_eq(const char *a, const char *b) { return a == NULL ? b == NULL : b != NULL && strcmp(a, b) == 0; }\n");
    out.push_str("static inline int64_t flux_div_i64(int64_t a, int64_t b) {\n");
    out.push_str("    if (b == 0 || (a == INT64_MIN && b == -1)) { fputs(\"Flux runtime error: invalid integer division\\n\", stderr); abort(); }\n");
    out.push_str("    return a / b;\n");
    out.push_str("}\n\n");

    for definition in struct_emit_order(program)? {
        out.push_str(&format!("struct {} {{\n", struct_c_name(&definition.name)));
        for field in &definition.fields {
            out.push_str(&format!(
                "    {} {};\n",
                c_type(&field.ty),
                field_c_name(&field.name)
            ));
        }
        out.push_str("};\n");
    }
    if !program.structs.is_empty() {
        out.push('\n');
    }

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
) -> Result<(), Diagnostic> {
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
) -> Result<(), Diagnostic> {
    for stmt in body {
        let pad = "    ".repeat(depth);
        match &stmt.kind {
            StmtKind::Let { name, ty, expr, .. } => {
                let value = emit_expr(expr, env, signatures)?;
                out.push_str(&format!("{pad}{} {name} = {};\n", c_type(ty), value.code));
                env.insert(name.clone(), ty.clone());
            }
            StmtKind::LetDestructure {
                bindings,
                expr,
                else_return,
            } => {
                let (value, tag) = emit_multi_expr(expr, env, signatures)?;
                let temp = format!("flux__multi_{}", *temp_counter);
                *temp_counter += 1;
                out.push_str(&format!("{pad}struct {tag} {temp} = {value};\n"));
                if *else_return {
                    let error_index = bindings.len() - 1;
                    let return_tag = multi_return_struct_name(&current_function.name);
                    let return_temp = format!("flux__return_{}", *temp_counter);
                    *temp_counter += 1;
                    out.push_str(&format!("{pad}if ({temp}.v{error_index} != NULL) {{\n"));
                    out.push_str(&format!("{pad}    struct {return_tag} {return_temp};\n"));
                    for index in 0..bindings.len() {
                        out.push_str(&format!(
                            "{pad}    {return_temp}.v{index} = {temp}.v{index};\n"
                        ));
                    }
                    out.push_str(&format!("{pad}    return {return_temp};\n"));
                    out.push_str(&format!("{pad}}}\n"));
                }
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
            StmtKind::Return(values) if values.len() == 1 && current_function.returns.len() > 1 => {
                let (value, source_tag) = emit_multi_expr(&values[0], env, signatures)?;
                let source_temp = format!("flux__forward_{}", *temp_counter);
                *temp_counter += 1;
                let return_tag = multi_return_struct_name(&current_function.name);
                let return_temp = format!("flux__return_{}", *temp_counter);
                *temp_counter += 1;
                out.push_str(&format!(
                    "{pad}struct {source_tag} {source_temp} = {value};\n"
                ));
                out.push_str(&format!("{pad}struct {return_tag} {return_temp};\n"));
                for index in 0..current_function.returns.len() {
                    out.push_str(&format!(
                        "{pad}{return_temp}.v{index} = {source_temp}.v{index};\n"
                    ));
                }
                out.push_str(&format!("{pad}return {return_temp};\n"));
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
            StmtKind::If {
                cond,
                body,
                else_body,
                ..
            } => {
                let cond = emit_expr(cond, env, signatures)?;
                out.push_str(&format!("{pad}if {} {{\n", c_condition(&cond.code)));
                let mut then_env = env.clone();
                emit_block(
                    out,
                    body,
                    depth + 1,
                    &mut then_env,
                    signatures,
                    temp_counter,
                    current_function,
                )?;
                if else_body.is_empty() {
                    out.push_str(&format!("{pad}}}\n"));
                } else {
                    out.push_str(&format!("{pad}}} else {{\n"));
                    let mut else_env = env.clone();
                    emit_block(
                        out,
                        else_body,
                        depth + 1,
                        &mut else_env,
                        signatures,
                        temp_counter,
                        current_function,
                    )?;
                    out.push_str(&format!("{pad}}}\n"));
                }
            }
            StmtKind::ForRange {
                name,
                start,
                end,
                body,
                ..
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
) -> Result<EmittedExpr, Diagnostic> {
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
        ExprKind::Nil => EmittedExpr {
            code: "NULL".to_string(),
            ty: Type::Error,
        },
        ExprKind::Var(name) => EmittedExpr {
            code: name.clone(),
            ty: env.get(name).cloned().ok_or_else(|| {
                diag(
                    expr.span,
                    &format!("unknown binding '{name}' during code generation"),
                )
            })?,
        },
        ExprKind::Call { name, args } if name == "print" => {
            let arg = emit_expr(&args[0], env, signatures)?;
            let helper = match arg.ty {
                Type::I64 => "flux_print_i64",
                Type::Bool => "flux_print_bool",
                Type::Str => "flux_print_str",
                Type::Error => "flux_print_error",
                Type::Named(_) => {
                    return Err(diag(expr.span, "cannot print a struct value directly"));
                }
                Type::Void => return Err(diag(expr.span, "cannot print void")),
            };
            EmittedExpr {
                code: format!("{helper}({})", arg.code),
                ty: Type::Void,
            }
        }
        ExprKind::Call { name, args } if name == "error" => {
            let message = emit_expr(&args[0], env, signatures)?;
            EmittedExpr {
                code: message.code,
                ty: Type::Error,
            }
        }
        ExprKind::Call { name, args } => {
            let signature = signatures.get(name).ok_or_else(|| {
                diag(
                    expr.span,
                    &format!("unknown function '{name}' during code generation"),
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
                    return Err(diag(
                        expr.span,
                        &format!(
                            "multi-value call '{name}' requires destructuring during code generation"
                        ),
                    ));
                }
            };
            EmittedExpr {
                code: format!("{name}({})", rendered.join(", ")),
                ty,
            }
        }
        ExprKind::StructLiteral { name, fields, .. } => {
            let mut rendered = Vec::with_capacity(fields.len());
            for field in fields {
                let value = emit_expr(&field.value, env, signatures)?;
                rendered.push(format!(".{} = {}", field_c_name(&field.name), value.code));
            }
            EmittedExpr {
                code: format!(
                    "((struct {}){{ {} }})",
                    struct_c_name(name),
                    rendered.join(", ")
                ),
                ty: Type::Named(name.clone()),
            }
        }
        ExprKind::Field { base, name, .. } => {
            let base = emit_expr(base, env, signatures)?;
            let result_ty = type_of_expr(expr, env, signatures)?;
            EmittedExpr {
                code: format!("({}).{}", base.code, field_c_name(name)),
                ty: result_ty,
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
            } else if matches!(op, BinOp::Eq | BinOp::Ne) && left.ty == Type::Error {
                let equality = format!("flux_error_eq({}, {})", left.code, right.code);
                if matches!(op, BinOp::Eq) {
                    equality
                } else {
                    format!("(!{equality})")
                }
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
) -> Result<(String, String), Diagnostic> {
    let ExprKind::Call { name, args } = &expr.kind else {
        return Err(diag(
            expr.span,
            "only multi-value function calls can be destructured",
        ));
    };
    let signature = signatures.get(name).ok_or_else(|| {
        diag(
            expr.span,
            &format!("unknown function '{name}' during code generation"),
        )
    })?;
    if signature.returns.len() < 2 {
        return Err(diag(
            expr.span,
            &format!("function '{name}' does not return multiple values"),
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
        [ty] => c_type(ty),
        _ => format!("struct {}", multi_return_struct_name(&function.name)),
    }
}

fn multi_return_struct_name(function_name: &str) -> String {
    format!("flux__ret_{function_name}")
}

fn c_type(ty: &Type) -> String {
    match ty {
        Type::I64 => "int64_t".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Str => "const char *".to_string(),
        Type::Error => "const char *".to_string(),
        Type::Void => "void".to_string(),
        Type::Named(name) => format!("struct {}", struct_c_name(name)),
    }
}

fn struct_c_name(name: &str) -> String {
    format!("flux__type_{name}")
}

fn field_c_name(name: &str) -> String {
    format!("flux__field_{name}")
}

fn struct_emit_order(program: &Program) -> Result<Vec<&StructDef>, Diagnostic> {
    let definitions = program
        .structs
        .iter()
        .map(|definition| (definition.name.as_str(), definition))
        .collect::<HashMap<_, _>>();
    let mut visiting = HashSet::new();
    let mut emitted = HashSet::new();
    let mut order = Vec::with_capacity(program.structs.len());

    for definition in &program.structs {
        visit_struct(
            definition,
            &definitions,
            &mut visiting,
            &mut emitted,
            &mut order,
        )?;
    }
    Ok(order)
}

fn visit_struct<'a>(
    definition: &'a StructDef,
    definitions: &HashMap<&str, &'a StructDef>,
    visiting: &mut HashSet<&'a str>,
    emitted: &mut HashSet<&'a str>,
    order: &mut Vec<&'a StructDef>,
) -> Result<(), Diagnostic> {
    if emitted.contains(definition.name.as_str()) {
        return Ok(());
    }
    if !visiting.insert(definition.name.as_str()) {
        return Err(diag(
            definition.name_span,
            &format!(
                "struct '{}' participates in a recursive by-value cycle",
                definition.name
            ),
        )
        .with_note("recursive value structs need an indirection/ownership type, which is not implemented yet"));
    }

    for field in &definition.fields {
        if let Type::Named(name) = &field.ty
            && let Some(dependency) = definitions.get(name.as_str())
        {
            visit_struct(dependency, definitions, visiting, emitted, order)?;
        }
    }

    visiting.remove(definition.name.as_str());
    emitted.insert(definition.name.as_str());
    order.push(definition);
    Ok(())
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

fn diag(span: SourceSpan, message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticStage::Codegen, span, message)
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
