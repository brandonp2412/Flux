use fluxc::{check_source, compile_to_c};

#[test]
fn accepts_hybrid_function_braces_and_indented_control_flow() {
    let source = r#"
fn add(a: i64, b: i64) -> i64 {
    return a + b
}

fn main() -> i64 {
    for i in 0..3:
        if i < 2:
            print(add(i, 1))
    return 0
}
"#;

    check_source(source).expect("program should typecheck");
    let generated = compile_to_c(source).expect("program should compile");
    assert!(generated.contains("for (int64_t i"));
    assert!(generated.contains("if (i < INT64_C(2))"));
}

#[test]
fn rejects_type_mismatches() {
    let source = r#"
fn main() -> i64 {
    let count: i64 = true
    return 0
}
"#;

    let error = check_source(source).expect_err("type mismatch should fail");
    assert!(error.contains("expected i64, got bool"));
}

#[test]
fn rejects_missing_function_brace() {
    let source = r#"
fn main() -> i64
    return 0
}
"#;

    let error = check_source(source).expect_err("function brace should be mandatory");
    assert!(error.contains("functions must open their body with '{'"));
}

#[test]
fn rejects_non_boolean_if_condition() {
    let source = r#"
fn main() -> i64 {
    if 1:
        print("no")
    return 0
}
"#;

    let error = check_source(source).expect_err("if condition must be bool");
    assert!(error.contains("if condition: expected bool, got i64"));
}

#[test]
fn accepts_multi_value_returns_and_typed_destructuring() {
    let source = r#"
fn divide(value: i64, by: i64) -> (i64, bool) {
    if by == 0:
        return 0, false
    return value / by, true
}

fn main() -> i64 {
    let result: i64, ok: bool = divide(84, 2)
    if ok:
        print(result)
    return 0
}
"#;

    check_source(source).expect("multi-value program should typecheck");
    let generated = compile_to_c(source).expect("multi-value program should compile");
    assert!(generated.contains("struct flux__ret_divide"));
    assert!(generated.contains("flux__multi_"));
    assert!(generated.contains(".v0"));
    assert!(generated.contains(".v1"));
}

#[test]
fn rejects_multi_value_call_in_scalar_binding() {
    let source = r#"
fn pair() -> (i64, bool) {
    return 1, true
}

fn main() -> i64 {
    let only: i64 = pair()
    return 0
}
"#;

    let error = check_source(source).expect_err("multi-value call in scalar binding must fail");
    assert!(error.contains("returns 2 values; use a destructuring binding"));
}

#[test]
fn rejects_destructuring_arity_mismatch() {
    let source = r#"
fn pair() -> (i64, bool) {
    return 1, true
}

fn main() -> i64 {
    let value: i64, ok: bool, extra: i64 = pair()
    return 0
}
"#;

    let error = check_source(source).expect_err("destructuring arity must match exactly");
    assert!(error.contains("destructuring expects 3 values, expression returns 2"));
}

#[test]
fn rejects_destructuring_type_mismatch() {
    let source = r#"
fn pair() -> (i64, bool) {
    return 1, true
}

fn main() -> i64 {
    let value: bool, ok: bool = pair()
    return 0
}
"#;

    let error = check_source(source).expect_err("destructuring types must match exactly");
    assert!(error.contains("destructured binding 'value': expected bool, got i64"));
}

#[test]
fn rejects_multi_return_arity_mismatch() {
    let source = r#"
fn pair() -> (i64, bool) {
    return 1
}

fn main() -> i64 {
    return 0
}
"#;

    let error = check_source(source).expect_err("return arity must match exactly");
    assert!(error.contains("return expects 2 values, got 1"));
}

#[test]
fn rejects_multi_return_value_type_mismatch() {
    let source = r#"
fn pair() -> (i64, bool) {
    return true, 1
}

fn main() -> i64 {
    return 0
}
"#;

    let error = check_source(source).expect_err("return value types must match exactly");
    assert!(error.contains("return value 1: expected i64, got bool"));
}
