use std::collections::{HashMap, HashSet};

use crate::ast::{
    BinOp, EnumDef, Expr, ExprKind, Function, ListMatchPattern, MatchPattern, NamedArg, Program,
    ShellRedirectMode, Stmt, StmtKind, StructDef, StructPatternField, Type, UnaryOp,
};
use crate::diagnostic::{Diagnostic, DiagnosticStage, SourceId, SourceSpan};
use crate::typecheck::{self, ConstantValue, Signature, Signatures, type_of_expr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTarget {
    Linux,
    Android,
}

pub fn emit_c(program: &Program, signatures: &Signatures) -> Result<String, Diagnostic> {
    emit_c_with_source_paths(program, signatures, &HashMap::new())
}

pub fn emit_c_with_source_paths(
    program: &Program,
    signatures: &Signatures,
    source_paths: &HashMap<SourceId, String>,
) -> Result<String, Diagnostic> {
    emit_c_for_target_with_source_paths(program, signatures, source_paths, NativeTarget::Linux)
}

pub fn emit_c_for_target_with_source_paths(
    program: &Program,
    signatures: &Signatures,
    source_paths: &HashMap<SourceId, String>,
    target: NativeTarget,
) -> Result<String, Diagnostic> {
    let function_ir = build_function_ir_cache(program, signatures);
    let mut reachable_interfaces = HashSet::new();
    let mut interface_pack_facts = InterfacePackFacts::external_roots(program, signatures);
    let mut reachable_functions = reachable_function_names(
        program,
        signatures,
        &reachable_interfaces,
        &interface_pack_facts,
        &function_ir,
    );
    let mut reachable_value_types = reachable_value_type_names(
        program,
        signatures,
        &reachable_functions,
        &reachable_interfaces,
        &function_ir,
    );
    loop {
        let next_interfaces = reachable_interface_names(
            program,
            signatures,
            &reachable_functions,
            &reachable_value_types,
            &function_ir,
        );
        let next_pack_facts = InterfacePackFacts::from_reachable_functions(
            program,
            signatures,
            &reachable_functions,
            &function_ir,
        );
        let next_functions = reachable_function_names(
            program,
            signatures,
            &next_interfaces,
            &next_pack_facts,
            &function_ir,
        );
        let next_value_types = reachable_value_type_names(
            program,
            signatures,
            &next_functions,
            &next_interfaces,
            &function_ir,
        );
        if next_interfaces == reachable_interfaces
            && next_functions == reachable_functions
            && next_value_types == reachable_value_types
            && next_pack_facts == interface_pack_facts
        {
            break;
        }
        reachable_interfaces = next_interfaces;
        reachable_functions = next_functions;
        reachable_value_types = next_value_types;
        interface_pack_facts = next_pack_facts;
    }
    let reachable_enum_variants =
        reachable_enum_variant_helpers(program, signatures, &reachable_functions, &function_ir);
    let anonymous_functions =
        collect_anonymous_functions(program, &reachable_functions, &function_ir);
    let mut generated_body = String::new();
    for function in &anonymous_functions {
        emit_anonymous_function(&mut generated_body, function, signatures, source_paths)?;
        generated_body.push('\n');
    }
    let mut temp_counter = 0usize;
    for function in &program.functions {
        if reachable_functions.contains(&function.name) {
            let cfg = function_ir
                .get(&function.name)
                .expect("all parsed functions have cached typed IR");
            emit_function(
                &mut generated_body,
                function,
                signatures,
                cfg,
                &mut temp_counter,
                source_paths,
            )?;
            generated_body.push('\n');
        }
    }
    let mut application_body = String::new();
    if program.application.is_some() {
        match target {
            NativeTarget::Linux => {
                emit_linux_gtk_application(&mut application_body, program, signatures)?
            }
            NativeTarget::Android => {
                emit_android_native_application(&mut application_body, program, signatures)?
            }
        }
    }
    let runtime_usage = format!("{generated_body}{application_body}");
    if target != NativeTarget::Android && runtime_usage.contains("flux__android_") {
        return Err(Diagnostic::global(
            DiagnosticStage::Codegen,
            "android.* platform APIs require the Android target",
        ));
    }

    let mut out = String::new();
    emit_runtime_prelude(
        &mut out,
        &runtime_usage,
        program_uses_background(program, &reachable_functions, &function_ir),
        program.application.is_some() && target == NativeTarget::Linux,
        program.application.is_some() && target == NativeTarget::Android,
    );

    for definition in &program.structs {
        if reachable_value_types.contains(&definition.name) {
            out.push_str(&format!("struct {};\n", struct_c_name(&definition.name)));
        }
    }
    for definition in &program.enums {
        if reachable_value_types.contains(&definition.name) {
            out.push_str(&format!("struct {};\n", struct_c_name(&definition.name)));
        }
    }
    for definition in &program.interfaces {
        if reachable_interfaces.contains(&definition.name) {
            out.push_str(&format!("struct {};\n", interface_c_name(&definition.name)));
        }
    }
    if !reachable_value_types.is_empty() || !reachable_interfaces.is_empty() {
        out.push('\n');
    }
    emit_function_type_typedefs(
        &mut out,
        program,
        signatures,
        &reachable_functions,
        &reachable_value_types,
        &reachable_interfaces,
        &function_ir,
    )?;

    for definition in value_type_emit_order(program, signatures)? {
        if !reachable_value_types.contains(definition.name()) {
            continue;
        }
        match definition {
            ValueDef::Struct(definition) => {
                emit_struct_definition(&mut out, definition, signatures)
            }
            ValueDef::Enum(definition) => emit_enum_definition(&mut out, definition, signatures),
        }
    }
    if !reachable_value_types.is_empty() {
        out.push('\n');
    }

    emit_interface_value_definitions(
        &mut out,
        program,
        signatures,
        &reachable_interfaces,
        &reachable_value_types,
        &interface_pack_facts,
    );
    out.push_str(&interface_pack_helpers(
        program,
        signatures,
        &reachable_interfaces,
        &reachable_value_types,
        &interface_pack_facts,
        &runtime_usage,
    ));
    out.push_str(&enum_variant_helpers(
        program,
        signatures,
        &reachable_value_types,
        &reachable_enum_variants,
    ));
    out.push_str(&struct_update_helpers(
        program,
        signatures,
        &reachable_functions,
        &function_ir,
    ));

    for function in &program.functions {
        if reachable_functions.contains(&function.name) && function.returns.len() > 1 {
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
        .any(|function| reachable_functions.contains(&function.name) && function.returns.len() > 1)
    {
        out.push('\n');
    }
    emit_interface_multi_return_structs(
        &mut out,
        program,
        signatures,
        &reachable_interfaces,
        &runtime_usage,
    );

    for function in &program.functions {
        if reachable_functions.contains(&function.name) {
            out.push_str(&function_prototype(function, signatures));
            out.push_str(";\n");
        }
    }
    for function in &anonymous_functions {
        out.push_str(&anonymous_function_prototype(function, signatures)?);
        out.push_str(";\n");
    }
    out.push('\n');
    emit_interface_dispatch_helpers(
        &mut out,
        program,
        signatures,
        &reachable_interfaces,
        &reachable_value_types,
        &interface_pack_facts,
        &runtime_usage,
    )?;
    out.push_str(&generated_body);
    out.push_str(&application_body);

    Ok(out)
}

fn emit_runtime_prelude(
    out: &mut String,
    runtime_usage: &str,
    uses_background: bool,
    uses_gtk: bool,
    uses_android: bool,
) {
    out.push_str("#include <stdbool.h>\n");
    out.push_str("#include <stdint.h>\n");
    out.push_str("#include <stddef.h>\n");
    out.push_str("#include <stdio.h>\n");
    out.push_str("#include <stdlib.h>\n");
    out.push_str("#include <string.h>\n");
    if uses_background {
        out.push_str("#include <errno.h>\n");
        out.push_str("#include <sys/types.h>\n");
        out.push_str("#include <sys/wait.h>\n");
        out.push_str("#include <unistd.h>\n");
    }
    if uses_gtk {
        out.push_str("#include <gtk/gtk.h>\n");
    }
    if uses_android {
        out.push_str("#include <android/native_activity.h>\n");
    }
    out.push('\n');

    let uses_android_vibrate = uses_android && runtime_usage.contains("flux__android_vibrate(");
    let uses_android_open_url = uses_android && runtime_usage.contains("flux__android_open_url(");
    let uses_android_platform_api = uses_android_vibrate || uses_android_open_url;
    if uses_android {
        out.push_str("static ANativeActivity *flux__android_activity = NULL;\n");
    }
    if uses_android_platform_api {
        out.push_str("static JNIEnv *flux__android_get_env(bool *detach) {\n");
        out.push_str("    *detach = false;\n");
        out.push_str("    if (flux__android_activity == NULL) return NULL;\n");
        out.push_str("    JavaVM *vm = flux__android_activity->vm;\n");
        out.push_str("    JNIEnv *env = NULL;\n");
        out.push_str("    jint status = (*vm)->GetEnv(vm, (void **)&env, JNI_VERSION_1_6);\n");
        out.push_str("    if (status == JNI_EDETACHED) {\n");
        out.push_str(
            "        if ((*vm)->AttachCurrentThread(vm, &env, NULL) != JNI_OK) return NULL;\n",
        );
        out.push_str("        *detach = true;\n");
        out.push_str("    } else if (status != JNI_OK) {\n");
        out.push_str("        return NULL;\n");
        out.push_str("    }\n");
        out.push_str("    return env;\n");
        out.push_str("}\n");
        out.push_str("static void flux__android_release_env(bool detach) {\n");
        out.push_str("    if (detach && flux__android_activity != NULL) {\n");
        out.push_str("        JavaVM *vm = flux__android_activity->vm;\n");
        out.push_str("        (*vm)->DetachCurrentThread(vm);\n");
        out.push_str("    }\n");
        out.push_str("}\n");
    }
    if uses_android_open_url {
        out.push_str(
            "static jstring flux__android_utf8_string(JNIEnv *env, const char *value) {\n",
        );
        out.push_str("    if (value == NULL) return NULL;\n");
        out.push_str("    size_t length = strlen(value);\n");
        out.push_str("    if (length > INT32_MAX) return NULL;\n");
        out.push_str("    jbyteArray bytes = (*env)->NewByteArray(env, (jsize)length);\n");
        out.push_str("    if (bytes == NULL) return NULL;\n");
        out.push_str(
            "    (*env)->SetByteArrayRegion(env, bytes, 0, (jsize)length, (const jbyte *)value);\n",
        );
        out.push_str("    if ((*env)->ExceptionCheck(env)) { (*env)->ExceptionClear(env); (*env)->DeleteLocalRef(env, bytes); return NULL; }\n");
        out.push_str("    jclass string_class = (*env)->FindClass(env, \"java/lang/String\");\n");
        out.push_str("    if (string_class == NULL) { if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env); (*env)->DeleteLocalRef(env, bytes); return NULL; }\n");
        out.push_str("    jmethodID ctor = (*env)->GetMethodID(env, string_class, \"<init>\", \"([BLjava/lang/String;)V\");\n");
        out.push_str("    jstring charset = (*env)->NewStringUTF(env, \"UTF-8\");\n");
        out.push_str("    jstring result = NULL;\n");
        out.push_str("    if (ctor != NULL && charset != NULL) result = (jstring)(*env)->NewObject(env, string_class, ctor, bytes, charset);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) { (*env)->ExceptionClear(env); result = NULL; }\n");
        out.push_str("    if (charset != NULL) (*env)->DeleteLocalRef(env, charset);\n");
        out.push_str("    (*env)->DeleteLocalRef(env, string_class);\n");
        out.push_str("    (*env)->DeleteLocalRef(env, bytes);\n");
        out.push_str("    return result;\n");
        out.push_str("}\n");
    }
    if uses_android_vibrate {
        out.push_str("static void flux__android_vibrate(int64_t duration_ms) {\n");
        out.push_str("    if (duration_ms <= 0 || flux__android_activity == NULL) return;\n");
        out.push_str("    bool detach = false;\n");
        out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
        out.push_str("    if (env == NULL) return;\n");
        out.push_str("    jobject activity = flux__android_activity->clazz;\n");
        out.push_str("    jclass activity_class = (*env)->GetObjectClass(env, activity);\n");
        out.push_str("    if (activity_class == NULL) goto done;\n");
        out.push_str("    jmethodID get_service = (*env)->GetMethodID(env, activity_class, \"getSystemService\", \"(Ljava/lang/String;)Ljava/lang/Object;\");\n");
        out.push_str("    if (get_service == NULL) goto done_activity_class;\n");
        out.push_str("    jstring service_name = (*env)->NewStringUTF(env, \"vibrator\");\n");
        out.push_str("    if (service_name == NULL) goto done_activity_class;\n");
        out.push_str("    jobject vibrator = (*env)->CallObjectMethod(env, activity, get_service, service_name);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) { (*env)->ExceptionClear(env); vibrator = NULL; }\n");
        out.push_str("    if (vibrator != NULL) {\n");
        out.push_str("        jclass vibrator_class = (*env)->GetObjectClass(env, vibrator);\n");
        out.push_str("        if (vibrator_class != NULL) {\n");
        out.push_str("            jmethodID vibrate = (*env)->GetMethodID(env, vibrator_class, \"vibrate\", \"(J)V\");\n");
        out.push_str("            if (vibrate != NULL) (*env)->CallVoidMethod(env, vibrator, vibrate, (jlong)duration_ms);\n");
        out.push_str("            if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
        out.push_str("            (*env)->DeleteLocalRef(env, vibrator_class);\n");
        out.push_str("        }\n");
        out.push_str("        (*env)->DeleteLocalRef(env, vibrator);\n");
        out.push_str("    }\n");
        out.push_str("    (*env)->DeleteLocalRef(env, service_name);\n");
        out.push_str("done_activity_class:\n");
        out.push_str("    (*env)->DeleteLocalRef(env, activity_class);\n");
        out.push_str("done:\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
        out.push_str("    flux__android_release_env(detach);\n");
        out.push_str("}\n");
    }
    if uses_android_open_url {
        out.push_str("static void flux__android_open_url(const char *url) {\n");
        out.push_str("    if (url == NULL || flux__android_activity == NULL) return;\n");
        out.push_str("    bool detach = false;\n");
        out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
        out.push_str("    if (env == NULL) return;\n");
        out.push_str("    jclass uri_class = NULL; jclass intent_class = NULL; jclass activity_class = NULL;\n");
        out.push_str("    jstring url_string = NULL; jstring action = NULL;\n");
        out.push_str("    jobject uri = NULL; jobject intent = NULL;\n");
        out.push_str("    uri_class = (*env)->FindClass(env, \"android/net/Uri\");\n");
        out.push_str("    if (uri_class == NULL) goto done;\n");
        out.push_str("    jmethodID parse = (*env)->GetStaticMethodID(env, uri_class, \"parse\", \"(Ljava/lang/String;)Landroid/net/Uri;\");\n");
        out.push_str("    if (parse == NULL) goto done;\n");
        out.push_str("    url_string = flux__android_utf8_string(env, url);\n");
        out.push_str("    if (url_string == NULL) goto done;\n");
        out.push_str(
            "    uri = (*env)->CallStaticObjectMethod(env, uri_class, parse, url_string);\n",
        );
        out.push_str("    if ((*env)->ExceptionCheck(env) || uri == NULL) goto done;\n");
        out.push_str("    intent_class = (*env)->FindClass(env, \"android/content/Intent\");\n");
        out.push_str("    if (intent_class == NULL) goto done;\n");
        out.push_str("    jmethodID ctor = (*env)->GetMethodID(env, intent_class, \"<init>\", \"(Ljava/lang/String;Landroid/net/Uri;)V\");\n");
        out.push_str("    if (ctor == NULL) goto done;\n");
        out.push_str("    action = (*env)->NewStringUTF(env, \"android.intent.action.VIEW\");\n");
        out.push_str("    if (action == NULL) goto done;\n");
        out.push_str("    intent = (*env)->NewObject(env, intent_class, ctor, action, uri);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || intent == NULL) goto done;\n");
        out.push_str(
            "    activity_class = (*env)->GetObjectClass(env, flux__android_activity->clazz);\n",
        );
        out.push_str("    if (activity_class == NULL) goto done;\n");
        out.push_str("    jmethodID start_activity = (*env)->GetMethodID(env, activity_class, \"startActivity\", \"(Landroid/content/Intent;)V\");\n");
        out.push_str("    if (start_activity == NULL) goto done;\n");
        out.push_str("    (*env)->CallVoidMethod(env, flux__android_activity->clazz, start_activity, intent);\n");
        out.push_str("done:\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
        out.push_str(
            "    if (activity_class != NULL) (*env)->DeleteLocalRef(env, activity_class);\n",
        );
        out.push_str("    if (intent != NULL) (*env)->DeleteLocalRef(env, intent);\n");
        out.push_str("    if (action != NULL) (*env)->DeleteLocalRef(env, action);\n");
        out.push_str("    if (intent_class != NULL) (*env)->DeleteLocalRef(env, intent_class);\n");
        out.push_str("    if (uri != NULL) (*env)->DeleteLocalRef(env, uri);\n");
        out.push_str("    if (url_string != NULL) (*env)->DeleteLocalRef(env, url_string);\n");
        out.push_str("    if (uri_class != NULL) (*env)->DeleteLocalRef(env, uri_class);\n");
        out.push_str("    flux__android_release_env(detach);\n");
        out.push_str("}\n");
    }

    if runtime_usage.contains("flux_print_i64(") {
        out.push_str("static inline void flux_print_i64(int64_t value) { printf(\"%lld\\n\", (long long)value); }\n");
    }
    if runtime_usage.contains("flux_print_bool(") {
        out.push_str(
            "static inline void flux_print_bool(bool value) { puts(value ? \"true\" : \"false\"); }\n",
        );
    }
    if runtime_usage.contains("flux_print_str(") {
        out.push_str("static inline void flux_print_str(const char *value) { puts(value); }\n");
    }
    if runtime_usage.contains("flux_print_error(") {
        out.push_str("static inline void flux_print_error(const char *value) { puts(value ? value : \"nil\"); }\n");
    }

    let uses_redirect_i64 = runtime_usage.contains("flux_redirect_i64(");
    let uses_redirect_bool = runtime_usage.contains("flux_redirect_bool(");
    let uses_redirect_str = runtime_usage.contains("flux_redirect_str(");
    let uses_redirect_error = runtime_usage.contains("flux_redirect_error(");
    if uses_redirect_i64 || uses_redirect_bool || uses_redirect_str || uses_redirect_error {
        out.push_str("static inline FILE *flux_open_redirect(const char *path, bool append) { FILE *file = fopen(path, append ? \"a\" : \"w\"); if (file == NULL) { perror(path); abort(); } return file; }\n");
    }
    if uses_redirect_i64 {
        out.push_str("static inline void flux_redirect_i64(const char *path, bool append, int64_t value) { FILE *file = flux_open_redirect(path, append); fprintf(file, \"%lld\\n\", (long long)value); fclose(file); }\n");
    }
    if uses_redirect_bool {
        out.push_str("static inline void flux_redirect_bool(const char *path, bool append, bool value) { FILE *file = flux_open_redirect(path, append); fputs(value ? \"true\\n\" : \"false\\n\", file); fclose(file); }\n");
    }
    if uses_redirect_str {
        out.push_str("static inline void flux_redirect_str(const char *path, bool append, const char *value) { FILE *file = flux_open_redirect(path, append); fputs(value, file); fputc('\\n', file); fclose(file); }\n");
    }
    if uses_redirect_error {
        out.push_str("static inline void flux_redirect_error(const char *path, bool append, const char *value) { FILE *file = flux_open_redirect(path, append); fputs(value ? value : \"nil\", file); fputc('\\n', file); fclose(file); }\n");
    }
    if runtime_usage.contains("flux_error_eq(") {
        out.push_str("static inline bool flux_error_eq(const char *a, const char *b) { return a == NULL ? b == NULL : b != NULL && strcmp(a, b) == 0; }\n");
    }

    let uses_list_at = runtime_usage.contains("flux_list_at(");
    let uses_list_single = runtime_usage.contains("flux_list_single(");
    let uses_list_take = runtime_usage.contains("flux_list_take(");
    let uses_list_skip = runtime_usage.contains("flux_list_skip(");
    let uses_list_any = runtime_usage.contains("flux_list_any_bool(");
    let uses_list_every = runtime_usage.contains("flux_list_every_bool(");
    let uses_list_slice = runtime_usage.contains("flux_list_slice(");
    let uses_list_unchecked = runtime_usage.contains("flux_list_at_unchecked(")
        || uses_list_at
        || uses_list_any
        || uses_list_every;
    let uses_list_stride = runtime_usage.contains("flux_list_stride(")
        || uses_list_unchecked
        || uses_list_skip
        || uses_list_slice;
    let uses_list_count = uses_list_take || uses_list_skip;
    let uses_list = runtime_usage.contains("struct flux__list")
        || uses_list_at
        || uses_list_single
        || uses_list_take
        || uses_list_skip
        || uses_list_any
        || uses_list_every
        || uses_list_slice
        || uses_list_unchecked
        || uses_list_stride;
    if uses_list {
        out.push_str("struct flux__list { void *data; size_t len; ptrdiff_t stride; };\n");
    }
    if uses_list_stride {
        out.push_str("static inline ptrdiff_t flux_list_stride(struct flux__list list, size_t elem_size) { return list.stride == 0 ? (ptrdiff_t)elem_size : list.stride; }\n");
    }
    if uses_list_at {
        out.push_str("static inline size_t flux_list_index(size_t len, int64_t index) { int64_t resolved = index; if (resolved < 0) resolved += (int64_t)len; if (resolved < 0 || (uint64_t)resolved >= (uint64_t)len) { fputs(\"Flux runtime error: list index out of range\\n\", stderr); abort(); } return (size_t)resolved; }\n");
    }
    if uses_list_unchecked {
        out.push_str("static inline void *flux_list_at_unchecked(struct flux__list list, size_t index, size_t elem_size) { ptrdiff_t stride = flux_list_stride(list, elem_size); return (void *)((char *)list.data + (ptrdiff_t)index * stride); }\n");
    }
    if uses_list_at {
        out.push_str("static inline void *flux_list_at(struct flux__list list, int64_t index, size_t elem_size) { return flux_list_at_unchecked(list, flux_list_index(list.len, index), elem_size); }\n");
    }
    if uses_list_single {
        out.push_str("static inline void *flux_list_single(struct flux__list list) { if (list.len != 1) { fputs(\"Flux runtime error: list.single requires exactly one element\\n\", stderr); abort(); } return list.data; }\n");
    }
    if uses_list_count {
        out.push_str("static inline size_t flux_list_count(size_t len, int64_t count) { if (count < 0) { fputs(\"Flux runtime error: list count must be non-negative\\n\", stderr); abort(); } uint64_t value = (uint64_t)count; return value > (uint64_t)len ? len : (size_t)value; }\n");
    }
    if uses_list_take {
        out.push_str("static inline struct flux__list flux_list_take(struct flux__list list, int64_t count) { list.len = flux_list_count(list.len, count); return list; }\n");
    }
    if uses_list_skip {
        out.push_str("static inline struct flux__list flux_list_skip(struct flux__list list, int64_t count, size_t elem_size) { size_t skipped = flux_list_count(list.len, count); ptrdiff_t stride = flux_list_stride(list, elem_size); list.data = (void *)((char *)list.data + (ptrdiff_t)skipped * stride); list.len -= skipped; list.stride = stride; return list; }\n");
    }
    if uses_list_any {
        out.push_str("static inline bool flux_list_any_bool(struct flux__list list) { for (size_t i = 0; i < list.len; ++i) { if (*((bool *)flux_list_at_unchecked(list, i, sizeof(bool)))) return true; } return false; }\n");
    }
    if uses_list_every {
        out.push_str("static inline bool flux_list_every_bool(struct flux__list list) { for (size_t i = 0; i < list.len; ++i) { if (!*((bool *)flux_list_at_unchecked(list, i, sizeof(bool)))) return false; } return true; }\n");
    }
    if uses_list_slice {
        out.push_str("static inline int64_t flux_slice_bound(size_t len, bool present, int64_t value, bool end, int64_t step) { if (step > 0) { if (!present) return end ? (int64_t)len : 0; int64_t resolved = value; if (resolved < 0) resolved += (int64_t)len; if (resolved < 0) return 0; if ((uint64_t)resolved > (uint64_t)len) return (int64_t)len; return resolved; } if (!present) return end ? -1 : (len == 0 ? -1 : (int64_t)len - 1); int64_t resolved = value; if (resolved < 0) resolved += (int64_t)len; if (resolved < 0) return -1; if ((uint64_t)resolved >= (uint64_t)len) return len == 0 ? -1 : (int64_t)len - 1; return resolved; }\n");
        out.push_str("static inline struct flux__list flux_list_slice(struct flux__list list, bool has_start, int64_t start, bool has_end, int64_t end, int64_t step, size_t elem_size) { if (step == 0) { fputs(\"Flux runtime error: list slice step cannot be zero\\n\", stderr); abort(); } if (step > (int64_t)PTRDIFF_MAX || step < (int64_t)PTRDIFF_MIN) { fputs(\"Flux runtime error: list slice step is too large\\n\", stderr); abort(); } int64_t first = flux_slice_bound(list.len, has_start, start, false, step); int64_t last = flux_slice_bound(list.len, has_end, end, true, step); size_t count = 0; if (step > 0 && first < last) { count = (size_t)(1 + (uint64_t)(last - 1 - first) / (uint64_t)step); } else if (step < 0 && first > last) { uint64_t magnitude = (uint64_t)(-(step + 1)) + 1; count = (size_t)(1 + (uint64_t)(first - 1 - last) / magnitude); } ptrdiff_t base_stride = flux_list_stride(list, elem_size); ptrdiff_t next_stride = 0; if (__builtin_mul_overflow(base_stride, (ptrdiff_t)step, &next_stride)) { fputs(\"Flux runtime error: list slice stride overflow\\n\", stderr); abort(); } void *data = list.data; if (count != 0) data = (void *)((char *)list.data + (ptrdiff_t)first * base_stride); struct flux__list result = { .data = data, .len = count, .stride = next_stride }; return result; }\n");
    }

    if runtime_usage.contains("flux_add_i64(") {
        out.push_str("static inline int64_t flux_add_i64(int64_t a, int64_t b) { int64_t result; if (__builtin_add_overflow(a, b, &result)) { fputs(\"Flux runtime error: integer addition overflow\\n\", stderr); abort(); } return result; }\n");
    }
    if runtime_usage.contains("flux_sub_i64(") {
        out.push_str("static inline int64_t flux_sub_i64(int64_t a, int64_t b) { int64_t result; if (__builtin_sub_overflow(a, b, &result)) { fputs(\"Flux runtime error: integer subtraction overflow\\n\", stderr); abort(); } return result; }\n");
    }
    if runtime_usage.contains("flux_mul_i64(") {
        out.push_str("static inline int64_t flux_mul_i64(int64_t a, int64_t b) { int64_t result; if (__builtin_mul_overflow(a, b, &result)) { fputs(\"Flux runtime error: integer multiplication overflow\\n\", stderr); abort(); } return result; }\n");
    }
    if runtime_usage.contains("flux_neg_i64(") {
        out.push_str("static inline int64_t flux_neg_i64(int64_t value) { if (value == INT64_MIN) { fputs(\"Flux runtime error: integer negation overflow\\n\", stderr); abort(); } return -value; }\n");
    }
    if runtime_usage.contains("flux_div_i64(") {
        out.push_str("static inline int64_t flux_div_i64(int64_t a, int64_t b) {\n");
        out.push_str("    if (b == 0 || (a == INT64_MIN && b == -1)) { fputs(\"Flux runtime error: invalid integer division\\n\", stderr); abort(); }\n");
        out.push_str("    return a / b;\n");
        out.push_str("}\n");
    }
    out.push('\n');
}

fn emit_android_native_application(
    out: &mut String,
    program: &Program,
    _signatures: &Signatures,
) -> Result<(), Diagnostic> {
    let application = program
        .application
        .as_ref()
        .expect("application lowering requires app declaration");
    if program
        .views
        .iter()
        .all(|view| view.name != application.view_name)
    {
        return Err(diag(
            application.view_span,
            "app root view was not found during Android codegen",
        ));
    }

    if let Some(function) = application_metadata_function(application, "on_exit") {
        out.push_str(&format!(
            "static void flux__android_on_destroy(ANativeActivity *activity) {{ (void)activity; {}(); }}\n\n",
            function_c_name(function),
        ));
    }
    out.push_str("__attribute__((visibility(\"default\"))) void ANativeActivity_onCreate(ANativeActivity *activity, void *saved_state, size_t saved_state_size) {\n    (void)saved_state;\n    (void)saved_state_size;\n    flux__android_activity = activity;\n");
    if application_metadata_function(application, "on_exit").is_some() {
        out.push_str("    activity->callbacks->onDestroy = flux__android_on_destroy;\n");
    }
    if let Some(function) = application_metadata_function(application, "on_start") {
        out.push_str(&format!("    {}();\n", function_c_name(function)));
    }
    out.push_str("}\n");
    Ok(())
}

fn emit_linux_gtk_application(
    out: &mut String,
    program: &Program,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    let application = program
        .application
        .as_ref()
        .expect("application lowering requires app declaration");
    let view = program
        .views
        .iter()
        .find(|view| view.name == application.view_name)
        .ok_or_else(|| {
            diag(
                application.view_span,
                "app root view was not found during codegen",
            )
        })?;

    let (bootstrap_width, bootstrap_height) = bootstrap_window_size(view);
    let initial_window_width = application_metadata_i64(application, "width", signatures)
        .unwrap_or(i64::from(bootstrap_width));
    let initial_window_height = application_metadata_i64(application, "height", signatures)
        .unwrap_or(i64::from(bootstrap_height));

    for element in &view.elements {
        if !matches!(
            element.kind.as_str(),
            "Text" | "Button" | "TextInput" | "Image" | "Toggle" | "Radio"
        ) {
            return Err(diag(
                element.kind_span,
                "bootstrap Linux app backend currently renders Text, Button, TextInput, Image, Toggle, and Radio elements",
            ));
        }
    }

    out.push_str(&format!(
        "static int64_t flux__ui_window_width = INT64_C({initial_window_width});\nstatic int64_t flux__ui_window_height = INT64_C({initial_window_height});\nstatic int64_t flux__ui_display_scale = INT64_C(1);\n"
    ));
    for state in &view.states {
        let state_name = ui_state_c_name(&state.name);
        match signatures.canonical_type(&state.ty) {
            Type::Bool => {
                let Some(initial) = static_expr_bool(&state.initial, signatures) else {
                    return Err(diag(
                        state.initial.span,
                        "bootstrap Linux bool state requires a compile-time bool initial value",
                    ));
                };
                out.push_str(&format!(
                    "static bool {state_name} = {};\n",
                    if initial { "true" } else { "false" }
                ));
            }
            Type::I64 => {
                let Some(initial) = static_expr_i64(&state.initial, signatures) else {
                    return Err(diag(
                        state.initial.span,
                        "bootstrap Linux i64 state requires a compile-time integer initial value",
                    ));
                };
                out.push_str(&format!(
                    "static int64_t {state_name} = INT64_C({initial});\n"
                ));
            }
            _ => {
                return Err(diag(
                    state.type_span,
                    "bootstrap Linux view state currently supports bool and i64; owned/string/aggregate state remains pending",
                ));
            }
        }
    }
    for derived in &view.derived {
        let derived_name = ui_derived_c_name(&derived.name);
        let initial = match signatures.canonical_type(&derived.ty) {
            Type::I64 => "INT64_C(0)",
            Type::Bool => "false",
            Type::Str => "NULL",
            _ => {
                return Err(diag(
                    derived.type_span,
                    "bootstrap Linux derived view values currently support i64, bool, and str",
                ));
            }
        };
        out.push_str(&format!(
            "static {} {derived_name} = {initial};\n",
            c_type(&derived.ty, signatures)
        ));
    }
    for element in &view.elements {
        out.push_str(&format!(
            "static GtkWidget *{} = NULL;\n",
            ui_widget_c_name(&element.name)
        ));
        if element_has_dynamic_transform(element, signatures) {
            out.push_str(&format!(
                "static GtkCssProvider *{} = NULL;\n",
                ui_transform_provider_c_name(&element.name)
            ));
        }
    }
    out.push('\n');
    emit_ui_refresh(out, view, signatures)?;
    out.push_str(
        "static void flux__ui_window_environment_changed(GObject *object, GParamSpec *pspec, gpointer data) {\n    (void)pspec;\n    (void)data;\n    int width = -1;\n    int height = -1;\n    gtk_window_get_default_size(GTK_WINDOW(object), &width, &height);\n    int scale = gtk_widget_get_scale_factor(GTK_WIDGET(object));\n    int64_t next_width = width > 0 ? (int64_t)width : flux__ui_window_width;\n    int64_t next_height = height > 0 ? (int64_t)height : flux__ui_window_height;\n    int64_t next_scale = scale > 0 ? (int64_t)scale : INT64_C(1);\n    if (next_width == flux__ui_window_width && next_height == flux__ui_window_height && next_scale == flux__ui_display_scale) return;\n    flux__ui_window_width = next_width;\n    flux__ui_window_height = next_height;\n    flux__ui_display_scale = next_scale;\n    flux__ui_refresh();\n}\n\n",
    );

    for element in &view.elements {
        if element.kind == "TextInput" {
            for (property_name, callback_name) in [("on_change", "change"), ("on_submit", "submit")]
            {
                let Some(action) = view_property(element, property_name) else {
                    continue;
                };
                let ExprKind::Var(function) = &action.value.kind else {
                    return Err(diag(
                        action.value.span,
                        &format!(
                            "bootstrap TextInput.{property_name} lowering requires a named fn(str) -> void callback"
                        ),
                    ));
                };
                out.push_str(&format!(
                    "static void flux__ui_{callback_name}_{}(GtkWidget *widget, gpointer data) {{ (void)data; {}(gtk_editable_get_text(GTK_EDITABLE(widget))); flux__ui_refresh(); }}\n",
                    element.name,
                    function_c_name(function),
                ));
            }
            continue;
        }
        let action_name = match element.kind.as_str() {
            "Button" => "on_press",
            "Toggle" => "on_change",
            "Radio" => "on_select",
            _ => continue,
        };
        let Some(action) = view_property(element, action_name) else {
            continue;
        };
        if let Some(transition) = &action.transition {
            let next = ui_expr_c(&action.value, view, signatures)?;
            let active_guard = if element.kind == "Radio" {
                " if (!gtk_check_button_get_active(GTK_CHECK_BUTTON(widget))) return;"
            } else {
                ""
            };
            out.push_str(&format!(
                "static void flux__ui_click_{}(GtkWidget *widget, gpointer data) {{{active_guard} (void)widget; (void)data; {} = {next}; flux__ui_refresh(); }}\n",
                element.name,
                ui_state_c_name(&transition.state),
            ));
            continue;
        }
        let ExprKind::Var(function) = &action.value.kind else {
            return Err(diag(
                action.value.span,
                "bootstrap UI event lowering requires a named fn() -> void callback or state transition",
            ));
        };
        let active_guard = if element.kind == "Radio" {
            " if (!gtk_check_button_get_active(GTK_CHECK_BUTTON(widget))) return;"
        } else {
            ""
        };
        out.push_str(&format!(
            "static void flux__ui_click_{}(GtkWidget *widget, gpointer data) {{{active_guard} (void)widget; (void)data; {}(); flux__ui_refresh(); }}\n",
            element.name,
            function_c_name(function),
        ));
    }
    for element in &view.elements {
        if element.kind == "Button" && view_property(element, "shortcut").is_some() {
            out.push_str(&format!(
                "static gboolean flux__ui_shortcut_{}(GtkWidget *widget, GVariant *args, gpointer data) {{ (void)args; (void)data; flux__ui_click_{}(widget, NULL); return TRUE; }}\n",
                element.name, element.name
            ));
        }
    }
    for element in &view.elements {
        for (property_name, callback_name) in [("on_hover", "hover"), ("on_leave", "leave")] {
            let Some(action) = view_property(element, property_name) else {
                continue;
            };
            let body = ui_zero_arg_event_body(action, view, signatures)?;
            if property_name == "on_hover" {
                out.push_str(&format!(
                    "static void flux__ui_{callback_name}_{}(GtkEventControllerMotion *controller, double x, double y, gpointer data) {{ (void)controller; (void)x; (void)y; (void)data; {body} }}\n",
                    element.name,
                ));
            } else {
                out.push_str(&format!(
                    "static void flux__ui_{callback_name}_{}(GtkEventControllerMotion *controller, gpointer data) {{ (void)controller; (void)data; {body} }}\n",
                    element.name,
                ));
            }
        }
        for (property_name, callback_name) in [("on_focus", "focus"), ("on_blur", "blur")] {
            let Some(action) = view_property(element, property_name) else {
                continue;
            };
            let body = ui_zero_arg_event_body(action, view, signatures)?;
            out.push_str(&format!(
                "static void flux__ui_{callback_name}_{}(GtkEventControllerFocus *controller, gpointer data) {{ (void)controller; (void)data; {body} }}\n",
                element.name,
            ));
        }
    }
    out.push('\n');
    if let Some(function) = application_metadata_function(application, "on_exit") {
        out.push_str(&format!(
            "static void flux__ui_shutdown(GtkApplication *application, gpointer data) {{ (void)application; (void)data; {}(); }}\n\n",
            function_c_name(function),
        ));
    }
    out.push_str("static void flux__ui_activate(GtkApplication *application, gpointer data) {\n");
    out.push_str("    (void)data;\n");
    if let Some(function) = application_metadata_function(application, "on_start") {
        out.push_str(&format!("    {}();\n", function_c_name(function)));
    }
    if let Some(theme) = application_metadata_string(application, "theme", signatures) {
        match theme.as_str() {
            "dark" => out.push_str("    g_object_set(gtk_settings_get_default(), \"gtk-application-prefer-dark-theme\", TRUE, NULL);\n"),
            "light" => out.push_str("    g_object_set(gtk_settings_get_default(), \"gtk-application-prefer-dark-theme\", FALSE, NULL);\n"),
            "system" => {}
            _ => unreachable!("application theme validated by type checking"),
        }
    }
    out.push_str("    GtkWidget *window = gtk_application_window_new(application);\n    flux__ui_display_scale = gtk_widget_get_scale_factor(window);\n");
    let title = application_metadata_string(application, "title", signatures)
        .unwrap_or_else(|| view.name.clone());
    out.push_str(&format!(
        "    gtk_window_set_title(GTK_WINDOW(window), {});\n",
        c_string(&title)
    ));
    out.push_str(&format!(
        "    gtk_window_set_default_size(GTK_WINDOW(window), {initial_window_width}, {initial_window_height});\n"
    ));
    out.push_str("    g_signal_connect(window, \"notify::default-width\", G_CALLBACK(flux__ui_window_environment_changed), NULL);\n    g_signal_connect(window, \"notify::default-height\", G_CALLBACK(flux__ui_window_environment_changed), NULL);\n    g_signal_connect(window, \"notify::scale-factor\", G_CALLBACK(flux__ui_window_environment_changed), NULL);\n");
    if let Some(resizable) = application_metadata_bool(application, "resizable", signatures) {
        out.push_str(&format!(
            "    gtk_window_set_resizable(GTK_WINDOW(window), {});\n",
            if resizable { "TRUE" } else { "FALSE" }
        ));
    }
    out.push_str("    GtkWidget *grid = gtk_grid_new();\n");
    if let Some(gap) = view.grid.gap {
        out.push_str(&format!(
            "    gtk_grid_set_column_spacing(GTK_GRID(grid), {gap});\n    gtk_grid_set_row_spacing(GTK_GRID(grid), {gap});\n"
        ));
    }
    let padding = view.grid.padding.unwrap_or(20);
    out.push_str(&format!(
        "    gtk_widget_set_margin_top(grid, {padding});\n    gtk_widget_set_margin_bottom(grid, {padding});\n    gtk_widget_set_margin_start(grid, {padding});\n    gtk_widget_set_margin_end(grid, {padding});\n"
    ));
    if view.grid.scroll.unwrap_or(false) {
        out.push_str("    GtkWidget *scroller = gtk_scrolled_window_new();\n");
        out.push_str("    gtk_scrolled_window_set_policy(GTK_SCROLLED_WINDOW(scroller), GTK_POLICY_AUTOMATIC, GTK_POLICY_AUTOMATIC);\n");
        out.push_str("    gtk_scrolled_window_set_child(GTK_SCROLLED_WINDOW(scroller), grid);\n");
        out.push_str("    gtk_window_set_child(GTK_WINDOW(window), scroller);\n");
    } else {
        out.push_str("    gtk_window_set_child(GTK_WINDOW(window), grid);\n");
    }

    for element in &view.elements {
        let variable = ui_widget_c_name(&element.name);
        match element.kind.as_str() {
            "Text" => {
                let text = match view_property(element, "text") {
                    None => c_string(&element.name),
                    Some(property) => ui_expr_c(&property.value, view, signatures)?,
                };
                out.push_str(&format!("    {variable} = gtk_label_new({text});\n",));
                out.push_str(&format!(
                    "    gtk_widget_set_halign({variable}, GTK_ALIGN_START);\n"
                ));
                let wrap = view_property(element, "wrap")
                    .map(|property| ui_expr_c(&property.value, view, signatures))
                    .transpose()?
                    .unwrap_or_else(|| "true".to_string());
                out.push_str(&format!(
                    "    gtk_label_set_wrap(GTK_LABEL({variable}), {wrap});\n"
                ));
                let size = view_property(element, "size");
                let bold = view_property(element, "bold");
                let italic = view_property(element, "italic");
                let underline = view_property(element, "underline");
                let strikethrough = view_property(element, "strikethrough");
                let font_family = view_property(element, "font_family");
                let letter_spacing = view_property(element, "letter_spacing");
                let line_height_percent = view_property(element, "line_height_percent");
                let color = view_property(element, "color");
                if size.is_some()
                    || bold.is_some()
                    || italic.is_some()
                    || underline.is_some()
                    || strikethrough.is_some()
                    || font_family.is_some()
                    || letter_spacing.is_some()
                    || line_height_percent.is_some()
                    || color.is_some()
                {
                    let attrs = format!("flux__ui_attrs_{}", element.name);
                    out.push_str(&format!(
                        "    PangoAttrList *{attrs} = pango_attr_list_new();\n"
                    ));
                    if let Some(property) = size {
                        let Some(value) = static_expr_i64(&property.value, signatures) else {
                            return Err(diag(
                                property.value.span,
                                "bootstrap Linux Text.size must be a compile-time i64 value",
                            ));
                        };
                        if value <= 0 {
                            return Err(diag(
                                property.value.span,
                                "Text.size must be greater than zero",
                            ));
                        }
                        out.push_str(&format!(
                            "    pango_attr_list_insert({attrs}, pango_attr_size_new({value} * PANGO_SCALE));\n"
                        ));
                    }
                    if let Some(property) = bold {
                        let Some(value) = static_expr_bool(&property.value, signatures) else {
                            return Err(diag(
                                property.value.span,
                                "bootstrap Linux Text.bold must be a compile-time bool value",
                            ));
                        };
                        if value {
                            out.push_str(&format!(
                                "    pango_attr_list_insert({attrs}, pango_attr_weight_new(PANGO_WEIGHT_BOLD));\n"
                            ));
                        }
                    }
                    if let Some(property) = italic {
                        let Some(value) = static_expr_bool(&property.value, signatures) else {
                            return Err(diag(
                                property.value.span,
                                "bootstrap Linux Text.italic must be a compile-time bool value",
                            ));
                        };
                        if value {
                            out.push_str(&format!(
                                "    pango_attr_list_insert({attrs}, pango_attr_style_new(PANGO_STYLE_ITALIC));\n"
                            ));
                        }
                    }
                    if let Some(property) = underline {
                        let Some(value) = static_expr_bool(&property.value, signatures) else {
                            return Err(diag(
                                property.value.span,
                                "bootstrap Linux Text.underline must be a compile-time bool value",
                            ));
                        };
                        if value {
                            out.push_str(&format!(
                                "    pango_attr_list_insert({attrs}, pango_attr_underline_new(PANGO_UNDERLINE_SINGLE));\n"
                            ));
                        }
                    }
                    if let Some(property) = strikethrough {
                        let Some(value) = static_expr_bool(&property.value, signatures) else {
                            return Err(diag(
                                property.value.span,
                                "bootstrap Linux Text.strikethrough must be a compile-time bool value",
                            ));
                        };
                        if value {
                            out.push_str(&format!(
                                "    pango_attr_list_insert({attrs}, pango_attr_strikethrough_new(TRUE));\n"
                            ));
                        }
                    }
                    if let Some(property) = font_family {
                        let Some(value) = static_expr_str(&property.value, signatures) else {
                            return Err(diag(
                                property.value.span,
                                "bootstrap Linux Text.font_family must be a compile-time str value",
                            ));
                        };
                        if value.is_empty() {
                            return Err(diag(
                                property.value.span,
                                "Text.font_family cannot be empty",
                            ));
                        }
                        out.push_str(&format!(
                            "    pango_attr_list_insert({attrs}, pango_attr_family_new({}));\n",
                            c_string(&value)
                        ));
                    }
                    if let Some(property) = letter_spacing {
                        let Some(value) = static_expr_i64(&property.value, signatures) else {
                            return Err(diag(
                                property.value.span,
                                "bootstrap Linux Text.letter_spacing must be a compile-time i64 value",
                            ));
                        };
                        if !(i64::from(i32::MIN) / 1024..=i64::from(i32::MAX) / 1024)
                            .contains(&value)
                        {
                            return Err(diag(
                                property.value.span,
                                "Text.letter_spacing is outside the native Pango range",
                            ));
                        }
                        out.push_str(&format!(
                            "    pango_attr_list_insert({attrs}, pango_attr_letter_spacing_new({value} * PANGO_SCALE));\n"
                        ));
                    }
                    if let Some(property) = line_height_percent {
                        let Some(value) = static_expr_i64(&property.value, signatures) else {
                            return Err(diag(
                                property.value.span,
                                "bootstrap Linux Text.line_height_percent must be a compile-time i64 value",
                            ));
                        };
                        if value <= 0 {
                            return Err(diag(
                                property.value.span,
                                "Text.line_height_percent must be greater than zero",
                            ));
                        }
                        out.push_str(&format!(
                            "    pango_attr_list_insert({attrs}, pango_attr_line_height_new({:.4}));\n",
                            value as f64 / 100.0
                        ));
                    }
                    if let Some(property) = color {
                        let Some(value) = static_expr_str(&property.value, signatures) else {
                            return Err(diag(
                                property.value.span,
                                "bootstrap Linux Text.color must be a compile-time str value",
                            ));
                        };
                        let Some((red, green, blue, alpha)) = parse_hex_rgba(&value) else {
                            return Err(diag(
                                property.value.span,
                                "Text.color must use '#RRGGBB' or '#RRGGBBAA' hexadecimal syntax",
                            ));
                        };
                        out.push_str(&format!(
                            "    pango_attr_list_insert({attrs}, pango_attr_foreground_new({red}, {green}, {blue}));\n"
                        ));
                        if let Some(alpha) = alpha {
                            out.push_str(&format!(
                                "    pango_attr_list_insert({attrs}, pango_attr_foreground_alpha_new({alpha}));\n"
                            ));
                        }
                    }
                    out.push_str(&format!(
                        "    gtk_label_set_attributes(GTK_LABEL({variable}), {attrs});\n    pango_attr_list_unref({attrs});\n"
                    ));
                }
                if let Some(property) = view_property(element, "text_align") {
                    let Some(value) = static_expr_str(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Linux Text.text_align must be a compile-time str value",
                        ));
                    };
                    let justify = match value.as_str() {
                        "left" => "GTK_JUSTIFY_LEFT",
                        "center" => "GTK_JUSTIFY_CENTER",
                        "right" => "GTK_JUSTIFY_RIGHT",
                        "fill" => "GTK_JUSTIFY_FILL",
                        _ => {
                            return Err(diag(
                                property.value.span,
                                "Text.text_align must be one of 'left', 'center', 'right', or 'fill'",
                            ));
                        }
                    };
                    out.push_str(&format!(
                        "    gtk_label_set_justify(GTK_LABEL({variable}), {justify});\n"
                    ));
                }
                if let Some(property) = view_property(element, "wrap_mode") {
                    let Some(value) = static_expr_str(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Linux Text.wrap_mode must be a compile-time str value",
                        ));
                    };
                    let wrap_mode = match value.as_str() {
                        "word" => "PANGO_WRAP_WORD",
                        "char" => "PANGO_WRAP_CHAR",
                        "wordChar" | "word_char" => "PANGO_WRAP_WORD_CHAR",
                        _ => {
                            return Err(diag(
                                property.value.span,
                                "Text.wrapMode must be one of 'word', 'char', or 'wordChar'",
                            ));
                        }
                    };
                    out.push_str(&format!(
                        "    gtk_label_set_wrap_mode(GTK_LABEL({variable}), {wrap_mode});\n"
                    ));
                }
                if let Some(property) = view_property(element, "ellipsize") {
                    let Some(value) = static_expr_str(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Linux Text.ellipsize must be a compile-time str value",
                        ));
                    };
                    let ellipsize = match value.as_str() {
                        "none" => "PANGO_ELLIPSIZE_NONE",
                        "start" => "PANGO_ELLIPSIZE_START",
                        "middle" => "PANGO_ELLIPSIZE_MIDDLE",
                        "end" => "PANGO_ELLIPSIZE_END",
                        _ => {
                            return Err(diag(
                                property.value.span,
                                "Text.ellipsize must be one of 'none', 'start', 'middle', or 'end'",
                            ));
                        }
                    };
                    out.push_str(&format!(
                        "    gtk_label_set_ellipsize(GTK_LABEL({variable}), {ellipsize});\n"
                    ));
                }
                if let Some(property) = view_property(element, "max_lines") {
                    let Some(value) = static_expr_i64(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Linux Text.max_lines must be a compile-time i64 value",
                        ));
                    };
                    if !(1..=i64::from(i32::MAX)).contains(&value) {
                        return Err(diag(
                            property.value.span,
                            "Text.max_lines must be between 1 and 2147483647",
                        ));
                    }
                    out.push_str(&format!(
                        "    gtk_label_set_lines(GTK_LABEL({variable}), {value});\n"
                    ));
                }
                if let Some(property) = view_property(element, "selectable") {
                    let selectable = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    gtk_label_set_selectable(GTK_LABEL({variable}), {selectable});\n"
                    ));
                }
            }
            "TextInput" => {
                let text = match view_property(element, "text") {
                    None => c_string(""),
                    Some(property) => ui_expr_c(&property.value, view, signatures)?,
                };
                out.push_str(&format!("    {variable} = gtk_entry_new();\n"));
                out.push_str(&format!(
                    "    gtk_editable_set_text(GTK_EDITABLE({variable}), {text});\n"
                ));
                if let Some(property) = view_property(element, "placeholder") {
                    let placeholder = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    gtk_entry_set_placeholder_text(GTK_ENTRY({variable}), {placeholder});\n"
                    ));
                }
                if let Some(property) = view_property(element, "enabled") {
                    let enabled = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    gtk_widget_set_sensitive({variable}, {enabled});\n"
                    ));
                }
                if let Some(property) = view_property(element, "password") {
                    let Some(password) = static_expr_bool(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Linux TextInput.password must be a compile-time bool value",
                        ));
                    };
                    if password {
                        out.push_str(&format!(
                            "    gtk_entry_set_visibility(GTK_ENTRY({variable}), FALSE);\n"
                        ));
                    }
                }
                if let Some(property) = view_property(element, "max_length") {
                    let Some(max_length) = static_expr_i64(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Linux TextInput.max_length must be a compile-time i64 value",
                        ));
                    };
                    if !(0..=i64::from(i32::MAX)).contains(&max_length) {
                        return Err(diag(
                            property.value.span,
                            "TextInput.max_length must be between 0 and 2147483647",
                        ));
                    }
                    out.push_str(&format!(
                        "    gtk_entry_set_max_length(GTK_ENTRY({variable}), {max_length});\n"
                    ));
                }
                if view_property(element, "on_change").is_some() {
                    out.push_str(&format!(
                        "    g_signal_connect({variable}, \"changed\", G_CALLBACK(flux__ui_change_{}), NULL);\n",
                        element.name
                    ));
                }
                if view_property(element, "on_submit").is_some() {
                    out.push_str(&format!(
                        "    g_signal_connect({variable}, \"activate\", G_CALLBACK(flux__ui_submit_{}), NULL);\n",
                        element.name
                    ));
                }
                if let Some(property) = view_property(element, "autofocus") {
                    let Some(autofocus) = static_expr_bool(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Linux TextInput.autofocus must be a compile-time bool value",
                        ));
                    };
                    if autofocus {
                        out.push_str(&format!("    gtk_widget_grab_focus({variable});\n"));
                    }
                }
            }
            "Image" => {
                let source = match view_property(element, "source") {
                    Some(property) => ui_expr_c(&property.value, view, signatures)?,
                    None => c_string(""),
                };
                out.push_str(&format!(
                    "    {variable} = gtk_picture_new_for_filename({source});\n"
                ));
                if let Some(property) = view_property(element, "alt") {
                    let alt = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    gtk_picture_set_alternative_text(GTK_PICTURE({variable}), {alt});\n"
                    ));
                }
                if let Some(property) = view_property(element, "can_shrink") {
                    let can_shrink = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    gtk_picture_set_can_shrink(GTK_PICTURE({variable}), {can_shrink});\n"
                    ));
                }
                if let Some(property) = view_property(element, "fit") {
                    let Some(fit) = static_expr_str(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "Image.fit must be a compile-time string",
                        ));
                    };
                    let fit = match fit.as_str() {
                        "fill" => "GTK_CONTENT_FIT_FILL",
                        "contain" => "GTK_CONTENT_FIT_CONTAIN",
                        "cover" => "GTK_CONTENT_FIT_COVER",
                        "scaleDown" | "scale_down" => "GTK_CONTENT_FIT_SCALE_DOWN",
                        _ => {
                            return Err(diag(
                                property.value.span,
                                "Image.fit must be one of 'fill', 'contain', 'cover', or 'scaleDown'",
                            ));
                        }
                    };
                    out.push_str(&format!(
                        "    gtk_picture_set_content_fit(GTK_PICTURE({variable}), {fit});\n"
                    ));
                }
            }
            "Radio" => {
                let label = match view_property(element, "label") {
                    None => c_string(&element.name),
                    Some(property) => ui_expr_c(&property.value, view, signatures)?,
                };
                out.push_str(&format!(
                    "    {variable} = gtk_check_button_new_with_label({label});\n",
                ));
                if let Some(group) = view
                    .elements
                    .iter()
                    .find(|candidate| candidate.kind == "Radio" && candidate.line < element.line)
                {
                    out.push_str(&format!(
                        "    gtk_check_button_set_group(GTK_CHECK_BUTTON({variable}), GTK_CHECK_BUTTON({}));\n",
                        ui_widget_c_name(&group.name)
                    ));
                }
                if let Some(property) = view_property(element, "selected") {
                    let selected = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    gtk_check_button_set_active(GTK_CHECK_BUTTON({variable}), {selected});\n"
                    ));
                }
                if let Some(property) = view_property(element, "enabled") {
                    let enabled = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    gtk_widget_set_sensitive({variable}, {enabled});\n"
                    ));
                }
                if view_property(element, "on_select").is_some() {
                    out.push_str(&format!(
                        "    g_signal_connect({variable}, \"toggled\", G_CALLBACK(flux__ui_click_{}), NULL);\n",
                        element.name
                    ));
                }
            }
            "Toggle" => {
                let label = match view_property(element, "label") {
                    None => c_string(&element.name),
                    Some(property) => ui_expr_c(&property.value, view, signatures)?,
                };
                out.push_str(&format!(
                    "    {variable} = gtk_check_button_new_with_label({label});\n",
                ));
                if let Some(property) = view_property(element, "checked") {
                    let checked = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    gtk_check_button_set_active(GTK_CHECK_BUTTON({variable}), {checked});\n"
                    ));
                }
                if let Some(property) = view_property(element, "enabled") {
                    let enabled = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    gtk_widget_set_sensitive({variable}, {enabled});\n"
                    ));
                }
                if view_property(element, "on_change").is_some() {
                    out.push_str(&format!(
                        "    g_signal_connect({variable}, \"toggled\", G_CALLBACK(flux__ui_click_{}), NULL);\n",
                        element.name
                    ));
                }
            }
            "Button" => {
                let text = match view_property(element, "text") {
                    None => c_string(&element.name),
                    Some(property) => ui_expr_c(&property.value, view, signatures)?,
                };
                out.push_str(&format!(
                    "    {variable} = gtk_button_new_with_label({text});\n",
                ));
                if let Some(property) = view_property(element, "enabled") {
                    let enabled = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    gtk_widget_set_sensitive({variable}, {enabled});\n"
                    ));
                }
                if let Some(property) = view_property(element, "primary") {
                    let Some(primary) = static_expr_bool(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Linux Button.primary must be a compile-time bool value",
                        ));
                    };
                    if primary {
                        out.push_str(&format!(
                            "    gtk_widget_add_css_class({variable}, \"suggested-action\");\n"
                        ));
                    }
                }
                if let Some(property) = view_property(element, "shortcut") {
                    let Some(shortcut) = static_expr_str(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "Button.shortcut must be a compile-time string",
                        ));
                    };
                    let Some(trigger) = gtk_shortcut_trigger(&shortcut) else {
                        return Err(diag(
                            property.value.span,
                            "Button.shortcut must use modifiers Ctrl/Shift/Alt plus one key, for example 'Ctrl+K' or 'Ctrl+Shift+Enter'",
                        ));
                    };
                    let controller = format!("flux__shortcut_controller_{}", element.name);
                    out.push_str(&format!(
                        "    GtkEventController *{controller} = gtk_shortcut_controller_new();\n    gtk_shortcut_controller_set_scope(GTK_SHORTCUT_CONTROLLER({controller}), GTK_SHORTCUT_SCOPE_GLOBAL);\n    gtk_shortcut_controller_add_shortcut(GTK_SHORTCUT_CONTROLLER({controller}), gtk_shortcut_new(gtk_shortcut_trigger_parse_string({}), gtk_callback_action_new(flux__ui_shortcut_{}, NULL, NULL)));\n    gtk_widget_add_controller({variable}, {controller});\n",
                        c_string(&trigger),
                        element.name,
                    ));
                }
                if view_property(element, "on_press").is_some() {
                    out.push_str(&format!(
                        "    g_signal_connect({variable}, \"clicked\", G_CALLBACK(flux__ui_click_{}), NULL);\n",
                        element.name
                    ));
                }
            }
            _ => unreachable!("unsupported app element rejected before lowering"),
        }
        if let Some(property) = view_property(element, "tooltip") {
            let tooltip = ui_expr_c(&property.value, view, signatures)?;
            out.push_str(&format!(
                "    gtk_widget_set_tooltip_text({variable}, {tooltip});\n"
            ));
        }
        if let Some(property) = view_property(element, "accessibility_label") {
            let label = ui_expr_c(&property.value, view, signatures)?;
            out.push_str(&format!(
                "    gtk_accessible_update_property(GTK_ACCESSIBLE({variable}), GTK_ACCESSIBLE_PROPERTY_LABEL, {label}, -1);\n"
            ));
        }
        if let Some(property) = view_property(element, "accessibility_description") {
            let description = ui_expr_c(&property.value, view, signatures)?;
            out.push_str(&format!(
                "    gtk_accessible_update_property(GTK_ACCESSIBLE({variable}), GTK_ACCESSIBLE_PROPERTY_DESCRIPTION, {description}, -1);\n"
            ));
        }
        emit_element_alignment(out, element, &variable, signatures)?;
        emit_element_margins(out, element, &variable, signatures)?;
        emit_element_style(out, element, &variable, signatures)?;
        emit_dynamic_transform_setup(out, element, &variable, signatures)?;
        if let Some(property) = view_property(element, "clip") {
            let clip = ui_expr_c(&property.value, view, signatures)?;
            out.push_str(&format!(
                "    gtk_widget_set_overflow({variable}, ({clip}) ? GTK_OVERFLOW_HIDDEN : GTK_OVERFLOW_VISIBLE);\n"
            ));
        }
        if view_property(element, "on_hover").is_some()
            || view_property(element, "on_leave").is_some()
        {
            let controller = format!("flux__motion_{}", element.name);
            out.push_str(&format!(
                "    GtkEventController *{controller} = gtk_event_controller_motion_new();\n"
            ));
            if view_property(element, "on_hover").is_some() {
                out.push_str(&format!(
                    "    g_signal_connect({controller}, \"enter\", G_CALLBACK(flux__ui_hover_{}), NULL);\n",
                    element.name
                ));
            }
            if view_property(element, "on_leave").is_some() {
                out.push_str(&format!(
                    "    g_signal_connect({controller}, \"leave\", G_CALLBACK(flux__ui_leave_{}), NULL);\n",
                    element.name
                ));
            }
            out.push_str(&format!(
                "    gtk_widget_add_controller({variable}, {controller});\n"
            ));
        }
        if view_property(element, "on_focus").is_some()
            || view_property(element, "on_blur").is_some()
        {
            let controller = format!("flux__focus_{}", element.name);
            out.push_str(&format!(
                "    GtkEventController *{controller} = gtk_event_controller_focus_new();\n"
            ));
            if view_property(element, "on_focus").is_some() {
                out.push_str(&format!(
                    "    g_signal_connect({controller}, \"enter\", G_CALLBACK(flux__ui_focus_{}), NULL);\n",
                    element.name
                ));
            }
            if view_property(element, "on_blur").is_some() {
                out.push_str(&format!(
                    "    g_signal_connect({controller}, \"leave\", G_CALLBACK(flux__ui_blur_{}), NULL);\n",
                    element.name
                ));
            }
            out.push_str(&format!(
                "    gtk_widget_add_controller({variable}, {controller});\n"
            ));
        }
        emit_grid_sizing(out, view, element, &variable, signatures)?;
        out.push_str(&format!(
            "    gtk_grid_attach(GTK_GRID(grid), {variable}, {}, {}, {}, {});\n",
            element.column - 1,
            element.row - 1,
            element.column_span,
            element.row_span,
        ));
    }
    out.push_str("    flux__ui_refresh();\n");
    out.push_str("    gtk_window_present(GTK_WINDOW(window));\n}\n\n");
    let application_id = application_metadata_string(application, "id", signatures)
        .unwrap_or_else(|| "app.flux.bootstrap".to_string());
    out.push_str(&format!(
        "int main(int argc, char **argv) {{\n    GtkApplication *application = gtk_application_new({}, G_APPLICATION_DEFAULT_FLAGS);\n    g_signal_connect(application, \"activate\", G_CALLBACK(flux__ui_activate), NULL);\n",
        c_string(&application_id)
    ));
    if application_metadata_function(application, "on_exit").is_some() {
        out.push_str("    g_signal_connect(application, \"shutdown\", G_CALLBACK(flux__ui_shutdown), NULL);\n");
    }
    out.push_str("    int status = g_application_run(G_APPLICATION(application), argc, argv);\n    g_object_unref(application);\n    return status;\n}\n");
    Ok(())
}

