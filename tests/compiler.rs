use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use fluxc::ir::{
    ControlFlowEdgeKind, ControlFlowEvaluationKind, ControlFlowNodeKind, ControlFlowValueKind,
};
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
    assert!(generated.contains("for (int64_t flux__local_i"));
    assert!(generated.contains("if (flux__local_i < INT64_C(2))"));
}

#[test]
fn accepts_inclusive_integer_ranges_without_end_overflow() {
    let source = r#"
fn main() -> i64 {
    var total: i64 = 0
    var visits: i64 = 0
    for value in 2..=4:
        total = total + value
    for edge in 9223372036854775807..=9223372036854775807:
        visits = edge
        continue
    print(total)
    print(visits)
    return 0
}
"#;

    check_source(source).expect("inclusive integer ranges should typecheck");
    let generated = compile_to_c(source).expect("inclusive integer ranges should lower natively");
    assert!(generated.contains("flux__range_done_"));
    assert!(generated.contains("<= flux__end_"));
    assert!(generated.contains("= (flux__local_value == flux__end_"));
    assert!(generated.contains("flux__local_value += !flux__range_done_"));
    assert!(generated.contains("flux__local_edge += !flux__range_done_"));

    let formatted =
        fluxc::formatter::format_source(source).expect("inclusive range source should format");
    assert!(formatted.contains("for value in 2..=4:"));
    assert!(formatted.contains("for edge in 9223372036854775807..=9223372036854775807:"));
    let formatted_again = fluxc::formatter::format_source(&formatted)
        .expect("formatted inclusive ranges should reparse");
    assert_eq!(formatted_again, formatted);
}

#[test]
fn accepts_break_and_continue_inside_nested_loop_control_flow() {
    let source = r#"
enum Decision {
    Keep
    Skip
}

fn main() -> i64 {
    for i in 0..6:
        if i == 2:
            continue
        match Decision.Keep():
            Decision.Keep():
                if i == 4:
                    break
            Decision.Skip():
                continue
        print(i)
    return 0
}
"#;

    check_source(source).expect("break/continue should be valid inside loop-nested blocks");
    let generated = compile_to_c(source).expect("loop control should lower natively");
    assert!(generated.contains("continue;"));
    assert!(generated.contains("break;"));
}

#[test]
fn rejects_break_and_continue_outside_loops() {
    let source = r#"
fn main() -> i64 {
    break
    continue
    return 0
}
"#;

    let diagnostics = check_source_all(source).expect_err("loop control outside loops should fail");
    assert_eq!(diagnostics.len(), 2);
    assert!(
        diagnostics[0]
            .message
            .contains("'break' is only valid inside a loop")
    );
    assert!(
        diagnostics[1]
            .message
            .contains("'continue' is only valid inside a loop")
    );
}

#[test]
fn accepts_named_required_and_default_parameters_with_native_reordering() {
    let source = r#"
const DEFAULT_COUNT: i64 = 3

fn describe(prefix: str, suffix: str = "!", *, count: i64 = DEFAULT_COUNT, label: str) -> i64 {
    print(prefix)
    print(suffix)
    print(label)
    return count
}

fn parenthesized(value: i64 = (40 + 2)) -> i64 {
    return value
}

fn main() -> i64 {
    print(describe("hello", label: "world"))
    print(describe("hi", "?", count: 5, label: "there"))
    print(parenthesized())
    return 0
}
"#;

    check_source(source).expect("named/default parameter program should typecheck");
    let generated = compile_to_c(source).expect("named/default parameters should lower natively");
    assert!(generated.contains("describe(\"hello\", \"!\", INT64_C(3), \"world\")"));
    assert!(generated.contains("describe(\"hi\", \"?\", INT64_C(5), \"there\")"));
    assert!(generated.contains("parenthesized(INT64_C(42))"));
}

#[test]
fn rejects_invalid_named_and_default_parameter_calls() {
    let missing = r#"
fn describe(prefix: str, *, label: str) -> i64 {
    print(prefix)
    print(label)
    return 0
}
fn main() -> i64 {
    return describe("hello")
}
"#;
    let error = check_source(missing).expect_err("missing required named arg should fail");
    assert!(error.message.contains("missing required argument label"));

    let positional_named = r#"
fn describe(prefix: str, *, label: str) -> i64 {
    print(prefix)
    print(label)
    return 0
}
fn main() -> i64 {
    return describe("hello", "world")
}
"#;
    let error =
        check_source(positional_named).expect_err("named-only arg passed positionally should fail");
    assert!(
        error
            .message
            .contains("accepts at most 1 positional argument")
    );

    let positional_by_name = r#"
fn describe(prefix: str, *, label: str) -> i64 {
    print(prefix)
    print(label)
    return 0
}
fn main() -> i64 {
    return describe(prefix: "hello", label: "world")
}
"#;
    let error =
        check_source(positional_by_name).expect_err("positional arg passed by name should fail");
    assert!(
        error
            .message
            .contains("'prefix' of 'describe' is positional")
    );

    let unknown = r#"
fn describe(*, label: str) -> i64 {
    print(label)
    return 0
}
fn main() -> i64 {
    return describe(nope: "hello", label: "world")
}
"#;
    let error = check_source(unknown).expect_err("unknown named arg should fail");
    assert!(error.message.contains("has no named parameter 'nope'"));

    let wrong_type = r#"
fn describe(*, count: i64) -> i64 {
    return count
}
fn main() -> i64 {
    return describe(count: false)
}
"#;
    let error = check_source(wrong_type).expect_err("wrong named arg type should fail");
    assert!(error.message.contains("named argument 'count'"));
    assert!(error.message.contains("expected i64, got bool"));
}

#[test]
fn rejects_invalid_parameter_defaults_and_call_argument_ordering() {
    let default_type = r#"
fn value(count: i64 = false) -> i64 {
    return count
}
fn main() -> i64 {
    return value()
}
"#;
    let error = check_source(default_type).expect_err("wrong default type should fail");
    assert!(error.message.contains("default for parameter 'count'"));

    let non_constant = r#"
fn value(seed: i64, next: i64 = seed + 1) -> i64 {
    return next
}
fn main() -> i64 {
    return value(1)
}
"#;
    let error = check_source(non_constant).expect_err("parameter reference in default should fail");
    assert!(
        error
            .message
            .contains("parameter defaults may reference only compile-time constants")
    );

    let bad_order = r#"
fn value(first: i64 = 1, second: i64) -> i64 {
    return first + second
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(bad_order).expect_err("required positional after default should fail");
    assert!(
        error
            .message
            .contains("required positional parameters cannot follow")
    );

    let positional_after_named = r#"
fn value(first: i64, *, second: i64) -> i64 {
    return first + second
}
fn main() -> i64 {
    return value(1, second: 2, 3)
}
"#;
    let error =
        check_source(positional_after_named).expect_err("positional after named should fail");
    assert!(
        error
            .message
            .contains("positional arguments cannot follow named arguments")
    );

    let duplicate_named = r#"
fn value(*, count: i64) -> i64 {
    return count
}
fn main() -> i64 {
    return value(count: 1, count: 2)
}
"#;
    let error = check_source(duplicate_named).expect_err("duplicate named args should fail");
    assert!(error.message.contains("duplicate named argument 'count'"));
}

#[test]
fn formatter_and_semantic_database_preserve_named_parameter_metadata() {
    let source = "const DEFAULT:i64=3\nfn describe(prefix:str=\"x\",*,count:i64=DEFAULT,label:str)->i64 {\n print(prefix)\n print(label)\n return count\n}\nfn main()->i64 {\n return describe(label:\"ok\")\n}\n";
    let expected = "const DEFAULT: i64 = 3\nfn describe(prefix: str = \"x\", *, count: i64 = DEFAULT, label: str) -> i64 {\n    print(prefix)\n    print(label)\n    return count\n}\nfn main() -> i64 {\n    return describe(label: \"ok\")\n}\n";
    let formatted =
        fluxc::formatter::format_source(source).expect("parameter source should format");
    assert_eq!(formatted, expected);

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(703))
        .expect("parameter source should analyze");
    let signature = database
        .signature("describe")
        .expect("signature should exist");
    assert_eq!(
        signature.params,
        vec![
            fluxc::ast::Type::Str,
            fluxc::ast::Type::I64,
            fluxc::ast::Type::Str
        ]
    );
    assert!(!signature.param_details[0].named_only);
    assert!(signature.param_details[1].named_only);
    assert!(signature.param_details[1].default.is_some());
    assert!(signature.param_details[2].named_only);
    assert!(signature.param_details[2].default.is_none());
}

#[test]
fn accepts_zero_runtime_interface_capability_declarations() {
    let source = r#"
interface Storage {
    fn load(path: str) -> (str, error)
    fn save(path: str, data: str, *, durable: bool) -> error
}

interface Clock {
    fn now() -> i64
}

fn main() -> i64 {
    print("interfaces")
    return 0
}
"#;

    check_source(source).expect("interface declarations should typecheck");
    let generated = compile_to_c(source).expect("interfaces should have zero-runtime lowering");
    assert!(!generated.contains("flux__fn_load"));
    assert!(!generated.contains("flux__fn_save"));
    assert!(!generated.contains("flux__fn_now"));

    let formatted = fluxc::formatter::format_source(source).expect("interfaces should format");
    assert!(formatted.contains("interface Storage {"));
    assert!(formatted.contains("    fn load(path: str) -> (str, error)"));
    assert!(formatted.contains("    fn save(path: str, data: str, *, durable: bool) -> error"));

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(705))
        .expect("interfaces should analyze");
    let storage = database
        .symbols_named("Storage")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::Interface)
        .expect("interface should be indexed");
    assert_eq!(span_text(&formatted, storage.span), "Storage");
    let load = database
        .symbols_named("load")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::InterfaceFunction)
        .expect("interface function should be indexed");
    assert_eq!(span_text(&formatted, load.span), "load");
    assert_eq!(
        load.ty,
        Some(fluxc::ast::Type::Function {
            params: vec![fluxc::ast::Type::Str],
            returns: vec![fluxc::ast::Type::Str, fluxc::ast::Type::Error],
        })
    );
    let signature = database
        .signatures()
        .interface("Storage")
        .expect("interface signature should be queryable");
    assert_eq!(signature.functions["save"].params.len(), 3);
    assert!(signature.functions["save"].param_details[2].named_only);
}

#[test]
fn composes_interfaces_without_inheritance() {
    let source = r#"
interface Readable {
    fn load(path: str) -> (str, error)
}

interface Writable {
    fn save(path: str, data: str, *, durable: bool) -> error
}

interface Storage: Readable, Writable {
    fn label() -> str
}

struct MemoryStorage {
    label: str
}

fn memory_load(storage: MemoryStorage, path: str) -> (str, error) {
    print(storage.label)
    return path, nil
}

fn memory_save(storage: MemoryStorage, path: str, data: str, *, durable: bool) -> error {
    print(storage.label)
    print(path)
    print(data)
    print(durable)
    return nil
}

fn memory_label(storage: MemoryStorage) -> str {
    return storage.label
}

impl Storage for MemoryStorage {
    load: memory_load
    save: memory_save
    label: memory_label
}

fn main() -> i64 {
    let concrete: MemoryStorage = MemoryStorage { label: "memory" }
    let storage: Storage = Storage(concrete)
    let data: str, err: error = Storage.load(storage, "settings.flux")
    print(data)
    print(err)
    print(Storage.label(storage))
    return 0
}
"#;

    check_source(source).expect("composed interface should typecheck");
    let generated = compile_to_c(source).expect("composed interface should compile");
    assert!(generated.contains("switch (receiver.tag)"));
    assert!(generated.contains("flux__fn_memory_load"));
    assert!(!generated.contains("flux__fn_memory_save"));
    assert!(generated.contains("flux__fn_memory_label"));

    let formatted = fluxc::formatter::format_source(source).expect("composition should format");
    assert!(formatted.contains("interface Storage: Readable, Writable {"));
    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(708))
        .expect("composition should analyze");
    let storage = database
        .signatures()
        .interface("Storage")
        .expect("composed interface should be queryable");
    assert_eq!(storage.functions.len(), 3);
    assert!(storage.functions.contains_key("load"));
    assert!(storage.functions.contains_key("save"));
    assert!(storage.functions.contains_key("label"));
}

#[test]
fn rejects_invalid_interface_composition() {
    let unknown = r#"
interface Storage: Missing {
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(unknown).expect_err("unknown parent should fail");
    assert!(
        error
            .message
            .contains("unknown composed interface 'Missing'")
    );

    let cycle = r#"
interface A: B {
}
interface B: A {
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(cycle).expect_err("composition cycle should fail");
    assert!(error.message.contains("composition cycle"));

    let conflict = r#"
interface Numeric {
    fn value() -> i64
}
interface Textual {
    fn value() -> str
}
interface Both: Numeric, Textual {
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(conflict).expect_err("conflicting contracts should fail");
    assert!(error.message.contains("conflicting capability 'value'"));
}

#[test]
fn accepts_explicit_function_mapped_interface_implementations() {
    let source = r#"
interface Storage {
    fn load(path: str) -> (str, error)
    fn save(path: str, data: str, *, durable: bool) -> error
}

struct FileStorage {
    root: str
}

impl Storage for FileStorage {
    load: file_load
    save: file_save
}

fn file_load(_storage: FileStorage, path: str) -> (str, error) {
    return path, nil
}

fn file_save(storage: FileStorage, path: str, data: str, *, durable: bool) -> error {
    print(storage.root)
    print(path)
    print(data)
    print(durable)
    return nil
}

fn main() -> i64 {
    return 0
}
"#;

    check_source(source).expect("interface implementation should typecheck");
    let generated = compile_to_c(source).expect("interface implementation should compile");
    assert!(!generated.contains("flux__fn_file_load"));
    assert!(!generated.contains("flux__fn_file_save"));
    assert!(!generated.contains("flux__interface_Storage"));
    assert!(!generated.contains("flux__type_FileStorage"));
    assert!(!generated.contains("vtable"));

    let formatted = fluxc::formatter::format_source(source).expect("implementation should format");
    assert!(formatted.contains("impl Storage for FileStorage {"));
    assert!(formatted.contains("    load: file_load"));
    assert!(formatted.contains("    save: file_save"));

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(706))
        .expect("implementation should analyze");
    let mapping = database
        .symbols_named("load")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::InterfaceImplementationMapping)
        .expect("implementation mapping should be indexed");
    assert_eq!(span_text(&formatted, mapping.span), "load");
    let implementation = database
        .signatures()
        .implementation("Storage", "FileStorage")
        .expect("implementation should be queryable");
    assert_eq!(implementation.functions["load"], "file_load");
    assert_eq!(implementation.functions["save"], "file_save");
}

#[test]
fn statically_dispatches_interface_calls_to_mapped_free_functions() {
    let source = r#"
interface Storage {
    fn load(path: str) -> (str, error)
    fn save(path: str, data: str, *, durable: bool) -> error
}

struct FileStorage {
    root: str
}

impl Storage for FileStorage {
    load: file_load
    save: file_save
}

fn file_load(storage: FileStorage, path: str) -> (str, error) {
    print(storage.root)
    return path, nil
}

fn file_save(storage: FileStorage, path: str, data: str, *, durable: bool) -> error {
    print(storage.root)
    print(path)
    print(data)
    print(durable)
    return nil
}

fn reload(storage: FileStorage, path: str) -> (str, error) {
    return Storage.load(storage, path)
}

fn main() -> i64 {
    let storage: FileStorage = FileStorage { root: "/tmp" }
    let data: str, err: error = Storage.load(storage, "config.flux")
    if err != nil:
        print(err)
    let save_err: error = Storage.save(storage, "config.flux", data, durable: true)
    if save_err != nil:
        print(save_err)
    print(data)
    return 0
}
"#;

    check_source(source).expect("static interface dispatch should typecheck");
    let generated = compile_to_c(source).expect("static interface dispatch should compile");
    assert!(generated.contains("flux__fn_file_load(flux__local_storage"));
    assert!(generated.contains("flux__fn_file_save(flux__local_storage"));
    assert!(!generated.contains("vtable"));
    assert!(!generated.contains("dynamic_dispatch"));

    let formatted = fluxc::formatter::format_source(source).expect("static calls should format");
    assert!(formatted.contains("Storage.load(storage, \"config.flux\")"));
    assert!(formatted.contains("Storage.save(storage, \"config.flux\", data, durable: true)"));
}

#[test]
fn supports_allocation_free_interface_values_and_dynamic_dispatch() {
    let source = r#"
interface Storage {
    fn load(path: str) -> (str, error)
    fn label() -> str
}

struct FileStorage {
    root: str
}

struct MemoryStorage {
    name: str
}

impl Storage for FileStorage {
    load: file_load
    label: file_label
}

impl Storage for MemoryStorage {
    load: memory_load
    label: memory_label
}

fn file_load(_storage: FileStorage, path: str) -> (str, error) {
    return path, nil
}
fn file_label(storage: FileStorage) -> str {
    return storage.root
}
fn memory_load(_storage: MemoryStorage, path: str) -> (str, error) {
    return path, nil
}
fn memory_label(storage: MemoryStorage) -> str {
    return storage.name
}

fn load_any(storage: Storage, path: str) -> (str, error) {
    return Storage.load(storage, path)
}

fn label_any(storage: Storage) -> str {
    return Storage.label(storage)
}

fn choose(memory: bool) -> Storage {
    if memory:
        let ram: MemoryStorage = MemoryStorage { name: "ram" }
        return Storage(ram)
    let file: FileStorage = FileStorage { root: "/tmp" }
    return Storage(file)
}

fn main() -> i64 {
    let storage: Storage = choose(true)
    print(label_any(storage))
    let data: str, err: error = load_any(storage, "config.flux")
    if err != nil:
        print(err)
    print(data)
    return 0
}
"#;

    check_source(source).expect("interface values and dynamic dispatch should typecheck");
    let generated = compile_to_c(source).expect("interface values should lower natively");
    assert!(generated.contains("struct flux__iface_Storage"));
    assert!(generated.contains("flux__iface_pack_Storage_FileStorage"));
    assert!(generated.contains("flux__iface_pack_Storage_MemoryStorage"));
    assert!(generated.contains("flux__iface_call_Storage_load"));
    assert!(generated.contains("flux__fn_file_load"));
    assert!(generated.contains("flux__fn_memory_load"));
    assert!(generated.contains("flux__fn_file_label"));
    assert!(generated.contains("flux__fn_memory_label"));
    assert!(generated.contains("switch (receiver.tag)"));
    assert!(generated.contains("case flux__iface_tag_Storage_FileStorage"));
    assert!(generated.contains("case flux__iface_tag_Storage_MemoryStorage"));
    assert!(generated.contains("struct flux__iface_ret_Storage_load"));
    assert!(!generated.contains("malloc("));
    assert!(!generated.contains("vtable"));
}

#[test]
fn rejects_invalid_interface_value_conversions_and_layout_embedding() {
    let missing_impl = r#"
interface Storage {
    fn label() -> str
}
struct FileStorage {
    root: str
}
fn main() -> i64 {
    let file: FileStorage = FileStorage { root: "/tmp" }
    let storage: Storage = Storage(file)
    return 0
}
"#;
    let error = check_source(missing_impl).expect_err("packing requires implementation");
    assert!(
        error
            .message
            .contains("does not implement interface 'Storage'")
    );

    let wrong_arity = r#"
interface Storage {
    fn label() -> str
}
struct FileStorage {
    root: str
}
fn file_label(storage: FileStorage) -> str {
    return storage.root
}
impl Storage for FileStorage {
    label: file_label
}
fn main() -> i64 {
    let storage: Storage = Storage()
    return 0
}
"#;
    let error = check_source(wrong_arity).expect_err("packing requires one value");
    assert!(error.message.contains("expects exactly one concrete value"));

    let embedded = r#"
interface Storage {
    fn label() -> str
}
struct Wrapper {
    storage: Storage
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(embedded).expect_err("interface values are not layout fields yet");
    assert!(
        error
            .message
            .contains("not inside struct/enum by-value layouts")
    );
}

#[test]
fn rejects_invalid_static_interface_dispatch() {
    let missing_impl = r#"
interface Storage {
    fn load(path: str) -> str
}
struct MemoryStorage {
    name: str
}
fn main() -> i64 {
    let storage: MemoryStorage = MemoryStorage { name: "memory" }
    let value: str = Storage.load(storage, "config")
    print(value)
    return 0
}
"#;
    let error = check_source(missing_impl).expect_err("receiver must implement interface");
    assert!(
        error
            .message
            .contains("does not implement interface 'Storage'")
    );

    let missing_receiver = r#"
interface Storage {
    fn load(path: str) -> str
}
struct MemoryStorage {
    name: str
}
fn file_load(storage: MemoryStorage, path: str) -> str {
    print(storage.name)
    return path
}
impl Storage for MemoryStorage {
    load: file_load
}
fn main() -> i64 {
    let value: str = Storage.load()
    print(value)
    return 0
}
"#;
    let error = check_source(missing_receiver).expect_err("static call requires receiver");
    assert!(error.message.contains("requires a concrete receiver"));

    let unknown_capability = r#"
interface Storage {
    fn load(path: str) -> str
}
struct MemoryStorage {
    name: str
}
fn file_load(storage: MemoryStorage, path: str) -> str {
    print(storage.name)
    return path
}
impl Storage for MemoryStorage {
    load: file_load
}
fn main() -> i64 {
    let storage: MemoryStorage = MemoryStorage { name: "memory" }
    let value: str = Storage.missing(storage, "config")
    print(value)
    return 0
}
"#;
    let error = check_source(unknown_capability).expect_err("unknown capability should fail");
    assert!(error.message.contains("has no capability 'missing'"));

    let wrong_arg = r#"
interface Storage {
    fn load(path: str) -> str
}
struct MemoryStorage {
    name: str
}
fn file_load(storage: MemoryStorage, path: str) -> str {
    print(storage.name)
    return path
}
impl Storage for MemoryStorage {
    load: file_load
}
fn main() -> i64 {
    let storage: MemoryStorage = MemoryStorage { name: "memory" }
    let value: str = Storage.load(storage, 42)
    print(value)
    return 0
}
"#;
    let error = check_source(wrong_arg).expect_err("capability arguments remain typed");
    assert!(error.message.contains("expected str, got i64"));

    let named_enum_payload = r#"
enum Choice {
    One(i64)
}
fn main() -> i64 {
    let choice: Choice = Choice.One(value: 1)
    return 0
}
"#;
    let error = check_source(named_enum_payload).expect_err("enum payloads stay positional");
    assert!(error.message.contains("does not accept named payloads"));
}

#[test]
fn rejects_invalid_interface_implementations() {
    let missing_mapping = r#"
interface Storage {
    fn load(path: str) -> str
    fn save(path: str) -> error
}
struct FileStorage {
    root: str
}
impl Storage for FileStorage {
    load: file_load
}
fn file_load(storage: FileStorage, path: str) -> str {
    return path
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(missing_mapping).expect_err("missing capability mapping should fail");
    assert!(error.message.contains("missing capability mapping"));
    assert!(error.message.contains("save"));

    let wrong_receiver = r#"
interface Storage {
    fn load(path: str) -> str
}
struct FileStorage {
    root: str
}
fn file_load(path: str) -> str {
    return path
}
impl Storage for FileStorage {
    load: file_load
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(wrong_receiver).expect_err("receiver mismatch should fail");
    assert!(
        error
            .message
            .contains("must accept the concrete 'FileStorage' receiver")
    );

    let wrong_signature = r#"
interface Storage {
    fn save(path: str, *, durable: bool) -> error
}
struct FileStorage {
    root: str
}
fn file_save(storage: FileStorage, path: str, *, flush: bool) -> error {
    return nil
}
impl Storage for FileStorage {
    save: file_save
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(wrong_signature).expect_err("named contract mismatch should fail");
    assert!(
        error
            .message
            .contains("must keep capability name 'durable'")
    );

    let wrong_return = r#"
interface Storage {
    fn load(path: str) -> str
}
struct FileStorage {
    root: str
}
fn file_load(storage: FileStorage, path: str) -> i64 {
    return 1
}
impl Storage for FileStorage {
    load: file_load
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(wrong_return).expect_err("return mismatch should fail");
    assert!(
        error
            .message
            .contains("capability 'Storage.load' requires str")
    );

    let unknown_member = r#"
interface Storage {
    fn load(path: str) -> str
}
struct FileStorage {
    root: str
}
fn file_load(storage: FileStorage, path: str) -> str {
    return path
}
impl Storage for FileStorage {
    missing: file_load
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(unknown_member).expect_err("unknown capability should fail");
    assert!(error.message.contains("has no capability 'missing'"));
}

#[test]
fn rejects_invalid_interface_declarations() {
    let unknown_type = r#"
interface Storage {
    fn load(path: Missing) -> str
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(unknown_type).expect_err("interface types must be known");
    assert!(error.message.contains("unknown type 'Missing'"));

    let default_param = r#"
interface Storage {
    fn load(path: str = "default") -> str
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(default_param).expect_err("interface defaults should fail");
    assert!(
        error
            .message
            .contains("interface function parameters cannot declare defaults")
    );

    let duplicate_member = r#"
interface Storage {
    fn load(path: str) -> str
    fn load(path: str) -> str
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(duplicate_member).expect_err("duplicate members should fail");
    assert!(
        error
            .message
            .contains("duplicate interface function 'load'")
    );

    let conflict = r#"
interface User {
    fn name() -> str
}
struct User {
    name: str
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(conflict).expect_err("interface/type name conflicts should fail");
    assert!(error.message.contains("conflicts with an interface name"));
}

#[test]
fn accepts_concise_single_expression_functions() {
    let source = r#"
struct Point {
    x: i64
}

fn square(value: i64) -> i64 { value * value }
fn choose(flag: bool) -> i64 {
    if flag:
        return 7
    return 2
}
fn point(value: i64) -> Point { Point { x: value } }
fn pair(value: i64) -> (i64, error) { checked(value) }

fn checked(value: i64) -> (i64, error) {
    return value, nil
}

fn main() -> i64 {
    print(square(6))
    print(choose(false))
    let item: Point = point(9)
    print(item.x)
    let value: i64, err: error = pair(12)
    print(value)
    print(err)
    return 0
}
"#;

    check_source(source).expect("concise functions should typecheck");
    let generated = compile_to_c(source).expect("concise functions should compile");
    assert!(generated.contains("flux__fn_square"));
    assert!(generated.contains("return flux_mul_i64(flux__local_value, flux__local_value);"));
    assert!(generated.contains("flux__fn_pair"));

    let formatted =
        fluxc::formatter::format_source(source).expect("concise functions should format");
    assert!(formatted.contains("fn square(value: i64) -> i64 { value * value }"));
    assert!(formatted.contains("fn point(value: i64) -> Point { Point { x: value } }"));
    let formatted_again = fluxc::formatter::format_source(&formatted)
        .expect("formatted concise functions should reparse");
    assert_eq!(formatted_again, formatted);
}

#[test]
fn shell_style_calls_pipelines_redirection_and_background_are_typed_and_native() {
    let source = r#"
fn increment(value: i64) -> i64 {
    return value + 1
}

fn scale(value: i64, factor: i64) -> i64 {
    return value * factor
}

fn message() -> str {
    return "hello"
}

fn main() -> i64 {
    let result: i64 = increment 2 | scale 5
    print result
    message > "/tmp/flux-shell-output.txt"
    message >> "/tmp/flux-shell-output.txt"
    increment 1 | print &
    return result
}
"#;

    check_source(source).expect("shell-style syntax should typecheck");
    let generated = compile_to_c(source).expect("shell-style syntax should lower natively");
    assert!(generated.contains("flux__fn_scale(flux__fn_increment(INT64_C(2)), INT64_C(5))"));
    assert!(
        generated.contains(
            "flux_redirect_str(\"/tmp/flux-shell-output.txt\", false, flux__fn_message())"
        )
    );
    assert!(
        generated.contains(
            "flux_redirect_str(\"/tmp/flux-shell-output.txt\", true, flux__fn_message())"
        )
    );
    assert!(generated.contains("pid_t flux__bg_pid_"));
    assert!(generated.contains("waitpid("));

    let formatted = fluxc::formatter::format_source(source).expect("shell syntax should format");
    assert!(formatted.contains("let result: i64 = increment 2 | scale 5"));
    assert!(formatted.contains("print result"));
    assert!(formatted.contains("message > \"/tmp/flux-shell-output.txt\""));
    assert!(formatted.contains("message >> \"/tmp/flux-shell-output.txt\""));
    assert!(formatted.contains("increment 1 | print &"));
    let formatted_again =
        fluxc::formatter::format_source(&formatted).expect("formatted shell syntax should reparse");
    assert_eq!(formatted_again, formatted);
}

#[test]
fn rejects_invalid_typed_shell_operations() {
    let bad_pipe = r#"
fn text() -> str {
    return "x"
}
fn consume(value: i64) -> i64 {
    return value
}
fn main() -> i64 {
    return text | consume
}
"#;
    let error = check_source(bad_pipe).expect_err("pipeline types must match");
    assert!(error.message.contains("expected i64, got str"));

    let bad_path = r#"
fn value() -> i64 {
    return 1
}
fn main() -> i64 {
    value > true
    return 0
}
"#;
    let error = check_source(bad_path).expect_err("redirection path must be str");
    assert!(
        error
            .message
            .contains("redirection path: expected str, got bool")
    );

    let bad_value = r#"
fn log() -> void {
    print "x"
}
fn main() -> i64 {
    log > "/tmp/out"
    return 0
}
"#;
    let error = check_source(bad_value).expect_err("void cannot be redirected");
    assert!(
        error
            .message
            .contains("redirection requires a scalar result, got void")
    );
}

#[test]
fn list_literals_indexing_slicing_and_comprehensions_are_typed_and_native() {
    let source = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2, 3, 4, 5]
    let middle: i64[] = values[1:4]
    let evens: i64[] = values[::2]
    let reversed: i64[] = values[::-1]
    let reverse_middle: i64[] = values[3:0:-2]
    let chained: i64[] = values[::-1][1:4:2]
    let reverse_window: i64[] = values[::-1] | skip 1 | take 2
    let spread_values: i64[] = [0, ...middle, ...reversed[::2], 9]
    let include_high: bool = true
    let conditional_values: i64[] = [0, if include_high: 7, if false: 8 else: 9, ...middle]
    print spread_values.length
    print spread_values.first
    print spread_values.last
    print conditional_values.length
    print conditional_values.first
    print conditional_values.last
    print evens.first
    print evens.last
    print reversed.first
    print reversed.last
    print reverse_middle.first
    print reverse_middle.last
    print chained.first
    print chained.last
    print reverse_window.first
    print reverse_window.last
    print values.length
    print middle.isEmpty
    print middle.isNotEmpty
    print values.first
    print values.last
    let one: i64[] = values[2:3]
    print one.single
    print middle[0]
    print values[-1]
    let doubled: i64[] = [value * 2 for value in values if value > 2]
    print doubled[0]
    print doubled[-1]
    let window: i64[] = values | skip 1 | take 3
    print window.length
    print window.first
    print window.last
    let checks: bool[] = [value > 2 for value in values]
    let has_large: bool = checks | any
    let all_large: bool = checks | every
    print has_large
    print all_large
    let no_checks: bool[] = checks[:0]
    let empty_any: bool = no_checks | any
    let empty_every: bool = no_checks | every
    print empty_any
    print empty_every
    return 0
}
"#;

    check_source(source).expect("list syntax should typecheck");
    let generated = compile_to_c(source).expect("list syntax should lower natively");
    assert!(generated.contains("struct flux__list"));
    assert!(generated.contains("flux_list_at(flux__local_middle, INT64_C(0), sizeof(int64_t))"));
    assert!(generated.contains("flux_list_at(flux__local_values, INT64_C(-1), sizeof(int64_t))"));
    assert!(generated.contains("flux_list_at_unchecked"));
    assert!(generated.contains("flux_list_slice"));
    assert!(generated.contains("ptrdiff_t stride"));
    assert!(generated.contains("list slice step cannot be zero"));
    assert!(generated.contains(".stride = sizeof(int64_t)"));
    assert!(generated.contains("flux__list_buffer_"));
    assert!(generated.contains("(flux__local_values).len"));
    assert!(generated.contains("((flux__local_middle).len == 0)"));
    assert!(generated.contains("((flux__local_middle).len != 0)"));
    assert!(generated.contains("flux_list_at(flux__local_values, INT64_C(0)"));
    assert!(generated.contains("flux_list_at(flux__local_values, INT64_C(-1)"));
    assert!(generated.contains("flux_list_single(flux__local_one)"));
    assert!(generated.contains("Flux runtime error: list.single requires exactly one element"));
    assert!(generated.contains("flux_list_skip("));
    assert!(generated.contains("flux_list_take("));
    assert!(generated.contains("flux__list_build_capacity_"));
    assert!(generated.contains("flux__list_build_source_"));
    assert!(generated.contains("flux__list_build_condition_"));
    assert!(generated.contains("Flux runtime error: constructed list is too large"));
    assert!(generated.contains("Flux runtime error: list count must be non-negative"));
    assert!(generated.contains("flux_list_any_bool"));
    assert!(generated.contains("flux_list_every_bool"));
    assert!(generated.contains("flux__local_value > INT64_C(2)"));

    let formatted = fluxc::formatter::format_source(source).expect("list source should format");
    assert!(formatted.contains("let values: i64[] = [1, 2, 3, 4, 5]"));
    assert!(formatted.contains("let middle: i64[] = values[1:4]"));
    assert!(formatted.contains("let evens: i64[] = values[::2]"));
    assert!(formatted.contains("let reversed: i64[] = values[::-1]"));
    assert!(formatted.contains("let reverse_middle: i64[] = values[3:0:-2]"));
    assert!(formatted.contains("let chained: i64[] = values[::-1][1:4:2]"));
    assert!(formatted.contains("let reverse_window: i64[] = values[::-1] | skip 1 | take 2"));
    assert!(formatted.contains("let spread_values: i64[] = [0, ...middle, ...reversed[::2], 9]"));
    assert!(formatted.contains(
        "let conditional_values: i64[] = [0, if include_high: 7, if false: 8 else: 9, ...middle]"
    ));
    assert!(formatted.contains("[value * 2 for value in values if value > 2]"));
    assert!(formatted.contains("let window: i64[] = values | skip 1 | take 3"));
    assert!(formatted.contains("let has_large: bool = checks | any"));
    assert!(formatted.contains("let all_large: bool = checks | every"));
    let formatted_again =
        fluxc::formatter::format_source(&formatted).expect("formatted lists should reparse");
    assert_eq!(formatted_again, formatted);
}

#[test]
fn rejects_invalid_list_spreads() {
    let non_list = r#"
fn main() -> i64 {
    let values: i64[] = [1, ...2]
    print values.length
    return 0
}
"#;
    let error = check_source(non_list).expect_err("spread source should require a list");
    assert!(
        error
            .message
            .contains("list spread expression must be a list")
    );

    let wrong_element = r#"
fn main() -> i64 {
    let checks: bool[] = [true, false]
    let values: i64[] = [1, ...checks]
    print values.length
    return 0
}
"#;
    let error = check_source(wrong_element).expect_err("spread elements must match the list type");
    assert!(
        error
            .message
            .contains("list element: expected i64, got bool")
    );
}

#[test]
fn rejects_invalid_list_if_elements() {
    let bad_condition = r#"
fn main() -> i64 {
    let values: i64[] = [if 1: 2]
    print values.length
    return 0
}
"#;
    let error = check_source(bad_condition).expect_err("list if condition should require bool");
    assert!(
        error
            .message
            .contains("list if condition: expected bool, got i64")
    );

    let bad_else = r#"
fn main() -> i64 {
    let values: i64[] = [if true: 1 else: false]
    print values.length
    return 0
}
"#;
    let error = check_source(bad_else).expect_err("list else branch should match its element type");
    assert!(
        error
            .message
            .contains("list else element: expected i64, got bool")
    );
}

#[test]
fn list_destructuring_patterns_infer_types_and_lower_strided_views() {
    let source = r#"
fn main() -> i64 {
    let values: i64[] = [10, 20, 30]
    let reversed: i64[] = values[::-1]
    let [first, _, last] = reversed
    print first
    print last
    return 0
}
"#;

    check_source(source).expect("exact list destructuring should typecheck");
    let generated = compile_to_c(source).expect("list destructuring should lower natively");
    assert!(generated.contains("flux__list_pattern_"));
    assert!(generated.contains(".len != 3"));
    assert!(generated.contains("Flux runtime error: list pattern requires exactly 3 elements"));
    assert!(generated.contains("flux_list_at_unchecked(flux__list_pattern_"));
    assert!(generated.contains(", 0, sizeof(int64_t)"));
    assert!(generated.contains(", 2, sizeof(int64_t)"));

    let formatted = fluxc::formatter::format_source(source).expect("list pattern should format");
    assert!(formatted.contains("let [first, _, last] = reversed"));
    let formatted_again =
        fluxc::formatter::format_source(&formatted).expect("formatted list pattern should reparse");
    assert_eq!(formatted_again, formatted);

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(741))
        .expect("list pattern should analyze semantically");
    for name in ["first", "last"] {
        let symbol = database
            .symbols_named(name)
            .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::Binding)
            .expect("list pattern binding should be indexed");
        assert_eq!(symbol.ty, Some(fluxc::ast::Type::I64));
    }
    assert!(database.symbols_named("_").next().is_none());
}

#[test]
fn list_rest_patterns_bind_zero_copy_middle_views() {
    let source = r#"
fn main() -> i64 {
    let values: i64[] = [10, 20, 30, 40, 50]
    let reversed: i64[] = values[::-1]
    let [first, ...middle, last] = reversed
    print first
    print middle.length
    print middle.first
    print middle.last
    print last
    return 0
}
"#;

    check_source(source).expect("list rest destructuring should typecheck");
    let generated = compile_to_c(source).expect("list rest destructuring should lower natively");
    assert!(generated.contains(".len < 2"));
    assert!(generated.contains("Flux runtime error: list pattern requires at least 2 elements"));
    assert!(generated.contains(".len - 1, sizeof(int64_t)"));
    assert!(generated.contains(".len = flux__list_pattern_"));
    assert!(generated.contains("flux_list_stride(flux__list_pattern_"));

    let formatted =
        fluxc::formatter::format_source(source).expect("list rest pattern should format");
    assert!(formatted.contains("let [first, ...middle, last] = reversed"));
    let formatted_again = fluxc::formatter::format_source(&formatted)
        .expect("formatted list rest pattern should reparse");
    assert_eq!(formatted_again, formatted);

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(742))
        .expect("list rest pattern should analyze semantically");
    let middle = database
        .symbols_named("middle")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::Binding)
        .expect("rest binding should be indexed");
    assert_eq!(
        middle.ty,
        Some(fluxc::ast::Type::List(Box::new(fluxc::ast::Type::I64)))
    );
}

#[test]
fn rejects_invalid_list_destructuring_patterns() {
    let non_list = r#"
fn main() -> i64 {
    let [value] = 7
    print value
    return 0
}
"#;
    let error = check_source(non_list).expect_err("list pattern source should require a list");
    assert!(
        error
            .message
            .contains("list destructuring requires a list value, got i64")
    );

    let duplicate = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let [value, value] = values
    print value
    return 0
}
"#;
    let error = check_source(duplicate).expect_err("duplicate list pattern bindings should fail");
    assert!(
        error
            .message
            .contains("duplicate list pattern binding 'value'")
    );

    let empty = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let [] = values
    return 0
}
"#;
    let error = check_source(empty).expect_err("empty list patterns should fail");
    assert!(
        error
            .message
            .contains("list destructuring requires at least one binding")
    );

    let multiple_rest = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let [...left, ...right] = values
    print left.length
    print right.length
    return 0
}
"#;
    let error = check_source(multiple_rest).expect_err("multiple list rest patterns should fail");
    assert!(
        error
            .message
            .contains("list destructuring allows only one rest pattern")
    );

    let bare_rest = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let [first, ...] = values
    print first
    return 0
}
"#;
    let error = check_source(bare_rest).expect_err("bare list rest patterns should fail");
    assert!(
        error
            .message
            .contains("list rest patterns require a binding after '...'")
    );
}

#[test]
fn rejects_invalid_slice_steps() {
    let bad_type = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2, 3]
    let selected: i64[] = values[::true]
    print selected.length
    return 0
}
"#;
    let error = check_source(bad_type).expect_err("slice step should require i64");
    assert!(error.message.contains("slice step: expected i64, got bool"));

    let zero = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2, 3]
    let selected: i64[] = values[::0]
    print selected.length
    return 0
}
"#;
    let error = check_source(zero).expect_err("literal zero slice step should fail statically");
    assert!(error.message.contains("list slice step cannot be zero"));
}

