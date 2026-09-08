use std::collections::{HashMap, HashSet};

use crate::ast::{
    BinOp, EnumDef, Expr, ExprKind, Function, MatchPattern, NamedArg, Program, Stmt, StmtKind,
    StructDef, StructPatternField, Type, UnaryOp,
};
use crate::diagnostic::{Diagnostic, DiagnosticStage, SourceSpan};
use crate::typecheck::{ConstantValue, Signature, Signatures, type_of_expr};

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

    for definition in &program.structs {
        out.push_str(&format!("struct {};\n", struct_c_name(&definition.name)));
    }
    for definition in &program.enums {
        out.push_str(&format!("struct {};\n", struct_c_name(&definition.name)));
    }
    if !program.structs.is_empty() || !program.enums.is_empty() {
        out.push('\n');
    }
    emit_function_type_typedefs(&mut out, program, signatures)?;

    for definition in value_type_emit_order(program, signatures)? {
        match definition {
            ValueDef::Struct(definition) => {
                emit_struct_definition(&mut out, definition, signatures)
            }
            ValueDef::Enum(definition) => emit_enum_definition(&mut out, definition, signatures),
        }
    }
    if !program.structs.is_empty() || !program.enums.is_empty() {
        out.push('\n');
    }

    out.push_str(&enum_variant_helpers(program, signatures));
    out.push_str(&struct_update_helpers(program, signatures));

    for function in &program.functions {
        if function.returns.len() > 1 {
            let tag = multi_return_struct_name(&function.name);
            out.push_str(&format!("struct {tag} {{\n"));
            for (index, ty) in function.returns.iter().enumerate() {
                out.push_str(&format!("    {} v{index};\n", c_type(ty, signatures)));
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
        out.push_str(&function_prototype(function, signatures));
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

fn function_prototype(function: &Function, signatures: &Signatures) -> String {
    let ret = if function.name == "main" {
        "int".to_string()
    } else {
        c_function_return_type(function, signatures)
    };
    let params = if function.params.is_empty() {
        "void".to_string()
    } else {
        function
            .params
            .iter()
            .map(|param| {
                format!(
                    "{} {}",
                    c_type(&param.ty, signatures),
                    local_c_name(&param.name)
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!("{ret} {}({params})", function_c_name(&function.name))
}

fn emit_function(
    out: &mut String,
    function: &Function,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<(), Diagnostic> {
    out.push_str(&function_prototype(function, signatures));
    out.push_str(" {\n");
    let mut env = HashMap::new();
    for param in &function.params {
        env.insert(param.name.clone(), signatures.canonical_type(&param.ty));
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
                out.push_str(&format!(
                    "{pad}{} {} = {};\n",
                    c_type(ty, signatures),
                    local_c_name(name),
                    value.code
                ));
                env.insert(name.clone(), signatures.canonical_type(ty));
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
                        c_type(&binding.ty, signatures),
                        local_c_name(&binding.name)
                    ));
                    env.insert(binding.name.clone(), signatures.canonical_type(&binding.ty));
                }
            }
            StmtKind::LetStructDestructure { fields, expr, .. } => {
                let value = emit_expr(expr, env, signatures)?;
                let Type::Named(struct_name) = &value.ty else {
                    return Err(diag(
                        stmt.span,
                        "struct destructuring code generation requires a struct value",
                    ));
                };
                if signatures.struct_type(struct_name).is_none() {
                    return Err(diag(
                        stmt.span,
                        "struct destructuring code generation requires a struct value",
                    ));
                }
                let temp = format!("flux__destructure_{}", *temp_counter);
                *temp_counter += 1;
                out.push_str(&format!(
                    "{pad}{} {temp} = {};\n",
                    c_type(&value.ty, signatures),
                    value.code
                ));
                emit_struct_pattern_bindings(
                    out,
                    &pad,
                    fields,
                    struct_name,
                    &temp,
                    env,
                    signatures,
                )?;
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
            StmtKind::Break => out.push_str(&format!("{pad}break;\n")),
            StmtKind::Continue => out.push_str(&format!("{pad}continue;\n")),
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
                let c_name = local_c_name(name);
                out.push_str(&format!(
                    "{pad}for (int64_t {c_name} = {}, {temp} = {}; {c_name} < {temp}; ++{c_name}) {{\n",
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
            StmtKind::Match { value, arms } => {
                let value = emit_expr(value, env, signatures)?;
                let Type::Named(enum_name) = &value.ty else {
                    return Err(diag(
                        stmt.span,
                        "match code generation requires an enum value",
                    ));
                };
                let definition = signatures.enum_type(enum_name).ok_or_else(|| {
                    diag(stmt.span, "match code generation requires an enum value")
                })?;
                let temp = format!("flux__match_{}", *temp_counter);
                *temp_counter += 1;
                out.push_str(&format!(
                    "{pad}struct {} {temp} = {};\n",
                    struct_c_name(enum_name),
                    value.code
                ));
                out.push_str(&format!("{pad}switch ({temp}.tag) {{\n"));
                for arm in arms {
                    let variant = definition
                        .variant(&arm.variant)
                        .expect("type checking guarantees match variants exist");
                    out.push_str(&format!(
                        "{pad}    case {}: {{\n",
                        enum_tag_value_name(enum_name, &arm.variant)
                    ));
                    let mut nested = env.clone();
                    for (index, (pattern, payload_ty)) in
                        arm.patterns.iter().zip(&variant.payloads).enumerate()
                    {
                        let payload_access = format!(
                            "{temp}.payload.{}.v{index}",
                            enum_payload_member_name(&arm.variant)
                        );
                        match pattern {
                            MatchPattern::Binding(binding) => {
                                if binding.name == "_" {
                                    continue;
                                }
                                out.push_str(&format!(
                                    "{pad}        {} {} = {payload_access};\n",
                                    c_type(payload_ty, signatures),
                                    local_c_name(&binding.name),
                                ));
                                nested.insert(binding.name.clone(), payload_ty.clone());
                            }
                            MatchPattern::Struct(pattern) => {
                                let Type::Named(struct_name) =
                                    signatures.canonical_type(payload_ty)
                                else {
                                    return Err(diag(
                                        pattern.struct_span,
                                        "match struct pattern code generation requires a struct value",
                                    ));
                                };
                                emit_struct_pattern_bindings(
                                    out,
                                    &format!("{pad}        "),
                                    &pattern.fields,
                                    &struct_name,
                                    &payload_access,
                                    &mut nested,
                                    signatures,
                                )?;
                            }
                        }
                    }
                    emit_block(
                        out,
                        &arm.body,
                        depth + 2,
                        &mut nested,
                        signatures,
                        temp_counter,
                        current_function,
                    )?;
                    out.push_str(&format!("{pad}        break;\n"));
                    out.push_str(&format!("{pad}    }}\n"));
                }
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

fn emit_struct_pattern_bindings(
    out: &mut String,
    pad: &str,
    fields: &[StructPatternField],
    struct_name: &str,
    base: &str,
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    let definition = signatures
        .struct_type(struct_name)
        .expect("type checking guarantees struct pattern type exists");
    for field in fields {
        let field_signature = definition
            .field(&field.field)
            .expect("type checking guarantees struct pattern fields exist");
        let access = format!("{base}.{}", field_c_name(&field.field));
        if let Some(nested) = &field.nested {
            let Type::Named(nested_name) = signatures.canonical_type(&field_signature.ty) else {
                return Err(diag(
                    nested.struct_span,
                    "nested struct pattern code generation requires a struct value",
                ));
            };
            emit_struct_pattern_bindings(
                out,
                pad,
                &nested.fields,
                &nested_name,
                &access,
                env,
                signatures,
            )?;
            continue;
        }
        if field.binding.name == "_" {
            continue;
        }
        out.push_str(&format!(
            "{pad}{} {} = {access};\n",
            c_type(&field_signature.ty, signatures),
            local_c_name(&field.binding.name),
        ));
        env.insert(field.binding.name.clone(), field_signature.ty.clone());
    }
    Ok(())
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
        ExprKind::Var(name) => {
            if let Some(ty) = env.get(name) {
                EmittedExpr {
                    code: local_c_name(name),
                    ty: ty.clone(),
                }
            } else if let Some(constant) = signatures.constant(name) {
                EmittedExpr {
                    code: constant_c_value(&constant.value),
                    ty: constant.ty.clone(),
                }
            } else if let Some(signature) = signatures.get(name) {
                EmittedExpr {
                    code: function_c_name(name),
                    ty: Type::Function {
                        params: signature.params.clone(),
                        returns: signature.returns.clone(),
                    },
                }
            } else {
                return Err(diag(
                    expr.span,
                    &format!(
                        "unknown binding, constant, or function '{name}' during code generation"
                    ),
                ));
            }
        }
        ExprKind::Call { name, args, .. } if name == "print" => {
            let arg = emit_expr(&args[0], env, signatures)?;
            let helper = match arg.ty {
                Type::I64 => "flux_print_i64",
                Type::Bool => "flux_print_bool",
                Type::Str => "flux_print_str",
                Type::Error => "flux_print_error",
                Type::Named(_) => {
                    return Err(diag(
                        expr.span,
                        "cannot print a named aggregate value directly",
                    ));
                }
                Type::Function { .. } => {
                    return Err(diag(expr.span, "cannot print a function value directly"));
                }
                Type::Void => return Err(diag(expr.span, "cannot print void")),
            };
            EmittedExpr {
                code: format!("{helper}({})", arg.code),
                ty: Type::Void,
            }
        }
        ExprKind::Call { name, args, .. } if name == "error" => {
            let message = emit_expr(&args[0], env, signatures)?;
            EmittedExpr {
                code: message.code,
                ty: Type::Error,
            }
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } => {
            let (rendered, returns, callee) = if let Some(signature) = signatures.get(name) {
                (
                    emit_call_arguments(signature, args, named_args, env, signatures)?,
                    signature.returns.clone(),
                    function_c_name(name),
                )
            } else {
                let Some(Type::Function { params, returns }) = env.get(name) else {
                    return Err(diag(
                        expr.span,
                        &format!("unknown function or callable '{name}' during code generation"),
                    ));
                };
                if !named_args.is_empty() {
                    return Err(diag(
                        expr.span,
                        "first-class function values accept positional arguments only",
                    ));
                }
                if args.len() != params.len() {
                    return Err(diag(
                        expr.span,
                        "invalid function value call reached code generation",
                    ));
                }
                let mut rendered = Vec::with_capacity(args.len());
                for arg in args {
                    rendered.push(emit_expr(arg, env, signatures)?.code);
                }
                (rendered, returns.clone(), local_c_name(name))
            };
            let ty = match returns.as_slice() {
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
                code: format!("{callee}({})", rendered.join(", ")),
                ty,
            }
        }
        ExprKind::EnumVariant {
            enum_name,
            variant,
            args,
            ..
        } => {
            let mut rendered = Vec::with_capacity(args.len());
            for arg in args {
                rendered.push(emit_expr(arg, env, signatures)?.code);
            }
            EmittedExpr {
                code: format!(
                    "{}({})",
                    enum_variant_helper_name(enum_name, variant),
                    rendered.join(", ")
                ),
                ty: Type::Named(enum_name.clone()),
            }
        }
        ExprKind::StructLiteral {
            name, base, fields, ..
        } => {
            if let Some(base) = base {
                let base = emit_expr(base, env, signatures)?;
                let mut args = Vec::with_capacity(fields.len() + 1);
                args.push(base.code);
                for field in fields {
                    args.push(emit_expr(&field.value, env, signatures)?.code);
                }
                EmittedExpr {
                    code: format!(
                        "{}({})",
                        struct_update_helper_name(name, fields),
                        args.join(", ")
                    ),
                    ty: Type::Named(name.clone()),
                }
            } else {
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

fn emit_call_arguments(
    signature: &Signature,
    args: &[Expr],
    named_args: &[NamedArg],
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Vec<String>, Diagnostic> {
    let mut rendered = Vec::with_capacity(signature.param_details.len());
    let mut positional_index = 0usize;
    for param in &signature.param_details {
        if !param.named_only && positional_index < args.len() {
            rendered.push(emit_expr(&args[positional_index], env, signatures)?.code);
            positional_index += 1;
            continue;
        }
        if let Some(named) = named_args.iter().find(|arg| arg.name == param.name) {
            rendered.push(emit_expr(&named.value, env, signatures)?.code);
            continue;
        }
        if let Some(default) = &param.default {
            rendered.push(constant_c_value(default));
            continue;
        }
        return Err(diag(
            param.span,
            &format!(
                "missing argument '{}' reached code generation after type checking",
                param.name
            ),
        ));
    }
    Ok(rendered)
}

fn emit_multi_expr(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<(String, String), Diagnostic> {
    let ExprKind::Call {
        name,
        args,
        named_args,
    } = &expr.kind
    else {
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
    let rendered = emit_call_arguments(signature, args, named_args, env, signatures)?;
    Ok((
        format!("{}({})", function_c_name(name), rendered.join(", ")),
        multi_return_struct_name(name),
    ))
}

fn c_function_return_type(function: &Function, signatures: &Signatures) -> String {
    match function.returns.as_slice() {
        [] => "void".to_string(),
        [ty] => c_type(ty, signatures),
        _ => format!("struct {}", multi_return_struct_name(&function.name)),
    }
}

fn multi_return_struct_name(function_name: &str) -> String {
    format!("flux__ret_{function_name}")
}

fn c_type(ty: &Type, signatures: &Signatures) -> String {
    match signatures.canonical_type(ty) {
        Type::I64 => "int64_t".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Str => "const char *".to_string(),
        Type::Error => "const char *".to_string(),
        Type::Void => "void".to_string(),
        Type::Named(name) => format!("struct {}", struct_c_name(&name)),
        Type::Function { params, returns } => function_type_name(&params, &returns, signatures),
    }
}

fn function_type_name(params: &[Type], returns: &[Type], signatures: &Signatures) -> String {
    let params = if params.is_empty() {
        "void".to_string()
    } else {
        params
            .iter()
            .map(|ty| type_mangle(ty, signatures))
            .collect::<Vec<_>>()
            .join("__")
    };
    let returns = if returns.is_empty() {
        "void".to_string()
    } else {
        returns
            .iter()
            .map(|ty| type_mangle(ty, signatures))
            .collect::<Vec<_>>()
            .join("__")
    };
    format!("flux__fn_{params}__to__{returns}")
}

fn type_mangle(ty: &Type, signatures: &Signatures) -> String {
    match signatures.canonical_type(ty) {
        Type::I64 => "i64".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Str => "str".to_string(),
        Type::Error => "error".to_string(),
        Type::Void => "void".to_string(),
        Type::Named(name) => format!("named_{name}"),
        Type::Function { params, returns } => {
            let name = function_type_name(&params, &returns, signatures);
            name.trim_start_matches("flux__fn_").to_string()
        }
    }
}

fn emit_function_type_typedefs(
    out: &mut String,
    program: &Program,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    let mut types = HashSet::new();
    for alias in &program.aliases {
        collect_function_type(&alias.target, signatures, &mut types);
    }
    for definition in &program.structs {
        for field in &definition.fields {
            collect_function_type(&field.ty, signatures, &mut types);
        }
    }
    for definition in &program.enums {
        for variant in &definition.variants {
            for payload in &variant.payloads {
                collect_function_type(&payload.ty, signatures, &mut types);
            }
        }
    }
    for function in &program.functions {
        for param in &function.params {
            collect_function_type(&param.ty, signatures, &mut types);
        }
        for ty in &function.returns {
            collect_function_type(ty, signatures, &mut types);
        }
        collect_function_types_from_block(&function.body, signatures, &mut types);
    }

    let mut types = types.into_iter().collect::<Vec<_>>();
    types.sort_by(|left, right| {
        function_type_depth(left)
            .cmp(&function_type_depth(right))
            .then_with(|| left.name().cmp(&right.name()))
    });
    for ty in types {
        let Type::Function { params, returns } = ty else {
            continue;
        };
        if returns.len() > 1 {
            return Err(diag(
                SourceSpan::line(1),
                "first-class function types currently support zero or one return value",
            ));
        }
        let return_type = returns
            .first()
            .map(|ty| c_type(ty, signatures))
            .unwrap_or_else(|| "void".to_string());
        let params_text = if params.is_empty() {
            "void".to_string()
        } else {
            params
                .iter()
                .map(|ty| c_type(ty, signatures))
                .collect::<Vec<_>>()
                .join(", ")
        };
        out.push_str(&format!(
            "typedef {return_type} (*{})({params_text});\n",
            function_type_name(&params, &returns, signatures)
        ));
    }
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
    Ok(())
}

fn collect_function_type(ty: &Type, signatures: &Signatures, types: &mut HashSet<Type>) {
    let ty = signatures.canonical_type(ty);
    if let Type::Function { params, returns } = &ty {
        for nested in params.iter().chain(returns) {
            collect_function_type(nested, signatures, types);
        }
        types.insert(ty);
    }
}

fn collect_function_types_from_block(
    body: &[Stmt],
    signatures: &Signatures,
    types: &mut HashSet<Type>,
) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Let { ty, .. } => collect_function_type(ty, signatures, types),
            StmtKind::LetDestructure { bindings, .. } => {
                for binding in bindings {
                    collect_function_type(&binding.ty, signatures, types);
                }
            }
            StmtKind::LetStructDestructure { .. }
            | StmtKind::Return(_)
            | StmtKind::Break
            | StmtKind::Continue
            | StmtKind::Expr(_) => {}
            StmtKind::If {
                body, else_body, ..
            } => {
                collect_function_types_from_block(body, signatures, types);
                collect_function_types_from_block(else_body, signatures, types);
            }
            StmtKind::ForRange { body, .. } => {
                collect_function_types_from_block(body, signatures, types);
            }
            StmtKind::Match { arms, .. } => {
                for arm in arms {
                    collect_function_types_from_block(&arm.body, signatures, types);
                }
            }
        }
    }
}

fn function_type_depth(ty: &Type) -> usize {
    match ty {
        Type::Function { params, returns } => {
            1 + params
                .iter()
                .chain(returns)
                .map(function_type_depth)
                .max()
                .unwrap_or(0)
        }
        _ => 0,
    }
}

fn function_c_name(name: &str) -> String {
    if name == "main" {
        "main".to_string()
    } else {
        format!("flux__fn_{name}")
    }
}

fn local_c_name(name: &str) -> String {
    format!("flux__local_{name}")
}

fn struct_c_name(name: &str) -> String {
    format!("flux__type_{name}")
}

fn field_c_name(name: &str) -> String {
    format!("flux__field_{name}")
}

fn struct_update_helpers(program: &Program, signatures: &Signatures) -> String {
    let mut helpers = String::new();
    let mut emitted = HashSet::new();
    for function in &program.functions {
        collect_update_helpers_from_block(&function.body, signatures, &mut emitted, &mut helpers);
    }
    if !helpers.is_empty() {
        helpers.push('\n');
    }
    helpers
}

fn collect_update_helpers_from_block(
    body: &[Stmt],
    signatures: &Signatures,
    emitted: &mut HashSet<String>,
    helpers: &mut String,
) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Let { expr, .. }
            | StmtKind::LetDestructure { expr, .. }
            | StmtKind::LetStructDestructure { expr, .. } => {
                collect_update_helpers_from_expr(expr, signatures, emitted, helpers);
            }
            StmtKind::Return(values) => {
                for expr in values {
                    collect_update_helpers_from_expr(expr, signatures, emitted, helpers);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Expr(expr) => {
                collect_update_helpers_from_expr(expr, signatures, emitted, helpers);
            }
            StmtKind::If {
                cond,
                body,
                else_body,
                ..
            } => {
                collect_update_helpers_from_expr(cond, signatures, emitted, helpers);
                collect_update_helpers_from_block(body, signatures, emitted, helpers);
                collect_update_helpers_from_block(else_body, signatures, emitted, helpers);
            }
            StmtKind::ForRange {
                start, end, body, ..
            } => {
                collect_update_helpers_from_expr(start, signatures, emitted, helpers);
                collect_update_helpers_from_expr(end, signatures, emitted, helpers);
                collect_update_helpers_from_block(body, signatures, emitted, helpers);
            }
            StmtKind::Match { value, arms } => {
                collect_update_helpers_from_expr(value, signatures, emitted, helpers);
                for arm in arms {
                    collect_update_helpers_from_block(&arm.body, signatures, emitted, helpers);
                }
            }
        }
    }
}

fn collect_update_helpers_from_expr(
    expr: &Expr,
    signatures: &Signatures,
    emitted: &mut HashSet<String>,
    helpers: &mut String,
) {
    match &expr.kind {
        ExprKind::StructLiteral {
            name, base, fields, ..
        } => {
            if let Some(base) = base {
                collect_update_helpers_from_expr(base, signatures, emitted, helpers);
                let helper_name = struct_update_helper_name(name, fields);
                if emitted.insert(helper_name.clone()) {
                    let definition = signatures
                        .struct_type(name)
                        .expect("type checking guarantees update struct exists");
                    helpers.push_str(&format!(
                        "static inline struct {} {helper_name}(struct {} base",
                        struct_c_name(name),
                        struct_c_name(name)
                    ));
                    for field in fields {
                        let signature = definition
                            .field(&field.name)
                            .expect("type checking guarantees update field exists");
                        helpers.push_str(&format!(
                            ", {} value_{}",
                            c_type(&signature.ty, signatures),
                            field.name
                        ));
                    }
                    helpers.push_str(") {\n");
                    for field in fields {
                        helpers.push_str(&format!(
                            "    base.{} = value_{};\n",
                            field_c_name(&field.name),
                            field.name
                        ));
                    }
                    helpers.push_str("    return base;\n}\n");
                }
            }
            for field in fields {
                collect_update_helpers_from_expr(&field.value, signatures, emitted, helpers);
            }
        }
        ExprKind::Field { base, .. } | ExprKind::Unary { expr: base, .. } => {
            collect_update_helpers_from_expr(base, signatures, emitted, helpers);
        }
        ExprKind::Binary { left, right, .. } => {
            collect_update_helpers_from_expr(left, signatures, emitted, helpers);
            collect_update_helpers_from_expr(right, signatures, emitted, helpers);
        }
        ExprKind::Call {
            args, named_args, ..
        } => {
            for arg in args {
                collect_update_helpers_from_expr(arg, signatures, emitted, helpers);
            }
            for arg in named_args {
                collect_update_helpers_from_expr(&arg.value, signatures, emitted, helpers);
            }
        }
        ExprKind::EnumVariant { args, .. } => {
            for arg in args {
                collect_update_helpers_from_expr(arg, signatures, emitted, helpers);
            }
        }
        ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::Var(_) => {}
    }
}

fn struct_update_helper_name(name: &str, fields: &[crate::ast::StructLiteralField]) -> String {
    let suffix = if fields.is_empty() {
        "copy".to_string()
    } else {
        fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>()
            .join("__")
    };
    format!("flux__update_{name}__{suffix}")
}

fn emit_struct_definition(out: &mut String, definition: &StructDef, signatures: &Signatures) {
    out.push_str(&format!("struct {} {{\n", struct_c_name(&definition.name)));
    for field in &definition.fields {
        out.push_str(&format!(
            "    {} {};\n",
            c_type(&field.ty, signatures),
            field_c_name(&field.name)
        ));
    }
    out.push_str("};\n");
}

fn emit_enum_definition(out: &mut String, definition: &EnumDef, signatures: &Signatures) {
    let tag_type = enum_tag_type_name(&definition.name);
    out.push_str(&format!("enum {tag_type} {{\n"));
    for variant in &definition.variants {
        out.push_str(&format!(
            "    {},\n",
            enum_tag_value_name(&definition.name, &variant.name)
        ));
    }
    out.push_str("};\n");
    out.push_str(&format!("struct {} {{\n", struct_c_name(&definition.name)));
    out.push_str(&format!("    enum {tag_type} tag;\n"));
    if definition
        .variants
        .iter()
        .any(|variant| !variant.payloads.is_empty())
    {
        out.push_str("    union {\n");
        for variant in &definition.variants {
            if variant.payloads.is_empty() {
                continue;
            }
            out.push_str("        struct {\n");
            for (index, payload) in variant.payloads.iter().enumerate() {
                out.push_str(&format!(
                    "            {} v{index};\n",
                    c_type(&payload.ty, signatures)
                ));
            }
            out.push_str(&format!(
                "        }} {};\n",
                enum_payload_member_name(&variant.name)
            ));
        }
        out.push_str("    } payload;\n");
    }
    out.push_str("};\n");
}

fn enum_variant_helpers(program: &Program, signatures: &Signatures) -> String {
    let mut out = String::new();
    for definition in &program.enums {
        for variant in &definition.variants {
            let helper = enum_variant_helper_name(&definition.name, &variant.name);
            out.push_str(&format!(
                "static inline struct {} {helper}(",
                struct_c_name(&definition.name)
            ));
            if variant.payloads.is_empty() {
                out.push_str("void");
            } else {
                out.push_str(
                    &variant
                        .payloads
                        .iter()
                        .enumerate()
                        .map(|(index, payload)| {
                            format!("{} v{index}", c_type(&payload.ty, signatures))
                        })
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
            out.push_str(") {\n");
            out.push_str(&format!(
                "    struct {} value = {{ .tag = {} }};\n",
                struct_c_name(&definition.name),
                enum_tag_value_name(&definition.name, &variant.name)
            ));
            for index in 0..variant.payloads.len() {
                out.push_str(&format!(
                    "    value.payload.{}.v{index} = v{index};\n",
                    enum_payload_member_name(&variant.name)
                ));
            }
            out.push_str("    return value;\n}\n");
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

fn enum_tag_type_name(name: &str) -> String {
    format!("flux__tag_{name}")
}

fn enum_tag_value_name(enum_name: &str, variant: &str) -> String {
    format!("flux__tag_{enum_name}_{variant}")
}

fn enum_payload_member_name(variant: &str) -> String {
    format!("flux__payload_{variant}")
}

fn enum_variant_helper_name(enum_name: &str, variant: &str) -> String {
    format!("flux__variant_{enum_name}_{variant}")
}

#[derive(Debug, Clone, Copy)]
enum ValueDef<'a> {
    Struct(&'a StructDef),
    Enum(&'a EnumDef),
}

impl ValueDef<'_> {
    fn name(&self) -> &str {
        match self {
            Self::Struct(definition) => &definition.name,
            Self::Enum(definition) => &definition.name,
        }
    }

    fn span(&self) -> SourceSpan {
        match self {
            Self::Struct(definition) => definition.name_span,
            Self::Enum(definition) => definition.name_span,
        }
    }
}

fn value_type_emit_order<'a>(
    program: &'a Program,
    signatures: &Signatures,
) -> Result<Vec<ValueDef<'a>>, Diagnostic> {
    let definitions = program
        .structs
        .iter()
        .map(|definition| (definition.name.as_str(), ValueDef::Struct(definition)))
        .chain(
            program
                .enums
                .iter()
                .map(|definition| (definition.name.as_str(), ValueDef::Enum(definition))),
        )
        .collect::<HashMap<_, _>>();
    let mut visiting = HashSet::new();
    let mut emitted = HashSet::new();
    let mut order = Vec::with_capacity(definitions.len());

    for definition in &program.structs {
        visit_value_type(
            ValueDef::Struct(definition),
            &definitions,
            &mut visiting,
            &mut emitted,
            &mut order,
            signatures,
        )?;
    }
    for definition in &program.enums {
        visit_value_type(
            ValueDef::Enum(definition),
            &definitions,
            &mut visiting,
            &mut emitted,
            &mut order,
            signatures,
        )?;
    }
    Ok(order)
}

fn visit_value_type<'a>(
    definition: ValueDef<'a>,
    definitions: &HashMap<&str, ValueDef<'a>>,
    visiting: &mut HashSet<String>,
    emitted: &mut HashSet<String>,
    order: &mut Vec<ValueDef<'a>>,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    let name = definition.name();
    if emitted.contains(name) {
        return Ok(());
    }
    if !visiting.insert(name.to_string()) {
        return Err(diag(
            definition.span(),
            &format!("type '{name}' participates in a recursive by-value cycle"),
        )
        .with_note(
            "recursive values need an explicit indirection/ownership type, which is not implemented yet",
        ));
    }

    let dependencies = match definition {
        ValueDef::Struct(definition) => definition
            .fields
            .iter()
            .map(|field| &field.ty)
            .collect::<Vec<_>>(),
        ValueDef::Enum(definition) => definition
            .variants
            .iter()
            .flat_map(|variant| variant.payloads.iter().map(|payload| &payload.ty))
            .collect::<Vec<_>>(),
    };
    for dependency_type in dependencies {
        if let Type::Named(dependency_name) = signatures.canonical_type(dependency_type)
            && let Some(dependency) = definitions.get(dependency_name.as_str())
        {
            visit_value_type(
                *dependency,
                definitions,
                visiting,
                emitted,
                order,
                signatures,
            )?;
        }
    }

    visiting.remove(name);
    emitted.insert(name.to_string());
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

fn constant_c_value(value: &ConstantValue) -> String {
    match value {
        ConstantValue::I64(value) => format!("INT64_C({value})"),
        ConstantValue::Bool(value) => if *value { "true" } else { "false" }.to_string(),
        ConstantValue::Str(value) => c_string(value),
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