fn internal_name_to_source(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut uppercase_next = false;
    for ch in name.chars() {
        if ch == '_' {
            uppercase_next = true;
        } else if uppercase_next {
            out.push(ch.to_ascii_uppercase());
            uppercase_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn application_metadata_function<'a>(
    application: &'a crate::ast::ApplicationDef,
    name: &str,
) -> Option<&'a str> {
    let source_name = internal_name_to_source(name);
    let field = application
        .metadata
        .iter()
        .find(|field| field.name == source_name || field.name == name)?;
    match &field.value.kind {
        ExprKind::Var(function) => Some(function.as_str()),
        _ => None,
    }
}

fn application_metadata_string(
    application: &crate::ast::ApplicationDef,
    name: &str,
    signatures: &Signatures,
) -> Option<String> {
    let source_name = internal_name_to_source(name);
    let field = application
        .metadata
        .iter()
        .find(|field| field.name == source_name || field.name == name)?;
    match &field.value.kind {
        ExprKind::Str(value) => Some(value.clone()),
        ExprKind::Var(name) => {
            signatures
                .constant(name)
                .and_then(|constant| match &constant.value {
                    ConstantValue::Str(value) => Some(value.clone()),
                    _ => None,
                })
        }
        _ => None,
    }
}

fn application_metadata_bool(
    application: &crate::ast::ApplicationDef,
    name: &str,
    signatures: &Signatures,
) -> Option<bool> {
    let source_name = internal_name_to_source(name);
    let field = application
        .metadata
        .iter()
        .find(|field| field.name == source_name || field.name == name)?;
    static_expr_bool(&field.value, signatures)
}

fn application_metadata_i64(
    application: &crate::ast::ApplicationDef,
    name: &str,
    signatures: &Signatures,
) -> Option<i64> {
    let source_name = internal_name_to_source(name);
    let field = application
        .metadata
        .iter()
        .find(|field| field.name == source_name || field.name == name)?;
    static_expr_i64(&field.value, signatures)
}

fn view_property<'a>(
    element: &'a crate::ast::ViewElement,
    name: &str,
) -> Option<&'a crate::ast::ViewProperty> {
    let source_name = internal_name_to_source(name);
    element
        .properties
        .iter()
        .find(|property| property.name == source_name || property.name == name)
}

fn ui_zero_arg_event_body(
    action: &crate::ast::ViewProperty,
    view: &crate::ast::ViewDef,
    signatures: &Signatures,
) -> Result<String, Diagnostic> {
    if let Some(transition) = &action.transition {
        let next = ui_expr_c(&action.value, view, signatures)?;
        return Ok(format!(
            "{} = {next}; flux__ui_refresh();",
            ui_state_c_name(&transition.state)
        ));
    }
    let ExprKind::Var(function) = &action.value.kind else {
        return Err(diag(
            action.value.span,
            "bootstrap UI event lowering requires a named fn() -> void callback or state transition",
        ));
    };
    Ok(format!(
        "{}(); flux__ui_refresh();",
        function_c_name(function)
    ))
}

fn ui_state_c_name(name: &str) -> String {
    format!("flux__ui_state_{name}")
}

fn ui_derived_c_name(name: &str) -> String {
    format!("flux__ui_derived_{name}")
}

fn ui_widget_c_name(name: &str) -> String {
    format!("flux__ui_{name}")
}

fn static_expr_i64(expr: &Expr, signatures: &Signatures) -> Option<i64> {
    match fold_primitive_expr(expr, &HashMap::new(), signatures)
        .ok()
        .flatten()?
    {
        ConstantValue::I64(value) => Some(value),
        _ => None,
    }
}

fn static_expr_str(expr: &Expr, signatures: &Signatures) -> Option<String> {
    match fold_primitive_expr(expr, &HashMap::new(), signatures)
        .ok()
        .flatten()?
    {
        ConstantValue::Str(value) => Some(value),
        _ => None,
    }
}

fn gtk_shortcut_trigger(value: &str) -> Option<String> {
    let parts = value.split('+').map(str::trim).collect::<Vec<_>>();
    let (key, modifiers) = parts.split_last()?;
    if key.is_empty() || modifiers.is_empty() {
        return None;
    }
    let mut control = false;
    let mut shift = false;
    let mut alt = false;
    for modifier in modifiers {
        match *modifier {
            "Ctrl" if !control => control = true,
            "Shift" if !shift => shift = true,
            "Alt" if !alt => alt = true,
            _ => return None,
        }
    }
    let key = match *key {
        "Enter" => "Return".to_string(),
        "Space" => "space".to_string(),
        "Tab" | "Escape" | "Delete" | "Up" | "Down" | "Left" | "Right" => key.to_string(),
        key if key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric() => {
            key.to_ascii_lowercase()
        }
        _ => return None,
    };
    let mut trigger = String::new();
    if control {
        trigger.push_str("<Control>");
    }
    if shift {
        trigger.push_str("<Shift>");
    }
    if alt {
        trigger.push_str("<Alt>");
    }
    trigger.push_str(&key);
    Some(trigger)
}

fn parse_hex_rgba(value: &str) -> Option<(u16, u16, u16, Option<u16>)> {
    let hex = value.strip_prefix('#')?;
    if !matches!(hex.len(), 6 | 8) || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
    let alpha = (hex.len() == 8)
        .then(|| u8::from_str_radix(&hex[6..8], 16).ok())
        .flatten();
    Some((
        u16::from(red) * 257,
        u16::from(green) * 257,
        u16::from(blue) * 257,
        alpha.map(|value| u16::from(value) * 257),
    ))
}

fn static_expr_bool(expr: &Expr, signatures: &Signatures) -> Option<bool> {
    match fold_primitive_expr(expr, &HashMap::new(), signatures)
        .ok()
        .flatten()?
    {
        ConstantValue::Bool(value) => Some(value),
        _ => None,
    }
}