#[test]
fn rejects_invalid_take_and_skip_calls() {
    let non_list = r#"
fn main() -> i64 {
    let value: i64[] = take(7, 1)
    print value.length
    return 0
}
"#;
    let error = check_source(non_list).expect_err("take should require a list");
    assert!(
        error
            .message
            .contains("take expects a list as its first argument, got i64")
    );

    let bad_count = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let value: i64[] = skip(values, true)
    print value.length
    return 0
}
"#;
    let error = check_source(bad_count).expect_err("skip count should be i64");
    assert!(error.message.contains("skip count: expected i64, got bool"));
}

#[test]
fn rejects_removed_safe_list_access_names() {
    for removed in ["firstOrDefault", "lastOrDefault", "first_or", "last_or"] {
        let source = format!(
            "fn main() -> i64 {{\n    let values: i64[] = [1, 2]\n    let value: i64 = {removed}(values, 0)\n    print value\n    return 0\n}}\n"
        );
        let error =
            check_source(&source).expect_err("removed safe list accessor should be rejected");
        assert!(
            error
                .message
                .contains(&format!("unknown function or callable '{removed}'"))
        );
    }

    let old_property_name = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    print values.is_empty
    return 0
}
"#;
    let error =
        check_source(old_property_name).expect_err("old property spelling should be rejected");
    assert!(
        error
            .message
            .contains("list type 'i64[]' has no property 'is_empty'")
    );
}

#[test]
fn map_filter_and_where_are_typed_and_native() {
    let source = r#"
type Mapper = fn(i64) -> i64

fn double(value: i64) -> i64 {
    return value * 2
}

fn add(left: i64, right: i64) -> i64 {
    return left + right
}

fn greaterThanTwo(value: i64) -> bool {
    return value > 2
}

fn main() -> i64 {
    let values: i64[] = [1, 2, 3, 4, 5]
    let reversed: i64[] = values[::-1]
    let mapper: Mapper = double
    let mapped: i64[] = reversed | map mapper
    let filtered: i64[] = values | filter greaterThanTwo
    let selected: i64[] = where(values, greaterThanTwo)
    let chained: i64[] = values | map double | filter greaterThanTwo
    let mappedTotal: i64 = values | map double | reduce add
    let empty: i64[] = values[:0]
    let emptyMapped: i64[] = empty | map double
    let emptyFiltered: i64[] = empty | filter greaterThanTwo
    print mapped.first
    print mapped.last
    print filtered.length
    print filtered.first
    print filtered.last
    print selected.length
    print chained.length
    print chained.first
    print chained.last
    print mappedTotal
    print emptyMapped.length
    print emptyFiltered.length
    return 0
}
"#;

    check_source(source).expect("map/filter/where should typecheck");
    let generated = compile_to_c(source).expect("map/filter/where should lower natively");
    assert!(generated.contains("flux__transform_source_"));
    assert!(generated.contains("flux__transform_buffer_"));
    assert!(generated.contains("flux__transform_count_"));
    assert!(generated.contains("flux__local_mapper(flux__transform_item_"));
    assert!(generated.contains("flux__fn_greaterThanTwo(flux__transform_item_"));
    assert!(generated.matches("flux__transform_result_").count() >= 4);
    assert!(generated.contains("flux__fn_add(flux__local_mappedTotal"));
    assert!(generated.contains("flux_list_at_unchecked(flux__transform_source_"));
    assert!(generated.contains(".stride = sizeof(int64_t)"));

    let formatted =
        fluxc::formatter::format_source(source).expect("map/filter/where source should format");
    assert!(formatted.contains("let mapped: i64[] = reversed | map mapper"));
    assert!(formatted.contains("let filtered: i64[] = values | filter greaterThanTwo"));
    assert!(formatted.contains("let selected: i64[] = where(values, greaterThanTwo)"));
    assert!(formatted.contains("let chained: i64[] = values | map double | filter greaterThanTwo"));
    assert!(formatted.contains("let mappedTotal: i64 = values | map double | reduce add"));
    let formatted_again = fluxc::formatter::format_source(&formatted)
        .expect("formatted map/filter/where source should reparse");
    assert_eq!(formatted_again, formatted);
}

#[test]
fn concat_is_typed_strided_and_composable() {
    let source = r#"
fn double(value: i64) -> i64 {
    return value * 2
}

fn add(left: i64, right: i64) -> i64 {
    return left + right
}

fn main() -> i64 {
    let left: i64[] = [1, 2, 3, 4][::2]
    let right: i64[] = [5, 6, 7][::-1]
    let joined: i64[] = left | concat right
    let doubled: i64[] = left | concat right | map double
    let total: i64 = left | concat right | reduce add
    print joined.length
    print joined.first
    print joined.last
    print doubled.first
    print doubled.last
    print total
    return 0
}
"#;

    check_source(source).expect("concat should typecheck for matching concrete lists");
    let generated = compile_to_c(source).expect("concat should lower natively");
    assert!(generated.contains("flux__concat_left_"));
    assert!(generated.contains("flux__concat_right_"));
    assert!(generated.contains("flux__concat_buffer_"));
    assert!(generated.contains("Flux runtime error: concatenated list is too large"));
    assert!(generated.contains("flux_list_at_unchecked(flux__concat_left_"));
    assert!(generated.contains("flux_list_at_unchecked(flux__concat_right_"));
    assert!(generated.contains("flux__fn_double(flux__transform_item_"));
    assert!(generated.contains("flux__fn_add(flux__local_total"));

    let formatted = fluxc::formatter::format_source(source).expect("concat source should format");
    assert!(formatted.contains("let joined: i64[] = left | concat right"));
    assert!(formatted.contains("let doubled: i64[] = left | concat right | map double"));
    assert!(formatted.contains("let total: i64 = left | concat right | reduce add"));
    let formatted_again =
        fluxc::formatter::format_source(&formatted).expect("formatted concat should reparse");
    assert_eq!(formatted_again, formatted);
}

#[test]
fn rejects_invalid_concat_calls() {
    let non_list = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let joined: i64[] = concat(7, values)
    print joined.length
    return 0
}
"#;
    let error = check_source(non_list).expect_err("concat should require a list first argument");
    assert!(
        error
            .message
            .contains("concat expects a list as its first argument")
    );

    let mismatch = r#"
fn main() -> i64 {
    let numbers: i64[] = [1, 2]
    let flags: bool[] = [true, false]
    let joined: i64[] = numbers | concat flags
    print joined.length
    return 0
}
"#;
    let error = check_source(mismatch).expect_err("concat list element types must match");
    assert!(
        error
            .message
            .contains("concat right list: expected i64[], got bool[]")
    );
}

#[test]
fn distinct_is_stable_scalar_and_composable() {
    let source = r#"
fn double(value: i64) -> i64 {
    return value * 2
}

fn add(left: i64, right: i64) -> i64 {
    return left + right
}

fn main() -> i64 {
    let values: i64[] = [3, 1, 3, 2, 1, 2][::-1]
    let unique: i64[] = values | distinct
    let doubled: i64[] = values | distinct | map double
    let total: i64 = values | distinct | reduce add
    let words: str[] = ["flux", "native", "flux", "fast", "native"]
    let uniqueWords: str[] = words | distinct
    print unique.length
    print unique.first
    print unique.last
    print doubled.first
    print doubled.last
    print total
    print uniqueWords.length
    print uniqueWords.first
    print uniqueWords.last
    return 0
}
"#;

    check_source(source).expect("distinct should typecheck for scalar lists");
    let generated = compile_to_c(source).expect("distinct should lower natively");
    assert!(generated.contains("flux__distinct_source_"));
    assert!(generated.contains("flux__distinct_buffer_"));
    assert!(generated.contains("flux__distinct_duplicate_"));
    assert!(generated.contains("strcmp(flux__distinct_buffer_"));
    assert!(generated.contains("flux_list_at_unchecked(flux__distinct_source_"));
    assert!(generated.contains("flux__fn_double(flux__transform_item_"));
    assert!(generated.contains("flux__fn_add(flux__local_total"));

    let formatted = fluxc::formatter::format_source(source).expect("distinct source should format");
    assert!(formatted.contains("let unique: i64[] = values | distinct"));
    assert!(formatted.contains("let doubled: i64[] = values | distinct | map double"));
    assert!(formatted.contains("let total: i64 = values | distinct | reduce add"));
    let formatted_again =
        fluxc::formatter::format_source(&formatted).expect("formatted distinct should reparse");
    assert_eq!(formatted_again, formatted);
}

#[test]
fn rejects_invalid_distinct_calls() {
    let non_list = r#"
fn main() -> i64 {
    let value: i64 = distinct(7)
    print value
    return 0
}
"#;
    let error = check_source(non_list).expect_err("distinct should require a list");
    assert!(error.message.contains("distinct expects a list argument"));

    let nested = r#"
fn main() -> i64 {
    let nested: i64[][] = [[1], [1]]
    let unique: i64[][] = nested | distinct
    print unique.length
    return 0
}
"#;
    let error = check_source(nested).expect_err("distinct should reject aggregate equality");
    assert!(
        error
            .message
            .contains("distinct requires list elements with scalar equality, got i64[]")
    );
}

#[test]
fn flatten_is_typed_strided_and_composable() {
    let source = r#"
fn double(value: i64) -> i64 {
    return value * 2
}

fn add(left: i64, right: i64) -> i64 {
    return left + right
}

fn main() -> i64 {
    let left: i64[] = [1, 2, 3][::2]
    let right: i64[] = [4, 5, 6][::-1]
    let nested: i64[][] = [left, right]
    let flat: i64[] = nested[::-1] | flatten
    let doubled: i64[] = nested[::-1] | flatten | map double
    let total: i64 = nested[::-1] | flatten | reduce add
    print flat.length
    print flat.first
    print flat.last
    print doubled.first
    print doubled.last
    print total
    return 0
}
"#;

    check_source(source).expect("flatten should typecheck for nested lists");
    let generated = compile_to_c(source).expect("flatten should lower natively");
    assert!(generated.contains("flux__flatten_source_"));
    assert!(generated.contains("flux__flatten_capacity_"));
    assert!(generated.contains("flux__flatten_buffer_"));
    assert!(generated.contains("Flux runtime error: flattened list is too large"));
    assert!(generated.contains("sizeof(struct flux__list)"));
    assert!(generated.contains("flux_list_at_unchecked(flux__flatten_inner_"));
    assert!(generated.contains("flux__fn_double(flux__transform_item_"));
    assert!(generated.contains("flux__fn_add(flux__local_total"));

    let formatted = fluxc::formatter::format_source(source).expect("flatten source should format");
    assert!(formatted.contains("let flat: i64[] = nested[::-1] | flatten"));
    assert!(formatted.contains("let doubled: i64[] = nested[::-1] | flatten | map double"));
    assert!(formatted.contains("let total: i64 = nested[::-1] | flatten | reduce add"));
    let formatted_again =
        fluxc::formatter::format_source(&formatted).expect("formatted flatten should reparse");
    assert_eq!(formatted_again, formatted);
}

#[test]
fn rejects_invalid_flatten_calls() {
    let non_list = r#"
fn main() -> i64 {
    let value: i64[] = flatten(7)
    print value.length
    return 0
}
"#;
    let error = check_source(non_list).expect_err("flatten should require a list");
    assert!(
        error
            .message
            .contains("flatten expects a nested list argument")
    );

    let flat_list = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let flattened: i64[] = values | flatten
    print flattened.length
    return 0
}
"#;
    let error = check_source(flat_list).expect_err("flatten should require nested list elements");
    assert!(
        error
            .message
            .contains("flatten expects a list whose elements are lists")
    );
}

#[test]
fn sorted_is_immutable_ordered_strided_and_composable() {
    let source = r#"
fn double(value: i64) -> i64 {
    return value * 2
}

fn add(left: i64, right: i64) -> i64 {
    return left + right
}

fn main() -> i64 {
    let values: i64[] = [4, 1, 3, 2, 3]
    let ordered: i64[] = values[::-1] | sorted
    let doubled: i64[] = values[::-1] | sorted | map double
    let total: i64 = values[::-1] | sorted | reduce add
    let words: str[] = ["beta", "alpha", "gamma", "alpha"]
    let orderedWords: str[] = words | sorted
    let flags: bool[] = [true, false, true, false]
    let orderedFlags: bool[] = flags | sorted
    print values.first
    print values.last
    print ordered.first
    print ordered.last
    print doubled.first
    print doubled.last
    print total
    print orderedWords.first
    print orderedWords.last
    print orderedFlags.first
    print orderedFlags.last
    return 0
}
"#;

    check_source(source).expect("sorted should typecheck for ordered scalar lists");
    let generated = compile_to_c(source).expect("sorted should lower natively");
    assert!(generated.contains("flux__sorted_source_"));
    assert!(generated.contains("flux__sorted_buffer_"));
    assert!(generated.contains("flux__sorted_key_"));
    assert!(generated.contains("strcmp(flux__sorted_key_"));
    assert!(generated.contains("flux_list_at_unchecked(flux__sorted_source_"));
    assert!(generated.contains("flux__fn_double(flux__transform_item_"));
    assert!(generated.contains("flux__fn_add(flux__local_total"));

    let formatted = fluxc::formatter::format_source(source).expect("sorted source should format");
    assert!(formatted.contains("let ordered: i64[] = values[::-1] | sorted"));
    assert!(formatted.contains("let doubled: i64[] = values[::-1] | sorted | map double"));
    assert!(formatted.contains("let total: i64 = values[::-1] | sorted | reduce add"));
    let formatted_again =
        fluxc::formatter::format_source(&formatted).expect("formatted sorted should reparse");
    assert_eq!(formatted_again, formatted);
}

#[test]
fn rejects_invalid_sorted_calls() {
    let non_list = r#"
fn main() -> i64 {
    let value: i64[] = sorted(7)
    print value.length
    return 0
}
"#;
    let error = check_source(non_list).expect_err("sorted should require a list");
    assert!(error.message.contains("sorted expects a list argument"));

    let nested = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let nested: i64[][] = [values, values]
    let ordered: i64[][] = nested | sorted
    print ordered.length
    return 0
}
"#;
    let error = check_source(nested).expect_err("sorted should reject aggregate ordering");
    assert!(
        error
            .message
            .contains("sorted requires ordered scalar list elements, got i64[]")
    );
}

#[test]
fn chunked_is_zero_copy_strided_and_composable() {
    let source = r#"
fn add(left: i64, right: i64) -> i64 {
    return left + right
}

fn main() -> i64 {
    let values: i64[] = [1, 2, 3, 4, 5]
    let chunks: i64[][] = values[::-1] | chunked 2
    let firstChunk: i64[] = chunks[0]
    let lastChunk: i64[] = chunks[-1]
    let flattened: i64[] = chunks | flatten
    let total: i64 = chunks | flatten | reduce add
    let emptyChunks: i64[][] = values[:0] | chunked 3
    print chunks.length
    print firstChunk.first
    print firstChunk.last
    print lastChunk.single
    print flattened.first
    print flattened.last
    print total
    print emptyChunks.length
    return 0
}
"#;

    check_source(source).expect("chunked should typecheck for concrete lists");
    let generated = compile_to_c(source).expect("chunked should lower natively");
    assert!(generated.contains("flux__chunked_source_"));
    assert!(generated.contains("flux__chunked_buffer_"));
    assert!(generated.contains("Flux runtime error: chunked size must be greater than zero"));
    assert!(generated.contains(".stride = flux__chunked_source_"));
    assert!(generated.contains("(ptrdiff_t)flux__chunked_start_"));
    assert!(generated.contains("sizeof(struct flux__list)"));
    assert!(generated.contains("flux__fn_add(flux__local_total"));

    let formatted = fluxc::formatter::format_source(source).expect("chunked source should format");
    assert!(formatted.contains("let chunks: i64[][] = values[::-1] | chunked 2"));
    assert!(formatted.contains("let flattened: i64[] = chunks | flatten"));
    assert!(formatted.contains("let total: i64 = chunks | flatten | reduce add"));
    let formatted_again =
        fluxc::formatter::format_source(&formatted).expect("formatted chunked should reparse");
    assert_eq!(formatted_again, formatted);
}

#[test]
fn rejects_invalid_chunked_calls() {
    let non_list = r#"
fn main() -> i64 {
    let chunks: i64[][] = chunked(7, 2)
    print chunks.length
    return 0
}
"#;
    let error = check_source(non_list).expect_err("chunked should require a list");
    assert!(
        error
            .message
            .contains("chunked expects a list as its first argument")
    );

    let bad_size = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let chunks: i64[][] = values | chunked true
    print chunks.length
    return 0
}
"#;
    let error = check_source(bad_size).expect_err("chunked size should be i64");
    assert!(
        error
            .message
            .contains("chunked size: expected i64, got bool")
    );

    let zero_size = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let chunks: i64[][] = values | chunked 0
    print chunks.length
    return 0
}
"#;
    let error = check_source(zero_size).expect_err("zero chunk size should fail statically");
    assert!(
        error
            .message
            .contains("chunked size must be greater than zero")
    );
}

#[test]
fn fuses_map_filter_pipelines_and_terminal_reductions() {
    let source = r#"
fn double(value: i64) -> i64 {
    return value * 2
}

fn greaterThanFour(value: i64) -> bool {
    return value > 4
}

fn add(left: i64, right: i64) -> i64 {
    return left + right
}

fn main() -> i64 {
    let values: i64[] = [1, 2, 3, 4]
    let chained: i64[] = values | map double | filter greaterThanFour
    let total: i64 = values | map double | filter greaterThanFour | reduce add
    let seeded: i64 = values | map double | filter greaterThanFour | fold 10 add
    print chained.length
    print total
    print seeded
    return 0
}
"#;

    check_source(source).expect("fused sequence pipelines should typecheck");
    let generated = compile_to_c(source).expect("fused sequence pipelines should lower natively");
    assert_eq!(
        generated.matches("int64_t flux__transform_buffer_").count(),
        1,
        "only the collection-producing chain should allocate a result buffer"
    );
    assert!(generated.contains("flux__fn_double(flux__transform_item_"));
    assert!(generated.contains("if (!flux__fn_greaterThanFour(flux__transform_value_"));
    assert!(generated.contains("flux__fn_add(flux__local_total, flux__transform_value_"));
    assert!(generated.contains("flux__fn_add(flux__local_seeded, flux__transform_value_"));
}

#[test]
fn rejects_invalid_map_filter_and_where_calls() {
    let non_list = r#"
fn double(value: i64) -> i64 {
    return value * 2
}
fn main() -> i64 {
    let mapped: i64[] = map(7, double)
    print mapped.length
    return 0
}
"#;
    let error = check_source(non_list).expect_err("map should require a list");
    assert!(
        error
            .message
            .contains("map expects a list as its first argument")
    );

    let wrong_map_input = r#"
fn textLength(_value: str) -> i64 {
    return 1
}
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let mapped: i64[] = values | map textLength
    print mapped.length
    return 0
}
"#;
    let error = check_source(wrong_map_input).expect_err("map callback input should match");
    assert!(
        error
            .message
            .contains("map callback must have type fn(i64) -> U")
    );

    let wrong_filter = r#"
fn double(value: i64) -> i64 {
    return value * 2
}
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let filtered: i64[] = values | filter double
    print filtered.length
    return 0
}
"#;
    let error = check_source(wrong_filter).expect_err("filter callback should return bool");
    assert!(error.message.contains("filter callback"));
    assert!(error.message.contains("expected fn(i64) -> bool"));

    let wrong_where = r#"
fn predicate(value: bool) -> bool {
    return value
}
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let filtered: i64[] = values | where predicate
    print filtered.length
    return 0
}
"#;
    let error = check_source(wrong_where).expect_err("where callback input should match");
    assert!(error.message.contains("where callback"));
    assert!(error.message.contains("expected fn(i64) -> bool"));
}

#[test]
fn fold_and_reduce_are_typed_and_native() {
    let source = r#"
type Reducer = fn(i64, i64) -> i64

fn add(left: i64, right: i64) -> i64 {
    return left + right
}

fn allPositive(current: bool, value: i64) -> bool {
    return current && value > 0
}

fn main() -> i64 {
    let values: i64[] = [1, 2, 3, 4, 5]
    let reversed: i64[] = values[::-1]
    let total: i64 = reversed | reduce add
    let reducer: Reducer = add
    let boundTotal: i64 = values | reduce reducer
    let directTotal: i64 = fold(values, 0, add)
    let positive: bool = values | fold true allPositive
    let empty: i64[] = values[:0]
    let emptyTotal: i64 = empty | fold 7 add
    print total
    print boundTotal
    print directTotal
    print positive
    print emptyTotal
    return 0
}
"#;

    check_source(source).expect("fold/reduce should typecheck");
    let generated = compile_to_c(source).expect("fold/reduce should lower natively");
    assert!(generated.contains("Flux runtime error: reduce requires a non-empty list"));
    assert!(generated.contains("flux__reduce_source_"));
    assert!(generated.contains("flux__fn_add(flux__local_total"));
    assert!(generated.contains("flux__local_reducer(flux__local_boundTotal"));
    assert!(generated.contains("flux__fn_add(flux__local_directTotal"));
    assert!(generated.contains("flux__fn_allPositive(flux__local_positive"));
    assert!(generated.contains("flux_list_at_unchecked(flux__reduce_source_"));

    let formatted =
        fluxc::formatter::format_source(source).expect("fold/reduce source should format");
    assert!(formatted.contains("let total: i64 = reversed | reduce add"));
    assert!(formatted.contains("let boundTotal: i64 = values | reduce reducer"));
    assert!(formatted.contains("let directTotal: i64 = fold(values, 0, add)"));
    assert!(formatted.contains("let positive: bool = values | fold true allPositive"));
    let formatted_again = fluxc::formatter::format_source(&formatted)
        .expect("formatted fold/reduce source should reparse");
    assert_eq!(formatted_again, formatted);
}

#[test]
fn rejects_invalid_fold_and_reduce_calls() {
    let non_list = r#"
fn add(left: i64, right: i64) -> i64 {
    return left + right
}
fn main() -> i64 {
    let total: i64 = fold(7, 0, add)
    print total
    return 0
}
"#;
    let error = check_source(non_list).expect_err("fold should require a list");
    assert!(
        error
            .message
            .contains("fold expects a list as its first argument")
    );

    let wrong_reducer = r#"
fn wrong(value: i64) -> i64 {
    return value
}
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let total: i64 = values | reduce wrong
    print total
    return 0
}
"#;
    let error = check_source(wrong_reducer).expect_err("reduce reducer shape should be checked");
    assert!(error.message.contains("reduce reducer"));
    assert!(error.message.contains("expected fn(i64, i64) -> i64"));

    let wrong_accumulator = r#"
fn add(left: i64, right: i64) -> i64 {
    return left + right
}
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let valid: bool = values | fold true add
    print valid
    return 0
}
"#;
    let error = check_source(wrong_accumulator).expect_err("fold reducer accumulator should match");
    assert!(error.message.contains("fold reducer"));
    assert!(error.message.contains("expected fn(bool, i64) -> bool"));
}

#[test]
fn rejects_invalid_boolean_sequence_operations() {
    let source = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let result: bool = any(values)
    print result
    return 0
}
"#;
    let error = check_source(source).expect_err("any should require bool[]");
    assert!(
        error
            .message
            .contains("any input: expected bool[], got i64[]")
    );
}

#[test]
fn list_iteration_and_indexed_iteration_are_typed_and_native() {
    let source = r#"
fn main() -> i64 {
    let values: i64[] = [2, 4, 6]
    for value in values:
        print value
    for index, value in values:
        print(index + value)
    return 0
}
"#;

    check_source(source).expect("list iteration should typecheck");
    let generated = compile_to_c(source).expect("list iteration should lower natively");
    assert!(generated.contains("struct flux__list flux__iter_source_"));
    assert!(generated.contains("flux_list_at_unchecked(flux__iter_source_"));
    assert!(generated.contains("int64_t flux__local_index = 0"));
    assert!(generated.contains("int64_t flux__local_value = *((int64_t *)flux_list_at_unchecked"));

    let formatted = fluxc::formatter::format_source(source).expect("list iteration should format");
    assert!(formatted.contains("for value in values:"));
    assert!(formatted.contains("for index, value in values:"));
    let formatted_again = fluxc::formatter::format_source(&formatted)
        .expect("formatted list iteration should reparse");
    assert_eq!(formatted_again, formatted);
}

#[test]
fn rejects_invalid_list_iteration_forms() {
    let non_list = r#"
fn main() -> i64 {
    for value in 7:
        print value
    return 0
}
"#;
    let error = check_source(non_list).expect_err("foreach source must be a list");
    assert!(
        error
            .message
            .contains("for-loop source must be a list, got i64")
    );

    let indexed_range = r#"
fn main() -> i64 {
    for index, value in 0..3:
        print(index + value)
    return 0
}
"#;
    let error = check_source(indexed_range).expect_err("ranges bind one variable");
    assert!(
        error
            .message
            .contains("range loops bind exactly one loop variable")
    );

    let duplicate = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    for value, value in values:
        print value
    return 0
}
"#;
    let error = check_source(duplicate).expect_err("foreach bindings must be unique");
    assert!(error.message.contains("duplicate for-loop binding 'value'"));
}

#[test]
fn rejects_unsafe_or_invalid_bootstrap_list_forms() {
    let mixed = r#"
fn main() -> i64 {
    let values: i64[] = [1, "two"]
    print values[0]
    return 0
}
"#;
    let error = check_source(mixed).expect_err("mixed list element types should fail");
    assert!(
        error
            .message
            .contains("list element: expected i64, got str")
    );

    let bad_property = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    print values.capacity
    return 0
}
"#;
    let error = check_source(bad_property).expect_err("unknown list properties should fail");
    assert!(
        error
            .message
            .contains("list type 'i64[]' has no property 'capacity'")
    );

    let bad_index = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    print values[true]
    return 0
}
"#;
    let error = check_source(bad_index).expect_err("list index must be i64");
    assert!(error.message.contains("list index: expected i64, got bool"));

    let bad_filter = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    let selected: i64[] = [value for value in values if value]
    print selected[0]
    return 0
}
"#;
    let error = check_source(bad_filter).expect_err("comprehension filter must be bool");
    assert!(
        error
            .message
            .contains("list comprehension filter: expected bool, got i64")
    );

    let returned = r#"
