use std::collections::HashMap;

use crate::ast::{BinOp, Expr, ExprKind, Function, Program, Stmt, StmtKind, Type, UnaryOp};

#[derive(Debug, Clone)]
pub struct Signature {
    pub params: Vec<Type>,
    pub ret: Type,
}

pub type Signatures = HashMap<String, Signature>;

pub fn check(program: &Program) -> Result<Signatures, String> {
    let mut signatures = HashMap::new();

    for function in &program.functions {
        if function.name == "print" {
            return Err(diag(function.line, "'print' is a built-in function name"));
        }
        if signatures.contains_key(&function.name) {
            return Err(diag(
                function.line,
                &format!("duplicate function '{}'", function.name),
            ));
        }
        signatures.insert(
            function.name.clone(),
            Signature {
                params: function
                    .params
                    .iter()
                    .map(|param| param.ty.clone())
                    .collect(),
                ret: function.ret.clone(),
            },
        );
    }

    let Some(main) = signatures.get("main") else {
        return Err("program requires fn main() -> i64 { ... }".to_string());
    };
    if !main.params.is_empty() || main.ret != Type::I64 {
        return Err("main must have signature fn main() -> i64".to_string());
    }

    for function in &program.functions {
        check_function(function, &signatures)?;
    }

    Ok(signatures)
}

fn check_function(function: &Function, signatures: &Signatures) -> Result<(), String> {
    let mut env = HashMap::new();
    for param in &function.params {
        env.insert(param.name.clone(), param.ty.clone());
    }

    check_block(&function.body, &mut env, &function.ret, signatures)?;

    if function.ret != Type::Void && !block_guarantees_return(&function.body) {
        return Err(diag(
            function.line,
            &format!(
                "function '{}' can reach the end without returning {}",
                function.name,
                function.ret.name()
            ),
        ));
    }

    Ok(())
}

fn check_block(
    body: &[Stmt],
    env: &mut HashMap<String, Type>,
    return_type: &Type,
    signatures: &Signatures,
) -> Result<(), String> {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Let { name, ty, expr } => {
                if env.contains_key(name) {
                    return Err(diag(
                        stmt.line,
                        &format!("'{name}' is already defined in this scope"),
                    ));
                }
                let actual = type_of_expr(expr, env, signatures)?;
                require_type(stmt.line, ty, &actual, "binding")?;
                env.insert(name.clone(), ty.clone());
            }
            StmtKind::Return(expr) => match (return_type, expr) {
                (Type::Void, None) => {}
                (Type::Void, Some(_)) => {
                    return Err(diag(stmt.line, "void functions cannot return a value"));
                }
                (_, None) => {
                    return Err(diag(
                        stmt.line,
                        &format!("expected return value of type {}", return_type.name()),
                    ));
                }
                (expected, Some(expr)) => {
                    let actual = type_of_expr(expr, env, signatures)?;
                    require_type(stmt.line, expected, &actual, "return")?;
                }
            },
            StmtKind::Expr(expr) => {
                type_of_expr(expr, env, signatures)?;
            }
            StmtKind::If { cond, body } => {
                let cond_type = type_of_expr(cond, env, signatures)?;
                require_type(stmt.line, &Type::Bool, &cond_type, "if condition")?;
                let mut nested = env.clone();
                check_block(body, &mut nested, return_type, signatures)?;
            }
            StmtKind::ForRange {
                name,
                start,
                end,
                body,
            } => {
                if env.contains_key(name) {
                    return Err(diag(
                        stmt.line,
                        &format!("loop variable '{name}' shadows an existing binding"),
                    ));
                }
                let start_type = type_of_expr(start, env, signatures)?;
                let end_type = type_of_expr(end, env, signatures)?;
                require_type(stmt.line, &Type::I64, &start_type, "range start")?;
                require_type(stmt.line, &Type::I64, &end_type, "range end")?;
                let mut nested = env.clone();
                nested.insert(name.clone(), Type::I64);
                check_block(body, &mut nested, return_type, signatures)?;
            }
        }
    }
    Ok(())
}

