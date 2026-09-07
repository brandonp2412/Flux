use std::fs;
use std::process::Command;

use fluxc::{
    DiagnosticStage, SourceId, check_source, check_source_all, check_source_all_with_id,
    compile_to_c, diagnostics_to_json,
};

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
    assert_eq!((span.line, span.column, span.length), (3, 22, 4));
}

#[test]
fn reports_multiple_type_diagnostics_without_cascading_failed_bindings() {
    let source = r#"
fn helper(value: i64) -> i64 {
    let count: i64 = false
    print(count)
    if 42:
        return value
    return true
}

fn main() -> i64 {
    let label: str = 99
    return 0
}
"#;

    let diagnostics = check_source_all(source).expect_err("independent type errors should batch");
    assert_eq!(diagnostics.len(), 4);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.stage == DiagnosticStage::Type)
    );
    assert!(
        diagnostics[0]
            .message
            .contains("binding: expected i64, got bool")
    );
    assert!(
        diagnostics[1]
            .message
            .contains("if condition: expected bool, got i64")
    );
    assert!(
        diagnostics[2]
            .message
            .contains("return value 1: expected i64, got bool")
    );
    assert!(
        diagnostics[3]
            .message
            .contains("binding: expected str, got i64")
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("unknown binding 'count'"))
    );
}

#[test]
fn preserves_source_identity_across_parse_and_type_diagnostics() {
    let parse_source = r#"
fn main() -> i64 {
    let count: i64 = 1 @ 2
    return 0
}
"#;
    let parse_id = SourceId::new(41);
    let diagnostics = check_source_all_with_id(parse_source, parse_id)
        .expect_err("parse diagnostic should carry the caller source id");
    assert_eq!(diagnostics[0].span.unwrap().source_id, parse_id);

    let type_source = r#"
fn main() -> i64 {
    let count: i64 = false
    return 0
}
"#;
    let type_id = SourceId::new(42);
    let diagnostics = check_source_all_with_id(type_source, type_id)
        .expect_err("type diagnostic should inherit the AST source id");
    assert_eq!(diagnostics[0].stage, DiagnosticStage::Type);
    assert_eq!(diagnostics[0].span.unwrap().source_id, type_id);
}

#[test]
fn attaches_source_identity_to_nested_ast_spans() {
    let source = r#"
fn main() -> i64 {
    if 1 + 2 < 4:
        return 0
    return 1
}
"#;
    let source_id = SourceId::new(77);
    let program = fluxc::parser::parse_with_source(source, source_id).expect("source should parse");
    let function = &program.functions[0];
    assert_eq!(function.span.source_id, source_id);
    assert_eq!(function.body[0].span.source_id, source_id);
    let fluxc::ast::StmtKind::If { cond, .. } = &function.body[0].kind else {
        panic!("expected if statement");
    };
    assert_eq!(cond.span.source_id, source_id);
    let fluxc::ast::ExprKind::Binary { left, right, .. } = &cond.kind else {
        panic!("expected binary condition");
    };
    assert_eq!(left.span.source_id, source_id);
    assert_eq!(right.span.source_id, source_id);
}

#[test]
fn accepts_struct_values_literals_field_access_and_nested_layout() {
    let source = r#"
struct Profile {
    user: User
    active: bool
}

struct User {
    name: str
    age: i64
}

fn birthday(user: User) -> User {
    return User { name: user.name, age: user.age + 1 }
}

fn main() -> i64 {
    let profile: Profile = Profile { user: User { name: "Ada", age: 41 }, active: true }
    let older: User = birthday(profile.user)
    print(older.name)
    print(older.age)
    return 0
}
"#;

    check_source(source).expect("struct program should typecheck");
    let generated = compile_to_c(source).expect("struct program should compile");
    let user_pos = generated
        .find("struct flux__type_User {")
        .expect("User C struct should be emitted");
    let profile_pos = generated
        .find("struct flux__type_Profile {")
        .expect("Profile C struct should be emitted");
    assert!(
        user_pos < profile_pos,
        "dependencies should be emitted first"
    );
    assert!(generated.contains(".flux__field_name = \"Ada\""));
    assert!(generated.contains("flux__field_age"));
}