fn ui_expr_c(
    expr: &Expr,
    view: &crate::ast::ViewDef,
    signatures: &Signatures,
) -> Result<String, Diagnostic> {
    if let Some(value) = fold_primitive_expr(expr, &HashMap::new(), signatures)? {
        return Ok(constant_c_value(&value));
    }
    match &expr.kind {
        ExprKind::Bool(value) => Ok(if *value { "true" } else { "false" }.to_string()),
        ExprKind::Int(value) => Ok(format!("INT64_C({value})")),
        ExprKind::Str(value) => Ok(c_string(value)),
        ExprKind::Var(name) => {
            let environment = match name.as_str() {
                "windowWidth" | "window_width" => Some("flux__ui_window_width"),
                "windowHeight" | "window_height" => Some("flux__ui_window_height"),
                "windowIsLandscape" | "window_is_landscape" => {
                    Some("(flux__ui_window_width > flux__ui_window_height)")
                }
                "windowIsPortrait" | "window_is_portrait" => {
                    Some("(flux__ui_window_height >= flux__ui_window_width)")
                }
                "displayScale" | "display_scale" => Some("flux__ui_display_scale"),
                _ => None,
            };
            if let Some(environment) = environment {
                return Ok(environment.to_string());
            }
            if view.states.iter().any(|state| state.name == *name) {
                return Ok(ui_state_c_name(name));
            }
            if view.derived.iter().any(|derived| derived.name == *name) {
                return Ok(ui_derived_c_name(name));
            }
            let Some(constant) = signatures.constant(name) else {
                return Err(diag(
                    expr.span,
                    "bootstrap Linux dynamic UI expression may reference only view environment, view state, derived view values, or compile-time constants",
                ));
            };
            match &constant.value {
                ConstantValue::Bool(value) => Ok(if *value { "true" } else { "false" }.to_string()),
                ConstantValue::I64(value) => Ok(format!("INT64_C({value})")),
                ConstantValue::Str(value) => Ok(c_string(value)),
            }
        }
        ExprKind::Unary { op, expr: inner } => {
            let inner = ui_expr_c(inner, view, signatures)?;
            Ok(match op {
                UnaryOp::Neg => format!("flux_neg_i64({inner})"),
                UnaryOp::Not => format!("(!({inner}))"),
            })
        }
        ExprKind::Binary { left, op, right } => {
            let left = ui_expr_c(left, view, signatures)?;
            let right = ui_expr_c(right, view, signatures)?;
            match op {
                BinOp::Add => Ok(format!("flux_add_i64({left}, {right})")),
                BinOp::Sub => Ok(format!("flux_sub_i64({left}, {right})")),
                BinOp::Mul => Ok(format!("flux_mul_i64({left}, {right})")),
                BinOp::Div => Ok(format!("flux_div_i64({left}, {right})")),
                _ => Ok(format!("({left} {} {right})", c_operator(*op))),
            }
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => Ok(format!(
            "(({}) ? ({}) : ({}))",
            ui_expr_c(cond, view, signatures)?,
            ui_expr_c(then_expr, view, signatures)?,
            ui_expr_c(else_expr, view, signatures)?,
        )),
        _ => Err(diag(
            expr.span,
            "bootstrap Linux dynamic UI expression currently supports primitive literals, view environment/state/constants, primitive operators, and conditional expressions",
        )),
    }
}

fn emit_ui_refresh(
    out: &mut String,
    view: &crate::ast::ViewDef,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    out.push_str("static void flux__ui_refresh(void) {\n");
    for derived in &view.derived {
        let value = ui_expr_c(&derived.value, view, signatures)?;
        out.push_str(&format!(
            "    {} = {value};\n",
            ui_derived_c_name(&derived.name)
        ));
    }
    for element in &view.elements {
        let widget = ui_widget_c_name(&element.name);
        if let Some(property) = view_property(element, "visible") {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str(&format!(
                "    if ({widget} != NULL) gtk_widget_set_visible({widget}, {value});\n"
            ));
        }
        if let Some(property) = view_property(element, "clip") {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str(&format!(
                "    if ({widget} != NULL) gtk_widget_set_overflow({widget}, ({value}) ? GTK_OVERFLOW_HIDDEN : GTK_OVERFLOW_VISIBLE);\n"
            ));
        }
        if let Some(property) = view_property(element, "tooltip") {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str(&format!(
                "    if ({widget} != NULL) gtk_widget_set_tooltip_text({widget}, {value});\n"
            ));
        }
        if let Some(property) = view_property(element, "accessibility_label") {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str(&format!(
                "    if ({widget} != NULL) gtk_accessible_update_property(GTK_ACCESSIBLE({widget}), GTK_ACCESSIBLE_PROPERTY_LABEL, {value}, -1);\n"
            ));
        }
        if let Some(property) = view_property(element, "accessibility_description") {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str(&format!(
                "    if ({widget} != NULL) gtk_accessible_update_property(GTK_ACCESSIBLE({widget}), GTK_ACCESSIBLE_PROPERTY_DESCRIPTION, {value}, -1);\n"
            ));
        }
        emit_dynamic_transform_refresh(out, element, view, signatures)?;
        match element.kind.as_str() {
            "Text" => {
                if let Some(property) = view_property(element, "text") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_label_set_text(GTK_LABEL({widget}), {value});\n"
                    ));
                }
                if let Some(property) = view_property(element, "selectable") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_label_set_selectable(GTK_LABEL({widget}), {value});\n"
                    ));
                }
                if let Some(property) = view_property(element, "wrap") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_label_set_wrap(GTK_LABEL({widget}), {value});\n"
                    ));
                }
            }
            "TextInput" => {
                if let Some(property) = view_property(element, "placeholder") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_entry_set_placeholder_text(GTK_ENTRY({widget}), {value});\n"
                    ));
                }
                if let Some(property) = view_property(element, "enabled") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_widget_set_sensitive({widget}, {value});\n"
                    ));
                }
            }
            "Image" => {
                if let Some(property) = view_property(element, "source") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_picture_set_filename(GTK_PICTURE({widget}), {value});\n"
                    ));
                }
                if let Some(property) = view_property(element, "alt") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_picture_set_alternative_text(GTK_PICTURE({widget}), {value});\n"
                    ));
                }
                if let Some(property) = view_property(element, "can_shrink") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_picture_set_can_shrink(GTK_PICTURE({widget}), {value});\n"
                    ));
                }
            }
            "Radio" => {
                if let Some(property) = view_property(element, "label") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_check_button_set_label(GTK_CHECK_BUTTON({widget}), {value});\n"
                    ));
                }
                if let Some(property) = view_property(element, "selected") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_check_button_set_active(GTK_CHECK_BUTTON({widget}), {value});\n"
                    ));
                }
                if let Some(property) = view_property(element, "enabled") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_widget_set_sensitive({widget}, {value});\n"
                    ));
                }
            }
            "Toggle" => {
                if let Some(property) = view_property(element, "label") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_check_button_set_label(GTK_CHECK_BUTTON({widget}), {value});\n"
                    ));
                }
                if let Some(property) = view_property(element, "checked") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_check_button_set_active(GTK_CHECK_BUTTON({widget}), {value});\n"
                    ));
                }
                if let Some(property) = view_property(element, "enabled") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_widget_set_sensitive({widget}, {value});\n"
                    ));
                }
            }
            "Button" => {
                if let Some(property) = view_property(element, "text") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_button_set_label(GTK_BUTTON({widget}), {value});\n"
                    ));
                }
                if let Some(property) = view_property(element, "enabled") {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    if ({widget} != NULL) gtk_widget_set_sensitive({widget}, {value});\n"
                    ));
                }
            }
            _ => {}
        }
    }
    out.push_str("}\n\n");
    Ok(())
}

fn bootstrap_window_size(view: &crate::ast::ViewDef) -> (u32, u32) {
    fn tracks_size(tracks: &[crate::ast::GridTrack], fallback: u32) -> u32 {
        let fixed = tracks
            .iter()
            .map(|track| match track {
                crate::ast::GridTrack::Units(value) => *value,
                crate::ast::GridTrack::Fraction(_) | crate::ast::GridTrack::Auto => fallback,
            })
            .sum::<u32>();
        fixed.max(fallback)
    }
    (
        tracks_size(&view.grid.columns, 220)
            .saturating_add(view.grid.padding.unwrap_or(20).saturating_mul(2)),
        tracks_size(&view.grid.rows, 90)
            .saturating_add(view.grid.padding.unwrap_or(20).saturating_mul(2)),
    )
}

fn emit_element_alignment(
    out: &mut String,
    element: &crate::ast::ViewElement,
    variable: &str,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    for (property_name, setter) in [("align_x", "halign"), ("align_y", "valign")] {
        let Some(property) = view_property(element, property_name) else {
            continue;
        };
        let Some(value) = static_expr_str(&property.value, signatures) else {
            return Err(diag(
                property.value.span,
                &format!("{property_name} must be a compile-time string"),
            ));
        };
        let alignment = match value.as_str() {
            "start" => "GTK_ALIGN_START",
            "center" => "GTK_ALIGN_CENTER",
            "end" => "GTK_ALIGN_END",
            "fill" => "GTK_ALIGN_FILL",
            _ => {
                return Err(diag(
                    property.value.span,
                    &format!("{property_name} must be one of 'start', 'center', 'end', or 'fill'"),
                ));
            }
        };
        out.push_str(&format!(
            "    gtk_widget_set_{setter}({variable}, {alignment});\n"
        ));
    }
    Ok(())
}

fn emit_element_style(
    out: &mut String,
    element: &crate::ast::ViewElement,
    variable: &str,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    let mut declarations = Vec::new();
    for (property_name, css_name) in [
        ("background_color", "background-color"),
        ("border_color", "border-color"),
    ] {
        let Some(property) = view_property(element, property_name) else {
            continue;
        };
        let Some(value) = static_expr_str(&property.value, signatures) else {
            return Err(diag(
                property.value.span,
                &format!("{property_name} must be a compile-time string"),
            ));
        };
        if parse_hex_rgba(&value).is_none() {
            return Err(diag(
                property.value.span,
                &format!("{property_name} must use '#RRGGBB' or '#RRGGBBAA' hexadecimal syntax"),
            ));
        }
        declarations.push(format!("{css_name}: {value};"));
    }
    let padding = static_non_negative_style_i64(element, "padding", signatures)?;
    for (property_name, css_name) in [
        ("padding_top", "padding-top"),
        ("padding_bottom", "padding-bottom"),
        ("padding_start", "padding-left"),
        ("padding_end", "padding-right"),
    ] {
        let value = if view_property(element, property_name).is_some() {
            static_non_negative_style_i64(element, property_name, signatures)?
        } else {
            padding
        };
        if let Some(value) = value {
            declarations.push(format!("{css_name}: {value}px;"));
        }
    }
    let border_width = static_non_negative_style_i64(element, "border_width", signatures)?;
    let mut has_visible_border_width = border_width.is_some_and(|value| value > 0);
    if let Some(value) = border_width {
        declarations.push(format!("border-width: {value}px;"));
    }
    for (property_name, css_name) in [
        ("border_top_width", "border-top-width"),
        ("border_bottom_width", "border-bottom-width"),
        ("border_start_width", "border-left-width"),
        ("border_end_width", "border-right-width"),
    ] {
        if let Some(value) = static_non_negative_style_i64(element, property_name, signatures)? {
            has_visible_border_width |= value > 0;
            declarations.push(format!("{css_name}: {value}px;"));
        }
    }
    if let Some(property) = view_property(element, "border_style") {
        let Some(value) = static_expr_str(&property.value, signatures) else {
            return Err(diag(
                property.value.span,
                "border_style must be a compile-time string",
            ));
        };
        if !matches!(
            value.as_str(),
            "none" | "solid" | "dashed" | "dotted" | "double"
        ) {
            return Err(diag(
                property.value.span,
                "border_style must be one of 'none', 'solid', 'dashed', 'dotted', or 'double'",
            ));
        }
        declarations.push(format!("border-style: {value};"));
    } else if has_visible_border_width {
        declarations.push("border-style: solid;".to_string());
    }

    let radius = static_non_negative_style_i64(element, "radius", signatures)?;
    if let Some(value) = radius {
        declarations.push(format!("border-radius: {value}px;"));
    }
    for (property_name, css_name) in [
        ("radius_top_left", "border-top-left-radius"),
        ("radius_top_right", "border-top-right-radius"),
        ("radius_bottom_left", "border-bottom-left-radius"),
        ("radius_bottom_right", "border-bottom-right-radius"),
    ] {
        if let Some(value) = static_non_negative_style_i64(element, property_name, signatures)? {
            declarations.push(format!("{css_name}: {value}px;"));
        }
    }
    let shadow_color = view_property(element, "shadow_color")
        .map(|property| {
            let Some(value) = static_expr_str(&property.value, signatures) else {
                return Err(diag(
                    property.value.span,
                    "shadow_color must be a compile-time string",
                ));
            };
            if parse_hex_rgba(&value).is_none() {
                return Err(diag(
                    property.value.span,
                    "shadow_color must use '#RRGGBB' or '#RRGGBBAA' hexadecimal syntax",
                ));
            }
            Ok(value)
        })
        .transpose()?;
    let shadow_blur = static_style_i64(element, "shadow_blur", signatures)?.unwrap_or(0);
    let shadow_offset_x = static_style_i64(element, "shadow_offset_x", signatures)?.unwrap_or(0);
    let shadow_offset_y = static_style_i64(element, "shadow_offset_y", signatures)?.unwrap_or(0);
    if shadow_blur < 0 {
        let span = view_property(element, "shadow_blur")
            .expect("shadow blur exists when negative")
            .value
            .span;
        return Err(diag(span, "shadow_blur must be non-negative"));
    }
    if shadow_color.is_some() || shadow_blur != 0 || shadow_offset_x != 0 || shadow_offset_y != 0 {
        declarations.push(format!(
            "box-shadow: {shadow_offset_x}px {shadow_offset_y}px {shadow_blur}px {};",
            shadow_color.unwrap_or_else(|| "#00000080".to_string())
        ));
    }
    let has_transform = element_has_transform(element);
    if has_transform && !element_has_dynamic_transform(element, signatures) {
        let translate_x = static_style_i64(element, "translate_x", signatures)?.unwrap_or(0);
        let translate_y = static_style_i64(element, "translate_y", signatures)?.unwrap_or(0);
        let rotate_degrees = static_style_i64(element, "rotate_degrees", signatures)?.unwrap_or(0);
        let scale_percent = static_style_i64(element, "scale_percent", signatures)?.unwrap_or(100);
        let scale_x_percent =
            static_style_i64(element, "scale_x_percent", signatures)?.unwrap_or(scale_percent);
        let scale_y_percent =
            static_style_i64(element, "scale_y_percent", signatures)?.unwrap_or(scale_percent);
        let skew_x_degrees = static_style_i64(element, "skew_x_degrees", signatures)?.unwrap_or(0);
        let skew_y_degrees = static_style_i64(element, "skew_y_degrees", signatures)?.unwrap_or(0);
        declarations.push(format!(
            "transform: translate({translate_x}px, {translate_y}px) rotate({rotate_degrees}deg) scale({:.2}, {:.2}) skewX({skew_x_degrees}deg) skewY({skew_y_degrees}deg);",
            scale_x_percent as f64 / 100.0,
            scale_y_percent as f64 / 100.0,
        ));
        if view_property(element, "transform_origin_x_percent").is_some()
            || view_property(element, "transform_origin_y_percent").is_some()
        {
            let origin_x =
                static_style_i64(element, "transform_origin_x_percent", signatures)?.unwrap_or(50);
            let origin_y =
                static_style_i64(element, "transform_origin_y_percent", signatures)?.unwrap_or(50);
            declarations.push(format!("transform-origin: {origin_x}% {origin_y}%;"));
        }
    }
    let transition_ms = static_non_negative_style_i64(element, "transition_ms", signatures)?;
    let transition_delay_ms =
        static_non_negative_style_i64(element, "transition_delay_ms", signatures)?;
    let transition_easing = view_property(element, "transition_easing")
        .map(|property| {
            let Some(value) = static_expr_str(&property.value, signatures) else {
                return Err(diag(
                    property.value.span,
                    "transition_easing must be a compile-time string",
                ));
            };
            let css_value = match value.as_str() {
                "linear" => "linear",
                "ease" => "ease",
                "easeIn" | "ease_in" => "ease-in",
                "easeOut" | "ease_out" => "ease-out",
                "easeInOut" | "ease_in_out" => "ease-in-out",
                _ => {
                    return Err(diag(
                        property.value.span,
                        "transitionEasing must be one of 'linear', 'ease', 'easeIn', 'easeOut', or 'easeInOut'",
                    ));
                }
            };
            Ok(css_value)
        })
        .transpose()?;
    if (transition_delay_ms.is_some() || transition_easing.is_some()) && transition_ms.is_none() {
        let property = view_property(element, "transition_delay_ms")
            .or_else(|| view_property(element, "transition_easing"))
            .expect("a transition option exists");
        return Err(diag(
            property.value.span,
            "transition_delay_ms and transition_easing require transition_ms",
        ));
    }
    if let Some(duration) = transition_ms {
        declarations.push("transition-property: all;".to_string());
        declarations.push(format!("transition-duration: {duration}ms;"));
        if let Some(delay) = transition_delay_ms {
            declarations.push(format!("transition-delay: {delay}ms;"));
        }
        declarations.push(format!(
            "transition-timing-function: {};",
            transition_easing.unwrap_or("ease")
        ));
    }
    if declarations.is_empty() {
        return Ok(());
    }
    let widget_name = format!("flux-ui-{}", element.name);
    let provider = format!("flux__style_{}", element.name);
    let css = if transition_ms.is_some() {
        format!(
            "#{widget_name} {{ {} }} @media (prefers-reduced-motion: reduce) {{ #{widget_name} {{ transition-duration: 0ms; transition-delay: 0ms; }} }}",
            declarations.join(" ")
        )
    } else {
        format!("#{widget_name} {{ {} }}", declarations.join(" "))
    };
    out.push_str(&format!(
        "    gtk_widget_set_name({variable}, {});\n    GtkCssProvider *{provider} = gtk_css_provider_new();\n    gtk_css_provider_load_from_data({provider}, {}, -1);\n    gtk_style_context_add_provider_for_display(gtk_widget_get_display({variable}), GTK_STYLE_PROVIDER({provider}), GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);\n",
        c_string(&widget_name),
        c_string(&css),
    ));
    if transition_ms.is_some() {
        let settings = format!("flux__settings_{}", element.name);
        out.push_str(&format!(
            "    GtkSettings *{settings} = gtk_settings_get_default();\n    if ({settings} != NULL) g_object_bind_property({settings}, \"gtk-interface-reduced-motion\", {provider}, \"prefers-reduced-motion\", G_BINDING_SYNC_CREATE);\n"
        ));
    }
    out.push_str(&format!("    g_object_unref({provider});\n"));
    Ok(())
}

const TRANSFORM_VIEW_PROPERTIES: &[&str] = &[
    "translate_x",
    "translate_y",
    "rotate_degrees",
    "scale_percent",
    "scale_x_percent",
    "scale_y_percent",
    "skew_x_degrees",
    "skew_y_degrees",
    "transform_origin_x_percent",
    "transform_origin_y_percent",
];

fn element_has_transform(element: &crate::ast::ViewElement) -> bool {
    TRANSFORM_VIEW_PROPERTIES
        .iter()
        .any(|name| view_property(element, name).is_some())
}

fn element_has_dynamic_transform(
    element: &crate::ast::ViewElement,
    signatures: &Signatures,
) -> bool {
    TRANSFORM_VIEW_PROPERTIES
        .iter()
        .filter_map(|name| view_property(element, name))
        .any(|property| static_expr_i64(&property.value, signatures).is_none())
}

fn ui_transform_provider_c_name(name: &str) -> String {
    format!("flux__transform_style_{name}")
}

fn emit_dynamic_transform_setup(
    out: &mut String,
    element: &crate::ast::ViewElement,
    variable: &str,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    if !element_has_dynamic_transform(element, signatures) {
        return Ok(());
    }
    let widget_name = format!("flux-ui-{}", element.name);
    let provider = ui_transform_provider_c_name(&element.name);
    out.push_str(&format!(
        "    gtk_widget_set_name({variable}, {});\n    if ({provider} == NULL) {{\n        {provider} = gtk_css_provider_new();\n        gtk_style_context_add_provider_for_display(gtk_widget_get_display({variable}), GTK_STYLE_PROVIDER({provider}), GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);\n    }}\n",
        c_string(&widget_name),
    ));
    Ok(())
}

fn emit_dynamic_transform_refresh(
    out: &mut String,
    element: &crate::ast::ViewElement,
    view: &crate::ast::ViewDef,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    if !element_has_dynamic_transform(element, signatures) {
        return Ok(());
    }
    let value = |property_name: &str, fallback: &str| -> Result<String, Diagnostic> {
        view_property(element, property_name)
            .map(|property| ui_expr_c(&property.value, view, signatures))
            .unwrap_or_else(|| Ok(fallback.to_string()))
    };
    let translate_x = value("translate_x", "0")?;
    let translate_y = value("translate_y", "0")?;
    let rotate_degrees = value("rotate_degrees", "0")?;
    let scale_percent = value("scale_percent", "100")?;
    let scale_x_percent = view_property(element, "scale_x_percent")
        .map(|property| ui_expr_c(&property.value, view, signatures))
        .unwrap_or_else(|| Ok(scale_percent.clone()))?;
    let scale_y_percent = view_property(element, "scale_y_percent")
        .map(|property| ui_expr_c(&property.value, view, signatures))
        .unwrap_or_else(|| Ok(scale_percent.clone()))?;
    let skew_x_degrees = value("skew_x_degrees", "0")?;
    let skew_y_degrees = value("skew_y_degrees", "0")?;
    let origin_x = value("transform_origin_x_percent", "50")?;
    let origin_y = value("transform_origin_y_percent", "50")?;
    let provider = ui_transform_provider_c_name(&element.name);
    let widget_name = format!("flux-ui-{}", element.name);
    let css_var = format!("flux__transform_css_{}", element.name);
    out.push_str(&format!(
        "    if ({provider} != NULL) {{\n        gchar *{css_var} = g_strdup_printf(\"#{} {{ transform: translate(%lldpx, %lldpx) rotate(%llddeg) scale(%.2f, %.2f) skewX(%llddeg) skewY(%llddeg); transform-origin: %lld%% %lld%%; }}\", (long long)({translate_x}), (long long)({translate_y}), (long long)({rotate_degrees}), ((double)({scale_x_percent}) / 100.0), ((double)({scale_y_percent}) / 100.0), (long long)({skew_x_degrees}), (long long)({skew_y_degrees}), (long long)({origin_x}), (long long)({origin_y}));\n        gtk_css_provider_load_from_data({provider}, {css_var}, -1);\n        g_free({css_var});\n    }}\n",
        widget_name,
    ));
    Ok(())
}

fn static_non_negative_style_i64(
    element: &crate::ast::ViewElement,
    property_name: &str,
    signatures: &Signatures,
) -> Result<Option<i64>, Diagnostic> {
    let value = static_style_i64(element, property_name, signatures)?;
    if let Some(value) = value
        && value < 0
    {
        let span = view_property(element, property_name)
            .expect("style value exists when validation runs")
            .value
            .span;
        return Err(diag(span, &format!("{property_name} must be non-negative")));
    }
    Ok(value)
}

fn static_style_i64(
    element: &crate::ast::ViewElement,
    property_name: &str,
    signatures: &Signatures,
) -> Result<Option<i64>, Diagnostic> {
    let Some(property) = view_property(element, property_name) else {
        return Ok(None);
    };
    let Some(value) = static_expr_i64(&property.value, signatures) else {
        return Err(diag(
            property.value.span,
            &format!("{property_name} must be a compile-time i64 value"),
        ));
    };
    if !(i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&value) {
        return Err(diag(
            property.value.span,
            &format!("{property_name} must fit within a 32-bit signed integer"),
        ));
    }
    Ok(Some(value))
}

fn emit_element_margins(
    out: &mut String,
    element: &crate::ast::ViewElement,
    variable: &str,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    let base = view_property(element, "margin")
        .map(|property| element_margin_value(property, "margin", signatures))
        .transpose()?;
    for (property_name, setter) in [
        ("margin_top", "top"),
        ("margin_bottom", "bottom"),
        ("margin_start", "start"),
        ("margin_end", "end"),
    ] {
        let value = if let Some(property) = view_property(element, property_name) {
            Some(element_margin_value(property, property_name, signatures)?)
        } else {
            base
        };
        if let Some(value) = value {
            out.push_str(&format!(
                "    gtk_widget_set_margin_{setter}({variable}, {value});\n"
            ));
        }
    }
    Ok(())
}

fn element_margin_value(
    property: &crate::ast::ViewProperty,
    property_name: &str,
    signatures: &Signatures,
) -> Result<i64, Diagnostic> {
    let Some(value) = static_expr_i64(&property.value, signatures) else {
        return Err(diag(
            property.value.span,
            &format!("{property_name} must be a compile-time i64 value"),
        ));
    };
    if !(0..=i64::from(i32::MAX)).contains(&value) {
        return Err(diag(
            property.value.span,
            &format!("{property_name} must be between 0 and 2147483647"),
        ));
    }
    Ok(value)
}

fn emit_grid_sizing(
    out: &mut String,
    view: &crate::ast::ViewDef,
    element: &crate::ast::ViewElement,
    variable: &str,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    let column_index = element.column.saturating_sub(1) as usize;
    let row_index = element.row.saturating_sub(1) as usize;
    if view
        .grid
        .columns
        .get(column_index)
        .is_some_and(|track| matches!(track, crate::ast::GridTrack::Fraction(_)))
    {
        out.push_str(&format!("    gtk_widget_set_hexpand({variable}, TRUE);\n"));
    }
    if view
        .grid
        .rows
        .get(row_index)
        .is_some_and(|track| matches!(track, crate::ast::GridTrack::Fraction(_)))
    {
        out.push_str(&format!("    gtk_widget_set_vexpand({variable}, TRUE);\n"));
    }
    let fixed_width = view
        .grid
        .columns
        .get(column_index)
        .and_then(|track| match track {
            crate::ast::GridTrack::Units(value) => Some(i64::from(*value)),
            _ => None,
        });
    let fixed_height = view.grid.rows.get(row_index).and_then(|track| match track {
        crate::ast::GridTrack::Units(value) => Some(i64::from(*value)),
        _ => None,
    });
    let min_width = static_minimum_size(element, "min_width", signatures)?;
    let min_height = static_minimum_size(element, "min_height", signatures)?;
    let width = match (fixed_width, min_width) {
        (Some(fixed), Some(minimum)) => Some(fixed.max(minimum)),
        (fixed, minimum) => fixed.or(minimum),
    };
    let height = match (fixed_height, min_height) {
        (Some(fixed), Some(minimum)) => Some(fixed.max(minimum)),
        (fixed, minimum) => fixed.or(minimum),
    };
    if width.is_some() || height.is_some() {
        out.push_str(&format!(
            "    gtk_widget_set_size_request({variable}, {}, {});\n",
            width.unwrap_or(-1),
            height.unwrap_or(-1),
        ));
    }
    Ok(())
}

fn static_minimum_size(
    element: &crate::ast::ViewElement,
    property_name: &str,
    signatures: &Signatures,
) -> Result<Option<i64>, Diagnostic> {
    let Some(property) = view_property(element, property_name) else {
        return Ok(None);
    };
    let Some(value) = static_expr_i64(&property.value, signatures) else {
        return Err(diag(
            property.value.span,
            &format!("{property_name} must be a compile-time i64 value"),
        ));
    };
    if !(1..=i64::from(i32::MAX)).contains(&value) {
        return Err(diag(
            property.value.span,
            &format!("{property_name} must be between 1 and {}", i32::MAX),
        ));
    }
    Ok(Some(value))
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

fn anonymous_function_c_name(span: SourceSpan) -> String {
    format!(
        "flux__lambda_{}_{}_{}",
        span.source_id.value(),
        span.line,
        span.column
    )
}

fn anonymous_function_prototype(
    expr: &Expr,
    signatures: &Signatures,
) -> Result<String, Diagnostic> {
    let ExprKind::AnonymousFunction { params, .. } = &expr.kind else {
        return Err(diag(
            expr.span,
            "expected anonymous function during code generation",
        ));
    };
    let ty = type_of_expr(expr, &HashMap::new(), signatures)?;
    let Type::Function {
        params: param_types,
        returns,
    } = ty
    else {
        return Err(diag(
            expr.span,
            "anonymous function did not produce a function type",
        ));
    };
    if returns.len() > 1 {
        return Err(diag(
            expr.span,
            "anonymous functions currently support zero or one return value",
        ));
    }
    let ret = returns
        .first()
        .map(|ty| c_type(ty, signatures))
        .unwrap_or_else(|| "void".to_string());
    let params_text = if params.is_empty() {
        "void".to_string()
    } else {
        params
            .iter()
            .zip(param_types.iter())
            .map(|(param, ty)| format!("{} {}", c_type(ty, signatures), local_c_name(&param.name)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    Ok(format!(
        "{ret} {}({params_text})",
        anonymous_function_c_name(expr.span)
    ))
}

fn emit_anonymous_function(
    out: &mut String,
    expr: &Expr,
    signatures: &Signatures,
    source_paths: &HashMap<SourceId, String>,
) -> Result<(), Diagnostic> {
    let ExprKind::AnonymousFunction { params, body, .. } = &expr.kind else {
        return Err(diag(
            expr.span,
            "expected anonymous function during code generation",
        ));
    };
    let ty = type_of_expr(expr, &HashMap::new(), signatures)?;
    let Type::Function { returns, .. } = ty else {
        return Err(diag(
            expr.span,
            "anonymous function did not produce a function type",
        ));
    };
    emit_source_line(out, expr.span, source_paths);
    out.push_str(&anonymous_function_prototype(expr, signatures)?);
    out.push_str(" {\n");
    let mut env = HashMap::new();
    for param in params {
        env.insert(param.name.clone(), signatures.canonical_type(&param.ty));
    }
    let body = emit_expr(body, &env, signatures)?;
    if returns.is_empty() {
        out.push_str(&format!("    {};\n", body.code));
    } else {
        out.push_str(&format!("    return {};\n", body.code));
    }
    out.push_str("}\n");
    Ok(())
}

type FunctionIrCache = HashMap<String, crate::ir::ControlFlowGraph>;

fn build_function_ir_cache(program: &Program, signatures: &Signatures) -> FunctionIrCache {
    program
        .functions
        .iter()
        .map(|function| {
            (
                function.name.clone(),
                crate::ir::ControlFlowGraph::from_function(function, signatures),
            )
        })
        .collect()
}

fn runtime_views(program: &Program) -> impl Iterator<Item = &crate::ast::ViewDef> {
    program.application.iter().filter_map(|application| {
        program
            .views
            .iter()
            .find(|view| view.name == application.view_name)
    })
}

fn collect_interface_names_from_ir(
    function: &Function,
    signatures: &Signatures,
    function_ir: &FunctionIrCache,
    reachable: &mut HashSet<String>,
    pending: &mut Vec<String>,
) {
    let Some(cfg) = function_ir.get(&function.name) else {
        return;
    };
    for node in cfg.nodes().iter().filter(|node| cfg.is_reachable(node.id)) {
        for definition in &node.definitions {
            collect_interface_names_from_type(&definition.ty, signatures, reachable, pending);
        }
    }
    for value in cfg
        .values()
        .iter()
        .filter(|value| cfg.is_value_reachable(value.id))
    {
        collect_interface_names_from_type(&value.ty, signatures, reachable, pending);
        if let crate::ir::ControlFlowValueKind::InterfaceDispatch { interface, .. } = &value.kind {
            enqueue_interface_name(interface, signatures, reachable, pending);
        }
    }
}

fn reachable_interface_names(
    program: &Program,
    signatures: &Signatures,
    reachable_functions: &HashSet<String>,
    reachable_value_types: &HashSet<String>,
    function_ir: &FunctionIrCache,
) -> HashSet<String> {
    let mut reachable = HashSet::new();
    let mut pending = Vec::new();

    for definition in &program.interfaces {
        if definition.public {
            enqueue_interface_name(&definition.name, signatures, &mut reachable, &mut pending);
        }
    }
    for alias in &program.aliases {
        if alias.public {
            collect_interface_names_from_type(
                &alias.target,
                signatures,
                &mut reachable,
                &mut pending,
            );
        }
    }
    for definition in &program.structs {
        if !reachable_value_types.contains(&definition.name) {
            continue;
        }
        for field in &definition.fields {
            collect_interface_names_from_type(&field.ty, signatures, &mut reachable, &mut pending);
        }
    }
    for definition in &program.enums {
        if !reachable_value_types.contains(&definition.name) {
            continue;
        }
        for variant in &definition.variants {
            for payload in &variant.payloads {
                collect_interface_names_from_type(
                    &payload.ty,
                    signatures,
                    &mut reachable,
                    &mut pending,
                );
            }
        }
    }
    if let Some(application) = &program.application {
        for field in &application.metadata {
            collect_interface_names_from_expr(
                &field.value,
                signatures,
                &mut reachable,
                &mut pending,
            );
        }
    }
    for view in runtime_views(program) {
        for param in &view.params {
            collect_interface_names_from_type(&param.ty, signatures, &mut reachable, &mut pending);
            if let Some(default) = &param.default {
                collect_interface_names_from_expr(
                    default,
                    signatures,
                    &mut reachable,
                    &mut pending,
                );
            }
        }
        for state in &view.states {
            collect_interface_names_from_type(&state.ty, signatures, &mut reachable, &mut pending);
            collect_interface_names_from_expr(
                &state.initial,
                signatures,
                &mut reachable,
                &mut pending,
            );
        }
        for derived in &view.derived {
            collect_interface_names_from_type(
                &derived.ty,
                signatures,
                &mut reachable,
                &mut pending,
            );
            collect_interface_names_from_expr(
                &derived.value,
                signatures,
                &mut reachable,
                &mut pending,
            );
        }
        for element in &view.elements {
            for property in &element.properties {
                collect_interface_names_from_expr(
                    &property.value,
                    signatures,
                    &mut reachable,
                    &mut pending,
                );
            }
        }
    }
    for function in &program.functions {
        if !reachable_functions.contains(&function.name) {
            continue;
        }
        for param in &function.params {
            collect_interface_names_from_type(&param.ty, signatures, &mut reachable, &mut pending);
            if let Some(default) = &param.default {
                collect_interface_names_from_expr(
                    default,
                    signatures,
                    &mut reachable,
                    &mut pending,
                );
            }
        }
        for ty in &function.returns {
            collect_interface_names_from_type(ty, signatures, &mut reachable, &mut pending);
        }
        collect_interface_names_from_ir(
            function,
            signatures,
            function_ir,
            &mut reachable,
            &mut pending,
        );
    }

    while let Some(name) = pending.pop() {
        let Some(definition) = program
            .interfaces
            .iter()
            .find(|definition| definition.name == name)
        else {
            continue;
        };
        for parent in &definition.parents {
            enqueue_interface_name(&parent.name, signatures, &mut reachable, &mut pending);
        }
        for function in &definition.functions {
            for param in &function.params {
                collect_interface_names_from_type(
                    &param.ty,
                    signatures,
                    &mut reachable,
                    &mut pending,
                );
            }
            for ty in &function.returns {
                collect_interface_names_from_type(ty, signatures, &mut reachable, &mut pending);
            }
        }
    }

    reachable
}

fn enqueue_interface_name(
    name: &str,
    signatures: &Signatures,
    reachable: &mut HashSet<String>,
    pending: &mut Vec<String>,
) {
    if signatures.interface(name).is_some() && reachable.insert(name.to_string()) {
        pending.push(name.to_string());
    }
}

fn collect_interface_names_from_type(
    ty: &Type,
    signatures: &Signatures,
    reachable: &mut HashSet<String>,
    pending: &mut Vec<String>,
) {
    match signatures.canonical_type(ty) {
        Type::Named(name) => {
            enqueue_interface_name(&name, signatures, reachable, pending);
        }
        Type::List(element) => {
            collect_interface_names_from_type(&element, signatures, reachable, pending);
        }
        Type::Function { params, returns } => {
            for ty in params.iter().chain(&returns) {
                collect_interface_names_from_type(ty, signatures, reachable, pending);
            }
        }
        Type::I64 | Type::Bool | Type::Str | Type::Error | Type::Void => {}
    }
}

fn collect_interface_names_from_expr(
    expr: &Expr,
    signatures: &Signatures,
    reachable: &mut HashSet<String>,
    pending: &mut Vec<String>,
) {
    match &expr.kind {
        ExprKind::AnonymousFunction {
            params,
            return_type,
            body,
        } => {
            for param in params {
                collect_interface_names_from_type(&param.ty, signatures, reachable, pending);
            }
            if let Some(return_type) = return_type {
                collect_interface_names_from_type(return_type, signatures, reachable, pending);
            }
            collect_interface_names_from_expr(body, signatures, reachable, pending);
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } => {
            enqueue_interface_name(name, signatures, reachable, pending);
            for arg in args {
                collect_interface_names_from_expr(arg, signatures, reachable, pending);
            }
            for arg in named_args {
                collect_interface_names_from_expr(&arg.value, signatures, reachable, pending);
            }
        }
        ExprKind::QualifiedCall {
            args, named_args, ..
        } => {
            for arg in args {
                collect_interface_names_from_expr(arg, signatures, reachable, pending);
            }
            for arg in named_args {
                collect_interface_names_from_expr(&arg.value, signatures, reachable, pending);
            }
        }
        ExprKind::ShellCall { args, .. } => {
            for arg in args {
                collect_interface_names_from_expr(arg, signatures, reachable, pending);
            }
        }
        ExprKind::Pipe { input, args, .. } => {
            collect_interface_names_from_expr(input, signatures, reachable, pending);
            for arg in args {
                collect_interface_names_from_expr(arg, signatures, reachable, pending);
            }
        }
        ExprKind::List(items) => {
            for item in items {
                collect_interface_names_from_expr(item, signatures, reachable, pending);
            }
        }
        ExprKind::ListSpread { value, .. } => {
            collect_interface_names_from_expr(value, signatures, reachable, pending);
        }
        ExprKind::ListIf {
            condition,
            value,
            else_value,
            ..
        } => {
            collect_interface_names_from_expr(condition, signatures, reachable, pending);
            collect_interface_names_from_expr(value, signatures, reachable, pending);
            if let Some(else_value) = else_value {
                collect_interface_names_from_expr(else_value, signatures, reachable, pending);
            }
        }
        ExprKind::Index { base, index } => {
            collect_interface_names_from_expr(base, signatures, reachable, pending);
            collect_interface_names_from_expr(index, signatures, reachable, pending);
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            collect_interface_names_from_expr(base, signatures, reachable, pending);
            for part in [start, end, step].into_iter().flatten() {
                collect_interface_names_from_expr(part, signatures, reachable, pending);
            }
        }
        ExprKind::ListComprehension {
            value,
            iterable,
            condition,
            ..
        } => {
            collect_interface_names_from_expr(value, signatures, reachable, pending);
            collect_interface_names_from_expr(iterable, signatures, reachable, pending);
            if let Some(condition) = condition {
                collect_interface_names_from_expr(condition, signatures, reachable, pending);
            }
        }
        ExprKind::StructLiteral { base, fields, .. } => {
            if let Some(base) = base {
                collect_interface_names_from_expr(base, signatures, reachable, pending);
            }
            for field in fields {
                collect_interface_names_from_expr(&field.value, signatures, reachable, pending);
            }
        }
        ExprKind::Field { base, .. } | ExprKind::Unary { expr: base, .. } => {
            collect_interface_names_from_expr(base, signatures, reachable, pending);
        }
        ExprKind::Match { value, arms } => {
            collect_interface_names_from_expr(value, signatures, reachable, pending);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_interface_names_from_expr(guard, signatures, reachable, pending);
                }
                collect_interface_names_from_expr(&arm.value, signatures, reachable, pending);
            }
        }
        ExprKind::ListMatch { value, arms } => {
            collect_interface_names_from_expr(value, signatures, reachable, pending);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_interface_names_from_expr(guard, signatures, reachable, pending);
                }
                collect_interface_names_from_expr(&arm.value, signatures, reachable, pending);
            }
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            collect_interface_names_from_expr(then_expr, signatures, reachable, pending);
            collect_interface_names_from_expr(cond, signatures, reachable, pending);
            collect_interface_names_from_expr(else_expr, signatures, reachable, pending);
        }
        ExprKind::Binary { left, right, .. } => {
            collect_interface_names_from_expr(left, signatures, reachable, pending);
            collect_interface_names_from_expr(right, signatures, reachable, pending);
        }
        ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::Var(_) => {}
    }
}

