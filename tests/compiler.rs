use fluxc::{DiagnosticStage, check_source, compile_to_c};

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
    assert!(error.message.contains("expected i64, got bool"));
    assert_eq!(error.stage, DiagnosticStage::Type);
    let span = error
        .span
        .expect("type diagnostic should have a source span");
    assert_eq!((span.line, span.column), (3, 5));
}

#[test]
fn rejects_missing_function_brace() {
    let source = r#"
fn main() -> i64
    return 0
}
"#;

    let error = check_source(source).expect_err("function brace should be mandatory");
    assert!(
        error
            .message
            .contains("functions must open their body with '{'")
    );
    assert_eq!(error.stage, DiagnosticStage::Parse);
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
    assert!(
        error
            .message
            .contains("if condition: expected bool, got i64")
    );
}

#[test]
fn accepts_elif_else_and_exhaustive_branch_returns() {
    let source = r#"
fn classify(value: i64) -> str {
    if value < 0:
        return "negative"
    elif value == 0:
        return "zero"
    else:
        return "positive"
}

fn main() -> i64 {
    print(classify(4))
    return 0
}
"#;

    check_source(source).expect("complete branch chain should typecheck and return");
    let generated = compile_to_c(source).expect("branch chain should compile");
    assert!(generated.matches("else {").count() >= 2);
    assert!(generated.contains("if (value < INT64_C(0))"));
    assert!(generated.contains("if (value == INT64_C(0))"));
}

#[test]
fn rejects_non_boolean_elif_condition() {
    let source = r#"
fn main() -> i64 {
    if false:
        print("no")
    elif 1:
        print("still no")
    return 0
}
"#;

    let error = check_source(source).expect_err("elif condition must be bool");
    assert!(
        error
            .message
            .contains("if condition: expected bool, got i64")
    );
    let span = error.span.expect("elif diagnostic should retain its span");
    assert_eq!((span.line, span.column), (5, 5));
}

#[test]
fn rejects_stray_else_and_elif() {
    let stray_else = r#"
fn main() -> i64 {
    else:
        return 1
    return 0
}
"#;
    let error = check_source(stray_else).expect_err("stray else must fail");
    assert!(error.message.contains("else must immediately follow"));

    let stray_elif = r#"
fn main() -> i64 {
    elif true:
        return 1
    return 0
}
"#;
    let error = check_source(stray_elif).expect_err("stray elif must fail");
    assert!(error.message.contains("elif must immediately follow"));
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
    assert!(
        error
            .message
            .contains("returns 2 values; use a destructuring binding")
    );
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
    assert!(
        error
            .message
            .contains("destructuring expects 3 values, expression returns 2")
    );
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
    assert!(
        error
            .message
            .contains("destructured binding 'value': expected bool, got i64")
    );
}

#[test]
fn accepts_multi_value_return_forwarding() {
    let source = r#"
fn load(path: str) -> (str, error) {
    if path == "":
        return "", error("path is required")
    return "config", nil
}

fn load_config(path: str) -> (str, error) {
    return load(path)
}

fn main() -> i64 {
    let data: str, err: error = load_config("settings")
    if err != nil:
        print(err)
    print(data)
    return 0
}
"#;

    check_source(source).expect("multi-value forwarding should typecheck");
    let generated = compile_to_c(source).expect("multi-value forwarding should compile");
    assert!(generated.contains("flux__forward_"));
    assert!(generated.contains("struct flux__ret_load_config"));
}

#[test]
fn rejects_multi_value_return_forwarding_type_mismatch() {
    let source = r#"
fn pair() -> (i64, bool) {
    return 1, true
}

fn forward() -> (i64, error) {
    return pair()
}

fn main() -> i64 {
    return 0
}
"#;

    let error = check_source(source).expect_err("forwarded return types must match exactly");
    assert!(
        error
            .message
            .contains("return value 2: expected error, got bool")
    );
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
    assert!(error.message.contains("return expects 2 values, got 1"));
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
    assert!(
        error
            .message
            .contains("return value 1: expected i64, got bool")
    );
}

#[test]
fn accepts_typed_error_returns_and_nil_checks() {
    let source = r#"
fn load(path: str) -> (str, error) {
    if path == "":
        return "", error("path is required")
    return "config", nil
}

fn main() -> i64 {
    let data: str, err: error = load("settings")
    if err != nil:
        print(err)
    print(data)
    return 0
}
"#;

    check_source(source).expect("error-return program should typecheck");
    let generated = compile_to_c(source).expect("error-return program should compile");
    assert!(generated.contains("flux_error_eq"));
    assert!(generated.contains("NULL"));
    assert!(generated.contains("flux_print_error"));
}

#[test]
fn rejects_non_string_error_message() {
    let source = r#"
fn main() -> i64 {
    let err: error = error(42)
    print(err)
    return 0
}
"#;

    let error = check_source(source).expect_err("error messages must be strings");
    assert!(
        error
            .message
            .contains("error message: expected str, got i64")
    );
}

#[test]
fn rejects_nil_in_non_error_binding() {
    let source = r#"
fn main() -> i64 {
    let value: i64 = nil
    return value
}
"#;

    let error = check_source(source).expect_err("nil should only type as error");
    assert!(error.message.contains("binding: expected i64, got error"));
}

#[test]
fn rejects_error_comparison_with_string() {
    let source = r#"
fn main() -> i64 {
    let err: error = error("failed")
    if err == "failed":
        print(err)
    return 0
}
"#;

    let error = check_source(source).expect_err("error values must not compare as strings");
    assert!(
        error
            .message
            .contains("equality operand: expected error, got str")
    );
}