fn values() -> i64[] {
    return [1, 2]
}
fn main() -> i64 {
    return 0
}
"#;
    let errors = check_source_all(returned).expect_err("list returns must remain blocked");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("list values cannot be returned from functions")
    }));

    let mutable = r#"
fn main() -> i64 {
    var values: i64[] = [1, 2]
    print values[0]
    return 0
}
"#;
    let error = check_source(mutable).expect_err("mutable list bindings must remain blocked");
    assert!(
        error
            .message
            .contains("list bindings are currently immutable local values")
    );

    let stored = r#"
struct Box {
    values: i64[]
}
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(stored).expect_err("lists cannot be stored in aggregates yet");
    assert!(
        error
            .message
            .contains("list values are currently local-only")
    );
}

#[test]
fn ownership_copy_classification_is_structural_and_alias_aware() {
    let source = r#"
struct Pair {
    count: i64
    label: str
}

enum Choice {
    PairValue(Pair)
    Missing
}

type PairAlias = Pair

interface Readable {
    fn read(value: i64) -> i64
}

fn main() -> i64 {
    return 0
}
"#;

    let database = fluxc::semantic::SemanticDatabase::analyze(source, SourceId::new(1204))
        .expect("copyable value declarations should analyze");
    let signatures = database.signatures();
    assert!(signatures.is_copy_type(&fluxc::ast::Type::I64));
    assert!(signatures.is_copy_type(&fluxc::ast::Type::Bool));
    assert!(signatures.is_copy_type(&fluxc::ast::Type::Str));
    assert!(signatures.is_copy_type(&fluxc::ast::Type::Error));
    assert!(signatures.is_copy_type(&fluxc::ast::Type::Named("Pair".to_string())));
    assert!(signatures.is_copy_type(&fluxc::ast::Type::Named("PairAlias".to_string())));
    assert!(signatures.is_copy_type(&fluxc::ast::Type::Named("Choice".to_string())));
    assert!(signatures.is_copy_type(&fluxc::ast::Type::Named("Readable".to_string())));
    assert!(signatures.is_copy_type(&fluxc::ast::Type::Function {
        params: vec![fluxc::ast::Type::I64],
        returns: vec![fluxc::ast::Type::Bool],
    }));
    assert!(!signatures.is_copy_type(&fluxc::ast::Type::Void));
    assert!(!signatures.is_copy_type(&fluxc::ast::Type::List(Box::new(fluxc::ast::Type::I64,))));
}

#[test]
fn direct_non_copy_transfers_move_locals_and_reject_use_after_move() {
    let moved = r#"
fn main() -> i64 {
    let source: i64[] = [10, 20]
    let destination: i64[] = source
    print(destination[0])
    return 0
}
"#;
    check_source(moved).expect("a direct non-copy local transfer should move the source");
    compile_to_c(moved).expect("a valid non-copy local move should lower natively");

    let use_after_move = r#"
fn main() -> i64 {
    let source: i64[] = [10, 20]
    let destination: i64[] = source
    print(destination[0])
    print(source[1])
    return 0
}
"#;
    let errors = check_source_all(use_after_move).expect_err("using a moved list must fail");
    let moved_error = errors
        .iter()
        .find(|error| {
            error
                .message
                .contains("use of moved non-copy binding 'source'")
        })
        .expect("use-after-move should identify the consumed binding");
    assert!(
        moved_error
            .labels
            .iter()
            .any(|label| label.message.contains("'source' moved here"))
    );

    let copy_values = r#"
fn main() -> i64 {
    let source: i64 = 7
    let destination: i64 = source
    print(source)
    print(destination)
    return 0
}
"#;
    check_source(copy_values).expect("copy values must remain reusable after assignment");
}

#[test]
fn constant_cfg_edges_do_not_poison_ownership_from_unreachable_moves() {
    let source = r#"
const NEVER: bool = 2 > 3

fn main() -> i64 {
    let source: i64[] = [10, 20]
    if NEVER:
        let _branchMove: i64[] = source
    while false:
        let _loopMove: i64[] = source
    for _i in 4..4:
        let _rangeMove: i64[] = source
    return source.first
}
"#;

    check_source(source)
        .expect("statically unreachable moves must not make a live owner appear moved");
    compile_to_c(source).expect("constant-unreachable ownership paths should lower natively");

    let database = fluxc::semantic::SemanticDatabase::analyze(source, SourceId::new(1205))
        .expect("constant-aware CFG should analyze");
    let graph = database
        .control_flow_graph("main")
        .expect("main should expose a CFG");
    for moved_name in ["_branchMove", "_loopMove", "_rangeMove"] {
        let node = graph
            .nodes()
            .iter()
            .find(|node| {
                matches!(
                    &node.kind,
                    ControlFlowNodeKind::Binding { name, .. } if name == moved_name
                )
            })
            .expect("dead move binding should remain represented structurally");
        assert!(
            !graph.is_reachable(node.id),
            "{moved_name} should be unreachable"
        );
    }
}

#[test]
fn non_copy_list_reads_and_parameters_are_immutable_borrows() {
    let source = r#"
fn firstValue(values: i64[]) -> i64 {
    return values.first
}

fn main() -> i64 {
    let values: i64[] = [3, 4]
    print(firstValue(values))
    print(values[1])
    let tail: i64[] = values[1:]
    print(tail.first)
    print(values.first)
    return 0
}
"#;

    check_source(source).expect("list reads, slices, and parameters should not consume the owner");
}

#[test]
fn branch_moves_are_conservative_and_loop_moves_are_rejected() {
    let branch = r#"
fn main() -> i64 {
    let source: i64[] = [1]
    if true:
        let destination: i64[] = source
        print(destination.first)
    print(source.first)
    return 0
}
"#;
    let errors =
        check_source_all(branch).expect_err("a move on one branch must invalidate later use");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("use of moved non-copy binding 'source'")
    }));

    let loop_move = r#"
fn main() -> i64 {
    let source: i64[] = [1]
    for index in 0..1:
        let destination: i64[] = source
        print(destination[index])
    return 0
}
"#;
    let errors =
        check_source_all(loop_move).expect_err("fallthrough loop moves must remain rejected");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("use of moved non-copy binding 'source'")
    }));

    let break_after_move = r#"
fn main() -> i64 {
    let source: i64[] = [10, 20]
    for index in 0..2:
        let destination: i64[] = source
        print(destination[index])
        break
    return 0
}
"#;
    check_source(break_after_move)
        .expect("a non-copy loop move followed by a guaranteed break should be iteration-safe");
    compile_to_c(break_after_move).expect("a break-terminated loop move should lower natively");

    let continue_after_move = r#"
fn main() -> i64 {
    let source: i64[] = [10, 20]
    for index in 0..2:
        let destination: i64[] = source
        print(destination[index])
        continue
    return 0
}
"#;
    let errors = check_source_all(continue_after_move)
        .expect_err("a move followed by continue could consume the same value twice");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("use of moved non-copy binding 'source'")
    }));
}

#[test]
fn nested_control_flow_inherits_enclosing_loop_exit_for_move_safety() {
    let nested_if = r#"
fn consume(flag: bool) -> i64 {
    let source: i64[] = [4, 5]
    for index in 0..1:
        if flag:
            let destination: i64[] = source
            print(destination[index])
        break
    return 0
}

fn main() -> i64 {
    return consume(true)
}
"#;
    check_source(nested_if)
        .expect("a nested move should inherit a guaranteed enclosing-loop break");
    compile_to_c(nested_if).expect("nested break-terminated ownership flow should lower natively");

    let nested_match = r#"
enum Choice {
    Take
    Skip
}

fn consume(choice: Choice) -> i64 {
    let source: i64[] = [4, 5]
    for index in 0..1:
        match choice:
            Choice.Take():
                let destination: i64[] = source
                print(destination[index])
            Choice.Skip():
                print(index)
        break
    return 0
}

fn main() -> i64 {
    return consume(Choice.Take())
}
"#;
    check_source(nested_match)
        .expect("match-arm moves should inherit the enclosing loop's guaranteed break");
    compile_to_c(nested_match).expect("nested match ownership flow should lower natively");

    let nested_loops = r#"
fn safe() -> i64 {
    let source: i64[] = [4, 5]
    for outer in 0..2:
        for inner in 0..1:
            let destination: i64[] = source
            print(destination[inner])
            break
        print(outer)
        break
    return 0
}

fn main() -> i64 {
    return safe()
}
"#;
    check_source(nested_loops)
        .expect("an inner-loop move is safe when the enclosing loop also exits before repeating");
    compile_to_c(nested_loops).expect("nested loop ownership flow should lower natively");

    let unsafe_nested_loops = r#"
fn unsafe() -> i64 {
    let source: i64[] = [4, 5]
    for outer in 0..2:
        for inner in 0..1:
            let destination: i64[] = source
            print(destination[inner])
            break
        print(outer)
    return 0
}

fn main() -> i64 {
    return unsafe()
}
"#;
    let errors = check_source_all(unsafe_nested_loops)
        .expect_err("an inner-loop move must not be reused by another enclosing-loop iteration");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("use of moved non-copy binding 'source'")
    }));

    let conditional_break = r#"
fn consume(flag: bool) -> i64 {
    let source: i64[] = [4, 5]
    for index in 0..1:
        if flag:
            let destination: i64[] = source
            print(destination[index])
            break
        print(source[index])
        break
    return 0
}

fn main() -> i64 {
    return consume(false)
}
"#;
    check_source(conditional_break)
        .expect("a move on a breaking branch must not poison the sibling fallthrough path");
    compile_to_c(conditional_break).expect("branch-specific break ownership should lower natively");

    let post_break_use = r#"
fn consume(flag: bool) -> i64 {
    let source: i64[] = [4, 5]
    for index in 0..1:
        if flag:
            let destination: i64[] = source
            print(destination[index])
            break
        break
    print(source.first)
    return 0
}

fn main() -> i64 {
    return consume(false)
}
"#;
    let errors = check_source_all(post_break_use)
        .expect_err("a move that exits through break must still invalidate post-loop ownership");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("use of moved non-copy binding 'source'")
    }));

    let unsafe_nested_if = r#"
fn consume(flag: bool) -> i64 {
    let source: i64[] = [4, 5]
    for index in 0..2:
        if flag:
            let destination: i64[] = source
            print(destination[index])
        print(index)
    return 0
}

fn main() -> i64 {
    return consume(true)
}
"#;
    let errors = check_source_all(unsafe_nested_if)
        .expect_err("nested moves must still fail when the enclosing loop can repeat");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("use of moved non-copy binding 'source'")
    }));
}

#[test]
fn return_terminated_move_paths_do_not_poison_reachable_ownership_state() {
    let branch_return = r#"
fn consumeOrRead(flag: bool) -> i64 {
    let source: i64[] = [7, 8]
    if flag:
        let destination: i64[] = source
        print(destination.first)
        return 1
    print(source.last)
    return 0
}

fn main() -> i64 {
    return consumeOrRead(false)
}
"#;
    check_source(branch_return)
        .expect("a move on a returning branch must not invalidate the surviving branch");
    compile_to_c(branch_return).expect("return-separated ownership paths should lower natively");

    let loop_return = r#"
fn consumeOrRead(flag: bool) -> i64 {
    let source: i64[] = [7, 8]
    while flag:
        let destination: i64[] = source
        print(destination.first)
        return 1
    print(source.last)
    return 0
}

fn main() -> i64 {
    return consumeOrRead(false)
}
"#;
    check_source(loop_return)
        .expect("a loop move followed by return cannot be consumed on another iteration");
    compile_to_c(loop_return).expect("return-terminated loop moves should lower natively");
}

#[test]
fn rejects_invalid_concise_single_expression_functions() {
    let void_body = r#"
fn log(value: str) -> void { print(value) }
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(void_body).expect_err("concise void functions should fail");
    assert!(error.message.contains("require a non-void return type"));

    let mismatch = r#"
fn value() -> i64 { "wrong" }
fn main() -> i64 {
    return value()
}
"#;
    let error = check_source(mismatch).expect_err("concise return type mismatch should fail");
    assert!(error.message.contains("expected i64, got str"));
}

#[test]
fn accepts_first_class_named_function_values_and_higher_order_calls() {
    let source = r#"
type Number = i64
type Mapper = fn(Number) -> Number

fn double(value: i64) -> i64 {
    return value * 2
}

fn increment(value: i64) -> i64 {
    return value + 1
}

fn apply(transform: Mapper, value: i64) -> i64 {
    return transform(value)
}

fn choose(double_it: bool) -> Mapper {
    if double_it:
        return double
    return increment
}

fn main() -> i64 {
    let mapper: Mapper = double
    print(apply(mapper, 21))
    let selected: Mapper = choose(false)
    print(selected(41))
    return 0
}
"#;

    check_source(source).expect("first-class function values should typecheck");
    let generated =
        compile_to_c(source).expect("function values should lower to function pointers");
    assert!(generated.contains("typedef int64_t (*flux__fn_i64__to__i64)(int64_t);"));
    assert!(generated.contains("flux__fn_i64__to__i64 flux__local_mapper = flux__fn_double;"));
    assert!(generated.contains("return flux__local_transform(flux__local_value);"));
    assert!(generated.contains("return flux__fn_increment;"));
}

#[test]
fn accepts_capture_free_anonymous_functions_and_inline_higher_order_calls() {
    let source = r#"
type Mapper = fn(i64) -> i64

fn apply(transform: Mapper, value: i64) -> i64 {
    return transform(value)
}

fn main() -> i64 {
    let double: Mapper = fn(value: i64) { value * 2 }
    print(apply(double, 21))
    let values: i64[] = [1, 2, 3]
    let doubled: i64[] = map(values, fn(value: i64) { value * 2 })
    let total: i64 = fold(doubled, 0, fn(total: i64, value: i64) { total + value })
    print(total)
    return apply(fn(value: i64) -> i64 { value + 1 }, 41)
}
"#;

    check_source(source).expect("capture-free anonymous functions should typecheck");
    let generated = compile_to_c(source).expect("anonymous functions should lower natively");
    assert!(generated.contains("flux__lambda_0_"));
    assert!(generated.contains("return flux_mul_i64(flux__local_value, INT64_C(2));"));
    assert!(generated.contains("flux__fn_i64__to__i64 flux__local_double = flux__lambda_0_"));
    assert!(generated.contains("return flux__fn_apply(flux__lambda_0_"));
}

#[test]
fn formatter_and_semantic_database_preserve_anonymous_functions() {
    let source = "type Mapper=fn(i64)->i64\nfn main()->i64 {\n let mapper:Mapper=fn(value:i64)->i64 { value+1 }\n return mapper(41)\n}\n";
    let expected = "type Mapper = fn(i64) -> i64\nfn main() -> i64 {\n    let mapper: Mapper = fn(value: i64) -> i64 { value + 1 }\n    return mapper(41)\n}\n";
    let formatted =
        fluxc::formatter::format_source(source).expect("anonymous functions should format");
    assert_eq!(formatted, expected);
    let formatted_again = fluxc::formatter::format_source(&formatted)
        .expect("formatted anonymous functions should reparse");
    assert_eq!(formatted_again, formatted);

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(705))
        .expect("anonymous function source should analyze");
    let parameter = database
        .symbols_named("value")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::Parameter)
        .expect("anonymous parameter should be indexed");
    assert_eq!(parameter.ty, Some(fluxc::ast::Type::I64));
}

#[test]
fn rejects_anonymous_function_captures_and_invalid_bodies() {
    let capture = r#"
type Mapper = fn(i64) -> i64
fn main() -> i64 {
    let factor: i64 = 2
    let mapper: Mapper = fn(value: i64) { value * factor }
    return mapper(21)
}
"#;
    let error = check_source(capture).expect_err("captures must wait for safe closure semantics");
    assert!(
        error
            .message
            .contains("anonymous function captures outer binding 'factor'")
    );

    let wrong_return = r#"
type Mapper = fn(i64) -> bool
fn main() -> i64 {
    let mapper: Mapper = fn(value: i64) -> bool { value + 1 }
    print(mapper(1))
    return 0
}
"#;
    let error = check_source(wrong_return).expect_err("explicit anonymous return type must match");
    assert!(error.message.contains("anonymous function body"));
    assert!(error.message.contains("expected bool, got i64"));

    let unused = r#"
type Mapper = fn(i64) -> i64
fn main() -> i64 {
    let mapper: Mapper = fn(value: i64) { 1 }
    return mapper(0)
}
"#;
    let error = check_source(unused).expect_err("anonymous parameters obey no-warning cleanliness");
    assert!(
        error
            .message
            .contains("unused anonymous function parameter 'value'")
    );
}

#[test]
fn rejects_invalid_first_class_function_value_usage() {
    let wrong_shape = r#"
type Mapper = fn(i64) -> i64
fn label(value: str) -> str {
    return value
}
fn main() -> i64 {
    let mapper: Mapper = label
    return 0
}
"#;
    let error = check_source(wrong_shape).expect_err("wrong function shape should fail");
    assert!(
        error
            .message
            .contains("binding: expected fn(i64) -> i64, got fn(str) -> str")
    );

    let wrong_call = r#"
type Mapper = fn(i64) -> i64
fn double(value: i64) -> i64 {
    return value * 2
}
fn apply(transform: Mapper) -> i64 {
    return transform(false)
}
fn main() -> i64 {
    return apply(double)
}
"#;
    let error = check_source(wrong_call).expect_err("wrong callback argument should fail");
    assert!(
        error
            .message
            .contains("argument 1 to function value 'transform'")
    );
    assert!(error.message.contains("expected i64, got bool"));

    let named_call = r#"
type Mapper = fn(i64) -> i64
fn double(value: i64) -> i64 {
    return value * 2
}
fn apply(transform: Mapper) -> i64 {
    return transform(value: 2)
}
fn main() -> i64 {
    return apply(double)
}
"#;
    let error = check_source(named_call).expect_err("function values should be positional-only");
    assert!(
        error
            .message
            .contains("function values accept positional arguments only")
    );

    let multi_return = r#"
type Loader = fn(str) -> (str, error)
fn main() -> i64 {
    return 0
}
"#;
    let error =
        check_source(multi_return).expect_err("multi-return function values are not supported yet");
    assert!(
        error
            .message
            .contains("first-class function types currently support zero or one return value")
    );
}

#[test]
fn formatter_and_semantic_database_preserve_function_types() {
    let source = "type Mapper=fn(i64)->i64\nfn double(value:i64)->i64 {\n return value*2\n}\nfn apply(transform:Mapper,value:i64)->i64 {\n return transform(value)\n}\nfn main()->i64 {\n let mapper:Mapper=double\n return apply(mapper,21)\n}\n";
    let expected = "type Mapper = fn(i64) -> i64\nfn double(value: i64) -> i64 {\n    return value * 2\n}\nfn apply(transform: Mapper, value: i64) -> i64 {\n    return transform(value)\n}\nfn main() -> i64 {\n    let mapper: Mapper = double\n    return apply(mapper, 21)\n}\n";
    let formatted = fluxc::formatter::format_source(source).expect("function types should format");
    assert_eq!(formatted, expected);

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(704))
        .expect("function type source should analyze");
    let signature = database
        .signature("apply")
        .expect("apply signature should exist");
    assert_eq!(
        signature.params[0],
        fluxc::ast::Type::Function {
            params: vec![fluxc::ast::Type::I64],
            returns: vec![fluxc::ast::Type::I64],
        }
    );
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
fn accepts_struct_destructuring_with_inferred_field_types_and_single_evaluation() {
    let source = r#"
struct User {
    name: str
    age: i64
}

type Person = User

fn make_user() -> User {
    return User { name: "Ada", age: 42 }
}

fn main() -> i64 {
    let Person { name, age: years } = make_user()
    print(name)
    print(years)
    return 0
}
"#;

    check_source(source).expect("struct destructuring should typecheck");
    let generated = compile_to_c(source).expect("struct destructuring should lower natively");
    assert_eq!(generated.matches("= flux__fn_make_user();").count(), 1);
    assert!(generated.contains("flux__field_name"));
    assert!(generated.contains("flux__field_age"));
}

#[test]
fn accepts_nested_struct_destructuring_patterns() {
    let source = r#"
struct User {
    name: str
    age: i64
}
struct Profile {
    user: User
    active: bool
}
fn make_profile() -> Profile {
    return Profile { user: User { name: "Ada", age: 42 }, active: true }
}
fn main() -> i64 {
    let Profile { user: User { name, age: years }, active } = make_profile()
    print(name)
    print(years)
    print(active)
    return 0
}
"#;

    check_source(source).expect("nested struct patterns should typecheck");
    let generated = compile_to_c(source).expect("nested struct patterns should lower natively");
    assert_eq!(generated.matches("= flux__fn_make_profile();").count(), 1);
    assert!(generated.contains("flux__destructure_0.flux__field_user.flux__field_name"));
    assert!(generated.contains("flux__destructure_0.flux__field_user.flux__field_age"));

    let formatted = fluxc::formatter::format_source(source).expect("nested pattern should format");
    assert!(formatted.contains("Profile { user: User { name, age: years }, active }"));
    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(705))
        .expect("nested pattern should analyze");
    let years = database
        .symbols_named("years")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::PatternBinding)
        .expect("nested binding should be indexed");
    assert_eq!(years.ty, Some(fluxc::ast::Type::I64));
}

#[test]
fn rejects_invalid_struct_destructuring_patterns() {
    let unknown_field = r#"
struct User {
    name: str
}
fn main() -> i64 {
    let user: User = User { name: "Ada" }
    let User { nope } = user
    return 0
}
"#;
    let error = check_source(unknown_field).expect_err("unknown pattern field should fail");
    assert!(error.message.contains("has no field 'nope'"));

    let wrong_value = r#"
struct User {
    name: str
}
struct Other {
    name: str
}
fn main() -> i64 {
    let other: Other = Other { name: "Ada" }
    let User { name } = other
    return 0
}
"#;
    let error = check_source(wrong_value).expect_err("wrong pattern value type should fail");
    assert!(
        error
            .message
            .contains("struct destructuring: expected User, got Other")
    );

    let wrong_nested_type = r#"
struct User {
    name: str
}
struct Profile {
    user: User
}
fn main() -> i64 {
    let profile: Profile = Profile { user: User { name: "Ada" } }
    let Profile { user: Profile { user } } = profile
    return 0
}
"#;
    let error = check_source(wrong_nested_type).expect_err("wrong nested pattern type should fail");
    assert!(
        error
            .message
            .contains("nested struct pattern: expected Profile, got User")
    );

    let shadow = r#"
struct User {
    name: str
}
fn main() -> i64 {
    let name: str = "existing"
    let user: User = User { name: "Ada" }
    let User { name } = user
    return 0
}
"#;
    let error = check_source(shadow).expect_err("pattern binding shadow should fail");
    assert!(
        error
            .message
            .contains("'name' is already defined in this scope")
    );
}

#[test]
fn formatter_and_semantic_database_preserve_struct_destructuring() {
    let source = "struct User {\n name:str\n age:i64\n}\nfn main()->i64 {\n let user:User=User{name:\"Ada\",age:42}\n let User{name,age: years}=user\n print(name)\n print(years)\n return 0\n}\n";
    let expected = "struct User {\n    name: str\n    age: i64\n}\nfn main() -> i64 {\n    let user: User = User { name: \"Ada\", age: 42 }\n    let User { name, age: years } = user\n    print(name)\n    print(years)\n    return 0\n}\n";
    let formatted = fluxc::formatter::format_source(source).expect("struct pattern should format");
    assert_eq!(formatted, expected);

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(702))
        .expect("struct pattern should analyze");
    let years = database
        .symbols_named("years")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::PatternBinding)
        .expect("renamed struct pattern binding should be indexed");
    assert_eq!(years.ty, Some(fluxc::ast::Type::I64));
    assert_eq!(span_text(&formatted, years.span), "years");
}

#[test]
fn accepts_exhaustive_enum_match_with_typed_payload_bindings() {
    let source = r#"
enum Outcome {
    Ok(i64)
    Error(str)
    Pending
}

fn score(outcome: Outcome) -> i64 {
    match outcome:
        Outcome.Ok(value):
            return value
        Outcome.Error(message):
            print(message)
            return -1
        Outcome.Pending():
            return 0
}

fn main() -> i64 {
    print(score(Outcome.Ok(42)))
    return 0
}
"#;

    check_source(source).expect("exhaustive enum match should typecheck");
    let generated = compile_to_c(source).expect("enum match should lower natively");
    assert!(generated.contains("switch (flux__match_"));
    assert!(generated.contains("case flux__tag_Outcome_Ok"));
    assert!(generated.contains("flux__payload_Ok.v0"));
    assert!(generated.contains("case flux__tag_Outcome_Error"));
    assert!(generated.contains("case flux__tag_Outcome_Pending"));
}

#[test]
fn accepts_struct_patterns_inside_enum_match_arms() {
    let source = r#"
struct Profile {
    name: str
    age: i64
}

struct User {
    profile: Profile
}

enum Event {
    Loaded(User)
    Empty
}

fn describe(event: Event) -> i64 {
    match event:
        Event.Loaded(User { profile: Profile { name, age: years } }):
            print(name)
            return years
        Event.Empty():
            return 0
}

fn main() -> i64 {
    let profile: Profile = Profile { name: "Ada", age: 42 }
    let user: User = User { profile: profile }
    print(describe(Event.Loaded(user)))
    return 0
}
"#;

    check_source(source).expect("struct match pattern should typecheck");
    let generated = compile_to_c(source).expect("struct match pattern should lower natively");
    assert!(
        generated.contains("payload.flux__payload_Loaded.v0.flux__field_profile.flux__field_name")
    );
    assert!(
        generated.contains("payload.flux__payload_Loaded.v0.flux__field_profile.flux__field_age")
    );

    let formatted =
        fluxc::formatter::format_source(source).expect("struct match pattern should format");
    assert!(formatted.contains("Event.Loaded(User { profile: Profile { name, age: years } }):"));
    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(703))
        .expect("struct match pattern should analyze");
    let years = database
        .symbols_named("years")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::PatternBinding)
        .expect("nested struct match binding should be indexed");
    assert_eq!(years.ty, Some(fluxc::ast::Type::I64));
    assert_eq!(span_text(&formatted, years.span), "years");
}

#[test]
fn rejects_struct_match_pattern_with_wrong_payload_type() {
    let source = r#"
struct User {
    name: str
}

enum Event {
    Count(i64)
}

fn main() -> i64 {
    let event: Event = Event.Count(42)
    match event:
        Event.Count(User { name }):
            print(name)
    return 0
}
"#;

    let error = check_source(source).expect_err("wrong struct payload pattern should fail");
    assert!(
        error
            .message
            .contains("match struct pattern: expected User, got i64")
    );
}

#[test]
fn rejects_conditional_ternary_expressions() {
    let source = r#"
fn main() -> i64 {
    let value: i64 = 1 if true else 2
    return value
}
"#;

    let error = check_source(source).expect_err("Flux must not accept ternary expressions");
    assert!(
        error
            .message
            .contains("conditional/ternary expressions are not part of Flux")
    );
}

#[test]
fn rejects_flux_comments() {
    let source = "fn main() -> i64 { # comments are forbidden\n    return 0\n}\n";
    let error = check_source(source).expect_err("Flux comments must be syntax errors");
    assert!(
        error
            .message
            .contains("comments are not part of Flux syntax")
    );
}

#[test]
fn accepts_raw_string_literals_and_canonicalizes_them() {
    let source = r#"
fn main() -> i64 {
    let path: str = r"C:\Flux\bin\"
    let pattern: str = r"\d+\w+"
    print(path)
    print(pattern)
    return 0
}
"#;

    check_source(source).expect("raw strings should typecheck as str");
    let generated = compile_to_c(source).expect("raw strings should lower as ordinary strings");
    assert!(generated.contains("C:\\\\Flux\\\\bin\\\\"));
    assert!(generated.contains("\\\\d+\\\\w+"));

    let formatted = fluxc::formatter::format_source(source).expect("raw strings should format");
    assert!(formatted.contains("let path: str = \"C:\\\\Flux\\\\bin\\\\\""));
    assert!(formatted.contains("let pattern: str = \"\\\\d+\\\\w+\""));
    assert!(!formatted.contains("r\""));
    let formatted_again =
        fluxc::formatter::format_source(&formatted).expect("canonical strings should reparse");
    assert_eq!(formatted_again, formatted);

    let escaped = r#"
fn main() -> i64 {
    let pattern: str = "\d"
    print(pattern)
    return 0
}
"#;
    let error = check_source(escaped).expect_err("ordinary strings should still validate escapes");
    assert!(error.message.contains("unsupported escape '\\d'"));
}