#[test]
fn rejects_invalid_struct_literals_and_field_access() {
    let missing = r#"
struct User {
    name: str
    age: i64
}
fn main() -> i64 {
    let user: User = User { name: "Ada" }
    return 0
}
"#;
    let error = check_source(missing).expect_err("missing struct field should fail");
    assert!(error.message.contains("missing field age"));

    let unknown = r#"
struct User {
    name: str
}
fn main() -> i64 {
    let user: User = User { name: "Ada", nope: 1 }
    return 0
}
"#;
    let error = check_source(unknown).expect_err("unknown struct field should fail");
    assert!(error.message.contains("has no field 'nope'"));

    let wrong_type = r#"
struct User {
    age: i64
}
fn main() -> i64 {
    let user: User = User { age: false }
    return 0
}
"#;
    let error = check_source(wrong_type).expect_err("wrong field type should fail");
    assert!(
        error
            .message
            .contains("field 'User.age': expected i64, got bool")
    );

    let bad_access = r#"
struct User {
    age: i64
}
fn main() -> i64 {
    let user: User = User { age: 1 }
    print(user.nope)
    return 0
}
"#;
    let error = check_source(bad_access).expect_err("unknown field access should fail");
    assert!(error.message.contains("has no field 'nope'"));
}

#[test]
fn rejects_unknown_struct_types_and_recursive_by_value_cycles() {
    let unknown_type = r#"
struct User {
    missing: Missing
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(unknown_type).expect_err("unknown field type should fail");
    assert!(error.message.contains("unknown type 'Missing'"));

    let recursive = r#"
struct Node {
    next: Node
}
fn main() -> i64 {
    return 0
}
"#;
    let error = compile_to_c(recursive).expect_err("recursive value struct should not lower");
    assert_eq!(error.stage, DiagnosticStage::Codegen);
    assert!(error.message.contains("recursive by-value cycle"));
}

#[test]
fn formatter_handles_struct_declarations_and_literals() {
    let source = "struct User {\n name:str\n age:i64\n}\nfn main()->i64 {\n let user:User=User{name:\"Ada\",age:41}\n print(user.name)\n return 0\n}\n";
    let expected = "struct User {\n    name: str\n    age: i64\n}\nfn main() -> i64 {\n    let user: User = User { name: \"Ada\", age: 41 }\n    print(user.name)\n    return 0\n}\n";
    let formatted = fluxc::formatter::format_source(source).expect("struct source should format");
    assert_eq!(formatted, expected);
}

#[test]
fn semantic_database_indexes_structs_and_fields() {
    let source = "struct User {\n    name: str\n}\nfn main() -> i64 {\n    return 0\n}\n";
    let database = fluxc::semantic::SemanticDatabase::analyze(source, SourceId::new(301))
        .expect("struct source should analyze");
    let definition = database
        .symbols_named("User")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::Struct)
        .expect("struct symbol should be indexed");
    assert_eq!(span_text(source, definition.span), "User");
    let field = database
        .symbols_named("name")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::StructField)
        .expect("field symbol should be indexed");
    assert_eq!(field.ty, Some(fluxc::ast::Type::Str));
}

#[test]
fn formatter_is_deterministic_and_preserves_comments() {
    let source = "fn add(a:i64,b: i64)->i64 { # header\n  let value:i64=a+b # sum\n  if value>0:\n      return value\n  else:\n      return 0\n}\n\n\nfn main()->i64 {\n    return add(1,2)\n}\n";
    let expected = "fn add(a: i64, b: i64) -> i64 { # header\n    let value: i64 = a + b # sum\n    if value > 0:\n        return value\n    else:\n        return 0\n}\n\nfn main() -> i64 {\n    return add(1, 2)\n}\n";

    let formatted = fluxc::formatter::format_source(source).expect("source should format");
    assert_eq!(formatted, expected);
    let second =
        fluxc::formatter::format_source(&formatted).expect("formatted source should parse");
    assert_eq!(second, formatted, "formatting should be idempotent");
}