fn reachable_enum_variant_helpers(
    program: &Program,
    signatures: &Signatures,
    reachable_functions: &HashSet<String>,
    function_ir: &FunctionIrCache,
) -> HashSet<(String, String)> {
    let mut variants = HashSet::new();
    if let Some(application) = &program.application {
        for field in &application.metadata {
            collect_enum_variant_refs_from_expr(&field.value, signatures, &mut variants);
        }
    }
    for view in runtime_views(program) {
        for param in &view.params {
            if let Some(default) = &param.default {
                collect_enum_variant_refs_from_expr(default, signatures, &mut variants);
            }
        }
        for state in &view.states {
            collect_enum_variant_refs_from_expr(&state.initial, signatures, &mut variants);
        }
        for derived in &view.derived {
            collect_enum_variant_refs_from_expr(&derived.value, signatures, &mut variants);
        }
        for element in &view.elements {
            for property in &element.properties {
                collect_enum_variant_refs_from_expr(&property.value, signatures, &mut variants);
            }
        }
    }
    for function in &program.functions {
        if !reachable_functions.contains(&function.name) {
            continue;
        }
        for param in &function.params {
            if let Some(default) = &param.default {
                collect_enum_variant_refs_from_expr(default, signatures, &mut variants);
            }
        }
        if let Some(cfg) = function_ir.get(&function.name) {
            for value in cfg
                .values()
                .iter()
                .filter(|value| cfg.is_value_reachable(value.id))
            {
                if let crate::ir::ControlFlowValueKind::QualifiedCall {
                    namespace, name, ..
                } = &value.kind
                    && signatures.enum_type(namespace).is_some()
                {
                    variants.insert((namespace.clone(), name.clone()));
                }
            }
        }
    }
    variants
}

fn collect_enum_variant_refs_from_expr(
    expr: &Expr,
    signatures: &Signatures,
    variants: &mut HashSet<(String, String)>,
) {
    match &expr.kind {
        ExprKind::QualifiedCall {
            namespace,
            name,
            args,
            named_args,
            ..
        } => {
            if signatures.enum_type(namespace).is_some() {
                variants.insert((namespace.clone(), name.clone()));
            }
            for arg in args {
                collect_enum_variant_refs_from_expr(arg, signatures, variants);
            }
            for arg in named_args {
                collect_enum_variant_refs_from_expr(&arg.value, signatures, variants);
            }
        }
        ExprKind::AnonymousFunction { body, .. } => {
            collect_enum_variant_refs_from_expr(body, signatures, variants);
        }
        ExprKind::Call {
            args, named_args, ..
        } => {
            for arg in args {
                collect_enum_variant_refs_from_expr(arg, signatures, variants);
            }
            for arg in named_args {
                collect_enum_variant_refs_from_expr(&arg.value, signatures, variants);
            }
        }
        ExprKind::ShellCall { args, .. } => {
            for arg in args {
                collect_enum_variant_refs_from_expr(arg, signatures, variants);
            }
        }
        ExprKind::Pipe { input, args, .. } => {
            collect_enum_variant_refs_from_expr(input, signatures, variants);
            for arg in args {
                collect_enum_variant_refs_from_expr(arg, signatures, variants);
            }
        }
        ExprKind::List(items) => {
            for item in items {
                collect_enum_variant_refs_from_expr(item, signatures, variants);
            }
        }
        ExprKind::ListSpread { value, .. } => {
            collect_enum_variant_refs_from_expr(value, signatures, variants);
        }
        ExprKind::ListIf {
            condition,
            value,
            else_value,
            ..
        } => {
            collect_enum_variant_refs_from_expr(condition, signatures, variants);
            collect_enum_variant_refs_from_expr(value, signatures, variants);
            if let Some(else_value) = else_value {
                collect_enum_variant_refs_from_expr(else_value, signatures, variants);
            }
        }
        ExprKind::Index { base, index } => {
            collect_enum_variant_refs_from_expr(base, signatures, variants);
            collect_enum_variant_refs_from_expr(index, signatures, variants);
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            collect_enum_variant_refs_from_expr(base, signatures, variants);
            for part in [start, end, step].into_iter().flatten() {
                collect_enum_variant_refs_from_expr(part, signatures, variants);
            }
        }
        ExprKind::ListComprehension {
            value,
            iterable,
            condition,
            ..
        } => {
            collect_enum_variant_refs_from_expr(value, signatures, variants);
            collect_enum_variant_refs_from_expr(iterable, signatures, variants);
            if let Some(condition) = condition {
                collect_enum_variant_refs_from_expr(condition, signatures, variants);
            }
        }
        ExprKind::StructLiteral { base, fields, .. } => {
            if let Some(base) = base {
                collect_enum_variant_refs_from_expr(base, signatures, variants);
            }
            for field in fields {
                collect_enum_variant_refs_from_expr(&field.value, signatures, variants);
            }
        }
        ExprKind::Field { base, .. } | ExprKind::Unary { expr: base, .. } => {
            collect_enum_variant_refs_from_expr(base, signatures, variants);
        }
        ExprKind::Match { value, arms } => {
            collect_enum_variant_refs_from_expr(value, signatures, variants);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_enum_variant_refs_from_expr(guard, signatures, variants);
                }
                collect_enum_variant_refs_from_expr(&arm.value, signatures, variants);
            }
        }
        ExprKind::ListMatch { value, arms } => {
            collect_enum_variant_refs_from_expr(value, signatures, variants);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_enum_variant_refs_from_expr(guard, signatures, variants);
                }
                collect_enum_variant_refs_from_expr(&arm.value, signatures, variants);
            }
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            collect_enum_variant_refs_from_expr(then_expr, signatures, variants);
            collect_enum_variant_refs_from_expr(cond, signatures, variants);
            collect_enum_variant_refs_from_expr(else_expr, signatures, variants);
        }
        ExprKind::Binary { left, right, .. } => {
            collect_enum_variant_refs_from_expr(left, signatures, variants);
            collect_enum_variant_refs_from_expr(right, signatures, variants);
        }
        ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::Var(_) => {}
    }
}

fn collect_value_type_names_from_ir(
    function: &Function,
    signatures: &Signatures,
    function_ir: &FunctionIrCache,
    known: &HashSet<String>,
    reachable: &mut HashSet<String>,
    pending: &mut Vec<String>,
) {
    let Some(cfg) = function_ir.get(&function.name) else {
        return;
    };
    for node in cfg.nodes().iter().filter(|node| cfg.is_reachable(node.id)) {
        for definition in &node.definitions {
            collect_value_type_names_from_type(
                &definition.ty,
                signatures,
                known,
                reachable,
                pending,
            );
        }
    }
    for value in cfg
        .values()
        .iter()
        .filter(|value| cfg.is_value_reachable(value.id))
    {
        collect_value_type_names_from_type(&value.ty, signatures, known, reachable, pending);
    }
}

fn reachable_value_type_names(
    program: &Program,
    signatures: &Signatures,
    reachable_functions: &HashSet<String>,
    reachable_interfaces: &HashSet<String>,
    function_ir: &FunctionIrCache,
) -> HashSet<String> {
    let known = program
        .structs
        .iter()
        .map(|definition| definition.name.clone())
        .chain(
            program
                .enums
                .iter()
                .map(|definition| definition.name.clone()),
        )
        .collect::<HashSet<_>>();
    let mut reachable = HashSet::new();
    let mut pending = Vec::new();

    for definition in &program.structs {
        if definition.public {
            enqueue_value_type_name(&definition.name, &known, &mut reachable, &mut pending);
        }
    }
    for definition in &program.enums {
        if definition.public {
            enqueue_value_type_name(&definition.name, &known, &mut reachable, &mut pending);
        }
    }
    for definition in &program.interfaces {
        if !reachable_interfaces.contains(&definition.name) {
            continue;
        }
        for function in &definition.functions {
            for param in &function.params {
                collect_value_type_names_from_type(
                    &param.ty,
                    signatures,
                    &known,
                    &mut reachable,
                    &mut pending,
                );
            }
            for ty in &function.returns {
                collect_value_type_names_from_type(
                    ty,
                    signatures,
                    &known,
                    &mut reachable,
                    &mut pending,
                );
            }
        }
    }

    if let Some(application) = &program.application {
        for field in &application.metadata {
            collect_value_type_names_from_expr(
                &field.value,
                signatures,
                &known,
                &mut reachable,
                &mut pending,
            );
        }
    }
    for view in runtime_views(program) {
        for param in &view.params {
            collect_value_type_names_from_type(
                &param.ty,
                signatures,
                &known,
                &mut reachable,
                &mut pending,
            );
            if let Some(default) = &param.default {
                collect_value_type_names_from_expr(
                    default,
                    signatures,
                    &known,
                    &mut reachable,
                    &mut pending,
                );
            }
        }
        for state in &view.states {
            collect_value_type_names_from_type(
                &state.ty,
                signatures,
                &known,
                &mut reachable,
                &mut pending,
            );
            collect_value_type_names_from_expr(
                &state.initial,
                signatures,
                &known,
                &mut reachable,
                &mut pending,
            );
        }
        for derived in &view.derived {
            collect_value_type_names_from_type(
                &derived.ty,
                signatures,
                &known,
                &mut reachable,
                &mut pending,
            );
            collect_value_type_names_from_expr(
                &derived.value,
                signatures,
                &known,
                &mut reachable,
                &mut pending,
            );
        }
        for element in &view.elements {
            for property in &element.properties {
                collect_value_type_names_from_expr(
                    &property.value,
                    signatures,
                    &known,
                    &mut reachable,
                    &mut pending,
                );
            }
        }
    }

    for function in &program.functions {
        if !reachable_functions.contains(&function.name) {
            continue;
        }
        for param in &function.params {
            collect_value_type_names_from_type(
                &param.ty,
                signatures,
                &known,
                &mut reachable,
                &mut pending,
            );
            if let Some(default) = &param.default {
                collect_value_type_names_from_expr(
                    default,
                    signatures,
                    &known,
                    &mut reachable,
                    &mut pending,
                );
            }
        }
        for ty in &function.returns {
            collect_value_type_names_from_type(
                ty,
                signatures,
                &known,
                &mut reachable,
                &mut pending,
            );
        }
        collect_value_type_names_from_ir(
            function,
            signatures,
            function_ir,
            &known,
            &mut reachable,
            &mut pending,
        );
    }

    while let Some(name) = pending.pop() {
        if let Some(definition) = program
            .structs
            .iter()
            .find(|definition| definition.name == name)
        {
            for field in &definition.fields {
                collect_value_type_names_from_type(
                    &field.ty,
                    signatures,
                    &known,
                    &mut reachable,
                    &mut pending,
                );
            }
            continue;
        }
        if let Some(definition) = program
            .enums
            .iter()
            .find(|definition| definition.name == name)
        {
            for variant in &definition.variants {
                for payload in &variant.payloads {
                    collect_value_type_names_from_type(
                        &payload.ty,
                        signatures,
                        &known,
                        &mut reachable,
                        &mut pending,
                    );
                }
            }
        }
    }

    reachable
}

fn enqueue_value_type_name(
    name: &str,
    known: &HashSet<String>,
    reachable: &mut HashSet<String>,
    pending: &mut Vec<String>,
) {
    if known.contains(name) && reachable.insert(name.to_string()) {
        pending.push(name.to_string());
    }
}

fn collect_value_type_names_from_type(
    ty: &Type,
    signatures: &Signatures,
    known: &HashSet<String>,
    reachable: &mut HashSet<String>,
    pending: &mut Vec<String>,
) {
    match signatures.canonical_type(ty) {
        Type::Named(name) => enqueue_value_type_name(&name, known, reachable, pending),
        Type::List(element) => {
            collect_value_type_names_from_type(&element, signatures, known, reachable, pending)
        }
        Type::Function { params, returns } => {
            for ty in params.iter().chain(&returns) {
                collect_value_type_names_from_type(ty, signatures, known, reachable, pending);
            }
        }
        Type::I64 | Type::Bool | Type::Str | Type::Error | Type::Void => {}
    }
}

fn collect_value_type_names_from_struct_pattern(
    pattern: &crate::ast::StructPattern,
    known: &HashSet<String>,
    reachable: &mut HashSet<String>,
    pending: &mut Vec<String>,
) {
    enqueue_value_type_name(&pattern.struct_name, known, reachable, pending);
    for field in &pattern.fields {
        if let Some(nested) = &field.nested {
            collect_value_type_names_from_struct_pattern(nested, known, reachable, pending);
        }
    }
}

fn collect_value_type_names_from_match_pattern(
    pattern: &MatchPattern,
    known: &HashSet<String>,
    reachable: &mut HashSet<String>,
    pending: &mut Vec<String>,
) {
    if let MatchPattern::Struct(pattern) = pattern {
        collect_value_type_names_from_struct_pattern(pattern, known, reachable, pending);
    }
}

fn collect_value_type_names_from_expr(
    expr: &Expr,
    signatures: &Signatures,
    known: &HashSet<String>,
    reachable: &mut HashSet<String>,
    pending: &mut Vec<String>,
) {
    match &expr.kind {
        ExprKind::AnonymousFunction {
            params,
            return_type,
            body,
        } => {
            for param in params {
                collect_value_type_names_from_type(
                    &param.ty, signatures, known, reachable, pending,
                );
            }
            if let Some(return_type) = return_type {
                collect_value_type_names_from_type(
                    return_type,
                    signatures,
                    known,
                    reachable,
                    pending,
                );
            }
            collect_value_type_names_from_expr(body, signatures, known, reachable, pending);
        }
        ExprKind::Call {
            args, named_args, ..
        }
        | ExprKind::QualifiedCall {
            args, named_args, ..
        } => {
            if let ExprKind::QualifiedCall { namespace, .. } = &expr.kind {
                enqueue_value_type_name(namespace, known, reachable, pending);
            }
            for arg in args {
                collect_value_type_names_from_expr(arg, signatures, known, reachable, pending);
            }
            for arg in named_args {
                collect_value_type_names_from_expr(
                    &arg.value, signatures, known, reachable, pending,
                );
            }
        }
        ExprKind::ShellCall { args, .. } => {
            for arg in args {
                collect_value_type_names_from_expr(arg, signatures, known, reachable, pending);
            }
        }
        ExprKind::Pipe { input, args, .. } => {
            collect_value_type_names_from_expr(input, signatures, known, reachable, pending);
            for arg in args {
                collect_value_type_names_from_expr(arg, signatures, known, reachable, pending);
            }
        }
        ExprKind::List(items) => {
            for item in items {
                collect_value_type_names_from_expr(item, signatures, known, reachable, pending);
            }
        }
        ExprKind::ListSpread { value, .. } => {
            collect_value_type_names_from_expr(value, signatures, known, reachable, pending);
        }
        ExprKind::ListIf {
            condition,
            value,
            else_value,
            ..
        } => {
            collect_value_type_names_from_expr(condition, signatures, known, reachable, pending);
            collect_value_type_names_from_expr(value, signatures, known, reachable, pending);
            if let Some(else_value) = else_value {
                collect_value_type_names_from_expr(
                    else_value, signatures, known, reachable, pending,
                );
            }
        }
        ExprKind::Index { base, index } => {
            collect_value_type_names_from_expr(base, signatures, known, reachable, pending);
            collect_value_type_names_from_expr(index, signatures, known, reachable, pending);
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            collect_value_type_names_from_expr(base, signatures, known, reachable, pending);
            for part in [start, end, step].into_iter().flatten() {
                collect_value_type_names_from_expr(part, signatures, known, reachable, pending);
            }
        }
        ExprKind::ListComprehension {
            value,
            iterable,
            condition,
            ..
        } => {
            collect_value_type_names_from_expr(value, signatures, known, reachable, pending);
            collect_value_type_names_from_expr(iterable, signatures, known, reachable, pending);
            if let Some(condition) = condition {
                collect_value_type_names_from_expr(
                    condition, signatures, known, reachable, pending,
                );
            }
        }
        ExprKind::StructLiteral {
            name, base, fields, ..
        } => {
            enqueue_value_type_name(name, known, reachable, pending);
            if let Some(base) = base {
                collect_value_type_names_from_expr(base, signatures, known, reachable, pending);
            }
            for field in fields {
                collect_value_type_names_from_expr(
                    &field.value,
                    signatures,
                    known,
                    reachable,
                    pending,
                );
            }
        }
        ExprKind::Field { base, .. } | ExprKind::Unary { expr: base, .. } => {
            collect_value_type_names_from_expr(base, signatures, known, reachable, pending);
        }
        ExprKind::Match { value, arms } => {
            collect_value_type_names_from_expr(value, signatures, known, reachable, pending);
            for arm in arms {
                enqueue_value_type_name(&arm.enum_name, known, reachable, pending);
                for pattern in &arm.patterns {
                    collect_value_type_names_from_match_pattern(pattern, known, reachable, pending);
                }
                if let Some(guard) = &arm.guard {
                    collect_value_type_names_from_expr(
                        guard, signatures, known, reachable, pending,
                    );
                }
                collect_value_type_names_from_expr(
                    &arm.value, signatures, known, reachable, pending,
                );
            }
        }
        ExprKind::ListMatch { value, arms } => {
            collect_value_type_names_from_expr(value, signatures, known, reachable, pending);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_value_type_names_from_expr(
                        guard, signatures, known, reachable, pending,
                    );
                }
                collect_value_type_names_from_expr(
                    &arm.value, signatures, known, reachable, pending,
                );
            }
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            collect_value_type_names_from_expr(then_expr, signatures, known, reachable, pending);
            collect_value_type_names_from_expr(cond, signatures, known, reachable, pending);
            collect_value_type_names_from_expr(else_expr, signatures, known, reachable, pending);
        }
        ExprKind::Binary { left, right, .. } => {
            collect_value_type_names_from_expr(left, signatures, known, reachable, pending);
            collect_value_type_names_from_expr(right, signatures, known, reachable, pending);
        }
        ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::Var(_) => {}
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct InterfacePackFacts {
    targets: HashMap<String, HashSet<String>>,
    open_interfaces: HashSet<String>,
}

impl InterfacePackFacts {
    fn external_roots(program: &Program, signatures: &Signatures) -> Self {
        let mut facts = Self::default();
        for function in &program.functions {
            if !function.public {
                continue;
            }
            for param in &function.params {
                facts.mark_open_type(&param.ty, signatures);
            }
        }
        facts
    }

    fn from_reachable_functions(
        program: &Program,
        signatures: &Signatures,
        reachable_functions: &HashSet<String>,
        function_ir: &FunctionIrCache,
    ) -> Self {
        let mut facts = Self::external_roots(program, signatures);
        let empty_env = HashMap::new();

        if let Some(application) = &program.application {
            for field in &application.metadata {
                collect_interface_pack_facts_from_expr(
                    &field.value,
                    &empty_env,
                    signatures,
                    &mut facts,
                );
            }
        }

        for view in runtime_views(program) {
            let env =
                view.params
                    .iter()
                    .map(|param| (param.name.clone(), signatures.canonical_type(&param.ty)))
                    .chain(
                        view.states.iter().map(|state| {
                            (state.name.clone(), signatures.canonical_type(&state.ty))
                        }),
                    )
                    .chain(view.derived.iter().map(|derived| {
                        (derived.name.clone(), signatures.canonical_type(&derived.ty))
                    }))
                    .collect::<HashMap<_, _>>();
            for param in &view.params {
                if let Some(default) = &param.default {
                    collect_interface_pack_facts_from_expr(default, &env, signatures, &mut facts);
                }
            }
            for state in &view.states {
                collect_interface_pack_facts_from_expr(
                    &state.initial,
                    &env,
                    signatures,
                    &mut facts,
                );
            }
            for derived in &view.derived {
                collect_interface_pack_facts_from_expr(
                    &derived.value,
                    &env,
                    signatures,
                    &mut facts,
                );
            }
            for element in &view.elements {
                for property in &element.properties {
                    collect_interface_pack_facts_from_expr(
                        &property.value,
                        &env,
                        signatures,
                        &mut facts,
                    );
                }
            }
        }

        for function in &program.functions {
            if !reachable_functions.contains(&function.name) {
                continue;
            }
            let Some(cfg) = function_ir.get(&function.name) else {
                continue;
            };
            for value in cfg
                .values()
                .iter()
                .filter(|value| cfg.is_value_reachable(value.id))
            {
                if let crate::ir::ControlFlowValueKind::InterfacePack {
                    interface, target, ..
                } = &value.kind
                {
                    facts.record_target(interface, target.clone());
                }
            }
        }

        facts
    }

    fn mark_open_type(&mut self, ty: &Type, signatures: &Signatures) {
        match signatures.canonical_type(ty) {
            Type::Named(name) if signatures.interface(&name).is_some() => {
                self.open_interfaces.insert(name);
            }
            Type::List(element) => self.mark_open_type(&element, signatures),
            Type::Function { params, returns } => {
                for ty in params.iter().chain(&returns) {
                    self.mark_open_type(ty, signatures);
                }
            }
            Type::I64 | Type::Bool | Type::Str | Type::Error | Type::Void | Type::Named(_) => {}
        }
    }

    fn record_target(&mut self, interface_name: &str, target_name: String) {
        self.targets
            .entry(interface_name.to_string())
            .or_default()
            .insert(target_name);
    }

    fn mark_open(&mut self, interface_name: &str) {
        self.open_interfaces.insert(interface_name.to_string());
    }

    fn allows(&self, interface_name: &str, target_name: &str) -> bool {
        self.open_interfaces.contains(interface_name)
            || self
                .targets
                .get(interface_name)
                .is_some_and(|targets| targets.contains(target_name))
    }
}

fn collect_interface_pack_facts_from_expr(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    facts: &mut InterfacePackFacts,
) {
    if let ExprKind::Call {
        name,
        args,
        named_args,
    } = &expr.kind
        && signatures.interface(name).is_some()
        && named_args.is_empty()
        && args.len() == 1
    {
        match type_of_expr(&args[0], env, signatures)
            .ok()
            .map(|ty| signatures.canonical_type(&ty))
        {
            Some(Type::Named(target_name))
                if target_name != *name
                    && signatures.implementation(name, &target_name).is_some() =>
            {
                facts.record_target(name, target_name);
            }
            _ => facts.mark_open(name),
        }
    }

    match &expr.kind {
        ExprKind::AnonymousFunction { params, body, .. } => {
            let mut body_env = env.clone();
            for param in params {
                body_env.insert(param.name.clone(), signatures.canonical_type(&param.ty));
            }
            collect_interface_pack_facts_from_expr(body, &body_env, signatures, facts);
        }
        ExprKind::Call {
            args, named_args, ..
        }
        | ExprKind::QualifiedCall {
            args, named_args, ..
        } => {
            for arg in args {
                collect_interface_pack_facts_from_expr(arg, env, signatures, facts);
            }
            for arg in named_args {
                collect_interface_pack_facts_from_expr(&arg.value, env, signatures, facts);
            }
        }
        ExprKind::ShellCall { args, .. } => {
            for arg in args {
                collect_interface_pack_facts_from_expr(arg, env, signatures, facts);
            }
        }
        ExprKind::Pipe { input, args, .. } => {
            collect_interface_pack_facts_from_expr(input, env, signatures, facts);
            for arg in args {
                collect_interface_pack_facts_from_expr(arg, env, signatures, facts);
            }
        }
        ExprKind::List(items) => {
            for item in items {
                collect_interface_pack_facts_from_expr(item, env, signatures, facts);
            }
        }
        ExprKind::ListSpread { value, .. } => {
            collect_interface_pack_facts_from_expr(value, env, signatures, facts)
        }
        ExprKind::ListIf {
            condition,
            value,
            else_value,
            ..
        } => {
            collect_interface_pack_facts_from_expr(condition, env, signatures, facts);
            collect_interface_pack_facts_from_expr(value, env, signatures, facts);
            if let Some(else_value) = else_value {
                collect_interface_pack_facts_from_expr(else_value, env, signatures, facts);
            }
        }
        ExprKind::Index { base, index } => {
            collect_interface_pack_facts_from_expr(base, env, signatures, facts);
            collect_interface_pack_facts_from_expr(index, env, signatures, facts);
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            collect_interface_pack_facts_from_expr(base, env, signatures, facts);
            for part in [start, end, step].into_iter().flatten() {
                collect_interface_pack_facts_from_expr(part, env, signatures, facts);
            }
        }
        ExprKind::ListComprehension {
            value,
            binding,
            iterable,
            condition,
            ..
        } => {
            collect_interface_pack_facts_from_expr(iterable, env, signatures, facts);
            let mut item_env = env.clone();
            if let Ok(Type::List(element)) = type_of_expr(iterable, env, signatures) {
                item_env.insert(binding.clone(), signatures.canonical_type(&element));
            }
            collect_interface_pack_facts_from_expr(value, &item_env, signatures, facts);
            if let Some(condition) = condition {
                collect_interface_pack_facts_from_expr(condition, &item_env, signatures, facts);
            }
        }
        ExprKind::StructLiteral { base, fields, .. } => {
            if let Some(base) = base {
                collect_interface_pack_facts_from_expr(base, env, signatures, facts);
            }
            for field in fields {
                collect_interface_pack_facts_from_expr(&field.value, env, signatures, facts);
            }
        }
        ExprKind::Field { base, .. } | ExprKind::Unary { expr: base, .. } => {
            collect_interface_pack_facts_from_expr(base, env, signatures, facts);
        }
        ExprKind::Match { value, arms } => {
            collect_interface_pack_facts_from_expr(value, env, signatures, facts);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_interface_pack_facts_from_expr(guard, env, signatures, facts);
                }
                collect_interface_pack_facts_from_expr(&arm.value, env, signatures, facts);
            }
        }
        ExprKind::ListMatch { value, arms } => {
            collect_interface_pack_facts_from_expr(value, env, signatures, facts);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_interface_pack_facts_from_expr(guard, env, signatures, facts);
                }
                collect_interface_pack_facts_from_expr(&arm.value, env, signatures, facts);
            }
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            collect_interface_pack_facts_from_expr(then_expr, env, signatures, facts);
            collect_interface_pack_facts_from_expr(cond, env, signatures, facts);
            collect_interface_pack_facts_from_expr(else_expr, env, signatures, facts);
        }
        ExprKind::Binary { left, right, .. } => {
            collect_interface_pack_facts_from_expr(left, env, signatures, facts);
            collect_interface_pack_facts_from_expr(right, env, signatures, facts);
        }
        ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::Var(_) => {}
    }
}

fn collect_function_reachability_from_ir(
    function: &Function,
    function_ir: &FunctionIrCache,
    known_functions: &HashSet<String>,
    direct_functions: &mut HashSet<String>,
    dynamic_capabilities: &mut HashSet<(String, String)>,
) {
    let Some(cfg) = function_ir.get(&function.name) else {
        return;
    };
    for value in cfg
        .values()
        .iter()
        .filter(|value| cfg.is_value_reachable(value.id))
    {
        match &value.kind {
            crate::ir::ControlFlowValueKind::NameRead { name, definitions }
                if definitions.is_empty() && known_functions.contains(name) =>
            {
                direct_functions.insert(name.clone());
            }
            crate::ir::ControlFlowValueKind::Call { callee, .. }
                if known_functions.contains(callee) =>
            {
                direct_functions.insert(callee.clone());
            }
            crate::ir::ControlFlowValueKind::InterfaceDispatch {
                interface,
                capability,
                mapped_function,
                ..
            } => {
                if let Some(mapped) = mapped_function {
                    direct_functions.insert(mapped.clone());
                } else {
                    dynamic_capabilities.insert((interface.clone(), capability.clone()));
                }
            }
            _ => {}
        }
    }
}

fn reachable_function_names(
    program: &Program,
    signatures: &Signatures,
    reachable_interfaces: &HashSet<String>,
    interface_pack_facts: &InterfacePackFacts,
    function_ir: &FunctionIrCache,
) -> HashSet<String> {
    let known = program
        .functions
        .iter()
        .map(|function| function.name.clone())
        .collect::<HashSet<_>>();
    let mut reachable = HashSet::new();
    let mut pending = Vec::new();

    for function in &program.functions {
        if function.name == "main" || function.public {
            enqueue_function(&function.name, &known, &mut reachable, &mut pending);
        }
    }
    let mut roots = HashSet::new();
    let mut interface_capabilities = HashSet::new();
    let empty_env = HashMap::new();
    if let Some(application) = &program.application {
        for field in &application.metadata {
            collect_named_function_refs_from_expr(&field.value, &known, &mut roots);
            collect_interface_dispatch_refs_from_expr(
                &field.value,
                &empty_env,
                signatures,
                &mut roots,
                &mut interface_capabilities,
            );
        }
    }
    for view in runtime_views(program) {
        for param in &view.params {
            if let Some(default) = &param.default {
                collect_named_function_refs_from_expr(default, &known, &mut roots);
                collect_interface_dispatch_refs_from_expr(
                    default,
                    &empty_env,
                    signatures,
                    &mut roots,
                    &mut interface_capabilities,
                );
            }
        }
        for state in &view.states {
            collect_named_function_refs_from_expr(&state.initial, &known, &mut roots);
            collect_interface_dispatch_refs_from_expr(
                &state.initial,
                &empty_env,
                signatures,
                &mut roots,
                &mut interface_capabilities,
            );
        }
        for derived in &view.derived {
            collect_named_function_refs_from_expr(&derived.value, &known, &mut roots);
            collect_interface_dispatch_refs_from_expr(
                &derived.value,
                &empty_env,
                signatures,
                &mut roots,
                &mut interface_capabilities,
            );
        }
        for element in &view.elements {
            for property in &element.properties {
                collect_named_function_refs_from_expr(&property.value, &known, &mut roots);
                collect_interface_dispatch_refs_from_expr(
                    &property.value,
                    &empty_env,
                    signatures,
                    &mut roots,
                    &mut interface_capabilities,
                );
            }
        }
    }
    for name in roots {
        enqueue_function(&name, &known, &mut reachable, &mut pending);
    }
    enqueue_interface_capability_mappings(
        program,
        reachable_interfaces,
        &interface_capabilities,
        interface_pack_facts,
        &known,
        &mut reachable,
        &mut pending,
    );

    while let Some(name) = pending.pop() {
        let Some(function) = program
            .functions
            .iter()
            .find(|function| function.name == name)
        else {
            continue;
        };
        let mut references = HashSet::new();
        let mut capabilities = HashSet::new();
        let env = function
            .params
            .iter()
            .map(|param| (param.name.clone(), signatures.canonical_type(&param.ty)))
            .collect::<HashMap<_, _>>();
        for param in &function.params {
            if let Some(default) = &param.default {
                collect_named_function_refs_from_expr(default, &known, &mut references);
                collect_interface_dispatch_refs_from_expr(
                    default,
                    &env,
                    signatures,
                    &mut references,
                    &mut capabilities,
                );
            }
        }
        collect_function_reachability_from_ir(
            function,
            function_ir,
            &known,
            &mut references,
            &mut capabilities,
        );
        for reference in references {
            enqueue_function(&reference, &known, &mut reachable, &mut pending);
        }
        enqueue_interface_capability_mappings(
            program,
            reachable_interfaces,
            &capabilities,
            interface_pack_facts,
            &known,
            &mut reachable,
            &mut pending,
        );
    }

    reachable
}

fn enqueue_interface_capability_mappings(
    program: &Program,
    reachable_interfaces: &HashSet<String>,
    capabilities: &HashSet<(String, String)>,
    interface_pack_facts: &InterfacePackFacts,
    known: &HashSet<String>,
    reachable: &mut HashSet<String>,
    pending: &mut Vec<String>,
) {
    for (interface_name, capability_name) in capabilities {
        if !reachable_interfaces.contains(interface_name) {
            continue;
        }
        for implementation in &program.implementations {
            if implementation.interface_name != *interface_name
                || !interface_pack_facts.allows(interface_name, &implementation.target_name)
            {
                continue;
            }
            if let Some(mapping) = implementation
                .mappings
                .iter()
                .find(|mapping| mapping.member == *capability_name)
            {
                enqueue_function(&mapping.function, known, reachable, pending);
            }
        }
    }
}

fn enqueue_function(
    name: &str,
    known: &HashSet<String>,
    reachable: &mut HashSet<String>,
    pending: &mut Vec<String>,
) {
    if known.contains(name) && reachable.insert(name.to_string()) {
        pending.push(name.to_string());
    }
}

