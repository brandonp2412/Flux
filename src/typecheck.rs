use std::collections::{HashMap, HashSet};

use crate::ast::{BinOp, Expr, ExprKind, Function, Program, Stmt, StmtKind, Type, UnaryOp};
use crate::diagnostic::{Diagnostic, DiagnosticStage, SourceSpan};

#[derive(Debug, Clone)]
pub struct Signature {
    pub params: Vec<Type>,
    pub returns: Vec<Type>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct StructFieldSignature {
    pub name: String,
    pub ty: Type,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct StructSignature {
    pub fields: Vec<StructFieldSignature>,
    pub span: SourceSpan,
}

impl StructSignature {
    pub fn field(&self, name: &str) -> Option<&StructFieldSignature> {
        self.fields.iter().find(|field| field.name == name)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Signatures {
    functions: HashMap<String, Signature>,
    structs: HashMap<String, StructSignature>,
}

impl Signatures {
    pub fn get(&self, name: &str) -> Option<&Signature> {
        self.functions.get(name)
    }

    pub fn struct_type(&self, name: &str) -> Option<&StructSignature> {
        self.structs.get(name)
    }

    pub fn structs(&self) -> &HashMap<String, StructSignature> {
        &self.structs
    }

    fn contains_function(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    fn insert_function(&mut self, name: String, signature: Signature) {
        self.functions.insert(name, signature);
    }
}

pub fn check(program: &Program) -> Result<Signatures, Diagnostic> {
    check_all(program).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .next()
            .expect("check_all always returns at least one diagnostic on failure")
    })
}

pub fn check_all(program: &Program) -> Result<Signatures, Vec<Diagnostic>> {
    let mut signatures = Signatures::default();
    let mut diagnostics = Vec::new();

    for definition in &program.structs {
        if signatures.structs.contains_key(&definition.name) {
            diagnostics.push(diag(
                definition.name_span,
                &format!("duplicate struct '{}'", definition.name),
            ));
            continue;
        }
        signatures.structs.insert(
            definition.name.clone(),
            StructSignature {
                fields: definition
                    .fields
                    .iter()
                    .map(|field| StructFieldSignature {
                        name: field.name.clone(),
                        ty: field.ty.clone(),
                        span: field.name_span,
                    })
                    .collect(),
                span: definition.name_span,
            },
        );
    }

    for definition in &program.structs {
        for field in &definition.fields {
            if let Err(diagnostic) = require_known_type(field.type_span, &field.ty, &signatures) {
                diagnostics.push(diagnostic);
            }
        }
    }

    for function in &program.functions {
        if function.name == "print" || function.name == "error" {
            diagnostics.push(diag(
                function.name_span,
                &format!("'{}' is a built-in function name", function.name),
            ));
            continue;
        }
        if signatures.structs.contains_key(&function.name) {
            diagnostics.push(diag(
                function.name_span,
                &format!("function '{}' conflicts with a struct name", function.name),
            ));
            continue;
        }
        if signatures.contains_function(&function.name) {
            diagnostics.push(diag(
                function.name_span,
                &format!("duplicate function '{}'", function.name),
            ));
            continue;
        }
        for param in &function.params {
            if let Err(diagnostic) = require_known_type(param.type_span, &param.ty, &signatures) {
                diagnostics.push(diagnostic);
            }
        }
        for (index, ty) in function.returns.iter().enumerate() {
            let span = function
                .return_type_spans
                .get(index)
                .copied()
                .unwrap_or(function.return_span);
            if let Err(diagnostic) = require_known_type(span, ty, &signatures) {
                diagnostics.push(diagnostic);
            }
        }
        signatures.insert_function(
            function.name.clone(),
            Signature {
                params: function
                    .params
                    .iter()
                    .map(|param| param.ty.clone())
                    .collect(),
                returns: function.returns.clone(),
                span: function.name_span,
            },
        );
    }

    match signatures.get("main") {
        None => diagnostics.push(
            Diagnostic::global(
                DiagnosticStage::Type,
                "program requires fn main() -> i64 { ... }",
            )
            .with_note("native executables enter Flux through a parameterless main returning i64"),
        ),
        Some(main) if !main.params.is_empty() || main.returns != vec![Type::I64] => {
            diagnostics.push(
                Diagnostic::global(
                    DiagnosticStage::Type,
                    "main must have signature fn main() -> i64",
                )
                .with_note(
                    "main currently receives no parameters and returns the process exit code",
                ),
            );
        }
        Some(_) => {}
    }

    for function in &program.functions {
        check_function_all(function, &signatures, &mut diagnostics);
    }

    if diagnostics.is_empty() {
        Ok(signatures)
    } else {
        Err(diagnostics)
    }
}

fn check_function_all(
    function: &Function,
    signatures: &Signatures,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut env = HashMap::new();
    for param in &function.params {
        env.insert(param.name.clone(), param.ty.clone());
    }

    check_block_all(
        &function.body,
        &mut env,
        &function.returns,
        signatures,
        diagnostics,
    );

    if !function.returns.is_empty() && !block_guarantees_return(&function.body) {
        diagnostics.push(diag(
            function.span,
            &format!(
                "function '{}' can reach the end without returning {}",
                function.name,
                return_types_name(&function.returns)
            ),
        ));
    }
}

fn check_block_all(
    body: &[Stmt],
    env: &mut HashMap<String, Type>,
    return_types: &[Type],
    signatures: &Signatures,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Let {
                name,
                ty,
                type_span,
                expr,
                ..
            } => {
                if let Err(diagnostic) = require_known_type(*type_span, ty, signatures) {
                    diagnostics.push(diagnostic);
                }
                let duplicate = env.contains_key(name);
                if duplicate {
                    diagnostics.push(diag(
                        stmt.span,
                        &format!("'{name}' is already defined in this scope"),
                    ));
                }
                match type_of_expr(expr, env, signatures) {
                    Ok(actual) => {
                        if let Err(diagnostic) = require_type(expr.span, ty, &actual, "binding") {
                            diagnostics.push(diagnostic);
                        }
                    }
                    Err(diagnostic) => diagnostics.push(diagnostic),
                }
                if !duplicate {
                    env.insert(name.clone(), ty.clone());
                }
            }
            StmtKind::LetDestructure {
                bindings,
                expr,
                else_return,
            } => {
                let actuals = match value_types_of_expr(expr, env, signatures) {
                    Ok(actuals) => Some(actuals),
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                        None
                    }
                };

                for binding in bindings {
                    if let Err(diagnostic) =
                        require_known_type(binding.type_span, &binding.ty, signatures)
                    {
                        diagnostics.push(diagnostic);
                    }
                }

                if let Some(actuals) = actuals.as_ref() {
                    if actuals.len() != bindings.len() {
                        diagnostics.push(diag(
                            stmt.span,
                            &format!(
                                "destructuring expects {} values, expression returns {}",
                                bindings.len(),
                                actuals.len()
                            ),
                        ));
                    }
                    for (binding, actual) in bindings.iter().zip(actuals) {
                        if let Err(diagnostic) = require_type(
                            stmt.span,
                            &binding.ty,
                            actual,
                            &format!("destructured binding '{}'", binding.name),
                        ) {
                            diagnostics.push(diagnostic);
                        }
                    }
                }

                for binding in bindings {
                    if env.contains_key(&binding.name) {
                        diagnostics.push(diag(
                            stmt.span,
                            &format!("'{}' is already defined in this scope", binding.name),
                        ));
                    } else {
                        env.insert(binding.name.clone(), binding.ty.clone());
                    }
                }

                if *else_return {
                    if bindings.last().map(|binding| &binding.ty) != Some(&Type::Error) {
                        diagnostics.push(diag(
                            stmt.span,
                            "'else return' requires the final destructured value to have type error",
                        ));
                    }
                    if let Some(actuals) = actuals.as_ref()
                        && actuals.as_slice() != return_types
                    {
                        diagnostics.push(diag(
                            stmt.span,
                            &format!(
                                "'else return' can only forward an exact return shape: function returns {}, call returns {}",
                                return_types_name(return_types),
                                return_types_name(actuals)
                            ),
                        ));
                    }
                }
            }
            StmtKind::Return(expressions) => {
                let actuals = if expressions.len() == 1 {
                    match value_types_of_expr(&expressions[0], env, signatures) {
                        Ok(actuals) => Some(actuals),
                        Err(diagnostic) => {
                            diagnostics.push(diagnostic);
                            None
                        }
                    }
                } else {
                    let mut actuals = Vec::with_capacity(expressions.len());
                    let mut complete = true;
                    for expr in expressions {
                        match type_of_expr(expr, env, signatures) {
                            Ok(actual) => actuals.push(actual),
                            Err(diagnostic) => {
                                diagnostics.push(diagnostic);
                                complete = false;
                            }
                        }
                    }
                    complete.then_some(actuals)
                };

                if let Some(actuals) = actuals {
                    if actuals.len() != return_types.len() {
                        diagnostics.push(diag(
                            stmt.span,
                            &format!(
                                "return expects {} values, got {}",
                                return_types.len(),
                                actuals.len()
                            ),
                        ));
                    } else {
                        for (index, (actual, expected)) in
                            actuals.iter().zip(return_types).enumerate()
                        {
                            if let Err(diagnostic) = require_type(
                                stmt.span,
                                expected,
                                actual,
                                &format!("return value {}", index + 1),
                            ) {
                                diagnostics.push(diagnostic);
                            }
                        }
                    }
                }
            }
            StmtKind::Expr(expr) => {
                if let Err(diagnostic) = type_of_expr(expr, env, signatures) {
                    diagnostics.push(diagnostic);
                }
            }
            StmtKind::If {
                cond,
                body,
                else_body,
                ..
            } => {
                match type_of_expr(cond, env, signatures) {
                    Ok(cond_type) => {
                        if let Err(diagnostic) =
                            require_type(cond.span, &Type::Bool, &cond_type, "if condition")
                        {
                            diagnostics.push(diagnostic);
                        }
                    }
                    Err(diagnostic) => diagnostics.push(diagnostic),
                }
                let mut then_env = env.clone();
                check_block_all(body, &mut then_env, return_types, signatures, diagnostics);
                let mut else_env = env.clone();
                check_block_all(
                    else_body,
                    &mut else_env,
                    return_types,
                    signatures,
                    diagnostics,
                );
            }
            StmtKind::ForRange {
                name,
                start,
                end,
                body,
                ..
            } => {
                let shadows = env.contains_key(name);
                if shadows {
                    diagnostics.push(diag(
                        stmt.span,
                        &format!("loop variable '{name}' shadows an existing binding"),
                    ));
                }
                match type_of_expr(start, env, signatures) {
                    Ok(start_type) => {
                        if let Err(diagnostic) =
                            require_type(start.span, &Type::I64, &start_type, "range start")
                        {
                            diagnostics.push(diagnostic);
                        }
                    }
                    Err(diagnostic) => diagnostics.push(diagnostic),
                }
                match type_of_expr(end, env, signatures) {
                    Ok(end_type) => {
                        if let Err(diagnostic) =
                            require_type(end.span, &Type::I64, &end_type, "range end")
                        {
                            diagnostics.push(diagnostic);
                        }
                    }
                    Err(diagnostic) => diagnostics.push(diagnostic),
                }
                let mut nested = env.clone();
                if !shadows {
                    nested.insert(name.clone(), Type::I64);
                }
                check_block_all(body, &mut nested, return_types, signatures, diagnostics);
            }
        }
    }
}