#[test]
fn semantic_database_shares_symbols_and_signatures_for_editor_queries() {
    let source = "fn double(value: i64) -> i64 {\n    let result: i64 = value * 2\n    return result\n}\n\nfn main() -> i64 {\n    for i in 0..2:\n        print(double(i))\n    return 0\n}\n";
    let source_id = SourceId::new(101);
    let database = fluxc::semantic::SemanticDatabase::analyze(source, source_id)
        .expect("valid source should build a semantic database");

    let signature = database
        .signature("double")
        .expect("signature should exist");
    assert_eq!(signature.params, vec![fluxc::ast::Type::I64]);
    assert_eq!(signature.returns, vec![fluxc::ast::Type::I64]);

    let function = database
        .symbols_named("double")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::Function)
        .expect("function symbol should exist");
    assert_eq!(span_text(source, function.span), "double");

    let result = database
        .symbols_named("result")
        .next()
        .expect("binding symbol should exist");
    assert_eq!(result.ty, Some(fluxc::ast::Type::I64));
    assert_eq!(span_text(source, result.span), "result");
    assert_eq!(
        database
            .symbol_at(source_id, result.span.line, result.span.column)
            .expect("position query should resolve the binding")
            .name,
        "result"
    );

    let loop_variable = database
        .symbols_named("i")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::LoopVariable)
        .expect("loop variable should be indexed");
    assert_eq!(loop_variable.ty, Some(fluxc::ast::Type::I64));
}

#[test]
fn editor_parse_snapshot_reuses_unchanged_source_and_reparses_changes() {
    let source = "fn main() -> i64 {\n    return 0\n}\n";
    let source_id = SourceId::new(88);
    let snapshot = fluxc::parser::parse_snapshot(source, source_id);
    assert!(snapshot.is_valid());
    let unchanged = fluxc::parser::reparse_snapshot(&snapshot, source);
    assert_eq!(unchanged.fingerprint, snapshot.fingerprint);
    assert_eq!(unchanged.source_id, source_id);
    assert!(unchanged.is_valid());

    let broken = "fn main() -> i64 {\n    return true\n}\n";
    let changed = fluxc::parser::reparse_snapshot(&snapshot, broken);
    assert_ne!(changed.fingerprint, snapshot.fingerprint);
    assert!(
        changed.is_valid(),
        "parse snapshots only report syntax errors"
    );
    let program = changed.program.expect("changed source should parse");
    let diagnostics = fluxc::typecheck::check_all(&program)
        .expect_err("changed program should retain its later type error");
    assert!(diagnostics[0].message.contains("expected i64, got bool"));
}

#[test]
fn exposes_precise_declaration_binding_and_type_spans() {
    let source = "fn add(value: i64) -> i64 {\n    let doubled: i64 = value * 2\n    for i in 0..2:\n        print(i)\n    return doubled\n}\n";
    let source_id = SourceId::new(91);
    let program = fluxc::parser::parse_with_source(source, source_id).expect("source should parse");
    let function = &program.functions[0];

    assert_eq!(span_text(source, function.name_span), "add");
    assert_eq!(span_text(source, function.params[0].name_span), "value");
    assert_eq!(span_text(source, function.params[0].type_span), "i64");
    assert_eq!(span_text(source, function.return_span), "i64");

    let fluxc::ast::StmtKind::Let {
        name_span,
        type_span,
        ..
    } = &function.body[0].kind
    else {
        panic!("expected let binding");
    };
    assert_eq!(span_text(source, *name_span), "doubled");
    assert_eq!(span_text(source, *type_span), "i64");

    let fluxc::ast::StmtKind::ForRange { name_span, .. } = &function.body[1].kind else {
        panic!("expected range loop");
    };
    assert_eq!(span_text(source, *name_span), "i");
    assert_eq!(name_span.source_id, source_id);
}