pub fn type_of_expr(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Type, String> {
    match &expr.kind {
        ExprKind::Int(_) => Ok(Type::I64),
        ExprKind::Bool(_) => Ok(Type::Bool),
        ExprKind::Str(_) => Ok(Type::Str),
        ExprKind::Var(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| diag(expr.line, &format!("unknown binding '{name}'"))),
        ExprKind::Call { name, args } if name == "print" => {
            if args.len() != 1 {
                return Err(diag(expr.line, "print expects exactly one argument"));
            }
            let ty = type_of_expr(&args[0], env, signatures)?;
            if !matches!(ty, Type::I64 | Type::Bool | Type::Str) {
                return Err(diag(
                    expr.line,
                    &format!("print does not support {}", ty.name()),
                ));
            }
            Ok(Type::Void)
        }
        ExprKind::Call { name, args } => {
            let Some(signature) = signatures.get(name) else {
                return Err(diag(expr.line, &format!("unknown function '{name}'")));
            };
            if args.len() != signature.params.len() {
                return Err(diag(
                    expr.line,
                    &format!(
                        "function '{name}' expects {} arguments, got {}",
                        signature.params.len(),
                        args.len()
                    ),
                ));
            }
            for (index, (arg, expected)) in args.iter().zip(&signature.params).enumerate() {
                let actual = type_of_expr(arg, env, signatures)?;
                require_type(
                    expr.line,
                    expected,
                    &actual,
                    &format!("argument {} to '{name}'", index + 1),
                )?;
            }
            Ok(signature.ret.clone())
        }
        ExprKind::Unary { op, expr: inner } => {
            let ty = type_of_expr(inner, env, signatures)?;
            match op {
                UnaryOp::Neg => {
                    require_type(expr.line, &Type::I64, &ty, "unary '-'")?;
                    Ok(Type::I64)
                }
                UnaryOp::Not => {
                    require_type(expr.line, &Type::Bool, &ty, "unary '!'")?;
                    Ok(Type::Bool)
                }
            }
        }
        ExprKind::Binary { left, op, right } => {
            let left_ty = type_of_expr(left, env, signatures)?;
            let right_ty = type_of_expr(right, env, signatures)?;
            match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                    require_type(expr.line, &Type::I64, &left_ty, "left arithmetic operand")?;
                    require_type(expr.line, &Type::I64, &right_ty, "right arithmetic operand")?;
                    Ok(Type::I64)
                }
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    require_type(expr.line, &Type::I64, &left_ty, "left comparison operand")?;
                    require_type(expr.line, &Type::I64, &right_ty, "right comparison operand")?;
                    Ok(Type::Bool)
                }
                BinOp::Eq | BinOp::Ne => {
                    if left_ty == Type::Void || right_ty == Type::Void {
                        return Err(diag(expr.line, "void values cannot be compared"));
                    }
                    require_type(expr.line, &left_ty, &right_ty, "equality operand")?;
                    Ok(Type::Bool)
                }
                BinOp::And | BinOp::Or => {
                    require_type(expr.line, &Type::Bool, &left_ty, "left boolean operand")?;
                    require_type(expr.line, &Type::Bool, &right_ty, "right boolean operand")?;
                    Ok(Type::Bool)
                }
            }
        }
    }
}

fn block_guarantees_return(body: &[Stmt]) -> bool {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Return(_) => return true,
            StmtKind::If { .. }
            | StmtKind::ForRange { .. }
            | StmtKind::Let { .. }
            | StmtKind::Expr(_) => {}
        }
    }
    false
}

fn require_type(line: usize, expected: &Type, actual: &Type, context: &str) -> Result<(), String> {
    if expected == actual {
        Ok(())
    } else {
        Err(diag(
            line,
            &format!(
                "{context}: expected {}, got {}",
                expected.name(),
                actual.name()
            ),
        ))
    }
}

fn diag(line: usize, message: &str) -> String {
    format!("line {line}: {message}")
}