fn collect_interface_dispatch_refs_from_expr(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    direct_functions: &mut HashSet<String>,
    dynamic_capabilities: &mut HashSet<(String, String)>,
) {
    if let ExprKind::QualifiedCall {
        namespace,
        name,
        args,
        ..
    } = &expr.kind
        && signatures.interface(namespace).is_some()
    {
        let direct_target = args.first().and_then(|receiver| {
            let receiver_ty = type_of_expr(receiver, env, signatures).ok()?;
            let Type::Named(target_name) = signatures.canonical_type(&receiver_ty) else {
                return None;
            };
            if target_name == *namespace && signatures.interface(&target_name).is_some() {
                return None;
            }
            signatures
                .implementation(namespace, &target_name)
                .and_then(|implementation| implementation.functions.get(name))
                .cloned()
        });
        if let Some(mapped) = direct_target {
            direct_functions.insert(mapped);
        } else {
            dynamic_capabilities.insert((namespace.clone(), name.clone()));
        }
    }

    match &expr.kind {
        ExprKind::AnonymousFunction { params, body, .. } => {
            let mut body_env = env.clone();
            for param in params {
                body_env.insert(param.name.clone(), signatures.canonical_type(&param.ty));
            }
            collect_interface_dispatch_refs_from_expr(
                body,
                &body_env,
                signatures,
                direct_functions,
                dynamic_capabilities,
            );
        }
        ExprKind::Call {
            args, named_args, ..
        }
        | ExprKind::QualifiedCall {
            args, named_args, ..
        } => {
            for arg in args {
                collect_interface_dispatch_refs_from_expr(
                    arg,
                    env,
                    signatures,
                    direct_functions,
                    dynamic_capabilities,
                );
            }
            for arg in named_args {
                collect_interface_dispatch_refs_from_expr(
                    &arg.value,
                    env,
                    signatures,
                    direct_functions,
                    dynamic_capabilities,
                );
            }
        }
        ExprKind::ShellCall { args, .. } => {
            for arg in args {
                collect_interface_dispatch_refs_from_expr(
                    arg,
                    env,
                    signatures,
                    direct_functions,
                    dynamic_capabilities,
                );
            }
        }
        ExprKind::Pipe { input, args, .. } => {
            collect_interface_dispatch_refs_from_expr(
                input,
                env,
                signatures,
                direct_functions,
                dynamic_capabilities,
            );
            for arg in args {
                collect_interface_dispatch_refs_from_expr(
                    arg,
                    env,
                    signatures,
                    direct_functions,
                    dynamic_capabilities,
                );
            }
        }
        ExprKind::List(items) => {
            for item in items {
                collect_interface_dispatch_refs_from_expr(
                    item,
                    env,
                    signatures,
                    direct_functions,
                    dynamic_capabilities,
                );
            }
        }
        ExprKind::ListSpread { value, .. } => collect_interface_dispatch_refs_from_expr(
            value,
            env,
            signatures,
            direct_functions,
            dynamic_capabilities,
        ),
        ExprKind::ListIf {
            condition,
            value,
            else_value,
            ..
        } => {
            collect_interface_dispatch_refs_from_expr(
                condition,
                env,
                signatures,
                direct_functions,
                dynamic_capabilities,
            );
            collect_interface_dispatch_refs_from_expr(
                value,
                env,
                signatures,
                direct_functions,
                dynamic_capabilities,
            );
            if let Some(else_value) = else_value {
                collect_interface_dispatch_refs_from_expr(
                    else_value,
                    env,
                    signatures,
                    direct_functions,
                    dynamic_capabilities,
                );
            }
        }
        ExprKind::Index { base, index } => {
            for item in [base.as_ref(), index.as_ref()] {
                collect_interface_dispatch_refs_from_expr(
                    item,
                    env,
                    signatures,
                    direct_functions,
                    dynamic_capabilities,
                );
            }
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            collect_interface_dispatch_refs_from_expr(
                base,
                env,
                signatures,
                direct_functions,
                dynamic_capabilities,
            );
            for part in [start, end, step].into_iter().flatten() {
                collect_interface_dispatch_refs_from_expr(
                    part,
                    env,
                    signatures,
                    direct_functions,
                    dynamic_capabilities,
                );
            }
        }
        ExprKind::ListComprehension {
            value,
            iterable,
            condition,
            binding,
            ..
        } => {
            collect_interface_dispatch_refs_from_expr(
                iterable,
                env,
                signatures,
                direct_functions,
                dynamic_capabilities,
            );
            let mut item_env = env.clone();
            if let Ok(Type::List(element)) = type_of_expr(iterable, env, signatures) {
                item_env.insert(binding.clone(), signatures.canonical_type(&element));
            }
            collect_interface_dispatch_refs_from_expr(
                value,
                &item_env,
                signatures,
                direct_functions,
                dynamic_capabilities,
            );
            if let Some(condition) = condition {
                collect_interface_dispatch_refs_from_expr(
                    condition,
                    &item_env,
                    signatures,
                    direct_functions,
                    dynamic_capabilities,
                );
            }
        }
        ExprKind::StructLiteral { base, fields, .. } => {
            if let Some(base) = base {
                collect_interface_dispatch_refs_from_expr(
                    base,
                    env,
                    signatures,
                    direct_functions,
                    dynamic_capabilities,
                );
            }
            for field in fields {
                collect_interface_dispatch_refs_from_expr(
                    &field.value,
                    env,
                    signatures,
                    direct_functions,
                    dynamic_capabilities,
                );
            }
        }
        ExprKind::Field { base, .. } | ExprKind::Unary { expr: base, .. } => {
            collect_interface_dispatch_refs_from_expr(
                base,
                env,
                signatures,
                direct_functions,
                dynamic_capabilities,
            );
        }
        ExprKind::Match { value, arms } => {
            collect_interface_dispatch_refs_from_expr(
                value,
                env,
                signatures,
                direct_functions,
                dynamic_capabilities,
            );
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_interface_dispatch_refs_from_expr(
                        guard,
                        env,
                        signatures,
                        direct_functions,
                        dynamic_capabilities,
                    );
                }
                collect_interface_dispatch_refs_from_expr(
                    &arm.value,
                    env,
                    signatures,
                    direct_functions,
                    dynamic_capabilities,
                );
            }
        }
        ExprKind::ListMatch { value, arms } => {
            collect_interface_dispatch_refs_from_expr(
                value,
                env,
                signatures,
                direct_functions,
                dynamic_capabilities,
            );
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_interface_dispatch_refs_from_expr(
                        guard,
                        env,
                        signatures,
                        direct_functions,
                        dynamic_capabilities,
                    );
                }
                collect_interface_dispatch_refs_from_expr(
                    &arm.value,
                    env,
                    signatures,
                    direct_functions,
                    dynamic_capabilities,
                );
            }
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            for item in [then_expr.as_ref(), cond.as_ref(), else_expr.as_ref()] {
                collect_interface_dispatch_refs_from_expr(
                    item,
                    env,
                    signatures,
                    direct_functions,
                    dynamic_capabilities,
                );
            }
        }
        ExprKind::Binary { left, right, .. } => {
            for item in [left.as_ref(), right.as_ref()] {
                collect_interface_dispatch_refs_from_expr(
                    item,
                    env,
                    signatures,
                    direct_functions,
                    dynamic_capabilities,
                );
            }
        }
        ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::Var(_) => {}
    }
}

fn collect_named_function_refs_from_expr(
    expr: &Expr,
    known: &HashSet<String>,
    references: &mut HashSet<String>,
) {
    match &expr.kind {
        ExprKind::Var(name) => {
            if known.contains(name) {
                references.insert(name.clone());
            }
        }
        ExprKind::AnonymousFunction { body, .. } => {
            collect_named_function_refs_from_expr(body, known, references);
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } => {
            if known.contains(name) {
                references.insert(name.clone());
            }
            for arg in args {
                collect_named_function_refs_from_expr(arg, known, references);
            }
            for arg in named_args {
                collect_named_function_refs_from_expr(&arg.value, known, references);
            }
        }
        ExprKind::ShellCall { name, args, .. } => {
            if known.contains(name) {
                references.insert(name.clone());
            }
            for arg in args {
                collect_named_function_refs_from_expr(arg, known, references);
            }
        }
        ExprKind::Pipe {
            input, name, args, ..
        } => {
            if known.contains(name) {
                references.insert(name.clone());
            }
            collect_named_function_refs_from_expr(input, known, references);
            for arg in args {
                collect_named_function_refs_from_expr(arg, known, references);
            }
        }
        ExprKind::List(items) => {
            for item in items {
                collect_named_function_refs_from_expr(item, known, references);
            }
        }
        ExprKind::ListSpread { value, .. } => {
            collect_named_function_refs_from_expr(value, known, references);
        }
        ExprKind::ListIf {
            condition,
            value,
            else_value,
            ..
        } => {
            collect_named_function_refs_from_expr(condition, known, references);
            collect_named_function_refs_from_expr(value, known, references);
            if let Some(else_value) = else_value {
                collect_named_function_refs_from_expr(else_value, known, references);
            }
        }
        ExprKind::Index { base, index } => {
            collect_named_function_refs_from_expr(base, known, references);
            collect_named_function_refs_from_expr(index, known, references);
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            collect_named_function_refs_from_expr(base, known, references);
            for part in [start, end, step].into_iter().flatten() {
                collect_named_function_refs_from_expr(part, known, references);
            }
        }
        ExprKind::ListComprehension {
            value,
            iterable,
            condition,
            ..
        } => {
            collect_named_function_refs_from_expr(value, known, references);
            collect_named_function_refs_from_expr(iterable, known, references);
            if let Some(condition) = condition {
                collect_named_function_refs_from_expr(condition, known, references);
            }
        }
        ExprKind::StructLiteral { base, fields, .. } => {
            if let Some(base) = base {
                collect_named_function_refs_from_expr(base, known, references);
            }
            for field in fields {
                collect_named_function_refs_from_expr(&field.value, known, references);
            }
        }
        ExprKind::QualifiedCall {
            name,
            args,
            named_args,
            ..
        } => {
            if known.contains(name) {
                references.insert(name.clone());
            }
            for arg in args {
                collect_named_function_refs_from_expr(arg, known, references);
            }
            for arg in named_args {
                collect_named_function_refs_from_expr(&arg.value, known, references);
            }
        }
        ExprKind::Field { base, .. } | ExprKind::Unary { expr: base, .. } => {
            collect_named_function_refs_from_expr(base, known, references);
        }
        ExprKind::Match { value, arms } => {
            collect_named_function_refs_from_expr(value, known, references);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_named_function_refs_from_expr(guard, known, references);
                }
                collect_named_function_refs_from_expr(&arm.value, known, references);
            }
        }
        ExprKind::ListMatch { value, arms } => {
            collect_named_function_refs_from_expr(value, known, references);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_named_function_refs_from_expr(guard, known, references);
                }
                collect_named_function_refs_from_expr(&arm.value, known, references);
            }
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            collect_named_function_refs_from_expr(then_expr, known, references);
            collect_named_function_refs_from_expr(cond, known, references);
            collect_named_function_refs_from_expr(else_expr, known, references);
        }
        ExprKind::Binary { left, right, .. } => {
            collect_named_function_refs_from_expr(left, known, references);
            collect_named_function_refs_from_expr(right, known, references);
        }
        ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Str(_) | ExprKind::Nil => {}
    }
}

fn collect_anonymous_functions<'a>(
    program: &'a Program,
    reachable_functions: &HashSet<String>,
    function_ir: &FunctionIrCache,
) -> Vec<&'a Expr> {
    let mut functions = Vec::new();
    for function in &program.functions {
        if !reachable_functions.contains(&function.name) {
            continue;
        }
        let mut candidates = Vec::new();
        collect_anonymous_functions_from_block(&function.body, &mut candidates);
        let Some(cfg) = function_ir.get(&function.name) else {
            functions.extend(candidates);
            continue;
        };
        let reachable_spans = cfg
            .values()
            .iter()
            .filter(|value| cfg.is_value_reachable(value.id))
            .filter(|value| {
                matches!(
                    value.kind,
                    crate::ir::ControlFlowValueKind::AnonymousFunction { .. }
                )
            })
            .map(|value| source_span_key(value.span))
            .collect::<HashSet<_>>();
        functions.extend(
            candidates
                .into_iter()
                .filter(|expr| reachable_spans.contains(&source_span_key(expr.span))),
        );
    }
    functions
}

fn collect_anonymous_functions_from_block<'a>(body: &'a [Stmt], functions: &mut Vec<&'a Expr>) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Let { expr, .. }
            | StmtKind::Var { expr, .. }
            | StmtKind::Assign { expr, .. }
            | StmtKind::LetDestructure { expr, .. }
            | StmtKind::LetMultiDestructure { expr, .. }
            | StmtKind::LetListDestructure { expr, .. }
            | StmtKind::LetStructDestructure { expr, .. }
            | StmtKind::Expr(expr) => collect_anonymous_functions_from_expr(expr, functions),
            StmtKind::Return(values) => {
                for value in values {
                    collect_anonymous_functions_from_expr(value, functions);
                }
            }
            StmtKind::Shell { expr, redirect, .. } => {
                collect_anonymous_functions_from_expr(expr, functions);
                if let Some(redirect) = redirect {
                    collect_anonymous_functions_from_expr(&redirect.path, functions);
                }
            }
            StmtKind::If {
                cond,
                body,
                else_body,
                ..
            } => {
                collect_anonymous_functions_from_expr(cond, functions);
                collect_anonymous_functions_from_block(body, functions);
                collect_anonymous_functions_from_block(else_body, functions);
            }
            StmtKind::ForRange {
                start, end, body, ..
            } => {
                collect_anonymous_functions_from_expr(start, functions);
                collect_anonymous_functions_from_expr(end, functions);
                collect_anonymous_functions_from_block(body, functions);
            }
            StmtKind::ForEach { iterable, body, .. } => {
                collect_anonymous_functions_from_expr(iterable, functions);
                collect_anonymous_functions_from_block(body, functions);
            }
            StmtKind::While { cond, body } => {
                collect_anonymous_functions_from_expr(cond, functions);
                collect_anonymous_functions_from_block(body, functions);
            }
            StmtKind::Match { value, arms } => {
                collect_anonymous_functions_from_expr(value, functions);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        collect_anonymous_functions_from_expr(guard, functions);
                    }
                    collect_anonymous_functions_from_block(&arm.body, functions);
                }
            }
            StmtKind::ListMatch { value, arms } => {
                collect_anonymous_functions_from_expr(value, functions);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        collect_anonymous_functions_from_expr(guard, functions);
                    }
                    collect_anonymous_functions_from_block(&arm.body, functions);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
        }
    }
}

fn collect_anonymous_functions_from_expr<'a>(expr: &'a Expr, functions: &mut Vec<&'a Expr>) {
    match &expr.kind {
        ExprKind::AnonymousFunction { body, .. } => {
            functions.push(expr);
            collect_anonymous_functions_from_expr(body, functions);
        }
        ExprKind::Call {
            args, named_args, ..
        }
        | ExprKind::QualifiedCall {
            args, named_args, ..
        } => {
            for arg in args {
                collect_anonymous_functions_from_expr(arg, functions);
            }
            for arg in named_args {
                collect_anonymous_functions_from_expr(&arg.value, functions);
            }
        }
        ExprKind::ShellCall { args, .. } => {
            for arg in args {
                collect_anonymous_functions_from_expr(arg, functions);
            }
        }
        ExprKind::Pipe { input, args, .. } => {
            collect_anonymous_functions_from_expr(input, functions);
            for arg in args {
                collect_anonymous_functions_from_expr(arg, functions);
            }
        }
        ExprKind::List(items) => {
            for item in items {
                collect_anonymous_functions_from_expr(item, functions);
            }
        }
        ExprKind::ListSpread { value, .. } => {
            collect_anonymous_functions_from_expr(value, functions)
        }
        ExprKind::ListIf {
            condition,
            value,
            else_value,
            ..
        } => {
            collect_anonymous_functions_from_expr(condition, functions);
            collect_anonymous_functions_from_expr(value, functions);
            if let Some(else_value) = else_value {
                collect_anonymous_functions_from_expr(else_value, functions);
            }
        }
        ExprKind::Index { base, index } => {
            collect_anonymous_functions_from_expr(base, functions);
            collect_anonymous_functions_from_expr(index, functions);
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            collect_anonymous_functions_from_expr(base, functions);
            for part in [start, end, step].into_iter().flatten() {
                collect_anonymous_functions_from_expr(part, functions);
            }
        }
        ExprKind::ListComprehension {
            value,
            iterable,
            condition,
            ..
        } => {
            collect_anonymous_functions_from_expr(value, functions);
            collect_anonymous_functions_from_expr(iterable, functions);
            if let Some(condition) = condition {
                collect_anonymous_functions_from_expr(condition, functions);
            }
        }
        ExprKind::StructLiteral { base, fields, .. } => {
            if let Some(base) = base {
                collect_anonymous_functions_from_expr(base, functions);
            }
            for field in fields {
                collect_anonymous_functions_from_expr(&field.value, functions);
            }
        }
        ExprKind::Field { base, .. } | ExprKind::Unary { expr: base, .. } => {
            collect_anonymous_functions_from_expr(base, functions)
        }
        ExprKind::Match { value, arms } => {
            collect_anonymous_functions_from_expr(value, functions);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_anonymous_functions_from_expr(guard, functions);
                }
                collect_anonymous_functions_from_expr(&arm.value, functions);
            }
        }
        ExprKind::ListMatch { value, arms } => {
            collect_anonymous_functions_from_expr(value, functions);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_anonymous_functions_from_expr(guard, functions);
                }
                collect_anonymous_functions_from_expr(&arm.value, functions);
            }
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            collect_anonymous_functions_from_expr(then_expr, functions);
            collect_anonymous_functions_from_expr(cond, functions);
            collect_anonymous_functions_from_expr(else_expr, functions);
        }
        ExprKind::Binary { left, right, .. } => {
            collect_anonymous_functions_from_expr(left, functions);
            collect_anonymous_functions_from_expr(right, functions);
        }
        ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::Var(_) => {}
    }
}

fn source_span_key(span: SourceSpan) -> (u32, usize, usize, usize) {
    (span.source_id.value(), span.line, span.column, span.length)
}

fn emit_source_line(out: &mut String, span: SourceSpan, source_paths: &HashMap<SourceId, String>) {
    let path = source_paths
        .get(&span.source_id)
        .cloned()
        .unwrap_or_else(|| {
            if span.source_id == SourceId::UNKNOWN {
                "flux.flux".to_string()
            } else {
                format!("flux-source-{}.flux", span.source_id.value())
            }
        });
    let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
    out.push_str(&format!("#line {} \"{}\"\n", span.line.max(1), escaped));
}

fn emit_function(
    out: &mut String,
    function: &Function,
    signatures: &Signatures,
    cfg: &crate::ir::ControlFlowGraph,
    temp_counter: &mut usize,
    source_paths: &HashMap<SourceId, String>,
) -> Result<(), Diagnostic> {
    let reachable_spans = cfg
        .nodes()
        .iter()
        .filter(|node| cfg.is_reachable(node.id))
        .map(|node| source_span_key(node.span))
        .collect::<HashSet<_>>();
    let dead_assignment_spans = cfg
        .nodes()
        .iter()
        .filter(|node| cfg.is_reachable(node.id))
        .filter_map(|node| match &node.kind {
            crate::ir::ControlFlowNodeKind::Assignment { name }
                if cfg
                    .live_after(node.id)
                    .is_some_and(|state| !state.contains(name)) =>
            {
                Some(source_span_key(node.span))
            }
            _ => None,
        })
        .collect::<HashSet<_>>();
    let dead_var_initializer_spans = cfg
        .nodes()
        .iter()
        .filter(|node| cfg.is_reachable(node.id))
        .filter_map(|node| match &node.kind {
            crate::ir::ControlFlowNodeKind::Binding {
                name,
                mutable: true,
                ..
            } if cfg
                .live_after(node.id)
                .is_some_and(|state| !state.contains(name)) =>
            {
                Some(source_span_key(node.span))
            }
            _ => None,
        })
        .collect::<HashSet<_>>();
    let dead_definition_names = cfg
        .nodes()
        .iter()
        .filter(|node| cfg.is_reachable(node.id))
        .filter(|node| !node.definitions.is_empty())
        .filter_map(|node| {
            let live_after = cfg.live_after(node.id)?;
            let dead = node
                .definitions
                .iter()
                .filter(|definition| !live_after.contains(&definition.name))
                .map(|definition| definition.name.clone())
                .collect::<HashSet<_>>();
            (!dead.is_empty()).then_some((source_span_key(node.span), dead))
        })
        .collect::<HashMap<_, _>>();
    emit_source_line(out, function.span, source_paths);
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
        BlockEmitContext {
            current_function: function,
            source_paths,
            reachable_spans: &reachable_spans,
            dead_assignment_spans: &dead_assignment_spans,
            dead_var_initializer_spans: &dead_var_initializer_spans,
            dead_definition_names: &dead_definition_names,
        },
    )?;
    out.push_str("}\n");
    Ok(())
}

#[derive(Clone, Copy)]
struct BlockEmitContext<'a> {
    current_function: &'a Function,
    source_paths: &'a HashMap<SourceId, String>,
    reachable_spans: &'a HashSet<(u32, usize, usize, usize)>,
    dead_assignment_spans: &'a HashSet<(u32, usize, usize, usize)>,
    dead_var_initializer_spans: &'a HashSet<(u32, usize, usize, usize)>,
    dead_definition_names: &'a HashMap<(u32, usize, usize, usize), HashSet<String>>,
}

fn dead_store_rhs_is_discardable(expr: &Expr, signatures: &Signatures) -> bool {
    typecheck::constant_primitive_value(expr, signatures).is_some()
        || matches!(expr.kind, ExprKind::Nil | ExprKind::Var(_))
}

fn emit_block(
    out: &mut String,
    body: &[Stmt],
    depth: usize,
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
    context: BlockEmitContext<'_>,
) -> Result<(), Diagnostic> {
    for stmt in body {
        if !context
            .reachable_spans
            .contains(&source_span_key(stmt.span))
        {
            continue;
        }
        if let StmtKind::Assign { expr, .. } = &stmt.kind
            && context
                .dead_assignment_spans
                .contains(&source_span_key(stmt.span))
            && dead_store_rhs_is_discardable(expr, signatures)
        {
            continue;
        }
        emit_source_line(out, stmt.span, context.source_paths);
        let pad = "    ".repeat(depth);
        let dead_definitions = context
            .dead_definition_names
            .get(&source_span_key(stmt.span));
        match &stmt.kind {
            StmtKind::Var { name, ty, expr, .. }
                if context
                    .dead_var_initializer_spans
                    .contains(&source_span_key(stmt.span))
                    && dead_store_rhs_is_discardable(expr, signatures) =>
            {
                out.push_str(&format!(
                    "{pad}{} {};\n",
                    c_type(ty, signatures),
                    local_c_name(name)
                ));
                env.insert(name.clone(), signatures.canonical_type(ty));
            }
            StmtKind::Let { name, ty, expr, .. } if sequence_chunked(expr).is_some() => {
                emit_sequence_chunked_binding(
                    out,
                    &pad,
                    (name, ty),
                    expr,
                    env,
                    signatures,
                    temp_counter,
                )?;
            }
            StmtKind::Let { name, ty, expr, .. } if sequence_sorted(expr).is_some() => {
                emit_sequence_sorted_binding(
                    out,
                    &pad,
                    (name, ty),
                    expr,
                    env,
                    signatures,
                    temp_counter,
                )?;
            }
            StmtKind::Let { name, ty, expr, .. } if sequence_flatten(expr).is_some() => {
                emit_sequence_flatten_binding(
                    out,
                    &pad,
                    (name, ty),
                    expr,
                    env,
                    signatures,
                    temp_counter,
                )?;
            }
            StmtKind::Let { name, ty, expr, .. } if sequence_distinct(expr).is_some() => {
                emit_sequence_distinct_binding(
                    out,
                    &pad,
                    (name, ty),
                    expr,
                    env,
                    signatures,
                    temp_counter,
                )?;
            }
            StmtKind::Let { name, ty, expr, .. } if sequence_concat(expr).is_some() => {
                emit_sequence_concat_binding(
                    out,
                    &pad,
                    (name, ty),
                    expr,
                    env,
                    signatures,
                    temp_counter,
                )?;
            }
            StmtKind::Let { name, ty, expr, .. } if sequence_transform(expr).is_some() => {
                emit_sequence_transform_binding(
                    out,
                    &pad,
                    (name, ty),
                    expr,
                    env,
                    signatures,
                    temp_counter,
                )?;
            }
            StmtKind::Let { name, ty, expr, .. } | StmtKind::Var { name, ty, expr, .. }
                if sequence_reduction(expr).is_some() =>
            {
                emit_sequence_reduction_binding(
                    out,
                    &pad,
                    (name, ty),
                    expr,
                    env,
                    signatures,
                    temp_counter,
                )?;
            }
            StmtKind::Let { name, ty, expr, .. } if list_literal_needs_builder(expr) => {
                emit_list_builder_binding(
                    out,
                    &pad,
                    (name, ty),
                    expr,
                    env,
                    signatures,
                    temp_counter,
                )?;
            }
            StmtKind::Let { name, ty, expr, .. }
                if matches!(expr.kind, ExprKind::ListComprehension { .. }) =>
            {
                emit_list_comprehension_binding(
                    out,
                    &pad,
                    (name, ty),
                    expr,
                    env,
                    signatures,
                    temp_counter,
                )?;
            }
            StmtKind::Let { name, ty, expr, .. } | StmtKind::Var { name, ty, expr, .. }
                if matches!(
                    expr.kind,
                    ExprKind::Match { .. } | ExprKind::ListMatch { .. }
                ) =>
            {
                let target = local_c_name(name);
                out.push_str(&format!("{pad}{} {target};\n", c_type(ty, signatures)));
                emit_match_expr_into(out, expr, &target, depth, env, signatures, temp_counter)?;
                env.insert(name.clone(), signatures.canonical_type(ty));
            }
            StmtKind::Let { name, ty, expr, .. } | StmtKind::Var { name, ty, expr, .. } => {
                let value = emit_expr(expr, env, signatures)?;
                out.push_str(&format!(
                    "{pad}{} {} = {};\n",
                    c_type(ty, signatures),
                    local_c_name(name),
                    value.code
                ));
                env.insert(name.clone(), signatures.canonical_type(ty));
            }
            StmtKind::Assign { name, expr, .. } => {
                let value = emit_expr(expr, env, signatures)?;
                out.push_str(&format!("{pad}{} = {};\n", local_c_name(name), value.code));
            }
            StmtKind::LetDestructure {
                bindings,
                expr,
                else_return,
            } => {
                let (value, tag, _) = emit_multi_expr(expr, env, signatures)?;
                let temp = format!("flux__multi_{}", *temp_counter);
                *temp_counter += 1;
                out.push_str(&format!("{pad}struct {tag} {temp} = {value};\n"));
                if *else_return {
                    let error_index = bindings.len() - 1;
                    let return_tag = multi_return_struct_name(&context.current_function.name);
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
                    if dead_definitions.is_some_and(|dead| dead.contains(&binding.name)) {
                        continue;
                    }
                    out.push_str(&format!(
                        "{pad}{} {} = {temp}.v{index};\n",
                        c_type(&binding.ty, signatures),
                        local_c_name(&binding.name)
                    ));
                    env.insert(binding.name.clone(), signatures.canonical_type(&binding.ty));
                }
            }
            StmtKind::LetMultiDestructure {
                bindings,
                expr,
                else_return,
            } => {
                let (value, tag, actuals) = emit_multi_expr(expr, env, signatures)?;
                let temp = format!("flux__multi_pattern_{}", *temp_counter);
                *temp_counter += 1;
                out.push_str(&format!("{pad}struct {tag} {temp} = {value};\n"));
                if *else_return {
                    let error_index = bindings.len() - 1;
                    let return_tag = multi_return_struct_name(&context.current_function.name);
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
                    if binding.name == "_"
                        || dead_definitions.is_some_and(|dead| dead.contains(&binding.name))
                    {
                        continue;
                    }
                    let ty = actuals.get(index).ok_or_else(|| {
                        diag(
                            stmt.span,
                            "multi-value pattern arity changed after type checking",
                        )
                    })?;
                    out.push_str(&format!(
                        "{pad}{} {} = {temp}.v{index};\n",
                        c_type(ty, signatures),
                        local_c_name(&binding.name)
                    ));
                    env.insert(binding.name.clone(), signatures.canonical_type(ty));
                }
            }
            StmtKind::LetListDestructure {
                bindings,
                rest,
                expr,
            } => {
                let value = emit_expr(expr, env, signatures)?;
                let Type::List(element) = &value.ty else {
                    return Err(diag(
                        stmt.span,
                        "list destructuring code generation requires a list value",
                    ));
                };
                let temp = format!("flux__list_pattern_{}", *temp_counter);
                *temp_counter += 1;
                let element_c = c_type(element, signatures);
                out.push_str(&format!(
                    "{pad}struct flux__list {temp} = {};\n",
                    value.code
                ));
                if rest.is_some() {
                    out.push_str(&format!(
                        "{pad}if ({temp}.len < {}) {{ fputs(\"Flux runtime error: list pattern requires at least {} elements\\n\", stderr); abort(); }}\n",
                        bindings.len(),
                        bindings.len()
                    ));
                } else {
                    out.push_str(&format!(
                        "{pad}if ({temp}.len != {}) {{ fputs(\"Flux runtime error: list pattern requires exactly {} elements\\n\", stderr); abort(); }}\n",
                        bindings.len(),
                        bindings.len()
                    ));
                }
                for (index, binding) in bindings.iter().enumerate() {
                    if binding.name == "_"
                        || dead_definitions.is_some_and(|dead| dead.contains(&binding.name))
                    {
                        continue;
                    }
                    let index_code = if let Some(rest) = rest {
                        if index < rest.index {
                            index.to_string()
                        } else {
                            format!("{temp}.len - {}", bindings.len() - index)
                        }
                    } else {
                        index.to_string()
                    };
                    out.push_str(&format!(
                        "{pad}{element_c} {} = *(({element_c} *)flux_list_at_unchecked({temp}, {index_code}, sizeof({element_c})));\n",
                        local_c_name(&binding.name)
                    ));
                    env.insert(binding.name.clone(), (**element).clone());
                }
                if let Some(rest) = rest
                    && rest.binding.name != "_"
                    && !dead_definitions.is_some_and(|dead| dead.contains(&rest.binding.name))
                {
                    let rest_name = local_c_name(&rest.binding.name);
                    out.push_str(&format!(
                        "{pad}struct flux__list {rest_name} = {{ .data = {temp}.data, .len = {temp}.len - {}, .stride = flux_list_stride({temp}, sizeof({element_c})) }};\n",
                        bindings.len()
                    ));
                    out.push_str(&format!(
                        "{pad}if ({rest_name}.len != 0) {{ {rest_name}.data = flux_list_at_unchecked({temp}, {}, sizeof({element_c})); }}\n",
                        rest.index
                    ));
                    env.insert(
                        rest.binding.name.clone(),
                        Type::List(Box::new((**element).clone())),
                    );
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
                    &mut PatternBindingEmitContext {
                        env,
                        signatures,
                        dead_definitions,
                    },
                )?;
            }
            StmtKind::Return(values) if values.is_empty() => {
                out.push_str(&format!("{pad}return;\n"));
            }
            StmtKind::Return(values)
                if values.len() == 1 && context.current_function.returns.len() > 1 =>
            {
                let (value, source_tag, _) = emit_multi_expr(&values[0], env, signatures)?;
                let source_temp = format!("flux__forward_{}", *temp_counter);
                *temp_counter += 1;
                let return_tag = multi_return_struct_name(&context.current_function.name);
                let return_temp = format!("flux__return_{}", *temp_counter);
                *temp_counter += 1;
                out.push_str(&format!(
                    "{pad}struct {source_tag} {source_temp} = {value};\n"
                ));
                out.push_str(&format!("{pad}struct {return_tag} {return_temp};\n"));
                for index in 0..context.current_function.returns.len() {
                    out.push_str(&format!(
                        "{pad}{return_temp}.v{index} = {source_temp}.v{index};\n"
                    ));
                }
                out.push_str(&format!("{pad}return {return_temp};\n"));
            }
            StmtKind::Return(values)
                if values.len() == 1
                    && matches!(
                        values[0].kind,
                        ExprKind::Match { .. } | ExprKind::ListMatch { .. }
                    ) =>
            {
                let result_ty = type_of_expr(&values[0], env, signatures)?;
                let target = format!("flux__match_result_{}", *temp_counter);
                *temp_counter += 1;
                out.push_str(&format!(
                    "{pad}{} {target};\n",
                    c_type(&result_ty, signatures)
                ));
                emit_match_expr_into(
                    out,
                    &values[0],
                    &target,
                    depth,
                    env,
                    signatures,
                    temp_counter,
                )?;
                out.push_str(&format!("{pad}return {target};\n"));
            }
            StmtKind::Return(values) if values.len() == 1 => {
                let value = emit_expr(&values[0], env, signatures)?;
                out.push_str(&format!("{pad}return {};\n", value.code));
            }
            StmtKind::Return(values) => {
                let tag = multi_return_struct_name(&context.current_function.name);
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
            StmtKind::Shell {
                expr,
                redirect,
                background,
            } => {
                let value = emit_expr(expr, env, signatures)?;
                let emit_command = |out: &mut String,
                                    command_pad: &str|
                 -> Result<(), Diagnostic> {
                    if let Some(redirect) = redirect {
                        let path = emit_expr(&redirect.path, env, signatures)?;
                        let helper = match value.ty {
                            Type::I64 => "flux_redirect_i64",
                            Type::Bool => "flux_redirect_bool",
                            Type::Str => "flux_redirect_str",
                            Type::Error => "flux_redirect_error",
                            _ => {
                                return Err(diag(
                                    expr.span,
                                    "redirection reached code generation with a non-scalar result",
                                ));
                            }
                        };
                        let append = matches!(redirect.mode, ShellRedirectMode::Append);
                        out.push_str(&format!(
                            "{command_pad}{helper}({}, {}, {});\n",
                            path.code,
                            if append { "true" } else { "false" },
                            value.code
                        ));
                    } else {
                        out.push_str(&format!("{command_pad}{};\n", value.code));
                    }
                    Ok(())
                };
                if *background {
                    let pid = format!("flux__bg_pid_{}", *temp_counter);
                    *temp_counter += 1;
                    let worker = format!("flux__bg_worker_{}", *temp_counter);
                    *temp_counter += 1;
                    out.push_str(&format!("{pad}pid_t {pid} = fork();\n"));
                    out.push_str(&format!(
                        "{pad}if ({pid} < 0) {{ perror(\"fork\"); abort(); }}\n"
                    ));
                    out.push_str(&format!("{pad}if ({pid} == 0) {{\n"));
                    out.push_str(&format!("{pad}    pid_t {worker} = fork();\n"));
                    out.push_str(&format!("{pad}    if ({worker} < 0) _exit(127);\n"));
                    out.push_str(&format!("{pad}    if ({worker} > 0) _exit(0);\n"));
                    emit_command(out, &format!("{pad}    "))?;
                    out.push_str(&format!("{pad}    fflush(NULL);\n"));
                    out.push_str(&format!("{pad}    _exit(0);\n"));
                    out.push_str(&format!("{pad}}}\n"));
                    out.push_str(&format!(
                        "{pad}while (waitpid({pid}, NULL, 0) < 0 && errno == EINTR) {{}}\n"
                    ));
                } else {
                    emit_command(out, &pad)?;
                }
            }
            StmtKind::If {
                cond,
                body,
                else_body,
                ..
            } => {
                if let Some(ConstantValue::Bool(condition)) =
                    fold_primitive_expr(cond, env, signatures)?
                {
                    let selected = if condition { body } else { else_body };
                    let mut nested = env.clone();
                    emit_block(
                        out,
                        selected,
                        depth,
                        &mut nested,
                        signatures,
                        temp_counter,
                        context,
                    )?;
                } else {
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
                        context,
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
                            context,
                        )?;
                        out.push_str(&format!("{pad}}}\n"));
                    }
                }
            }
            StmtKind::While { cond, body } => {
                if matches!(
                    fold_primitive_expr(cond, env, signatures)?,
                    Some(ConstantValue::Bool(false))
                ) {
                    continue;
                }
                let cond = emit_expr(cond, env, signatures)?;
                out.push_str(&format!("{pad}while {} {{\n", c_condition(&cond.code)));
                let mut nested = env.clone();
                emit_block(
                    out,
                    body,
                    depth + 1,
                    &mut nested,
                    signatures,
                    temp_counter,
                    context,
                )?;
                out.push_str(&format!("{pad}}}\n"));
            }
            StmtKind::ForRange {
                name,
                start,
                end,
                inclusive,
                body,
                ..
            } => {
                let folded_start = fold_primitive_expr(start, env, signatures)?;
                let folded_end = fold_primitive_expr(end, env, signatures)?;
                if let (Some(ConstantValue::I64(start_value)), Some(ConstantValue::I64(end_value))) =
                    (&folded_start, &folded_end)
                    && if *inclusive {
                        start_value > end_value
                    } else {
                        start_value >= end_value
                    }
                {
                    continue;
                }
                let start = emit_expr(start, env, signatures)?;
                let end = emit_expr(end, env, signatures)?;
                let temp = format!("flux__end_{}", *temp_counter);
                *temp_counter += 1;
                let name_is_live = !dead_definitions.is_some_and(|dead| dead.contains(name));
                let c_name = if name_is_live {
                    local_c_name(name)
                } else {
                    let generated = format!("flux__range_index_{}", *temp_counter);
                    *temp_counter += 1;
                    generated
                };
                if *inclusive {
                    let done = format!("flux__range_done_{}", *temp_counter);
                    *temp_counter += 1;
                    out.push_str(&format!(
                        "{pad}for (int64_t {c_name} = {}, {temp} = {}, {done} = 0; !{done} && {c_name} <= {temp}; {done} = ({c_name} == {temp}), {c_name} += !{done}) {{\n",
                        start.code, end.code
                    ));
                } else {
                    out.push_str(&format!(
                        "{pad}for (int64_t {c_name} = {}, {temp} = {}; {c_name} < {temp}; ++{c_name}) {{\n",
                        start.code, end.code
                    ));
                }
                let mut nested = env.clone();
                if name_is_live {
                    nested.insert(name.clone(), Type::I64);
                }
                emit_block(
                    out,
                    body,
                    depth + 1,
                    &mut nested,
                    signatures,
                    temp_counter,
                    context,
                )?;
                out.push_str(&format!("{pad}}}\n"));
            }
            StmtKind::ForEach {
                index_name,
                name,
                iterable,
                body,
                ..
            } => {
                let source = emit_expr(iterable, env, signatures)?;
                let Type::List(element) = signatures.canonical_type(&source.ty) else {
                    return Err(diag(
                        stmt.span,
                        "for-loop code generation requires a list source",
                    ));
                };
                let source_name = format!("flux__iter_source_{}", *temp_counter);
                *temp_counter += 1;
                let index_is_live = index_name.as_ref().is_some_and(|index| {
                    !dead_definitions.is_some_and(|dead| dead.contains(index))
                });
                let index_c = if index_is_live {
                    local_c_name(index_name.as_ref().expect("live index must be named"))
                } else {
                    let generated = format!("flux__iter_index_{}", *temp_counter);
                    *temp_counter += 1;
                    generated
                };
                let item_is_live = !dead_definitions.is_some_and(|dead| dead.contains(name));
                let item_c = local_c_name(name);
                let element_c = c_type(&element, signatures);
                out.push_str(&format!(
                    "{pad}struct flux__list {source_name} = {};\n",
                    source.code
                ));
                out.push_str(&format!(
                    "{pad}for (int64_t {index_c} = 0; {index_c} < (int64_t){source_name}.len; ++{index_c}) {{\n"
                ));
                if item_is_live {
                    out.push_str(&format!(
                        "{pad}    {element_c} {item_c} = *(({element_c} *)flux_list_at_unchecked({source_name}, (size_t){index_c}, sizeof({element_c})));\n"
                    ));
                }
                let mut nested = env.clone();
                if index_is_live && let Some(index_name) = index_name {
                    nested.insert(index_name.clone(), Type::I64);
                }
                if item_is_live {
                    nested.insert(name.clone(), *element);
                }
                emit_block(
                    out,
                    body,
                    depth + 1,
                    &mut nested,
                    signatures,
                    temp_counter,
                    context,
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
                for variant in &definition.variants {
                    let variant_arms = arms
                        .iter()
                        .filter(|arm| arm.variant == variant.name)
                        .collect::<Vec<_>>();
                    if variant_arms.is_empty() {
                        continue;
                    }
                    out.push_str(&format!(
                        "{pad}    case {}: {{\n",
                        enum_tag_value_name(enum_name, &variant.name)
                    ));
                    for arm in variant_arms {
                        out.push_str(&format!("{pad}        {{\n"));
                        let mut nested = env.clone();
                        let dead_arm_definitions = context
                            .dead_definition_names
                            .get(&source_span_key(arm.span));
                        for (index, (pattern, payload_ty)) in
                            arm.patterns.iter().zip(&variant.payloads).enumerate()
                        {
                            let payload_access = format!(
                                "{temp}.payload.{}.v{index}",
                                enum_payload_member_name(&arm.variant)
                            );
                            match pattern {
                                MatchPattern::Binding(binding) => {
                                    if binding.name == "_"
                                        || dead_arm_definitions
                                            .is_some_and(|dead| dead.contains(&binding.name))
                                    {
                                        continue;
                                    }
                                    out.push_str(&format!(
                                        "{pad}            {} {} = {payload_access};\n",
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
                                        &format!("{pad}            "),
                                        &pattern.fields,
                                        &struct_name,
                                        &payload_access,
                                        &mut PatternBindingEmitContext {
                                            env: &mut nested,
                                            signatures,
                                            dead_definitions: dead_arm_definitions,
                                        },
                                    )?;
                                }
                            }
                        }
                        if let Some(guard) = &arm.guard {
                            let guard = emit_expr(guard, &nested, signatures)?;
                            out.push_str(&format!(
                                "{pad}            if ({}) {{\n",
                                c_condition(&guard.code)
                            ));
                            emit_block(
                                out,
                                &arm.body,
                                depth + 4,
                                &mut nested,
                                signatures,
                                temp_counter,
                                context,
                            )?;
                            out.push_str(&format!("{pad}                break;\n"));
                            out.push_str(&format!("{pad}            }}\n"));
                        } else {
                            emit_block(
                                out,
                                &arm.body,
                                depth + 3,
                                &mut nested,
                                signatures,
                                temp_counter,
                                context,
                            )?;
                            out.push_str(&format!("{pad}            break;\n"));
                        }
                        out.push_str(&format!("{pad}        }}\n"));
                    }
                    out.push_str(&format!("{pad}    }}\n"));
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            StmtKind::ListMatch { value, arms } => {
                let value = emit_expr(value, env, signatures)?;
                let Type::List(element) = &value.ty else {
                    return Err(diag(
                        stmt.span,
                        "list match code generation requires a list value",
                    ));
                };
                let temp = format!("flux__list_match_{}", *temp_counter);
                *temp_counter += 1;
                out.push_str(&format!(
                    "{pad}struct flux__list {temp} = {};\n",
                    value.code
                ));
                if arms.iter().all(|arm| arm.guard.is_none()) {
                    for (arm_index, arm) in arms.iter().enumerate() {
                        let condition = list_match_condition(&arm.pattern, &temp);
                        if arm_index == 0 {
                            out.push_str(&format!("{pad}if ({condition}) {{\n"));
                        } else if matches!(arm.pattern, ListMatchPattern::Wildcard { .. }) {
                            out.push_str(&format!("{pad}else {{\n"));
                        } else {
                            out.push_str(&format!("{pad}else if ({condition}) {{\n"));
                        }
                        let mut nested = env.clone();
                        let dead_arm_definitions = context
                            .dead_definition_names
                            .get(&source_span_key(arm.span));
                        emit_list_match_pattern_bindings(
                            out,
                            &format!("{pad}    "),
                            &arm.pattern,
                            &temp,
                            element,
                            &mut PatternBindingEmitContext {
                                env: &mut nested,
                                signatures,
                                dead_definitions: dead_arm_definitions,
                            },
                        )?;
                        emit_block(
                            out,
                            &arm.body,
                            depth + 1,
                            &mut nested,
                            signatures,
                            temp_counter,
                            context,
                        )?;
                        out.push_str(&format!("{pad}}}\n"));
                    }
                } else {
                    let matched = format!("flux__list_match_done_{}", *temp_counter);
                    *temp_counter += 1;
                    out.push_str(&format!("{pad}bool {matched} = false;\n"));
                    for arm in arms {
                        let condition = list_match_condition(&arm.pattern, &temp);
                        out.push_str(&format!("{pad}if (!{matched} && ({condition})) {{\n"));
                        let mut nested = env.clone();
                        let dead_arm_definitions = context
                            .dead_definition_names
                            .get(&source_span_key(arm.span));
                        emit_list_match_pattern_bindings(
                            out,
                            &format!("{pad}    "),
                            &arm.pattern,
                            &temp,
                            element,
                            &mut PatternBindingEmitContext {
                                env: &mut nested,
                                signatures,
                                dead_definitions: dead_arm_definitions,
                            },
                        )?;
                        if let Some(guard) = &arm.guard {
                            let guard = emit_expr(guard, &nested, signatures)?;
                            out.push_str(&format!(
                                "{pad}    if ({}) {{\n",
                                c_condition(&guard.code)
                            ));
                            out.push_str(&format!("{pad}        {matched} = true;\n"));
                            emit_block(
                                out,
                                &arm.body,
                                depth + 2,
                                &mut nested,
                                signatures,
                                temp_counter,
                                context,
                            )?;
                            out.push_str(&format!("{pad}    }}\n"));
                        } else {
                            out.push_str(&format!("{pad}    {matched} = true;\n"));
                            emit_block(
                                out,
                                &arm.body,
                                depth + 1,
                                &mut nested,
                                signatures,
                                temp_counter,
                                context,
                            )?;
                        }
                        out.push_str(&format!("{pad}}}\n"));
                    }
                }
            }
        }
    }
    Ok(())
}

struct EmittedExpr {
    code: String,
    ty: Type,
}

fn list_match_condition(pattern: &ListMatchPattern, temp: &str) -> String {
    match pattern {
        ListMatchPattern::Wildcard { .. } => "true".to_string(),
        ListMatchPattern::List { bindings, rest, .. } => {
            if rest.is_some() {
                format!("{temp}.len >= {}", bindings.len())
            } else {
                format!("{temp}.len == {}", bindings.len())
            }
        }
    }
}

struct PatternBindingEmitContext<'a> {
    env: &'a mut HashMap<String, Type>,
    signatures: &'a Signatures,
    dead_definitions: Option<&'a HashSet<String>>,
}

fn emit_list_match_pattern_bindings(
    out: &mut String,
    pad: &str,
    pattern: &ListMatchPattern,
    temp: &str,
    element: &Type,
    context: &mut PatternBindingEmitContext<'_>,
) -> Result<(), Diagnostic> {
    let ListMatchPattern::List { bindings, rest, .. } = pattern else {
        return Ok(());
    };
    let element_c = c_type(element, context.signatures);
    for (index, binding) in bindings.iter().enumerate() {
        if binding.name == "_"
            || context
                .dead_definitions
                .is_some_and(|dead| dead.contains(&binding.name))
        {
            continue;
        }
        let index_code = if let Some(rest) = rest {
            if index < rest.index {
                index.to_string()
            } else {
                format!("{temp}.len - {}", bindings.len() - index)
            }
        } else {
            index.to_string()
        };
        out.push_str(&format!(
            "{pad}{element_c} {} = *(({element_c} *)flux_list_at_unchecked({temp}, {index_code}, sizeof({element_c})));\n",
            local_c_name(&binding.name)
        ));
        context.env.insert(binding.name.clone(), element.clone());
    }
    if let Some(rest) = rest
        && rest.binding.name != "_"
        && !context
            .dead_definitions
            .is_some_and(|dead| dead.contains(&rest.binding.name))
    {
        let rest_name = local_c_name(&rest.binding.name);
        out.push_str(&format!(
            "{pad}struct flux__list {rest_name} = {{ .data = {temp}.data, .len = {temp}.len - {}, .stride = flux_list_stride({temp}, sizeof({element_c})) }};\n",
            bindings.len()
        ));
        out.push_str(&format!(
            "{pad}if ({rest_name}.len != 0) {{ {rest_name}.data = flux_list_at_unchecked({temp}, {}, sizeof({element_c})); }}\n",
            rest.index
        ));
        context.env.insert(
            rest.binding.name.clone(),
            Type::List(Box::new(element.clone())),
        );
    }
    Ok(())
}

fn emit_match_expr_into(
    out: &mut String,
    expr: &Expr,
    target: &str,
    depth: usize,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<(), Diagnostic> {
    if matches!(expr.kind, ExprKind::ListMatch { .. }) {
        return emit_list_match_expr_into(out, expr, target, depth, env, signatures, temp_counter);
    }
    let ExprKind::Match { value, arms } = &expr.kind else {
        return Err(diag(
            expr.span,
            "expected match expression during code generation",
        ));
    };
    let emitted_value = emit_expr(value, env, signatures)?;
    let Type::Named(enum_name) = &emitted_value.ty else {
        return Err(diag(
            value.span,
            "match expression code generation requires an enum value",
        ));
    };
    let definition = signatures.enum_type(enum_name).ok_or_else(|| {
        diag(
            value.span,
            "match expression code generation requires an enum value",
        )
    })?;
    let temp = format!("flux__match_{}", *temp_counter);
    *temp_counter += 1;
    let pad = "    ".repeat(depth);
    out.push_str(&format!(
        "{pad}struct {} {temp} = {};\n",
        struct_c_name(enum_name),
        emitted_value.code
    ));
    out.push_str(&format!("{pad}switch ({temp}.tag) {{\n"));
    for variant in &definition.variants {
        let variant_arms = arms
            .iter()
            .filter(|arm| arm.variant == variant.name)
            .collect::<Vec<_>>();
        if variant_arms.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "{pad}    case {}: {{\n",
            enum_tag_value_name(enum_name, &variant.name)
        ));
        for arm in variant_arms {
            out.push_str(&format!("{pad}        {{\n"));
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
                            "{pad}            {} {} = {payload_access};\n",
                            c_type(payload_ty, signatures),
                            local_c_name(&binding.name),
                        ));
                        nested.insert(binding.name.clone(), payload_ty.clone());
                    }
                    MatchPattern::Struct(pattern) => {
                        let Type::Named(struct_name) = signatures.canonical_type(payload_ty) else {
                            return Err(diag(
                                pattern.struct_span,
                                "match struct pattern code generation requires a struct value",
                            ));
                        };
                        emit_struct_pattern_bindings(
                            out,
                            &format!("{pad}            "),
                            &pattern.fields,
                            &struct_name,
                            &payload_access,
                            &mut PatternBindingEmitContext {
                                env: &mut nested,
                                signatures,
                                dead_definitions: None,
                            },
                        )?;
                    }
                }
            }
            if let Some(guard) = &arm.guard {
                let guard = emit_expr(guard, &nested, signatures)?;
                out.push_str(&format!(
                    "{pad}            if ({}) {{\n",
                    c_condition(&guard.code)
                ));
                let arm_value = emit_expr(&arm.value, &nested, signatures)?;
                out.push_str(&format!(
                    "{pad}                {target} = {};\n",
                    arm_value.code
                ));
                out.push_str(&format!("{pad}                break;\n"));
                out.push_str(&format!("{pad}            }}\n"));
            } else {
                let arm_value = emit_expr(&arm.value, &nested, signatures)?;
                out.push_str(&format!(
                    "{pad}            {target} = {};\n",
                    arm_value.code
                ));
                out.push_str(&format!("{pad}            break;\n"));
            }
            out.push_str(&format!("{pad}        }}\n"));
        }
        out.push_str(&format!("{pad}    }}\n"));
    }
    out.push_str(&format!("{pad}}}\n"));
    Ok(())
}