pub fn type_of_expr(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Type, Diagnostic> {
    match &expr.kind {
        ExprKind::Int(_) => Ok(Type::I64),
        ExprKind::Bool(_) => Ok(Type::Bool),
        ExprKind::Str(_) => Ok(Type::Str),
        ExprKind::Nil => Ok(Type::Error),
        ExprKind::Var(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| diag(expr.span, &format!("unknown binding '{name}'"))),
        ExprKind::Call { name, args } if name == "print" => {
            if args.len() != 1 {
                return Err(diag(expr.span, "print expects exactly one argument"));
            }
            let ty = type_of_expr(&args[0], env, signatures)?;
            if !matches!(ty, Type::I64 | Type::Bool | Type::Str | Type::Error) {
                return Err(diag(
                    expr.span,
                    &format!("print does not support {}", ty.name()),
                ));
            }
            Ok(Type::Void)
        }
        ExprKind::Call { name, args } if name == "error" => {
            if args.len() != 1 {
                return Err(diag(expr.span, "error expects exactly one string argument"));
            }
            let ty = type_of_expr(&args[0], env, signatures)?;
            require_type(expr.span, &Type::Str, &ty, "error message")?;
            Ok(Type::Error)
        }
        ExprKind::Call { name, args } => {
            let returns = check_call(expr.span, name, args, env, signatures)?;
            match returns.as_slice() {
                [] => Ok(Type::Void),
                [ty] => Ok(ty.clone()),
                _ => Err(diag(
                    expr.span,
                    &format!(
                        "function '{name}' returns {} values; use a destructuring binding",
                        returns.len()
                    ),
                )),
            }
        }
        ExprKind::StructLiteral {
            name,
            name_span,
            fields,
        } => {
            let Some(definition) = signatures.struct_type(name) else {
                return Err(diag(*name_span, &format!("unknown struct '{name}'")));
            };
            let mut seen = HashSet::new();
            for field in fields {
                if !seen.insert(field.name.as_str()) {
                    return Err(diag(
                        field.name_span,
                        &format!("duplicate field '{}' in {name} literal", field.name),
                    ));
                }
                let Some(expected) = definition.field(&field.name) else {
                    return Err(diag(
                        field.name_span,
                        &format!("struct '{name}' has no field '{}'", field.name),
                    )
                    .with_label(definition.span, format!("'{name}' is declared here")));
                };
                let actual = type_of_expr(&field.value, env, signatures)?;
                require_type(
                    field.value.span,
                    &expected.ty,
                    &actual,
                    &format!("field '{}.{}'", name, field.name),
                )?;
            }
            let missing = definition
                .fields
                .iter()
                .filter(|field| !seen.contains(field.name.as_str()))
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                return Err(diag(
                    expr.span,
                    &format!(
                        "{name} literal is missing field{} {}",
                        if missing.len() == 1 { "" } else { "s" },
                        missing.join(", ")
                    ),
                ));
            }
            Ok(Type::Named(name.clone()))
        }
        ExprKind::Field {
            base,
            name,
            name_span,
        } => {
            let base_ty = type_of_expr(base, env, signatures)?;
            let Type::Named(struct_name) = base_ty else {
                return Err(diag(
                    *name_span,
                    &format!(
                        "field access requires a struct value, got {}",
                        base_ty.name()
                    ),
                ));
            };
            let Some(definition) = signatures.struct_type(&struct_name) else {
                return Err(diag(base.span, &format!("unknown struct '{struct_name}'")));
            };
            definition
                .field(name)
                .map(|field| field.ty.clone())
                .ok_or_else(|| {
                    diag(
                        *name_span,
                        &format!("struct '{struct_name}' has no field '{name}'"),
                    )
                    .with_label(definition.span, format!("'{struct_name}' is declared here"))
                })
        }
        ExprKind::Unary { op, expr: inner } => {
            let ty = type_of_expr(inner, env, signatures)?;
            match op {
                UnaryOp::Neg => {
                    require_type(expr.span, &Type::I64, &ty, "unary '-'")?;
                    Ok(Type::I64)
                }
                UnaryOp::Not => {
                    require_type(expr.span, &Type::Bool, &ty, "unary '!'")?;
                    Ok(Type::Bool)
                }
            }
        }
        ExprKind::Binary { left, op, right } => {
            let left_ty = type_of_expr(left, env, signatures)?;
            let right_ty = type_of_expr(right, env, signatures)?;
            match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                    require_type(expr.span, &Type::I64, &left_ty, "left arithmetic operand")?;
                    require_type(expr.span, &Type::I64, &right_ty, "right arithmetic operand")?;
                    Ok(Type::I64)
                }
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    require_type(expr.span, &Type::I64, &left_ty, "left comparison operand")?;
                    require_type(expr.span, &Type::I64, &right_ty, "right comparison operand")?;
                    Ok(Type::Bool)
                }
                BinOp::Eq | BinOp::Ne => {
                    if left_ty == Type::Void || right_ty == Type::Void {
                        return Err(diag(expr.span, "void values cannot be compared"));
                    }
                    if matches!(left_ty, Type::Named(_)) || matches!(right_ty, Type::Named(_)) {
                        return Err(diag(
                            expr.span,
                            "whole-struct equality is not defined yet; compare fields explicitly",
                        ));
                    }
                    require_type(expr.span, &left_ty, &right_ty, "equality operand")?;
                    Ok(Type::Bool)
                }
                BinOp::And | BinOp::Or => {
                    require_type(expr.span, &Type::Bool, &left_ty, "left boolean operand")?;
                    require_type(expr.span, &Type::Bool, &right_ty, "right boolean operand")?;
                    Ok(Type::Bool)
                }
            }
        }
    }
}