#[test]
fn precise_spans_distinguish_repeated_text_occurrences() {
    let source = "fn same(same: str) -> str {\n    for i in same..same:\n        print(i)\n    return same, same\n}\n";
    let program = fluxc::parser::parse(source).expect("source should parse");
    let function = &program.functions[0];

    assert_eq!(span_text(source, function.name_span), "same");
    assert_eq!(span_text(source, function.params[0].name_span), "same");
    assert_eq!(span_text(source, function.params[0].type_span), "str");
    assert_eq!(span_text(source, function.return_span), "str");
    assert!(function.return_span.column > function.params[0].type_span.column);

    let fluxc::ast::StmtKind::ForRange { start, end, .. } = &function.body[0].kind else {
        panic!("expected range loop");
    };
    assert_eq!(span_text(source, start.span), "same");
    assert_eq!(span_text(source, end.span), "same");
    assert!(end.span.column > start.span.column);

    let fluxc::ast::StmtKind::Return(values) = &function.body[1].kind else {
        panic!("expected return statement");
    };
    assert_eq!(values.len(), 2);
    assert_eq!(span_text(source, values[0].span), "same");
    assert_eq!(span_text(source, values[1].span), "same");
    assert!(values[1].span.column > values[0].span.column);
}

#[test]
fn multi_return_type_spans_track_each_repeated_type() {
    let source = "fn pair() -> (i64, i64) {\n    return 1, 2\n}\n";
    let program = fluxc::parser::parse(source).expect("source should parse");
    let spans = &program.functions[0].return_type_spans;
    assert_eq!(spans.len(), 2);
    assert_eq!(span_text(source, spans[0]), "i64");
    assert_eq!(span_text(source, spans[1]), "i64");
    assert!(spans[1].column > spans[0].column);
}

fn span_text(source: &str, span: fluxc::SourceSpan) -> &str {
    let line = source
        .lines()
        .nth(span.line - 1)
        .expect("span line should exist");
    &line[span.column - 1..span.column - 1 + span.length]
}

#[test]
fn reports_token_level_expression_parse_spans() {
    let source = r#"
fn main() -> i64 {
    let count: i64 = 1 @ 2
    return 0
}
"#;

    let error = check_source(source).expect_err("invalid expression token should fail");
    assert_eq!(error.stage, DiagnosticStage::Parse);
    assert!(error.message.contains("unexpected character '@'"));
    let span = error.span.expect("parse error should have a token span");
    assert_eq!((span.line, span.column, span.length), (3, 24, 1));
}

#[test]
fn reports_multiple_parse_diagnostics_after_recovery() {
    let source = r#"
fn broken() -> i64
    return 1
}

fn main() -> i64 {
    let count: i64 = 1 @ 2
    return 0
}
"#;

    let diagnostics = check_source_all(source).expect_err("both parse errors should be reported");
    assert_eq!(diagnostics.len(), 2);
    assert!(
        diagnostics[0]
            .message
            .contains("functions must open their body with '{'")
    );
    assert!(diagnostics[1].message.contains("unexpected character '@'"));
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.stage == DiagnosticStage::Parse)
    );

    let second_span = diagnostics[1]
        .span
        .expect("recovered expression diagnostic should retain its span");
    assert_eq!(
        (second_span.line, second_span.column, second_span.length),
        (7, 24, 1)
    );
}

#[test]
fn diagnostics_carry_labels_notes_fixes_and_machine_json() {
    let missing_brace = r#"
fn main() -> i64
    return 0
}
"#;
    let error = check_source(missing_brace).expect_err("missing brace should offer a fix");
    assert_eq!(error.fixes.len(), 1);
    assert_eq!(error.fixes[0].replacement, " {");
    assert!(error.fixes[0].message.contains("body opener"));

    let bad_argument = r#"
