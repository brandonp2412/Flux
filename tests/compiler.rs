use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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
    let source = "const DEFAULT:i64=3\nfn describe(prefix:str=\"x\",*,count:i64=DEFAULT,label:str)->i64 {\n return count\n}\nfn main()->i64 {\n return describe(label:\"ok\")\n}\n";
    let expected = "const DEFAULT: i64 = 3\nfn describe(prefix: str = \"x\", *, count: i64 = DEFAULT, label: str) -> i64 {\n    return count\n}\nfn main() -> i64 {\n    return describe(label: \"ok\")\n}\n";
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
    assert!(generated.contains("flux__fn_memory_save"));
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

fn file_load(storage: FileStorage, path: str) -> (str, error) {
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
    assert!(generated.contains("flux__fn_file_load"));
    assert!(generated.contains("flux__fn_file_save"));
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

fn file_load(storage: FileStorage, path: str) -> (str, error) {
    return path, nil
}
fn file_label(storage: FileStorage) -> str {
    return storage.root
}
fn memory_load(storage: MemoryStorage, path: str) -> (str, error) {
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
    let file: FileStorage = FileStorage { root: "/tmp" }
    let ram: MemoryStorage = MemoryStorage { name: "ram" }
    return Storage(ram) if memory else Storage(file)
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
fn choose(flag: bool) -> i64 { 7 if flag else 2 }
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
    assert!(generated.contains("return (flux__local_value * flux__local_value);"));
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
fn accepts_python_style_conditional_expressions() {
    let source = r#"
const FALLBACK: i64 = 7 if true else 9

fn choose(flag: bool, value: i64 = 5 if true else 6) -> i64 {
    return value if flag else FALLBACK
}

fn main() -> i64 {
    let enabled: bool = false
    let value: i64 = choose(enabled)
    let nested: i64 = 1 if enabled else 2 if value == 7 else 3
    print(value)
    print(nested)
    return value
}
"#;

    check_source(source).expect("conditional expressions should typecheck");
    let generated = compile_to_c(source).expect("conditional expressions should lower natively");
    assert!(generated.contains(" ? "));
    assert!(generated.contains("INT64_C(7)"));
}

#[test]
fn rejects_invalid_conditional_expression_types() {
    let bad_condition = r#"
fn main() -> i64 {
    let value: i64 = 1 if 42 else 2
    return value
}
"#;
    let error = check_source(bad_condition).expect_err("conditional condition must be bool");
    assert!(
        error
            .message
            .contains("conditional expression condition: expected bool, got i64")
    );

    let mismatched = r#"
fn main() -> i64 {
    let value: i64 = 1 if true else "two"
    return value
}
"#;
    let error = check_source(mismatched).expect_err("conditional branches must agree");
    assert!(
        error
            .message
            .contains("conditional expression branch: expected i64, got str")
    );

    let void_branch = r#"
fn main() -> i64 {
    let value: i64 = print("one") if true else 2
    return value
}
"#;
    let error = check_source(void_branch).expect_err("conditional branches cannot be void");
    assert!(
        error
            .message
            .contains("conditional expression branches cannot produce void")
    );
}

#[test]
fn formatter_preserves_conditional_expression_precedence() {
    let source = "fn main()->i64 {\n let enabled:bool=true\n let value:i64=1+2 if enabled else 3+4\n let nested:i64=1 if enabled else 2 if false else 3\n return value+nested\n}\n";
    let expected = "fn main() -> i64 {\n    let enabled: bool = true\n    let value: i64 = 1 + 2 if enabled else 3 + 4\n    let nested: i64 = 1 if enabled else 2 if false else 3\n    return value + nested\n}\n";
    let formatted =
        fluxc::formatter::format_source(source).expect("conditional source should format");
    assert_eq!(formatted, expected);
    check_source(&formatted).expect("formatted conditional source should still typecheck");
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
    let second: Outcome = Outcome.Error("nope", 7)
    let third: Outcome = Outcome.Pending()
    let carried: Outcome = pass(first)
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
    let envelope: Envelope = Envelope { outcome: outcome }
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
    let source = "enum Outcome {\n Ok(i64)\n Pending\n}\nfn main()->i64 {\n let value:Outcome=Outcome.Ok(42)\n return 0\n}\n";
    let expected = "enum Outcome {\n    Ok(i64)\n    Pending\n}\nfn main() -> i64 {\n    let value: Outcome = Outcome.Ok(42)\n    return 0\n}\n";
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
    assert!(source.contains("on_press: clicked => !clicked"));
    assert!(source.contains("app App"));

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
    let source = r#"
view Counter {
    grid columns: 1fr
    grid rows: auto auto
    state clicked: bool = false
    Text title at 1,1
        text: "Clicked!" if clicked else "Hello"
    Button action at 2,1
        text: "Reset" if clicked else "Click me"
        on_press: clicked => !clicked
}
app Counter
"#;

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
    assert!(formatted.contains("state clicked: bool = false"));
    assert!(formatted.contains("on_press: clicked => !clicked"));

    let generated = compile_to_c(source).expect("stateful Linux app should lower");
    assert!(generated.contains("static bool flux__ui_state_clicked = false;"));
    assert!(generated.contains("flux__ui_state_clicked = (!(flux__ui_state_clicked));"));
    assert!(generated.contains("flux__ui_refresh();"));
    assert!(generated.contains("gtk_label_set_text"));
    assert!(generated.contains("gtk_button_set_label"));
    assert!(generated.contains("flux__ui_state_clicked") && generated.contains("Clicked!"));
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
    let choice: VisibleChoice = VisibleChoice.Yes()
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
    assert!(generated.contains("flux__local_count = (flux__local_count + INT64_C(1));"));
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