fn value_types_of_expr(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Vec<Type>, Diagnostic> {
    match &expr.kind {
        ExprKind::Call { name, args } if name != "print" && name != "error" => {
            check_call(expr.span, name, args, env, signatures)
        }
        _ => Ok(vec![type_of_expr(expr, env, signatures)?]),
    }
}

fn check_call(
    span: SourceSpan,
    name: &str,
    args: &[Expr],
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Vec<Type>, Diagnostic> {
    let Some(signature) = signatures.get(name) else {
        return Err(diag(span, &format!("unknown function '{name}'")));
    };
    if args.len() != signature.params.len() {
        return Err(diag(
            span,
            &format!(
                "function '{name}' expects {} arguments, got {}",
                signature.params.len(),
                args.len()
            ),
        )
        .with_label(signature.span, format!("'{name}' is declared here")));
    }
    for (index, (arg, expected)) in args.iter().zip(&signature.params).enumerate() {
        let actual = type_of_expr(arg, env, signatures)?;
        require_type(
            arg.span,
            expected,
            &actual,
            &format!("argument {} to '{name}'", index + 1),
        )
        .map_err(|diagnostic| {
            diagnostic.with_label(signature.span, format!("'{name}' is declared here"))
        })?;
    }
    Ok(signature.returns.clone())
}

fn block_guarantees_return(body: &[Stmt]) -> bool {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Return(_) => return true,
            StmtKind::If {
                body, else_body, ..
            } if !else_body.is_empty()
                && block_guarantees_return(body)
                && block_guarantees_return(else_body) =>
            {
                return true;
            }
            StmtKind::If { .. }
            | StmtKind::ForRange { .. }
            | StmtKind::Let { .. }
            | StmtKind::LetDestructure { .. }
            | StmtKind::Expr(_) => {}
        }
    }
    false
}

fn return_types_name(types: &[Type]) -> String {
    match types {
        [] => "void".to_string(),
        [ty] => ty.name().to_string(),
        _ => format!(
            "({})",
            types.iter().map(Type::name).collect::<Vec<_>>().join(", ")
        ),
    }
}

fn require_known_type(
    span: SourceSpan,
    ty: &Type,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    match ty {
        Type::Named(name) if signatures.struct_type(name).is_none() => {
            Err(diag(span, &format!("unknown type '{name}'")))
        }
        _ => Ok(()),
    }
}

fn require_type(
    span: SourceSpan,
    expected: &Type,
    actual: &Type,
    context: &str,
) -> Result<(), Diagnostic> {
    if expected == actual {
        Ok(())
    } else {
        Err(diag(
            span,
            &format!(
                "{context}: expected {}, got {}",
                expected.name(),
                actual.name()
            ),
        ))
    }
}

fn diag(span: SourceSpan, message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticStage::Type, span, message)
}