fn emit_list_match_expr_into(
    out: &mut String,
    expr: &Expr,
    target: &str,
    depth: usize,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<(), Diagnostic> {
    let ExprKind::ListMatch { value, arms } = &expr.kind else {
        return Err(diag(
            expr.span,
            "expected list match expression during code generation",
        ));
    };
    let emitted_value = emit_expr(value, env, signatures)?;
    let Type::List(element) = &emitted_value.ty else {
        return Err(diag(
            value.span,
            "list match expression code generation requires a list value",
        ));
    };
    let temp = format!("flux__list_match_{}", *temp_counter);
    *temp_counter += 1;
    let pad = "    ".repeat(depth);
    out.push_str(&format!(
        "{pad}struct flux__list {temp} = {};\n",
        emitted_value.code
    ));
    if arms.iter().all(|arm| arm.guard.is_none()) {
        for (arm_index, arm) in arms.iter().enumerate() {
            let condition = list_match_condition(&arm.pattern, &temp);
            if arm_index == 0 {
                out.push_str(&format!("{pad}if ({condition}) {{\n"));
            } else if matches!(arm.pattern, ListMatchPattern::Wildcard { .. }) {
                out.push_str(&format!("{pad}else {{\n"));
            } else {
                out.push_str(&format!("{pad}else if ({condition}) {{\n"));
            }
            let mut nested = env.clone();
            emit_list_match_pattern_bindings(
                out,
                &format!("{pad}    "),
                &arm.pattern,
                &temp,
                element,
                &mut PatternBindingEmitContext {
                    env: &mut nested,
                    signatures,
                    dead_definitions: None,
                },
            )?;
            let arm_value = emit_expr(&arm.value, &nested, signatures)?;
            out.push_str(&format!("{pad}    {target} = {};\n", arm_value.code));
            out.push_str(&format!("{pad}}}\n"));
        }
    } else {
        let matched = format!("flux__list_match_done_{}", *temp_counter);
        *temp_counter += 1;
        out.push_str(&format!("{pad}bool {matched} = false;\n"));
        for arm in arms {
            let condition = list_match_condition(&arm.pattern, &temp);
            out.push_str(&format!("{pad}if (!{matched} && ({condition})) {{\n"));
            let mut nested = env.clone();
            emit_list_match_pattern_bindings(
                out,
                &format!("{pad}    "),
                &arm.pattern,
                &temp,
                element,
                &mut PatternBindingEmitContext {
                    env: &mut nested,
                    signatures,
                    dead_definitions: None,
                },
            )?;
            if let Some(guard) = &arm.guard {
                let guard = emit_expr(guard, &nested, signatures)?;
                out.push_str(&format!("{pad}    if ({}) {{\n", c_condition(&guard.code)));
                let arm_value = emit_expr(&arm.value, &nested, signatures)?;
                out.push_str(&format!("{pad}        {target} = {};\n", arm_value.code));
                out.push_str(&format!("{pad}        {matched} = true;\n"));
                out.push_str(&format!("{pad}    }}\n"));
            } else {
                let arm_value = emit_expr(&arm.value, &nested, signatures)?;
                out.push_str(&format!("{pad}    {target} = {};\n", arm_value.code));
                out.push_str(&format!("{pad}    {matched} = true;\n"));
            }
            out.push_str(&format!("{pad}}}\n"));
        }
    }
    Ok(())
}

fn emit_struct_pattern_bindings(
    out: &mut String,
    pad: &str,
    fields: &[StructPatternField],
    struct_name: &str,
    base: &str,
    context: &mut PatternBindingEmitContext<'_>,
) -> Result<(), Diagnostic> {
    let definition = context
        .signatures
        .struct_type(struct_name)
        .expect("type checking guarantees struct pattern type exists");
    for field in fields {
        let field_signature = definition
            .field(&field.field)
            .expect("type checking guarantees struct pattern fields exist");
        let access = format!("{base}.{}", field_c_name(&field.field));
        if let Some(nested) = &field.nested {
            let Type::Named(nested_name) = context.signatures.canonical_type(&field_signature.ty)
            else {
                return Err(diag(
                    nested.struct_span,
                    "nested struct pattern code generation requires a struct value",
                ));
            };
            emit_struct_pattern_bindings(out, pad, &nested.fields, &nested_name, &access, context)?;
            continue;
        }
        if field.binding.name == "_"
            || context
                .dead_definitions
                .is_some_and(|dead| dead.contains(&field.binding.name))
        {
            continue;
        }
        out.push_str(&format!(
            "{pad}{} {} = {access};\n",
            c_type(&field_signature.ty, context.signatures),
            local_c_name(&field.binding.name),
        ));
        context
            .env
            .insert(field.binding.name.clone(), field_signature.ty.clone());
    }
    Ok(())
}

enum SequenceTransform<'a> {
    Map { list: &'a Expr, callback: &'a Expr },
    Filter { list: &'a Expr, callback: &'a Expr },
}

fn sequence_transform(expr: &Expr) -> Option<SequenceTransform<'_>> {
    match &expr.kind {
        ExprKind::Call {
            name,
            args,
            named_args,
        } if named_args.is_empty() && name == "map" && args.len() == 2 => {
            Some(SequenceTransform::Map {
                list: &args[0],
                callback: &args[1],
            })
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if named_args.is_empty() && (name == "filter" || name == "where") && args.len() == 2 => {
            Some(SequenceTransform::Filter {
                list: &args[0],
                callback: &args[1],
            })
        }
        ExprKind::Pipe {
            input, name, args, ..
        } if name == "map" && args.len() == 1 => Some(SequenceTransform::Map {
            list: input,
            callback: &args[0],
        }),
        ExprKind::Pipe {
            input, name, args, ..
        } if (name == "filter" || name == "where") && args.len() == 1 => {
            Some(SequenceTransform::Filter {
                list: input,
                callback: &args[0],
            })
        }
        _ => None,
    }
}

fn sequence_chunked(expr: &Expr) -> Option<(&Expr, &Expr)> {
    match &expr.kind {
        ExprKind::Call {
            name,
            args,
            named_args,
        } if named_args.is_empty() && name == "chunked" && args.len() == 2 => {
            Some((&args[0], &args[1]))
        }
        ExprKind::Pipe {
            input, name, args, ..
        } if name == "chunked" && args.len() == 1 => Some((input, &args[0])),
        _ => None,
    }
}

fn sequence_sorted(expr: &Expr) -> Option<&Expr> {
    match &expr.kind {
        ExprKind::Call {
            name,
            args,
            named_args,
        } if named_args.is_empty() && name == "sorted" && args.len() == 1 => Some(&args[0]),
        ExprKind::Pipe {
            input, name, args, ..
        } if name == "sorted" && args.is_empty() => Some(input),
        _ => None,
    }
}

fn sequence_flatten(expr: &Expr) -> Option<&Expr> {
    match &expr.kind {
        ExprKind::Call {
            name,
            args,
            named_args,
        } if named_args.is_empty() && name == "flatten" && args.len() == 1 => Some(&args[0]),
        ExprKind::Pipe {
            input, name, args, ..
        } if name == "flatten" && args.is_empty() => Some(input),
        _ => None,
    }
}

fn sequence_distinct(expr: &Expr) -> Option<&Expr> {
    match &expr.kind {
        ExprKind::Call {
            name,
            args,
            named_args,
        } if named_args.is_empty() && name == "distinct" && args.len() == 1 => Some(&args[0]),
        ExprKind::Pipe {
            input, name, args, ..
        } if name == "distinct" && args.is_empty() => Some(input),
        _ => None,
    }
}

fn sequence_concat(expr: &Expr) -> Option<(&Expr, &Expr)> {
    match &expr.kind {
        ExprKind::Call {
            name,
            args,
            named_args,
        } if named_args.is_empty() && name == "concat" && args.len() == 2 => {
            Some((&args[0], &args[1]))
        }
        ExprKind::Pipe {
            input, name, args, ..
        } if name == "concat" && args.len() == 1 => Some((input, &args[0])),
        _ => None,
    }
}