#[test]
fn accepts_multiline_string_literals_with_indent_normalization() {
    let source = r####"
fn main() -> i64 {
    let message: str = """
        Flux says "hello".
        Path: C:\\Flux
        # literal text
    """
    print(message)
    return 0
}
"####;

    check_source(source).expect("multiline strings should typecheck as str");
    let generated =
        compile_to_c(source).expect("multiline strings should lower as ordinary strings");
    assert!(generated.contains("Flux says \\\"hello\\\"."));
    assert!(generated.contains("Path: C:\\\\Flux"));
    assert!(generated.contains("# literal text"));

    let formatted =
        fluxc::formatter::format_source(source).expect("multiline strings should format");
    assert!(formatted.contains("let message: str = \"\"\""));
    assert!(formatted.contains("        Flux says \"hello\"."));
    assert!(formatted.contains("        Path: C:\\\\Flux"));
    assert!(formatted.contains("        # literal text"));
    let formatted_again = fluxc::formatter::format_source(&formatted)
        .expect("formatted multiline string should reparse");
    assert_eq!(formatted_again, formatted);

    let unterminated = r####"
fn main() -> i64 {
    let message: str = """
        never closes
}
"####;
    let error = check_source(unterminated).expect_err("unterminated multiline string must fail");
    assert!(
        error
            .message
            .contains("unterminated multiline string literal")
    );
}

#[test]
fn formatter_refuses_ternary_syntax() {
    let source = "fn main()->i64 {\n let value:i64=1 if true else 2\n return value\n}\n";
    let diagnostics = fluxc::formatter::format_source(source)
        .expect_err("formatter must reject invalid ternary syntax");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("conditional/ternary expressions are not part of Flux")
    }));
}

#[test]
fn accepts_value_producing_match_expressions_in_bindings_and_returns() {
    let source = r#"
enum Outcome {
    Ok(i64)
    Error(str)
    Pending
}

fn score(outcome: Outcome) -> i64 {
    return match outcome:
        Outcome.Ok(value): value
        Outcome.Error(_): -1
        Outcome.Pending(): 0
}

fn main() -> i64 {
    let outcome: Outcome = Outcome.Ok(42)
    let value: i64 = match outcome:
        Outcome.Ok(payload): payload + 1
        Outcome.Error(_): -1
        Outcome.Pending(): 0
    print(value)
    return score(outcome)
}
"#;

    check_source(source).expect("match expressions should typecheck");
    let generated = compile_to_c(source).expect("match expressions should lower natively");
    assert!(generated.contains("flux__match_result_"));
    assert!(generated.contains("switch (flux__match_"));
    assert!(generated.contains("flux__local_value ="));
}

#[test]
fn match_expressions_require_exhaustive_same_typed_non_void_arms() {
    let non_exhaustive = r#"
enum Choice {
    One
    Two
}
fn main() -> i64 {
    let choice: Choice = Choice.One()
    let value: i64 = match choice:
        Choice.One(): 1
    return value
}
"#;
    let error = check_source(non_exhaustive).expect_err("match expression must be exhaustive");
    assert!(error.message.contains("non-exhaustive match"));

    let mismatched = r#"
enum Choice {
    One
    Two
}
fn main() -> i64 {
    let choice: Choice = Choice.One()
    let value: i64 = match choice:
        Choice.One(): 1
        Choice.Two(): "two"
    return value
}
"#;
    let error = check_source(mismatched).expect_err("match expression arms must agree");
    assert!(
        error
            .message
            .contains("match expression arm: expected i64, got str")
    );

    let void_arm = r#"
enum Choice {
    One
}
fn main() -> i64 {
    let choice: Choice = Choice.One()
    let value: i64 = match choice:
        Choice.One(): print("one")
    return value
}
"#;
    let error = check_source(void_arm).expect_err("match expression cannot produce void");
    assert!(
        error
            .message
            .contains("match expression arms cannot produce void")
    );
}

#[test]
fn formatter_and_semantic_database_preserve_match_expressions() {
    let source = "enum Choice {\n One(i64)\n None\n}\nfn main()->i64 {\n let choice:Choice=Choice.One(42)\n let value:i64 = match choice:\n  Choice.One(payload): payload\n  Choice.None(): 0\n return value\n}\n";
    let expected = "enum Choice {\n    One(i64)\n    None\n}\nfn main() -> i64 {\n    let choice: Choice = Choice.One(42)\n    let value: i64 = match choice:\n        Choice.One(payload): payload\n        Choice.None(): 0\n    return value\n}\n";
    let formatted =
        fluxc::formatter::format_source(source).expect("match expression should format");
    assert_eq!(formatted, expected);

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(704))
        .expect("match expression should analyze");
    let payload = database
        .symbols_named("payload")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::PatternBinding)
        .expect("match expression binding should be indexed");
    assert_eq!(payload.ty, Some(fluxc::ast::Type::I64));
    assert_eq!(span_text(&formatted, payload.span), "payload");
}

#[test]
fn list_match_statements_and_expressions_are_exhaustive_typed_and_native() {
    let source = r#"
fn main() -> i64 {
    let values: i64[] = [10, 20, 30, 40]
    match values[::-1]:
        []:
            print 0
        [single]:
            print single
        [first, ...middle, last]:
            print first
            print middle.length
            print middle.first
            print middle.last
            print last
    let count: i64 = match values:
        []: 0
        [_]: 1
        [_, ...body, _]: body.length + 2
    print count
    return 0
}
"#;

    check_source(source).expect("list matches should typecheck");
    let generated = compile_to_c(source).expect("list matches should lower natively");
    assert!(generated.contains("flux__list_match_"));
    assert!(generated.contains(".len == 0"));
    assert!(generated.contains(".len == 1"));
    assert!(generated.contains(".len >= 2"));
    assert!(generated.contains(".len - 1, sizeof(int64_t)"));
    assert!(generated.contains("flux_list_stride(flux__list_match_"));

    let formatted = fluxc::formatter::format_source(source).expect("list match should format");
    assert!(formatted.contains("[first, ...middle, last]:"));
    assert!(formatted.contains("[_, ...body, _]: body.length + 2"));
    let formatted_again =
        fluxc::formatter::format_source(&formatted).expect("formatted list match should reparse");
    assert_eq!(formatted_again, formatted);

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(743))
        .expect("list match should analyze semantically");
    let middle = database
        .symbols_named("middle")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::PatternBinding)
        .expect("list rest match binding should be indexed");
    assert_eq!(
        middle.ty,
        Some(fluxc::ast::Type::List(Box::new(fluxc::ast::Type::I64)))
    );
    let first = database
        .symbols_named("first")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::PatternBinding)
        .expect("list element match binding should be indexed");
    assert_eq!(first.ty, Some(fluxc::ast::Type::I64));
}

#[test]
fn list_match_supports_wildcard_fallbacks() {
    let source = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2, 3]
    let result: i64 = match values:
        [only]: only
        _: values.length
    return result
}
"#;

    check_source(source).expect("wildcard list match should be exhaustive");
    let generated = compile_to_c(source).expect("wildcard list match should lower natively");
    assert!(generated.contains(".len == 1"));
    assert!(generated.contains("else {"));
}

#[test]
fn rejects_invalid_or_non_exhaustive_list_matches() {
    let non_list = r#"
fn main() -> i64 {
    match 7:
        []:
            print 0
        _:
            print 1
    return 0
}
"#;
    let error = check_source(non_list).expect_err("list match source must be a list");
    assert!(
        error
            .message
            .contains("list match requires a list value, got i64")
    );

    let missing_length = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2, 3]
    match values:
        []:
            print 0
        [_, ...middle, _]:
            print middle.length
    return 0
}
"#;
    let error = check_source(missing_length).expect_err("list match must cover every length");
    assert!(error.message.contains("non-exhaustive list match"));
    assert!(error.message.contains("missing exact length 1"));

    let unreachable = r#"
fn main() -> i64 {
    let values: i64[] = [1, 2]
    match values:
        [...all]:
            print all.length
        _:
            print 0
    return 0
}
"#;
    let error = check_source(unreachable)
        .expect_err("wildcard after an exhaustive rest arm is unreachable");
    assert!(error.message.contains("unreachable list match arm"));

    let mismatched = r#"
fn main() -> i64 {
    let values: i64[] = [1]
    let result: i64 = match values:
        []: 0
        _: "value"
    return result
}
"#;
    let error = check_source(mismatched).expect_err("list match expression arms must agree");
    assert!(
        error
            .message
            .contains("match expression arm: expected i64, got str")
    );

    let unused_binding = r#"
fn main() -> i64 {
    let values: i64[] = [1]
    let result: i64 = match values:
        [unused]: 1
        _: 0
    return result
}
"#;
    let error = check_source(unused_binding).expect_err("named list match bindings must be used");
    assert!(error.message.contains("unused match binding 'unused'"));
}

#[test]
fn match_guards_use_pattern_bindings_and_preserve_exhaustiveness() {
    let source = r#"
enum Outcome {
    Value(i64)
    Empty
}

fn score(outcome: Outcome) -> i64 {
    return match outcome:
        Outcome.Value(value) if value > 10: 2
        Outcome.Value(_): 1
        Outcome.Empty(): 0
}

fn main() -> i64 {
    let values: i64[] = [12]
    match values:
        [only] if only > 10:
            print only
        [only]:
            print only
        _:
            print 0
    let outcome: Outcome = Outcome.Value(12)
    return score(outcome)
}
"#;

    check_source(source).expect("guarded enum and list matches should typecheck");
    let generated = compile_to_c(source).expect("guarded matches should lower natively");
    assert!(generated.contains("switch (flux__match_"));
    assert!(generated.contains("flux__list_match_done_"));
    assert!(generated.contains("flux__local_value > INT64_C(10)"));
    assert!(generated.contains("flux__local_only > INT64_C(10)"));

    let formatted = fluxc::formatter::format_source(source).expect("match guards should format");
    assert!(formatted.contains("Outcome.Value(value) if value > 10: 2"));
    assert!(formatted.contains("[only] if only > 10:"));
    let formatted_again =
        fluxc::formatter::format_source(&formatted).expect("formatted guards should reparse");
    assert_eq!(formatted_again, formatted);

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(744))
        .expect("guarded matches should analyze semantically");
    assert!(
        database
            .symbols_named("value")
            .any(|symbol| symbol.kind == fluxc::semantic::SymbolKind::PatternBinding)
    );
}

#[test]
fn match_guards_must_be_boolean_and_do_not_make_matches_exhaustive() {
    let non_boolean = r#"
enum Outcome {
    Value(i64)
    Empty
}
fn main() -> i64 {
    let outcome: Outcome = Outcome.Value(1)
    match outcome:
        Outcome.Value(value) if value:
            print value
        Outcome.Value(_):
            print 0
        Outcome.Empty():
            print 0
    return 0
}
"#;
    let error = check_source(non_boolean).expect_err("match guards must be boolean");
    assert!(
        error
            .message
            .contains("match guard: expected bool, got i64")
    );

    let guarded_only = r#"
enum Outcome {
    Value(i64)
    Empty
}
fn main() -> i64 {
    let outcome: Outcome = Outcome.Value(1)
    match outcome:
        Outcome.Value(value) if value > 0:
            print value
        Outcome.Empty():
            print 0
    return 0
}
"#;
    let error = check_source(guarded_only)
        .expect_err("a guarded variant alone cannot make an enum match exhaustive");
    assert!(error.message.contains("non-exhaustive match"));
    assert!(error.message.contains("Value"));
}

#[test]
fn unguarded_match_arms_make_later_same_shape_arms_unreachable() {
    let source = r#"
enum Outcome {
    Value(i64)
}
fn main() -> i64 {
    let outcome: Outcome = Outcome.Value(1)
    match outcome:
        Outcome.Value(value):
            print value
        Outcome.Value(value) if value > 0:
            print value
    return 0
}
"#;
    let error = check_source(source).expect_err("later guarded duplicate must be unreachable");
    assert!(error.message.contains("duplicate match arm"));
    assert!(error.message.contains("earlier unguarded arm"));
}

#[test]
fn match_scrutinee_is_evaluated_exactly_once() {
    let source = r#"
enum Outcome {
    Ok(i64)
    Empty
}

fn make() -> Outcome {
    return Outcome.Ok(42)
}

fn main() -> i64 {
    match make():
        Outcome.Ok(value):
            print(value)
        Outcome.Empty():
            print(0)
    return 0
}
"#;

    let generated = compile_to_c(source).expect("match should compile");
    let assignment = generated
        .lines()
        .find(|line| line.contains("flux__match_") && line.contains("= flux__fn_make();"))
        .expect("match should assign the scrutinee once");
    assert!(assignment.contains("struct flux__type_Outcome"));
    assert_eq!(generated.matches("= flux__fn_make();").count(), 1);
}

#[test]
fn rejects_non_exhaustive_or_invalid_enum_matches() {
    let missing = r#"
enum Outcome {
    Ok(i64)
    Empty
}
fn main() -> i64 {
    let value: Outcome = Outcome.Empty()
    match value:
        Outcome.Ok(payload):
            print(payload)
    return 0
}
"#;
    let error = check_source(missing).expect_err("non-exhaustive match should fail");
    assert!(error.message.contains("non-exhaustive match"));
    assert!(error.message.contains("Empty"));

    let duplicate = r#"
enum Outcome {
    Ok(i64)
    Empty
}
fn main() -> i64 {
    let value: Outcome = Outcome.Empty()
    match value:
        Outcome.Ok(first):
            print(first)
        Outcome.Ok(second):
            print(second)
        Outcome.Empty():
            print(0)
    return 0
}
"#;
    let error = check_source(duplicate).expect_err("duplicate match arm should fail");
    assert!(error.message.contains("duplicate match arm"));

    let arity = r#"
enum Outcome {
    Ok(i64)
}
fn main() -> i64 {
    let value: Outcome = Outcome.Ok(1)
    match value:
        Outcome.Ok():
            print(0)
    return 0
}
"#;
    let error = check_source(arity).expect_err("wrong match payload arity should fail");
    assert!(error.message.contains("expects 1 payload pattern, got 0"));

    let non_enum = r#"
fn main() -> i64 {
    match 42:
        Outcome.Ok(value):
            print(value)
    return 0
}
"#;
    let error = check_source(non_enum).expect_err("matching a non-enum should fail");
    assert!(
        error
            .message
            .contains("match requires an enum value, got i64")
    );
}

#[test]
fn formatter_and_semantic_database_preserve_match_patterns() {
    let source = "enum Outcome {\n Ok(i64)\n Empty\n}\nfn main()->i64 {\n let value:Outcome=Outcome.Ok(42)\n match value:\n  Outcome.Ok(payload):\n   print(payload)\n  Outcome.Empty():\n   print(0)\n return 0\n}\n";
    let expected = "enum Outcome {\n    Ok(i64)\n    Empty\n}\nfn main() -> i64 {\n    let value: Outcome = Outcome.Ok(42)\n    match value:\n        Outcome.Ok(payload):\n            print(payload)\n        Outcome.Empty():\n            print(0)\n    return 0\n}\n";
    let formatted = fluxc::formatter::format_source(source).expect("match source should format");
    assert_eq!(formatted, expected);

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(701))
        .expect("match source should analyze");
    let payload = database
        .symbols_named("payload")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::PatternBinding)
        .expect("match payload binding should be indexed");
    assert_eq!(payload.ty, Some(fluxc::ast::Type::I64));
    assert_eq!(span_text(&formatted, payload.span), "payload");
}

#[test]
fn accepts_closed_enums_with_typed_payloads_and_native_tags() {
    let source = r#"
enum Outcome {
    Ok(i64)
    Error(str, i64)
    Pending
}

fn pass(value: Outcome) -> Outcome {
    return value
}

fn main() -> i64 {
    let first: Outcome = Outcome.Ok(42)
    let _second: Outcome = Outcome.Error("nope", 7)
    let _third: Outcome = Outcome.Pending()
    let _carried: Outcome = pass(first)
    print(1)
    return 0
}
"#;

    check_source(source).expect("enum constructors should typecheck");
    let generated = compile_to_c(source).expect("enum constructors should lower natively");
    assert!(generated.contains("enum flux__tag_Outcome"));
    assert!(generated.contains("struct flux__type_Outcome"));
    assert!(generated.contains("union {"));
    assert!(generated.contains("flux__tag_Outcome_Ok"));
    assert!(generated.contains("flux__variant_Outcome_Ok(INT64_C(42))"));
    assert!(generated.contains("flux__variant_Outcome_Error(\"nope\", INT64_C(7))"));
    assert!(generated.contains("flux__variant_Outcome_Pending()"));
}

#[test]
fn supports_forward_dependencies_between_structs_and_enums() {
    let source = r#"
struct Envelope {
    outcome: Outcome
}

enum Outcome {
    Ok(User)
    Empty
}

struct User {
    name: str
}

fn main() -> i64 {
    let user: User = User { name: "Ada" }
    let outcome: Outcome = Outcome.Ok(user)
    let _envelope: Envelope = Envelope { outcome: outcome }
    print(1)
    return 0
}
"#;

    check_source(source).expect("cross value-type references should typecheck");
    let generated = compile_to_c(source).expect("cross value-type references should lower");
    let user = generated.find("struct flux__type_User {").unwrap();
    let outcome = generated.find("struct flux__type_Outcome {").unwrap();
    let envelope = generated.find("struct flux__type_Envelope {").unwrap();
    assert!(user < outcome && outcome < envelope);
}

#[test]
fn rejects_invalid_enum_construction_and_recursive_value_cycles() {
    let bad_variant = r#"
enum Outcome {
    Ok(i64)
}
fn main() -> i64 {
    let value: Outcome = Outcome.Nope(1)
    return 0
}
"#;
    let error = check_source(bad_variant).expect_err("unknown enum variant should fail");
    assert!(error.message.contains("has no variant 'Nope'"));

    let bad_arity = r#"
enum Outcome {
    Ok(i64)
}
fn main() -> i64 {
    let value: Outcome = Outcome.Ok()
    return 0
}
"#;
    let error = check_source(bad_arity).expect_err("wrong payload arity should fail");
    assert!(error.message.contains("expects 1 payload value, got 0"));

    let bad_type = r#"
enum Outcome {
    Ok(i64)
}
fn main() -> i64 {
    let value: Outcome = Outcome.Ok(false)
    return 0
}
"#;
    let error = check_source(bad_type).expect_err("wrong payload type should fail");
    assert!(error.message.contains("expected i64, got bool"));

    let cycle = r#"
struct Boxed {
    value: Loop
}

enum Loop {
    Again(Boxed)
}

fn main() -> i64 {
    return 0
}
"#;
    let error = compile_to_c(cycle).expect_err("enum/struct recursive value cycle should fail");
    assert_eq!(error.stage, DiagnosticStage::Codegen);
    assert!(error.message.contains("recursive by-value cycle"));
}

#[test]
fn formatter_and_semantic_database_preserve_enums() {
    let source = "enum Outcome {\n Ok(i64)\n Pending\n}\nfn main()->i64 {\n let _value:Outcome=Outcome.Ok(42)\n return 0\n}\n";
    let expected = "enum Outcome {\n    Ok(i64)\n    Pending\n}\nfn main() -> i64 {\n    let _value: Outcome = Outcome.Ok(42)\n    return 0\n}\n";
    let formatted = fluxc::formatter::format_source(source).expect("enum source should format");
    assert_eq!(formatted, expected);

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(601))
        .expect("enum source should analyze");
    let definition = database
        .symbols_named("Outcome")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::Enum)
        .expect("enum should be indexed");
    assert_eq!(span_text(&formatted, definition.span), "Outcome");
    let variant = database
        .symbols_named("Ok")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::EnumVariant)
        .expect("enum variant should be indexed");
    assert_eq!(
        variant.ty,
        Some(fluxc::ast::Type::Named("Outcome".to_string()))
    );
}

#[test]
fn tree_shakes_unreachable_private_functions_but_keeps_public_and_transitive_roots() {
    let source = r#"
interface Operation {
    fn apply(value: i64) -> i64
}

struct Offset {
    amount: i64
}

fn mappedApply(offset: Offset, value: i64) -> i64 {
    return offset.amount + value
}

impl Operation for Offset {
    apply: mappedApply
}

pub fn exported(value: i64) -> i64 {
    return value + 10
}

fn leaf(value: i64) -> i64 {
    return value + 1
}

fn helper(value: i64) -> i64 {
    return leaf(value)
}

fn unreachable(value: i64) -> i64 {
    return value + 1000
}

fn main() -> i64 {
    return helper(41)
}
"#;

    check_source(source).expect("tree-shaking fixture should typecheck");
    let generated = compile_to_c(source).expect("tree-shaking fixture should lower natively");
    assert!(generated.contains("flux__fn_helper"));
    assert!(generated.contains("flux__fn_leaf"));
    assert!(generated.contains("flux__fn_exported"));
    assert!(!generated.contains("flux__fn_mappedApply"));
    assert!(!generated.contains("flux__interface_Operation"));
    assert!(!generated.contains("flux__type_Offset"));
    assert!(!generated.contains("flux__fn_unreachable"));
}

#[test]
fn tree_shakes_unreachable_value_types_helpers_typedefs_and_background_runtime() {
    let source = r#"
type DeadCallback = fn(i64) -> i64

struct DeadRecord {
    value: i64
}

enum DeadChoice {
    Only(DeadRecord)
}

fn unreachable(seed: i64) -> i64 {
    let base: DeadRecord = DeadRecord { value: seed }
    let updated: DeadRecord = DeadRecord { ..base, value: seed + 1 }
    let choice: DeadChoice = DeadChoice.Only(updated)
    match choice:
        DeadChoice.Only(item):
            print item.value &
    return updated.value
}

fn main() -> i64 {
    return 0
}
"#;

    check_source(source).expect("dead tree-shaking fixture should typecheck");
    let generated = compile_to_c(source).expect("dead tree-shaking fixture should lower natively");
    assert!(!generated.contains("flux__fn_unreachable"));
    assert!(!generated.contains("flux__type_DeadRecord"));
    assert!(!generated.contains("flux__type_DeadChoice"));
    assert!(!generated.contains("flux__variant_DeadChoice_Only"));
    assert!(!generated.contains("flux__update_DeadRecord__value"));
    assert!(!generated.contains("typedef "));
    assert!(!generated.contains("#include <unistd.h>"));
    assert!(!generated.contains("flux_print_"));
    assert!(!generated.contains("flux_redirect_"));
    assert!(!generated.contains("flux_error_eq("));
    assert!(!generated.contains("flux_list_at"));
    assert!(!generated.contains("flux_add_i64("));
    assert!(!generated.contains("flux_sub_i64("));
    assert!(!generated.contains("flux_mul_i64("));
    assert!(!generated.contains("flux_neg_i64("));
    assert!(!generated.contains("flux_div_i64("));
}

#[test]
fn tree_shakes_unused_interface_pack_and_dispatch_helpers() {
    let source = r#"
interface Tool {
    fn used(value: i64) -> (i64, error)
    fn unused(value: i64) -> (i64, error)
}

struct Offset {
    amount: i64
}

fn usedImpl(offset: Offset, value: i64) -> (i64, error) {
    return offset.amount + value, nil
}

fn unusedImpl(_offset: Offset, value: i64) -> (i64, error) {
    return value, nil
}

impl Tool for Offset {
    used: usedImpl
    unused: unusedImpl
}

fn run(tool: Tool, value: i64) -> (i64, error) {
    return Tool.used(tool, value)
}

fn main() -> i64 {
    let offset: Offset = Offset { amount: 2 }
    let tool: Tool = Tool(offset)
    let value: i64, err: error = run(tool, 40)
    if err != nil:
        return 1
    return value
}
"#;

    check_source(source).expect("interface helper tree-shaking fixture should typecheck");
    let generated =
        compile_to_c(source).expect("interface helper tree-shaking fixture should lower");
    assert!(generated.contains("flux__iface_pack_Tool_Offset"));
    assert!(generated.contains("flux__iface_call_Tool_used"));
    assert!(generated.contains("struct flux__iface_ret_Tool_used"));
    assert!(!generated.contains("flux__iface_call_Tool_unused"));
    assert!(!generated.contains("struct flux__iface_ret_Tool_unused"));
    assert!(!generated.contains("flux__fn_unusedImpl"));
}

#[test]
fn tree_shakes_interface_value_layout_targets_that_cannot_be_packed() {
    let source = r#"
interface Tool {
    fn apply(value: i64) -> i64
}

struct Used {
    amount: i64
}

struct Dead {
    amount: i64
}

fn usedApply(receiver: Used, value: i64) -> i64 {
    return receiver.amount + value
}

fn deadApply(receiver: Dead, value: i64) -> i64 {
    return receiver.amount + value
}

impl Tool for Used {
    apply: usedApply
}

impl Tool for Dead {
    apply: deadApply
}

fn main() -> i64 {
    let receiver: Used = Used { amount: 2 }
    let _tool: Tool = Tool(receiver)
    return 0
}
"#;

    check_source(source).expect("interface layout tree-shaking fixture should typecheck");
    let generated = compile_to_c(source).expect("interface layout fixture should lower");
    assert!(generated.contains("flux__type_Used"));
    assert!(generated.contains("flux__iface_tag_Tool_Used"));
    assert!(generated.contains("flux__iface_pack_Tool_Used"));
    assert!(!generated.contains("flux__type_Dead"));
    assert!(!generated.contains("flux__iface_tag_Tool_Dead"));
    assert!(!generated.contains("flux__iface_pack_Tool_Dead"));
    assert!(!generated.contains("flux__fn_deadApply"));
}

#[test]
fn tree_shakes_other_interface_implementations_for_static_dispatch() {
    let source = r#"
interface Tool {
    fn apply(value: i64) -> i64
}

struct Used {
    amount: i64
}

struct Dead {
    amount: i64
}

fn usedApply(receiver: Used, value: i64) -> i64 {
    return receiver.amount + value
}

fn deadApply(receiver: Dead, value: i64) -> i64 {
    return receiver.amount + value
}

impl Tool for Used {
    apply: usedApply
}

impl Tool for Dead {
    apply: deadApply
}

fn main() -> i64 {
    let receiver: Used = Used { amount: 2 }
    return Tool.apply(receiver, 40)
}
"#;

    check_source(source).expect("static interface dispatch fixture should typecheck");
    let generated = compile_to_c(source).expect("static interface dispatch fixture should lower");
    assert!(generated.contains("flux__fn_usedApply"));
    assert!(generated.contains("flux__type_Used"));
    assert!(!generated.contains("flux__interface_Tool"));
    assert!(!generated.contains("flux__iface_call_Tool_apply"));
    assert!(!generated.contains("flux__fn_deadApply"));
    assert!(!generated.contains("flux__type_Dead"));
    assert!(!generated.contains("flux__iface_tag_Tool_Dead"));
}

#[test]
fn tree_shakes_metadata_from_views_the_native_backend_does_not_emit() {
    let source = r#"
fn unusedViewAction() -> void {
    print("dead")
}

view Unused {
    grid columns: 1fr
    grid rows: auto
    Button action at 1,1
        text: "Dead"
        onPress: unusedViewAction
}

fn main() -> i64 {
    return 0
}
"#;

    check_source(source).expect("unused view tree-shaking fixture should typecheck");
    let generated = compile_to_c(source).expect("unused view fixture should lower");
    assert!(!generated.contains("flux__fn_unusedViewAction"));
    assert!(!generated.contains("Flux runtime error"));
    assert!(!generated.contains("flux_print_str"));
}

#[test]
fn tree_shakes_unused_enum_constructor_helpers_per_variant() {
    let source = r#"
enum Choice {
    Used(i64)
    Unused(i64)
}

fn main() -> i64 {
    let value: Choice = Choice.Used(7)
    match value:
        Choice.Used(item):
            return item
        Choice.Unused(item):
            return item
}
"#;

    check_source(source).expect("enum helper tree-shaking fixture should typecheck");
    let generated = compile_to_c(source).expect("enum helper tree-shaking fixture should lower");
    assert!(generated.contains("flux__tag_Choice_Used"));
    assert!(generated.contains("flux__tag_Choice_Unused"));
    assert!(generated.contains("flux__variant_Choice_Used"));
    assert!(!generated.contains("flux__variant_Choice_Unused"));
}

#[test]
fn removes_proven_list_bounds_checks_but_keeps_unproven_checks() {
    let proven = r#"
fn main() -> i64 {
    let first: i64 = [10, 20, 30][0]
    let last: i64 = [10, 20, 30][-1]
    let firstProperty: i64 = [4, 5].first
    let lastProperty: i64 = [4, 5].last
    let single: i64 = [7].single
    return first + last + firstProperty + lastProperty + single
}
"#;
    check_source(proven).expect("proven list indexing should typecheck");
    let generated = compile_to_c(proven).expect("proven list indexing should lower");
    assert!(generated.contains("flux_list_at_unchecked"));
    assert!(!generated.contains("flux_list_index("));
    assert!(!generated.contains("static inline void *flux_list_at("));

    let unproven = r#"
fn main() -> i64 {
    return [10, 20, 30][3]
}
"#;
    check_source(unproven).expect("runtime-checked list indexing should typecheck");
    let generated = compile_to_c(unproven).expect("runtime-checked list indexing should lower");
    assert!(generated.contains("flux_list_index("));
    assert!(generated.contains("static inline void *flux_list_at("));
}

#[test]
fn folds_compile_time_constants_and_inlines_them_natively() {
    let source = r#"
type Count = i64
const ANSWER: Count = BASE + 2
const BASE: Count = 40
const ENABLED: bool = ANSWER == 42 && true
const LABEL: str = "Flux"
const SHORT_CIRCUIT: bool = false && (1 / 0 == 0)

fn main() -> i64 {
    print(LABEL)
    if ENABLED:
        print(ANSWER)
    print(SHORT_CIRCUIT)
    return 0
}
"#;

    check_source(source).expect("constant expressions should typecheck and fold");
    let generated = compile_to_c(source).expect("constants should compile to inline values");
    assert!(generated.contains("flux_print_str(\"Flux\")"));
    assert!(generated.contains("flux_print_i64(INT64_C(42))"));
    assert!(generated.contains("flux_print_bool(false)"));
    assert!(!generated.contains("static const"));
}

#[test]
fn folds_pure_primitive_expressions_before_native_codegen() {
    let source = r#"
fn main() -> i64 {
    let answer: i64 = 2 * 20 + 2
    let enabled: bool = 10 > 3 && !false
    let same: bool = "Flux" == "Flux"
    let shortCircuit: bool = false && (1 / 0 == 0)
    let input: i64 = 5
    let dynamic: i64 = input + 2
    print(answer)
    print(enabled)
    print(same)
    print(shortCircuit)
    print(dynamic)
    return 0
}
"#;

    check_source(source).expect("pure primitive expressions should typecheck");
    let generated = compile_to_c(source).expect("pure primitive expressions should fold");
    assert!(generated.contains("flux__local_answer = INT64_C(42);"));
    assert!(generated.contains("flux__local_enabled = true;"));
    assert!(generated.contains("flux__local_same = true;"));
    assert!(generated.contains("flux__local_shortCircuit = false;"));
    assert!(
        generated.contains("flux__local_dynamic = flux_add_i64(flux__local_input, INT64_C(2));")
    );
    assert!(!generated.contains("flux_mul_i64(INT64_C(2), INT64_C(20))"));

    let static_failure = r#"
fn main() -> i64 {
    let bad: i64 = 1 / 0
    print(bad)
    return 0
}
"#;
    check_source(static_failure).expect("static division remains type-correct");
    let error = compile_to_c(static_failure)
        .expect_err("static division by zero should fail before native execution");
    assert!(error.message.contains("constant integer division by zero"));
}

#[test]
fn eliminates_cfg_unreachable_statements_before_native_codegen() {
    let source = r#"
fn main() -> i64 {
    for i in 0..2:
        if i == 0:
            continue
            print("dead-after-continue")
        break
        print("dead-after-break")
    return 0
    print("dead-after-return")
}
"#;

    check_source(source).expect("structurally unreachable statements should remain type-correct");
    let generated = compile_to_c(source).expect("CFG-unreachable statements should be eliminated");
    assert!(!generated.contains("dead-after-continue"));
    assert!(!generated.contains("dead-after-break"));
    assert!(!generated.contains("dead-after-return"));
}

#[test]
fn eliminates_compile_time_dead_control_flow_before_native_codegen() {
    let source = r#"
const ENABLED: bool = 2 + 2 == 4

fn main() -> i64 {
    if ENABLED:
        print("live-if")
    else:
        print("dead-else")
    if false:
        print("dead-if")
    else:
        print("live-else")
    while false:
        print("dead-while")
    for i in 3..3:
        print(i)
    for j in 5..=4:
        print(j)
    return 0
}
"#;

    check_source(source).expect("constant control flow should typecheck");
    let generated = compile_to_c(source).expect("constant control flow should optimize");
    assert!(generated.contains("flux_print_str(\"live-if\")"));
    assert!(generated.contains("flux_print_str(\"live-else\")"));
    assert!(!generated.contains("dead-else"));
    assert!(!generated.contains("dead-if"));
    assert!(!generated.contains("dead-while"));
    assert!(!generated.contains("flux__local_i"));
    assert!(!generated.contains("flux__local_j"));
}

#[test]
fn eliminates_dead_local_assignments_without_dropping_rhs_effects() {
    let source = r#"
fn observe(value: i64) -> i64 {
    print(value)
    return value
}

fn main() -> i64 {
    var value: i64 = 1
    value = 10 + 20
    value = 2
    value = observe(4)
    value = 3
    return value
}
"#;

    check_source(source).expect("dead-store example should typecheck");
    let generated = compile_to_c(source).expect("dead stores should optimize safely");
    assert!(generated.contains("int64_t flux__local_value;"));
    assert!(!generated.contains("int64_t flux__local_value = INT64_C(1);"));
    assert!(!generated.contains("flux__local_value = INT64_C(30);"));
    assert!(!generated.contains("flux__local_value = INT64_C(2);"));
    assert!(generated.contains("flux__local_value = flux__fn_observe(INT64_C(4));"));
    assert!(generated.contains("flux__local_value = INT64_C(3);"));
}

#[test]
fn rejects_invalid_compile_time_constants() {
    let cycle = r#"
const A: i64 = B
const B: i64 = A
fn main() -> i64 {
    return 0
}
"#;
    let diagnostics = check_source_all(cycle).expect_err("constant cycle should fail");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("constant 'A' is recursive") })
    );

    let division = r#"
const BAD: i64 = 1 / 0
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(division).expect_err("constant division by zero should fail");
    assert!(error.message.contains("division by zero"));

    let wrong_type = r#"
const BAD: bool = 42
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(wrong_type).expect_err("constant type mismatch should fail");
    assert!(
        error
            .message
            .contains("constant 'BAD': expected bool, got i64")
    );

    let unsupported = r#"
fn value() -> i64 {
    return 1
}
const BAD: i64 = value()
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(unsupported).expect_err("constant function calls should fail");
    assert!(
        error
            .message
            .contains("constant expressions currently support")
    );
}

#[test]
fn rejects_compile_time_integer_overflow() {
    let addition = r#"
const BAD: i64 = 9223372036854775807 + 1
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(addition).expect_err("constant addition overflow should fail");
    assert!(
        error
            .message
            .contains("constant integer addition overflows i64")
    );

    let subtraction = r#"
const BAD: i64 = -9223372036854775807 - 2
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(subtraction).expect_err("constant subtraction overflow should fail");
    assert!(
        error
            .message
            .contains("constant integer subtraction overflows i64")
    );

    let multiplication = r#"
const BAD: i64 = 9223372036854775807 * 2
fn main() -> i64 {
    return 0
}
"#;
    let error =
        check_source(multiplication).expect_err("constant multiplication overflow should fail");
    assert!(
        error
            .message
            .contains("constant integer multiplication overflows i64")
    );

    let negation = r#"
const MIN: i64 = -9223372036854775807 - 1
const BAD: i64 = -MIN
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(negation).expect_err("constant negation overflow should fail");
    assert!(
        error
            .message
            .contains("constant integer negation overflows i64")
    );
}

#[test]
fn integer_arithmetic_lowers_through_checked_helpers() {
    let source = r#"
fn calculate(left: i64, right: i64) -> i64 {
    let sum: i64 = left + right
    let difference: i64 = sum - right
    let product: i64 = difference * right
    return -product
}

fn main() -> i64 {
    print(calculate(4, 3))
    return 0
}
"#;

    check_source(source).expect("checked integer arithmetic should typecheck");
    let generated = compile_to_c(source).expect("checked integer arithmetic should lower");
    assert!(generated.contains("flux_add_i64(flux__local_left, flux__local_right)"));
    assert!(generated.contains("flux_sub_i64(flux__local_sum, flux__local_right)"));
    assert!(generated.contains("flux_mul_i64(flux__local_difference, flux__local_right)"));
    assert!(generated.contains("return flux_neg_i64(flux__local_product);"));
    assert!(generated.contains("__builtin_add_overflow"));
    assert!(generated.contains("__builtin_sub_overflow"));
    assert!(generated.contains("__builtin_mul_overflow"));
}

#[test]
fn formatter_and_semantic_database_preserve_constants() {
    let source = "const ANSWER:i64=40+2\nfn main()->i64 {\n return ANSWER\n}\n";
    let expected = "const ANSWER: i64 = 40 + 2\nfn main() -> i64 {\n    return ANSWER\n}\n";
    let formatted = fluxc::formatter::format_source(source).expect("constant source should format");
    assert_eq!(formatted, expected);

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(501))
        .expect("constant source should analyze");
    let constant = database
        .symbols_named("ANSWER")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::Constant)
        .expect("constant should be indexed");
    assert_eq!(constant.ty, Some(fluxc::ast::Type::I64));
    assert_eq!(span_text(&formatted, constant.span), "ANSWER");
    assert_eq!(
        database
            .signatures()
            .constant("ANSWER")
            .expect("constant value should be available")
            .value,
        fluxc::typecheck::ConstantValue::I64(42)
    );
}

#[test]
fn accepts_zero_cost_type_aliases_for_primitives_and_structs() {
    let source = r#"
type UserId = i64
type Person = User
type Account = Person

struct User {
    id: UserId
    name: str
}

fn user_id(user: Account) -> UserId {
    return user.id
}

fn main() -> i64 {
    let user: Person = User { id: 7, name: "Ada" }
    print(user_id(user))
    return 0
}
"#;

    check_source(source).expect("aliases should typecheck transparently");
    let generated = compile_to_c(source).expect("aliases should lower to concrete native types");
    assert!(generated.contains("int64_t flux__field_id;"));
    assert!(
        generated.contains("int64_t flux__fn_user_id(struct flux__type_User flux__local_user)")
    );
    assert!(!generated.contains("flux__type_UserId"));
    assert!(!generated.contains("flux__type_Person"));
    assert!(!generated.contains("flux__type_Account"));
}

#[test]
fn rejects_invalid_type_aliases() {
    let cycle = r#"
type A = B
type B = A
fn main() -> i64 {
    return 0
}
"#;
    let diagnostics = check_source_all(cycle).expect_err("alias cycle should fail");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("type alias 'A' is recursive"))
    );

    let unknown = r#"
type UserId = Missing
fn main() -> i64 {
    return 0
}
"#;
    let error = check_source(unknown).expect_err("unknown alias target should fail");
    assert!(error.message.contains("unknown type 'Missing'"));

    let conflict = r#"
type User = i64
struct User {
    value: i64
}
fn main() -> i64 {
    return 0
}
"#;
    let diagnostics = check_source_all(conflict).expect_err("alias/struct conflict should fail");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("struct 'User' conflicts with a type alias")
    }));
}

#[test]
fn formatter_and_semantic_database_preserve_type_aliases() {
    let source = "type UserId=i64\nfn identity(value:UserId)->UserId {\n return value\n}\nfn main()->i64 {\n return identity(7)\n}\n";
    let expected = "type UserId = i64\nfn identity(value: UserId) -> UserId {\n    return value\n}\nfn main() -> i64 {\n    return identity(7)\n}\n";
    let formatted = fluxc::formatter::format_source(source).expect("alias source should format");
    assert_eq!(formatted, expected);

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(401))
        .expect("alias source should analyze");
    let alias = database
        .symbols_named("UserId")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::TypeAlias)
        .expect("alias should be indexed");
    assert_eq!(alias.ty, Some(fluxc::ast::Type::I64));
    assert_eq!(span_text(&formatted, alias.span), "UserId");
    assert_eq!(
        database
            .signature("identity")
            .expect("function signature should exist")
            .params,
        vec![fluxc::ast::Type::I64]
    );
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
fn accepts_functional_struct_updates_with_single_base_evaluation() {
    let source = r#"
struct User {
    name: str
    age: i64
}

fn make_user(age: i64) -> User {
    return User { name: "Ada", age: age }
}

fn main() -> i64 {
    let older: User = User { ..make_user(41), age: 42 }
    print(older.name)
    print(older.age)
    return 0
}
"#;

    check_source(source).expect("struct update should typecheck");
    let generated = compile_to_c(source).expect("struct update should compile");
    assert!(generated.contains(
        "static inline struct flux__type_User flux__update_User__age(struct flux__type_User base, int64_t value_age)"
    ));
    assert!(
        generated.contains("flux__update_User__age(flux__fn_make_user(INT64_C(41)), INT64_C(42))")
    );
}

#[test]
fn rejects_struct_updates_from_the_wrong_base_type() {
    let source = r#"
struct User {
    age: i64
}

struct Profile {
    active: bool
}

fn main() -> i64 {
    let profile: Profile = Profile { active: true }
    let user: User = User { ..profile, age: 42 }
    return 0
}
"#;

    let error = check_source(source).expect_err("wrong update base should fail");
    assert!(
        error
            .message
            .contains("User update base: expected User, got Profile")
    );
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
    let source = "struct User {\n name:str\n age:i64\n}\nfn main()->i64 {\n let user:User=User{name:\"Ada\",age:41}\n let older:User=User{..user,age:42}\n print(older.name)\n return 0\n}\n";
    let expected = "struct User {\n    name: str\n    age: i64\n}\nfn main() -> i64 {\n    let user: User = User { name: \"Ada\", age: 41 }\n    let older: User = User { ..user, age: 42 }\n    print(older.name)\n    return 0\n}\n";
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
fn formatter_is_deterministic_without_comments() {
    let source = "fn add(a:i64,b: i64)->i64 {\n  let value:i64=a+b\n  if value>0:\n      return value\n  else:\n      return 0\n}\n\n\nfn main()->i64 {\n    return add(1,2)\n}\n";
    let expected = "fn add(a: i64, b: i64) -> i64 {\n    let value: i64 = a + b\n    if value > 0:\n        return value\n    else:\n        return 0\n}\n\nfn main() -> i64 {\n    return add(1, 2)\n}\n";

    let formatted = fluxc::formatter::format_source(source).expect("source should format");
    assert_eq!(formatted, expected);
    let second =
        fluxc::formatter::format_source(&formatted).expect("formatted source should parse");
    assert_eq!(second, formatted, "formatting should be idempotent");
}

#[test]
fn semantic_database_exposes_structural_control_flow_graphs_with_source_spans() {
    let source_id = SourceId::new(1305);
    let source = r#"
fn route(flag: bool) -> i64 {
    let value: i64 = 1
    while flag:
        if value == 1:
            break
        continue
    return value
}

fn main() -> i64 {
    return route(false)
}
"#;
    let database = fluxc::semantic::SemanticDatabase::analyze(source, source_id)
        .expect("valid control flow should analyze");
    let graph = database
        .control_flow_graph("route")
        .expect("semantic database should expose a graph for every function");

    assert_eq!(graph.function(), "route");
    assert_eq!(graph.parameters().len(), 1);
    assert_eq!(graph.parameters()[0].name, "flag");
    assert_eq!(graph.parameters()[0].ty, fluxc::ast::Type::Bool);
    assert_eq!(graph.returns(), &[fluxc::ast::Type::I64]);
    assert_eq!(database.control_flow_graphs().len(), 2);
    assert!(
        graph
            .nodes()
            .iter()
            .all(|node| node.span.source_id == source_id)
    );
    assert!(graph.nodes().iter().any(|node| matches!(
        &node.kind,
        ControlFlowNodeKind::Binding { name, ty, mutable: false }
            if name == "value" && *ty == fluxc::ast::Type::I64
    )));
    for expected in [
        ControlFlowNodeKind::Loop,
        ControlFlowNodeKind::Conditional,
        ControlFlowNodeKind::Break,
        ControlFlowNodeKind::Continue,
        ControlFlowNodeKind::Return,
    ] {
        assert!(
            graph
                .nodes()
                .iter()
                .any(|node| std::mem::discriminant(&node.kind) == std::mem::discriminant(&expected)),
            "missing control-flow node {expected:?}"
        );
    }
    for expected in [
        ControlFlowEdgeKind::True,
        ControlFlowEdgeKind::False,
        ControlFlowEdgeKind::Break,
        ControlFlowEdgeKind::Continue,
        ControlFlowEdgeKind::Return,
    ] {
        assert!(
            graph.edges().iter().any(|edge| edge.kind == expected),
            "missing control-flow edge {expected:?}"
        );
    }
    let loop_node = graph
        .nodes()
        .iter()
        .find(|node| matches!(node.kind, ControlFlowNodeKind::Loop))
        .expect("loop node should exist");
    assert!(
        graph
            .outgoing(loop_node.id)
            .any(|edge| edge.kind == ControlFlowEdgeKind::True)
    );
    assert!(
        graph
            .outgoing(loop_node.id)
            .any(|edge| edge.kind == ControlFlowEdgeKind::False)
    );
    let continue_edge = graph
        .edges()
        .iter()
        .find(|edge| edge.kind == ControlFlowEdgeKind::Continue)
        .expect("continue edge should exist");
    let condition = graph
        .node(continue_edge.to)
        .expect("continue should target condition re-evaluation");
    assert!(matches!(
        condition.kind,
        ControlFlowNodeKind::Evaluation(ControlFlowEvaluationKind::Condition)
    ));
    assert!(
        graph
            .outgoing(condition.id)
            .any(|edge| edge.kind == ControlFlowEdgeKind::Next && edge.to == loop_node.id)
    );

    let propagation = r#"
fn load(path: str) -> (str, error) {
    if path == "":
        return "", error("missing")
    return path, nil
}

fn loadConfig(path: str) -> (str, error) {
    let data: str, err: error = load(path) else return
    print(data)
    return data, nil
}

fn main() -> i64 {
    return 0
}
"#;
    let propagation_database =
        fluxc::semantic::SemanticDatabase::analyze(propagation, SourceId::new(1306))
            .expect("explicit error propagation should analyze");
    let propagation_graph = propagation_database
        .control_flow_graph("loadConfig")
        .expect("error-propagating function should have a graph");
    let destructure = propagation_graph
        .nodes()
        .iter()
        .find(|node| {
            matches!(
                node.kind,
                ControlFlowNodeKind::Destructure {
                    propagates_error: true
                }
            )
        })
        .expect("else-return destructuring should be explicit in the graph");
    let destructure_evaluation = propagation_graph
        .incoming(destructure.id)
        .find_map(|edge| propagation_graph.node(edge.from))
        .expect("destructure source evaluation should precede propagation");
    assert_eq!(
        destructure_evaluation.value_types,
        vec![fluxc::ast::Type::Str, fluxc::ast::Type::Error]
    );
    assert_eq!(destructure_evaluation.values.len(), 2);
    for (index, value_id) in destructure_evaluation.values.iter().enumerate() {
        let value = propagation_graph
            .value(*value_id)
            .expect("typed CFG value should be indexed by its deterministic id");
        assert_eq!(value.producer, destructure_evaluation.id);
        assert_eq!(value.result_index, Some(index));
        assert_eq!(value.ty, destructure_evaluation.value_types[index]);
        assert_eq!(value.span, destructure_evaluation.span);
    }
    let outgoing = propagation_graph
        .outgoing(destructure.id)
        .map(|edge| edge.kind)
        .collect::<Vec<_>>();
    assert!(outgoing.contains(&ControlFlowEdgeKind::Success));
    assert!(outgoing.contains(&ControlFlowEdgeKind::Error));
    assert!(propagation_graph.outgoing(destructure.id).any(|edge| {
        edge.kind == ControlFlowEdgeKind::Error && edge.to == propagation_graph.exit()
    }));

    let ownership_source = r#"
fn consume() -> i64 {
    let source: i64[] = [1, 2]
    let destination: i64[] = source
    print(destination.first)
    return 0
}

fn main() -> i64 {
    return consume()
}
"#;
    let ownership_database =
        fluxc::semantic::SemanticDatabase::analyze(ownership_source, SourceId::new(1307))
            .expect("direct ownership transfer should analyze");
    let ownership_graph = ownership_database
        .control_flow_graph("consume")
        .expect("ownership function should have a graph");
    let destination = ownership_graph
        .nodes()
        .iter()
        .find(|node| {
            matches!(
                &node.kind,
                ControlFlowNodeKind::Binding { name, .. } if name == "destination"
            )
        })
        .expect("destination binding should be explicit in the graph");
    assert!(destination.ownership.reads.is_empty());
    assert_eq!(destination.ownership.moves.len(), 1);
    let ownership_move = &destination.ownership.moves[0];
    assert_eq!(ownership_move.source, "source");
    assert_eq!(ownership_move.destination, "destination");
    assert_eq!(ownership_move.span.source_id, SourceId::new(1307));

    let source_evaluation = ownership_graph
        .incoming(destination.id)
        .find_map(|edge| ownership_graph.node(edge.from))
        .expect("binding initializer evaluation should precede the move");
    assert!(matches!(
        source_evaluation.kind,
        ControlFlowNodeKind::Evaluation(ControlFlowEvaluationKind::BindingInitializer)
    ));
    assert_eq!(
        source_evaluation.value_types,
        vec![fluxc::ast::Type::List(Box::new(fluxc::ast::Type::I64))]
    );
    assert_eq!(source_evaluation.values.len(), 1);
    let source_value = ownership_graph
        .value(source_evaluation.values[0])
        .expect("initializer should produce one typed CFG value");
    assert_eq!(source_value.producer, source_evaluation.id);
    assert_eq!(source_value.result_index, Some(0));
    assert_eq!(source_value.ty, source_evaluation.value_types[0]);
    assert_eq!(source_evaluation.ownership.reads, vec!["source"]);
    assert!(source_evaluation.ownership.moves.is_empty());

    let destination_read = ownership_graph
        .nodes()
        .iter()
        .find(|node| {
            node.ownership
                .reads
                .iter()
                .any(|read| read == "destination")
        })
        .expect("later expression evaluation should retain its binding reads");
    let moved_before_read = ownership_graph
        .move_state_before(destination_read.id)
        .expect("every graph node should have a move-state slot");
    assert!(moved_before_read.reachable());
    assert!(moved_before_read.is_moved("source"));
    assert_eq!(
        moved_before_read.origin("source"),
        Some(ownership_move.span)
    );
}

#[test]
fn semantic_cfg_normalizes_nested_expression_values() {
    let source = r#"
fn scale(input: i64) -> i64 {
    let result: i64 = (input + 2) * 3
    return result
}

fn main() -> i64 {
    return scale(4)
}
"#;
    let database = fluxc::semantic::SemanticDatabase::analyze(source, SourceId::new(1310))
        .expect("nested primitive expression should analyze");
    let graph = database
        .control_flow_graph("scale")
        .expect("scale should have a control-flow graph");
    let evaluation = graph
        .nodes()
        .iter()
        .find(|node| {
            matches!(
                node.kind,
                ControlFlowNodeKind::Evaluation(ControlFlowEvaluationKind::BindingInitializer)
            )
        })
        .expect("binding initializer should be explicit");
    assert_eq!(evaluation.values.len(), 1);
    let root = graph
        .value(evaluation.values[0])
        .expect("initializer should have a typed result value");
    assert_eq!(root.result_index, Some(0));
    let ControlFlowValueKind::Binary {
        op: fluxc::ast::BinOp::Mul,
        left,
        right,
    } = root.kind
    else {
        panic!("initializer root should be the multiplication value");
    };
    let multiply_right = graph.value(right).expect("multiply RHS should be typed");
    assert_eq!(multiply_right.result_index, None);
    assert!(matches!(multiply_right.kind, ControlFlowValueKind::Literal));
    assert!(matches!(
        multiply_right.constant,
        Some(fluxc::typecheck::ConstantValue::I64(3))
    ));

    let addition = graph.value(left).expect("multiply LHS should be typed");
    let ControlFlowValueKind::Binary {
        op: fluxc::ast::BinOp::Add,
        left: add_left,
        right: add_right,
    } = addition.kind
    else {
        panic!("multiply LHS should retain the nested addition");
    };
    assert_eq!(addition.result_index, None);
    let input = graph
        .value(add_left)
        .expect("addition input should be typed");
    assert!(matches!(
        &input.kind,
        ControlFlowValueKind::NameRead(name) if name == "input"
    ));
    assert_eq!(input.constant, None);
    assert_eq!(addition.constant, None);
    assert_eq!(root.constant, None);
    assert!(matches!(
        graph.value(add_right).map(|value| &value.kind),
        Some(ControlFlowValueKind::Literal)
    ));
    assert!(
        [root.id, left, right, add_left, add_right]
            .into_iter()
            .all(|id| graph
                .value(id)
                .is_some_and(|value| value.producer == evaluation.id))
    );
}

#[test]
fn semantic_cfg_computes_local_liveness_for_dead_store_analysis() {
    let source = r#"
fn main() -> i64 {
    var value: i64 = 1
    value = 2
    value = 3
    return value
}
"#;
    let database = fluxc::semantic::SemanticDatabase::analyze(source, SourceId::new(1309))
        .expect("mutable local flow should analyze");
    let graph = database
        .control_flow_graph("main")
        .expect("main should have a control-flow graph");
    let mut assignments = graph
        .nodes()
        .iter()
        .filter(|node| {
            matches!(
                &node.kind,
                ControlFlowNodeKind::Assignment { name } if name == "value"
            )
        })
        .collect::<Vec<_>>();
    assignments.sort_by_key(|node| node.span.line);
    assert_eq!(assignments.len(), 2);
    assert!(
        !graph
            .live_after(assignments[0].id)
            .expect("assignment should have liveness facts")
            .contains("value"),
        "the first overwritten store should be dead"
    );
    assert!(
        graph
            .live_after(assignments[1].id)
            .expect("assignment should have liveness facts")
            .contains("value"),
        "the final store should stay live because return reads it"
    );
    assert_eq!(
        graph
            .live_after(assignments[0].id)
            .expect("liveness should be deterministic")
            .live(),
        &[] as &[String]
    );
}

#[test]
fn semantic_cfg_tracks_destructure_loop_and_match_definitions() {
    let source = r#"
struct Point {
    x: i64
    y: i64
}

enum Choice {
    Value(i64)
    Empty
}

fn pair() -> (i64, bool) {
    return 41, true
}

fn main() -> i64 {
    let (multiValue, multiOk) = pair()
    let [head, ...middle, tail] = [1, 2, 3]
    let Point { x, y } = Point { x: 4, y: 5 }
    if false:
        print(multiValue)
        print(multiOk)
        print(head)
        print(middle.length)
        print(tail)
        print(x)
        print(y)
    for index, item in [1, 2]:
        if false:
            print(index)
            print(item)
    match Choice.Value(9):
        Choice.Value(payload):
            if false:
                print(payload)
        Choice.Empty():
            print(0)
    return 0
}
"#;

    let database = fluxc::semantic::SemanticDatabase::analyze(source, SourceId::new(1311))
        .expect("definition-rich control flow should analyze");
    let graph = database
        .control_flow_graph("main")
        .expect("main should have a control-flow graph");

    let definitions = graph
        .nodes()
        .iter()
        .flat_map(|node| {
            node.definitions
                .iter()
                .map(move |definition| (node, definition))
        })
        .collect::<Vec<_>>();
    for (name, expected_type) in [
        ("multiValue", fluxc::ast::Type::I64),
        ("multiOk", fluxc::ast::Type::Bool),
        ("head", fluxc::ast::Type::I64),
        (
            "middle",
            fluxc::ast::Type::List(Box::new(fluxc::ast::Type::I64)),
        ),
        ("tail", fluxc::ast::Type::I64),
        ("x", fluxc::ast::Type::I64),
        ("y", fluxc::ast::Type::I64),
        ("index", fluxc::ast::Type::I64),
        ("item", fluxc::ast::Type::I64),
        ("payload", fluxc::ast::Type::I64),
    ] {
        let (node, definition) = definitions
            .iter()
            .find(|(_, definition)| definition.name == name)
            .copied()
            .unwrap_or_else(|| panic!("missing CFG definition for {name}"));
        assert_eq!(definition.ty, expected_type, "wrong type for {name}");
        assert_eq!(definition.span.source_id, SourceId::new(1311));
        assert!(
            !graph
                .live_after(node.id)
                .expect("definition node should have liveness")
                .contains(name),
            "compile-time-dead uses should leave {name} dead after its definition"
        );
    }
    assert!(graph.nodes().iter().any(|node| {
        matches!(node.kind, ControlFlowNodeKind::PatternBindings)
            && node
                .definitions
                .iter()
                .any(|definition| definition.name == "payload")
    }));
}

#[test]
fn eliminates_dead_destructure_loop_and_match_extractions() {
    let source = r#"
struct Point {
    x: i64
    y: i64
}

enum Choice {
    Value(i64)
    Empty
}

fn pair() -> (i64, bool) {
    return 41, true
}

fn main() -> i64 {
    let (multiValue, multiOk) = pair()
    let [head, ...middle, tail] = [1, 2, 3]
    let Point { x, y } = Point { x: 4, y: 5 }
    if false:
        print(multiValue)
        print(multiOk)
        print(head)
        print(middle.length)
        print(tail)
        print(x)
        print(y)
    for index, item in [1, 2]:
        if false:
            print(index)
            print(item)
    match Choice.Value(9):
        Choice.Value(payload):
            if false:
                print(payload)
        Choice.Empty():
            print(0)
    return 0
}
"#;

    check_source(source).expect("dead-definition fixture should typecheck");
    let generated = compile_to_c(source).expect("dead definitions should lower natively");
    assert!(generated.contains("flux__fn_pair()"));
    assert!(generated.contains("Flux runtime error: list pattern requires at least 2 elements"));
    assert!(generated.contains("flux__multi_pattern_"));
    assert!(generated.contains("flux__list_pattern_"));
    assert!(generated.contains("flux__destructure_"));
    assert!(generated.contains("flux__range_index_") || generated.contains("flux__iter_index_"));
    for dead_name in [
        "multiValue",
        "multiOk",
        "head",
        "middle",
        "tail",
        "x",
        "y",
        "index",
        "item",
        "payload",
    ] {
        assert!(
            !generated.contains(&format!("flux__local_{dead_name}")),
            "dead definition {dead_name} should not materialize in native code"
        );
    }
}

#[test]
fn semantic_cfg_normalizes_match_guard_evaluation() {
    let source = r#"
enum Choice {
    Value(i64)
    Missing
}

fn classify(choice: Choice, allow: bool) -> i64 {
    match choice:
        Choice.Value(value) if allow:
            return value
        Choice.Value(value):
            return value + 1
        Choice.Missing():
            return 0
}

fn main() -> i64 {
    return classify(Choice.Value(4), true)
}
"#;
    let database = fluxc::semantic::SemanticDatabase::analyze(source, SourceId::new(1308))
        .expect("guarded match should analyze");
    let graph = database
        .control_flow_graph("classify")
        .expect("guarded match should have a graph");
    let match_node = graph
        .nodes()
        .iter()
        .find(|node| matches!(node.kind, ControlFlowNodeKind::Match))
        .expect("match dispatch should be explicit");
    let value_evaluation = graph
        .incoming(match_node.id)
        .find_map(|edge| graph.node(edge.from))
        .expect("match scrutinee evaluation should precede dispatch");
    assert!(matches!(
        value_evaluation.kind,
        ControlFlowNodeKind::Evaluation(ControlFlowEvaluationKind::MatchValue)
    ));
    assert_eq!(value_evaluation.ownership.reads, vec!["choice"]);

    let guard = graph
        .nodes()
        .iter()
        .find(|node| {
            matches!(
                node.kind,
                ControlFlowNodeKind::Evaluation(ControlFlowEvaluationKind::MatchGuard(0))
            )
        })
        .expect("guard evaluation should be explicit");
    assert_eq!(guard.value_types, vec![fluxc::ast::Type::Bool]);
    assert_eq!(guard.ownership.reads, vec!["allow"]);
    assert!(
        graph
            .outgoing(guard.id)
            .any(|edge| edge.kind == ControlFlowEdgeKind::GuardTrue)
    );
    assert!(
        graph
            .outgoing(guard.id)
            .any(|edge| edge.kind == ControlFlowEdgeKind::GuardFalse)
    );
}

#[test]
fn performance_benchmark_flux_sources_stay_compiler_valid() {
    for (name, source) in [
        ("compute", include_str!("../benchmarks/perf/compute.flux")),
        (
            "collections",
            include_str!("../benchmarks/perf/collections.flux"),
        ),
        ("dispatch", include_str!("../benchmarks/perf/dispatch.flux")),
    ] {
        check_source(source)
            .unwrap_or_else(|error| panic!("{name} benchmark must compile: {error:?}"));
    }
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
fn check_cli_renders_width_aware_source_diagnostics_and_color() {
    let path =
        std::env::temp_dir().join(format!("flux-human-diagnostic-{}.flux", std::process::id()));
    fs::write(
        &path,
        "fn identity(value: i64) -> i64 {\n    return value\n}\n\nfn main() -> i64 {\n    let deliberately_long_binding_name_for_terminal_width: i64 = identity(false)\n    return 0\n}\n",
    )
    .expect("temporary Flux source should be writable");

    let plain = Command::new(env!("CARGO_BIN_EXE_fluxc"))
        .arg("check")
        .arg(&path)
        .env("COLUMNS", "40")
        .env("NO_COLOR", "1")
        .env_remove("FORCE_COLOR")
        .env_remove("CLICOLOR_FORCE")
        .output()
        .expect("fluxc should render a human diagnostic");
    assert!(!plain.status.success());
    let stderr = String::from_utf8(plain.stderr).expect("human diagnostic must be UTF-8");
    assert!(stderr.contains("error[type]:"));
    assert!(stderr.contains("^^^^^"));
    assert!(stderr.contains("'identity' is declared here"));
    assert!(stderr.lines().all(|line| line.chars().count() <= 40));
    assert!(!stderr.contains("\x1b["));

    let colored = Command::new(env!("CARGO_BIN_EXE_fluxc"))
        .arg("check")
        .arg(&path)
        .env("COLUMNS", "40")
        .env("FORCE_COLOR", "1")
        .env_remove("NO_COLOR")
        .output()
        .expect("fluxc should render forced ANSI color");
    let _ = fs::remove_file(&path);
    assert!(!colored.status.success());
    assert!(
        colored
            .stderr
            .windows(5)
            .any(|window| window == b"\x1b[31m")
    );
}

#[test]
fn run_cli_rebuilds_on_dependency_saves_without_a_reload_hotkey() {
    let root = std::env::temp_dir().join(format!("flux-run-watch-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("temporary run project should be writable");
    let dependency = root.join("message.flux");
    let entry = root.join("main.flux");
    let log = root.join("run.log");
    fs::write(&dependency, "pub fn message() -> str { \"version-one\" }\n")
        .expect("run dependency should be writable");
    fs::write(
        &entry,
        "import \"message.flux\"\nfn main() -> i64 {\n    print(message())\n    return 0\n}\n",
    )
    .expect("run entry should be writable");

    let status_path =
        fluxc::project::development_status_path(&entry).expect("run status path should resolve");
    let _ = fs::remove_file(&status_path);
    let stdout = fs::File::create(&log).expect("run log should be writable");
    let stderr = stdout.try_clone().expect("run log should be cloneable");
    let mut runner = Command::new(env!("CARGO_BIN_EXE_fluxc"))
        .arg("run")
        .arg(&entry)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("fluxc run should start");

    wait_for_log(
        &log,
        &["version-one", "run: started"],
        Duration::from_secs(5),
    );
    fs::write(&dependency, "pub fn message() -> str { \"version-two\" }\n")
        .expect("dependency update should be writable");
    wait_for_log(
        &log,
        &[
            "version-two",
            "reload: rebuilt and restarted after source change",
        ],
        Duration::from_secs(5),
    );
    wait_for_run_status(&status_path, "restarted", 1, Duration::from_secs(5));

    fs::write(&dependency, "pub fn message() -> str { false }\n")
        .expect("broken dependency should be writable");
    wait_for_log(
        &log,
        &["expected str, got bool", "reload: compile failed"],
        Duration::from_secs(5),
    );
    wait_for_run_status(&status_path, "compile_error", 1, Duration::from_secs(5));
    fs::write(
        &dependency,
        "pub fn message() -> str { \"version-three\" }\n",
    )
    .expect("repaired dependency should be writable");
    wait_for_log(&log, &["version-three"], Duration::from_secs(5));
    wait_for_run_status(&status_path, "restarted", 2, Duration::from_secs(5));

    let _ = runner.kill();
    let _ = runner.wait();
    let _ = fs::remove_file(status_path);
    let _ = fs::remove_dir_all(&root);
}

fn wait_for_run_status(path: &std::path::Path, state: &str, generation: usize, timeout: Duration) {
    let start = Instant::now();
    loop {
        let text = fs::read_to_string(path).unwrap_or_default();
        if text.contains(&format!("\"state\":\"{state}\""))
            && text.contains(&format!("\"generation\":{generation}"))
        {
            return;
        }
        assert!(
            start.elapsed() < timeout,
            "timed out waiting for run status {state}/{generation}; status was:\n{text}"
        );
        thread::sleep(Duration::from_millis(40));
    }
}

fn wait_for_log(path: &std::path::Path, needles: &[&str], timeout: Duration) {
    let start = Instant::now();
    loop {
        let text = fs::read_to_string(path).unwrap_or_default();
        if needles.iter().all(|needle| text.contains(needle)) {
            return;
        }
        assert!(
            start.elapsed() < timeout,
            "timed out waiting for {needles:?}; log was:\n{text}"
        );
        thread::sleep(Duration::from_millis(40));
    }
}

#[test]
fn lsp_cli_publishes_open_and_change_diagnostics_over_json_rpc() {
    let uri = "file:///tmp/flux-lsp-test.flux";
    let initialize = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{"general":{"positionEncodings":["utf-8","utf-16"]}}}}"#;
    let initialized = r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#;
    let open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{uri}","languageId":"flux","version":1,"text":"fn main() -> i64 {{\n    let count: i64 = false\n    return count\n}}\n"}}}}}}"#
    );
    let change = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{uri}","version":2}},"contentChanges":[{{"text":"fn main()->i64 {{\n  let count:i64=1\n  return count\n}}\n"}}]}}}}"#
    );
    let hover = format!(
        r#"{{"jsonrpc":"2.0","id":5,"method":"textDocument/hover","params":{{"textDocument":{{"uri":"{uri}"}},"position":{{"line":1,"character":7}}}}}}"#
    );
    let semantic_tokens = format!(
        r#"{{"jsonrpc":"2.0","id":6,"method":"textDocument/semanticTokens/full","params":{{"textDocument":{{"uri":"{uri}"}}}}}}"#
    );
    let completion = format!(
        r#"{{"jsonrpc":"2.0","id":7,"method":"textDocument/completion","params":{{"textDocument":{{"uri":"{uri}"}},"position":{{"line":1,"character":2}}}}}}"#
    );
    let formatting = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"textDocument/formatting","params":{{"textDocument":{{"uri":"{uri}"}},"options":{{"tabSize":4,"insertSpaces":true}}}}}}"#
    );
    let broken_change = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{uri}","version":3}},"contentChanges":[{{"text":"fn main() -> i64\n    return 0\n}}\n"}}]}}}}"#
    );
    let code_action = format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"textDocument/codeAction","params":{{"textDocument":{{"uri":"{uri}"}},"range":{{"start":{{"line":0,"character":0}},"end":{{"line":0,"character":16}}}},"context":{{"diagnostics":[]}}}}}}"#
    );
    let close = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didClose","params":{{"textDocument":{{"uri":"{uri}"}}}}}}"#
    );
    let shutdown = r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}"#;
    let exit = r#"{"jsonrpc":"2.0","method":"exit","params":null}"#;
    let input = [
        initialize,
        initialized,
        &open,
        &change,
        &hover,
        &semantic_tokens,
        &completion,
        &formatting,
        &broken_change,
        &code_action,
        &close,
        shutdown,
        exit,
    ]
    .into_iter()
    .map(lsp_frame)
    .collect::<String>();

    let mut child = Command::new(env!("CARGO_BIN_EXE_fluxc"))
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("fluxc lsp should start");
    child
        .stdin
        .as_mut()
        .expect("LSP stdin should be piped")
        .write_all(input.as_bytes())
        .expect("LSP input should be writable");
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("LSP should exit cleanly");
    assert!(
        output.status.success(),
        "LSP exited unsuccessfully: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("LSP output should be UTF-8");
    assert!(stdout.contains("\"positionEncoding\":\"utf-8\""));
    assert!(stdout.contains("\"codeActionProvider\":true"));
    assert!(stdout.contains("\"completionProvider\""));
    assert!(stdout.contains("\"documentFormattingProvider\":true"));
    assert!(stdout.contains("\"hoverProvider\":true"));
    assert!(stdout.contains("\"fluxHotReloadStatus\":true"));
    assert!(stdout.contains("\"inlayHintProvider\":true"));
    assert!(stdout.contains("\"definitionProvider\":true"));
    assert!(stdout.contains("\"referencesProvider\":true"));
    assert!(stdout.contains("\"renameProvider\":true"));
    assert!(stdout.contains("\"signatureHelpProvider\""));
    assert!(stdout.contains("\"semanticTokensProvider\""));
    assert!(stdout.contains("\"tokenTypes\":[\"keyword\",\"string\",\"number\""));
    assert!(stdout.contains("textDocument/publishDiagnostics"));
    assert!(stdout.contains("binding: expected i64, got bool"));
    assert!(stdout.contains("\"severity\":1"));
    assert!(stdout.matches("\"diagnostics\":[]").count() >= 2);
    assert!(stdout.contains("\"id\":5"));
    assert!(stdout.contains("let count: i64"));
    assert!(stdout.contains("\"id\":6,\"jsonrpc\":\"2.0\",\"result\":{\"data\":["));
    assert!(!stdout.contains("\"id\":6,\"jsonrpc\":\"2.0\",\"result\":{\"data\":[]}"));
    assert!(stdout.contains("\"id\":7"));
    assert!(stdout.contains("\"label\":\"while\""));
    assert!(stdout.contains("\"label\":\"main\""));
    assert!(stdout.contains("\"id\":3"));
    assert!(stdout.contains("fn main() -> i64"));
    assert!(stdout.contains("\\n    let count: i64 = 1\\n"));
    assert!(stdout.contains("\"id\":4"));
    assert!(stdout.contains("insert the function body opener"));
    assert!(stdout.contains("\"kind\":\"quickfix\""));
    assert!(stdout.contains("\"newText\":\" {\""));
    assert!(stdout.contains("\"id\":2,\"jsonrpc\":\"2.0\",\"result\":null"));
}

#[test]
fn lsp_cli_serves_signature_navigation_references_and_safe_rename() {
    let uri = "file:///tmp/flux-lsp-navigation.flux";
    let initialize =
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#;
    let open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{uri}","languageId":"flux","version":1,"text":"fn add(left: i64, right: i64) -> i64 {{ left + right }}\nfn main() -> i64 {{ add(1, 2) }}\n"}}}}}}"#
    );
    let signature = format!(
        r#"{{"jsonrpc":"2.0","id":8,"method":"textDocument/signatureHelp","params":{{"textDocument":{{"uri":"{uri}"}},"position":{{"line":1,"character":25}}}}}}"#
    );
    let definition = format!(
        r#"{{"jsonrpc":"2.0","id":9,"method":"textDocument/definition","params":{{"textDocument":{{"uri":"{uri}"}},"position":{{"line":1,"character":19}}}}}}"#
    );
    let references = format!(
        r#"{{"jsonrpc":"2.0","id":10,"method":"textDocument/references","params":{{"textDocument":{{"uri":"{uri}"}},"position":{{"line":1,"character":19}},"context":{{"includeDeclaration":true}}}}}}"#
    );
    let rename = format!(
        r#"{{"jsonrpc":"2.0","id":11,"method":"textDocument/rename","params":{{"textDocument":{{"uri":"{uri}"}},"position":{{"line":1,"character":19}},"newName":"sum"}}}}"#
    );
    let invalid_rename = format!(
        r#"{{"jsonrpc":"2.0","id":12,"method":"textDocument/rename","params":{{"textDocument":{{"uri":"{uri}"}},"position":{{"line":1,"character":19}},"newName":"while"}}}}"#
    );
    let shutdown = r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}"#;
    let exit = r#"{"jsonrpc":"2.0","method":"exit","params":null}"#;
    let input = [
        initialize,
        &open,
        &signature,
        &definition,
        &references,
        &rename,
        &invalid_rename,
        shutdown,
        exit,
    ]
    .into_iter()
    .map(lsp_frame)
    .collect::<String>();

    let mut child = Command::new(env!("CARGO_BIN_EXE_fluxc"))
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("fluxc lsp should start");
    child
        .stdin
        .as_mut()
        .expect("LSP stdin should be piped")
        .write_all(input.as_bytes())
        .expect("LSP input should be writable");
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("LSP should exit cleanly");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("LSP output should be UTF-8");
    assert!(stdout.contains("\"id\":8"));
    assert!(stdout.contains("fn add(left: i64, right: i64) -> i64"));
    assert!(stdout.contains("\"activeParameter\":1"));
    assert!(stdout.contains("\"id\":9"));
    assert!(stdout.contains("\"character\":3,\"line\":0"));
    assert!(stdout.contains("\"id\":10"));
    assert!(
        stdout
            .matches("\"uri\":\"file:///tmp/flux-lsp-navigation.flux\"")
            .count()
            >= 4
    );
    assert!(stdout.contains("\"id\":11"));
    assert_eq!(stdout.matches("\"newText\":\"sum\"").count(), 2);
    assert!(stdout.contains("\"id\":12,\"jsonrpc\":\"2.0\",\"result\":null"));
}

#[test]
fn lsp_cli_republishes_dependent_diagnostics_for_unsaved_import_overlays() {
    let root = std::env::temp_dir().join(format!("flux-lsp-import-overlay-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("temporary LSP project should be writable");
    let dependency = root.join("dep.flux");
    let entry = root.join("main.flux");
    fs::write(&dependency, "pub fn value() -> i64 { 1 }\n").expect("dependency should be writable");
    fs::write(
        &entry,
        "import \"dep.flux\"\nfn main() -> i64 { value() }\n",
    )
    .expect("entry should be writable");
    let dependency = fs::canonicalize(dependency).unwrap();
    let entry = fs::canonicalize(entry).unwrap();
    let dependency_uri = format!("file://{}", dependency.display());
    let entry_uri = format!("file://{}", entry.display());

    let initialize =
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#;
    let open_entry = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{entry_uri}","languageId":"flux","version":1,"text":"import \"dep.flux\"\nfn main() -> i64 {{ value() }}\n"}}}}}}"#
    );
    let open_dependency = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{dependency_uri}","languageId":"flux","version":1,"text":"pub fn value() -> str {{ \"unsaved\" }}\n"}}}}}}"#
    );
    let shutdown = r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}"#;
    let exit = r#"{"jsonrpc":"2.0","method":"exit","params":null}"#;
    let input = [initialize, &open_entry, &open_dependency, shutdown, exit]
        .into_iter()
        .map(lsp_frame)
        .collect::<String>();

    let mut child = Command::new(env!("CARGO_BIN_EXE_fluxc"))
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("fluxc lsp should start");
    child
        .stdin
        .as_mut()
        .expect("LSP stdin should be piped")
        .write_all(input.as_bytes())
        .expect("LSP input should be writable");
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("LSP should exit cleanly");
    let _ = fs::remove_dir_all(root);

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("LSP output should be UTF-8");
    assert!(stdout.contains("expected i64, got str"));
    assert!(stdout.contains(&entry_uri));
    assert!(stdout.contains(&dependency_uri));
    assert!(!stdout.contains("program requires fn main"));
}

fn lsp_frame(payload: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{payload}", payload.len())
}

#[test]
fn check_cli_points_at_missing_syntax_and_prints_the_fix() {
    let path =
        std::env::temp_dir().join(format!("flux-missing-syntax-{}.flux", std::process::id()));
    fs::write(&path, "fn main() -> i64\n    return 0\n}\n")
        .expect("temporary Flux source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_fluxc"))
        .arg("check")
        .arg(&path)
        .env("COLUMNS", "60")
        .env("NO_COLOR", "1")
        .output()
        .expect("fluxc should render a missing-syntax diagnostic");
    let _ = fs::remove_file(&path);

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("human diagnostic must be UTF-8");
    assert!(stderr.contains("functions must open their body with '{'"));
    assert!(stderr.contains("fn main() -> i64"));
    assert!(stderr.contains("help: insert the function body opener"));
    assert!(stderr.contains("^"));
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
fn flux_binary_exposes_the_user_facing_cli_name_and_commands() {
    let checked = Command::new(env!("CARGO_BIN_EXE_flux"))
        .arg("check")
        .arg("examples/hello.flux")
        .output()
        .expect("flux user-facing binary should run");
    assert!(checked.status.success());

    let analyzed = Command::new(env!("CARGO_BIN_EXE_flux"))
        .arg("analyze")
        .arg("examples/hello.flux")
        .output()
        .expect("flux analyze should run");
    assert!(analyzed.status.success());

    let usage = Command::new(env!("CARGO_BIN_EXE_flux"))
        .output()
        .expect("flux usage should run");
    assert!(!usage.status.success());
    let stderr = String::from_utf8(usage.stderr).expect("flux usage must be UTF-8");
    assert!(stderr.starts_with("flux: usage: flux new <directory>"));
    assert!(!stderr.contains("fluxc: usage:"));
}

#[test]
fn new_cli_scaffolds_a_checked_native_gui_package_without_overwriting() {
    let root = std::env::temp_dir().join(format!("Flux New App {}", std::process::id()));
    let _ = fs::remove_dir_all(&root);

    let created = Command::new(env!("CARGO_BIN_EXE_fluxc"))
        .arg("new")
        .arg(&root)
        .output()
        .expect("fluxc new should run");
    assert!(
        created.status.success(),
        "fluxc new failed: {}",
        String::from_utf8_lossy(&created.stderr)
    );

    let manifest =
        fs::read_to_string(root.join("flux.toml")).expect("new project should contain a manifest");
    assert!(manifest.contains("name = \"flux-new-app-"));
    assert!(manifest.contains("entry = \"src/main.flux\""));
    let source = fs::read_to_string(root.join("src/main.flux"))
        .expect("new project should contain an entry source");
    assert!(source.contains("view App {"));
    assert!(source.contains("state clicked: bool = false"));
    assert!(source.contains("onPress: clicked => !clicked"));
    assert!(source.contains("app App"));
    let smoke = fs::read_to_string(root.join("tests/smoke.flux"))
        .expect("new project should contain a starter integration test");
    assert!(smoke.contains("fn main() -> i64"));
    assert!(smoke.contains("return 0"));

    let checked = Command::new(env!("CARGO_BIN_EXE_fluxc"))
        .arg("check")
        .arg(&root)
        .output()
        .expect("generated package should be checkable");
    assert!(
        checked.status.success(),
        "generated package failed check: {}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let tested = Command::new(env!("CARGO_BIN_EXE_flux"))
        .arg("test")
        .arg(&root)
        .output()
        .expect("generated package tests should run");
    assert!(
        tested.status.success(),
        "generated package tests failed: {}",
        String::from_utf8_lossy(&tested.stderr)
    );
    assert!(String::from_utf8_lossy(&tested.stdout).contains("1 passed; 0 failed"));

    let refused = Command::new(env!("CARGO_BIN_EXE_fluxc"))
        .arg("new")
        .arg(&root)
        .output()
        .expect("second fluxc new should run");
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("directory is not empty"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn project_codegen_preserves_flux_source_paths_and_statement_lines() {
    let root = std::env::temp_dir().join(format!("flux-debug-lines-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("debug-line fixture should be writable");
    let source = root.join("main.flux");
    fs::write(
        &source,
        "fn main() -> i64 {\n    let answer: i64 = 40 + 2\n    print(answer)\n    return answer\n}\n",
    )
    .expect("debug-line source should be writable");

    let generated = fluxc::project::compile_to_c(&source)
        .expect("project source should lower with debug line metadata");
    let canonical = fs::canonicalize(&source).expect("source path should canonicalize");
    let escaped = canonical
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    assert!(generated.contains(&format!("#line 1 \"{escaped}\"\nint main(void)")));
    assert!(generated.contains(&format!("#line 2 \"{escaped}\"\n")));
    assert!(generated.contains(&format!("#line 3 \"{escaped}\"\n")));
    assert!(generated.contains(&format!("#line 4 \"{escaped}\"\n")));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn native_builds_are_byte_reproducible_with_isolated_caches() {
    let root = std::env::temp_dir().join(format!("flux-reproducible-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("reproducibility fixture should be writable");
    let source = root.join("main.flux");
    fs::write(
        &source,
        "fn main() -> i64 {\n    print(42)\n    return 0\n}\n",
    )
    .expect("reproducibility source should be writable");

    for mode in ["debug", "profile", "release"] {
        let first = root.join(format!("{mode}-first"));
        let second = root.join(format!("{mode}-second"));
        for (output, cache) in [
            (&first, root.join(format!("{mode}-cache-a"))),
            (&second, root.join(format!("{mode}-cache-b"))),
        ] {
            let built = Command::new(env!("CARGO_BIN_EXE_flux"))
                .arg("build")
                .arg(&source)
                .arg("-o")
                .arg(output)
                .arg("--mode")
                .arg(mode)
                .env("FLUX_CACHE_DIR", cache)
                .output()
                .expect("reproducibility build should run");
            assert!(
                built.status.success(),
                "{mode} reproducibility build failed: {}",
                String::from_utf8_lossy(&built.stderr)
            );
        }
        assert_eq!(
            fs::read(&first).expect("first reproducible build should be readable"),
            fs::read(&second).expect("second reproducible build should be readable"),
            "{mode} builds of identical Flux source should be byte-identical"
        );
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn native_build_cache_reuses_identical_codegen_across_output_paths() {
    let root = std::env::temp_dir().join(format!("flux-native-cache-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("native cache fixture should be writable");
    let source = root.join("main.flux");
    fs::write(&source, "fn main() -> i64 {\n    return 0\n}\n")
        .expect("native cache source should be writable");
    let cache = root.join("cache");
    let first = root.join("first");
    let second = root.join("second");

    let built = Command::new(env!("CARGO_BIN_EXE_flux"))
        .arg("build")
        .arg(&source)
        .arg("-o")
        .arg(&first)
        .env("FLUX_CACHE_DIR", &cache)
        .output()
        .expect("first cached build should run");
    assert!(
        built.status.success(),
        "first cached build failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let artifacts = fs::read_dir(cache.join("native"))
        .expect("native cache directory should exist")
        .collect::<Result<Vec<_>, _>>()
        .expect("native cache should be readable");
    assert_eq!(artifacts.len(), 1);
    let cache_artifact = artifacts[0].path();
    fs::write(&cache_artifact, b"cached-native-artifact")
        .expect("cache fixture should be replaceable");

    let rebuilt = Command::new(env!("CARGO_BIN_EXE_flux"))
        .arg("build")
        .arg(&source)
        .arg("-o")
        .arg(&second)
        .env("FLUX_CACHE_DIR", &cache)
        .output()
        .expect("second cached build should run");
    assert!(
        rebuilt.status.success(),
        "second cached build failed: {}",
        String::from_utf8_lossy(&rebuilt.stderr)
    );
    assert_eq!(
        fs::read(&second).expect("second build output should be readable"),
        b"cached-native-artifact"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn flux_package_builds_a_manifest_backed_linux_bundle() {
    let root = std::env::temp_dir().join(format!("flux-package-cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src"))
        .expect("package fixture source directory should be writable");
    fs::write(
        root.join("flux.toml"),
        "[package]\nname = \"package-test\"\nversion = \"1.2.3\"\nentry = \"src/main.flux\"\n",
    )
    .expect("package fixture manifest should be writable");
    fs::write(
        root.join("src/main.flux"),
        "fn main() -> i64 {\n    print(42)\n    return 0\n}\n",
    )
    .expect("package fixture entry should be writable");
    let output = root.join("bundle");

    let packaged = Command::new(env!("CARGO_BIN_EXE_flux"))
        .arg("package")
        .arg(&root)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("flux package should run");
    assert!(
        packaged.status.success(),
        "flux package failed: {}",
        String::from_utf8_lossy(&packaged.stderr)
    );
    assert!(output.join("package-test").is_file());
    assert_eq!(
        fs::read_to_string(output.join("flux.toml")).expect("bundle manifest should be readable"),
        fs::read_to_string(root.join("flux.toml")).expect("source manifest should be readable")
    );
    let run = Command::new(output.join("package-test"))
        .output()
        .expect("packaged native binary should execute");
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "42");

    let repeated = Command::new(env!("CARGO_BIN_EXE_flux"))
        .arg("package")
        .arg(&root)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("repeated flux package should run");
    assert!(!repeated.status.success());
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("already exists"));

    let raw_source = Command::new(env!("CARGO_BIN_EXE_flux"))
        .arg("package")
        .arg(root.join("src/main.flux"))
        .arg("-o")
        .arg(root.join("source-bundle"))
        .output()
        .expect("source package rejection should run");
    assert!(!raw_source.status.success());
    assert!(String::from_utf8_lossy(&raw_source.stderr).contains("manifest-backed"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn flux_devices_reports_the_bootstrap_linux_target() {
    let output = Command::new(env!("CARGO_BIN_EXE_flux"))
        .arg("devices")
        .output()
        .expect("flux devices should run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("device output should be UTF-8");
    assert!(stdout.starts_with("Flux devices\n"));
    if cfg!(target_os = "linux") {
        assert!(stdout.contains("linux-desktop"));
        assert!(stdout.contains("GTK4"));
    }
}

#[test]
fn flux_clean_removes_default_build_and_development_status_artifacts() {
    let root = std::env::temp_dir().join(format!("flux-clean-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).expect("clean test source directory should be writable");
    fs::write(
        root.join("flux.toml"),
        "[package]\nname = \"clean-test\"\nentry = \"src/main.flux\"\n",
    )
    .expect("clean test manifest should be writable");
    let entry = root.join("src/main.flux");
    fs::write(&entry, "fn main() -> i64 {\n    return 0\n}\n")
        .expect("clean test entry should be writable");

    let built = Command::new(env!("CARGO_BIN_EXE_flux"))
        .arg("build")
        .arg(&root)
        .output()
        .expect("flux build should run");
    assert!(
        built.status.success(),
        "clean fixture build failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let binary = entry.with_extension("");
    assert!(binary.exists());
    let status = fluxc::project::development_status_path(&root)
        .expect("clean fixture should have a status path");
    fs::write(&status, "{}\n").expect("status fixture should be writable");

    let cleaned = Command::new(env!("CARGO_BIN_EXE_flux"))
        .arg("clean")
        .arg(&root)
        .output()
        .expect("flux clean should run");
    assert!(
        cleaned.status.success(),
        "flux clean failed: {}",
        String::from_utf8_lossy(&cleaned.stderr)
    );
    assert!(!binary.exists());
    assert!(!status.exists());

    let cleaned_again = Command::new(env!("CARGO_BIN_EXE_flux"))
        .arg("clean")
        .arg(&root)
        .output()
        .expect("second flux clean should run");
    assert!(cleaned_again.status.success());
    assert!(String::from_utf8_lossy(&cleaned_again.stdout).contains("nothing to remove"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn flux_test_propagates_native_nonzero_exit_status() {
    let path = std::env::temp_dir().join(format!("flux-failing-test-{}.flux", std::process::id()));
    fs::write(&path, "fn main() -> i64 {\n    return 7\n}\n")
        .expect("failing Flux test should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_flux"))
        .arg("test")
        .arg(&path)
        .output()
        .expect("flux test should run");
    let _ = fs::remove_file(&path);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exited with 7"));
}

#[test]
fn package_manifest_resolves_entry_and_builds_from_directory_or_manifest() {
    let root = std::env::temp_dir().join(format!("flux-package-manifest-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).expect("temporary package directory should be writable");
    let manifest = root.join("flux.toml");
    fs::write(
        &manifest,
        "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nentry = \"src/main.flux\"\n",
    )
    .expect("manifest should be writable");
    let entry = root.join("src/main.flux");
    fs::write(
        &entry,
        "fn main() -> i64 {\n    print(42)\n    return 0\n}\n",
    )
    .expect("package entry should be writable");

    let parsed = fluxc::project::read_manifest(&manifest).expect("manifest should parse");
    assert_eq!(parsed.name, "sample");
    assert_eq!(parsed.version.as_deref(), Some("0.1.0"));
    assert_eq!(parsed.entry, fs::canonicalize(&entry).unwrap());
    assert_eq!(
        fluxc::project::resolve_entry(&root).expect("directory should resolve through flux.toml"),
        parsed.entry
    );
    assert_eq!(
        fluxc::project::resolve_entry(&manifest).expect("manifest path should resolve its entry"),
        parsed.entry
    );

    for target in [&root, &manifest] {
        let binary = root.join(if target == &root {
            "from-dir"
        } else {
            "from-manifest"
        });
        let output = Command::new(env!("CARGO_BIN_EXE_fluxc"))
            .arg("build")
            .arg(target)
            .arg("-o")
            .arg(&binary)
            .output()
            .expect("fluxc build should run");
        assert!(
            output.status.success(),
            "package build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let run = Command::new(&binary)
            .output()
            .expect("package binary should run");
        assert!(run.status.success());
        assert_eq!(String::from_utf8(run.stdout).unwrap(), "42\n");
    }

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn package_manifest_rejects_invalid_schema_and_escaping_entries() {
    let root = std::env::temp_dir().join(format!("flux-package-errors-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).expect("temporary package directory should be writable");
    fs::write(root.join("src/main.flux"), "fn main() -> i64 { 0 }\n")
        .expect("entry should be writable");

    let missing_name = root.join("missing-name.toml");
    fs::write(&missing_name, "[package]\nentry = \"src/main.flux\"\n")
        .expect("invalid manifest should be writable");
    let errors = fluxc::project::read_manifest(&missing_name)
        .expect_err("manifest without package name must fail");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("requires a non-empty [package].name")
    }));

    let unknown = root.join("unknown.toml");
    fs::write(
        &unknown,
        "[package]\nname = \"sample\"\nentry = \"src/main.flux\"\nlicense = \"MIT\"\n",
    )
    .expect("invalid manifest should be writable");
    let errors = fluxc::project::read_manifest(&unknown)
        .expect_err("unknown package fields must fail until specified");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("unknown [package] field 'license'"))
    );

    let outside = root.join("outside.flux");
    fs::write(&outside, "fn main() -> i64 { 0 }\n").expect("outside source should be writable");
    let nested = root.join("package");
    fs::create_dir_all(&nested).expect("nested package should be writable");
    let escaping = nested.join("flux.toml");
    fs::write(
        &escaping,
        "[package]\nname = \"escape\"\nentry = \"../outside.flux\"\n",
    )
    .expect("escaping manifest should be writable");
    let errors = fluxc::project::read_manifest(&escaping)
        .expect_err("package entries may not escape the package root");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("must remain inside the package root")
    }));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn project_imports_compile_transitively_through_cli() {
    let root = std::env::temp_dir().join(format!("flux-project-imports-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("temporary project directory should be writable");
    fs::write(
        root.join("math.flux"),
        "pub fn double(value: i64) -> i64 { value * 2 }\n",
    )
    .expect("math module should be writable");
    fs::write(
        root.join("service.flux"),
        "import \"math.flux\"\npub fn answer() -> i64 { double(21) }\n",
    )
    .expect("service module should be writable");
    let entry = root.join("main.flux");
    fs::write(
        &entry,
        "import \"service.flux\"\nimport \"math.flux\"\nfn main() -> i64 {\n    print(answer())\n    return 0\n}\n",
    )
    .expect("entry module should be writable");

    let (program, sources) = fluxc::project::load(&entry).expect("import graph should load");
    assert_eq!(sources.len(), 3, "duplicate imports must be loaded once");
    assert_eq!(program.functions.len(), 3);
    let module_names = sources
        .iter()
        .map(|source| source.module_name.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        module_names,
        std::collections::HashSet::from(["main", "math", "service"])
    );
    let source_ids = sources
        .iter()
        .map(|source| source.source_id)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        source_ids.len(),
        3,
        "each source should retain a distinct ID"
    );

    let binary = root.join("app");
    let output = Command::new(env!("CARGO_BIN_EXE_fluxc"))
        .arg("build")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("fluxc build should run");
    assert!(
        output.status.success(),
        "project build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = Command::new(&binary)
        .output()
        .expect("built project binary should run");
    assert!(run.status.success());
    assert_eq!(String::from_utf8(run.stdout).unwrap(), "42\n");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn project_analysis_uses_unsaved_source_overlays_across_imports() {
    let root = std::env::temp_dir().join(format!("flux-project-overlays-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("temporary overlay project should be writable");
    let dependency = root.join("dep.flux");
    let entry = root.join("main.flux");
    fs::write(&dependency, "pub fn value() -> i64 { 1 }\n").expect("dependency should be writable");
    fs::write(
        &entry,
        "import \"dep.flux\"\nfn main() -> i64 { value() }\n",
    )
    .expect("entry should be writable");
    fluxc::project::check(&entry).expect("on-disk project should typecheck");

    let dependency = fs::canonicalize(dependency).unwrap();
    let overlays = std::collections::HashMap::from([(
        dependency,
        "pub fn value() -> str { \"unsaved\" }\n".to_string(),
    )]);
    let (diagnostics, sources) = fluxc::project::check_with_overlays(&entry, &overlays);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("expected i64, got str") })
    );
    assert!(sources.iter().any(|source| source.text.contains("unsaved")));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn project_analysis_cache_reuses_unchanged_graphs_and_invalidates_changed_sources() {
    let root = std::env::temp_dir().join(format!("flux-project-cache-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("temporary cached project should be writable");
    let dependency = root.join("dep.flux");
    let entry = root.join("main.flux");
    fs::write(&dependency, "pub fn value() -> i64 { 1 }\n").expect("dependency should be writable");
    fs::write(
        &entry,
        "import \"dep.flux\"\nfn main() -> i64 { value() }\n",
    )
    .expect("entry should be writable");

    let mut cache = fluxc::project::ProjectAnalysisCache::default();
    let overlays = std::collections::HashMap::new();
    cache
        .analyze_with_overlays(&entry, &overlays)
        .expect("first analysis should succeed");
    cache
        .analyze_with_overlays(&entry, &overlays)
        .expect("unchanged analysis should be reused");
    assert_eq!(
        cache.stats(),
        fluxc::project::ProjectAnalysisCacheStats { hits: 1, misses: 1 }
    );
    assert_eq!(
        cache.module_parse_stats(),
        fluxc::project::ModuleParseCacheStats { hits: 0, misses: 2 }
    );

    let dependency = fs::canonicalize(dependency).unwrap();
    cache.invalidate_path(&dependency);
    cache
        .analyze_with_overlays(&entry, &overlays)
        .expect("unchanged modules should be reused after graph invalidation");
    assert_eq!(
        cache.module_parse_stats(),
        fluxc::project::ModuleParseCacheStats { hits: 2, misses: 2 }
    );

    let overlays = std::collections::HashMap::from([(
        dependency.clone(),
        "pub fn value() -> str { \"overlay\" }\n".to_string(),
    )]);
    let errors = cache
        .analyze_with_overlays(&entry, &overlays)
        .expect_err("changed overlay must invalidate the cached project");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("expected i64, got str"))
    );
    assert_eq!(cache.stats().misses, 3);
    assert_eq!(
        cache.module_parse_stats(),
        fluxc::project::ModuleParseCacheStats { hits: 3, misses: 3 }
    );

    cache.invalidate_path(&dependency);
    fs::write(&dependency, "pub fn value() -> i64 { 2 }\n")
        .expect("dependency update should be writable");
    cache
        .analyze_with_overlays(&entry, &std::collections::HashMap::new())
        .expect("invalidated dependency should be reanalyzed");
    assert_eq!(cache.stats().misses, 4);
    assert_eq!(
        cache.module_parse_stats(),
        fluxc::project::ModuleParseCacheStats { hits: 4, misses: 4 }
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn package_sources_receive_stable_package_qualified_module_names() {
    let root = std::env::temp_dir().join(format!("flux-package-modules-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src/util"))
        .expect("temporary package directory should be writable");
    fs::write(
        root.join("flux.toml"),
        "[package]\nname = \"sample-app\"\nentry = \"src/main.flux\"\n",
    )
    .expect("manifest should be writable");
    fs::write(
        root.join("src/util/math.flux"),
        "pub fn answer() -> i64 { 42 }\n",
    )
    .expect("dependency should be writable");
    fs::write(
        root.join("src/main.flux"),
        "import \"util/math.flux\"\nfn main() -> i64 { answer() }\n",
    )
    .expect("entry should be writable");

    let (_, sources) = fluxc::project::load(&root).expect("package import graph should load");
    let module_names = sources
        .iter()
        .map(|source| source.module_name.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        module_names,
        std::collections::HashSet::from(["sample-app::src::main", "sample-app::src::util::math"])
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn accepts_flat_grid_views_without_widget_nesting() {
    let source = r#"
view Dashboard {
    grid columns: 240 1fr 320
    grid rows: 64 1fr
    grid gap: 16
    Text title at 1,2 span columns 2
        text: "Flux dashboard"
    Nav sidebar at 1,1 span rows 2
        label: "Navigation"
    Chart revenue at 2,2
        label: "Revenue"
}

fn main() -> i64 { 42 }
"#;

    let program = fluxc::parser::parse(source).expect("flat grid view should parse");
    assert_eq!(program.views.len(), 1);
    let view = &program.views[0];
    assert_eq!(view.name, "Dashboard");
    assert_eq!(view.grid.columns.len(), 3);
    assert_eq!(view.grid.rows.len(), 2);
    assert_eq!(view.grid.gap, Some(16));
    assert_eq!(view.elements.len(), 3);
    assert_eq!(view.elements[0].name, "title");
    assert_eq!(view.elements[0].column_span, 2);
    assert_eq!(view.elements[1].row_span, 2);
    check_source(source).expect("view declarations must not disturb ordinary type checking");
    let database =
        fluxc::semantic::SemanticDatabase::analyze(source, SourceId::from_name("grid.flux"))
            .expect("view should be available to editor semantics");
    assert!(
        database
            .symbols()
            .iter()
            .any(|symbol| symbol.name == "Dashboard")
    );
    assert!(
        database
            .symbols()
            .iter()
            .any(|symbol| symbol.name == "revenue")
    );
    let generated = compile_to_c(source).expect("views should be zero-runtime metadata for now");
    assert!(generated.contains("int main(void)"));
}

#[test]
fn lowers_app_root_view_to_native_gtk_grid_text_button_and_callback() {
    let source = r#"
fn clicked() -> void {
    print("clicked")
}

view HelloApp {
    grid columns: 1fr
    grid rows: auto auto
    grid gap: 12
    Text title at 1,1
        text: "Hello, Flux!"
    Button action at 2,1
        text: "Click me"
        on_press: clicked
}

app HelloApp
"#;

    check_source(source).expect("zero-parameter app root should typecheck without fn main");
    let program = fluxc::parser::parse(source).expect("application declaration should parse");
    assert_eq!(program.application.as_ref().unwrap().view_name, "HelloApp");
    let generated = compile_to_c(source).expect("Linux app root should lower to GTK4 C");
    assert!(generated.contains("#include <gtk/gtk.h>"));
    assert!(generated.contains("gtk_application_window_new"));
    assert!(generated.contains("gtk_grid_attach"));
    assert!(generated.contains("gtk_label_new(\"Hello, Flux!\")"));
    assert!(generated.contains("gtk_button_new_with_label(\"Click me\")"));
    assert!(generated.contains("G_CALLBACK(flux__ui_click_action)"));
    assert!(generated.contains("flux__fn_clicked();"));
    assert!(generated.contains("int main(int argc, char **argv)"));
}

#[test]
fn app_view_state_transitions_lower_to_native_state_and_refresh() {
    let source = r##"
view Counter {
    grid columns: 1fr
    grid rows: auto auto auto
    grid padding: 24
    state clicked: bool = false
    Text title at 1,1
        text: "Hello"
        visible: !clicked
        size: 28
        bold: true
        color: "#4F46E5"
    Text status at 2,1
        text: "Clicked!"
        visible: clicked
    Button action at 3,1
        text: "Toggle"
        primary: true
        on_press: clicked => !clicked
}
app Counter
"##;

    check_source(source).expect("typed view state transition should typecheck");
    let program = fluxc::parser::parse(source).expect("stateful app should parse");
    let view = &program.views[0];
    assert_eq!(view.states.len(), 1);
    assert_eq!(view.states[0].name, "clicked");
    let action = view
        .elements
        .iter()
        .find(|element| element.name == "action")
        .and_then(|element| {
            element
                .properties
                .iter()
                .find(|property| property.name == "on_press")
        })
        .expect("button transition should be retained");
    assert_eq!(action.transition.as_ref().unwrap().state, "clicked");

    let formatted = fluxc::formatter::format_source(source).expect("stateful view should format");
    assert!(formatted.contains("grid padding: 24"));
    assert!(formatted.contains("state clicked: bool = false"));
    assert!(formatted.contains("on_press: clicked => !clicked"));

    let generated = compile_to_c(source).expect("stateful Linux app should lower");
    assert!(generated.contains("static bool flux__ui_state_clicked = false;"));
    assert!(generated.contains("flux__ui_state_clicked = (!(flux__ui_state_clicked));"));
    assert!(generated.contains("flux__ui_refresh();"));
    assert!(generated.contains("gtk_label_set_text"));
    assert!(generated.contains("gtk_button_set_label"));
    assert!(generated.contains("gtk_widget_set_margin_top(grid, 24)"));
    assert!(generated.contains("pango_attr_size_new(28 * PANGO_SCALE)"));
    assert!(generated.contains("PANGO_WEIGHT_BOLD"));
    assert!(generated.contains("pango_attr_foreground_new(20303, 17990, 58853)"));
    assert!(generated.contains("gtk_widget_add_css_class(flux__ui_action, \"suggested-action\")"));
    assert!(generated.contains("flux__ui_state_clicked") && generated.contains("Clicked!"));
}

#[test]
fn rejects_invalid_bootstrap_text_color() {
    let source = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        color: "indigo"
}
app Screen
"#;
    check_source(source).expect("color property should have the expected str type");
    let error = compile_to_c(source).expect_err("invalid bootstrap color syntax must fail codegen");
    assert!(error.message.contains("Text.color must use '#RRGGBB'"));
}

#[test]
fn app_i64_view_state_supports_arithmetic_and_derived_properties() {
    let source = r#"
view Counter {
    grid columns: 1fr
    grid rows: auto auto
    state count: i64 = 0
    Text status at 1,1
        text: "positive"
        visible: count > 0
    Button action at 2,1
        text: "Increment"
        enabled: count >= 0
        on_press: count => count + 1
}
app Counter
"#;

    check_source(source).expect("i64 view state transition should typecheck");
    let generated = compile_to_c(source).expect("i64 view state should lower natively");
    assert!(generated.contains("static int64_t flux__ui_state_count = INT64_C(0);"));
    assert!(generated.contains("flux_add_i64(flux__ui_state_count, INT64_C(1))"));
    assert!(generated.contains("flux__ui_state_count > INT64_C(0)"));
    assert!(generated.contains("gtk_label_set_text"));
    assert!(generated.contains("gtk_button_set_label"));
}

#[test]
fn app_i64_view_state_uses_checked_integer_division() {
    let source = r#"
view Counter {
    grid columns: 1fr
    grid rows: auto
    state count: i64 = 8
    Button action at 1,1
        on_press: count => count / 2
}
app Counter
"#;
    check_source(source).expect("i64 division state transition should typecheck");
    let generated = compile_to_c(source).expect("i64 division transition should lower");
    assert!(generated.contains("flux_div_i64(flux__ui_state_count, INT64_C(2))"));
}

#[test]
fn root_grid_can_lower_to_native_scrolling_without_source_wrappers() {
    let source = r#"
view Feed {
    grid columns: 1fr
    grid rows: auto auto auto
    grid scroll: true
    Text first at 1,1
        text: "One"
    Text second at 2,1
        text: "Two"
    Text third at 3,1
        text: "Three"
}
app Feed(width: 320, height: 120)
"#;
    check_source(source).expect("scrollable flat grid should typecheck");
    let formatted = fluxc::formatter::format_source(source).expect("scrolling should format");
    assert!(formatted.contains("grid scroll: true"));
    let generated = compile_to_c(source).expect("scrolling should lower natively");
    assert!(generated.contains("gtk_scrolled_window_new()"));
    assert!(
        generated.contains("gtk_scrolled_window_set_child(GTK_SCROLLED_WINDOW(scroller), grid)")
    );
    assert!(generated.contains("gtk_window_set_child(GTK_WINDOW(window), scroller)"));

    let invalid = r#"
view Feed {
    grid columns: 1fr
    grid rows: auto
    grid scroll: yes
}
app Feed
"#;
    let errors = check_source_all(invalid).expect_err("grid scroll must require a bool literal");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("grid scroll must be the boolean literal true or false")
    }));
}

#[test]
fn text_color_validates_hex_and_lowers_to_native_pango_attributes() {
    let source = r##"
view Screen {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: "Flux"
        color: "#2563EB80"
}
app Screen
"##;
    check_source(source).expect("Text.color should typecheck as a string property");
    let generated = compile_to_c(source).expect("valid hex Text.color should lower natively");
    assert!(generated.contains("pango_attr_foreground_new(9509, 25443, 60395)"));
    assert!(generated.contains("pango_attr_foreground_alpha_new(32896)"));

    let invalid = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: "Flux"
        color: "blue"
}
app Screen
"#;
    check_source(invalid).expect("hex validation happens in native lowering");
    let error = compile_to_c(invalid).expect_err("non-hex Text.color must fail");
    assert!(error.message.contains("#RRGGBBAA"));
}

#[test]
fn text_typography_properties_lower_to_native_pango_attributes() {
    let source = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: "Flux"
        font_family: "DejaVu Sans"
        size: 20
        bold: true
        italic: true
        underline: true
        strikethrough: true
        letter_spacing: 2
        line_height_percent: 140
        text_align: "center"
        wrap: false
        wrap_mode: "word_char"
        ellipsize: "end"
        max_lines: 2
}
app Screen
"#;
    check_source(source).expect("typography properties should have compiler-owned Text contracts");
    let generated =
        compile_to_c(source).expect("typography should lower to native Pango attributes");
    assert!(generated.contains("pango_attr_family_new(\"DejaVu Sans\")"));
    assert!(generated.contains("pango_attr_size_new(20 * PANGO_SCALE)"));
    assert!(generated.contains("pango_attr_weight_new(PANGO_WEIGHT_BOLD)"));
    assert!(generated.contains("pango_attr_style_new(PANGO_STYLE_ITALIC)"));
    assert!(generated.contains("pango_attr_underline_new(PANGO_UNDERLINE_SINGLE)"));
    assert!(generated.contains("pango_attr_strikethrough_new(TRUE)"));
    assert!(generated.contains("pango_attr_letter_spacing_new(2 * PANGO_SCALE)"));
    assert!(generated.contains("pango_attr_line_height_new(1.4000)"));
    assert!(
        generated.contains("gtk_label_set_justify(GTK_LABEL(flux__ui_title), GTK_JUSTIFY_CENTER)")
    );
    assert!(generated.contains("gtk_label_set_wrap(GTK_LABEL(flux__ui_title), false)"));
    assert!(
        generated
            .contains("gtk_label_set_wrap_mode(GTK_LABEL(flux__ui_title), PANGO_WRAP_WORD_CHAR)")
    );
    assert!(
        generated
            .contains("gtk_label_set_ellipsize(GTK_LABEL(flux__ui_title), PANGO_ELLIPSIZE_END)")
    );
    assert!(generated.contains("gtk_label_set_lines(GTK_LABEL(flux__ui_title), 2)"));

    let empty_family = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: "Flux"
        font_family: ""
}
app Screen
"#;
    check_source(empty_family).expect("font family emptiness is a native typography validation");
    let error = compile_to_c(empty_family).expect_err("empty font family should fail");
    assert!(error.message.contains("Text.font_family cannot be empty"));

    let bad_line_height = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: "Flux"
        line_height_percent: 0
}
app Screen
"#;
    check_source(bad_line_height).expect("line height range is validated by native lowering");
    let error = compile_to_c(bad_line_height).expect_err("zero line height should fail");
    assert!(
        error
            .message
            .contains("line_height_percent must be greater than zero")
    );

    let bad_ellipsize = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: "Flux"
        ellipsize: "around"
}
app Screen
"#;
    check_source(bad_ellipsize).expect("ellipsize enum is validated by native lowering");
    let error = compile_to_c(bad_ellipsize).expect_err("unknown ellipsize mode should fail");
    assert!(error.message.contains("Text.ellipsize must be one of"));
}

#[test]
fn native_elements_support_background_borders_and_radii() {
    let source = r##"
view Styled {
    grid columns: 1fr
    grid rows: auto
    Button action at 1,1
        text: "Styled"
        background_color: "#2563EB"
        border_color: "#1E3A8AFF"
        border_width: 2
        border_bottom_width: 5
        border_style: "dashed"
        radius: 12
        radius_top_left: 24
        radius_bottom_right: 4
        padding: 8
        padding_start: 20
}
app Styled
"##;
    check_source(source).expect("common style properties should typecheck");
    let generated = compile_to_c(source).expect("common styles should lower to native GTK CSS");
    assert!(generated.contains("gtk_widget_set_name(flux__ui_action, \"flux-ui-action\")"));
    assert!(generated.contains("background-color: #2563EB;"));
    assert!(generated.contains("border-color: #1E3A8AFF;"));
    assert!(generated.contains("border-width: 2px;"));
    assert!(generated.contains("border-bottom-width: 5px;"));
    assert!(generated.contains("border-style: dashed;"));
    assert!(generated.contains("border-radius: 12px;"));
    assert!(generated.contains("border-top-left-radius: 24px;"));
    assert!(generated.contains("border-bottom-right-radius: 4px;"));
    assert!(generated.contains("padding-top: 8px;"));
    assert!(generated.contains("padding-bottom: 8px;"));
    assert!(generated.contains("padding-left: 20px;"));
    assert!(generated.contains("padding-right: 8px;"));
    assert!(generated.contains("GTK_STYLE_PROVIDER_PRIORITY_APPLICATION"));

    let invalid = r#"
view Styled {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        background_color: "blue"
}
app Styled
"#;
    check_source(invalid).expect("style colors typecheck before native syntax validation");
    let error = compile_to_c(invalid).expect_err("non-hex background color must fail");
    assert!(
        error
            .message
            .contains("background_color must use '#RRGGBB'")
    );

    let bad_padding = r#"
view Styled {
    grid columns: 1fr
    grid rows: auto
    Button action at 1,1
        text: "Bad"
        padding: -1
}
app Styled
"#;
    check_source(bad_padding).expect("padding typechecks before native range validation");
    let error = compile_to_c(bad_padding).expect_err("negative padding must fail");
    assert!(error.message.contains("padding must be non-negative"));

    let bad_border_style = r#"
view Styled {
    grid columns: 1fr
    grid rows: auto
    Button action at 1,1
        text: "Bad"
        border_width: 1
        border_style: "wavy"
}
app Styled
"#;
    check_source(bad_border_style).expect("border style enum validates during native lowering");
    let error = compile_to_c(bad_border_style).expect_err("unsupported border style must fail");
    assert!(error.message.contains("border_style must be one of"));
}

#[test]
fn native_elements_support_dynamic_clipping() {
    let source = r#"
view Clipped {
    state clipped: bool = true
    grid columns: 1fr
    grid rows: auto
    Image picture at 1,1
        source: "image.png"
        clip: clipped
}
app Clipped
"#;
    check_source(source).expect("clip should typecheck as a common bool property");
    let generated = compile_to_c(source).expect("clip should lower to native GTK overflow");
    assert!(generated.contains("GTK_OVERFLOW_HIDDEN : GTK_OVERFLOW_VISIBLE"));
    assert!(generated.contains("gtk_widget_set_overflow(flux__ui_picture"));
    assert!(generated.contains("if (flux__ui_picture != NULL) gtk_widget_set_overflow"));
}

#[test]
fn native_elements_support_native_shadows() {
    let source = r##"
view Shadowed {
    grid columns: 1fr
    grid rows: auto
    Button panel at 1,1
        text: "Shadow"
        shadow_color: "#11223380"
        shadow_blur: 18
        shadow_offset_x: 3
        shadow_offset_y: 6
}
app Shadowed
"##;
    check_source(source).expect("shadow properties should typecheck");
    let generated = compile_to_c(source).expect("shadow properties should lower to GTK CSS");
    assert!(generated.contains("box-shadow: 3px 6px 18px #11223380;"));

    let defaults = r#"
view Shadowed {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: "Shadow"
        shadow_blur: 8
}
app Shadowed
"#;
    let generated = compile_to_c(defaults).expect("shadow color should have a safe default");
    assert!(generated.contains("box-shadow: 0px 0px 8px #00000080;"));

    let invalid = r#"
view Shadowed {
    grid columns: 1fr
    grid rows: auto
    Button action at 1,1
        text: "Bad"
        shadow_blur: -1
}
app Shadowed
"#;
    check_source(invalid).expect("shadow dimensions typecheck before native validation");
    let error = compile_to_c(invalid).expect_err("negative shadow blur must fail");
    assert!(error.message.contains("shadow_blur must be non-negative"));
}

#[test]
fn native_elements_support_static_and_state_driven_transforms() {
    let source = r#"
view Transformed {
    grid columns: 1fr
    grid rows: auto
    Button action at 1,1
        text: "Transform"
        translate_x: 12
        translate_y: -4
        rotate_degrees: 15
        scale_percent: 125
        scale_x_percent: 150
        scale_y_percent: 80
        skew_x_degrees: 5
        skew_y_degrees: -3
        transform_origin_x_percent: 0
        transform_origin_y_percent: 100
}
app Transformed
"#;
    check_source(source).expect("transform properties should typecheck as common i64 properties");
    let generated = compile_to_c(source).expect("transforms should lower through native GTK CSS");
    assert!(generated.contains(
        "transform: translate(12px, -4px) rotate(15deg) scale(1.50, 0.80) skewX(5deg) skewY(-3deg);"
    ));
    assert!(generated.contains("transform-origin: 0% 100%;"));

    let dynamic_transform = r#"
view Transformed {
    state moved: i64 = 0
    grid columns: 1fr
    grid rows: auto auto
    Text title at 1,1
        text: "Dynamic"
        translate_x: moved
        rotate_degrees: moved
        scale_percent: 100 + moved
        scale_y_percent: 100 - moved
        skew_x_degrees: moved
        transform_origin_x_percent: 50 + moved
        transition_ms: 180
        transition_delay_ms: 20
        transition_easing: "ease_out"
    Button toggle at 2,1
        text: "Move"
        on_press: moved => moved + 4
}
app Transformed
"#;
    check_source(dynamic_transform)
        .expect("state-derived transforms should reuse typed primitive UI expressions");
    let generated = compile_to_c(dynamic_transform)
        .expect("state-derived transforms should lower to a refreshable native CSS provider");
    assert!(generated.contains("static GtkCssProvider *flux__transform_style_title = NULL;"));
    assert!(generated.contains("g_strdup_printf(\"#flux-ui-title { transform:"));
    assert!(generated.contains("transform-origin: %lld%% %lld%%;"));
    assert!(generated.contains("flux__ui_state_moved"));
    assert!(generated.contains("gtk_css_provider_load_from_data(flux__transform_style_title"));
    assert!(generated.contains("transition-property: all;"));
    assert!(generated.contains("transition-duration: 180ms;"));
    assert!(generated.contains("transition-delay: 20ms;"));
    assert!(generated.contains("transition-timing-function: ease-out;"));
    assert!(generated.contains("@media (prefers-reduced-motion: reduce)"));
    assert!(generated.contains("\"gtk-interface-reduced-motion\""));
    assert!(generated.contains("\"prefers-reduced-motion\""));

    let invalid_transition = r#"
view Transformed {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: "Bad transition"
        transition_easing: "ease_out"
}
app Transformed
"#;
    check_source(invalid_transition).expect("transition options have compiler-owned types");
    let error = compile_to_c(invalid_transition)
        .expect_err("transition easing without a duration should fail clearly");
    assert!(error.message.contains("require transition_ms"));

    let oversized = r#"
view Transformed {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: "Too far"
        translate_x: 2147483648
}
app Transformed
"#;
    check_source(oversized).expect("transform dimensions are ordinary i64 properties");
    let error = compile_to_c(oversized)
        .expect_err("static native transform must fit GTK CSS integer range");
    assert!(
        error
            .message
            .contains("translate_x must fit within a 32-bit signed integer")
    );
}

#[test]
fn view_environment_tracks_window_geometry_orientation_and_scale() {
    let source = r#"
view Responsive {
    grid columns: 1fr
    grid rows: auto auto auto
    Text landscape at 1,1
        text: "Landscape"
        visible: window_is_landscape
    Text portrait at 2,1
        text: "Portrait"
        visible: window_is_portrait
    Button mode at 3,1
        text: "Responsive"
        enabled: window_width >= 700 && window_height >= 300 && display_scale >= 1
}
app Responsive(width: 720, height: 480)
"#;
    check_source(source)
        .expect("read-only view environment bindings should typecheck in UI properties");
    let generated =
        compile_to_c(source).expect("view environment should lower to native window state");
    assert!(generated.contains("static int64_t flux__ui_window_width = INT64_C(720);"));
    assert!(generated.contains("static int64_t flux__ui_window_height = INT64_C(480);"));
    assert!(generated.contains("static int64_t flux__ui_display_scale = INT64_C(1);"));
    assert!(generated.contains("gtk_window_get_default_size(GTK_WINDOW(object), &width, &height)"));
    assert!(generated.contains("gtk_widget_get_scale_factor(GTK_WIDGET(object))"));
    assert!(generated.contains("notify::default-width"));
    assert!(generated.contains("notify::default-height"));
    assert!(generated.contains("notify::scale-factor"));
    assert!(generated.contains("flux__ui_window_width > flux__ui_window_height"));
    assert!(generated.contains("flux__ui_window_height >= flux__ui_window_width"));
    assert!(generated.contains("flux__ui_window_width >= INT64_C(700)"));
    assert!(generated.contains("flux__ui_display_scale >= INT64_C(1)"));

    let collision = r#"
view Invalid {
    state window_width: i64 = 1
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: "Invalid"
}
app Invalid
"#;
    let errors = check_source_all(collision)
        .expect_err("view environment names must remain reserved and read-only");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("conflicts with a read-only view environment binding")
    }));
}

#[test]
fn native_elements_support_common_alignment_and_margins() {
    let source = r#"
view Layout {
    grid columns: 1fr
    grid rows: auto auto
    Text title at 1,1
        text: "Centered"
        align_x: "center"
        align_y: "end"
        margin: 8
        margin_start: 24
    Button action at 2,1
        text: "Action"
        align_x: "fill"
        margin_top: 12
}
app Layout
"#;
    check_source(source).expect("common alignment/margin properties should typecheck");
    let generated = compile_to_c(source).expect("alignment and margins should lower natively");
    assert!(generated.contains("gtk_widget_set_halign(flux__ui_title, GTK_ALIGN_CENTER)"));
    assert!(generated.contains("gtk_widget_set_valign(flux__ui_title, GTK_ALIGN_END)"));
    assert!(generated.contains("gtk_widget_set_margin_top(flux__ui_title, 8)"));
    assert!(generated.contains("gtk_widget_set_margin_start(flux__ui_title, 24)"));
    assert!(generated.contains("gtk_widget_set_margin_end(flux__ui_title, 8)"));
    assert!(generated.contains("gtk_widget_set_halign(flux__ui_action, GTK_ALIGN_FILL)"));
    assert!(generated.contains("gtk_widget_set_margin_top(flux__ui_action, 12)"));

    let bad_alignment = r#"
view Layout {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        align_x: "middle"
}
app Layout
"#;
    check_source(bad_alignment).expect("alignment enum validation happens in native lowering");
    let error = compile_to_c(bad_alignment).expect_err("unknown alignment must fail");
    assert!(error.message.contains("align_x must be one of"));

    let bad_margin = r#"
view Layout {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        margin: -1
}
app Layout
"#;
    check_source(bad_margin).expect("margin typechecks before native range validation");
    let error = compile_to_c(bad_margin).expect_err("negative margin must fail");
    assert!(error.message.contains("margin must be between 0"));
}

#[test]
fn native_elements_support_state_visibility_and_minimum_size_constraints() {
    let source = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto auto
    state expanded: bool = false
    Text title at 1,1
        text: "Details"
        visible: expanded
        min_width: 320
    Button action at 2,1
        text: "Toggle"
        min_height: 48
        on_press: expanded => !expanded
}
app Screen
"#;
    check_source(source).expect("common visibility/size properties should typecheck");
    let generated = compile_to_c(source).expect("common element layout properties should lower");
    assert!(generated.contains("gtk_widget_set_size_request(flux__ui_title, 320, -1)"));
    assert!(generated.contains("gtk_widget_set_size_request(flux__ui_action, -1, 48)"));
    assert!(generated.contains("gtk_widget_set_visible(flux__ui_title, flux__ui_state_expanded)"));

    let bad_size = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: "Bad"
        min_width: 0
}
app Screen
"#;
    check_source(bad_size)
        .expect("minimum size typechecking should succeed before backend validation");
    let error = compile_to_c(bad_size).expect_err("invalid native minimum size must fail");
    assert!(error.message.contains("min_width must be between 1"));
}

#[test]
fn image_control_lowers_file_source_alt_text_fit_and_state_refresh() {
    let source = r#"
view Gallery {
    grid columns: 1fr
    grid rows: auto auto
    state alternate: bool = false
    Image artwork at 1,1
        source: "default.png"
        alt: "Cover"
        fit: "cover"
        can_shrink: alternate
        min_width: 240
        min_height: 160
    Button swap at 2,1
        text: "Swap"
        on_press: alternate => !alternate
}
app Gallery
"#;
    check_source(source).expect("Image properties and state-derived source should typecheck");
    let generated = compile_to_c(source).expect("Image should lower to native GtkPicture");
    assert!(generated.contains("gtk_picture_new_for_filename"));
    assert!(generated.contains("gtk_picture_set_alternative_text"));
    assert!(generated.contains("GTK_CONTENT_FIT_COVER"));
    assert!(generated.contains("gtk_picture_set_can_shrink"));
    assert!(
        generated
            .contains("gtk_picture_set_filename(GTK_PICTURE(flux__ui_artwork), \"default.png\")")
    );
    assert!(generated.contains(
        "gtk_picture_set_can_shrink(GTK_PICTURE(flux__ui_artwork), flux__ui_state_alternate)"
    ));
    assert!(generated.contains("gtk_widget_set_size_request(flux__ui_artwork, 240, 160)"));

    let bad_fit = r#"
view Gallery {
    grid columns: 1fr
    grid rows: auto
    Image artwork at 1,1
        source: "cover.png"
        fit: "stretchy"
}
app Gallery
"#;
    check_source(bad_fit).expect("Image.fit typechecks before backend enum validation");
    let error = compile_to_c(bad_fit).expect_err("unknown Image.fit mode must fail");
    assert!(error.message.contains("Image.fit must be one of"));
}

#[test]
fn radio_controls_group_native_selection_and_update_shared_state_once() {
    let source = r#"
view Choice {
    grid columns: 1fr
    grid rows: auto auto
    state selected: i64 = 0
    Radio first at 1,1
        label: "First"
        selected: selected == 0
        on_select: selected => 0
    Radio second at 2,1
        label: "Second"
        selected: selected == 1
        on_select: selected => 1
}
app Choice
"#;
    check_source(source).expect("radio selection should typecheck through shared i64 state");
    let generated =
        compile_to_c(source).expect("radio controls should lower to grouped GTK buttons");
    assert!(generated.contains("gtk_check_button_new_with_label(\"First\")"));
    assert!(generated.contains("gtk_check_button_new_with_label(\"Second\")"));
    assert!(generated.contains("gtk_check_button_set_group(GTK_CHECK_BUTTON(flux__ui_second), GTK_CHECK_BUTTON(flux__ui_first))"));
    assert!(
        generated.contains("if (!gtk_check_button_get_active(GTK_CHECK_BUTTON(widget))) return;")
    );
    assert!(generated.contains("flux__ui_state_selected = INT64_C(1);"));
    assert!(generated.contains("gtk_check_button_set_active(GTK_CHECK_BUTTON(flux__ui_first), (flux__ui_state_selected == INT64_C(0)))"));
    assert!(generated.contains("gtk_check_button_set_active(GTK_CHECK_BUTTON(flux__ui_second), (flux__ui_state_selected == INT64_C(1)))"));
}

#[test]
fn text_input_lowers_native_entry_and_typed_submit_callback() {
    let source = r#"
fn submit(value: str) -> void {
    print(value)
}

view Form {
    grid columns: 1fr
    grid rows: auto
    TextInput query at 1,1
        text: "initial"
        placeholder: "Search Flux"
        enabled: true
        autofocus: true
        password: true
        max_length: 64
        tooltip: "Type a query"
        accessibility_label: "Search query"
        accessibility_description: "Enter text to search Flux"
        on_change: submit
        on_submit: submit
}
app Form
"#;
    check_source(source).expect("TextInput contract and submit callback should typecheck");
    let generated = compile_to_c(source).expect("TextInput should lower to native GTK entry");
    assert!(generated.contains("gtk_entry_new()"));
    assert_eq!(
        generated
            .matches("gtk_editable_set_text(GTK_EDITABLE(flux__ui_query), \"initial\")")
            .count(),
        1,
        "TextInput.text is an initial value until Flux owns editable string state; refresh must not clobber native user edits"
    );
    assert!(
        generated
            .contains("gtk_entry_set_placeholder_text(GTK_ENTRY(flux__ui_query), \"Search Flux\")")
    );
    assert!(generated.contains(
        "g_signal_connect(flux__ui_query, \"changed\", G_CALLBACK(flux__ui_change_query), NULL)"
    ));
    assert!(generated.contains(
        "g_signal_connect(flux__ui_query, \"activate\", G_CALLBACK(flux__ui_submit_query), NULL)"
    ));
    assert!(generated.contains("gtk_widget_grab_focus(flux__ui_query)"));
    assert!(generated.contains("gtk_entry_set_visibility(GTK_ENTRY(flux__ui_query), FALSE)"));
    assert!(generated.contains("gtk_entry_set_max_length(GTK_ENTRY(flux__ui_query), 64)"));
    assert!(generated.contains("gtk_widget_set_tooltip_text(flux__ui_query, \"Type a query\")"));
    assert!(generated.contains("GTK_ACCESSIBLE_PROPERTY_LABEL, \"Search query\", -1"));
    assert!(
        generated
            .contains("GTK_ACCESSIBLE_PROPERTY_DESCRIPTION, \"Enter text to search Flux\", -1")
    );
    assert!(generated.contains("gtk_editable_get_text(GTK_EDITABLE(widget))"));
    assert!(generated.contains("flux__fn_submit(gtk_editable_get_text"));

    let bad_callback = r#"
fn submit() -> void {}
view Form {
    grid columns: 1fr
    grid rows: auto
    TextInput query at 1,1
        on_submit: submit
}
app Form
"#;
    check_source(bad_callback).expect_err("TextInput.on_submit requires fn(str) -> void");

    let invalid_length = r#"
view Form {
    grid columns: 1fr
    grid rows: auto
    TextInput query at 1,1
        max_length: -1
}
app Form
"#;
    check_source(invalid_length).expect("max_length has the expected i64 type");
    let error =
        compile_to_c(invalid_length).expect_err("negative max_length must fail native lowering");
    assert!(
        error
            .message
            .contains("TextInput.max_length must be between 0")
    );
}

#[test]
fn button_shortcuts_validate_and_dispatch_through_existing_press_action() {
    let source = r#"
view Shortcuts {
    grid columns: 1fr
    grid rows: auto
    state count: i64 = 0
    Button action at 1,1
        text: "Add"
        shortcut: "Ctrl+Shift+Enter"
        on_press: count => count + 1
}
app Shortcuts
"#;
    check_source(source).expect("button shortcut should typecheck with an on_press action");
    let generated = compile_to_c(source).expect("button shortcut should lower natively");
    assert!(generated.contains("GTK_SHORTCUT_SCOPE_GLOBAL"));
    assert!(generated.contains("gtk_shortcut_trigger_parse_string(\"<Control><Shift>Return\")"));
    assert!(generated.contains("gtk_callback_action_new(flux__ui_shortcut_action, NULL, NULL)"));
    assert!(generated.contains("flux__ui_click_action(widget, NULL); return TRUE;"));

    let missing_action = r#"
view Shortcuts {
    grid columns: 1fr
    grid rows: auto
    Button action at 1,1
        shortcut: "Ctrl+K"
}
app Shortcuts
"#;
    let errors = check_source_all(missing_action).expect_err("shortcut without action must fail");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("Button.shortcut requires Button.onPress")
    }));

    let invalid = r#"
view Shortcuts {
    grid columns: 1fr
    grid rows: auto
    Button action at 1,1
        shortcut: "Command+K"
        on_press: action
}
fn action() -> void {
    print("action")
}
app Shortcuts
"#;
    check_source(invalid).expect("invalid shortcut syntax is a backend portability check");
    let error = compile_to_c(invalid).expect_err("unsupported shortcut syntax must fail");
    assert!(error.message.contains("modifiers Ctrl/Shift/Alt"));
}

#[test]
fn native_elements_support_hover_leave_callbacks_and_state_transitions() {
    let source = r#"
fn leave_notice() -> void {
    print("left")
}

view HoverCard {
    grid columns: 1fr
    grid rows: auto auto
    state hovered: bool = false
    Text title at 1,1
        text: "Hover state"
        tooltip: "Hover me"
        accessibility_label: "Hover state title"
        visible: hovered
        on_hover: hovered => true
        on_leave: hovered => false
    Button action at 2,1
        text: "Action"
        on_focus: hovered => true
        on_blur: leave_notice
        on_leave: leave_notice
}
app HoverCard
"#;

    check_source(source).expect("hover/leave callbacks and transitions should typecheck");
    let generated = compile_to_c(source).expect("hover/leave events should lower natively");
    assert!(generated.contains("GtkEventControllerMotion *controller"));
    assert!(generated.contains("flux__ui_state_hovered = true; flux__ui_refresh();"));
    assert!(generated.contains("flux__ui_state_hovered = false; flux__ui_refresh();"));
    assert!(generated.contains("flux__fn_leave_notice(); flux__ui_refresh();"));
    assert!(generated.contains("gtk_widget_set_tooltip_text(flux__ui_title, \"Hover me\")"));
    assert!(generated.contains("GTK_ACCESSIBLE_PROPERTY_LABEL, \"Hover state title\", -1"));
    assert!(generated.contains("gtk_widget_set_visible(flux__ui_title, flux__ui_state_hovered)"));
    assert!(generated.contains("gtk_event_controller_motion_new()"));
    assert!(generated.contains("gtk_event_controller_focus_new()"));
    assert!(generated.contains("\"enter\", G_CALLBACK(flux__ui_focus_action)"));
    assert!(generated.contains("\"leave\", G_CALLBACK(flux__ui_blur_action)"));
    assert!(generated.contains("\"enter\", G_CALLBACK(flux__ui_hover_title)"));
    assert!(generated.contains("\"leave\", G_CALLBACK(flux__ui_leave_title)"));
    assert!(generated.contains("gtk_widget_add_controller(flux__ui_title, flux__motion_title)"));
}

#[test]
fn toggle_control_binds_native_checked_state_and_functional_transition() {
    let source = r#"
view Settings {
    grid columns: 1fr
    grid rows: auto
    state enabled: bool = false
    Toggle enabled_toggle at 1,1
        label: "Enabled"
        checked: enabled
        on_change: enabled => !enabled
}
app Settings
"#;
    check_source(source).expect("toggle state transition should typecheck");
    let generated = compile_to_c(source).expect("toggle should lower to native GTK4 control");
    assert!(generated.contains("gtk_check_button_new_with_label(\"Enabled\")"));
    assert!(generated.contains("gtk_check_button_set_active"));
    assert!(generated.contains("\"toggled\""));
    assert!(generated.contains("flux__ui_state_enabled = (!(flux__ui_state_enabled));"));
    assert!(generated.contains("gtk_check_button_set_active(GTK_CHECK_BUTTON(flux__ui_enabled_toggle), flux__ui_state_enabled)"));
}

#[test]
fn i64_view_state_supports_arithmetic_transitions_and_derived_properties() {
    let source = r#"
view Counter {
    grid columns: 1fr
    grid rows: auto auto
    state count: i64 = 0
    Text title at 1,1
        text: "Many"
        visible: count >= 2
    Button action at 2,1
        text: "Add"
        enabled: count < 3
        on_press: count => count + 1
}
app Counter
"#;
    check_source(source).expect("i64 state and primitive derived expressions should typecheck");
    let generated = compile_to_c(source).expect("i64 state should lower natively");
    assert!(generated.contains("static int64_t flux__ui_state_count = INT64_C(0);"));
    assert!(
        generated
            .contains("flux__ui_state_count = flux_add_i64(flux__ui_state_count, INT64_C(1));")
    );
    assert!(generated.contains("flux__ui_state_count >= INT64_C(2)"));
    assert!(generated.contains("flux__ui_state_count < INT64_C(3)"));
    assert!(generated.contains("gtk_label_set_text"));
    assert!(generated.contains("gtk_widget_set_sensitive"));
}

#[test]
fn derived_view_values_typecheck_format_index_and_lower_natively() {
    let source = r#"
const LABEL: str = "Ready"
view Counter {
    grid columns: 1fr
    grid rows: auto auto
    state count: i64 = 1
    derived doubled: i64 = count * 2
    derived wide: bool = windowWidth >= 600
    derived ready: bool = doubled >= 2 && wide
    derived label: str = LABEL
    Text title at 1,1
        text: label
        visible: ready
    Button action at 2,1
        text: "Add"
        enabled: doubled < 10
        on_press: count => count + 1
}
app Counter(width: 640)
"#;

    check_source(source).expect("derived view values should typecheck");
    let formatted = fluxc::formatter::format_source(source).expect("derived values should format");
    assert!(formatted.contains("derived doubled: i64 = count * 2"));
    assert!(formatted.contains("derived wide: bool = windowWidth >= 600"));
    assert!(formatted.contains("derived ready: bool = doubled >= 2 && wide"));
    assert!(formatted.contains("derived label: str = LABEL"));

    let database = fluxc::semantic::SemanticDatabase::analyze(&formatted, SourceId::new(740))
        .expect("derived values should be indexed semantically");
    let doubled = database
        .symbols_named("doubled")
        .find(|symbol| symbol.kind == fluxc::semantic::SymbolKind::ViewDerived)
        .expect("derived value should have a semantic symbol");
    assert_eq!(doubled.ty, Some(fluxc::ast::Type::I64));

    let generated = compile_to_c(source).expect("derived values should lower natively");
    assert!(generated.contains("static int64_t flux__ui_derived_doubled = INT64_C(0);"));
    assert!(generated.contains("static bool flux__ui_derived_wide = false;"));
    assert!(generated.contains("static bool flux__ui_derived_ready = false;"));
    assert!(generated.contains("static const char * flux__ui_derived_label = NULL;"));
    assert!(
        generated
            .contains("flux__ui_derived_doubled = flux_mul_i64(flux__ui_state_count, INT64_C(2));")
    );
    assert!(generated.contains("flux__ui_derived_wide = (flux__ui_window_width >= INT64_C(600));"));
    assert!(generated.contains("flux__ui_derived_doubled >= INT64_C(2)"));
    assert!(generated.contains("flux__ui_derived_label = \"Ready\";"));
    assert!(
        generated.contains("gtk_label_set_text(GTK_LABEL(flux__ui_title), flux__ui_derived_label)")
    );
    assert!(generated.contains("gtk_widget_set_visible(flux__ui_title, flux__ui_derived_ready)"));
    assert!(generated.contains("flux__ui_derived_doubled < INT64_C(10)"));
}

#[test]
fn rejects_invalid_derived_view_values() {
    let forward_reference = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
    derived first: i64 = second + 1
    derived second: i64 = 2
    Text title at 1,1
        text: "Flux"
}
app Screen
"#;
    let errors = check_source_all(forward_reference)
        .expect_err("derived values should depend only on earlier derived values");
    assert!(errors.iter().any(|error| {
        error.message.contains("derived view value 'first'")
            && error
                .message
                .contains("unknown binding, constant, or function 'second'")
    }));

    let unsupported_type = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
    derived problem: error = nil
    Text title at 1,1
        text: "Flux"
}
app Screen
"#;
    let errors = check_source_all(unsupported_type)
        .expect_err("derived values should remain primitive during bootstrap");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("derived view values currently support i64, bool, and str")
    }));

    let transition_target = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
    state count: i64 = 1
    derived doubled: i64 = count * 2
    Button action at 1,1
        text: "Bad"
        on_press: doubled => doubled + 1
}
app Screen
"#;
    let errors = check_source_all(transition_target)
        .expect_err("derived values must not be mutable transition targets");
    assert!(
        errors
            .iter()
            .any(|error| { error.message.contains("unknown view state 'doubled'") })
    );
}

#[test]
fn rejects_invalid_view_state_transitions() {
    let unknown = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
    Button action at 1,1
        on_press: missing => true
}
app Screen
"#;
    let errors = check_source_all(unknown).expect_err("unknown state transition must fail");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("unknown view state 'missing'"))
    );

    let wrong_type = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
    state clicked: bool = false
    Button action at 1,1
        on_press: clicked => 42
}
app Screen
"#;
    let errors = check_source_all(wrong_type).expect_err("wrong next-state type must fail");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("next value for view state 'clicked'")
            && error.message.contains("expected bool, got i64")
    }));

    let wrong_property = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
    state clicked: bool = false
    Text title at 1,1
        text: clicked => !clicked
}
app Screen
"#;
    let errors =
        check_source_all(wrong_property).expect_err("transition syntax outside on_press must fail");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("unexpected character '='"))
    );
}

#[test]
fn application_metadata_types_formats_and_lowers_native_window_properties() {
    let source = r#"
const WINDOW_WIDTH: i64 = 400 + 20
view Screen {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: "Flux"
        size: 7 * 4
        visible: 10 > 3 && true
}
app Screen(title: "Flux App", width: WINDOW_WIDTH, height: 120 * 2)
"#;
    check_source(source).expect("typed compile-time app metadata should typecheck");
    let formatted = fluxc::formatter::format_source(source).expect("app metadata should format");
    assert!(
        formatted.contains("app Screen(title: \"Flux App\", width: WINDOW_WIDTH, height: 120 * 2)")
    );
    let generated = compile_to_c(source).expect("app metadata should lower natively");
    assert!(generated.contains("gtk_window_set_title(GTK_WINDOW(window), \"Flux App\")"));
    assert!(generated.contains("gtk_window_set_default_size(GTK_WINDOW(window), 420, 240)"));
    assert!(generated.contains("pango_attr_size_new(28 * PANGO_SCALE)"));
    assert!(generated.contains("gtk_widget_set_visible(flux__ui_title, true)"));

    let wrong_title = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
}
app Screen(title: 42)
"#;
    let errors = check_source_all(wrong_title).expect_err("non-string app title should fail");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("application title must be str"))
    );

    let invalid_size = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
}
app Screen(width: 0)
"#;
    let errors = check_source_all(invalid_size).expect_err("non-positive window size should fail");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("application width must be greater than zero")
    }));
}

#[test]
fn application_identity_and_resizable_metadata_validate_and_lower() {
    let source = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
}
app Screen(id: "app.example.screen", resizable: false)
"#;
    check_source(source).expect("valid application identity metadata should typecheck");
    let generated = compile_to_c(source).expect("application identity should lower natively");
    assert!(
        generated
            .contains("gtk_application_new(\"app.example.screen\", G_APPLICATION_DEFAULT_FLAGS)")
    );
    assert!(generated.contains("gtk_window_set_resizable(GTK_WINDOW(window), FALSE)"));

    let invalid_id = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
}
app Screen(id: "bad")
"#;
    let errors = check_source_all(invalid_id).expect_err("short non-DNS app id must fail");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("reverse-DNS-style identifier"))
    );

    let invalid_resizable = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
}
app Screen(resizable: 1)
"#;
    let errors = check_source_all(invalid_resizable).expect_err("resizable must be bool");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("application resizable must be bool"))
    );
}

#[test]
fn application_theme_metadata_validates_and_lowers() {
    let dark = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
}
app Screen(theme: "dark")
"#;
    check_source(dark).expect("dark application theme should typecheck");
    let generated = compile_to_c(dark).expect("dark application theme should lower natively");
    assert!(generated.contains("gtk-application-prefer-dark-theme"));
    assert!(generated.contains("TRUE, NULL"));

    let system = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
}
app Screen(theme: "system")
"#;
    let generated = compile_to_c(system).expect("system theme should preserve platform choice");
    assert!(!generated.contains("gtk-application-prefer-dark-theme"));

    let invalid = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
}
app Screen(theme: "sepia")
"#;
    let errors = check_source_all(invalid).expect_err("unknown application theme must fail");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("application theme must be one of 'system', 'light', or 'dark'")
    }));
}

#[test]
fn application_lifecycle_metadata_uses_typed_free_function_callbacks() {
    let source = r#"
fn started() -> void {
    print("started")
}
fn exiting() -> void {
    print("exiting")
}
view Screen {
    grid columns: 1fr
    grid rows: auto
}
app Screen(on_start: started, on_exit: exiting)
"#;
    check_source(source)
        .expect("lifecycle callbacks should typecheck as named fn() -> void values");
    let generated = compile_to_c(source).expect("lifecycle callbacks should lower natively");
    assert!(generated.contains("flux__fn_started();"));
    assert!(generated.contains("static void flux__ui_shutdown"));
    assert!(generated.contains("flux__fn_exiting();"));
    assert!(generated.contains("\"shutdown\", G_CALLBACK(flux__ui_shutdown)"));

    let wrong = r#"
fn bad(value: i64) -> void {
}
view Screen {
    grid columns: 1fr
    grid rows: auto
}
app Screen(on_start: bad)
"#;
    let errors = check_source_all(wrong).expect_err("wrong lifecycle signature should fail");
    assert!(errors.iter().any(|error| {
        error.message.contains("application on_start callback")
            && error.message.contains("expected fn() -> void")
    }));
}

#[test]
fn app_entry_rejects_unknown_parameterized_or_competing_main_roots() {
    let unknown = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
}
app Missing
"#;
    let errors = check_source_all(unknown).expect_err("unknown app root should fail");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("unknown app root view 'Missing'"))
    );

    let parameterized = r#"
view Screen(title: str) {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: title
}
app Screen
"#;
    let errors =
        check_source_all(parameterized).expect_err("bootstrap app root parameters should fail");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("app root view must not declare parameters")
    }));

    let competing = r#"
view Screen {
    grid columns: 1fr
    grid rows: auto
}
app Screen
fn main() -> i64 { 0 }
"#;
    let errors =
        check_source_all(competing).expect_err("app and main should not both define process entry");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("app declaration replaces fn main"))
    );
}

#[test]
fn validates_flat_grid_bounds_and_rejects_accidental_overlap() {
    let valid = r#"
view Dashboard {
    grid columns: 1fr 1fr 1fr
    grid rows: 1fr 1fr
    Text title at 1,1 span columns 2
    Button action at 1,3 span rows 2
    Chart chart at 2,1 span columns 2
}
fn main() -> i64 { 0 }
"#;
    check_source(valid).expect("touching grid regions that do not overlap should typecheck");

    let out_of_bounds = r#"
view BadBounds {
    grid columns: 1fr 1fr
    grid rows: 1fr 1fr
    Text title at 2,2 span rows 2 span columns 2
}
fn main() -> i64 { 0 }
"#;
    let diagnostics =
        check_source_all(out_of_bounds).expect_err("spans beyond declared tracks must fail");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("occupies grid row 3, but view 'BadBounds' declares only 2 rows")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("occupies grid column 3, but view 'BadBounds' declares only 2 columns")
    }));

    let overlap = r#"
view BadOverlap {
    grid columns: 1fr 1fr 1fr
    grid rows: 1fr 1fr
    Card summary at 1,1 span columns 2 span rows 2
    Text title at 2,2
}
fn main() -> i64 { 0 }
"#;
    let diagnostics = check_source_all(overlap)
        .expect_err("overlapping grid siblings must be explicit, not accidental");
    let overlap_error = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("overlaps sibling 'summary'"))
        .expect("overlap diagnostic should identify the earlier sibling");
    assert!(
        overlap_error
            .notes
            .iter()
            .any(|note| note.contains("explicit overlay/absolute positioning"))
    );
    assert_eq!(overlap_error.labels.len(), 1);
}

#[test]
fn typechecks_bootstrap_view_element_property_contracts() {
    let valid = r#"
fn handle_press() -> void {
    print("pressed")
}

view App {
    grid columns: 1fr 1fr
    grid rows: auto 1fr
    Text heading at 1,1
        text: "Hello"
        selectable: true
    Button action at 1,2
        text: "Press"
        enabled: true
        on_press: handle_press
    Chart chart at 2,1
        label: "Activity"
    Card summary at 2,2
        title: "Summary"
}

fn main() -> i64 { 0 }
"#;
    check_source(valid).expect("known view properties with matching types should typecheck");
    let database =
        fluxc::semantic::SemanticDatabase::analyze(valid, SourceId::from_name("typed-view.flux"))
            .expect("typed view properties should be available to editor semantics");
    let callback = database
        .symbols()
        .iter()
        .find(|symbol| symbol.name == "on_press")
        .expect("callback property should be indexed");
    assert_eq!(
        callback.ty,
        Some(fluxc::ast::Type::Function {
            params: vec![],
            returns: vec![]
        })
    );

    let invalid = r#"
view BadProperties {
    grid columns: 1fr 1fr
    grid rows: 1fr 1fr
    Text heading at 1,1
        text: false
    Button action at 1,2
        on_press: 42
    Text typo at 2,1
        texxt: "misspelled"
    Fancy custom at 2,2
}
fn main() -> i64 { 0 }
"#;
    let diagnostics =
        check_source_all(invalid).expect_err("invalid built-in view property contracts must fail");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("property 'Text.text'")
            && diagnostic.message.contains("expected str, got bool")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("property 'Button.on_press'")
            && diagnostic
                .message
                .contains("expected fn() -> void, got i64")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("view element type 'Text' has no property 'texxt'")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("unknown view element type 'Fancy'")
    }));
}

#[test]
fn composes_parameterized_views_through_typed_data_properties() {
    let source = r#"
view Greeting(name: str, *, selectable: bool = false) {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: name
        selectable: selectable
}

view App {
    grid columns: 1fr
    grid rows: 1fr
    Greeting greeting at 1,1
        name: "Flux"
        selectable: true
}

fn main() -> i64 { 0 }
"#;
    check_source(source).expect("parameterized views should compose through typed properties");
    let program = fluxc::parser::parse(source).expect("parameterized view syntax should parse");
    assert_eq!(program.views[0].params.len(), 2);
    assert_eq!(program.views[0].params[0].name, "name");
    assert_eq!(program.views[0].params[1].default.as_ref().unwrap().line, 2);

    let formatted =
        fluxc::formatter::format_source(source).expect("parameterized views should format");
    assert!(formatted.contains("view Greeting(name: str, *, selectable: bool = false) {"));

    let database = fluxc::semantic::SemanticDatabase::analyze(
        source,
        SourceId::from_name("composed-view.flux"),
    )
    .expect("composed views should be indexed semantically");
    assert!(database.symbols().iter().any(|symbol| {
        symbol.name == "name"
            && symbol.kind == fluxc::semantic::SymbolKind::ViewProperty
            && symbol.ty == Some(fluxc::ast::Type::Str)
    }));
}

#[test]
fn rejects_invalid_or_cyclic_parameterized_view_composition() {
    let invalid = r#"
view Greeting(name: str, enabled: bool = true) {
    grid columns: 1fr
    grid rows: 1fr
    Text title at 1,1
        text: name
}

view App {
    grid columns: 1fr 1fr
    grid rows: 1fr
    Greeting missing at 1,1
        enabled: false
    Greeting wrong at 1,2
        name: 42
        extra: "nope"
}
fn main() -> i64 { 0 }
"#;
    let diagnostics =
        check_source_all(invalid).expect_err("invalid custom view properties should fail");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("missing required parameter 'name' for 'Greeting'")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("property 'Greeting.name'")
            && diagnostic.message.contains("expected str, got i64")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("view 'Greeting' has no parameter 'extra'")
    }));

    let cycle = r#"
view A {
    grid columns: 1fr
    grid rows: 1fr
    B b at 1,1
}
view B {
    grid columns: 1fr
    grid rows: 1fr
    A a at 1,1
}
fn main() -> i64 { 0 }
"#;
    let diagnostics = check_source_all(cycle).expect_err("recursive view composition must fail");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("cyclic view composition")
            && diagnostic
                .notes
                .iter()
                .any(|note| note.contains("A -> B -> A") || note.contains("B -> A -> B"))
    }));
}

#[test]
fn view_composition_respects_module_visibility() {
    let root = std::env::temp_dir().join(format!("flux-view-visibility-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("temporary view project should be writable");
    fs::write(
        root.join("components.flux"),
        r#"
view PrivateGreeting(name: str) {
    grid columns: 1fr
    grid rows: 1fr
    Text title at 1,1
        text: name
}

pub view PublicGreeting(name: str) {
    grid columns: 1fr
    grid rows: 1fr
    Text title at 1,1
        text: name
}
"#,
    )
    .expect("view component module should be writable");

    let good = root.join("good.flux");
    fs::write(
        &good,
        r#"
import "components.flux"
view App {
    grid columns: 1fr
    grid rows: 1fr
    PublicGreeting greeting at 1,1
        name: "Flux"
}
fn main() -> i64 { 0 }
"#,
    )
    .expect("public view entry should be writable");
    fluxc::project::check(&good).expect("public views should compose across modules");

    let bad = root.join("bad.flux");
    fs::write(
        &bad,
        r#"
import "components.flux"
view App {
    grid columns: 1fr
    grid rows: 1fr
    PrivateGreeting greeting at 1,1
        name: "Flux"
}
fn main() -> i64 { 0 }
"#,
    )
    .expect("private view entry should be writable");
    let diagnostics =
        fluxc::project::check(&bad).expect_err("private views must remain module-local");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("private view 'PrivateGreeting' is not accessible from this module")
    }));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn formatter_preserves_flat_grid_view_structure() {
    let source = "view Dashboard {\n    grid columns: 240 1fr auto\n    grid rows: 64 1fr\n    grid gap: 16\n    Text title at 1,2 span columns 2\n        text: \"Hello\"\n}\n";
    let formatted = fluxc::formatter::format_source(source).expect("view should format");
    assert_eq!(formatted, source);
}

#[test]
fn rejects_nested_or_invalid_flat_grid_view_syntax() {
    let nested = r#"
view Bad {
    grid columns: 1fr
    grid rows: 1fr
    Text outer at 1,1
        text: "ok"
            Button nested at 1,1
}
"#;
    let errors = fluxc::parser::parse_all(nested).expect_err("nested elements must be rejected");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("exactly eight spaces"))
    );

    let invalid_track = r#"
view Bad {
    grid columns: 0fr
    grid rows: 1fr
}
"#;
    let errors = fluxc::parser::parse_all(invalid_track).expect_err("zero fractions must fail");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("greater than zero"))
    );
}

#[test]
fn public_symbols_require_an_actual_import_path_between_modules() {
    let root =
        std::env::temp_dir().join(format!("flux-import-reachability-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("temporary import project should be writable");
    let b = root.join("b.flux");
    let a = root.join("a.flux");
    let main = root.join("main.flux");
    fs::write(&b, "pub fn from_b() -> i64 { 42 }\n").expect("module b should be writable");
    fs::write(&a, "pub fn from_a() -> i64 { from_b() }\n").expect("module a should be writable");
    fs::write(
        &main,
        "import \"a.flux\"\nimport \"b.flux\"\nfn main() -> i64 { from_a() }\n",
    )
    .expect("entry should be writable");

    let errors = fluxc::project::check(&main)
        .expect_err("sibling modules must not gain visibility through the entry module");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("function 'from_b' is not imported into this module")
    }));

    fs::write(
        &a,
        "import \"b.flux\"\npub fn from_a() -> i64 { from_b() }\n",
    )
    .expect("module a import should be writable");
    fluxc::project::check(&main)
        .expect("a direct import should make the public dependency reachable");

    fs::write(&main, "import \"a.flux\"\nfn main() -> i64 { from_b() }\n")
        .expect("transitive entry should be writable");
    fluxc::project::check(&main)
        .expect("public declarations should remain reachable through transitive imports");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn project_imports_report_cycles_and_invalid_paths_at_import_sites() {
    let root = std::env::temp_dir().join(format!("flux-project-errors-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("temporary project directory should be writable");

    let a = root.join("a.flux");
    let b = root.join("b.flux");
    fs::write(&a, "import \"b.flux\"\nfn main() -> i64 { 0 }\n")
        .expect("cycle entry should be writable");
    fs::write(&b, "import \"a.flux\"\nfn helper() -> i64 { 1 }\n")
        .expect("cycle dependency should be writable");
    let errors = fluxc::project::check(&a).expect_err("cyclic import should fail");
    let cycle = errors
        .iter()
        .find(|error| error.message.contains("cyclic import"))
        .expect("cycle diagnostic should be present");
    assert_ne!(cycle.span.unwrap().source_id, SourceId::UNKNOWN);
    assert!(
        cycle
            .notes
            .iter()
            .any(|note| note.contains("import cycle:"))
    );

    let missing = root.join("missing_entry.flux");
    fs::write(
        &missing,
        "import \"does-not-exist.flux\"\nfn main() -> i64 { 0 }\n",
    )
    .expect("missing-import entry should be writable");
    let errors = fluxc::project::check(&missing).expect_err("missing import should fail");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("failed to read imported source"))
    );

    let bad_dependency = root.join("bad_dependency.flux");
    fs::write(&bad_dependency, "fn broken() -> i64 { \"wrong\" }\n")
        .expect("bad dependency should be writable");
    let bad_entry = root.join("bad_entry.flux");
    fs::write(
        &bad_entry,
        "import \"bad_dependency.flux\"\nfn main() -> i64 { broken() }\n",
    )
    .expect("bad-dependency entry should be writable");
    let errors = fluxc::project::check(&bad_entry).expect_err("imported type error should fail");
    let imported_error = errors
        .iter()
        .find(|error| error.message.contains("expected i64, got str"))
        .expect("imported type diagnostic should be present");
    let expected_source = SourceId::from_name(
        fs::canonicalize(&bad_dependency)
            .unwrap()
            .to_string_lossy()
            .as_ref(),
    );
    assert_eq!(imported_error.span.unwrap().source_id, expected_source);

    let parse_dependency = root.join("parse_dependency.flux");
    fs::write(&parse_dependency, "fn broken() -> i64\n    return 1\n}\n")
        .expect("parse-broken dependency should be writable");
    let parse_entry = root.join("parse_entry.flux");
    fs::write(
        &parse_entry,
        "import \"parse_dependency.flux\"\nfn main() -> i64 { 0 }\n",
    )
    .expect("parse-error entry should be writable");
    let (context_errors, context_sources) = fluxc::project::check_with_sources(&parse_entry);
    assert!(
        context_errors
            .iter()
            .any(|error| error.message.contains("open their body with '{'"))
    );
    let parse_dependency = fs::canonicalize(&parse_dependency).unwrap();
    let context_source = context_sources
        .iter()
        .find(|source| source.path == parse_dependency)
        .expect("parse-failing imported source must remain available for diagnostics");
    assert!(context_source.text.contains("fn broken() -> i64"));

    let invalid_extension = root.join("invalid_extension.flux");
    fs::write(
        &invalid_extension,
        "import \"module.txt\"\nfn main() -> i64 { 0 }\n",
    )
    .expect("invalid-extension entry should be writable");
    let errors =
        fluxc::project::check(&invalid_extension).expect_err("non-Flux import should fail");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("must end in '.flux'"))
    );

    let absolute = root.join("absolute.flux");
    let dependency = root.join("dep.flux");
    fs::write(&dependency, "fn helper() -> i64 { 1 }\n").expect("dependency should be writable");
    fs::write(
        &absolute,
        format!(
            "import {:?}\nfn main() -> i64 {{ 0 }}\n",
            dependency.to_string_lossy()
        ),
    )
    .expect("absolute-import entry should be writable");
    let errors = fluxc::project::check(&absolute).expect_err("absolute import should fail");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("must use relative paths"))
    );

    let normalized = root.join("normalized.flux");
    fs::write(
        &normalized,
        "import \"./dep.flux\"\nfn main() -> i64 { 0 }\n",
    )
    .expect("non-normalized import entry should be writable");
    let errors = fluxc::project::check(&normalized).expect_err("dot-segment import should fail");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("must be normalized and cannot contain '.' or '..' segments")
    }));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn module_visibility_is_private_by_default_and_pub_exports_cross_file_api() {
    let root = std::env::temp_dir().join(format!("flux-project-visibility-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("temporary visibility project should be writable");
    let api = root.join("api.flux");
    fs::write(
        &api,
        r#"
const hidden_value: i64 = 3
pub const visible_value: i64 = 7

type HiddenId = i64
pub type VisibleId = i64

struct HiddenBox {
    value: i64
}

pub struct VisibleBox {
    value: i64
}

enum HiddenChoice {
    Yes
}

pub enum VisibleChoice {
    Yes
}

interface HiddenCapability {
    fn value() -> i64
}

pub interface VisibleCapability {
    fn value() -> i64
}

fn hidden_helper(value: i64) -> i64 { value + hidden_value }
pub fn visible_helper(value: i64) -> i64 { hidden_helper(value) + visible_value }
"#,
    )
    .expect("API module should be writable");

    let good = root.join("good.flux");
    fs::write(
        &good,
        r#"
import "api.flux"
fn main() -> i64 {
    let id: VisibleId = visible_helper(1)
    let box: VisibleBox = VisibleBox { value: id }
    let _choice: VisibleChoice = VisibleChoice.Yes()
    print(box.value)
    print(visible_value)
    return 0
}
"#,
    )
    .expect("public API entry should be writable");
    fluxc::project::check(&good).expect("pub declarations should cross module boundaries");

    let private_function = root.join("private_function.flux");
    fs::write(
        &private_function,
        "import \"api.flux\"\nfn main() -> i64 { hidden_helper(1) }\n",
    )
    .expect("private-function entry should be writable");
    let errors = fluxc::project::check(&private_function)
        .expect_err("private function must not cross module boundary");
    assert!(
        errors.iter().any(|error| {
            error
                .message
                .contains("private function 'hidden_helper' is not accessible from this module")
        }),
        "unexpected diagnostics: {errors:#?}"
    );

    let private_constant = root.join("private_constant.flux");
    fs::write(
        &private_constant,
        "import \"api.flux\"\nfn main() -> i64 { hidden_value }\n",
    )
    .expect("private-constant entry should be writable");
    let errors = fluxc::project::check(&private_constant)
        .expect_err("private constant must not cross module boundary");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("private constant 'hidden_value' is not accessible from this module")
    }));

    let private_types = root.join("private_types.flux");
    fs::write(
        &private_types,
        r#"
import "api.flux"
fn main() -> i64 {
    let id: HiddenId = 1
    let box: HiddenBox = HiddenBox { value: id }
    let choice: HiddenChoice = HiddenChoice.Yes()
    return 0
}
"#,
    )
    .expect("private-types entry should be writable");
    let errors = fluxc::project::check(&private_types)
        .expect_err("private named types must not cross module boundary");
    let messages = errors
        .iter()
        .map(|error| error.message.as_str())
        .collect::<Vec<_>>();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("private type alias 'HiddenId'"))
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("private struct 'HiddenBox'"))
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("private enum 'HiddenChoice'"))
    );

    let private_interface = root.join("private_interface.flux");
    fs::write(
        &private_interface,
        "import \"api.flux\"\nfn consume(value: HiddenCapability) -> i64 { 0 }\nfn main() -> i64 { 0 }\n",
    )
    .expect("private-interface entry should be writable");
    let errors = fluxc::project::check(&private_interface)
        .expect_err("private interface must not cross module boundary");
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("private interface 'HiddenCapability' is not accessible from this module")
    }));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn public_apis_cannot_expose_private_named_types() {
    let source = r#"
struct Secret {
    value: i64
}

enum HiddenChoice {
    Yes
}

interface HiddenCapability {
    fn value() -> i64
}

pub type LeakedAlias = Secret

pub struct Wrapper {
    secret: Secret
}

pub enum Event {
    SecretValue(Secret)
}

pub interface PublicCapability {
    fn secret() -> Secret
}

pub interface InvalidComposition: HiddenCapability {
}

pub fn leak() -> Secret { Secret { value: 1 } }
pub fn consume(value: HiddenChoice) -> i64 { 0 }

fn main() -> i64 { 0 }
"#;
    let errors = check_source_all(source).expect_err("public APIs must not expose private types");
    let messages = errors
        .iter()
        .map(|error| error.message.as_str())
        .collect::<Vec<_>>();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("public API cannot expose private struct 'Secret'"))
    );
    assert!(messages.iter().any(|message| message.contains("public API cannot expose private enum 'HiddenChoice'")));
    assert!(
        messages
            .iter()
            .any(|message| message.contains("cannot compose private interface 'HiddenCapability'"))
    );
}

#[test]
fn formatter_preserves_pub_visibility_and_precise_name_spans() {
    let source = "pub type UserId=i64\npub const LIMIT:i64=3\npub struct Box {\n value:i64\n}\npub enum Choice {\n Yes\n}\npub interface Named {\n fn name()->str\n}\npub fn identity(value:i64)->i64 { value }\nfn main()->i64 { 0 }\n";
    let formatted =
        fluxc::formatter::format_source(source).expect("pub declarations should format");
    assert!(formatted.contains("pub type UserId = i64"));
    assert!(formatted.contains("pub const LIMIT: i64 = 3"));
    assert!(formatted.contains("pub struct Box {"));
    assert!(formatted.contains("pub enum Choice {"));
    assert!(formatted.contains("pub interface Named {"));
    assert!(formatted.contains("pub fn identity(value: i64) -> i64 { value }"));

    let program = fluxc::parser::parse_all_with_source(&formatted, SourceId::new(901))
        .expect("formatted pub source should parse");
    let identity = program
        .functions
        .iter()
        .find(|function| function.name == "identity")
        .expect("identity should exist");
    assert!(identity.public);
    assert_eq!(span_text(&formatted, identity.name_span), "identity");
    assert_eq!(identity.name_span.source_id, SourceId::new(901));
}

#[test]
fn rejects_pub_on_imports_and_implementations() {
    let import = "pub import \"dep.flux\"\nfn main() -> i64 { 0 }\n";
    let error = fluxc::parser::parse(import).expect_err("pub import should fail");
    assert!(error.message.contains("imports cannot be declared pub"));

    let implementation = r#"
interface Capability {
    fn value() -> i64
}
struct Data {
    value: i64
}
pub impl Capability for Data {
    value: data_value
}
fn data_value(data: Data) -> i64 { data.value }
fn main() -> i64 { 0 }
"#;
    let error = fluxc::parser::parse(implementation).expect_err("pub impl should fail");
    assert!(
        error
            .message
            .contains("interface implementations cannot be declared pub")
    );
}

#[test]
fn formatter_preserves_canonical_import_declarations() {
    let source = "import   \"utils.flux\"\nfn main()->i64 { 0 }\n";
    let formatted = fluxc::formatter::format_source(source).expect("imports should format");
    assert_eq!(formatted, "import \"utils.flux\"\nfn main() -> i64 { 0 }\n");
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
    assert!(generated.contains("if (flux__local_value < INT64_C(0))"));
    assert!(generated.contains("if (flux__local_value == INT64_C(0))"));
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
fn accepts_inferred_multi_value_pattern_destructuring() {
    let source = r#"
fn divide(value: i64, by: i64) -> (i64, bool) {
    return value / by, true
}

fn main() -> i64 {
    let (result, _) = divide(84, 2)
    let (shellResult, _) = divide 84 2
    let (pipedResult, _) = 84 | divide 2
    print(result)
    print(shellResult)
    return pipedResult
}
"#;

    check_source(source).expect("inferred multi-value pattern should typecheck");
    let generated = compile_to_c(source).expect("inferred multi-value pattern should compile");
    assert!(generated.contains("flux__multi_pattern_"));
    assert!(generated.contains("flux__local_result"));
    assert!(generated.contains("flux__local_shellResult"));
    assert!(generated.contains("flux__local_pipedResult"));
    assert!(!generated.contains("flux__local__"));

    let formatted = fluxc::formatter::format_source(source)
        .expect("inferred multi-value pattern should format");
    assert!(formatted.contains("let (result, _) = divide(84, 2)"));
    assert!(formatted.contains("let (shellResult, _) = divide 84 2"));
    assert!(formatted.contains("let (pipedResult, _) = 84 | divide 2"));
    let formatted_again = fluxc::formatter::format_source(&formatted)
        .expect("formatted inferred multi-value pattern should reparse");
    assert_eq!(formatted, formatted_again);
}

#[test]
fn accepts_inferred_multi_value_pattern_else_return() {
    let source = r#"
fn load(path: str) -> (str, error) {
    if path == "":
        return "", error("path is required")
    return "config", nil
}

fn loadConfig(path: str) -> (str, error) {
    let (data, _) = load(path) else return
    return data, nil
}

fn main() -> i64 {
    let (data, err) = loadConfig("settings")
    if err != nil:
        print(err)
    print(data)
    return 0
}
"#;

    check_source(source).expect("inferred else-return pattern should typecheck");
    let generated = compile_to_c(source).expect("inferred else-return pattern should compile");
    assert!(generated.contains("flux__multi_pattern_"));
    assert!(generated.contains(".v1 != NULL"));
}

#[test]
fn rejects_inferred_multi_value_pattern_arity_mismatch() {
    let source = r#"
fn pair() -> (i64, bool) {
    return 1, true
}

fn main() -> i64 {
    let (first, second, third) = pair()
    return 0
}
"#;

    let error = check_source(source).expect_err("multi-value pattern arity must match exactly");
    assert!(
        error
            .message
            .contains("multi-value pattern expects 3 values, expression returns 2")
    );
}

#[test]
fn rejects_duplicate_inferred_multi_value_pattern_binding() {
    let source = r#"
fn pair() -> (i64, bool) {
    return 1, true
}

fn main() -> i64 {
    let (value, value) = pair()
    return 0
}
"#;

    let error = check_source(source).expect_err("duplicate pattern bindings must fail");
    assert!(
        error
            .message
            .contains("duplicate destructured binding 'value'")
    );
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
    print(data)
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

#[test]
fn supports_explicit_mutable_bindings_and_while_loops() {
    let source = r#"
fn main() -> i64 {
    var count: i64 = 0
    var total: i64 = 0
    while count < 6:
        count = count + 1
        if count == 3:
            continue
        total = total + count
        if total > 10:
            break
    return total
}
"#;

    check_source(source).expect("explicit mutation and while should typecheck");
    let generated = compile_to_c(source).expect("explicit mutation and while should compile");
    assert!(generated.contains("while (flux__local_count < INT64_C(6))"));
    assert!(generated.contains("flux__local_count = flux_add_i64(flux__local_count, INT64_C(1));"));
    assert!(generated.contains("continue;"));
    assert!(generated.contains("break;"));
}

#[test]
fn mutation_is_explicit_and_statically_typed() {
    let immutable = r#"
fn main() -> i64 {
    let count: i64 = 0
    count = 1
    return count
}
"#;
    let error = check_source(immutable).expect_err("let bindings must remain immutable");
    assert!(
        error
            .message
            .contains("cannot assign to immutable binding 'count'")
    );
    assert!(error.message.contains("declare it with 'var'"));

    let parameter = r#"
fn bump(count: i64) -> i64 {
    count = count + 1
    return count
}
fn main() -> i64 { bump(1) }
"#;
    let error = check_source(parameter).expect_err("parameters must remain immutable");
    assert!(
        error
            .message
            .contains("cannot assign to immutable binding 'count'")
    );

    let wrong_type = r#"
fn main() -> i64 {
    var count: i64 = 0
    count = false
    return count
}
"#;
    let error = check_source(wrong_type).expect_err("assignment must preserve declared type");
    assert!(error.message.contains("assignment: expected i64, got bool"));

    let unknown = r#"
fn main() -> i64 {
    missing = 1
    return 0
}
"#;
    let error = check_source(unknown).expect_err("assignment needs an existing binding");
    assert!(error.message.contains("unknown binding 'missing'"));

    let bad_condition = r#"
fn main() -> i64 {
    var count: i64 = 0
    while count:
        count = count + 1
    return count
}
"#;
    let error = check_source(bad_condition).expect_err("while conditions must be boolean");
    assert!(
        error
            .message
            .contains("while condition: expected bool, got i64")
    );
}

#[test]
fn formatter_and_semantic_database_preserve_explicit_mutability() {
    let source = r#"
fn main() -> i64 {
  var count:i64=0
  while count<2:
    count=count+1
  return count
}
"#;
    let formatted = fluxc::formatter::format_source(source).expect("mutation source should format");
    assert!(formatted.contains("    var count: i64 = 0"));
    assert!(formatted.contains("    while count < 2:"));
    assert!(formatted.contains("        count = count + 1"));

    let database = fluxc::semantic::SemanticDatabase::analyze(source, SourceId::new(77))
        .expect("mutation source should analyze");
    let mutable = database
        .symbols()
        .iter()
        .find(|symbol| symbol.name == "count")
        .expect("mutable binding should be indexed");
    assert_eq!(mutable.kind, fluxc::semantic::SymbolKind::MutableBinding);
    assert_eq!(mutable.ty, Some(fluxc::ast::Type::I64));
}