fn identity(value: i64) -> i64 {
    return value
}

fn main() -> i64 {
    return identity(false)
}
"#;
    let error = check_source(bad_argument).expect_err("bad call argument should be labelled");
    assert_eq!(error.labels.len(), 1);
    assert!(error.labels[0].message.contains("identity"));
    assert_eq!(error.labels[0].span.line, 2);

    let json = diagnostics_to_json(&[error]);
    assert!(json.starts_with("[{") && json.ends_with("]"));
    assert!(json.contains("\"stage\":\"type\""));
    assert!(json.contains("\"labels\":[{"));
    assert!(json.contains("\"source_id\":0"));
}

#[test]
fn check_json_cli_emits_clean_machine_readable_output() {
    let path = std::env::temp_dir().join(format!("flux-json-{}.flux", std::process::id()));
    fs::write(
        &path,
        "fn main() -> i64 {\n    let count: i64 = false\n    return 0\n}\n",
    )
    .expect("temporary Flux source should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_fluxc"))
        .arg("check")
        .arg(&path)
        .arg("--json")
        .output()
        .expect("fluxc should run");
    let _ = fs::remove_file(&path);

    assert!(!output.status.success());
    assert!(
        output.stderr.is_empty(),
        "JSON mode must not mix human stderr output"
    );
    let stdout = String::from_utf8(output.stdout).expect("JSON output must be UTF-8");
    assert!(stdout.starts_with("{\"ok\":false,\"source_id\":"));
    assert!(stdout.contains("\"diagnostics\":[{"));
    assert!(stdout.contains("\"stage\":\"type\""));
    assert!(stdout.trim_end().ends_with("}"));
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
    assert_eq!((span.line, span.column, span.length), (5, 10, 1));
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
fn accepts_explicit_else_return_error_propagation() {
    let source = r#"
fn load(path: str) -> (str, error) {
    if path == "":
        return "", error("path is required")
    return "config", nil
}

fn load_config(path: str) -> (str, error) {
    let data: str, err: error = load(path) else return
    print(data)
    return data, nil
}

fn main() -> i64 {
    let data: str, err: error = load_config("settings")
    if err != nil:
        print(err)
    return 0
}
"#;

    check_source(source).expect("explicit error propagation should typecheck");
    let generated = compile_to_c(source).expect("explicit error propagation should compile");
    assert!(generated.contains(".v1 != NULL"));
    assert!(generated.contains("return flux__return_"));
}

#[test]
fn rejects_else_return_without_final_error() {
    let source = r#"
fn pair() -> (i64, bool) {
    return 1, true
}

fn forward() -> (i64, bool) {
    let value: i64, ok: bool = pair() else return
    return value, ok
}

fn main() -> i64 {
    return 0
}
"#;

    let error = check_source(source).expect_err("else return needs a final error value");
    assert!(
        error
            .message
            .contains("final destructured value to have type error")
    );
}

#[test]
fn rejects_else_return_when_return_shape_differs() {
    let source = r#"
fn load() -> (str, error) {
    return "config", nil
}

fn load_count() -> (i64, error) {
    let data: str, err: error = load() else return
    print(data)
    return 1, nil
}

fn main() -> i64 {
    return 0
}
"#;

    let error = check_source(source).expect_err("else return must forward the exact shape");
    assert!(
        error
            .message
            .contains("can only forward an exact return shape")
    );
}

#[test]
fn rejects_else_return_on_scalar_binding() {
    let source = r#"
fn load() -> (str, error) {
    return "config", nil
}

fn load_config() -> (str, error) {
    let data: str = load() else return
    return data, nil
}

fn main() -> i64 {
    return 0
}
"#;

    let error = check_source(source).expect_err("else return requires destructuring");
    assert!(
        error
            .message
            .contains("requires a multi-value destructuring binding")
    );
    assert_eq!(error.stage, DiagnosticStage::Parse);
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