fn emit_sequence_list_value(
    out: &mut String,
    pad: &str,
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<EmittedExpr, Diagnostic> {
    if sequence_transform(expr).is_some() {
        emit_sequence_transform_value(out, pad, expr, env, signatures, temp_counter)
    } else if sequence_chunked(expr).is_some() {
        emit_sequence_chunked_value(out, pad, expr, env, signatures, temp_counter)
    } else if sequence_concat(expr).is_some() {
        emit_sequence_concat_value(out, pad, expr, env, signatures, temp_counter)
    } else if sequence_distinct(expr).is_some() {
        emit_sequence_distinct_value(out, pad, expr, env, signatures, temp_counter)
    } else if sequence_flatten(expr).is_some() {
        emit_sequence_flatten_value(out, pad, expr, env, signatures, temp_counter)
    } else if sequence_sorted(expr).is_some() {
        emit_sequence_sorted_value(out, pad, expr, env, signatures, temp_counter)
    } else {
        emit_expr(expr, env, signatures)
    }
}

fn emit_sequence_chunked_binding(
    out: &mut String,
    pad: &str,
    target: (&str, &Type),
    expr: &Expr,
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<(), Diagnostic> {
    let (name, declared_ty) = target;
    let value = emit_sequence_chunked_value(out, pad, expr, env, signatures, temp_counter)?;
    let result_ty = signatures.canonical_type(declared_ty);
    if value.ty != result_ty {
        return Err(diag(
            expr.span,
            "chunked binding type mismatch reached code generation",
        ));
    }
    out.push_str(&format!(
        "{pad}{} {} = {};\n",
        c_type(declared_ty, signatures),
        local_c_name(name),
        value.code
    ));
    env.insert(name.to_string(), result_ty);
    Ok(())
}

fn emit_sequence_chunked_value(
    out: &mut String,
    pad: &str,
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<EmittedExpr, Diagnostic> {
    let (source_expr, size_expr) = sequence_chunked(expr)
        .ok_or_else(|| diag(expr.span, "invalid chunked call reached code generation"))?;
    let source = emit_sequence_list_value(out, pad, source_expr, env, signatures, temp_counter)?;
    let source_ty = signatures.canonical_type(&source.ty);
    let Type::List(_) = &source_ty else {
        return Err(diag(expr.span, "chunked source must be a list"));
    };
    let size = emit_expr(size_expr, env, signatures)?;
    if size.ty != Type::I64 {
        return Err(diag(expr.span, "chunked size must be i64"));
    }
    let result_ty = signatures.canonical_type(&type_of_expr(expr, env, signatures)?);
    if result_ty != Type::List(Box::new(source_ty.clone())) {
        return Err(diag(
            expr.span,
            "chunked result type mismatch reached code generation",
        ));
    }
    let source_name = format!("flux__chunked_source_{}", *temp_counter);
    *temp_counter += 1;
    let size_name = format!("flux__chunked_size_{}", *temp_counter);
    *temp_counter += 1;
    let size_unsigned_name = format!("flux__chunked_size_u_{}", *temp_counter);
    *temp_counter += 1;
    let count_name = format!("flux__chunked_count_{}", *temp_counter);
    *temp_counter += 1;
    let buffer_name = format!("flux__chunked_buffer_{}", *temp_counter);
    *temp_counter += 1;
    let index_name = format!("flux__chunked_index_{}", *temp_counter);
    *temp_counter += 1;
    let start_name = format!("flux__chunked_start_{}", *temp_counter);
    *temp_counter += 1;
    let remaining_name = format!("flux__chunked_remaining_{}", *temp_counter);
    *temp_counter += 1;
    let len_name = format!("flux__chunked_len_{}", *temp_counter);
    *temp_counter += 1;
    let result_name = format!("flux__chunked_result_{}", *temp_counter);
    *temp_counter += 1;
    out.push_str(&format!(
        "{pad}struct flux__list {source_name} = {};\n{pad}int64_t {size_name} = {};\n",
        source.code, size.code
    ));
    out.push_str(&format!(
        "{pad}if ({size_name} <= 0) {{ fputs(\"Flux runtime error: chunked size must be greater than zero\\n\", stderr); abort(); }}\n{pad}size_t {size_unsigned_name} = (size_t){size_name};\n"
    ));
    out.push_str(&format!(
        "{pad}size_t {count_name} = ({source_name}.len / {size_unsigned_name}) + (({source_name}.len % {size_unsigned_name}) != 0);\n{pad}struct flux__list {buffer_name}[{count_name} > 0 ? {count_name} : 1];\n"
    ));
    out.push_str(&format!(
        "{pad}for (size_t {index_name} = 0; {index_name} < {count_name}; ++{index_name}) {{\n{pad}    size_t {start_name} = {index_name} * {size_unsigned_name};\n{pad}    size_t {remaining_name} = {source_name}.len - {start_name};\n{pad}    size_t {len_name} = {remaining_name} < {size_unsigned_name} ? {remaining_name} : {size_unsigned_name};\n{pad}    {buffer_name}[{index_name}] = (struct flux__list){{ .data = (void *)((unsigned char *){source_name}.data + (ptrdiff_t){start_name} * {source_name}.stride), .len = {len_name}, .stride = {source_name}.stride }};\n{pad}}}\n"
    ));
    out.push_str(&format!(
        "{pad}struct flux__list {result_name} = (struct flux__list){{ .data = (void *){buffer_name}, .len = {count_name}, .stride = sizeof(struct flux__list) }};\n"
    ));
    Ok(EmittedExpr {
        code: result_name,
        ty: result_ty,
    })
}

fn sorted_less(left: &str, right: &str, ty: &Type) -> Option<String> {
    match ty {
        Type::I64 | Type::Bool => Some(format!("({left} < {right})")),
        Type::Str => Some(format!("(strcmp({left}, {right}) < 0)")),
        _ => None,
    }
}

fn emit_sequence_sorted_binding(
    out: &mut String,
    pad: &str,
    target: (&str, &Type),
    expr: &Expr,
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<(), Diagnostic> {
    let (name, declared_ty) = target;
    let value = emit_sequence_sorted_value(out, pad, expr, env, signatures, temp_counter)?;
    let result_ty = signatures.canonical_type(declared_ty);
    if value.ty != result_ty {
        return Err(diag(
            expr.span,
            "sorted binding type mismatch reached code generation",
        ));
    }
    out.push_str(&format!(
        "{pad}{} {} = {};\n",
        c_type(declared_ty, signatures),
        local_c_name(name),
        value.code
    ));
    env.insert(name.to_string(), result_ty);
    Ok(())
}

fn emit_sequence_sorted_value(
    out: &mut String,
    pad: &str,
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<EmittedExpr, Diagnostic> {
    let source_expr = sequence_sorted(expr)
        .ok_or_else(|| diag(expr.span, "invalid sorted call reached code generation"))?;
    let source = emit_sequence_list_value(out, pad, source_expr, env, signatures, temp_counter)?;
    let result_ty = signatures.canonical_type(&type_of_expr(expr, env, signatures)?);
    let Type::List(element) = &result_ty else {
        return Err(diag(expr.span, "sorted result must have a list type"));
    };
    if signatures.canonical_type(&source.ty) != result_ty {
        return Err(diag(
            expr.span,
            "sorted input type mismatch reached code generation",
        ));
    }
    let element_ty = signatures.canonical_type(element);
    let source_name = format!("flux__sorted_source_{}", *temp_counter);
    *temp_counter += 1;
    let buffer_name = format!("flux__sorted_buffer_{}", *temp_counter);
    *temp_counter += 1;
    let index_name = format!("flux__sorted_index_{}", *temp_counter);
    *temp_counter += 1;
    let key_name = format!("flux__sorted_key_{}", *temp_counter);
    *temp_counter += 1;
    let cursor_name = format!("flux__sorted_cursor_{}", *temp_counter);
    *temp_counter += 1;
    let result_name = format!("flux__sorted_result_{}", *temp_counter);
    *temp_counter += 1;
    let element_c = c_type(element, signatures);
    let less = sorted_less(
        &key_name,
        &format!("{buffer_name}[{cursor_name} - 1]"),
        &element_ty,
    )
    .ok_or_else(|| diag(expr.span, "sorted requires ordered scalar elements"))?;
    out.push_str(&format!(
        "{pad}struct flux__list {source_name} = {};\n",
        source.code
    ));
    out.push_str(&format!(
        "{pad}{element_c} {buffer_name}[{source_name}.len > 0 ? {source_name}.len : 1];\n"
    ));
    out.push_str(&format!(
        "{pad}for (size_t {index_name} = 0; {index_name} < {source_name}.len; ++{index_name}) {{ {buffer_name}[{index_name}] = *(({element_c} *)flux_list_at_unchecked({source_name}, {index_name}, sizeof({element_c}))); }}\n"
    ));
    out.push_str(&format!(
        "{pad}for (size_t {index_name} = 1; {index_name} < {source_name}.len; ++{index_name}) {{\n{pad}    {element_c} {key_name} = {buffer_name}[{index_name}];\n{pad}    size_t {cursor_name} = {index_name};\n{pad}    while ({cursor_name} > 0 && {less}) {{ {buffer_name}[{cursor_name}] = {buffer_name}[{cursor_name} - 1]; --{cursor_name}; }}\n{pad}    {buffer_name}[{cursor_name}] = {key_name};\n{pad}}}\n"
    ));
    out.push_str(&format!(
        "{pad}struct flux__list {result_name} = (struct flux__list){{ .data = (void *){buffer_name}, .len = {source_name}.len, .stride = sizeof({element_c}) }};\n"
    ));
    Ok(EmittedExpr {
        code: result_name,
        ty: result_ty,
    })
}

fn emit_sequence_flatten_binding(
    out: &mut String,
    pad: &str,
    target: (&str, &Type),
    expr: &Expr,
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<(), Diagnostic> {
    let (name, declared_ty) = target;
    let value = emit_sequence_flatten_value(out, pad, expr, env, signatures, temp_counter)?;
    let result_ty = signatures.canonical_type(declared_ty);
    if value.ty != result_ty {
        return Err(diag(
            expr.span,
            "flatten binding type mismatch reached code generation",
        ));
    }
    out.push_str(&format!(
        "{pad}{} {} = {};\n",
        c_type(declared_ty, signatures),
        local_c_name(name),
        value.code
    ));
    env.insert(name.to_string(), result_ty);
    Ok(())
}

fn emit_sequence_flatten_value(
    out: &mut String,
    pad: &str,
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<EmittedExpr, Diagnostic> {
    let source_expr = sequence_flatten(expr)
        .ok_or_else(|| diag(expr.span, "invalid flatten call reached code generation"))?;
    let source = emit_sequence_list_value(out, pad, source_expr, env, signatures, temp_counter)?;
    let source_ty = signatures.canonical_type(&source.ty);
    let Type::List(outer_element) = source_ty else {
        return Err(diag(expr.span, "flatten source must be a nested list"));
    };
    let outer_element = signatures.canonical_type(&outer_element);
    let Type::List(inner_element) = outer_element else {
        return Err(diag(expr.span, "flatten source elements must be lists"));
    };
    let result_ty = signatures.canonical_type(&type_of_expr(expr, env, signatures)?);
    if result_ty != Type::List(inner_element.clone()) {
        return Err(diag(
            expr.span,
            "flatten result type mismatch reached code generation",
        ));
    }
    let source_name = format!("flux__flatten_source_{}", *temp_counter);
    *temp_counter += 1;
    let capacity_name = format!("flux__flatten_capacity_{}", *temp_counter);
    *temp_counter += 1;
    let outer_index = format!("flux__flatten_outer_{}", *temp_counter);
    *temp_counter += 1;
    let inner_name = format!("flux__flatten_inner_{}", *temp_counter);
    *temp_counter += 1;
    let buffer_name = format!("flux__flatten_buffer_{}", *temp_counter);
    *temp_counter += 1;
    let count_name = format!("flux__flatten_count_{}", *temp_counter);
    *temp_counter += 1;
    let inner_index = format!("flux__flatten_index_{}", *temp_counter);
    *temp_counter += 1;
    let result_name = format!("flux__flatten_result_{}", *temp_counter);
    *temp_counter += 1;
    let element_c = c_type(&inner_element, signatures);
    out.push_str(&format!(
        "{pad}struct flux__list {source_name} = {};\n{pad}size_t {capacity_name} = 0;\n",
        source.code
    ));
    out.push_str(&format!(
        "{pad}for (size_t {outer_index} = 0; {outer_index} < {source_name}.len; ++{outer_index}) {{ struct flux__list {inner_name} = *((struct flux__list *)flux_list_at_unchecked({source_name}, {outer_index}, sizeof(struct flux__list))); if (SIZE_MAX - {capacity_name} < {inner_name}.len) {{ fputs(\"Flux runtime error: flattened list is too large\\n\", stderr); abort(); }} {capacity_name} += {inner_name}.len; }}\n"
    ));
    out.push_str(&format!(
        "{pad}{element_c} {buffer_name}[{capacity_name} > 0 ? {capacity_name} : 1];\n{pad}size_t {count_name} = 0;\n"
    ));
    out.push_str(&format!(
        "{pad}for (size_t {outer_index} = 0; {outer_index} < {source_name}.len; ++{outer_index}) {{\n{pad}    struct flux__list {inner_name} = *((struct flux__list *)flux_list_at_unchecked({source_name}, {outer_index}, sizeof(struct flux__list)));\n{pad}    for (size_t {inner_index} = 0; {inner_index} < {inner_name}.len; ++{inner_index}) {{ {buffer_name}[{count_name}++] = *(({element_c} *)flux_list_at_unchecked({inner_name}, {inner_index}, sizeof({element_c}))); }}\n{pad}}}\n"
    ));
    out.push_str(&format!(
        "{pad}struct flux__list {result_name} = (struct flux__list){{ .data = (void *){buffer_name}, .len = {count_name}, .stride = sizeof({element_c}) }};\n"
    ));
    Ok(EmittedExpr {
        code: result_name,
        ty: result_ty,
    })
}

fn distinct_equality(left: &str, right: &str, ty: &Type) -> Option<String> {
    match ty {
        Type::I64 | Type::Bool => Some(format!("({left} == {right})")),
        Type::Str => Some(format!("(strcmp({left}, {right}) == 0)")),
        Type::Error => Some(format!("flux_error_eq({left}, {right})")),
        _ => None,
    }
}

fn emit_sequence_distinct_binding(
    out: &mut String,
    pad: &str,
    target: (&str, &Type),
    expr: &Expr,
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<(), Diagnostic> {
    let (name, declared_ty) = target;
    let value = emit_sequence_distinct_value(out, pad, expr, env, signatures, temp_counter)?;
    let result_ty = signatures.canonical_type(declared_ty);
    if value.ty != result_ty {
        return Err(diag(
            expr.span,
            "distinct binding type mismatch reached code generation",
        ));
    }
    out.push_str(&format!(
        "{pad}{} {} = {};\n",
        c_type(declared_ty, signatures),
        local_c_name(name),
        value.code
    ));
    env.insert(name.to_string(), result_ty);
    Ok(())
}

fn emit_sequence_distinct_value(
    out: &mut String,
    pad: &str,
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<EmittedExpr, Diagnostic> {
    let source_expr = sequence_distinct(expr)
        .ok_or_else(|| diag(expr.span, "invalid distinct call reached code generation"))?;
    let source = emit_sequence_list_value(out, pad, source_expr, env, signatures, temp_counter)?;
    let result_ty = signatures.canonical_type(&type_of_expr(expr, env, signatures)?);
    let Type::List(element) = &result_ty else {
        return Err(diag(expr.span, "distinct result must have a list type"));
    };
    if signatures.canonical_type(&source.ty) != result_ty {
        return Err(diag(
            expr.span,
            "distinct input type mismatch reached code generation",
        ));
    }
    let element_ty = signatures.canonical_type(element);
    let source_name = format!("flux__distinct_source_{}", *temp_counter);
    *temp_counter += 1;
    let buffer_name = format!("flux__distinct_buffer_{}", *temp_counter);
    *temp_counter += 1;
    let count_name = format!("flux__distinct_count_{}", *temp_counter);
    *temp_counter += 1;
    let index_name = format!("flux__distinct_index_{}", *temp_counter);
    *temp_counter += 1;
    let seen_name = format!("flux__distinct_seen_{}", *temp_counter);
    *temp_counter += 1;
    let item_name = format!("flux__distinct_item_{}", *temp_counter);
    *temp_counter += 1;
    let duplicate_name = format!("flux__distinct_duplicate_{}", *temp_counter);
    *temp_counter += 1;
    let result_name = format!("flux__distinct_result_{}", *temp_counter);
    *temp_counter += 1;
    let element_c = c_type(element, signatures);
    let equality = distinct_equality(
        &format!("{buffer_name}[{seen_name}]"),
        &item_name,
        &element_ty,
    )
    .ok_or_else(|| diag(expr.span, "distinct requires scalar equality"))?;
    out.push_str(&format!(
        "{pad}struct flux__list {source_name} = {};\n",
        source.code
    ));
    out.push_str(&format!(
        "{pad}{element_c} {buffer_name}[{source_name}.len > 0 ? {source_name}.len : 1];\n{pad}size_t {count_name} = 0;\n"
    ));
    out.push_str(&format!(
        "{pad}for (size_t {index_name} = 0; {index_name} < {source_name}.len; ++{index_name}) {{\n"
    ));
    out.push_str(&format!(
        "{pad}    {element_c} {item_name} = *(({element_c} *)flux_list_at_unchecked({source_name}, {index_name}, sizeof({element_c})));\n{pad}    bool {duplicate_name} = false;\n"
    ));
    out.push_str(&format!(
        "{pad}    for (size_t {seen_name} = 0; {seen_name} < {count_name}; ++{seen_name}) {{ if ({equality}) {{ {duplicate_name} = true; break; }} }}\n"
    ));
    out.push_str(&format!(
        "{pad}    if (!{duplicate_name}) {buffer_name}[{count_name}++] = {item_name};\n{pad}}}\n"
    ));
    out.push_str(&format!(
        "{pad}struct flux__list {result_name} = (struct flux__list){{ .data = (void *){buffer_name}, .len = {count_name}, .stride = sizeof({element_c}) }};\n"
    ));
    Ok(EmittedExpr {
        code: result_name,
        ty: result_ty,
    })
}

fn emit_sequence_concat_binding(
    out: &mut String,
    pad: &str,
    target: (&str, &Type),
    expr: &Expr,
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<(), Diagnostic> {
    let (name, declared_ty) = target;
    let value = emit_sequence_concat_value(out, pad, expr, env, signatures, temp_counter)?;
    let result_ty = signatures.canonical_type(declared_ty);
    if value.ty != result_ty {
        return Err(diag(
            expr.span,
            "concat binding type mismatch reached code generation",
        ));
    }
    out.push_str(&format!(
        "{pad}{} {} = {};\n",
        c_type(declared_ty, signatures),
        local_c_name(name),
        value.code
    ));
    env.insert(name.to_string(), result_ty);
    Ok(())
}

fn emit_sequence_concat_value(
    out: &mut String,
    pad: &str,
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<EmittedExpr, Diagnostic> {
    let (left_expr, right_expr) = sequence_concat(expr)
        .ok_or_else(|| diag(expr.span, "invalid concat reached code generation"))?;
    let left = emit_sequence_list_value(out, pad, left_expr, env, signatures, temp_counter)?;
    let right = emit_sequence_list_value(out, pad, right_expr, env, signatures, temp_counter)?;
    let result_ty = signatures.canonical_type(&type_of_expr(expr, env, signatures)?);
    let Type::List(element) = &result_ty else {
        return Err(diag(expr.span, "concat result must have a list type"));
    };
    if signatures.canonical_type(&left.ty) != result_ty
        || signatures.canonical_type(&right.ty) != result_ty
    {
        return Err(diag(
            expr.span,
            "concat input type mismatch reached code generation",
        ));
    }
    let left_name = format!("flux__concat_left_{}", *temp_counter);
    *temp_counter += 1;
    let right_name = format!("flux__concat_right_{}", *temp_counter);
    *temp_counter += 1;
    let capacity_name = format!("flux__concat_capacity_{}", *temp_counter);
    *temp_counter += 1;
    let buffer_name = format!("flux__concat_buffer_{}", *temp_counter);
    *temp_counter += 1;
    let index_name = format!("flux__concat_index_{}", *temp_counter);
    *temp_counter += 1;
    let result_name = format!("flux__concat_result_{}", *temp_counter);
    *temp_counter += 1;
    let element_c = c_type(element, signatures);
    out.push_str(&format!(
        "{pad}struct flux__list {left_name} = {};\n{pad}struct flux__list {right_name} = {};\n",
        left.code, right.code
    ));
    out.push_str(&format!(
        "{pad}if (SIZE_MAX - {left_name}.len < {right_name}.len) {{ fputs(\"Flux runtime error: concatenated list is too large\\n\", stderr); abort(); }}\n"
    ));
    out.push_str(&format!(
        "{pad}size_t {capacity_name} = {left_name}.len + {right_name}.len;\n"
    ));
    out.push_str(&format!(
        "{pad}{element_c} {buffer_name}[{capacity_name} > 0 ? {capacity_name} : 1];\n"
    ));
    out.push_str(&format!(
        "{pad}for (size_t {index_name} = 0; {index_name} < {left_name}.len; ++{index_name}) {{ {buffer_name}[{index_name}] = *(({element_c} *)flux_list_at_unchecked({left_name}, {index_name}, sizeof({element_c}))); }}\n"
    ));
    out.push_str(&format!(
        "{pad}for (size_t {index_name} = 0; {index_name} < {right_name}.len; ++{index_name}) {{ {buffer_name}[{left_name}.len + {index_name}] = *(({element_c} *)flux_list_at_unchecked({right_name}, {index_name}, sizeof({element_c}))); }}\n"
    ));
    out.push_str(&format!(
        "{pad}struct flux__list {result_name} = (struct flux__list){{ .data = (void *){buffer_name}, .len = {capacity_name}, .stride = sizeof({element_c}) }};\n"
    ));
    Ok(EmittedExpr {
        code: result_name,
        ty: result_ty,
    })
}

fn emit_sequence_transform_binding(
    out: &mut String,
    pad: &str,
    target: (&str, &Type),
    expr: &Expr,
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<(), Diagnostic> {
    let (name, declared_ty) = target;
    let value = emit_sequence_transform_value(out, pad, expr, env, signatures, temp_counter)?;
    let result_ty = signatures.canonical_type(declared_ty);
    if value.ty != result_ty {
        return Err(diag(
            expr.span,
            "sequence transform binding type mismatch reached code generation",
        ));
    }
    out.push_str(&format!(
        "{pad}{} {} = {};\n",
        c_type(declared_ty, signatures),
        local_c_name(name),
        value.code
    ));
    env.insert(name.to_string(), result_ty);
    Ok(())
}

fn collect_sequence_transform_chain<'a>(expr: &'a Expr, stages: &mut Vec<&'a Expr>) -> &'a Expr {
    let Some(transform) = sequence_transform(expr) else {
        return expr;
    };
    let list = match transform {
        SequenceTransform::Map { list, .. } | SequenceTransform::Filter { list, .. } => list,
    };
    let source = collect_sequence_transform_chain(list, stages);
    stages.push(expr);
    source
}

fn emit_sequence_transform_stages(
    out: &mut String,
    pad: &str,
    stages: &[&Expr],
    mut value_name: String,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<(String, Type), Diagnostic> {
    let first_stage = stages
        .first()
        .expect("sequence transform stages are non-empty");
    let first_transform = sequence_transform(first_stage).ok_or_else(|| {
        diag(
            first_stage.span,
            "invalid sequence transform stage reached code generation",
        )
    })?;
    let first_list = match first_transform {
        SequenceTransform::Map { list, .. } | SequenceTransform::Filter { list, .. } => list,
    };
    let first_list_ty = signatures.canonical_type(&type_of_expr(first_list, env, signatures)?);
    let Type::List(first_element) = first_list_ty else {
        return Err(diag(
            first_stage.span,
            "sequence transform stage requires a list source",
        ));
    };
    let mut value_ty = (*first_element).clone();
    for stage in stages {
        let transform = sequence_transform(stage).ok_or_else(|| {
            diag(
                stage.span,
                "invalid sequence transform stage reached code generation",
            )
        })?;
        let (callback_expr, filter) = match transform {
            SequenceTransform::Map { callback, .. } => (callback, false),
            SequenceTransform::Filter { callback, .. } => (callback, true),
        };
        let callback = emit_expr(callback_expr, env, signatures)?;
        let Type::Function { .. } = callback.ty else {
            return Err(diag(
                stage.span,
                "sequence transform requires a function callback",
            ));
        };
        if filter {
            out.push_str(&format!(
                "{pad}if (!{}({value_name})) continue;\n",
                callback.code
            ));
            continue;
        }
        let stage_ty = signatures.canonical_type(&type_of_expr(stage, env, signatures)?);
        let Type::List(output_element) = stage_ty else {
            return Err(diag(
                stage.span,
                "sequence transform stage must produce a list",
            ));
        };
        let next_name = format!("flux__transform_value_{}", *temp_counter);
        *temp_counter += 1;
        let output_ty = (*output_element).clone();
        out.push_str(&format!(
            "{pad}{} {next_name} = {}({value_name});\n",
            c_type(&output_ty, signatures),
            callback.code
        ));
        value_name = next_name;
        value_ty = output_ty;
    }
    Ok((value_name, value_ty))
}

fn emit_sequence_transform_value(
    out: &mut String,
    pad: &str,
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<EmittedExpr, Diagnostic> {
    let mut stages = Vec::new();
    let source_expr = collect_sequence_transform_chain(expr, &mut stages);
    if stages.is_empty() {
        return Err(diag(
            expr.span,
            "invalid sequence transform reached code generation",
        ));
    }
    let source = emit_sequence_list_value(out, pad, source_expr, env, signatures, temp_counter)?;
    let Type::List(input_element) = signatures.canonical_type(&source.ty) else {
        return Err(diag(expr.span, "sequence transform requires a list source"));
    };
    let result_ty = signatures.canonical_type(&type_of_expr(expr, env, signatures)?);
    let Type::List(output_element) = &result_ty else {
        return Err(diag(
            expr.span,
            "sequence transform result must have a list type",
        ));
    };
    let source_name = format!("flux__transform_source_{}", *temp_counter);
    *temp_counter += 1;
    let buffer_name = format!("flux__transform_buffer_{}", *temp_counter);
    *temp_counter += 1;
    let count_name = format!("flux__transform_count_{}", *temp_counter);
    *temp_counter += 1;
    let index_name = format!("flux__transform_index_{}", *temp_counter);
    *temp_counter += 1;
    let item_name = format!("flux__transform_item_{}", *temp_counter);
    *temp_counter += 1;
    let result_name = format!("flux__transform_result_{}", *temp_counter);
    *temp_counter += 1;
    let input_c = c_type(&input_element, signatures);
    let output_c = c_type(output_element, signatures);
    out.push_str(&format!(
        "{pad}struct flux__list {source_name} = {};\n",
        source.code
    ));
    out.push_str(&format!(
        "{pad}{output_c} {buffer_name}[{source_name}.len > 0 ? {source_name}.len : 1];\n"
    ));
    out.push_str(&format!("{pad}size_t {count_name} = 0;\n"));
    out.push_str(&format!(
        "{pad}for (size_t {index_name} = 0; {index_name} < {source_name}.len; ++{index_name}) {{\n"
    ));
    out.push_str(&format!(
        "{pad}    {input_c} {item_name} = *(({input_c} *)flux_list_at_unchecked({source_name}, {index_name}, sizeof({input_c})));\n"
    ));
    let body_pad = format!("{pad}    ");
    let (value_name, value_ty) = emit_sequence_transform_stages(
        out,
        &body_pad,
        &stages,
        item_name,
        env,
        signatures,
        temp_counter,
    )?;
    if value_ty != **output_element {
        return Err(diag(
            expr.span,
            "sequence transform fused value type mismatch reached code generation",
        ));
    }
    out.push_str(&format!(
        "{pad}    {buffer_name}[{count_name}++] = {value_name};\n"
    ));
    out.push_str(&format!("{pad}}}\n"));
    out.push_str(&format!(
        "{pad}struct flux__list {result_name} = (struct flux__list){{ .data = (void *){buffer_name}, .len = {count_name}, .stride = sizeof({output_c}) }};\n"
    ));
    Ok(EmittedExpr {
        code: result_name,
        ty: result_ty,
    })
}

enum SequenceReduction<'a> {
    Fold {
        list: &'a Expr,
        initial: &'a Expr,
        reducer: &'a Expr,
    },
    Reduce {
        list: &'a Expr,
        reducer: &'a Expr,
    },
}

fn sequence_reduction(expr: &Expr) -> Option<SequenceReduction<'_>> {
    match &expr.kind {
        ExprKind::Call {
            name,
            args,
            named_args,
        } if named_args.is_empty() && name == "fold" && args.len() == 3 => {
            Some(SequenceReduction::Fold {
                list: &args[0],
                initial: &args[1],
                reducer: &args[2],
            })
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if named_args.is_empty() && name == "reduce" && args.len() == 2 => {
            Some(SequenceReduction::Reduce {
                list: &args[0],
                reducer: &args[1],
            })
        }
        ExprKind::Pipe {
            input, name, args, ..
        } if name == "fold" && args.len() == 2 => Some(SequenceReduction::Fold {
            list: input,
            initial: &args[0],
            reducer: &args[1],
        }),
        ExprKind::Pipe {
            input, name, args, ..
        } if name == "reduce" && args.len() == 1 => Some(SequenceReduction::Reduce {
            list: input,
            reducer: &args[0],
        }),
        _ => None,
    }
}

fn emit_sequence_reduction_binding(
    out: &mut String,
    pad: &str,
    target: (&str, &Type),
    expr: &Expr,
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<(), Diagnostic> {
    let (name, declared_ty) = target;
    let reduction = sequence_reduction(expr).ok_or_else(|| {
        diag(
            expr.span,
            "invalid sequence reduction reached code generation",
        )
    })?;
    let (list_expr, initial, reducer_expr, reduce) = match reduction {
        SequenceReduction::Fold {
            list,
            initial,
            reducer,
        } => (list, Some(initial), reducer, false),
        SequenceReduction::Reduce { list, reducer } => (list, None, reducer, true),
    };
    let mut stages = Vec::new();
    let source_expr = collect_sequence_transform_chain(list_expr, &mut stages);
    let source = emit_sequence_list_value(out, pad, source_expr, env, signatures, temp_counter)?;
    let Type::List(source_element) = signatures.canonical_type(&source.ty) else {
        return Err(diag(expr.span, "sequence reduction requires a list source"));
    };
    let list_ty = signatures.canonical_type(&type_of_expr(list_expr, env, signatures)?);
    let Type::List(element) = list_ty else {
        return Err(diag(
            expr.span,
            "sequence reduction input must remain a list",
        ));
    };
    let reducer = emit_expr(reducer_expr, env, signatures)?;
    let Type::Function { .. } = reducer.ty else {
        return Err(diag(
            expr.span,
            "sequence reduction requires a function reducer",
        ));
    };
    let source_name = format!("flux__reduce_source_{}", *temp_counter);
    *temp_counter += 1;
    let index_name = format!("flux__reduce_index_{}", *temp_counter);
    *temp_counter += 1;
    let item_name = format!("flux__reduce_item_{}", *temp_counter);
    *temp_counter += 1;
    let target_name = local_c_name(name);
    let source_element_c = c_type(&source_element, signatures);
    out.push_str(&format!(
        "{pad}struct flux__list {source_name} = {};\n",
        source.code
    ));
    let has_value_name = if reduce {
        let has_value_name = format!("flux__reduce_has_value_{}", *temp_counter);
        *temp_counter += 1;
        out.push_str(&format!(
            "{pad}{} {target_name};\n{pad}bool {has_value_name} = false;\n",
            c_type(declared_ty, signatures)
        ));
        Some(has_value_name)
    } else {
        let initial = emit_expr(
            initial.expect("fold reduction has an initial value"),
            env,
            signatures,
        )?;
        out.push_str(&format!(
            "{pad}{} {target_name} = {};\n",
            c_type(declared_ty, signatures),
            initial.code
        ));
        None
    };
    out.push_str(&format!(
        "{pad}for (size_t {index_name} = 0; {index_name} < {source_name}.len; ++{index_name}) {{\n"
    ));
    out.push_str(&format!(
        "{pad}    {source_element_c} {item_name} = *(({source_element_c} *)flux_list_at_unchecked({source_name}, {index_name}, sizeof({source_element_c})));\n"
    ));
    let body_pad = format!("{pad}    ");
    let (value_name, value_ty) = if stages.is_empty() {
        (item_name, (*source_element).clone())
    } else {
        emit_sequence_transform_stages(
            out,
            &body_pad,
            &stages,
            item_name,
            env,
            signatures,
            temp_counter,
        )?
    };
    if value_ty != *element {
        return Err(diag(
            expr.span,
            "sequence reduction fused value type mismatch reached code generation",
        ));
    }
    if let Some(has_value_name) = &has_value_name {
        out.push_str(&format!(
            "{pad}    if (!{has_value_name}) {{ {target_name} = {value_name}; {has_value_name} = true; }} else {{ {target_name} = {}({target_name}, {value_name}); }}\n",
            reducer.code
        ));
    } else {
        out.push_str(&format!(
            "{pad}    {target_name} = {}({target_name}, {value_name});\n",
            reducer.code
        ));
    }
    out.push_str(&format!("{pad}}}\n"));
    if let Some(has_value_name) = has_value_name {
        out.push_str(&format!(
            "{pad}if (!{has_value_name}) {{ fputs(\"Flux runtime error: reduce requires a non-empty list\\n\", stderr); abort(); }}\n"
        ));
    }
    env.insert(name.to_string(), signatures.canonical_type(declared_ty));
    Ok(())
}

fn list_literal_needs_builder(expr: &Expr) -> bool {
    matches!(
        &expr.kind,
        ExprKind::List(items)
            if items.iter().any(|item| {
                matches!(item.kind, ExprKind::ListSpread { .. } | ExprKind::ListIf { .. })
            })
    )
}

enum BufferedListItem {
    Scalar(String),
    Spread(String),
    Conditional {
        condition: String,
        value: String,
        has_else: bool,
    },
}

fn emit_list_builder_binding(
    out: &mut String,
    pad: &str,
    target: (&str, &Type),
    expr: &Expr,
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<(), Diagnostic> {
    let (name, declared_ty) = target;
    let ExprKind::List(items) = &expr.kind else {
        unreachable!()
    };
    let result_ty = signatures.canonical_type(declared_ty);
    let Type::List(element) = &result_ty else {
        return Err(diag(
            expr.span,
            "constructed list binding requires a list type",
        ));
    };
    let element_c = c_type(element, signatures);
    let capacity_name = format!("flux__list_build_capacity_{}", *temp_counter);
    *temp_counter += 1;
    let buffer_name = format!("flux__list_build_buffer_{}", *temp_counter);
    *temp_counter += 1;
    let count_name = format!("flux__list_build_count_{}", *temp_counter);
    *temp_counter += 1;
    let index_name = format!("flux__list_build_index_{}", *temp_counter);
    *temp_counter += 1;
    let mut buffered_items = Vec::with_capacity(items.len());

    out.push_str(&format!("{pad}size_t {capacity_name} = 0;\n"));
    for item in items {
        match &item.kind {
            ExprKind::ListSpread { value, .. } => {
                let spread = emit_expr(value, env, signatures)?;
                let source_name = format!("flux__list_build_source_{}", *temp_counter);
                *temp_counter += 1;
                out.push_str(&format!(
                    "{pad}struct flux__list {source_name} = {};\n",
                    spread.code
                ));
                out.push_str(&format!(
                    "{pad}if (SIZE_MAX - {capacity_name} < {source_name}.len) {{ fputs(\"Flux runtime error: constructed list is too large\\n\", stderr); abort(); }}\n{pad}{capacity_name} += {source_name}.len;\n"
                ));
                buffered_items.push(BufferedListItem::Spread(source_name));
            }
            ExprKind::ListIf {
                condition,
                value,
                else_value,
                ..
            } => {
                if let Some(ConstantValue::Bool(condition)) =
                    fold_primitive_expr(condition, env, signatures)?
                {
                    let selected = if condition {
                        Some(value.as_ref())
                    } else {
                        else_value.as_deref()
                    };
                    if let Some(selected) = selected {
                        let selected = emit_expr(selected, env, signatures)?;
                        let value_name = format!("flux__list_build_value_{}", *temp_counter);
                        *temp_counter += 1;
                        out.push_str(&format!(
                            "{pad}{element_c} {value_name} = {};\n{pad}if ({capacity_name} == SIZE_MAX) {{ fputs(\"Flux runtime error: constructed list is too large\\n\", stderr); abort(); }}\n{pad}++{capacity_name};\n",
                            selected.code
                        ));
                        buffered_items.push(BufferedListItem::Scalar(value_name));
                    }
                    continue;
                }
                let condition = emit_expr(condition, env, signatures)?;
                let condition_name = format!("flux__list_build_condition_{}", *temp_counter);
                *temp_counter += 1;
                let value_name = format!("flux__list_build_value_{}", *temp_counter);
                *temp_counter += 1;
                let then_value = emit_expr(value, env, signatures)?;
                out.push_str(&format!(
                    "{pad}bool {condition_name} = {};\n{pad}{element_c} {value_name};\n{pad}if ({condition_name}) {{\n{pad}    {value_name} = {};\n",
                    condition.code, then_value.code
                ));
                if else_value.is_none() {
                    out.push_str(&format!(
                        "{pad}    if ({capacity_name} == SIZE_MAX) {{ fputs(\"Flux runtime error: constructed list is too large\\n\", stderr); abort(); }}\n{pad}    ++{capacity_name};\n"
                    ));
                }
                if let Some(else_value) = else_value {
                    let else_value = emit_expr(else_value, env, signatures)?;
                    out.push_str(&format!(
                        "{pad}}} else {{\n{pad}    {value_name} = {};\n{pad}}}\n{pad}if ({capacity_name} == SIZE_MAX) {{ fputs(\"Flux runtime error: constructed list is too large\\n\", stderr); abort(); }}\n{pad}++{capacity_name};\n",
                        else_value.code
                    ));
                } else {
                    out.push_str(&format!("{pad}}}\n"));
                }
                buffered_items.push(BufferedListItem::Conditional {
                    condition: condition_name,
                    value: value_name,
                    has_else: else_value.is_some(),
                });
            }
            _ => {
                let value = emit_expr(item, env, signatures)?;
                let value_name = format!("flux__list_build_value_{}", *temp_counter);
                *temp_counter += 1;
                out.push_str(&format!(
                    "{pad}{element_c} {value_name} = {};\n{pad}if ({capacity_name} == SIZE_MAX) {{ fputs(\"Flux runtime error: constructed list is too large\\n\", stderr); abort(); }}\n{pad}++{capacity_name};\n",
                    value.code
                ));
                buffered_items.push(BufferedListItem::Scalar(value_name));
            }
        }
    }

    out.push_str(&format!(
        "{pad}{element_c} {buffer_name}[{capacity_name} > 0 ? {capacity_name} : 1];\n{pad}size_t {count_name} = 0;\n"
    ));
    for item in buffered_items {
        match item {
            BufferedListItem::Scalar(value_name) => {
                out.push_str(&format!(
                    "{pad}{buffer_name}[{count_name}++] = {value_name};\n"
                ));
            }
            BufferedListItem::Spread(source_name) => {
                out.push_str(&format!(
                    "{pad}for (size_t {index_name} = 0; {index_name} < {source_name}.len; ++{index_name}) {{ {buffer_name}[{count_name}++] = *(({element_c} *)flux_list_at_unchecked({source_name}, {index_name}, sizeof({element_c}))); }}\n"
                ));
            }
            BufferedListItem::Conditional {
                condition,
                value,
                has_else,
            } => {
                if has_else {
                    out.push_str(&format!("{pad}{buffer_name}[{count_name}++] = {value};\n"));
                } else {
                    out.push_str(&format!(
                        "{pad}if ({condition}) {buffer_name}[{count_name}++] = {value};\n"
                    ));
                }
            }
        }
    }
    out.push_str(&format!(
        "{pad}struct flux__list {} = (struct flux__list){{ .data = (void *){buffer_name}, .len = {count_name}, .stride = sizeof({element_c}) }};\n",
        local_c_name(name)
    ));
    env.insert(name.to_string(), result_ty);
    Ok(())
}

fn emit_list_comprehension_binding(
    out: &mut String,
    pad: &str,
    target: (&str, &Type),
    expr: &Expr,
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<(), Diagnostic> {
    let (name, declared_ty) = target;
    let ExprKind::ListComprehension {
        value,
        binding,
        iterable,
        condition,
        ..
    } = &expr.kind
    else {
        unreachable!()
    };
    let iterable_ty = type_of_expr(iterable, env, signatures)?;
    let Type::List(input_element) = signatures.canonical_type(&iterable_ty) else {
        return Err(diag(expr.span, "list comprehension requires a list source"));
    };
    let result_ty = signatures.canonical_type(declared_ty);
    let Type::List(output_element) = &result_ty else {
        return Err(diag(
            expr.span,
            "list comprehension binding must have a list type",
        ));
    };
    let source = emit_expr(iterable, env, signatures)?;
    let source_name = format!("flux__list_source_{}", *temp_counter);
    *temp_counter += 1;
    let buffer_name = format!("flux__list_buffer_{}", *temp_counter);
    *temp_counter += 1;
    let count_name = format!("flux__list_count_{}", *temp_counter);
    *temp_counter += 1;
    let index_name = format!("flux__list_index_{}", *temp_counter);
    *temp_counter += 1;
    out.push_str(&format!(
        "{pad}struct flux__list {source_name} = {};\n",
        source.code
    ));
    out.push_str(&format!(
        "{pad}{} {buffer_name}[{source_name}.len > 0 ? {source_name}.len : 1];\n",
        c_type(output_element, signatures)
    ));
    out.push_str(&format!("{pad}size_t {count_name} = 0;\n"));
    out.push_str(&format!(
        "{pad}for (size_t {index_name} = 0; {index_name} < {source_name}.len; {index_name}++) {{\n"
    ));
    let mut nested = env.clone();
    let binding_c = local_c_name(binding);
    out.push_str(&format!(
        "{pad}    {} {binding_c} = *(({} *)flux_list_at_unchecked({source_name}, {index_name}, sizeof({})));\n",
        c_type(&input_element, signatures),
        c_type(&input_element, signatures),
        c_type(&input_element, signatures)
    ));
    nested.insert(binding.clone(), (*input_element).clone());
    if let Some(condition) = condition {
        let condition = emit_expr(condition, &nested, signatures)?;
        out.push_str(&format!(
            "{pad}    if (!{}) continue;\n",
            c_condition(&condition.code)
        ));
    }
    let value = emit_expr(value, &nested, signatures)?;
    out.push_str(&format!(
        "{pad}    {buffer_name}[{count_name}++] = {};\n",
        value.code
    ));
    out.push_str(&format!("{pad}}}\n"));
    out.push_str(&format!(
        "{pad}struct flux__list {} = (struct flux__list){{ .data = (void *){buffer_name}, .len = {count_name}, .stride = sizeof({}) }};\n",
        local_c_name(name),
        c_type(output_element, signatures)
    ));
    env.insert(name.to_string(), result_ty);
    Ok(())
}

fn pipe_input_expr(input: &Expr, env: &HashMap<String, Type>, signatures: &Signatures) -> Expr {
    let ExprKind::Var(name) = &input.kind else {
        return input.clone();
    };
    let zero_arg_declared = signatures
        .get(name)
        .is_some_and(|signature| signature.params.is_empty());
    let zero_arg_value = matches!(
        env.get(name),
        Some(Type::Function { params, .. }) if params.is_empty()
    );
    if zero_arg_declared || zero_arg_value {
        Expr {
            line: input.line,
            span: input.span,
            kind: ExprKind::Call {
                name: name.clone(),
                args: Vec::new(),
                named_args: Vec::new(),
            },
        }
    } else {
        input.clone()
    }
}

fn emit_expr(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<EmittedExpr, Diagnostic> {
    if let Some(value) = fold_primitive_expr(expr, env, signatures)? {
        return Ok(EmittedExpr {
            ty: value.ty(),
            code: constant_c_value(&value),
        });
    }
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
        ExprKind::AnonymousFunction { .. } => EmittedExpr {
            code: anonymous_function_c_name(expr.span),
            ty: type_of_expr(expr, env, signatures)?,
        },
        ExprKind::ShellCall { name, args, .. } => {
            let call = Expr {
                line: expr.line,
                span: expr.span,
                kind: ExprKind::Call {
                    name: name.clone(),
                    args: args.clone(),
                    named_args: Vec::new(),
                },
            };
            return emit_expr(&call, env, signatures);
        }
        ExprKind::Pipe {
            input, name, args, ..
        } => {
            let mut call_args = Vec::with_capacity(args.len() + 1);
            call_args.push(pipe_input_expr(input, env, signatures));
            call_args.extend(args.iter().cloned());
            let call = Expr {
                line: expr.line,
                span: expr.span,
                kind: ExprKind::Call {
                    name: name.clone(),
                    args: call_args,
                    named_args: Vec::new(),
                },
            };
            return emit_expr(&call, env, signatures);
        }
        ExprKind::List(items) => {
            if items.iter().any(|item| {
                matches!(
                    item.kind,
                    ExprKind::ListSpread { .. } | ExprKind::ListIf { .. }
                )
            }) {
                return Err(diag(
                    expr.span,
                    "list literals with spread/if control currently lower only when bound directly to an immutable local value",
                ));
            }
            let result_ty = type_of_expr(expr, env, signatures)?;
            let Type::List(element) = &result_ty else {
                unreachable!()
            };
            let mut rendered = Vec::with_capacity(items.len());
            for item in items {
                rendered.push(emit_expr(item, env, signatures)?.code);
            }
            let element_c = c_type(element, signatures);
            EmittedExpr {
                code: format!(
                    "((struct flux__list){{ .data = (void *)({element_c}[]){{ {} }}, .len = {}, .stride = sizeof({element_c}) }})",
                    rendered.join(", "),
                    items.len()
                ),
                ty: result_ty,
            }
        }
        ExprKind::ListSpread { .. } => {
            return Err(diag(
                expr.span,
                "list spread syntax is only valid inside a list literal",
            ));
        }
        ExprKind::ListIf { .. } => {
            return Err(diag(
                expr.span,
                "list if/else syntax is only valid inside a list literal",
            ));
        }
        ExprKind::Index { base, index } => {
            let static_index = resolved_static_list_index(base, index, env, signatures)?;
            let base = emit_expr(base, env, signatures)?;
            let index = emit_expr(index, env, signatures)?;
            let result_ty = type_of_expr(expr, env, signatures)?;
            let element_c = c_type(&result_ty, signatures);
            let code = if let Some(static_index) = static_index {
                format!(
                    "(*(({element_c} *)flux_list_at_unchecked({}, {static_index}, sizeof({element_c}))))",
                    base.code
                )
            } else {
                format!(
                    "(*(({element_c} *)flux_list_at({}, {}, sizeof({element_c}))))",
                    base.code, index.code
                )
            };
            EmittedExpr {
                code,
                ty: result_ty,
            }
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            let base = emit_expr(base, env, signatures)?;
            let Type::List(element) = type_of_expr(expr, env, signatures)? else {
                unreachable!()
            };
            let (has_start, start_code) = if let Some(start) = start {
                ("true", emit_expr(start, env, signatures)?.code)
            } else {
                ("false", "INT64_C(0)".to_string())
            };
            let (has_end, end_code) = if let Some(end) = end {
                ("true", emit_expr(end, env, signatures)?.code)
            } else {
                ("false", "INT64_C(0)".to_string())
            };
            let step_code = if let Some(step) = step {
                emit_expr(step, env, signatures)?.code
            } else {
                "INT64_C(1)".to_string()
            };
            let element_c = c_type(&element, signatures);
            EmittedExpr {
                code: format!(
                    "flux_list_slice({}, {has_start}, {start_code}, {has_end}, {end_code}, {step_code}, sizeof({element_c}))",
                    base.code
                ),
                ty: Type::List(element),
            }
        }
        ExprKind::ListComprehension { .. } => {
            return Err(diag(
                expr.span,
                "list comprehensions currently lower only when bound directly to an immutable local 'let'",
            ));
        }
        ExprKind::Call { name, .. } if name == "chunked" => {
            return Err(diag(
                expr.span,
                "chunked currently lowers only when bound directly to an immutable local value",
            ));
        }
        ExprKind::Call { name, .. } if name == "sorted" => {
            return Err(diag(
                expr.span,
                "sorted currently lowers only when bound directly to an immutable local value",
            ));
        }
        ExprKind::Call { name, .. } if name == "flatten" => {
            return Err(diag(
                expr.span,
                "flatten currently lowers only when bound directly to an immutable local value",
            ));
        }
        ExprKind::Call { name, .. } if name == "distinct" => {
            return Err(diag(
                expr.span,
                "distinct currently lowers only when bound directly to an immutable local value",
            ));
        }
        ExprKind::Call { name, .. } if name == "concat" => {
            return Err(diag(
                expr.span,
                "concat currently lowers only when bound directly to an immutable local value",
            ));
        }
        ExprKind::Call { name, .. } if name == "map" || name == "filter" || name == "where" => {
            return Err(diag(
                expr.span,
                "map/filter/where currently lower only when bound directly to an immutable local value",
            ));
        }
        ExprKind::Call { name, .. } if name == "fold" || name == "reduce" => {
            return Err(diag(
                expr.span,
                "fold/reduce currently lower only when bound directly to a local value",
            ));
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if name == "any" || name == "every" => {
            if !named_args.is_empty() || args.len() != 1 {
                return Err(diag(
                    expr.span,
                    "invalid boolean sequence call reached code generation",
                ));
            }
            let list = emit_expr(&args[0], env, signatures)?;
            if list.ty != Type::List(Box::new(Type::Bool)) {
                return Err(diag(
                    expr.span,
                    "boolean sequence operation requires a bool[] value",
                ));
            }
            let helper = if name == "any" {
                "flux_list_any_bool"
            } else {
                "flux_list_every_bool"
            };
            EmittedExpr {
                code: format!("{helper}({})", list.code),
                ty: Type::Bool,
            }
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } if name == "take" || name == "skip" => {
            if !named_args.is_empty() || args.len() != 2 {
                return Err(diag(
                    expr.span,
                    "invalid list view call reached code generation",
                ));
            }
            let list = emit_expr(&args[0], env, signatures)?;
            let count = emit_expr(&args[1], env, signatures)?;
            let Type::List(element) = &list.ty else {
                return Err(diag(expr.span, "list view call requires a list value"));
            };
            let code = if name == "take" {
                format!("flux_list_take({}, {})", list.code, count.code)
            } else {
                let element_c = c_type(element, signatures);
                format!(
                    "flux_list_skip({}, {}, sizeof({element_c}))",
                    list.code, count.code
                )
            };
            EmittedExpr { code, ty: list.ty }
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
                Type::List(_) => return Err(diag(expr.span, "cannot print a list directly")),
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
        } if signatures.interface(name).is_some() => {
            if !named_args.is_empty() || args.len() != 1 {
                return Err(diag(
                    expr.span,
                    "invalid interface value conversion reached code generation",
                ));
            }
            let value = emit_expr(&args[0], env, signatures)?;
            let Type::Named(target_name) = signatures.canonical_type(&value.ty) else {
                return Err(diag(
                    args[0].span,
                    "interface conversion requires a concrete value",
                ));
            };
            if signatures.implementation(name, &target_name).is_none() {
                return Err(diag(
                    args[0].span,
                    "missing interface implementation during value conversion",
                ));
            }
            EmittedExpr {
                code: format!(
                    "{}({})",
                    interface_pack_helper_name(name, &target_name),
                    value.code
                ),
                ty: Type::Named(name.clone()),
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
        ExprKind::QualifiedCall {
            namespace,
            name,
            args,
            named_args,
            ..
        } => {
            let (code, returns, _) = emit_qualified_call(
                expr.span, namespace, name, args, named_args, env, signatures,
            )?;
            let ty = match returns.as_slice() {
                [] => Type::Void,
                [ty] => ty.clone(),
                _ => {
                    return Err(diag(
                        expr.span,
                        &format!(
                            "qualified call '{namespace}.{name}' returns multiple values and requires destructuring"
                        ),
                    ));
                }
            };
            EmittedExpr { code, ty }
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
        ExprKind::Match { .. } | ExprKind::ListMatch { .. } => {
            return Err(diag(
                expr.span,
                "multiline match expressions are lowered from binding/return statements",
            ));
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            let cond = emit_expr(cond, env, signatures)?;
            let then_expr = emit_expr(then_expr, env, signatures)?;
            let else_expr = emit_expr(else_expr, env, signatures)?;
            EmittedExpr {
                code: format!(
                    "({} ? {} : {})",
                    c_condition(&cond.code),
                    then_expr.code,
                    else_expr.code
                ),
                ty: then_expr.ty,
            }
        }
        ExprKind::Field { base, name, .. } => {
            let static_len = static_list_length(base);
            let base = emit_expr(base, env, signatures)?;
            let result_ty = type_of_expr(expr, env, signatures)?;
            let code = if let Type::List(element) = &base.ty {
                let element_c = c_type(element, signatures);
                match name.as_str() {
                    "length" => format!("({}).len", base.code),
                    "isEmpty" => format!("(({}).len == 0)", base.code),
                    "isNotEmpty" => format!("(({}).len != 0)", base.code),
                    "first" if static_len.is_some_and(|len| len > 0) => format!(
                        "(*(({element_c} *)flux_list_at_unchecked({}, 0, sizeof({element_c}))))",
                        base.code
                    ),
                    "last" if static_len.is_some_and(|len| len > 0) => format!(
                        "(*(({element_c} *)flux_list_at_unchecked({}, {}, sizeof({element_c}))))",
                        base.code,
                        static_len.expect("non-empty static list length") - 1
                    ),
                    "single" if static_len == Some(1) => format!(
                        "(*(({element_c} *)flux_list_at_unchecked({}, 0, sizeof({element_c}))))",
                        base.code
                    ),
                    "first" => format!(
                        "(*(({element_c} *)flux_list_at({}, INT64_C(0), sizeof({element_c}))))",
                        base.code
                    ),
                    "last" => format!(
                        "(*(({element_c} *)flux_list_at({}, INT64_C(-1), sizeof({element_c}))))",
                        base.code
                    ),
                    "single" => format!("(*(({element_c} *)flux_list_single({})))", base.code),
                    _ => {
                        return Err(diag(
                            expr.span,
                            "unknown list property reached code generation",
                        ));
                    }
                }
            } else {
                format!("({}).{}", base.code, field_c_name(name))
            };
            EmittedExpr {
                code,
                ty: result_ty,
            }
        }
        ExprKind::Unary { op, expr: inner } => {
            let inner = emit_expr(inner, env, signatures)?;
            EmittedExpr {
                code: match op {
                    UnaryOp::Neg => format!("flux_neg_i64({})", inner.code),
                    UnaryOp::Not => format!("(!{})", inner.code),
                },
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
            let code = if matches!(op, BinOp::Add) {
                format!("flux_add_i64({}, {})", left.code, right.code)
            } else if matches!(op, BinOp::Sub) {
                format!("flux_sub_i64({}, {})", left.code, right.code)
            } else if matches!(op, BinOp::Mul) {
                format!("flux_mul_i64({}, {})", left.code, right.code)
            } else if matches!(op, BinOp::Div) {
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

fn emit_qualified_call(
    span: SourceSpan,
    namespace: &str,
    name: &str,
    args: &[Expr],
    named_args: &[NamedArg],
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<(String, Vec<Type>, Option<String>), Diagnostic> {
    if namespace == "android" {
        if args.len() != 1 || !named_args.is_empty() {
            return Err(diag(
                span,
                "invalid android platform call reached code generation",
            ));
        }
        let value = emit_expr(&args[0], env, signatures)?;
        let code = match name {
            "vibrate" => format!("flux__android_vibrate({})", value.code),
            "open_url" => format!("flux__android_open_url({})", value.code),
            _ => {
                return Err(diag(
                    span,
                    "unknown android platform call reached code generation",
                ));
            }
        };
        return Ok((code, Vec::new(), None));
    }
    if let Some(definition) = signatures.enum_type(namespace) {
        let variant = definition
            .variant(name)
            .ok_or_else(|| diag(span, "unknown enum variant reached code generation"))?;
        if !named_args.is_empty() || args.len() != variant.payloads.len() {
            return Err(diag(
                span,
                "invalid enum variant call reached code generation",
            ));
        }
        let mut rendered = Vec::with_capacity(args.len());
        for arg in args {
            rendered.push(emit_expr(arg, env, signatures)?.code);
        }
        return Ok((
            format!(
                "{}({})",
                enum_variant_helper_name(namespace, name),
                rendered.join(", ")
            ),
            vec![Type::Named(namespace.to_string())],
            None,
        ));
    }

    let interface = signatures.interface(namespace).ok_or_else(|| {
        diag(
            span,
            "unknown interface reached static dispatch code generation",
        )
    })?;
    let member = interface
        .functions
        .get(name)
        .ok_or_else(|| diag(span, "unknown interface capability reached code generation"))?;
    let receiver = args
        .first()
        .ok_or_else(|| diag(span, "static interface dispatch requires a receiver"))?;
    let receiver_value = emit_expr(receiver, env, signatures)?;
    let receiver_ty = signatures.canonical_type(&receiver_value.ty);
    let Type::Named(target_name) = receiver_ty else {
        return Err(diag(
            receiver.span,
            "static interface dispatch requires a concrete receiver",
        ));
    };
    if target_name == namespace && signatures.interface(&target_name).is_some() {
        let mut rendered = Vec::with_capacity(member.param_details.len() + 1);
        rendered.push(receiver_value.code);
        rendered.extend(emit_call_arguments(
            member,
            &args[1..],
            named_args,
            env,
            signatures,
        )?);
        return Ok((
            format!(
                "{}({})",
                interface_dispatch_helper_name(namespace, name),
                rendered.join(", ")
            ),
            member.returns.clone(),
            (member.returns.len() > 1).then(|| interface_multi_return_struct_name(namespace, name)),
        ));
    }
    let implementation = signatures
        .implementation(namespace, &target_name)
        .ok_or_else(|| {
            diag(
                receiver.span,
                "missing interface implementation during code generation",
            )
        })?;
    let mapped = implementation.functions.get(name).ok_or_else(|| {
        diag(
            span,
            "missing interface capability mapping during code generation",
        )
    })?;
    let mut rendered = Vec::with_capacity(member.param_details.len() + 1);
    rendered.push(receiver_value.code);
    rendered.extend(emit_call_arguments(
        member,
        &args[1..],
        named_args,
        env,
        signatures,
    )?);
    Ok((
        format!("{}({})", function_c_name(mapped), rendered.join(", ")),
        member.returns.clone(),
        (member.returns.len() > 1).then(|| multi_return_struct_name(mapped)),
    ))
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
) -> Result<(String, String, Vec<Type>), Diagnostic> {
    match &expr.kind {
        ExprKind::ShellCall { name, args, .. } => {
            let call = Expr {
                line: expr.line,
                span: expr.span,
                kind: ExprKind::Call {
                    name: name.clone(),
                    args: args.clone(),
                    named_args: Vec::new(),
                },
            };
            emit_multi_expr(&call, env, signatures)
        }
        ExprKind::Pipe {
            input, name, args, ..
        } => {
            let mut call_args = Vec::with_capacity(args.len() + 1);
            call_args.push(pipe_input_expr(input, env, signatures));
            call_args.extend(args.iter().cloned());
            let call = Expr {
                line: expr.line,
                span: expr.span,
                kind: ExprKind::Call {
                    name: name.clone(),
                    args: call_args,
                    named_args: Vec::new(),
                },
            };
            emit_multi_expr(&call, env, signatures)
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } => {
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
                signature.returns.clone(),
            ))
        }
        ExprKind::QualifiedCall {
            namespace,
            name,
            args,
            named_args,
            ..
        } => {
            let (code, returns, mapped) = emit_qualified_call(
                expr.span, namespace, name, args, named_args, env, signatures,
            )?;
            if returns.len() < 2 {
                return Err(diag(
                    expr.span,
                    &format!("qualified call '{namespace}.{name}' does not return multiple values"),
                ));
            }
            let multi_struct = mapped.ok_or_else(|| {
                diag(
                    expr.span,
                    "multi-value interface dispatch is missing its native return shape",
                )
            })?;
            Ok((code, multi_struct, returns))
        }
        _ => Err(diag(
            expr.span,
            "only multi-value function or interface calls can be destructured",
        )),
    }
}

fn interface_targets(
    program: &Program,
    interface_name: &str,
    signatures: &Signatures,
    reachable_value_types: &HashSet<String>,
    interface_pack_facts: &InterfacePackFacts,
) -> Vec<String> {
    let mut targets = Vec::new();
    for implementation in &program.implementations {
        if implementation.interface_name != interface_name {
            continue;
        }
        let Type::Named(target_name) =
            signatures.canonical_type(&Type::Named(implementation.target_name.clone()))
        else {
            continue;
        };
        if interface_pack_facts.allows(interface_name, &target_name)
            && reachable_value_types.contains(&target_name)
            && !targets.contains(&target_name)
        {
            targets.push(target_name);
        }
    }
    targets
}

fn emit_interface_value_definitions(
    out: &mut String,
    program: &Program,
    signatures: &Signatures,
    reachable_interfaces: &HashSet<String>,
    reachable_value_types: &HashSet<String>,
    interface_pack_facts: &InterfacePackFacts,
) {
    for definition in &program.interfaces {
        if !reachable_interfaces.contains(&definition.name) {
            continue;
        }
        let targets = interface_targets(
            program,
            &definition.name,
            signatures,
            reachable_value_types,
            interface_pack_facts,
        );
        if targets.len() > 1 {
            out.push_str("enum {\n");
            for (index, target) in targets.iter().enumerate() {
                out.push_str(&format!(
                    "    {} = {},\n",
                    interface_tag_name(&definition.name, target),
                    index + 1
                ));
            }
            out.push_str("};\n");
        }
        out.push_str(&format!(
            "struct {} {{\n",
            interface_c_name(&definition.name)
        ));
        match targets.as_slice() {
            [] => out.push_str("    int32_t tag;\n"),
            [target] => out.push_str(&format!(
                "    struct {} {};\n",
                struct_c_name(target),
                interface_value_member_name(target)
            )),
            _ => {
                out.push_str("    int32_t tag;\n");
                out.push_str("    union {\n");
                for target in &targets {
                    out.push_str(&format!(
                        "        struct {} {};\n",
                        struct_c_name(target),
                        interface_value_member_name(target)
                    ));
                }
                out.push_str("    } value;\n");
            }
        }
        out.push_str("};\n\n");
    }
}

fn interface_pack_helpers(
    program: &Program,
    signatures: &Signatures,
    reachable_interfaces: &HashSet<String>,
    reachable_value_types: &HashSet<String>,
    interface_pack_facts: &InterfacePackFacts,
    runtime_usage: &str,
) -> String {
    let mut out = String::new();
    for definition in &program.interfaces {
        if !reachable_interfaces.contains(&definition.name) {
            continue;
        }
        for target in interface_targets(
            program,
            &definition.name,
            signatures,
            reachable_value_types,
            interface_pack_facts,
        ) {
            let helper = interface_pack_helper_name(&definition.name, &target);
            if !runtime_usage.contains(&format!("{helper}(")) {
                continue;
            }
            out.push_str(&format!(
                "static inline struct {} {helper}(struct {} value) {{\n",
                interface_c_name(&definition.name),
                struct_c_name(&target)
            ));
            out.push_str(&format!(
                "    struct {} result;\n",
                interface_c_name(&definition.name)
            ));
            let targets = interface_targets(
                program,
                &definition.name,
                signatures,
                reachable_value_types,
                interface_pack_facts,
            );
            if targets.len() == 1 {
                out.push_str(&format!(
                    "    result.{} = value;\n",
                    interface_value_member_name(&target)
                ));
            } else {
                out.push_str(&format!(
                    "    result.tag = {};\n",
                    interface_tag_name(&definition.name, &target)
                ));
                out.push_str(&format!(
                    "    result.value.{} = value;\n",
                    interface_value_member_name(&target)
                ));
            }
            out.push_str("    return result;\n}\n");
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

fn emit_interface_multi_return_structs(
    out: &mut String,
    program: &Program,
    signatures: &Signatures,
    reachable_interfaces: &HashSet<String>,
    runtime_usage: &str,
) {
    let mut emitted = false;
    for definition in &program.interfaces {
        if !reachable_interfaces.contains(&definition.name) {
            continue;
        }
        let Some(interface) = signatures.interface(&definition.name) else {
            continue;
        };
        let mut member_names = interface.functions.keys().collect::<Vec<_>>();
        member_names.sort();
        for member_name in member_names {
            let helper = interface_dispatch_helper_name(&definition.name, member_name);
            if !runtime_usage.contains(&format!("{helper}(")) {
                continue;
            }
            let signature = &interface.functions[member_name];
            if signature.returns.len() < 2 {
                continue;
            }
            emitted = true;
            out.push_str(&format!(
                "struct {} {{\n",
                interface_multi_return_struct_name(&definition.name, member_name)
            ));
            for (index, ty) in signature.returns.iter().enumerate() {
                out.push_str(&format!("    {} v{index};\n", c_type(ty, signatures)));
            }
            out.push_str("};\n");
        }
    }
    if emitted {
        out.push('\n');
    }
}

fn emit_interface_dispatch_helpers(
    out: &mut String,
    program: &Program,
    signatures: &Signatures,
    reachable_interfaces: &HashSet<String>,
    reachable_value_types: &HashSet<String>,
    interface_pack_facts: &InterfacePackFacts,
    runtime_usage: &str,
) -> Result<(), Diagnostic> {
    let mut emitted = false;
    for definition in &program.interfaces {
        if !reachable_interfaces.contains(&definition.name) {
            continue;
        }
        let Some(interface) = signatures.interface(&definition.name) else {
            continue;
        };
        let targets = interface_targets(
            program,
            &definition.name,
            signatures,
            reachable_value_types,
            interface_pack_facts,
        );
        let mut member_names = interface.functions.keys().collect::<Vec<_>>();
        member_names.sort();
        for member_name in member_names {
            let helper = interface_dispatch_helper_name(&definition.name, member_name);
            if !runtime_usage.contains(&format!("{helper}(")) {
                continue;
            }
            let member = &interface.functions[member_name];
            emitted = true;
            let return_type = match member.returns.as_slice() {
                [] => "void".to_string(),
                [ty] => c_type(ty, signatures),
                _ => format!(
                    "struct {}",
                    interface_multi_return_struct_name(&definition.name, member_name)
                ),
            };
            out.push_str(&format!(
                "static inline {return_type} {helper}(struct {} receiver",
                interface_c_name(&definition.name)
            ));
            for (index, param) in member.param_details.iter().enumerate() {
                out.push_str(&format!(", {} arg_{index}", c_type(&param.ty, signatures)));
            }
            if let [target] = targets.as_slice() {
                out.push_str(") {\n");
                let implementation = signatures
                    .implementation(&definition.name, target)
                    .expect("type checking records each interface implementation");
                let mapped = implementation
                    .functions
                    .get(member_name)
                    .expect("type checking requires exhaustive capability mappings");
                let mut args = vec![format!("receiver.{}", interface_value_member_name(target))];
                args.extend((0..member.param_details.len()).map(|index| format!("arg_{index}")));
                let call = format!("{}({})", function_c_name(mapped), args.join(", "));
                match member.returns.as_slice() {
                    [] => out.push_str(&format!("    {call};\n}}\n")),
                    [_] => out.push_str(&format!("    return {call};\n}}\n")),
                    returns => {
                        out.push_str(&format!(
                            "    struct {} raw = {call};\n",
                            multi_return_struct_name(mapped)
                        ));
                        out.push_str(&format!(
                            "    struct {} result;\n",
                            interface_multi_return_struct_name(&definition.name, member_name)
                        ));
                        for index in 0..returns.len() {
                            out.push_str(&format!("    result.v{index} = raw.v{index};\n"));
                        }
                        out.push_str("    return result;\n}\n");
                    }
                }
                continue;
            }
            out.push_str(") {\n    switch (receiver.tag) {\n");
            for target in &targets {
                let implementation = signatures
                    .implementation(&definition.name, target)
                    .expect("type checking records each interface implementation");
                let mapped = implementation
                    .functions
                    .get(member_name)
                    .expect("type checking requires exhaustive capability mappings");
                let mut args = vec![format!(
                    "receiver.value.{}",
                    interface_value_member_name(target)
                )];
                args.extend((0..member.param_details.len()).map(|index| format!("arg_{index}")));
                let call = format!("{}({})", function_c_name(mapped), args.join(", "));
                out.push_str(&format!(
                    "        case {}: {{\n",
                    interface_tag_name(&definition.name, target)
                ));
                match member.returns.as_slice() {
                    [] => {
                        out.push_str(&format!("            {call};\n            return;\n"));
                    }
                    [_] => {
                        out.push_str(&format!("            return {call};\n"));
                    }
                    returns => {
                        out.push_str(&format!(
                            "            struct {} raw = {call};\n",
                            multi_return_struct_name(mapped)
                        ));
                        out.push_str(&format!(
                            "            struct {} result;\n",
                            interface_multi_return_struct_name(&definition.name, member_name)
                        ));
                        for index in 0..returns.len() {
                            out.push_str(&format!("            result.v{index} = raw.v{index};\n"));
                        }
                        out.push_str("            return result;\n");
                    }
                }
                out.push_str("        }\n");
            }
            out.push_str(
                "        default: fputs(\"Flux runtime error: invalid interface value\\n\", stderr); abort();\n    }\n}\n",
            );
        }
    }
    if emitted {
        out.push('\n');
    }
    Ok(())
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

fn interface_c_name(interface_name: &str) -> String {
    format!("flux__iface_{interface_name}")
}

fn interface_tag_name(interface_name: &str, target_name: &str) -> String {
    format!("flux__iface_tag_{interface_name}_{target_name}")
}

fn interface_value_member_name(target_name: &str) -> String {
    format!("flux__value_{target_name}")
}

fn interface_pack_helper_name(interface_name: &str, target_name: &str) -> String {
    format!("flux__iface_pack_{interface_name}_{target_name}")
}

fn interface_dispatch_helper_name(interface_name: &str, member_name: &str) -> String {
    format!("flux__iface_call_{interface_name}_{member_name}")
}

fn interface_multi_return_struct_name(interface_name: &str, member_name: &str) -> String {
    format!("flux__iface_ret_{interface_name}_{member_name}")
}

fn c_type(ty: &Type, signatures: &Signatures) -> String {
    match signatures.canonical_type(ty) {
        Type::I64 => "int64_t".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Str => "const char *".to_string(),
        Type::Error => "const char *".to_string(),
        Type::Void => "void".to_string(),
        Type::Named(name) if signatures.interface(&name).is_some() => {
            format!("struct {}", interface_c_name(&name))
        }
        Type::Named(name) => format!("struct {}", struct_c_name(&name)),
        Type::List(_) => "struct flux__list".to_string(),
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
        Type::List(element) => format!("list_{}", type_mangle(&element, signatures)),
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
    reachable_functions: &HashSet<String>,
    reachable_value_types: &HashSet<String>,
    reachable_interfaces: &HashSet<String>,
    function_ir: &FunctionIrCache,
) -> Result<(), Diagnostic> {
    let mut types = HashSet::new();
    for definition in &program.structs {
        if !reachable_value_types.contains(&definition.name) {
            continue;
        }
        for field in &definition.fields {
            collect_function_type(&field.ty, signatures, &mut types);
        }
    }
    for definition in &program.enums {
        if !reachable_value_types.contains(&definition.name) {
            continue;
        }
        for variant in &definition.variants {
            for payload in &variant.payloads {
                collect_function_type(&payload.ty, signatures, &mut types);
            }
        }
    }
    for definition in &program.interfaces {
        if !reachable_interfaces.contains(&definition.name) {
            continue;
        }
        for function in &definition.functions {
            for param in &function.params {
                collect_function_type(&param.ty, signatures, &mut types);
            }
            for ty in &function.returns {
                collect_function_type(ty, signatures, &mut types);
            }
        }
    }
    for view in runtime_views(program) {
        for param in &view.params {
            collect_function_type(&param.ty, signatures, &mut types);
        }
        for state in &view.states {
            collect_function_type(&state.ty, signatures, &mut types);
        }
        for derived in &view.derived {
            collect_function_type(&derived.ty, signatures, &mut types);
        }
    }
    for function in &program.functions {
        if !reachable_functions.contains(&function.name) {
            continue;
        }
        for param in &function.params {
            collect_function_type(&param.ty, signatures, &mut types);
        }
        for ty in &function.returns {
            collect_function_type(ty, signatures, &mut types);
        }
        if let Some(cfg) = function_ir.get(&function.name) {
            for node in cfg.nodes().iter().filter(|node| cfg.is_reachable(node.id)) {
                for definition in &node.definitions {
                    collect_function_type(&definition.ty, signatures, &mut types);
                }
            }
            for value in cfg
                .values()
                .iter()
                .filter(|value| cfg.is_value_reachable(value.id))
            {
                collect_function_type(&value.ty, signatures, &mut types);
            }
        }
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

fn program_uses_background(
    program: &Program,
    reachable_functions: &HashSet<String>,
    function_ir: &FunctionIrCache,
) -> bool {
    program.functions.iter().any(|function| {
        if !reachable_functions.contains(&function.name) {
            return false;
        }
        let Some(cfg) = function_ir.get(&function.name) else {
            return block_uses_background(&function.body, None);
        };
        let reachable_spans = cfg
            .nodes()
            .iter()
            .filter(|node| cfg.is_reachable(node.id))
            .map(|node| source_span_key(node.span))
            .collect::<HashSet<_>>();
        block_uses_background(&function.body, Some(&reachable_spans))
    })
}

fn block_uses_background(
    body: &[Stmt],
    reachable_spans: Option<&HashSet<(u32, usize, usize, usize)>>,
) -> bool {
    body.iter().any(|stmt| {
        if reachable_spans.is_some_and(|spans| !spans.contains(&source_span_key(stmt.span))) {
            return false;
        }
        match &stmt.kind {
            StmtKind::Shell { background, .. } => *background,
            StmtKind::If {
                body, else_body, ..
            } => {
                block_uses_background(body, reachable_spans)
                    || block_uses_background(else_body, reachable_spans)
            }
            StmtKind::ForRange { body, .. }
            | StmtKind::ForEach { body, .. }
            | StmtKind::While { body, .. } => block_uses_background(body, reachable_spans),
            StmtKind::Match { arms, .. } => arms
                .iter()
                .any(|arm| block_uses_background(&arm.body, reachable_spans)),
            StmtKind::ListMatch { arms, .. } => arms
                .iter()
                .any(|arm| block_uses_background(&arm.body, reachable_spans)),
            _ => false,
        }
    })
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

fn struct_update_helpers(
    program: &Program,
    signatures: &Signatures,
    reachable_functions: &HashSet<String>,
    function_ir: &FunctionIrCache,
) -> String {
    let mut all_helpers = HashSet::new();
    let mut ignored = String::new();
    let mut reachable_helpers = HashSet::new();
    for function in &program.functions {
        if !reachable_functions.contains(&function.name) {
            continue;
        }
        collect_update_helpers_from_block(
            &function.body,
            signatures,
            &mut all_helpers,
            &mut ignored,
        );
        let Some(cfg) = function_ir.get(&function.name) else {
            reachable_helpers.extend(all_helpers.iter().cloned());
            continue;
        };
        for value in cfg
            .values()
            .iter()
            .filter(|value| cfg.is_value_reachable(value.id))
        {
            if let crate::ir::ControlFlowValueKind::StructLiteral {
                name,
                base: Some(_),
                fields,
            } = &value.kind
            {
                reachable_helpers.insert(struct_update_helper_name_from_names(
                    name,
                    fields.iter().map(|(name, _)| name.as_str()),
                ));
            }
        }
    }

    let mut helpers = String::new();
    let mut emitted = all_helpers
        .difference(&reachable_helpers)
        .cloned()
        .collect::<HashSet<_>>();
    for function in &program.functions {
        if reachable_functions.contains(&function.name) {
            collect_update_helpers_from_block(
                &function.body,
                signatures,
                &mut emitted,
                &mut helpers,
            );
        }
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
            | StmtKind::Var { expr, .. }
            | StmtKind::Assign { expr, .. }
            | StmtKind::LetDestructure { expr, .. }
            | StmtKind::LetMultiDestructure { expr, .. }
            | StmtKind::LetListDestructure { expr, .. }
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
            StmtKind::Shell { expr, redirect, .. } => {
                collect_update_helpers_from_expr(expr, signatures, emitted, helpers);
                if let Some(redirect) = redirect {
                    collect_update_helpers_from_expr(&redirect.path, signatures, emitted, helpers);
                }
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
            StmtKind::While { cond, body } => {
                collect_update_helpers_from_expr(cond, signatures, emitted, helpers);
                collect_update_helpers_from_block(body, signatures, emitted, helpers);
            }
            StmtKind::ForRange {
                start, end, body, ..
            } => {
                collect_update_helpers_from_expr(start, signatures, emitted, helpers);
                collect_update_helpers_from_expr(end, signatures, emitted, helpers);
                collect_update_helpers_from_block(body, signatures, emitted, helpers);
            }
            StmtKind::ForEach { iterable, body, .. } => {
                collect_update_helpers_from_expr(iterable, signatures, emitted, helpers);
                collect_update_helpers_from_block(body, signatures, emitted, helpers);
            }
            StmtKind::Match { value, arms } => {
                collect_update_helpers_from_expr(value, signatures, emitted, helpers);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        collect_update_helpers_from_expr(guard, signatures, emitted, helpers);
                    }
                    collect_update_helpers_from_block(&arm.body, signatures, emitted, helpers);
                }
            }
            StmtKind::ListMatch { value, arms } => {
                collect_update_helpers_from_expr(value, signatures, emitted, helpers);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        collect_update_helpers_from_expr(guard, signatures, emitted, helpers);
                    }
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
        ExprKind::AnonymousFunction { body, .. } => {
            collect_update_helpers_from_expr(body, signatures, emitted, helpers);
        }
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
        ExprKind::Match { value, arms } => {
            collect_update_helpers_from_expr(value, signatures, emitted, helpers);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_update_helpers_from_expr(guard, signatures, emitted, helpers);
                }
                collect_update_helpers_from_expr(&arm.value, signatures, emitted, helpers);
            }
        }
        ExprKind::ListMatch { value, arms } => {
            collect_update_helpers_from_expr(value, signatures, emitted, helpers);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_update_helpers_from_expr(guard, signatures, emitted, helpers);
                }
                collect_update_helpers_from_expr(&arm.value, signatures, emitted, helpers);
            }
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            collect_update_helpers_from_expr(then_expr, signatures, emitted, helpers);
            collect_update_helpers_from_expr(cond, signatures, emitted, helpers);
            collect_update_helpers_from_expr(else_expr, signatures, emitted, helpers);
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
        ExprKind::ShellCall { args, .. } => {
            for arg in args {
                collect_update_helpers_from_expr(arg, signatures, emitted, helpers);
            }
        }
        ExprKind::Pipe { input, args, .. } => {
            collect_update_helpers_from_expr(input, signatures, emitted, helpers);
            for arg in args {
                collect_update_helpers_from_expr(arg, signatures, emitted, helpers);
            }
        }
        ExprKind::List(items) => {
            for item in items {
                collect_update_helpers_from_expr(item, signatures, emitted, helpers);
            }
        }
        ExprKind::ListSpread { value, .. } => {
            collect_update_helpers_from_expr(value, signatures, emitted, helpers);
        }
        ExprKind::ListIf {
            condition,
            value,
            else_value,
            ..
        } => {
            collect_update_helpers_from_expr(condition, signatures, emitted, helpers);
            collect_update_helpers_from_expr(value, signatures, emitted, helpers);
            if let Some(else_value) = else_value {
                collect_update_helpers_from_expr(else_value, signatures, emitted, helpers);
            }
        }
        ExprKind::Index { base, index } => {
            collect_update_helpers_from_expr(base, signatures, emitted, helpers);
            collect_update_helpers_from_expr(index, signatures, emitted, helpers);
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            collect_update_helpers_from_expr(base, signatures, emitted, helpers);
            if let Some(start) = start {
                collect_update_helpers_from_expr(start, signatures, emitted, helpers);
            }
            if let Some(end) = end {
                collect_update_helpers_from_expr(end, signatures, emitted, helpers);
            }
            if let Some(step) = step {
                collect_update_helpers_from_expr(step, signatures, emitted, helpers);
            }
        }
        ExprKind::ListComprehension {
            value,
            iterable,
            condition,
            ..
        } => {
            collect_update_helpers_from_expr(iterable, signatures, emitted, helpers);
            collect_update_helpers_from_expr(value, signatures, emitted, helpers);
            if let Some(condition) = condition {
                collect_update_helpers_from_expr(condition, signatures, emitted, helpers);
            }
        }
        ExprKind::QualifiedCall {
            args, named_args, ..
        } => {
            for arg in args {
                collect_update_helpers_from_expr(arg, signatures, emitted, helpers);
            }
            for arg in named_args {
                collect_update_helpers_from_expr(&arg.value, signatures, emitted, helpers);
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
    struct_update_helper_name_from_names(name, fields.iter().map(|field| field.name.as_str()))
}

fn struct_update_helper_name_from_names<'a>(
    name: &str,
    fields: impl IntoIterator<Item = &'a str>,
) -> String {
    let fields = fields.into_iter().collect::<Vec<_>>();
    let suffix = if fields.is_empty() {
        "copy".to_string()
    } else {
        fields.join("__")
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

fn enum_variant_helpers(
    program: &Program,
    signatures: &Signatures,
    reachable_value_types: &HashSet<String>,
    reachable_enum_variants: &HashSet<(String, String)>,
) -> String {
    let mut out = String::new();
    for definition in &program.enums {
        if !reachable_value_types.contains(&definition.name) {
            continue;
        }
        for variant in &definition.variants {
            if !reachable_enum_variants.contains(&(definition.name.clone(), variant.name.clone())) {
                continue;
            }
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

fn static_list_length(expr: &Expr) -> Option<usize> {
    match &expr.kind {
        ExprKind::List(items)
            if items.iter().all(|item| {
                !matches!(
                    item.kind,
                    ExprKind::ListSpread { .. } | ExprKind::ListIf { .. }
                )
            }) =>
        {
            Some(items.len())
        }
        _ => None,
    }
}

fn resolved_static_list_index(
    base: &Expr,
    index: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Option<usize>, Diagnostic> {
    let Some(len) = static_list_length(base) else {
        return Ok(None);
    };
    let Some(ConstantValue::I64(index)) = fold_primitive_expr(index, env, signatures)? else {
        return Ok(None);
    };
    let resolved = if index < 0 {
        let Ok(len_i64) = i64::try_from(len) else {
            return Ok(None);
        };
        index.checked_add(len_i64)
    } else {
        Some(index)
    };
    let Some(resolved) = resolved else {
        return Ok(None);
    };
    if resolved < 0 {
        return Ok(None);
    }
    let Ok(resolved) = usize::try_from(resolved) else {
        return Ok(None);
    };
    Ok((resolved < len).then_some(resolved))
}

fn fold_primitive_expr(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Option<ConstantValue>, Diagnostic> {
    let value = match &expr.kind {
        ExprKind::Int(value) => Some(ConstantValue::I64(*value)),
        ExprKind::Bool(value) => Some(ConstantValue::Bool(*value)),
        ExprKind::Str(value) => Some(ConstantValue::Str(value.clone())),
        ExprKind::Var(name) => {
            if env.contains_key(name) {
                None
            } else {
                signatures
                    .constant(name)
                    .map(|constant| constant.value.clone())
            }
        }
        ExprKind::Unary { op, expr: inner } => {
            let Some(inner) = fold_primitive_expr(inner, env, signatures)? else {
                return Ok(None);
            };
            Some(match (op, inner) {
                (UnaryOp::Neg, ConstantValue::I64(value)) => {
                    ConstantValue::I64(value.checked_neg().ok_or_else(|| {
                        diag(expr.span, "constant integer negation overflows i64")
                    })?)
                }
                (UnaryOp::Not, ConstantValue::Bool(value)) => ConstantValue::Bool(!value),
                _ => return Ok(None),
            })
        }
        ExprKind::Binary { left, op, right } => {
            let Some(left) = fold_primitive_expr(left, env, signatures)? else {
                return Ok(None);
            };
            if matches!(op, BinOp::And) && left == ConstantValue::Bool(false) {
                return Ok(Some(ConstantValue::Bool(false)));
            }
            if matches!(op, BinOp::Or) && left == ConstantValue::Bool(true) {
                return Ok(Some(ConstantValue::Bool(true)));
            }
            let Some(right) = fold_primitive_expr(right, env, signatures)? else {
                return Ok(None);
            };
            Some(fold_primitive_binary(expr.span, *op, left, right)?)
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            let Some(condition) = fold_primitive_expr(cond, env, signatures)? else {
                return Ok(None);
            };
            match condition {
                ConstantValue::Bool(true) => fold_primitive_expr(then_expr, env, signatures)?,
                ConstantValue::Bool(false) => fold_primitive_expr(else_expr, env, signatures)?,
                _ => return Ok(None),
            }
        }
        _ => None,
    };
    Ok(value)
}

fn fold_primitive_binary(
    span: SourceSpan,
    op: BinOp,
    left: ConstantValue,
    right: ConstantValue,
) -> Result<ConstantValue, Diagnostic> {
    match (op, left, right) {
        (BinOp::Add, ConstantValue::I64(left), ConstantValue::I64(right)) => left
            .checked_add(right)
            .map(ConstantValue::I64)
            .ok_or_else(|| diag(span, "constant integer addition overflows i64")),
        (BinOp::Sub, ConstantValue::I64(left), ConstantValue::I64(right)) => left
            .checked_sub(right)
            .map(ConstantValue::I64)
            .ok_or_else(|| diag(span, "constant integer subtraction overflows i64")),
        (BinOp::Mul, ConstantValue::I64(left), ConstantValue::I64(right)) => left
            .checked_mul(right)
            .map(ConstantValue::I64)
            .ok_or_else(|| diag(span, "constant integer multiplication overflows i64")),
        (BinOp::Div, ConstantValue::I64(_), ConstantValue::I64(0)) => {
            Err(diag(span, "constant integer division by zero is invalid"))
        }
        (BinOp::Div, ConstantValue::I64(i64::MIN), ConstantValue::I64(-1)) => {
            Err(diag(span, "constant integer division overflows i64"))
        }
        (BinOp::Div, ConstantValue::I64(left), ConstantValue::I64(right)) => {
            Ok(ConstantValue::I64(left / right))
        }
        (BinOp::Lt, ConstantValue::I64(left), ConstantValue::I64(right)) => {
            Ok(ConstantValue::Bool(left < right))
        }
        (BinOp::Le, ConstantValue::I64(left), ConstantValue::I64(right)) => {
            Ok(ConstantValue::Bool(left <= right))
        }
        (BinOp::Gt, ConstantValue::I64(left), ConstantValue::I64(right)) => {
            Ok(ConstantValue::Bool(left > right))
        }
        (BinOp::Ge, ConstantValue::I64(left), ConstantValue::I64(right)) => {
            Ok(ConstantValue::Bool(left >= right))
        }
        (BinOp::Eq, left, right) if left.ty() == right.ty() => {
            Ok(ConstantValue::Bool(left == right))
        }
        (BinOp::Ne, left, right) if left.ty() == right.ty() => {
            Ok(ConstantValue::Bool(left != right))
        }
        (BinOp::And, ConstantValue::Bool(left), ConstantValue::Bool(right)) => {
            Ok(ConstantValue::Bool(left && right))
        }
        (BinOp::Or, ConstantValue::Bool(left), ConstantValue::Bool(right)) => {
            Ok(ConstantValue::Bool(left || right))
        }
        _ => Err(diag(
            span,
            "invalid primitive constant expression reached code generation",
        )),
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
