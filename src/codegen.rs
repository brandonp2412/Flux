use std::collections::{HashMap, HashSet};

use crate::ast::{
    BinOp, EnumDef, Expr, ExprKind, Function, ListMatchPattern, MatchPattern, NamedArg,
    PatternLogicalOp, Program, ShellRedirectMode, Stmt, StmtKind, StructDef, StructPatternField,
    Type, UnaryOp,
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
    if target == NativeTarget::Android && runtime_usage.contains("flux__process_") {
        return Err(Diagnostic::global(
            DiagnosticStage::Codegen,
            "process.* APIs require a desktop/server target",
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
    if runtime_usage.contains("flux__time_")
        || runtime_usage.contains("flux__process_termination_requested(")
    {
        out.push_str("#define _POSIX_C_SOURCE 200809L\n");
    }
    out.push_str("#include <stdbool.h>\n");
    out.push_str("#include <stdint.h>\n");
    out.push_str("#include <stddef.h>\n");
    out.push_str("#include <stdio.h>\n");
    out.push_str("#include <stdlib.h>\n");
    out.push_str("#include <string.h>\n");
    if uses_background || runtime_usage.contains("flux__time_sleep_millis(") {
        out.push_str("#include <errno.h>\n");
    }
    if uses_background {
        out.push_str("#include <sys/types.h>\n");
        out.push_str("#include <sys/wait.h>\n");
    }
    if runtime_usage.contains("flux__time_") {
        out.push_str("#include <time.h>\n");
    }
    if runtime_usage.contains("flux__process_termination_requested(") {
        out.push_str("#include <signal.h>\n");
    }
    if uses_background
        || runtime_usage.contains("flux__process_pid(")
        || runtime_usage.contains("flux__process_parent_pid(")
        || runtime_usage.contains("flux__fs_")
    {
        out.push_str("#include <unistd.h>\n");
    }
    if runtime_usage.contains("flux__fs_") {
        out.push_str("#include <sys/stat.h>\n");
    }
    if uses_gtk {
        out.push_str("#include <gtk/gtk.h>\n");
    }
    if uses_android {
        out.push_str("#include <android/native_activity.h>\n");
        if runtime_usage.contains("flux__android_sdk_int(")
            || runtime_usage.contains("flux__android_create_notification_channel(")
            || runtime_usage.contains("flux__android_notification_permission_granted(")
            || runtime_usage.contains("flux__android_request_notification_permission(")
            || runtime_usage.contains("flux__android_permission_granted(")
            || runtime_usage.contains("flux__android_request_permission(")
            || runtime_usage.contains("flux__android_notify(")
            || runtime_usage.contains("flux__android_notify_url_action(")
        {
            out.push_str("#include <android/api-level.h>\n");
        }
    }
    out.push('\n');

    let uses_locale = runtime_usage.contains("flux__locale_");
    let uses_android_sdk_int = uses_android && runtime_usage.contains("flux__android_sdk_int(");
    let uses_android_vibrate = uses_android && runtime_usage.contains("flux__android_vibrate(");
    let uses_android_open_url = uses_android && runtime_usage.contains("flux__android_open_url(");
    let uses_android_share = uses_android && runtime_usage.contains("flux__android_share(");
    let uses_android_show_keyboard =
        uses_android && runtime_usage.contains("flux__android_show_keyboard(");
    let uses_android_hide_keyboard =
        uses_android && runtime_usage.contains("flux__android_hide_keyboard(");
    let uses_android_focus_next =
        uses_android && runtime_usage.contains("flux__android_focus_next(");
    let uses_android_focus_previous =
        uses_android && runtime_usage.contains("flux__android_focus_previous(");
    let uses_android_focus_next_wrap =
        uses_android && runtime_usage.contains("flux__android_focus_next_wrap(");
    let uses_android_focus_previous_wrap =
        uses_android && runtime_usage.contains("flux__android_focus_previous_wrap(");
    let uses_android_focus_first =
        uses_android && runtime_usage.contains("flux__android_focus_first(");
    let uses_android_focus_last =
        uses_android && runtime_usage.contains("flux__android_focus_last(");
    let uses_android_focus_edges = uses_android_focus_first
        || uses_android_focus_last
        || uses_android_focus_next_wrap
        || uses_android_focus_previous_wrap;
    let uses_android_clear_focus =
        uses_android && runtime_usage.contains("flux__android_clear_focus(");
    let uses_android_focus_navigation = uses_android_focus_next
        || uses_android_focus_previous
        || uses_android_focus_next_wrap
        || uses_android_focus_previous_wrap
        || uses_android_focus_edges
        || uses_android_clear_focus;
    let uses_android_selection_start =
        uses_android && runtime_usage.contains("flux__android_selection_start(");
    let uses_android_selection_end =
        uses_android && runtime_usage.contains("flux__android_selection_end(");
    let uses_android_set_caret = uses_android && runtime_usage.contains("flux__android_set_caret(");
    let uses_android_set_selection =
        uses_android && runtime_usage.contains("flux__android_set_selection(");
    let uses_android_text_selection = uses_android_selection_start
        || uses_android_selection_end
        || uses_android_set_caret
        || uses_android_set_selection;
    let uses_android_create_notification_channel =
        uses_android && runtime_usage.contains("flux__android_create_notification_channel(");
    let uses_android_notification_permission_granted =
        uses_android && runtime_usage.contains("flux__android_notification_permission_granted(");
    let uses_android_request_notification_permission =
        uses_android && runtime_usage.contains("flux__android_request_notification_permission(");
    let uses_android_permission_granted =
        uses_android && runtime_usage.contains("flux__android_permission_granted(");
    let uses_android_request_permission =
        uses_android && runtime_usage.contains("flux__android_request_permission(");
    let uses_android_notify = uses_android && runtime_usage.contains("flux__android_notify(");
    let uses_android_notify_url_action =
        uses_android && runtime_usage.contains("flux__android_notify_url_action(");
    let uses_android_cancel_notification =
        uses_android && runtime_usage.contains("flux__android_cancel_notification(");
    let uses_android_generated_ui =
        uses_android && runtime_usage.contains("Java_app_flux_runtime_FluxActivity_nativeBuildUi");
    let uses_android_notifications = uses_android_create_notification_channel
        || uses_android_notification_permission_granted
        || uses_android_request_notification_permission
        || uses_android_notify
        || uses_android_notify_url_action
        || uses_android_cancel_notification;
    let uses_android_platform_api = uses_android_vibrate
        || uses_android_open_url
        || uses_android_share
        || uses_android_show_keyboard
        || uses_android_hide_keyboard
        || uses_android_focus_navigation
        || uses_android_text_selection
        || uses_android_notifications
        || uses_android_permission_granted
        || uses_android_request_permission
        || uses_android_generated_ui
        || (uses_android && uses_locale);
    if uses_android {
        out.push_str("static ANativeActivity *flux__android_activity = NULL;\n");
    }
    if uses_android_generated_ui {
        out.push_str("static ANativeActivity flux__android_activity_compat = {0};\n");
    }
    if uses_android_sdk_int {
        out.push_str("static inline int64_t flux__android_sdk_int(void) { return (int64_t)android_get_device_api_level(); }\n");
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
    if uses_android_open_url
        || uses_android_share
        || uses_android_notifications
        || uses_android_permission_granted
        || uses_android_request_permission
        || uses_android_generated_ui
    {
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
    if uses_android_show_keyboard || uses_android_hide_keyboard {
        out.push_str("static void flux__android_set_keyboard_visible(bool visible) {\n");
        out.push_str("    if (flux__android_activity == NULL) return;\n");
        out.push_str("    bool detach = false;\n");
        out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
        out.push_str("    if (env == NULL) return;\n");
        out.push_str("    jobject activity = flux__android_activity->clazz;\n");
        out.push_str("    jclass activity_class = NULL; jclass manager_class = NULL; jclass view_class = NULL;\n");
        out.push_str("    jstring service_name = NULL; jobject manager = NULL; jobject view = NULL; jobject token = NULL;\n");
        out.push_str("    activity_class = (*env)->GetObjectClass(env, activity);\n");
        out.push_str("    if (activity_class == NULL) goto done;\n");
        out.push_str("    jmethodID get_focus = (*env)->GetMethodID(env, activity_class, \"getCurrentFocus\", \"()Landroid/view/View;\");\n");
        out.push_str("    jmethodID get_service = (*env)->GetMethodID(env, activity_class, \"getSystemService\", \"(Ljava/lang/String;)Ljava/lang/Object;\");\n");
        out.push_str("    if (get_focus == NULL || get_service == NULL) goto done;\n");
        out.push_str("    view = (*env)->CallObjectMethod(env, activity, get_focus);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || view == NULL) goto done;\n");
        out.push_str("    service_name = (*env)->NewStringUTF(env, \"input_method\");\n");
        out.push_str("    if (service_name == NULL) goto done;\n");
        out.push_str(
            "    manager = (*env)->CallObjectMethod(env, activity, get_service, service_name);\n",
        );
        out.push_str("    if ((*env)->ExceptionCheck(env) || manager == NULL) goto done;\n");
        out.push_str("    manager_class = (*env)->GetObjectClass(env, manager);\n");
        out.push_str("    if (manager_class == NULL) goto done;\n");
        out.push_str("    if (visible) {\n");
        out.push_str("        jmethodID show = (*env)->GetMethodID(env, manager_class, \"showSoftInput\", \"(Landroid/view/View;I)Z\");\n");
        out.push_str("        if (show != NULL) (*env)->CallBooleanMethod(env, manager, show, view, (jint)0);\n");
        out.push_str("    } else {\n");
        out.push_str("        view_class = (*env)->GetObjectClass(env, view);\n");
        out.push_str("        if (view_class != NULL) {\n");
        out.push_str("            jmethodID get_token = (*env)->GetMethodID(env, view_class, \"getWindowToken\", \"()Landroid/os/IBinder;\");\n");
        out.push_str("            if (get_token != NULL) token = (*env)->CallObjectMethod(env, view, get_token);\n");
        out.push_str("        }\n");
        out.push_str("        if (token != NULL && !(*env)->ExceptionCheck(env)) {\n");
        out.push_str("            jmethodID hide = (*env)->GetMethodID(env, manager_class, \"hideSoftInputFromWindow\", \"(Landroid/os/IBinder;I)Z\");\n");
        out.push_str("            if (hide != NULL) (*env)->CallBooleanMethod(env, manager, hide, token, (jint)0);\n");
        out.push_str("        }\n");
        out.push_str("    }\n");
        out.push_str("done:\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
        out.push_str("    if (token != NULL) (*env)->DeleteLocalRef(env, token);\n");
        out.push_str("    if (view_class != NULL) (*env)->DeleteLocalRef(env, view_class);\n");
        out.push_str(
            "    if (manager_class != NULL) (*env)->DeleteLocalRef(env, manager_class);\n",
        );
        out.push_str("    if (manager != NULL) (*env)->DeleteLocalRef(env, manager);\n");
        out.push_str("    if (service_name != NULL) (*env)->DeleteLocalRef(env, service_name);\n");
        out.push_str("    if (view != NULL) (*env)->DeleteLocalRef(env, view);\n");
        out.push_str(
            "    if (activity_class != NULL) (*env)->DeleteLocalRef(env, activity_class);\n",
        );
        out.push_str("    flux__android_release_env(detach);\n");
        out.push_str("}\n");
        if uses_android_show_keyboard {
            out.push_str("static inline void flux__android_show_keyboard(void) { flux__android_set_keyboard_visible(true); }\n");
        }
        if uses_android_hide_keyboard {
            out.push_str("static inline void flux__android_hide_keyboard(void) { flux__android_set_keyboard_visible(false); }\n");
        }
    }
    if uses_android_focus_navigation {
        if uses_android_focus_edges {
            out.push_str("static void flux__android_focus_edge(int direction);\n");
        }
        out.push_str("static bool flux__android_change_focus(int direction) {\n");
        out.push_str("    if (flux__android_activity == NULL) return false;\n");
        out.push_str("    bool detach = false;\n");
        out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
        out.push_str("    if (env == NULL) return false;\n");
        out.push_str("    bool focused = false;\n");
        out.push_str("    jobject activity = flux__android_activity->clazz;\n");
        out.push_str("    jclass activity_class = NULL; jclass view_class = NULL; jclass target_class = NULL;\n");
        out.push_str("    jobject view = NULL; jobject target = NULL;\n");
        out.push_str("    activity_class = (*env)->GetObjectClass(env, activity);\n");
        out.push_str("    if (activity_class == NULL) goto done;\n");
        out.push_str("    jmethodID get_focus = (*env)->GetMethodID(env, activity_class, \"getCurrentFocus\", \"()Landroid/view/View;\");\n");
        out.push_str("    if (get_focus == NULL) goto done;\n");
        out.push_str("    view = (*env)->CallObjectMethod(env, activity, get_focus);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || view == NULL) goto done;\n");
        out.push_str("    view_class = (*env)->GetObjectClass(env, view);\n");
        out.push_str("    if (view_class == NULL) goto done;\n");
        out.push_str("    if (direction == 0) {\n");
        out.push_str("        jmethodID clear_focus = (*env)->GetMethodID(env, view_class, \"clearFocus\", \"()V\");\n");
        out.push_str("        if (clear_focus != NULL) { (*env)->CallVoidMethod(env, view, clear_focus); focused = true; }\n");
        out.push_str("        goto done;\n");
        out.push_str("    }\n");
        out.push_str("    jmethodID focus_search = (*env)->GetMethodID(env, view_class, \"focusSearch\", \"(I)Landroid/view/View;\");\n");
        out.push_str("    if (focus_search == NULL) goto done;\n");
        out.push_str(
            "    target = (*env)->CallObjectMethod(env, view, focus_search, (jint)direction);\n",
        );
        out.push_str("    if ((*env)->ExceptionCheck(env) || target == NULL) goto done;\n");
        out.push_str("    target_class = (*env)->GetObjectClass(env, target);\n");
        out.push_str("    if (target_class == NULL) goto done;\n");
        out.push_str("    jmethodID request_focus = (*env)->GetMethodID(env, target_class, \"requestFocusFromTouch\", \"()Z\");\n");
        out.push_str("    if (request_focus != NULL) focused = (*env)->CallBooleanMethod(env, target, request_focus) == JNI_TRUE;\n");
        out.push_str("done:\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
        out.push_str("    if (target_class != NULL) (*env)->DeleteLocalRef(env, target_class);\n");
        out.push_str("    if (target != NULL) (*env)->DeleteLocalRef(env, target);\n");
        out.push_str("    if (view_class != NULL) (*env)->DeleteLocalRef(env, view_class);\n");
        out.push_str("    if (view != NULL) (*env)->DeleteLocalRef(env, view);\n");
        out.push_str(
            "    if (activity_class != NULL) (*env)->DeleteLocalRef(env, activity_class);\n",
        );
        out.push_str("    flux__android_release_env(detach);\n");
        out.push_str("    return focused;\n");
        out.push_str("}\n");
        if uses_android_focus_next {
            out.push_str("static inline void flux__android_focus_next(void) { (void)flux__android_change_focus(2); }\n");
        }
        if uses_android_focus_previous {
            out.push_str("static inline void flux__android_focus_previous(void) { (void)flux__android_change_focus(1); }\n");
        }
        if uses_android_focus_next_wrap {
            out.push_str("static inline void flux__android_focus_next_wrap(bool wrap) { if (!flux__android_change_focus(2) && wrap) flux__android_focus_edge(2); }\n");
        }
        if uses_android_focus_previous_wrap {
            out.push_str("static inline void flux__android_focus_previous_wrap(bool wrap) { if (!flux__android_change_focus(1) && wrap) flux__android_focus_edge(1); }\n");
        }
        if uses_android_clear_focus {
            out.push_str("static inline void flux__android_clear_focus(void) { (void)flux__android_change_focus(0); }\n");
        }
        if uses_android_focus_edges {
            out.push_str("static void flux__android_focus_edge(int direction) {\n");
            out.push_str("    if (flux__android_activity == NULL) return;\n");
            out.push_str("    bool detach = false;\n");
            out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
            out.push_str("    if (env == NULL) return;\n");
            out.push_str("    jobject activity = flux__android_activity->clazz;\n");
            out.push_str("    jclass activity_class = NULL; jclass view_group_class = NULL; jclass focus_finder_class = NULL; jclass target_class = NULL;\n");
            out.push_str(
                "    jobject root = NULL; jobject finder = NULL; jobject target = NULL;\n",
            );
            out.push_str("    activity_class = (*env)->GetObjectClass(env, activity);\n");
            out.push_str("    if (activity_class == NULL) goto done;\n");
            out.push_str("    jmethodID find_view = (*env)->GetMethodID(env, activity_class, \"findViewById\", \"(I)Landroid/view/View;\");\n");
            out.push_str("    if (find_view == NULL) goto done;\n");
            out.push_str(
                "    root = (*env)->CallObjectMethod(env, activity, find_view, (jint)16908290);\n",
            );
            out.push_str("    if ((*env)->ExceptionCheck(env) || root == NULL) goto done;\n");
            out.push_str(
                "    view_group_class = (*env)->FindClass(env, \"android/view/ViewGroup\");\n",
            );
            out.push_str("    if (view_group_class == NULL || !(*env)->IsInstanceOf(env, root, view_group_class)) goto done;\n");
            out.push_str(
                "    focus_finder_class = (*env)->FindClass(env, \"android/view/FocusFinder\");\n",
            );
            out.push_str("    if (focus_finder_class == NULL) goto done;\n");
            out.push_str("    jmethodID get_instance = (*env)->GetStaticMethodID(env, focus_finder_class, \"getInstance\", \"()Landroid/view/FocusFinder;\");\n");
            out.push_str("    if (get_instance == NULL) goto done;\n");
            out.push_str("    finder = (*env)->CallStaticObjectMethod(env, focus_finder_class, get_instance);\n");
            out.push_str("    if ((*env)->ExceptionCheck(env) || finder == NULL) goto done;\n");
            out.push_str("    jmethodID find_next = (*env)->GetMethodID(env, focus_finder_class, \"findNextFocus\", \"(Landroid/view/ViewGroup;Landroid/view/View;I)Landroid/view/View;\");\n");
            out.push_str("    if (find_next == NULL) goto done;\n");
            out.push_str("    target = (*env)->CallObjectMethod(env, finder, find_next, root, NULL, (jint)direction);\n");
            out.push_str("    if ((*env)->ExceptionCheck(env) || target == NULL) goto done;\n");
            out.push_str("    target_class = (*env)->GetObjectClass(env, target);\n");
            out.push_str("    if (target_class == NULL) goto done;\n");
            out.push_str("    jmethodID request_focus = (*env)->GetMethodID(env, target_class, \"requestFocusFromTouch\", \"()Z\");\n");
            out.push_str("    if (request_focus != NULL) (*env)->CallBooleanMethod(env, target, request_focus);\n");
            out.push_str("done:\n");
            out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
            out.push_str(
                "    if (target_class != NULL) (*env)->DeleteLocalRef(env, target_class);\n",
            );
            out.push_str("    if (target != NULL) (*env)->DeleteLocalRef(env, target);\n");
            out.push_str("    if (finder != NULL) (*env)->DeleteLocalRef(env, finder);\n");
            out.push_str("    if (focus_finder_class != NULL) (*env)->DeleteLocalRef(env, focus_finder_class);\n");
            out.push_str("    if (view_group_class != NULL) (*env)->DeleteLocalRef(env, view_group_class);\n");
            out.push_str("    if (root != NULL) (*env)->DeleteLocalRef(env, root);\n");
            out.push_str(
                "    if (activity_class != NULL) (*env)->DeleteLocalRef(env, activity_class);\n",
            );
            out.push_str("    flux__android_release_env(detach);\n");
            out.push_str("}\n");
            if uses_android_focus_first {
                out.push_str("static inline void flux__android_focus_first(void) { flux__android_focus_edge(2); }\n");
            }
            if uses_android_focus_last {
                out.push_str("static inline void flux__android_focus_last(void) { flux__android_focus_edge(1); }\n");
            }
        }
    }
    if uses_android_text_selection {
        out.push_str("static jobject flux__android_current_edit_text(JNIEnv *env) {\n");
        out.push_str("    if (flux__android_activity == NULL) return NULL;\n");
        out.push_str("    jobject activity = flux__android_activity->clazz;\n");
        out.push_str("    jclass activity_class = (*env)->GetObjectClass(env, activity);\n");
        out.push_str("    if (activity_class == NULL) return NULL;\n");
        out.push_str("    jmethodID get_focus = (*env)->GetMethodID(env, activity_class, \"getCurrentFocus\", \"()Landroid/view/View;\");\n");
        out.push_str("    if (get_focus == NULL) { (*env)->DeleteLocalRef(env, activity_class); return NULL; }\n");
        out.push_str("    jobject view = (*env)->CallObjectMethod(env, activity, get_focus);\n");
        out.push_str("    (*env)->DeleteLocalRef(env, activity_class);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || view == NULL) { if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env); return NULL; }\n");
        out.push_str(
            "    jclass edit_text_class = (*env)->FindClass(env, \"android/widget/EditText\");\n",
        );
        out.push_str("    if (edit_text_class == NULL) { if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env); (*env)->DeleteLocalRef(env, view); return NULL; }\n");
        out.push_str(
            "    jboolean is_edit_text = (*env)->IsInstanceOf(env, view, edit_text_class);\n",
        );
        out.push_str("    (*env)->DeleteLocalRef(env, edit_text_class);\n");
        out.push_str(
            "    if (!is_edit_text) { (*env)->DeleteLocalRef(env, view); return NULL; }\n",
        );
        out.push_str("    return view;\n");
        out.push_str("}\n");
        if uses_android_selection_start || uses_android_selection_end {
            out.push_str("static int64_t flux__android_selection_position(bool end) {\n");
            out.push_str("    bool detach = false;\n");
            out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
            out.push_str("    if (env == NULL) return INT64_C(-1);\n");
            out.push_str("    jobject view = flux__android_current_edit_text(env);\n");
            out.push_str("    if (view == NULL) { flux__android_release_env(detach); return INT64_C(-1); }\n");
            out.push_str("    jclass view_class = (*env)->GetObjectClass(env, view);\n");
            out.push_str("    jint result = -1;\n");
            out.push_str("    if (view_class != NULL) {\n");
            out.push_str("        jmethodID get_position = (*env)->GetMethodID(env, view_class, end ? \"getSelectionEnd\" : \"getSelectionStart\", \"()I\");\n");
            out.push_str("        if (get_position != NULL) result = (*env)->CallIntMethod(env, view, get_position);\n");
            out.push_str("    }\n");
            out.push_str("    if ((*env)->ExceptionCheck(env)) { (*env)->ExceptionClear(env); result = -1; }\n");
            out.push_str("    if (view_class != NULL) (*env)->DeleteLocalRef(env, view_class);\n");
            out.push_str("    (*env)->DeleteLocalRef(env, view);\n");
            out.push_str("    flux__android_release_env(detach);\n");
            out.push_str("    return (int64_t)result;\n");
            out.push_str("}\n");
            if uses_android_selection_start {
                out.push_str("static inline int64_t flux__android_selection_start(void) { return flux__android_selection_position(false); }\n");
            }
            if uses_android_selection_end {
                out.push_str("static inline int64_t flux__android_selection_end(void) { return flux__android_selection_position(true); }\n");
            }
        }
        if uses_android_set_caret || uses_android_set_selection {
            out.push_str("static bool flux__android_set_selection(int64_t start, int64_t end) {\n");
            out.push_str("    if (start < 0 || end < start || start > INT32_MAX || end > INT32_MAX) return false;\n");
            out.push_str("    bool detach = false;\n");
            out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
            out.push_str("    if (env == NULL) return false;\n");
            out.push_str("    jobject view = flux__android_current_edit_text(env);\n");
            out.push_str(
                "    if (view == NULL) { flux__android_release_env(detach); return false; }\n",
            );
            out.push_str("    jclass view_class = (*env)->GetObjectClass(env, view);\n");
            out.push_str("    bool applied = false;\n");
            out.push_str("    if (view_class != NULL) {\n");
            out.push_str("        jmethodID length = (*env)->GetMethodID(env, view_class, \"length\", \"()I\");\n");
            out.push_str("        jmethodID set_selection = (*env)->GetMethodID(env, view_class, \"setSelection\", \"(II)V\");\n");
            out.push_str("        if (length != NULL && set_selection != NULL) {\n");
            out.push_str(
                "            jint text_length = (*env)->CallIntMethod(env, view, length);\n",
            );
            out.push_str(
                "            if (!(*env)->ExceptionCheck(env) && end <= (int64_t)text_length) {\n",
            );
            out.push_str("                (*env)->CallVoidMethod(env, view, set_selection, (jint)start, (jint)end);\n");
            out.push_str("                applied = !(*env)->ExceptionCheck(env);\n");
            out.push_str("            }\n");
            out.push_str("        }\n");
            out.push_str("    }\n");
            out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
            out.push_str("    if (view_class != NULL) (*env)->DeleteLocalRef(env, view_class);\n");
            out.push_str("    (*env)->DeleteLocalRef(env, view);\n");
            out.push_str("    flux__android_release_env(detach);\n");
            out.push_str("    return applied;\n");
            out.push_str("}\n");
            if uses_android_set_caret {
                out.push_str("static inline bool flux__android_set_caret(int64_t position) { return flux__android_set_selection(position, position); }\n");
            }
        }
    }
    if uses_android_share {
        out.push_str("static void flux__android_share(const char *text) {\n");
        out.push_str("    if (text == NULL || flux__android_activity == NULL) return;\n");
        out.push_str("    bool detach = false;\n");
        out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
        out.push_str("    if (env == NULL) return;\n");
        out.push_str("    jclass intent_class = NULL; jclass activity_class = NULL;\n");
        out.push_str("    jstring action = NULL; jstring mime = NULL; jstring extra_key = NULL; jstring text_string = NULL;\n");
        out.push_str(
            "    jobject intent = NULL; jobject chooser = NULL; jobject chained = NULL;\n",
        );
        out.push_str("    intent_class = (*env)->FindClass(env, \"android/content/Intent\");\n");
        out.push_str("    if (intent_class == NULL) goto done;\n");
        out.push_str("    jmethodID ctor = (*env)->GetMethodID(env, intent_class, \"<init>\", \"(Ljava/lang/String;)V\");\n");
        out.push_str("    if (ctor == NULL) goto done;\n");
        out.push_str("    action = (*env)->NewStringUTF(env, \"android.intent.action.SEND\");\n");
        out.push_str("    if (action == NULL) goto done;\n");
        out.push_str("    intent = (*env)->NewObject(env, intent_class, ctor, action);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || intent == NULL) goto done;\n");
        out.push_str("    jmethodID set_type = (*env)->GetMethodID(env, intent_class, \"setType\", \"(Ljava/lang/String;)Landroid/content/Intent;\");\n");
        out.push_str("    if (set_type == NULL) goto done;\n");
        out.push_str("    mime = (*env)->NewStringUTF(env, \"text/plain\");\n");
        out.push_str("    if (mime == NULL) goto done;\n");
        out.push_str("    chained = (*env)->CallObjectMethod(env, intent, set_type, mime);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str(
            "    if (chained != NULL) { (*env)->DeleteLocalRef(env, chained); chained = NULL; }\n",
        );
        out.push_str("    jmethodID put_extra = (*env)->GetMethodID(env, intent_class, \"putExtra\", \"(Ljava/lang/String;Ljava/lang/CharSequence;)Landroid/content/Intent;\");\n");
        out.push_str("    if (put_extra == NULL) goto done;\n");
        out.push_str("    extra_key = (*env)->NewStringUTF(env, \"android.intent.extra.TEXT\");\n");
        out.push_str("    text_string = flux__android_utf8_string(env, text);\n");
        out.push_str("    if (extra_key == NULL || text_string == NULL) goto done;\n");
        out.push_str("    chained = (*env)->CallObjectMethod(env, intent, put_extra, extra_key, text_string);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str(
            "    if (chained != NULL) { (*env)->DeleteLocalRef(env, chained); chained = NULL; }\n",
        );
        out.push_str("    jmethodID create_chooser = (*env)->GetStaticMethodID(env, intent_class, \"createChooser\", \"(Landroid/content/Intent;Ljava/lang/CharSequence;)Landroid/content/Intent;\");\n");
        out.push_str("    if (create_chooser == NULL) goto done;\n");
        out.push_str("    chooser = (*env)->CallStaticObjectMethod(env, intent_class, create_chooser, intent, NULL);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || chooser == NULL) goto done;\n");
        out.push_str(
            "    activity_class = (*env)->GetObjectClass(env, flux__android_activity->clazz);\n",
        );
        out.push_str("    if (activity_class == NULL) goto done;\n");
        out.push_str("    jmethodID start_activity = (*env)->GetMethodID(env, activity_class, \"startActivity\", \"(Landroid/content/Intent;)V\");\n");
        out.push_str("    if (start_activity == NULL) goto done;\n");
        out.push_str("    (*env)->CallVoidMethod(env, flux__android_activity->clazz, start_activity, chooser);\n");
        out.push_str("done:\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
        out.push_str("    if (chained != NULL) (*env)->DeleteLocalRef(env, chained);\n");
        out.push_str(
            "    if (activity_class != NULL) (*env)->DeleteLocalRef(env, activity_class);\n",
        );
        out.push_str("    if (chooser != NULL) (*env)->DeleteLocalRef(env, chooser);\n");
        out.push_str("    if (intent != NULL) (*env)->DeleteLocalRef(env, intent);\n");
        out.push_str("    if (text_string != NULL) (*env)->DeleteLocalRef(env, text_string);\n");
        out.push_str("    if (extra_key != NULL) (*env)->DeleteLocalRef(env, extra_key);\n");
        out.push_str("    if (mime != NULL) (*env)->DeleteLocalRef(env, mime);\n");
        out.push_str("    if (action != NULL) (*env)->DeleteLocalRef(env, action);\n");
        out.push_str("    if (intent_class != NULL) (*env)->DeleteLocalRef(env, intent_class);\n");
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

    if uses_android_create_notification_channel {
        out.push_str("static void flux__android_create_notification_channel(const char *channel_id, const char *name, const char *description) {\n");
        out.push_str("    if (android_get_device_api_level() < 26 || channel_id == NULL || name == NULL || description == NULL || flux__android_activity == NULL) return;\n");
        out.push_str("    bool detach = false;\n");
        out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
        out.push_str("    if (env == NULL) return;\n");
        out.push_str("    jclass activity_class = NULL; jclass channel_class = NULL; jclass manager_class = NULL;\n");
        out.push_str("    jstring service_name = NULL; jstring id_string = NULL; jstring name_string = NULL; jstring description_string = NULL;\n");
        out.push_str("    jobject manager = NULL; jobject channel = NULL;\n");
        out.push_str(
            "    activity_class = (*env)->GetObjectClass(env, flux__android_activity->clazz);\n",
        );
        out.push_str("    if (activity_class == NULL) goto done;\n");
        out.push_str("    jmethodID get_service = (*env)->GetMethodID(env, activity_class, \"getSystemService\", \"(Ljava/lang/String;)Ljava/lang/Object;\");\n");
        out.push_str("    if (get_service == NULL) goto done;\n");
        out.push_str("    service_name = (*env)->NewStringUTF(env, \"notification\");\n");
        out.push_str("    if (service_name == NULL) goto done;\n");
        out.push_str("    manager = (*env)->CallObjectMethod(env, flux__android_activity->clazz, get_service, service_name);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || manager == NULL) goto done;\n");
        out.push_str(
            "    channel_class = (*env)->FindClass(env, \"android/app/NotificationChannel\");\n",
        );
        out.push_str("    if (channel_class == NULL) goto done;\n");
        out.push_str("    jmethodID ctor = (*env)->GetMethodID(env, channel_class, \"<init>\", \"(Ljava/lang/String;Ljava/lang/CharSequence;I)V\");\n");
        out.push_str("    if (ctor == NULL) goto done;\n");
        out.push_str("    id_string = flux__android_utf8_string(env, channel_id);\n");
        out.push_str("    name_string = flux__android_utf8_string(env, name);\n");
        out.push_str("    description_string = flux__android_utf8_string(env, description);\n");
        out.push_str("    if (id_string == NULL || name_string == NULL || description_string == NULL) goto done;\n");
        out.push_str("    channel = (*env)->NewObject(env, channel_class, ctor, id_string, name_string, (jint)3);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || channel == NULL) goto done;\n");
        out.push_str("    jmethodID set_description = (*env)->GetMethodID(env, channel_class, \"setDescription\", \"(Ljava/lang/String;)V\");\n");
        out.push_str("    if (set_description != NULL) (*env)->CallVoidMethod(env, channel, set_description, description_string);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str("    manager_class = (*env)->GetObjectClass(env, manager);\n");
        out.push_str("    if (manager_class == NULL) goto done;\n");
        out.push_str("    jmethodID create_channel = (*env)->GetMethodID(env, manager_class, \"createNotificationChannel\", \"(Landroid/app/NotificationChannel;)V\");\n");
        out.push_str("    if (create_channel != NULL) (*env)->CallVoidMethod(env, manager, create_channel, channel);\n");
        out.push_str("done:\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
        out.push_str("    if (channel != NULL) (*env)->DeleteLocalRef(env, channel);\n");
        out.push_str("    if (description_string != NULL) (*env)->DeleteLocalRef(env, description_string);\n");
        out.push_str("    if (name_string != NULL) (*env)->DeleteLocalRef(env, name_string);\n");
        out.push_str("    if (id_string != NULL) (*env)->DeleteLocalRef(env, id_string);\n");
        out.push_str(
            "    if (manager_class != NULL) (*env)->DeleteLocalRef(env, manager_class);\n",
        );
        out.push_str(
            "    if (channel_class != NULL) (*env)->DeleteLocalRef(env, channel_class);\n",
        );
        out.push_str("    if (manager != NULL) (*env)->DeleteLocalRef(env, manager);\n");
        out.push_str("    if (service_name != NULL) (*env)->DeleteLocalRef(env, service_name);\n");
        out.push_str(
            "    if (activity_class != NULL) (*env)->DeleteLocalRef(env, activity_class);\n",
        );
        out.push_str("    flux__android_release_env(detach);\n");
        out.push_str("}\n");
    }
    if uses_android_permission_granted {
        out.push_str(
            "static bool flux__android_permission_granted(const char *permission_name) {\n",
        );
        out.push_str("    if (android_get_device_api_level() < 23) return true;\n");
        out.push_str(
            "    if (permission_name == NULL || flux__android_activity == NULL) return false;\n",
        );
        out.push_str("    bool detach = false;\n");
        out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
        out.push_str("    if (env == NULL) return false;\n");
        out.push_str("    bool granted = false;\n");
        out.push_str("    jclass activity_class = (*env)->GetObjectClass(env, flux__android_activity->clazz);\n");
        out.push_str("    jstring permission = NULL;\n");
        out.push_str("    if (activity_class == NULL) goto done;\n");
        out.push_str("    jmethodID check_permission = (*env)->GetMethodID(env, activity_class, \"checkSelfPermission\", \"(Ljava/lang/String;)I\");\n");
        out.push_str("    if (check_permission == NULL) goto done;\n");
        out.push_str("    permission = flux__android_utf8_string(env, permission_name);\n");
        out.push_str("    if (permission == NULL) goto done;\n");
        out.push_str("    jint result = (*env)->CallIntMethod(env, flux__android_activity->clazz, check_permission, permission);\n");
        out.push_str("    if (!(*env)->ExceptionCheck(env)) granted = result == 0;\n");
        out.push_str("done:\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
        out.push_str("    if (permission != NULL) (*env)->DeleteLocalRef(env, permission);\n");
        out.push_str(
            "    if (activity_class != NULL) (*env)->DeleteLocalRef(env, activity_class);\n",
        );
        out.push_str("    flux__android_release_env(detach);\n");
        out.push_str("    return granted;\n");
        out.push_str("}\n");
    }
    if uses_android_request_permission {
        out.push_str(
            "static void flux__android_request_permission(const char *permission_name) {\n",
        );
        out.push_str("    if (android_get_device_api_level() < 23 || permission_name == NULL || flux__android_activity == NULL) return;\n");
        out.push_str("    bool detach = false;\n");
        out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
        out.push_str("    if (env == NULL) return;\n");
        out.push_str("    jclass activity_class = NULL; jclass string_class = NULL;\n");
        out.push_str("    jstring permission = NULL; jobjectArray permissions = NULL;\n");
        out.push_str(
            "    activity_class = (*env)->GetObjectClass(env, flux__android_activity->clazz);\n",
        );
        out.push_str("    if (activity_class == NULL) goto done;\n");
        out.push_str("    jmethodID request_permissions = (*env)->GetMethodID(env, activity_class, \"requestPermissions\", \"([Ljava/lang/String;I)V\");\n");
        out.push_str("    if (request_permissions == NULL) goto done;\n");
        out.push_str("    string_class = (*env)->FindClass(env, \"java/lang/String\");\n");
        out.push_str("    if (string_class == NULL) goto done;\n");
        out.push_str("    permission = flux__android_utf8_string(env, permission_name);\n");
        out.push_str("    if (permission == NULL) goto done;\n");
        out.push_str("    permissions = (*env)->NewObjectArray(env, 1, string_class, NULL);\n");
        out.push_str("    if (permissions == NULL) goto done;\n");
        out.push_str("    (*env)->SetObjectArrayElement(env, permissions, 0, permission);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str("    (*env)->CallVoidMethod(env, flux__android_activity->clazz, request_permissions, permissions, (jint)6172);\n");
        out.push_str("done:\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
        out.push_str("    if (permissions != NULL) (*env)->DeleteLocalRef(env, permissions);\n");
        out.push_str("    if (permission != NULL) (*env)->DeleteLocalRef(env, permission);\n");
        out.push_str("    if (string_class != NULL) (*env)->DeleteLocalRef(env, string_class);\n");
        out.push_str(
            "    if (activity_class != NULL) (*env)->DeleteLocalRef(env, activity_class);\n",
        );
        out.push_str("    flux__android_release_env(detach);\n");
        out.push_str("}\n");
    }
    if uses_android_notification_permission_granted
        || uses_android_notify
        || uses_android_notify_url_action
    {
        out.push_str("static bool flux__android_notification_permission_granted(void) {\n");
        out.push_str("    if (android_get_device_api_level() < 33) return true;\n");
        out.push_str("    if (flux__android_activity == NULL) return false;\n");
        out.push_str("    bool detach = false;\n");
        out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
        out.push_str("    if (env == NULL) return false;\n");
        out.push_str("    bool granted = false;\n");
        out.push_str("    jclass activity_class = (*env)->GetObjectClass(env, flux__android_activity->clazz);\n");
        out.push_str("    jstring permission = NULL;\n");
        out.push_str("    if (activity_class == NULL) goto done;\n");
        out.push_str("    jmethodID check_permission = (*env)->GetMethodID(env, activity_class, \"checkSelfPermission\", \"(Ljava/lang/String;)I\");\n");
        out.push_str("    if (check_permission == NULL) goto done;\n");
        out.push_str("    permission = (*env)->NewStringUTF(env, \"android.permission.POST_NOTIFICATIONS\");\n");
        out.push_str("    if (permission == NULL) goto done;\n");
        out.push_str("    jint result = (*env)->CallIntMethod(env, flux__android_activity->clazz, check_permission, permission);\n");
        out.push_str("    if (!(*env)->ExceptionCheck(env)) granted = result == 0;\n");
        out.push_str("done:\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
        out.push_str("    if (permission != NULL) (*env)->DeleteLocalRef(env, permission);\n");
        out.push_str(
            "    if (activity_class != NULL) (*env)->DeleteLocalRef(env, activity_class);\n",
        );
        out.push_str("    flux__android_release_env(detach);\n");
        out.push_str("    return granted;\n");
        out.push_str("}\n");
    }
    if uses_android_request_notification_permission {
        out.push_str("static void flux__android_request_notification_permission(void) {\n");
        out.push_str("    if (android_get_device_api_level() < 33 || flux__android_activity == NULL) return;\n");
        out.push_str("    bool detach = false;\n");
        out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
        out.push_str("    if (env == NULL) return;\n");
        out.push_str("    jclass activity_class = NULL; jclass string_class = NULL;\n");
        out.push_str("    jstring permission = NULL; jobjectArray permissions = NULL;\n");
        out.push_str(
            "    activity_class = (*env)->GetObjectClass(env, flux__android_activity->clazz);\n",
        );
        out.push_str("    if (activity_class == NULL) goto done;\n");
        out.push_str("    jmethodID request_permissions = (*env)->GetMethodID(env, activity_class, \"requestPermissions\", \"([Ljava/lang/String;I)V\");\n");
        out.push_str("    if (request_permissions == NULL) goto done;\n");
        out.push_str("    string_class = (*env)->FindClass(env, \"java/lang/String\");\n");
        out.push_str("    if (string_class == NULL) goto done;\n");
        out.push_str("    permission = (*env)->NewStringUTF(env, \"android.permission.POST_NOTIFICATIONS\");\n");
        out.push_str("    if (permission == NULL) goto done;\n");
        out.push_str("    permissions = (*env)->NewObjectArray(env, 1, string_class, NULL);\n");
        out.push_str("    if (permissions == NULL) goto done;\n");
        out.push_str("    (*env)->SetObjectArrayElement(env, permissions, 0, permission);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str("    (*env)->CallVoidMethod(env, flux__android_activity->clazz, request_permissions, permissions, (jint)6171);\n");
        out.push_str("done:\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
        out.push_str("    if (permissions != NULL) (*env)->DeleteLocalRef(env, permissions);\n");
        out.push_str("    if (permission != NULL) (*env)->DeleteLocalRef(env, permission);\n");
        out.push_str("    if (string_class != NULL) (*env)->DeleteLocalRef(env, string_class);\n");
        out.push_str(
            "    if (activity_class != NULL) (*env)->DeleteLocalRef(env, activity_class);\n",
        );
        out.push_str("    flux__android_release_env(detach);\n");
        out.push_str("}\n");
    }
    if uses_android_notify {
        out.push_str("static void flux__android_notify(const char *channel_id, int64_t notification_id, const char *title, const char *body) {\n");
        out.push_str("    if (channel_id == NULL || title == NULL || body == NULL || flux__android_activity == NULL) return;\n");
        out.push_str(
            "    if (notification_id < INT32_MIN || notification_id > INT32_MAX) return;\n",
        );
        out.push_str("    if (!flux__android_notification_permission_granted()) return;\n");
        out.push_str("    bool detach = false;\n");
        out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
        out.push_str("    if (env == NULL) return;\n");
        out.push_str("    jclass builder_class = NULL; jclass activity_class = NULL; jclass manager_class = NULL;\n");
        out.push_str("    jstring channel_string = NULL; jstring title_string = NULL; jstring body_string = NULL; jstring service_name = NULL;\n");
        out.push_str("    jobject builder = NULL; jobject chained = NULL; jobject notification = NULL; jobject manager = NULL;\n");
        out.push_str(
            "    builder_class = (*env)->FindClass(env, \"android/app/Notification$Builder\");\n",
        );
        out.push_str("    if (builder_class == NULL) goto done;\n");
        out.push_str("    jmethodID ctor = (*env)->GetMethodID(env, builder_class, \"<init>\", \"(Landroid/content/Context;)V\");\n");
        out.push_str("    if (ctor == NULL) goto done;\n");
        out.push_str("    builder = (*env)->NewObject(env, builder_class, ctor, flux__android_activity->clazz);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || builder == NULL) goto done;\n");
        out.push_str("    jmethodID set_small_icon = (*env)->GetMethodID(env, builder_class, \"setSmallIcon\", \"(I)Landroid/app/Notification$Builder;\");\n");
        out.push_str("    if (set_small_icon == NULL) goto done;\n");
        out.push_str("    chained = (*env)->CallObjectMethod(env, builder, set_small_icon, (jint)17301625);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str(
            "    if (chained != NULL) { (*env)->DeleteLocalRef(env, chained); chained = NULL; }\n",
        );
        out.push_str("    title_string = flux__android_utf8_string(env, title);\n");
        out.push_str("    body_string = flux__android_utf8_string(env, body);\n");
        out.push_str("    if (title_string == NULL || body_string == NULL) goto done;\n");
        out.push_str("    jmethodID set_title = (*env)->GetMethodID(env, builder_class, \"setContentTitle\", \"(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;\");\n");
        out.push_str("    jmethodID set_body = (*env)->GetMethodID(env, builder_class, \"setContentText\", \"(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;\");\n");
        out.push_str("    jmethodID set_auto_cancel = (*env)->GetMethodID(env, builder_class, \"setAutoCancel\", \"(Z)Landroid/app/Notification$Builder;\");\n");
        out.push_str("    if (set_title == NULL || set_body == NULL || set_auto_cancel == NULL) goto done;\n");
        out.push_str(
            "    chained = (*env)->CallObjectMethod(env, builder, set_title, title_string);\n",
        );
        out.push_str("    if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str(
            "    if (chained != NULL) { (*env)->DeleteLocalRef(env, chained); chained = NULL; }\n",
        );
        out.push_str(
            "    chained = (*env)->CallObjectMethod(env, builder, set_body, body_string);\n",
        );
        out.push_str("    if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str(
            "    if (chained != NULL) { (*env)->DeleteLocalRef(env, chained); chained = NULL; }\n",
        );
        out.push_str(
            "    chained = (*env)->CallObjectMethod(env, builder, set_auto_cancel, JNI_TRUE);\n",
        );
        out.push_str("    if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str(
            "    if (chained != NULL) { (*env)->DeleteLocalRef(env, chained); chained = NULL; }\n",
        );
        out.push_str("    if (android_get_device_api_level() >= 26) {\n");
        out.push_str("        channel_string = flux__android_utf8_string(env, channel_id);\n");
        out.push_str("        if (channel_string == NULL) goto done;\n");
        out.push_str("        jmethodID set_channel = (*env)->GetMethodID(env, builder_class, \"setChannelId\", \"(Ljava/lang/String;)Landroid/app/Notification$Builder;\");\n");
        out.push_str("        if (set_channel == NULL) goto done;\n");
        out.push_str("        chained = (*env)->CallObjectMethod(env, builder, set_channel, channel_string);\n");
        out.push_str("        if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str("        if (chained != NULL) { (*env)->DeleteLocalRef(env, chained); chained = NULL; }\n");
        out.push_str("    }\n");
        out.push_str("    jmethodID build = (*env)->GetMethodID(env, builder_class, \"build\", \"()Landroid/app/Notification;\");\n");
        out.push_str("    if (build == NULL) goto done;\n");
        out.push_str("    notification = (*env)->CallObjectMethod(env, builder, build);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || notification == NULL) goto done;\n");
        out.push_str(
            "    activity_class = (*env)->GetObjectClass(env, flux__android_activity->clazz);\n",
        );
        out.push_str("    if (activity_class == NULL) goto done;\n");
        out.push_str("    jmethodID get_service = (*env)->GetMethodID(env, activity_class, \"getSystemService\", \"(Ljava/lang/String;)Ljava/lang/Object;\");\n");
        out.push_str("    if (get_service == NULL) goto done;\n");
        out.push_str("    service_name = (*env)->NewStringUTF(env, \"notification\");\n");
        out.push_str("    if (service_name == NULL) goto done;\n");
        out.push_str("    manager = (*env)->CallObjectMethod(env, flux__android_activity->clazz, get_service, service_name);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || manager == NULL) goto done;\n");
        out.push_str("    manager_class = (*env)->GetObjectClass(env, manager);\n");
        out.push_str("    if (manager_class == NULL) goto done;\n");
        out.push_str("    jmethodID notify = (*env)->GetMethodID(env, manager_class, \"notify\", \"(ILandroid/app/Notification;)V\");\n");
        out.push_str("    if (notify != NULL) (*env)->CallVoidMethod(env, manager, notify, (jint)notification_id, notification);\n");
        out.push_str("done:\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
        out.push_str(
            "    if (manager_class != NULL) (*env)->DeleteLocalRef(env, manager_class);\n",
        );
        out.push_str("    if (manager != NULL) (*env)->DeleteLocalRef(env, manager);\n");
        out.push_str("    if (service_name != NULL) (*env)->DeleteLocalRef(env, service_name);\n");
        out.push_str(
            "    if (activity_class != NULL) (*env)->DeleteLocalRef(env, activity_class);\n",
        );
        out.push_str("    if (notification != NULL) (*env)->DeleteLocalRef(env, notification);\n");
        out.push_str("    if (body_string != NULL) (*env)->DeleteLocalRef(env, body_string);\n");
        out.push_str("    if (title_string != NULL) (*env)->DeleteLocalRef(env, title_string);\n");
        out.push_str(
            "    if (channel_string != NULL) (*env)->DeleteLocalRef(env, channel_string);\n",
        );
        out.push_str("    if (chained != NULL) (*env)->DeleteLocalRef(env, chained);\n");
        out.push_str("    if (builder != NULL) (*env)->DeleteLocalRef(env, builder);\n");
        out.push_str(
            "    if (builder_class != NULL) (*env)->DeleteLocalRef(env, builder_class);\n",
        );
        out.push_str("    flux__android_release_env(detach);\n");
        out.push_str("}\n");
    }

    if uses_android_notify_url_action {
        out.push_str("static void flux__android_notify_url_action(const char *channel_id, int64_t notification_id, const char *title, const char *body, const char *action_label, const char *url) {\n");
        out.push_str("    if (channel_id == NULL || title == NULL || body == NULL || action_label == NULL || url == NULL || flux__android_activity == NULL) return;\n");
        out.push_str(
            "    if (notification_id < INT32_MIN || notification_id > INT32_MAX) return;\n",
        );
        out.push_str("    if (!flux__android_notification_permission_granted()) return;\n");
        out.push_str("    bool detach = false;\n");
        out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
        out.push_str("    if (env == NULL) return;\n");
        out.push_str("    jclass builder_class = NULL; jclass activity_class = NULL; jclass manager_class = NULL; jclass uri_class = NULL; jclass intent_class = NULL; jclass pending_intent_class = NULL;\n");
        out.push_str("    jstring channel_string = NULL; jstring title_string = NULL; jstring body_string = NULL; jstring service_name = NULL; jstring action_label_string = NULL; jstring url_string = NULL; jstring view_action = NULL;\n");
        out.push_str("    jobject builder = NULL; jobject chained = NULL; jobject notification = NULL; jobject manager = NULL; jobject uri = NULL; jobject intent = NULL; jobject pending_intent = NULL;\n");
        out.push_str(
            "    builder_class = (*env)->FindClass(env, \"android/app/Notification$Builder\");\n",
        );
        out.push_str("    if (builder_class == NULL) goto done;\n");
        out.push_str("    jmethodID ctor = (*env)->GetMethodID(env, builder_class, \"<init>\", \"(Landroid/content/Context;)V\");\n");
        out.push_str("    if (ctor == NULL) goto done;\n");
        out.push_str("    builder = (*env)->NewObject(env, builder_class, ctor, flux__android_activity->clazz);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || builder == NULL) goto done;\n");
        out.push_str("    jmethodID set_small_icon = (*env)->GetMethodID(env, builder_class, \"setSmallIcon\", \"(I)Landroid/app/Notification$Builder;\");\n");
        out.push_str("    if (set_small_icon == NULL) goto done;\n");
        out.push_str("    chained = (*env)->CallObjectMethod(env, builder, set_small_icon, (jint)17301625);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str(
            "    if (chained != NULL) { (*env)->DeleteLocalRef(env, chained); chained = NULL; }\n",
        );
        out.push_str("    title_string = flux__android_utf8_string(env, title);\n");
        out.push_str("    body_string = flux__android_utf8_string(env, body);\n");
        out.push_str("    if (title_string == NULL || body_string == NULL) goto done;\n");
        out.push_str("    jmethodID set_title = (*env)->GetMethodID(env, builder_class, \"setContentTitle\", \"(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;\");\n");
        out.push_str("    jmethodID set_body = (*env)->GetMethodID(env, builder_class, \"setContentText\", \"(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;\");\n");
        out.push_str("    jmethodID set_auto_cancel = (*env)->GetMethodID(env, builder_class, \"setAutoCancel\", \"(Z)Landroid/app/Notification$Builder;\");\n");
        out.push_str("    if (set_title == NULL || set_body == NULL || set_auto_cancel == NULL) goto done;\n");
        out.push_str(
            "    chained = (*env)->CallObjectMethod(env, builder, set_title, title_string);\n",
        );
        out.push_str("    if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str(
            "    if (chained != NULL) { (*env)->DeleteLocalRef(env, chained); chained = NULL; }\n",
        );
        out.push_str(
            "    chained = (*env)->CallObjectMethod(env, builder, set_body, body_string);\n",
        );
        out.push_str("    if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str(
            "    if (chained != NULL) { (*env)->DeleteLocalRef(env, chained); chained = NULL; }\n",
        );
        out.push_str(
            "    chained = (*env)->CallObjectMethod(env, builder, set_auto_cancel, JNI_TRUE);\n",
        );
        out.push_str("    if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str(
            "    if (chained != NULL) { (*env)->DeleteLocalRef(env, chained); chained = NULL; }\n",
        );
        out.push_str("    if (android_get_device_api_level() >= 26) {\n");
        out.push_str("        channel_string = flux__android_utf8_string(env, channel_id);\n");
        out.push_str("        if (channel_string == NULL) goto done;\n");
        out.push_str("        jmethodID set_channel = (*env)->GetMethodID(env, builder_class, \"setChannelId\", \"(Ljava/lang/String;)Landroid/app/Notification$Builder;\");\n");
        out.push_str("        if (set_channel == NULL) goto done;\n");
        out.push_str("        chained = (*env)->CallObjectMethod(env, builder, set_channel, channel_string);\n");
        out.push_str("        if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str("        if (chained != NULL) { (*env)->DeleteLocalRef(env, chained); chained = NULL; }\n");
        out.push_str("    }\n");
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
        out.push_str("    jmethodID intent_ctor = (*env)->GetMethodID(env, intent_class, \"<init>\", \"(Ljava/lang/String;Landroid/net/Uri;)V\");\n");
        out.push_str("    if (intent_ctor == NULL) goto done;\n");
        out.push_str(
            "    view_action = (*env)->NewStringUTF(env, \"android.intent.action.VIEW\");\n",
        );
        out.push_str("    if (view_action == NULL) goto done;\n");
        out.push_str(
            "    intent = (*env)->NewObject(env, intent_class, intent_ctor, view_action, uri);\n",
        );
        out.push_str("    if ((*env)->ExceptionCheck(env) || intent == NULL) goto done;\n");
        out.push_str(
            "    pending_intent_class = (*env)->FindClass(env, \"android/app/PendingIntent\");\n",
        );
        out.push_str("    if (pending_intent_class == NULL) goto done;\n");
        out.push_str("    jmethodID get_activity = (*env)->GetStaticMethodID(env, pending_intent_class, \"getActivity\", \"(Landroid/content/Context;ILandroid/content/Intent;I)Landroid/app/PendingIntent;\");\n");
        out.push_str("    if (get_activity == NULL) goto done;\n");
        out.push_str("    jint pending_flags = (jint)0x08000000;\n");
        out.push_str(
            "    if (android_get_device_api_level() >= 23) pending_flags |= (jint)0x04000000;\n",
        );
        out.push_str("    pending_intent = (*env)->CallStaticObjectMethod(env, pending_intent_class, get_activity, flux__android_activity->clazz, (jint)notification_id, intent, pending_flags);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || pending_intent == NULL) goto done;\n");
        out.push_str("    action_label_string = flux__android_utf8_string(env, action_label);\n");
        out.push_str("    if (action_label_string == NULL) goto done;\n");
        out.push_str("    jmethodID add_action = (*env)->GetMethodID(env, builder_class, \"addAction\", \"(ILjava/lang/CharSequence;Landroid/app/PendingIntent;)Landroid/app/Notification$Builder;\");\n");
        out.push_str("    if (add_action == NULL) goto done;\n");
        out.push_str("    chained = (*env)->CallObjectMethod(env, builder, add_action, (jint)0, action_label_string, pending_intent);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) goto done;\n");
        out.push_str(
            "    if (chained != NULL) { (*env)->DeleteLocalRef(env, chained); chained = NULL; }\n",
        );
        out.push_str("    jmethodID build = (*env)->GetMethodID(env, builder_class, \"build\", \"()Landroid/app/Notification;\");\n");
        out.push_str("    if (build == NULL) goto done;\n");
        out.push_str("    notification = (*env)->CallObjectMethod(env, builder, build);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || notification == NULL) goto done;\n");
        out.push_str(
            "    activity_class = (*env)->GetObjectClass(env, flux__android_activity->clazz);\n",
        );
        out.push_str("    if (activity_class == NULL) goto done;\n");
        out.push_str("    jmethodID get_service = (*env)->GetMethodID(env, activity_class, \"getSystemService\", \"(Ljava/lang/String;)Ljava/lang/Object;\");\n");
        out.push_str("    if (get_service == NULL) goto done;\n");
        out.push_str("    service_name = (*env)->NewStringUTF(env, \"notification\");\n");
        out.push_str("    if (service_name == NULL) goto done;\n");
        out.push_str("    manager = (*env)->CallObjectMethod(env, flux__android_activity->clazz, get_service, service_name);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || manager == NULL) goto done;\n");
        out.push_str("    manager_class = (*env)->GetObjectClass(env, manager);\n");
        out.push_str("    if (manager_class == NULL) goto done;\n");
        out.push_str("    jmethodID notify = (*env)->GetMethodID(env, manager_class, \"notify\", \"(ILandroid/app/Notification;)V\");\n");
        out.push_str("    if (notify != NULL) (*env)->CallVoidMethod(env, manager, notify, (jint)notification_id, notification);\n");
        out.push_str("done:\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
        out.push_str(
            "    if (manager_class != NULL) (*env)->DeleteLocalRef(env, manager_class);\n",
        );
        out.push_str("    if (manager != NULL) (*env)->DeleteLocalRef(env, manager);\n");
        out.push_str("    if (service_name != NULL) (*env)->DeleteLocalRef(env, service_name);\n");
        out.push_str(
            "    if (activity_class != NULL) (*env)->DeleteLocalRef(env, activity_class);\n",
        );
        out.push_str("    if (notification != NULL) (*env)->DeleteLocalRef(env, notification);\n");
        out.push_str(
            "    if (pending_intent != NULL) (*env)->DeleteLocalRef(env, pending_intent);\n",
        );
        out.push_str("    if (intent != NULL) (*env)->DeleteLocalRef(env, intent);\n");
        out.push_str("    if (uri != NULL) (*env)->DeleteLocalRef(env, uri);\n");
        out.push_str("    if (view_action != NULL) (*env)->DeleteLocalRef(env, view_action);\n");
        out.push_str("    if (url_string != NULL) (*env)->DeleteLocalRef(env, url_string);\n");
        out.push_str("    if (action_label_string != NULL) (*env)->DeleteLocalRef(env, action_label_string);\n");
        out.push_str("    if (pending_intent_class != NULL) (*env)->DeleteLocalRef(env, pending_intent_class);\n");
        out.push_str("    if (intent_class != NULL) (*env)->DeleteLocalRef(env, intent_class);\n");
        out.push_str("    if (uri_class != NULL) (*env)->DeleteLocalRef(env, uri_class);\n");
        out.push_str("    if (body_string != NULL) (*env)->DeleteLocalRef(env, body_string);\n");
        out.push_str("    if (title_string != NULL) (*env)->DeleteLocalRef(env, title_string);\n");
        out.push_str(
            "    if (channel_string != NULL) (*env)->DeleteLocalRef(env, channel_string);\n",
        );
        out.push_str("    if (chained != NULL) (*env)->DeleteLocalRef(env, chained);\n");
        out.push_str("    if (builder != NULL) (*env)->DeleteLocalRef(env, builder);\n");
        out.push_str(
            "    if (builder_class != NULL) (*env)->DeleteLocalRef(env, builder_class);\n",
        );
        out.push_str("    flux__android_release_env(detach);\n");
        out.push_str("}\n");
    }
    if uses_android_cancel_notification {
        out.push_str("static void flux__android_cancel_notification(int64_t notification_id) {\n");
        out.push_str("    if (notification_id < INT32_MIN || notification_id > INT32_MAX || flux__android_activity == NULL) return;\n");
        out.push_str("    bool detach = false;\n");
        out.push_str("    JNIEnv *env = flux__android_get_env(&detach);\n");
        out.push_str("    if (env == NULL) return;\n");
        out.push_str("    jclass activity_class = NULL; jclass manager_class = NULL;\n");
        out.push_str("    jstring service_name = NULL; jobject manager = NULL;\n");
        out.push_str(
            "    activity_class = (*env)->GetObjectClass(env, flux__android_activity->clazz);\n",
        );
        out.push_str("    if (activity_class == NULL) goto done;\n");
        out.push_str("    jmethodID get_service = (*env)->GetMethodID(env, activity_class, \"getSystemService\", \"(Ljava/lang/String;)Ljava/lang/Object;\");\n");
        out.push_str("    if (get_service == NULL) goto done;\n");
        out.push_str("    service_name = (*env)->NewStringUTF(env, \"notification\");\n");
        out.push_str("    if (service_name == NULL) goto done;\n");
        out.push_str("    manager = (*env)->CallObjectMethod(env, flux__android_activity->clazz, get_service, service_name);\n");
        out.push_str("    if ((*env)->ExceptionCheck(env) || manager == NULL) goto done;\n");
        out.push_str("    manager_class = (*env)->GetObjectClass(env, manager);\n");
        out.push_str("    if (manager_class == NULL) goto done;\n");
        out.push_str("    jmethodID cancel = (*env)->GetMethodID(env, manager_class, \"cancel\", \"(I)V\");\n");
        out.push_str("    if (cancel != NULL) (*env)->CallVoidMethod(env, manager, cancel, (jint)notification_id);\n");
        out.push_str("done:\n");
        out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
        out.push_str(
            "    if (manager_class != NULL) (*env)->DeleteLocalRef(env, manager_class);\n",
        );
        out.push_str("    if (manager != NULL) (*env)->DeleteLocalRef(env, manager);\n");
        out.push_str("    if (service_name != NULL) (*env)->DeleteLocalRef(env, service_name);\n");
        out.push_str(
            "    if (activity_class != NULL) (*env)->DeleteLocalRef(env, activity_class);\n",
        );
        out.push_str("    flux__android_release_env(detach);\n");
        out.push_str("}\n");
    }

    if runtime_usage.contains("flux__process_pid(") {
        out.push_str(
            "static inline int64_t flux__process_pid(void) { return (int64_t)getpid(); }\n",
        );
    }
    if runtime_usage.contains("flux__process_parent_pid(") {
        out.push_str(
            "static inline int64_t flux__process_parent_pid(void) { return (int64_t)getppid(); }\n",
        );
    }
    if runtime_usage.contains("flux__process_has_env(") {
        out.push_str("static inline bool flux__process_has_env(const char *name) { return getenv(name) != NULL; }\n");
    }
    if runtime_usage.contains("flux__process_env(") {
        out.push_str("static inline const char *flux__process_env(const char *name, const char *fallback) { const char *value = getenv(name); return value != NULL ? value : fallback; }\n");
    }
    if runtime_usage.contains("flux__process_termination_requested(") {
        out.push_str("static volatile sig_atomic_t flux__process_termination_flag = 0;\n");
        out.push_str("static void flux__process_termination_handler(int signal_number) { (void)signal_number; flux__process_termination_flag = 1; }\n");
        out.push_str("static inline void flux__process_install_termination_handlers(void) { static bool installed = false; if (installed) return; struct sigaction action; memset(&action, 0, sizeof(action)); action.sa_handler = flux__process_termination_handler; sigemptyset(&action.sa_mask); if (sigaction(SIGINT, &action, NULL) != 0 || sigaction(SIGTERM, &action, NULL) != 0) { fputs(\"Flux runtime error: failed to install process termination handlers\\n\", stderr); abort(); } installed = true; }\n");
        out.push_str("static inline bool flux__process_termination_requested(void) { flux__process_install_termination_handlers(); return flux__process_termination_flag != 0; }\n");
    }
    if runtime_usage.contains("flux__process_exit(") {
        out.push_str("static inline void flux__process_exit(int64_t code) { if (code < 0 || code > 255) { fputs(\"Flux runtime error: process.exit code must be between 0 and 255\\n\", stderr); abort(); } exit((int)code); }\n");
    }

    if uses_locale && !uses_android {
        out.push_str("static inline const char *flux__locale_source(void) { const char *value = getenv(\"LC_ALL\"); if (value == NULL || value[0] == '\\0') value = getenv(\"LC_MESSAGES\"); if (value == NULL || value[0] == '\\0') value = getenv(\"LANG\"); return value != NULL && value[0] != '\\0' ? value : \"C\"; }\n");
        if runtime_usage.contains("flux__locale_language(") {
            out.push_str("static const char *flux__locale_language(void) { static char language[32]; const char *source = flux__locale_source(); if (strcmp(source, \"POSIX\") == 0 || source[0] == 'C' && (source[1] == '\\0' || source[1] == '.')) return \"und\"; size_t index = 0; while (source[index] != '\\0' && source[index] != '_' && source[index] != '-' && source[index] != '.' && source[index] != '@' && index + 1 < sizeof(language)) { language[index] = source[index]; index += 1; } language[index] = '\\0'; return index > 0 ? language : \"und\"; }\n");
        }
        if runtime_usage.contains("flux__locale_region(") {
            out.push_str("static const char *flux__locale_region(void) { static char region[32]; const char *source = flux__locale_source(); if (strcmp(source, \"POSIX\") == 0 || source[0] == 'C' && (source[1] == '\\0' || source[1] == '.')) return \"\"; const char *separator = NULL; for (const char *cursor = source; *cursor != '\\0' && *cursor != '.' && *cursor != '@'; cursor += 1) { if (*cursor == '_' || *cursor == '-') { separator = cursor; break; } } if (separator == NULL) return \"\"; separator += 1; size_t index = 0; while (separator[index] != '\\0' && separator[index] != '_' && separator[index] != '-' && separator[index] != '.' && separator[index] != '@' && index + 1 < sizeof(region)) { region[index] = separator[index]; index += 1; } region[index] = '\\0'; return region; }\n");
        }
    }
    if uses_locale && uses_android {
        out.push_str("static const char *flux__android_locale_component(const char *method_name, const char *fallback, char *buffer, size_t capacity) { if (capacity == 0 || flux__android_activity == NULL) return fallback; bool detach = false; JNIEnv *env = flux__android_get_env(&detach); if (env == NULL) return fallback; jclass locale_class = (*env)->FindClass(env, \"java/util/Locale\"); if (locale_class == NULL) { if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env); flux__android_release_env(detach); return fallback; } jmethodID get_default = (*env)->GetStaticMethodID(env, locale_class, \"getDefault\", \"()Ljava/util/Locale;\"); jobject locale = get_default != NULL ? (*env)->CallStaticObjectMethod(env, locale_class, get_default) : NULL; jmethodID component = (*env)->GetMethodID(env, locale_class, method_name, \"()Ljava/lang/String;\"); jstring value = locale != NULL && component != NULL ? (jstring)(*env)->CallObjectMethod(env, locale, component) : NULL; const char *chars = value != NULL ? (*env)->GetStringUTFChars(env, value, NULL) : NULL; if (chars != NULL) { strncpy(buffer, chars, capacity - 1); buffer[capacity - 1] = '\\0'; (*env)->ReleaseStringUTFChars(env, value, chars); } else { buffer[0] = '\\0'; } if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env); if (value != NULL) (*env)->DeleteLocalRef(env, value); if (locale != NULL) (*env)->DeleteLocalRef(env, locale); (*env)->DeleteLocalRef(env, locale_class); flux__android_release_env(detach); return buffer[0] != '\\0' ? buffer : fallback; }\n");
        if runtime_usage.contains("flux__locale_language(") {
            out.push_str("static const char *flux__locale_language(void) { static char language[32]; return flux__android_locale_component(\"getLanguage\", \"und\", language, sizeof(language)); }\n");
        }
        if runtime_usage.contains("flux__locale_region(") {
            out.push_str("static const char *flux__locale_region(void) { static char region[32]; return flux__android_locale_component(\"getCountry\", \"\", region, sizeof(region)); }\n");
        }
    }

    if runtime_usage.contains("flux__time_unix_millis(")
        || runtime_usage.contains("flux__time_monotonic_millis(")
    {
        out.push_str("static inline int64_t flux__time_clock_millis(clockid_t clock_id) { struct timespec value; if (clock_gettime(clock_id, &value) != 0) { fputs(\"Flux runtime error: clock query failed\\n\", stderr); abort(); } if (value.tv_sec > (time_t)(INT64_MAX / INT64_C(1000)) || value.tv_sec < (time_t)(INT64_MIN / INT64_C(1000))) { fputs(\"Flux runtime error: clock value exceeds i64 milliseconds\\n\", stderr); abort(); } int64_t whole = (int64_t)value.tv_sec * INT64_C(1000); int64_t fraction = (int64_t)(value.tv_nsec / 1000000L); if (__builtin_add_overflow(whole, fraction, &whole)) { fputs(\"Flux runtime error: clock value exceeds i64 milliseconds\\n\", stderr); abort(); } return whole; }\n");
    }
    if runtime_usage.contains("flux__time_unix_millis(") {
        out.push_str("static inline int64_t flux__time_unix_millis(void) { return flux__time_clock_millis(CLOCK_REALTIME); }\n");
    }
    if runtime_usage.contains("flux__time_monotonic_millis(") {
        out.push_str("static inline int64_t flux__time_monotonic_millis(void) { return flux__time_clock_millis(CLOCK_MONOTONIC); }\n");
    }
    if runtime_usage.contains("flux__time_sleep_millis(") {
        out.push_str("static inline void flux__time_sleep_millis(int64_t duration_ms) { if (duration_ms < 0) { fputs(\"Flux runtime error: time.sleepMillis durationMs must be non-negative\\n\", stderr); abort(); } struct timespec remaining = { .tv_sec = (time_t)(duration_ms / INT64_C(1000)), .tv_nsec = (long)((duration_ms % INT64_C(1000)) * INT64_C(1000000)) }; while (nanosleep(&remaining, &remaining) != 0) { if (errno == EINTR) continue; fputs(\"Flux runtime error: sleep failed\\n\", stderr); abort(); } }\n");
    }
    if runtime_usage.contains("flux__time_utc_") {
        out.push_str("static inline int64_t flux__time_utc_part(int64_t unix_ms, int part) { int64_t seconds = unix_ms / INT64_C(1000); int64_t millis = unix_ms % INT64_C(1000); if (millis < 0) { millis += INT64_C(1000); seconds -= INT64_C(1); } time_t native_seconds = (time_t)seconds; if ((int64_t)native_seconds != seconds) { fputs(\"Flux runtime error: UTC timestamp exceeds platform time range\\n\", stderr); abort(); } struct tm value; if (gmtime_r(&native_seconds, &value) == NULL) { fputs(\"Flux runtime error: UTC calendar conversion failed\\n\", stderr); abort(); } switch (part) { case 0: return (int64_t)value.tm_year + INT64_C(1900); case 1: return (int64_t)value.tm_mon + INT64_C(1); case 2: return (int64_t)value.tm_mday; case 3: return (int64_t)value.tm_hour; case 4: return (int64_t)value.tm_min; case 5: return (int64_t)value.tm_sec; case 6: return millis; case 7: return value.tm_wday == 0 ? INT64_C(7) : (int64_t)value.tm_wday; case 8: return (int64_t)value.tm_yday + INT64_C(1); default: fputs(\"Flux runtime error: invalid UTC calendar part\\n\", stderr); abort(); } }\n");
    }

    if runtime_usage.contains("flux__fs_exists(") {
        out.push_str("static inline bool flux__fs_exists(const char *path) { struct stat info; return stat(path, &info) == 0; }\n");
    }
    if runtime_usage.contains("flux__fs_is_file(") {
        out.push_str("static inline bool flux__fs_is_file(const char *path) { struct stat info; return stat(path, &info) == 0 && S_ISREG(info.st_mode); }\n");
    }
    if runtime_usage.contains("flux__fs_is_directory(") {
        out.push_str("static inline bool flux__fs_is_directory(const char *path) { struct stat info; return stat(path, &info) == 0 && S_ISDIR(info.st_mode); }\n");
    }
    if runtime_usage.contains("flux__fs_create_directory(") {
        out.push_str("static inline const char *flux__fs_create_directory(const char *path) { return mkdir(path, 0777) == 0 ? NULL : \"failed to create directory\"; }\n");
    }
    if runtime_usage.contains("flux__fs_remove_file(") {
        out.push_str("static inline const char *flux__fs_remove_file(const char *path) { return unlink(path) == 0 ? NULL : \"failed to remove file\"; }\n");
    }
    if runtime_usage.contains("flux__fs_remove_directory(") {
        out.push_str("static inline const char *flux__fs_remove_directory(const char *path) { return rmdir(path) == 0 ? NULL : \"failed to remove directory\"; }\n");
    }
    if runtime_usage.contains("flux__fs_rename(") {
        out.push_str("static inline const char *flux__fs_rename(const char *source, const char *destination) { return rename(source, destination) == 0 ? NULL : \"failed to rename path\"; }\n");
    }
    if runtime_usage.contains("flux__fs_copy_file(") {
        out.push_str("static inline const char *flux__fs_copy_file(const char *source, const char *destination) { FILE *input = fopen(source, \"rb\"); if (input == NULL) return \"failed to open source file\"; FILE *output = fopen(destination, \"wb\"); if (output == NULL) { fclose(input); return \"failed to open destination file\"; } unsigned char buffer[16384]; const char *failure = NULL; for (;;) { size_t read_count = fread(buffer, 1, sizeof(buffer), input); if (read_count > 0 && fwrite(buffer, 1, read_count, output) != read_count) { failure = \"failed to write destination file\"; break; } if (read_count < sizeof(buffer)) { if (ferror(input)) failure = \"failed to read source file\"; break; } } if (fclose(input) != 0 && failure == NULL) failure = \"failed to close source file\"; if (fclose(output) != 0 && failure == NULL) failure = \"failed to close destination file\"; return failure; }\n");
    }
    if runtime_usage.contains("flux__fs_write_text(")
        || runtime_usage.contains("flux__fs_append_text(")
    {
        out.push_str("static inline const char *flux__fs_write_text_mode(const char *path, const char *text, const char *mode) { FILE *file = fopen(path, mode); if (file == NULL) return \"failed to open file for writing\"; if (fputs(text, file) == EOF) { fclose(file); return \"failed to write file\"; } if (fclose(file) != 0) return \"failed to close file after writing\"; return NULL; }\n");
    }
    if runtime_usage.contains("flux__fs_write_text(") {
        out.push_str("static inline const char *flux__fs_write_text(const char *path, const char *text) { return flux__fs_write_text_mode(path, text, \"wb\"); }\n");
    }
    if runtime_usage.contains("flux__fs_append_text(") {
        out.push_str("static inline const char *flux__fs_append_text(const char *path, const char *text) { return flux__fs_write_text_mode(path, text, \"ab\"); }\n");
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

fn stable_android_element_id(view_name: &str, element_name: &str) -> u32 {
    let mut hash = 2_166_136_261u32;
    for byte in view_name
        .bytes()
        .chain(std::iter::once(0))
        .chain(element_name.bytes())
    {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    let id = hash & 0x00FF_FFFF;
    if id == 0 { 1 } else { id }
}

fn emit_android_native_application(
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
                "app root view was not found during Android codegen",
            )
        })?;

    let accessibility_order = ordered_accessibility_elements(view, signatures)?;
    let mut stable_ids = HashMap::<u32, &str>::new();
    for element in &view.elements {
        let element_id = stable_android_element_id(&view.name, &element.name);
        if let Some(existing) = stable_ids.insert(element_id, &element.name) {
            return Err(diag(
                element.name_span,
                &format!(
                    "Android stable view ID collision between '{existing}' and '{}'; rename one element",
                    element.name
                ),
            ));
        }
        if !matches!(
            element.kind.as_str(),
            "Text" | "Button" | "TextInput" | "Image" | "Toggle" | "Radio"
        ) {
            return Err(diag(
                element.kind_span,
                "bootstrap Android app backend currently renders Text, Button, TextInput, Image, Toggle, and Radio elements",
            ));
        }
    }

    out.push_str("static int64_t flux__ui_window_width = INT64_C(0);\nstatic int64_t flux__ui_window_height = INT64_C(0);\nstatic int64_t flux__ui_display_scale = INT64_C(1);\nstatic float flux__ui_density = 1.0f;\n");
    for state in &view.states {
        let state_name = ui_state_c_name(&state.name);
        match signatures.canonical_type(&state.ty) {
            Type::Bool => {
                let Some(initial) = static_expr_bool(&state.initial, signatures) else {
                    return Err(diag(
                        state.initial.span,
                        "bootstrap Android bool state requires a compile-time bool initial value",
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
                        "bootstrap Android i64 state requires a compile-time integer initial value",
                    ));
                };
                out.push_str(&format!(
                    "static int64_t {state_name} = INT64_C({initial});\n"
                ));
            }
            _ => {
                return Err(diag(
                    state.type_span,
                    "bootstrap Android view state currently supports bool and i64; owned/string/aggregate state remains pending",
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
                    "bootstrap Android derived view values currently support i64, bool, and str",
                ));
            }
        };
        out.push_str(&format!(
            "static {} {derived_name} = {initial};\n",
            c_type(&derived.ty, signatures)
        ));
    }
    out.push('\n');

    out.push_str("JNIEXPORT void JNICALL Java_app_flux_runtime_FluxActivity_nativeBuildUi(JNIEnv *env, jobject activity) {\n");
    out.push_str("    jclass activity_context_class = (*env)->GetObjectClass(env, activity);\n");
    out.push_str("    if (activity_context_class != NULL) {\n");
    out.push_str("        jmethodID get_resources = (*env)->GetMethodID(env, activity_context_class, \"getResources\", \"()Landroid/content/res/Resources;\");\n");
    out.push_str("        if (get_resources != NULL) {\n");
    out.push_str(
        "            jobject resources = (*env)->CallObjectMethod(env, activity, get_resources);\n",
    );
    out.push_str("            if (resources != NULL && !(*env)->ExceptionCheck(env)) {\n");
    out.push_str("                jclass resources_class = (*env)->FindClass(env, \"android/content/res/Resources\");\n");
    out.push_str("                if (resources_class != NULL) {\n");
    out.push_str("                    jmethodID get_metrics = (*env)->GetMethodID(env, resources_class, \"getDisplayMetrics\", \"()Landroid/util/DisplayMetrics;\");\n");
    out.push_str("                    if (get_metrics != NULL) {\n");
    out.push_str("                        jobject metrics = (*env)->CallObjectMethod(env, resources, get_metrics);\n");
    out.push_str(
        "                        if (metrics != NULL && !(*env)->ExceptionCheck(env)) {\n",
    );
    out.push_str("                            jclass metrics_class = (*env)->FindClass(env, \"android/util/DisplayMetrics\");\n");
    out.push_str("                            if (metrics_class != NULL) {\n");
    out.push_str("                                jfieldID width_field = (*env)->GetFieldID(env, metrics_class, \"widthPixels\", \"I\");\n");
    out.push_str("                                jfieldID height_field = (*env)->GetFieldID(env, metrics_class, \"heightPixels\", \"I\");\n");
    out.push_str("                                jfieldID density_field = (*env)->GetFieldID(env, metrics_class, \"density\", \"F\");\n");
    out.push_str("                                if (width_field != NULL && height_field != NULL && density_field != NULL) {\n");
    out.push_str("                                    jint width = (*env)->GetIntField(env, metrics, width_field);\n");
    out.push_str("                                    jint height = (*env)->GetIntField(env, metrics, height_field);\n");
    out.push_str("                                    jfloat density = (*env)->GetFloatField(env, metrics, density_field);\n");
    out.push_str(
        "                                    flux__ui_density = density > 0.0f ? density : 1.0f;\n",
    );
    out.push_str("                                    if (width > 0) flux__ui_window_width = (int64_t)(((float)width / flux__ui_density) + 0.5f);\n");
    out.push_str("                                    if (height > 0) flux__ui_window_height = (int64_t)(((float)height / flux__ui_density) + 0.5f);\n");
    out.push_str("                                    flux__ui_display_scale = flux__ui_density >= 1.0f ? (int64_t)(flux__ui_density + 0.5f) : INT64_C(1);\n");
    out.push_str("                                }\n");
    out.push_str("                                (*env)->DeleteLocalRef(env, metrics_class);\n");
    out.push_str("                            }\n");
    out.push_str("                            (*env)->DeleteLocalRef(env, metrics);\n");
    out.push_str("                        }\n");
    out.push_str("                    }\n");
    out.push_str("                    (*env)->DeleteLocalRef(env, resources_class);\n");
    out.push_str("                }\n");
    out.push_str("                (*env)->DeleteLocalRef(env, resources);\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("        (*env)->DeleteLocalRef(env, activity_context_class);\n");
    out.push_str("    }\n");
    out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
    for derived in &view.derived {
        let value = ui_expr_c(&derived.value, view, signatures)?;
        out.push_str(&format!(
            "    {} = {value};\n",
            ui_derived_c_name(&derived.name)
        ));
    }
    out.push_str(
        "    jclass grid_class = (*env)->FindClass(env, \"android/widget/GridLayout\");\n",
    );
    out.push_str("    if (grid_class == NULL) return;\n");
    out.push_str("    jmethodID grid_ctor = (*env)->GetMethodID(env, grid_class, \"<init>\", \"(Landroid/content/Context;)V\");\n");
    out.push_str("    jmethodID set_columns = (*env)->GetMethodID(env, grid_class, \"setColumnCount\", \"(I)V\");\n");
    out.push_str("    jmethodID set_rows = (*env)->GetMethodID(env, grid_class, \"setRowCount\", \"(I)V\");\n");
    out.push_str("    jmethodID set_padding = (*env)->GetMethodID(env, grid_class, \"setPadding\", \"(IIII)V\");\n");
    out.push_str("    jmethodID set_clip_children = (*env)->GetMethodID(env, grid_class, \"setClipChildren\", \"(Z)V\");\n");
    out.push_str("    jmethodID set_clip_to_padding = (*env)->GetMethodID(env, grid_class, \"setClipToPadding\", \"(Z)V\");\n");
    out.push_str("    jmethodID add_view = (*env)->GetMethodID(env, grid_class, \"addView\", \"(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V\");\n");
    out.push_str("    jmethodID grid_spec = (*env)->GetStaticMethodID(env, grid_class, \"spec\", \"(II)Landroid/widget/GridLayout$Spec;\");\n");
    out.push_str("    jmethodID grid_spec_weight = (*env)->GetStaticMethodID(env, grid_class, \"spec\", \"(IIF)Landroid/widget/GridLayout$Spec;\");\n");
    out.push_str("    if (grid_ctor == NULL || set_columns == NULL || set_rows == NULL || set_padding == NULL || set_clip_children == NULL || set_clip_to_padding == NULL || add_view == NULL || grid_spec == NULL || grid_spec_weight == NULL) return;\n");
    out.push_str("    jobject grid = (*env)->NewObject(env, grid_class, grid_ctor, activity);\n");
    out.push_str("    if (grid == NULL || (*env)->ExceptionCheck(env)) { if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env); return; }\n");
    if let Some(direction) =
        application_metadata_string(application, "layout_direction", signatures)
    {
        match direction.as_str() {
            "ltr" => out.push_str("    jmethodID set_layout_direction = (*env)->GetMethodID(env, grid_class, \"setLayoutDirection\", \"(I)V\");\n    if (set_layout_direction == NULL) return;\n    (*env)->CallVoidMethod(env, grid, set_layout_direction, (jint)0);\n"),
            "rtl" => out.push_str("    jmethodID set_layout_direction = (*env)->GetMethodID(env, grid_class, \"setLayoutDirection\", \"(I)V\");\n    if (set_layout_direction == NULL) return;\n    (*env)->CallVoidMethod(env, grid, set_layout_direction, (jint)1);\n"),
            "system" => {}
            _ => unreachable!("application layoutDirection validated by type checking"),
        }
    }
    out.push_str(&format!(
        "    (*env)->CallVoidMethod(env, grid, set_columns, (jint){});\n",
        view.grid.columns.len().max(1)
    ));
    out.push_str(&format!(
        "    (*env)->CallVoidMethod(env, grid, set_rows, (jint){});\n",
        view.grid.rows.len().max(1)
    ));
    let padding = view.grid.padding.unwrap_or(20);
    out.push_str(&format!(
        "    jint grid_padding = (jint)(INT64_C({padding}) * flux__ui_density);\n    (*env)->CallVoidMethod(env, grid, set_padding, grid_padding, grid_padding, grid_padding, grid_padding);\n"
    ));
    out.push_str("    (*env)->CallVoidMethod(env, grid, set_clip_children, JNI_FALSE);\n");
    out.push_str("    (*env)->CallVoidMethod(env, grid, set_clip_to_padding, JNI_FALSE);\n");
    out.push_str("    jclass root_style_activity_class = (*env)->GetObjectClass(env, activity);\n");
    out.push_str("    if (root_style_activity_class == NULL) return;\n");
    out.push_str("    jmethodID style_root = (*env)->GetMethodID(env, root_style_activity_class, \"styleRoot\", \"(Landroid/view/View;)V\");\n");
    out.push_str("    if (style_root == NULL) return;\n");
    out.push_str("    (*env)->CallVoidMethod(env, activity, style_root, grid);\n");
    out.push_str("    (*env)->DeleteLocalRef(env, root_style_activity_class);\n");
    out.push_str("    jclass params_class = (*env)->FindClass(env, \"android/widget/GridLayout$LayoutParams\");\n");
    out.push_str("    if (params_class == NULL) return;\n");
    out.push_str("    jmethodID params_ctor = (*env)->GetMethodID(env, params_class, \"<init>\", \"()V\");\n");
    out.push_str("    jfieldID row_spec_field = (*env)->GetFieldID(env, params_class, \"rowSpec\", \"Landroid/widget/GridLayout$Spec;\");\n");
    out.push_str("    jfieldID column_spec_field = (*env)->GetFieldID(env, params_class, \"columnSpec\", \"Landroid/widget/GridLayout$Spec;\");\n");
    out.push_str(
        "    jfieldID width_field = (*env)->GetFieldID(env, params_class, \"width\", \"I\");\n",
    );
    out.push_str(
        "    jfieldID height_field = (*env)->GetFieldID(env, params_class, \"height\", \"I\");\n",
    );
    out.push_str("    jmethodID set_margins = (*env)->GetMethodID(env, params_class, \"setMargins\", \"(IIII)V\");\n");
    out.push_str("    if (params_ctor == NULL || row_spec_field == NULL || column_spec_field == NULL || width_field == NULL || height_field == NULL || set_margins == NULL) return;\n");

    for element in &view.elements {
        let element_id = stable_android_element_id(&view.name, &element.name);
        out.push_str("    {\n");
        let class_name = match element.kind.as_str() {
            "Text" => "android/widget/TextView",
            "Button" => "android/widget/Button",
            "TextInput" => "app/flux/runtime/FluxActivity$FluxEditText",
            "Image" => "android/widget/ImageView",
            "Toggle" => "android/widget/CheckBox",
            "Radio" => "android/widget/RadioButton",
            _ => unreachable!("validated Android native control kind"),
        };
        out.push_str(&format!(
            "    jclass child_class = (*env)->FindClass(env, \"{class_name}\");\n"
        ));
        out.push_str("    if (child_class == NULL) return;\n");
        out.push_str("    jmethodID child_ctor = (*env)->GetMethodID(env, child_class, \"<init>\", \"(Landroid/content/Context;)V\");\n");
        out.push_str("    if (child_ctor == NULL) return;\n");
        out.push_str(
            "    jobject child = (*env)->NewObject(env, child_class, child_ctor, activity);\n",
        );
        out.push_str("    if (child == NULL || (*env)->ExceptionCheck(env)) { if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env); return; }\n");
        out.push_str("    jmethodID set_stable_id = (*env)->GetMethodID(env, child_class, \"setId\", \"(I)V\");\n");
        out.push_str("    if (set_stable_id == NULL) return;\n");
        out.push_str(&format!(
            "    (*env)->CallVoidMethod(env, child, set_stable_id, (jint){element_id});\n"
        ));
        if let Some(order_index) = accessibility_order
            .iter()
            .position(|candidate| candidate.name == element.name)
            && order_index > 0
        {
            let previous = accessibility_order[order_index - 1];
            let previous_id = stable_android_element_id(&view.name, &previous.name);
            out.push_str("    jmethodID set_accessibility_traversal_after = (*env)->GetMethodID(env, child_class, \"setAccessibilityTraversalAfter\", \"(I)V\");\n");
            out.push_str("    if (set_accessibility_traversal_after == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, child, set_accessibility_traversal_after, (jint){previous_id});\n"
            ));
        }
        if element.kind == "Image" {
            let source = match view_property(element, "source") {
                Some(property) => ui_expr_c(&property.value, view, signatures)?,
                None => c_string(""),
            };
            let alt = match view_property(element, "alt") {
                Some(property) => ui_expr_c(&property.value, view, signatures)?,
                None => c_string(""),
            };
            let fit = match view_property(element, "fit") {
                Some(property) => {
                    let Some(value) = static_expr_str(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Android Image.fit must be a compile-time string",
                        ));
                    };
                    if !matches!(
                        value.as_str(),
                        "fill" | "contain" | "cover" | "scaleDown" | "scale_down"
                    ) {
                        return Err(diag(
                            property.value.span,
                            "Image.fit must be one of 'fill', 'contain', 'cover', or 'scaleDown'",
                        ));
                    }
                    value
                }
                None => "contain".to_string(),
            };
            let can_shrink = match view_property(element, "can_shrink") {
                Some(property) => ui_expr_c(&property.value, view, signatures)?,
                None => "false".to_string(),
            };
            out.push_str(&format!(
                "    jstring child_image_source = flux__android_utf8_string(env, {source});\n"
            ));
            out.push_str(&format!(
                "    jstring child_image_alt = flux__android_utf8_string(env, {alt});\n"
            ));
            out.push_str(&format!(
                "    jstring child_image_fit = flux__android_utf8_string(env, {});\n",
                c_string(&fit)
            ));
            out.push_str("    if (child_image_source == NULL || child_image_alt == NULL || child_image_fit == NULL) return;\n");
            out.push_str(
                "    jclass image_activity_class = (*env)->GetObjectClass(env, activity);\n",
            );
            out.push_str("    if (image_activity_class == NULL) return;\n");
            out.push_str("    jmethodID configure_image = (*env)->GetMethodID(env, image_activity_class, \"configureImage\", \"(Landroid/widget/ImageView;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Z)V\");\n");
            out.push_str("    if (configure_image == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, activity, configure_image, child, child_image_source, child_image_fit, child_image_alt, (jboolean)({can_shrink}));\n"
            ));
            out.push_str("    (*env)->DeleteLocalRef(env, image_activity_class);\n");
            out.push_str("    (*env)->DeleteLocalRef(env, child_image_source);\n");
            out.push_str("    (*env)->DeleteLocalRef(env, child_image_alt);\n");
            out.push_str("    (*env)->DeleteLocalRef(env, child_image_fit);\n");
        } else {
            let text_property = match element.kind.as_str() {
                "Toggle" | "Radio" => "label",
                _ => "text",
            };
            let text = match view_property(element, text_property) {
                Some(property) => ui_expr_c(&property.value, view, signatures)?,
                None if element.kind == "TextInput" => c_string(""),
                None => c_string(&element.name),
            };
            out.push_str("    jmethodID set_text = (*env)->GetMethodID(env, child_class, \"setText\", \"(Ljava/lang/CharSequence;)V\");\n");
            out.push_str("    if (set_text == NULL) return;\n");
            out.push_str(&format!(
                "    jstring child_text = flux__android_utf8_string(env, {text});\n"
            ));
            out.push_str("    if (child_text == NULL) return;\n");
            if element.kind == "TextInput" {
                out.push_str(
                    "    jclass initial_activity_class = (*env)->GetObjectClass(env, activity);\n",
                );
                out.push_str("    if (initial_activity_class == NULL) return;\n");
                out.push_str("    jmethodID restore_text_input = (*env)->GetMethodID(env, initial_activity_class, \"restoreTextInput\", \"(Landroid/widget/EditText;ILjava/lang/String;)V\");\n");
                out.push_str("    if (restore_text_input == NULL) return;\n");
                out.push_str(&format!(
                    "    (*env)->CallVoidMethod(env, activity, restore_text_input, child, (jint){element_id}, child_text);\n"
                ));
                out.push_str("    if ((*env)->ExceptionCheck(env)) { (*env)->ExceptionClear(env); return; }\n");
                out.push_str("    (*env)->DeleteLocalRef(env, initial_activity_class);\n");
            } else {
                out.push_str("    (*env)->CallVoidMethod(env, child, set_text, child_text);\n");
            }
        }
        if element.kind == "Button" {
            let primary = match view_property(element, "primary") {
                Some(property) => {
                    static_expr_bool(&property.value, signatures).ok_or_else(|| {
                        diag(
                            property.value.span,
                            "bootstrap Android Button.primary must be a compile-time bool value",
                        )
                    })?
                }
                None => false,
            };
            out.push_str(
                "    jclass button_style_activity_class = (*env)->GetObjectClass(env, activity);\n",
            );
            out.push_str("    if (button_style_activity_class == NULL) return;\n");
            out.push_str("    jmethodID style_button = (*env)->GetMethodID(env, button_style_activity_class, \"styleButton\", \"(Landroid/widget/Button;Z)V\");\n");
            out.push_str("    if (style_button == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, activity, style_button, child, {});\n",
                if primary { "JNI_TRUE" } else { "JNI_FALSE" }
            ));
            out.push_str("    (*env)->DeleteLocalRef(env, button_style_activity_class);\n");
        }
        if element.kind == "TextInput" {
            out.push_str(
                "    jclass input_style_activity_class = (*env)->GetObjectClass(env, activity);\n",
            );
            out.push_str("    if (input_style_activity_class == NULL) return;\n");
            out.push_str("    jmethodID style_input = (*env)->GetMethodID(env, input_style_activity_class, \"styleTextInput\", \"(Landroid/widget/EditText;)V\");\n");
            out.push_str("    if (style_input == NULL) return;\n");
            out.push_str("    (*env)->CallVoidMethod(env, activity, style_input, child);\n");
            out.push_str("    (*env)->DeleteLocalRef(env, input_style_activity_class);\n");
        }
        if matches!(element.kind.as_str(), "Toggle" | "Radio") {
            out.push_str(
                "    jclass check_style_activity_class = (*env)->GetObjectClass(env, activity);\n",
            );
            out.push_str("    if (check_style_activity_class == NULL) return;\n");
            out.push_str("    jmethodID style_checkable = (*env)->GetMethodID(env, check_style_activity_class, \"styleCheckable\", \"(Landroid/widget/CompoundButton;)V\");\n");
            out.push_str("    if (style_checkable == NULL) return;\n");
            out.push_str("    (*env)->CallVoidMethod(env, activity, style_checkable, child);\n");
            out.push_str("    (*env)->DeleteLocalRef(env, check_style_activity_class);\n");
        }
        if let Some(property) = view_property(element, "visible") {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str("    jmethodID set_visibility = (*env)->GetMethodID(env, child_class, \"setVisibility\", \"(I)V\");\n");
            out.push_str("    if (set_visibility == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, child, set_visibility, (jint)(({value}) ? 0 : 8));\n"
            ));
        }
        if let Some(property) = view_property(element, "enabled") {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str("    jmethodID set_enabled = (*env)->GetMethodID(env, child_class, \"setEnabled\", \"(Z)V\");\n");
            out.push_str("    if (set_enabled == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, child, set_enabled, (jboolean)({value}));\n"
            ));
        }
        if let Some(min_width) = static_minimum_size(element, "min_width", signatures)? {
            out.push_str("    jmethodID set_min_width = (*env)->GetMethodID(env, child_class, \"setMinimumWidth\", \"(I)V\");\n");
            out.push_str("    if (set_min_width == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, child, set_min_width, (jint)(INT64_C({min_width}) * flux__ui_density));\n"
            ));
        }
        let row_start_for_defaults = element.row.saturating_sub(1) as usize;
        let row_end_for_defaults = row_start_for_defaults + element.row_span as usize;
        let rows_are_explicitly_fixed = view
            .grid
            .rows
            .get(row_start_for_defaults..row_end_for_defaults)
            .is_some_and(|rows| {
                rows.iter()
                    .all(|track| matches!(track, crate::ast::GridTrack::Units(_)))
            });
        let min_height = static_minimum_size(element, "min_height", signatures)?.or_else(|| {
            (matches!(
                element.kind.as_str(),
                "Button" | "TextInput" | "Toggle" | "Radio"
            ) && !rows_are_explicitly_fixed)
                .then_some(48)
        });
        if let Some(min_height) = min_height {
            out.push_str("    jmethodID set_min_height = (*env)->GetMethodID(env, child_class, \"setMinimumHeight\", \"(I)V\");\n");
            out.push_str("    if (set_min_height == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, child, set_min_height, (jint)(INT64_C({min_height}) * flux__ui_density));\n"
            ));
        }
        let static_color = |property_name: &str| -> Result<Option<String>, Diagnostic> {
            let Some(property) = view_property(element, property_name) else {
                return Ok(None);
            };
            let Some(value) = static_expr_str(&property.value, signatures) else {
                return Err(diag(
                    property.value.span,
                    &format!("bootstrap Android {property_name} must be a compile-time string"),
                ));
            };
            if !valid_ui_color(&value) {
                return Err(diag(
                    property.value.span,
                    &format!(
                        "{property_name} must use '#RRGGBB', '#RRGGBBAA', or a semantic Flux color token"
                    ),
                ));
            }
            Ok(Some(value))
        };
        let mut background_color = static_color("background_color")?;
        let border_color = static_color("border_color")?;
        let border_top_color = static_color("border_top_color")?;
        let border_bottom_color = static_color("border_bottom_color")?;
        let border_start_color = static_color("border_start_color")?;
        let border_end_color = static_color("border_end_color")?;
        let border_width =
            static_non_negative_style_i64(element, "border_width", signatures)?.unwrap_or(0);
        let border_top_width =
            static_non_negative_style_i64(element, "border_top_width", signatures)?
                .unwrap_or(border_width);
        let border_bottom_width =
            static_non_negative_style_i64(element, "border_bottom_width", signatures)?
                .unwrap_or(border_width);
        let border_start_width =
            static_non_negative_style_i64(element, "border_start_width", signatures)?
                .unwrap_or(border_width);
        let border_end_width =
            static_non_negative_style_i64(element, "border_end_width", signatures)?
                .unwrap_or(border_width);
        let radius = static_non_negative_style_i64(element, "radius", signatures)?.unwrap_or(0);
        let radius_top_left =
            static_non_negative_style_i64(element, "radius_top_left", signatures)?
                .unwrap_or(radius);
        let radius_top_right =
            static_non_negative_style_i64(element, "radius_top_right", signatures)?
                .unwrap_or(radius);
        let radius_bottom_left =
            static_non_negative_style_i64(element, "radius_bottom_left", signatures)?
                .unwrap_or(radius);
        let radius_bottom_right =
            static_non_negative_style_i64(element, "radius_bottom_right", signatures)?
                .unwrap_or(radius);
        let border_style = if let Some(property) = view_property(element, "border_style") {
            let Some(style) = static_expr_str(&property.value, signatures) else {
                return Err(diag(
                    property.value.span,
                    "bootstrap Android border_style must be a compile-time string",
                ));
            };
            if !matches!(
                style.as_str(),
                "none" | "solid" | "dashed" | "dotted" | "double"
            ) {
                return Err(diag(
                    property.value.span,
                    "border_style must be one of 'none', 'solid', 'dashed', 'dotted', or 'double'",
                ));
            }
            style
        } else if border_top_width > 0
            || border_bottom_width > 0
            || border_start_width > 0
            || border_end_width > 0
        {
            "solid".to_string()
        } else {
            "none".to_string()
        };
        let resolve_border_color = |width: i64, side_color: &Option<String>| {
            if width <= 0 || border_style == "none" {
                None
            } else {
                side_color
                    .clone()
                    .or_else(|| border_color.clone())
                    .or_else(|| Some("outline".to_string()))
            }
        };
        let border_top_color = resolve_border_color(border_top_width, &border_top_color);
        let border_bottom_color = resolve_border_color(border_bottom_width, &border_bottom_color);
        let border_start_color = resolve_border_color(border_start_width, &border_start_color);
        let border_end_color = resolve_border_color(border_end_width, &border_end_color);
        let shadow_color = static_color("shadow_color")?;
        let shadow_blur = static_style_i64(element, "shadow_blur", signatures)?.unwrap_or(0);
        let shadow_offset_x =
            static_style_i64(element, "shadow_offset_x", signatures)?.unwrap_or(0);
        let shadow_offset_y =
            static_style_i64(element, "shadow_offset_y", signatures)?.unwrap_or(0);
        if shadow_blur < 0 {
            let span = view_property(element, "shadow_blur")
                .expect("shadow blur exists when negative")
                .value
                .span;
            return Err(diag(span, "shadow_blur must be non-negative"));
        }
        let has_shadow = shadow_color.is_some()
            || shadow_blur != 0
            || shadow_offset_x != 0
            || shadow_offset_y != 0;
        let shadow_color = if has_shadow {
            shadow_color.or_else(|| Some("shadow".to_string()))
        } else {
            None
        };
        let has_shape_style = border_top_color.is_some()
            || border_bottom_color.is_some()
            || border_start_color.is_some()
            || border_end_color.is_some()
            || radius_top_left > 0
            || radius_top_right > 0
            || radius_bottom_left > 0
            || radius_bottom_right > 0
            || has_shadow;
        if element.kind == "Button" && background_color.is_none() && has_shape_style {
            let primary = view_property(element, "primary")
                .and_then(|property| static_expr_bool(&property.value, signatures))
                .unwrap_or(false);
            background_color = Some(if primary { "accent" } else { "surfaceRaised" }.to_string());
        }
        if background_color.is_some()
            || border_top_color.is_some()
            || border_bottom_color.is_some()
            || border_start_color.is_some()
            || border_end_color.is_some()
            || radius_top_left > 0
            || radius_top_right > 0
            || radius_bottom_left > 0
            || radius_bottom_right > 0
            || has_shadow
        {
            let emit_optional_jstring = |out: &mut String, name: &str, value: &Option<String>| {
                if let Some(value) = value.as_ref() {
                    out.push_str(&format!(
                        "    jstring {name} = flux__android_utf8_string(env, {});\n",
                        c_string(value)
                    ));
                    out.push_str(&format!("    if ({name} == NULL) return;\n"));
                } else {
                    out.push_str(&format!("    jstring {name} = NULL;\n"));
                }
            };
            emit_optional_jstring(out, "child_background", &background_color);
            emit_optional_jstring(out, "child_border_top", &border_top_color);
            emit_optional_jstring(out, "child_border_end", &border_end_color);
            emit_optional_jstring(out, "child_border_bottom", &border_bottom_color);
            emit_optional_jstring(out, "child_border_start", &border_start_color);
            let border_style_value = Some(border_style.clone());
            emit_optional_jstring(out, "child_border_style", &border_style_value);
            emit_optional_jstring(out, "child_shadow", &shadow_color);
            out.push_str(
                "    jclass style_activity_class = (*env)->GetObjectClass(env, activity);\n",
            );
            out.push_str("    if (style_activity_class == NULL) return;\n");
            out.push_str("    jmethodID style_view = (*env)->GetMethodID(env, style_activity_class, \"styleView\", \"(Landroid/view/View;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;IIIIFFFFLjava/lang/String;Ljava/lang/String;FFF)V\");\n");
            out.push_str("    if (style_view == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, activity, style_view, child, child_background, child_border_top, child_border_end, child_border_bottom, child_border_start, (jint)(INT64_C({border_top_width}) * flux__ui_density), (jint)(INT64_C({border_end_width}) * flux__ui_density), (jint)(INT64_C({border_bottom_width}) * flux__ui_density), (jint)(INT64_C({border_start_width}) * flux__ui_density), (jfloat)(INT64_C({radius_top_left}) * flux__ui_density), (jfloat)(INT64_C({radius_top_right}) * flux__ui_density), (jfloat)(INT64_C({radius_bottom_right}) * flux__ui_density), (jfloat)(INT64_C({radius_bottom_left}) * flux__ui_density), child_border_style, child_shadow, (jfloat)(INT64_C({shadow_blur}) * flux__ui_density), (jfloat)(INT64_C({shadow_offset_x}) * flux__ui_density), (jfloat)(INT64_C({shadow_offset_y}) * flux__ui_density));\n"
            ));
            out.push_str("    (*env)->DeleteLocalRef(env, style_activity_class);\n");
            for name in [
                "child_background",
                "child_border_top",
                "child_border_end",
                "child_border_bottom",
                "child_border_start",
                "child_border_style",
                "child_shadow",
            ] {
                out.push_str(&format!(
                    "    if ({name} != NULL) (*env)->DeleteLocalRef(env, {name});\n"
                ));
            }
        }
        let padding = static_non_negative_style_i64(element, "padding", signatures)?.unwrap_or(0);
        let padding_top =
            static_non_negative_style_i64(element, "padding_top", signatures)?.unwrap_or(padding);
        let padding_bottom = static_non_negative_style_i64(element, "padding_bottom", signatures)?
            .unwrap_or(padding);
        let padding_start =
            static_non_negative_style_i64(element, "padding_start", signatures)?.unwrap_or(padding);
        let padding_end =
            static_non_negative_style_i64(element, "padding_end", signatures)?.unwrap_or(padding);
        if padding_top > 0 || padding_bottom > 0 || padding_start > 0 || padding_end > 0 {
            out.push_str("    jmethodID set_child_padding = (*env)->GetMethodID(env, child_class, \"setPadding\", \"(IIII)V\");\n");
            out.push_str("    if (set_child_padding == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, child, set_child_padding, (jint)(INT64_C({padding_start}) * flux__ui_density), (jint)(INT64_C({padding_top}) * flux__ui_density), (jint)(INT64_C({padding_end}) * flux__ui_density), (jint)(INT64_C({padding_bottom}) * flux__ui_density));\n"
            ));
        }
        if element_has_transform(element) {
            for property_name in ["skew_x_degrees", "skew_y_degrees"] {
                if let Some(property) = view_property(element, property_name)
                    && static_expr_i64(&property.value, signatures) != Some(0)
                {
                    return Err(diag(
                        property.value.span,
                        &format!(
                            "bootstrap Android {property_name} is not supported yet; translate/rotate/scale transforms are native"
                        ),
                    ));
                }
            }
            let transform_value =
                |property_name: &str, fallback: &str| -> Result<String, Diagnostic> {
                    view_property(element, property_name)
                        .map(|property| ui_expr_c(&property.value, view, signatures))
                        .unwrap_or_else(|| Ok(fallback.to_string()))
                };
            let translate_x = transform_value("translate_x", "0")?;
            let translate_y = transform_value("translate_y", "0")?;
            let rotate_degrees = transform_value("rotate_degrees", "0")?;
            let scale_percent = transform_value("scale_percent", "100")?;
            let scale_x_percent = view_property(element, "scale_x_percent")
                .map(|property| ui_expr_c(&property.value, view, signatures))
                .unwrap_or_else(|| Ok(scale_percent.clone()))?;
            let scale_y_percent = view_property(element, "scale_y_percent")
                .map(|property| ui_expr_c(&property.value, view, signatures))
                .unwrap_or_else(|| Ok(scale_percent.clone()))?;
            let origin_x = transform_value("transform_origin_x_percent", "50")?;
            let origin_y = transform_value("transform_origin_y_percent", "50")?;
            out.push_str(
                "    jclass transform_activity_class = (*env)->GetObjectClass(env, activity);\n",
            );
            out.push_str("    if (transform_activity_class == NULL) return;\n");
            out.push_str("    jmethodID transform_view = (*env)->GetMethodID(env, transform_activity_class, \"transformView\", \"(Landroid/view/View;FFFFFFF)V\");\n");
            out.push_str("    if (transform_view == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, activity, transform_view, child, (jfloat)(({translate_x}) * flux__ui_density), (jfloat)(({translate_y}) * flux__ui_density), (jfloat)({rotate_degrees}), (jfloat)(({scale_x_percent}) / 100.0f), (jfloat)(({scale_y_percent}) / 100.0f), (jfloat)({origin_x}), (jfloat)({origin_y}));\n"
            ));
            out.push_str("    (*env)->DeleteLocalRef(env, transform_activity_class);\n");
        }
        if let Some(property) = view_property(element, "tooltip") {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str(&format!(
                "    jstring child_tooltip = flux__android_utf8_string(env, {value});\n"
            ));
            out.push_str("    if (child_tooltip == NULL) return;\n");
            out.push_str(
                "    jclass tooltip_activity_class = (*env)->GetObjectClass(env, activity);\n",
            );
            out.push_str("    if (tooltip_activity_class == NULL) return;\n");
            out.push_str("    jmethodID set_tooltip = (*env)->GetMethodID(env, tooltip_activity_class, \"setTooltip\", \"(Landroid/view/View;Ljava/lang/String;)V\");\n");
            out.push_str("    if (set_tooltip == NULL) return;\n");
            out.push_str(
                "    (*env)->CallVoidMethod(env, activity, set_tooltip, child, child_tooltip);\n",
            );
            out.push_str("    (*env)->DeleteLocalRef(env, tooltip_activity_class);\n");
            out.push_str("    (*env)->DeleteLocalRef(env, child_tooltip);\n");
        }
        let accessibility_label = view_property(element, "accessibility_label");
        let accessibility_description = view_property(element, "accessibility_description");
        if accessibility_label.is_some() || accessibility_description.is_some() {
            if let Some(property) = accessibility_label {
                let value = ui_expr_c(&property.value, view, signatures)?;
                out.push_str(&format!(
                    "    jstring child_accessibility_label = flux__android_utf8_string(env, {value});\n"
                ));
                out.push_str("    if (child_accessibility_label == NULL) return;\n");
            } else {
                out.push_str("    jstring child_accessibility_label = NULL;\n");
            }
            if let Some(property) = accessibility_description {
                let value = ui_expr_c(&property.value, view, signatures)?;
                out.push_str(&format!(
                    "    jstring child_accessibility_description = flux__android_utf8_string(env, {value});\n"
                ));
                out.push_str("    if (child_accessibility_description == NULL) return;\n");
            } else {
                out.push_str("    jstring child_accessibility_description = NULL;\n");
            }
            out.push_str("    jclass accessibility_activity_class = (*env)->GetObjectClass(env, activity);\n");
            out.push_str("    if (accessibility_activity_class == NULL) return;\n");
            out.push_str("    jmethodID set_accessibility = (*env)->GetMethodID(env, accessibility_activity_class, \"setAccessibility\", \"(Landroid/view/View;Ljava/lang/String;Ljava/lang/String;)V\");\n");
            out.push_str("    if (set_accessibility == NULL) return;\n");
            out.push_str("    (*env)->CallVoidMethod(env, activity, set_accessibility, child, child_accessibility_label, child_accessibility_description);\n");
            out.push_str("    (*env)->DeleteLocalRef(env, accessibility_activity_class);\n");
            out.push_str("    if (child_accessibility_label != NULL) (*env)->DeleteLocalRef(env, child_accessibility_label);\n");
            out.push_str("    if (child_accessibility_description != NULL) (*env)->DeleteLocalRef(env, child_accessibility_description);\n");
        }
        if let Some(property) = view_property(element, "accessibility_hidden") {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str("    jclass accessibility_hidden_activity_class = (*env)->GetObjectClass(env, activity);\n");
            out.push_str("    if (accessibility_hidden_activity_class == NULL) return;\n");
            out.push_str("    jmethodID set_accessibility_hidden = (*env)->GetMethodID(env, accessibility_hidden_activity_class, \"setAccessibilityHidden\", \"(Landroid/view/View;Z)V\");\n");
            out.push_str("    if (set_accessibility_hidden == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, activity, set_accessibility_hidden, child, (jboolean)({value}));\n"
            ));
            out.push_str("    (*env)->DeleteLocalRef(env, accessibility_hidden_activity_class);\n");
        }
        if let Some(property) = view_property(element, "focusable") {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str("    jmethodID set_focusable = (*env)->GetMethodID(env, child_class, \"setFocusable\", \"(Z)V\");\n");
            out.push_str("    jmethodID set_focusable_in_touch_mode = (*env)->GetMethodID(env, child_class, \"setFocusableInTouchMode\", \"(Z)V\");\n");
            out.push_str(
                "    if (set_focusable == NULL || set_focusable_in_touch_mode == NULL) return;\n",
            );
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, child, set_focusable, (jboolean)({value}));\n"
            ));
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, child, set_focusable_in_touch_mode, (jboolean)({value}));\n"
            ));
        }
        if element.kind == "Text" {
            let (default_size, default_bold, default_line_height_percent) =
                text_semantic_typography(element, signatures)?;
            let text_color = view_property(element, "color")
                .map(|property| {
                    let Some(value) = static_expr_str(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Android Text.color must be a compile-time string",
                        ));
                    };
                    if !valid_ui_color(&value) {
                        return Err(diag(
                            property.value.span,
                            "Text.color must use '#RRGGBB', '#RRGGBBAA', or a semantic Flux color token",
                        ));
                    }
                    Ok(value)
                })
                .transpose()?;
            let text_size = view_property(element, "size")
                .map(|property| {
                    let Some(value) = static_expr_i64(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Android Text.size must be a compile-time i64 value",
                        ));
                    };
                    if value <= 0 || value > i64::from(i32::MAX) {
                        return Err(diag(
                            property.value.span,
                            "Text.size must be greater than zero",
                        ));
                    }
                    Ok(value)
                })
                .transpose()?
                .unwrap_or(default_size);
            let text_flag = |name: &str, default: bool| -> Result<bool, Diagnostic> {
                let Some(property) = view_property(element, name) else {
                    return Ok(default);
                };
                static_expr_bool(&property.value, signatures).ok_or_else(|| {
                    diag(
                        property.value.span,
                        &format!("bootstrap Android Text.{name} must be a compile-time bool value"),
                    )
                })
            };
            let bold = text_flag("bold", default_bold)?;
            let italic = text_flag("italic", false)?;
            let underline = text_flag("underline", false)?;
            let strikethrough = text_flag("strikethrough", false)?;
            if text_color.is_some() || text_size > 0 || bold || italic || underline || strikethrough
            {
                if let Some(value) = text_color.as_ref() {
                    out.push_str(&format!(
                        "    jstring child_text_color = flux__android_utf8_string(env, {});\n",
                        c_string(value)
                    ));
                    out.push_str("    if (child_text_color == NULL) return;\n");
                } else {
                    out.push_str("    jstring child_text_color = NULL;\n");
                }
                out.push_str("    jclass text_style_activity_class = (*env)->GetObjectClass(env, activity);\n");
                out.push_str("    if (text_style_activity_class == NULL) return;\n");
                out.push_str("    jmethodID style_text = (*env)->GetMethodID(env, text_style_activity_class, \"styleText\", \"(Landroid/widget/TextView;Ljava/lang/String;FZZZZ)V\");\n");
                out.push_str("    if (style_text == NULL) return;\n");
                out.push_str(&format!(
                    "    (*env)->CallVoidMethod(env, activity, style_text, child, child_text_color, (jfloat){text_size}.0f, (jboolean){bold}, (jboolean){italic}, (jboolean){underline}, (jboolean){strikethrough});\n"
                ));
                out.push_str("    (*env)->DeleteLocalRef(env, text_style_activity_class);\n");
                out.push_str("    if (child_text_color != NULL) (*env)->DeleteLocalRef(env, child_text_color);\n");
            }
            let font_family = view_property(element, "font_family")
                .map(|property| {
                    let Some(value) = static_expr_str(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Android Text.font_family must be a compile-time str value",
                        ));
                    };
                    if value.is_empty() {
                        return Err(diag(
                            property.value.span,
                            "Text.font_family cannot be empty",
                        ));
                    }
                    Ok(value)
                })
                .transpose()?;
            let letter_spacing = view_property(element, "letter_spacing")
                .map(|property| {
                    let Some(value) = static_expr_i64(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Android Text.letter_spacing must be a compile-time i64 value",
                        ));
                    };
                    if !(i64::from(i32::MIN) / 1024..=i64::from(i32::MAX) / 1024).contains(&value) {
                        return Err(diag(
                            property.value.span,
                            "Text.letter_spacing is outside the supported native range",
                        ));
                    }
                    Ok(value)
                })
                .transpose()?;
            let line_height_percent = view_property(element, "line_height_percent")
                .map(|property| {
                    let Some(value) = static_expr_i64(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Android Text.line_height_percent must be a compile-time i64 value",
                        ));
                    };
                    if value <= 0 || value > i64::from(i32::MAX) {
                        return Err(diag(
                            property.value.span,
                            "Text.line_height_percent must be greater than zero and fit within a 32-bit signed integer",
                        ));
                    }
                    Ok(value)
                })
                .transpose()?
                .or(Some(default_line_height_percent));
            let text_align = view_property(element, "text_align")
                .map(|property| {
                    let Some(value) = static_expr_str(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Android Text.text_align must be a compile-time str value",
                        ));
                    };
                    if !matches!(value.as_str(), "left" | "center" | "right" | "fill") {
                        return Err(diag(
                            property.value.span,
                            "Text.text_align must be one of 'left', 'center', 'right', or 'fill'",
                        ));
                    }
                    Ok(value)
                })
                .transpose()?;
            let wrap_mode = view_property(element, "wrap_mode")
                .map(|property| {
                    let Some(value) = static_expr_str(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Android Text.wrap_mode must be a compile-time str value",
                        ));
                    };
                    if !matches!(value.as_str(), "word" | "char" | "wordChar" | "word_char") {
                        return Err(diag(
                            property.value.span,
                            "Text.wrapMode must be one of 'word', 'char', or 'wordChar'",
                        ));
                    }
                    Ok(value)
                })
                .transpose()?;
            let ellipsize = view_property(element, "ellipsize")
                .map(|property| {
                    let Some(value) = static_expr_str(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Android Text.ellipsize must be a compile-time str value",
                        ));
                    };
                    if !matches!(value.as_str(), "none" | "start" | "middle" | "end") {
                        return Err(diag(
                            property.value.span,
                            "Text.ellipsize must be one of 'none', 'start', 'middle', or 'end'",
                        ));
                    }
                    Ok(value)
                })
                .transpose()?;
            let max_lines = view_property(element, "max_lines")
                .map(|property| {
                    let Some(value) = static_expr_i64(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Android Text.max_lines must be a compile-time i64 value",
                        ));
                    };
                    if !(1..=i64::from(i32::MAX)).contains(&value) {
                        return Err(diag(
                            property.value.span,
                            "Text.max_lines must be between 1 and 2147483647",
                        ));
                    }
                    Ok(value)
                })
                .transpose()?;
            if font_family.is_some()
                || letter_spacing.is_some()
                || line_height_percent.is_some()
                || text_align.is_some()
                || wrap_mode.is_some()
                || ellipsize.is_some()
                || max_lines.is_some()
            {
                let emit_optional_text =
                    |out: &mut String, variable: &str, value: Option<&String>| {
                        if let Some(value) = value {
                            out.push_str(&format!(
                                "    jstring {variable} = flux__android_utf8_string(env, {});\n",
                                c_string(value)
                            ));
                            out.push_str(&format!("    if ({variable} == NULL) return;\n"));
                        } else {
                            out.push_str(&format!("    jstring {variable} = NULL;\n"));
                        }
                    };
                emit_optional_text(out, "child_font_family", font_family.as_ref());
                emit_optional_text(out, "child_text_align", text_align.as_ref());
                emit_optional_text(out, "child_wrap_mode", wrap_mode.as_ref());
                emit_optional_text(out, "child_ellipsize", ellipsize.as_ref());
                out.push_str("    jclass text_layout_activity_class = (*env)->GetObjectClass(env, activity);\n");
                out.push_str("    if (text_layout_activity_class == NULL) return;\n");
                out.push_str("    jmethodID style_text_layout = (*env)->GetMethodID(env, text_layout_activity_class, \"styleTextLayout\", \"(Landroid/widget/TextView;Ljava/lang/String;IILjava/lang/String;Ljava/lang/String;Ljava/lang/String;I)V\");\n");
                out.push_str("    if (style_text_layout == NULL) return;\n");
                out.push_str(&format!(
                    "    (*env)->CallVoidMethod(env, activity, style_text_layout, child, child_font_family, (jint){}, (jint){}, child_text_align, child_wrap_mode, child_ellipsize, (jint){});\n",
                    letter_spacing.unwrap_or(i64::from(i32::MIN)),
                    line_height_percent.unwrap_or(0),
                    max_lines.unwrap_or(0),
                ));
                out.push_str("    (*env)->DeleteLocalRef(env, text_layout_activity_class);\n");
                out.push_str("    if (child_font_family != NULL) (*env)->DeleteLocalRef(env, child_font_family);\n");
                out.push_str("    if (child_text_align != NULL) (*env)->DeleteLocalRef(env, child_text_align);\n");
                out.push_str("    if (child_wrap_mode != NULL) (*env)->DeleteLocalRef(env, child_wrap_mode);\n");
                out.push_str("    if (child_ellipsize != NULL) (*env)->DeleteLocalRef(env, child_ellipsize);\n");
            }
            if let Some(property) = view_property(element, "selectable") {
                let value = ui_expr_c(&property.value, view, signatures)?;
                out.push_str("    jmethodID set_selectable = (*env)->GetMethodID(env, child_class, \"setTextIsSelectable\", \"(Z)V\");\n");
                out.push_str("    if (set_selectable == NULL) return;\n");
                out.push_str(&format!(
                    "    (*env)->CallVoidMethod(env, child, set_selectable, (jboolean)({value}));\n"
                ));
            }
            if let Some(property) = view_property(element, "wrap") {
                let value = ui_expr_c(&property.value, view, signatures)?;
                out.push_str("    jmethodID set_single_line = (*env)->GetMethodID(env, child_class, \"setSingleLine\", \"(Z)V\");\n");
                out.push_str("    if (set_single_line == NULL) return;\n");
                out.push_str(&format!(
                    "    (*env)->CallVoidMethod(env, child, set_single_line, (jboolean)(!({value})));\n"
                ));
            }
        }
        if element.kind == "TextInput" {
            let multiline = match view_property(element, "multiline") {
                Some(property) => static_expr_bool(&property.value, signatures).ok_or_else(|| {
                    diag(
                        property.value.span,
                        "bootstrap Android TextInput.multiline must be a compile-time bool value",
                    )
                })?,
                None => false,
            };
            let submit_on_enter = match view_property(element, "submit_on_enter") {
                Some(property) => static_expr_bool(&property.value, signatures).ok_or_else(|| {
                    diag(
                        property.value.span,
                        "bootstrap Android TextInput.submitOnEnter must be a compile-time bool value",
                    )
                })?,
                None => !multiline,
            };
            if let Some(property) = view_property(element, "placeholder") {
                let value = ui_expr_c(&property.value, view, signatures)?;
                out.push_str("    jmethodID set_hint = (*env)->GetMethodID(env, child_class, \"setHint\", \"(Ljava/lang/CharSequence;)V\");\n");
                out.push_str("    if (set_hint == NULL) return;\n");
                out.push_str(&format!(
                    "    jstring child_hint = flux__android_utf8_string(env, {value});\n"
                ));
                out.push_str("    if (child_hint == NULL) return;\n");
                out.push_str("    (*env)->CallVoidMethod(env, child, set_hint, child_hint);\n");
                out.push_str("    (*env)->DeleteLocalRef(env, child_hint);\n");
            }
            let mut android_input_type = None;
            if let Some(property) = view_property(element, "keyboard_type") {
                let Some(keyboard_type) = static_expr_str(&property.value, signatures) else {
                    return Err(diag(
                        property.value.span,
                        "bootstrap Android TextInput.keyboard_type must be a compile-time string",
                    ));
                };
                android_input_type = Some(match keyboard_type.as_str() {
                    "text" => 1,
                    "email" => 33,
                    "number" if multiline => {
                        return Err(diag(
                            property.value.span,
                            "TextInput.multiline is not supported with keyboardType 'number'",
                        ));
                    }
                    "number" => 2,
                    "decimal" if multiline => {
                        return Err(diag(
                            property.value.span,
                            "TextInput.multiline is not supported with keyboardType 'decimal'",
                        ));
                    }
                    "decimal" => 8194,
                    "phone" if multiline => {
                        return Err(diag(
                            property.value.span,
                            "TextInput.multiline is not supported with keyboardType 'phone'",
                        ));
                    }
                    "phone" => 3,
                    "url" => 17,
                    _ => {
                        return Err(diag(
                            property.value.span,
                            "TextInput.keyboard_type must be one of 'text', 'email', 'number', 'decimal', 'phone', or 'url'",
                        ));
                    }
                });
            }
            if let Some(property) = view_property(element, "password") {
                let Some(password) = static_expr_bool(&property.value, signatures) else {
                    return Err(diag(
                        property.value.span,
                        "bootstrap Android TextInput.password must be a compile-time bool value",
                    ));
                };
                if password {
                    if multiline {
                        return Err(diag(
                            property.value.span,
                            "TextInput.password and TextInput.multiline cannot both be true",
                        ));
                    }
                    android_input_type = Some(129);
                }
            }
            if multiline {
                android_input_type = Some(android_input_type.unwrap_or(1) | 131072);
            }
            out.push_str("    jmethodID set_single_line = (*env)->GetMethodID(env, child_class, \"setSingleLine\", \"(Z)V\");\n");
            out.push_str("    if (set_single_line == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, child, set_single_line, (jboolean){});\n",
                if multiline { "false" } else { "true" }
            ));
            if let Some(input_type) = android_input_type {
                out.push_str("    jmethodID set_input_type = (*env)->GetMethodID(env, child_class, \"setInputType\", \"(I)V\");\n");
                out.push_str("    if (set_input_type == NULL) return;\n");
                out.push_str(&format!(
                    "    (*env)->CallVoidMethod(env, child, set_input_type, (jint){input_type});\n"
                ));
            }
            if let Some(property) = view_property(element, "max_length") {
                let Some(max_length) = static_expr_i64(&property.value, signatures) else {
                    return Err(diag(
                        property.value.span,
                        "bootstrap Android TextInput.max_length must be a compile-time i64 value",
                    ));
                };
                if !(0..=i64::from(i32::MAX)).contains(&max_length) {
                    return Err(diag(
                        property.value.span,
                        "TextInput.max_length must be between 0 and 2147483647",
                    ));
                }
                out.push_str(
                    "    jclass max_length_activity_class = (*env)->GetObjectClass(env, activity);\n",
                );
                out.push_str("    if (max_length_activity_class == NULL) return;\n");
                out.push_str("    jmethodID set_max_length = (*env)->GetMethodID(env, max_length_activity_class, \"setMaxLength\", \"(Landroid/widget/EditText;I)V\");\n");
                out.push_str("    if (set_max_length == NULL) return;\n");
                out.push_str(&format!("    (*env)->CallVoidMethod(env, activity, set_max_length, child, (jint){max_length});\n"));
                out.push_str("    (*env)->DeleteLocalRef(env, max_length_activity_class);\n");
            }
            let on_change = view_property(element, "on_change").is_some();
            let on_submit = view_property(element, "on_submit").is_some();
            out.push_str(
                "    jclass wire_activity_class = (*env)->GetObjectClass(env, activity);\n",
            );
            out.push_str("    if (wire_activity_class == NULL) return;\n");
            out.push_str("    jmethodID wire_input = (*env)->GetMethodID(env, wire_activity_class, \"wireTextInput\", \"(Landroid/widget/EditText;ZZZZ)V\");\n");
            out.push_str("    if (wire_input == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, activity, wire_input, child, (jboolean){}, (jboolean){}, (jboolean){}, (jboolean){});\n",
                if on_change { "true" } else { "false" },
                if on_submit { "true" } else { "false" },
                if multiline { "true" } else { "false" },
                if submit_on_enter { "true" } else { "false" }
            ));
            out.push_str("    (*env)->DeleteLocalRef(env, wire_activity_class);\n");
            if let Some(property) = view_property(element, "autofocus") {
                let Some(autofocus) = static_expr_bool(&property.value, signatures) else {
                    return Err(diag(
                        property.value.span,
                        "bootstrap Android TextInput.autofocus must be a compile-time bool value",
                    ));
                };
                if autofocus {
                    out.push_str("    jmethodID request_focus = (*env)->GetMethodID(env, child_class, \"requestFocus\", \"()Z\");\n");
                    out.push_str("    if (request_focus == NULL) return;\n");
                    out.push_str("    (*env)->CallBooleanMethod(env, child, request_focus);\n");
                }
            }
        }
        if matches!(element.kind.as_str(), "Toggle" | "Radio") {
            let checked_property = if element.kind == "Toggle" {
                "checked"
            } else {
                "selected"
            };
            if let Some(property) = view_property(element, checked_property) {
                let value = ui_expr_c(&property.value, view, signatures)?;
                out.push_str("    jmethodID set_checked = (*env)->GetMethodID(env, child_class, \"setChecked\", \"(Z)V\");\n");
                out.push_str("    if (set_checked == NULL) return;\n");
                out.push_str(&format!(
                    "    (*env)->CallVoidMethod(env, child, set_checked, (jboolean)({value}));\n"
                ));
            }
            let action_name = if element.kind == "Toggle" {
                "on_change"
            } else {
                "on_select"
            };
            if view_property(element, action_name).is_some() {
                out.push_str("    jmethodID set_id = (*env)->GetMethodID(env, child_class, \"setId\", \"(I)V\");\n");
                out.push_str("    jmethodID set_checked_listener = (*env)->GetMethodID(env, child_class, \"setOnCheckedChangeListener\", \"(Landroid/widget/CompoundButton$OnCheckedChangeListener;)V\");\n");
                out.push_str("    if (set_id == NULL || set_checked_listener == NULL) return;\n");
                out.push_str(&format!(
                    "    (*env)->CallVoidMethod(env, child, set_id, (jint){element_id});\n"
                ));
                out.push_str(
                    "    (*env)->CallVoidMethod(env, child, set_checked_listener, activity);\n",
                );
            }
        }
        if element.kind == "Button"
            && let Some(action) = view_property(element, "on_press")
        {
            if action.transition.is_none() && !matches!(action.value.kind, ExprKind::Var(_)) {
                return Err(diag(
                    action.value.span,
                    "bootstrap Android Button.on_press requires a named fn() -> void callback or state transition",
                ));
            }
            out.push_str("    jmethodID set_id = (*env)->GetMethodID(env, child_class, \"setId\", \"(I)V\");\n");
            out.push_str("    jmethodID set_click = (*env)->GetMethodID(env, child_class, \"setOnClickListener\", \"(Landroid/view/View$OnClickListener;)V\");\n");
            out.push_str("    if (set_id == NULL || set_click == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, child, set_id, (jint){element_id});\n"
            ));
            out.push_str("    (*env)->CallVoidMethod(env, child, set_click, activity);\n");
        }
        if view_property(element, "on_tap").is_some() {
            out.push_str("    jmethodID set_id = (*env)->GetMethodID(env, child_class, \"setId\", \"(I)V\");\n");
            out.push_str("    jmethodID set_touch_listener = (*env)->GetMethodID(env, child_class, \"setOnTouchListener\", \"(Landroid/view/View$OnTouchListener;)V\");\n");
            out.push_str("    if (set_id == NULL || set_touch_listener == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, child, set_id, (jint){element_id});\n"
            ));
            out.push_str("    (*env)->CallVoidMethod(env, child, set_touch_listener, activity);\n");
        }
        if view_property(element, "on_long_press").is_some() {
            out.push_str("    jmethodID set_id = (*env)->GetMethodID(env, child_class, \"setId\", \"(I)V\");\n");
            out.push_str("    jmethodID set_long_click_listener = (*env)->GetMethodID(env, child_class, \"setOnLongClickListener\", \"(Landroid/view/View$OnLongClickListener;)V\");\n");
            out.push_str("    if (set_id == NULL || set_long_click_listener == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, child, set_id, (jint){element_id});\n"
            ));
            out.push_str(
                "    (*env)->CallVoidMethod(env, child, set_long_click_listener, activity);\n",
            );
        }
        if view_property(element, "on_focus").is_some()
            || view_property(element, "on_blur").is_some()
        {
            out.push_str("    jmethodID set_id = (*env)->GetMethodID(env, child_class, \"setId\", \"(I)V\");\n");
            out.push_str("    jmethodID set_focus_listener = (*env)->GetMethodID(env, child_class, \"setOnFocusChangeListener\", \"(Landroid/view/View$OnFocusChangeListener;)V\");\n");
            out.push_str("    if (set_id == NULL || set_focus_listener == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, child, set_id, (jint){element_id});\n"
            ));
            out.push_str("    (*env)->CallVoidMethod(env, child, set_focus_listener, activity);\n");
        }
        if view_property(element, "on_hover").is_some()
            || view_property(element, "on_leave").is_some()
        {
            out.push_str("    jmethodID set_id = (*env)->GetMethodID(env, child_class, \"setId\", \"(I)V\");\n");
            out.push_str("    jmethodID set_hover_listener = (*env)->GetMethodID(env, child_class, \"setOnHoverListener\", \"(Landroid/view/View$OnHoverListener;)V\");\n");
            out.push_str("    if (set_id == NULL || set_hover_listener == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, child, set_id, (jint){element_id});\n"
            ));
            out.push_str("    (*env)->CallVoidMethod(env, child, set_hover_listener, activity);\n");
        }
        if view_property(element, "on_key").is_some() {
            out.push_str("    jmethodID set_id = (*env)->GetMethodID(env, child_class, \"setId\", \"(I)V\");\n");
            out.push_str("    jmethodID set_key_listener = (*env)->GetMethodID(env, child_class, \"setOnKeyListener\", \"(Landroid/view/View$OnKeyListener;)V\");\n");
            if view_property(element, "focusable").is_none() {
                out.push_str("    jmethodID set_focusable = (*env)->GetMethodID(env, child_class, \"setFocusable\", \"(Z)V\");\n");
                out.push_str("    jmethodID set_focusable_in_touch_mode = (*env)->GetMethodID(env, child_class, \"setFocusableInTouchMode\", \"(Z)V\");\n");
                out.push_str("    if (set_id == NULL || set_focusable == NULL || set_focusable_in_touch_mode == NULL || set_key_listener == NULL) return;\n");
            } else {
                out.push_str("    if (set_id == NULL || set_key_listener == NULL) return;\n");
            }
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, child, set_id, (jint){element_id});\n"
            ));
            if view_property(element, "focusable").is_none() {
                out.push_str(
                    "    (*env)->CallVoidMethod(env, child, set_focusable, (jboolean)true);\n",
                );
                out.push_str("    (*env)->CallVoidMethod(env, child, set_focusable_in_touch_mode, (jboolean)true);\n");
            }
            out.push_str("    (*env)->CallVoidMethod(env, child, set_key_listener, activity);\n");
        }
        out.push_str("    jobject params = (*env)->NewObject(env, params_class, params_ctor);\n");
        out.push_str("    if (params == NULL) return;\n");
        let row_start = element.row.saturating_sub(1) as usize;
        let row_end = row_start + element.row_span as usize;
        let column_start = element.column.saturating_sub(1) as usize;
        let column_end = column_start + element.column_span as usize;
        let row_tracks = &view.grid.rows[row_start..row_end];
        let column_tracks = &view.grid.columns[column_start..column_end];
        let row_weight = row_tracks
            .iter()
            .all(|track| matches!(track, crate::ast::GridTrack::Fraction(_)))
            .then(|| {
                row_tracks
                    .iter()
                    .map(|track| match track {
                        crate::ast::GridTrack::Fraction(value) => *value,
                        _ => 0,
                    })
                    .sum::<u32>()
            });
        let column_weight = column_tracks
            .iter()
            .all(|track| matches!(track, crate::ast::GridTrack::Fraction(_)))
            .then(|| {
                column_tracks
                    .iter()
                    .map(|track| match track {
                        crate::ast::GridTrack::Fraction(value) => *value,
                        _ => 0,
                    })
                    .sum::<u32>()
            });
        if let Some(weight) = row_weight {
            out.push_str(&format!(
                "    jobject row_spec = (*env)->CallStaticObjectMethod(env, grid_class, grid_spec_weight, (jint){row_start}, (jint){}, (jfloat){weight}.0f);\n",
                element.row_span
            ));
        } else {
            out.push_str(&format!(
                "    jobject row_spec = (*env)->CallStaticObjectMethod(env, grid_class, grid_spec, (jint){row_start}, (jint){});\n",
                element.row_span
            ));
        }
        if let Some(weight) = column_weight {
            out.push_str(&format!(
                "    jobject column_spec = (*env)->CallStaticObjectMethod(env, grid_class, grid_spec_weight, (jint){column_start}, (jint){}, (jfloat){weight}.0f);\n",
                element.column_span
            ));
        } else {
            out.push_str(&format!(
                "    jobject column_spec = (*env)->CallStaticObjectMethod(env, grid_class, grid_spec, (jint){column_start}, (jint){});\n",
                element.column_span
            ));
        }
        out.push_str("    if (row_spec == NULL || column_spec == NULL) return;\n");
        out.push_str("    (*env)->SetObjectField(env, params, row_spec_field, row_spec);\n");
        out.push_str("    (*env)->SetObjectField(env, params, column_spec_field, column_spec);\n");
        let gap = view.grid.gap.unwrap_or(12);
        let margin = view_property(element, "margin")
            .map(|property| element_margin_value(property, "margin", signatures))
            .transpose()?
            .unwrap_or(0);
        let margin_top = view_property(element, "margin_top")
            .map(|property| element_margin_value(property, "margin_top", signatures))
            .transpose()?
            .unwrap_or(margin);
        let margin_bottom = view_property(element, "margin_bottom")
            .map(|property| element_margin_value(property, "margin_bottom", signatures))
            .transpose()?
            .unwrap_or(margin);
        let margin_start = view_property(element, "margin_start")
            .map(|property| element_margin_value(property, "margin_start", signatures))
            .transpose()?
            .unwrap_or(margin);
        let margin_end = view_property(element, "margin_end")
            .map(|property| element_margin_value(property, "margin_end", signatures))
            .transpose()?
            .unwrap_or(margin);
        let gap_half = i64::from(gap) / 2;
        if gap > 0 || margin_top > 0 || margin_bottom > 0 || margin_start > 0 || margin_end > 0 {
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, params, set_margins, (jint)(INT64_C({}) * flux__ui_density), (jint)(INT64_C({}) * flux__ui_density), (jint)(INT64_C({}) * flux__ui_density), (jint)(INT64_C({}) * flux__ui_density));\n",
                margin_start.saturating_add(gap_half),
                margin_top.saturating_add(gap_half),
                margin_end.saturating_add(gap_half),
                margin_bottom.saturating_add(gap_half),
            ));
        }
        let alignment = |property_name: &str, horizontal: bool| -> Result<i32, Diagnostic> {
            let Some(property) = view_property(element, property_name) else {
                return Ok(0);
            };
            let Some(value) = static_expr_str(&property.value, signatures) else {
                return Err(diag(
                    property.value.span,
                    &format!("bootstrap Android {property_name} must be a compile-time string"),
                ));
            };
            match (horizontal, value.as_str()) {
                (true, "start") => Ok(8_388_611),
                (true, "center") => Ok(1),
                (true, "end") => Ok(8_388_613),
                (true, "fill") => Ok(7),
                (false, "start") => Ok(48),
                (false, "center") => Ok(16),
                (false, "end") => Ok(80),
                (false, "fill") => Ok(112),
                _ => Err(diag(
                    property.value.span,
                    &format!("{property_name} must be one of 'start', 'center', 'end', or 'fill'"),
                )),
            }
        };
        let gravity = alignment("align_x", true)? | alignment("align_y", false)?;
        if gravity != 0 {
            out.push_str("    jmethodID set_gravity = (*env)->GetMethodID(env, params_class, \"setGravity\", \"(I)V\");\n");
            out.push_str("    if (set_gravity == NULL) return;\n");
            out.push_str(&format!(
                "    (*env)->CallVoidMethod(env, params, set_gravity, (jint){gravity});\n"
            ));
        }
        let fixed_width = column_tracks
            .iter()
            .try_fold(0u32, |total, track| match track {
                crate::ast::GridTrack::Units(value) => total.checked_add(*value),
                _ => None,
            });
        let fixed_height = row_tracks
            .iter()
            .try_fold(0u32, |total, track| match track {
                crate::ast::GridTrack::Units(value) => total.checked_add(*value),
                _ => None,
            });
        if let Some(width) = fixed_width {
            let width =
                width.saturating_add(gap.saturating_mul(element.column_span.saturating_sub(1)));
            out.push_str(&format!(
                "    (*env)->SetIntField(env, params, width_field, (jint)(INT64_C({width}) * flux__ui_density));\n"
            ));
        } else if column_weight.is_some() {
            out.push_str("    (*env)->SetIntField(env, params, width_field, (jint)0);\n");
        }
        if let Some(height) = fixed_height {
            let height =
                height.saturating_add(gap.saturating_mul(element.row_span.saturating_sub(1)));
            out.push_str(&format!(
                "    (*env)->SetIntField(env, params, height_field, (jint)(INT64_C({height}) * flux__ui_density));\n"
            ));
        } else if row_weight.is_some() {
            out.push_str("    (*env)->SetIntField(env, params, height_field, (jint)0);\n");
        }
        out.push_str("    (*env)->CallVoidMethod(env, grid, add_view, child, params);\n");
        out.push_str("    (*env)->DeleteLocalRef(env, column_spec);\n    (*env)->DeleteLocalRef(env, row_spec);\n    (*env)->DeleteLocalRef(env, params);\n");
        if element.kind != "Image" {
            out.push_str("    (*env)->DeleteLocalRef(env, child_text);\n");
        }
        out.push_str("    (*env)->DeleteLocalRef(env, child);\n    (*env)->DeleteLocalRef(env, child_class);\n");
        out.push_str("    }\n");
    }
    if view.grid.scroll.unwrap_or(false) {
        out.push_str(
            "    jclass scroll_class = (*env)->FindClass(env, \"android/widget/ScrollView\");\n",
        );
        out.push_str("    if (scroll_class == NULL) return;\n");
        out.push_str("    jmethodID scroll_ctor = (*env)->GetMethodID(env, scroll_class, \"<init>\", \"(Landroid/content/Context;)V\");\n");
        out.push_str("    jmethodID scroll_add_view = (*env)->GetMethodID(env, scroll_class, \"addView\", \"(Landroid/view/View;)V\");\n");
        out.push_str("    jmethodID scroll_fill_viewport = (*env)->GetMethodID(env, scroll_class, \"setFillViewport\", \"(Z)V\");\n");
        out.push_str("    if (scroll_ctor == NULL || scroll_add_view == NULL || scroll_fill_viewport == NULL) return;\n");
        out.push_str("    jobject content_root = (*env)->NewObject(env, scroll_class, scroll_ctor, activity);\n");
        out.push_str("    if (content_root == NULL || (*env)->ExceptionCheck(env)) { if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env); return; }\n");
        out.push_str("    (*env)->CallVoidMethod(env, content_root, scroll_fill_viewport, (jboolean)true);\n");
        out.push_str("    (*env)->CallVoidMethod(env, content_root, scroll_add_view, grid);\n");
    } else {
        out.push_str("    jobject content_root = grid;\n");
    }
    out.push_str("    jclass activity_class = (*env)->GetObjectClass(env, activity);\n");
    out.push_str("    if (activity_class == NULL) return;\n");
    out.push_str("    jmethodID set_content = (*env)->GetMethodID(env, activity_class, \"setContentView\", \"(Landroid/view/View;)V\");\n");
    out.push_str(
        "    if (set_content != NULL) (*env)->CallVoidMethod(env, activity, set_content, content_root);\n",
    );
    out.push_str("    if ((*env)->ExceptionCheck(env)) (*env)->ExceptionClear(env);\n");
    out.push_str("    (*env)->DeleteLocalRef(env, activity_class);\n");
    if view.grid.scroll.unwrap_or(false) {
        out.push_str("    (*env)->DeleteLocalRef(env, content_root);\n    (*env)->DeleteLocalRef(env, scroll_class);\n");
    }
    out.push_str("    (*env)->DeleteLocalRef(env, params_class);\n    (*env)->DeleteLocalRef(env, grid);\n    (*env)->DeleteLocalRef(env, grid_class);\n");
    out.push_str("}\n");
    emit_android_ui_refresh(out, view, signatures)?;

    out.push_str("JNIEXPORT void JNICALL Java_app_flux_runtime_FluxActivity_nativeOnTap(JNIEnv *env, jclass activity_class, jint view_id) {\n    (void)activity_class;\n    switch (view_id) {\n");
    for element in &view.elements {
        let Some(action) = view_property(element, "on_tap") else {
            continue;
        };
        let element_id = stable_android_element_id(&view.name, &element.name);
        let body = android_ui_zero_arg_event_body(action, view, signatures)?;
        out.push_str(&format!("        case {element_id}: {body} break;\n"));
    }
    out.push_str("        default: break;\n    }\n}\n\n");

    if view
        .elements
        .iter()
        .any(|element| view_property(element, "on_key").is_some())
    {
        out.push_str("JNIEXPORT void JNICALL Java_app_flux_runtime_FluxActivity_nativeOnKey(JNIEnv *env, jclass activity_class, jint view_id, jstring key) {\n    (void)activity_class;\n    if (key == NULL) return;\n    const char *value = (*env)->GetStringUTFChars(env, key, NULL);\n    if (value == NULL) return;\n    switch (view_id) {\n");
        for element in &view.elements {
            let Some(action) = view_property(element, "on_key") else {
                continue;
            };
            let ExprKind::Var(function) = &action.value.kind else {
                return Err(diag(
                    action.value.span,
                    "bootstrap native onKey requires a named fn(str) -> void callback",
                ));
            };
            let element_id = stable_android_element_id(&view.name, &element.name);
            out.push_str(&format!(
                "        case {element_id}: {}(value); break;\n",
                function_c_name(function)
            ));
        }
        out.push_str("        default: break;\n    }\n    (*env)->ReleaseStringUTFChars(env, key, value);\n}\n\n");
    }

    out.push_str("JNIEXPORT void JNICALL Java_app_flux_runtime_FluxActivity_nativeOnLongPress(JNIEnv *env, jclass activity_class, jint view_id) {\n    (void)activity_class;\n    switch (view_id) {\n");
    for element in &view.elements {
        let Some(action) = view_property(element, "on_long_press") else {
            continue;
        };
        let element_id = stable_android_element_id(&view.name, &element.name);
        let body = android_ui_zero_arg_event_body(action, view, signatures)?;
        out.push_str(&format!("        case {element_id}: {body} break;\n"));
    }
    out.push_str("        default: break;\n    }\n}\n\n");

    out.push_str("JNIEXPORT void JNICALL Java_app_flux_runtime_FluxActivity_nativeOnClick(JNIEnv *env, jclass activity_class, jint view_id) {\n    (void)activity_class;\n    switch (view_id) {\n");
    for element in &view.elements {
        if element.kind != "Button" {
            continue;
        }
        let Some(action) = view_property(element, "on_press") else {
            continue;
        };
        let element_id = stable_android_element_id(&view.name, &element.name);
        if let Some(transition) = &action.transition {
            let next = ui_expr_c(&action.value, view, signatures)?;
            let state_index = ui_state_index(view, &transition.state);
            out.push_str(&format!(
                "        case {element_id}: {} = {next}; if (flux__android_activity != NULL) flux__android_ui_refresh(env, flux__android_activity->clazz, {state_index}); break;\n",
                ui_state_c_name(&transition.state)
            ));
            continue;
        }
        let ExprKind::Var(function) = &action.value.kind else {
            return Err(diag(
                action.value.span,
                "bootstrap Android Button.on_press requires a named fn() -> void callback or state transition",
            ));
        };
        out.push_str(&format!(
            "        case {element_id}: {}(); break;\n",
            function_c_name(function)
        ));
    }
    out.push_str("        default: break;\n    }\n}\n\n");

    out.push_str("JNIEXPORT void JNICALL Java_app_flux_runtime_FluxActivity_nativeOnChecked(JNIEnv *env, jclass activity_class, jint view_id, jboolean checked) {\n    (void)activity_class;\n    switch (view_id) {\n");
    for element in &view.elements {
        let action_name = match element.kind.as_str() {
            "Toggle" => "on_change",
            "Radio" => "on_select",
            _ => continue,
        };
        let Some(action) = view_property(element, action_name) else {
            continue;
        };
        let element_id = stable_android_element_id(&view.name, &element.name);
        let radio_guard = if element.kind == "Radio" {
            "if (!checked) break; "
        } else {
            ""
        };
        if let Some(transition) = &action.transition {
            let next = ui_expr_c(&action.value, view, signatures)?;
            let state_index = ui_state_index(view, &transition.state);
            out.push_str(&format!(
                "        case {element_id}: {radio_guard}{} = {next}; if (flux__android_activity != NULL) flux__android_ui_refresh(env, flux__android_activity->clazz, {state_index}); break;\n",
                ui_state_c_name(&transition.state)
            ));
            continue;
        }
        let ExprKind::Var(function) = &action.value.kind else {
            return Err(diag(
                action.value.span,
                "bootstrap Android Toggle/Radio events require a named fn() -> void callback or state transition",
            ));
        };
        out.push_str(&format!(
            "        case {element_id}: {radio_guard}{}(); break;\n",
            function_c_name(function)
        ));
    }
    out.push_str("        default: break;\n    }\n}\n\n");

    for (native_name, positive_property, negative_property) in [
        ("nativeOnFocus", "on_focus", "on_blur"),
        ("nativeOnHover", "on_hover", "on_leave"),
    ] {
        out.push_str(&format!(
            "JNIEXPORT void JNICALL Java_app_flux_runtime_FluxActivity_{native_name}(JNIEnv *env, jclass activity_class, jint view_id, jboolean active) {{\n    (void)activity_class;\n    switch (view_id) {{\n"
        ));
        for element in &view.elements {
            let positive = view_property(element, positive_property);
            let negative = view_property(element, negative_property);
            if positive.is_none() && negative.is_none() {
                continue;
            }
            let element_id = stable_android_element_id(&view.name, &element.name);
            let positive_body = positive
                .map(|action| android_ui_zero_arg_event_body(action, view, signatures))
                .transpose()?
                .unwrap_or_default();
            let negative_body = negative
                .map(|action| android_ui_zero_arg_event_body(action, view, signatures))
                .transpose()?
                .unwrap_or_default();
            out.push_str(&format!(
                "        case {element_id}: if (active) {{ {positive_body} }} else {{ {negative_body} }} break;\n"
            ));
        }
        out.push_str("        default: break;\n    }\n}\n\n");
    }

    for (property_name, native_name) in [
        ("on_change", "nativeOnTextChanged"),
        ("on_submit", "nativeOnSubmit"),
    ] {
        out.push_str(&format!(
            "JNIEXPORT void JNICALL Java_app_flux_runtime_FluxActivity_{native_name}(JNIEnv *env, jclass activity_class, jint view_id, jstring text) {{\n    (void)activity_class;\n    if (text == NULL) return;\n    const char *value = (*env)->GetStringUTFChars(env, text, NULL);\n    if (value == NULL) return;\n    switch (view_id) {{\n"
        ));
        for element in &view.elements {
            if element.kind != "TextInput" {
                continue;
            }
            let Some(action) = view_property(element, property_name) else {
                continue;
            };
            let ExprKind::Var(function) = &action.value.kind else {
                return Err(diag(
                    action.value.span,
                    &format!(
                        "bootstrap Android TextInput.{property_name} lowering requires a named fn(str) -> void callback"
                    ),
                ));
            };
            let element_id = stable_android_element_id(&view.name, &element.name);
            out.push_str(&format!(
                "        case {element_id}: {}(value); break;\n",
                function_c_name(function)
            ));
        }
        out.push_str("        default: break;\n    }\n    (*env)->ReleaseStringUTFChars(env, text, value);\n}\n\n");
    }

    let theme_mode = match application_metadata_string(application, "theme", signatures).as_deref()
    {
        None | Some("system") => 0,
        Some("light") => 1,
        Some("dark") => 2,
        _ => unreachable!("application theme validated by type checking"),
    };
    out.push_str(&format!(
        "JNIEXPORT jint JNICALL Java_app_flux_runtime_FluxActivity_nativeThemeMode(JNIEnv *env, jobject activity) {{\n    (void)env;\n    (void)activity;\n    return (jint){theme_mode};\n}}\n\n"
    ));
    out.push_str("JNIEXPORT jstring JNICALL Java_app_flux_runtime_FluxActivity_nativeThemeColor(JNIEnv *env, jobject activity, jstring token) {\n    (void)activity;\n    if (token == NULL) return NULL;\n    const char *value = (*env)->GetStringUTFChars(env, token, NULL);\n    if (value == NULL) return NULL;\n    jstring result = NULL;\n");
    for (token, _) in APPLICATION_THEME_COLOR_FIELDS {
        if let Some(color) = application_theme_color(application, token, signatures) {
            out.push_str(&format!(
                "    if (strcmp(value, {}) == 0) result = (*env)->NewStringUTF(env, {});\n",
                c_string(token),
                c_string(&color)
            ));
        }
    }
    out.push_str(
        "    (*env)->ReleaseStringUTFChars(env, token, value);\n    return result;\n}\n\n",
    );

    out.push_str("JNIEXPORT void JNICALL Java_app_flux_runtime_FluxActivity_nativeCreate(JNIEnv *env, jobject activity, jstring restored_state) {\n");
    out.push_str("    JavaVM *vm = NULL;\n    if ((*env)->GetJavaVM(env, &vm) != JNI_OK || vm == NULL) return;\n");
    out.push_str("    jobject global_activity = (*env)->NewGlobalRef(env, activity);\n    if (global_activity == NULL) return;\n");
    out.push_str("    if (flux__android_activity == &flux__android_activity_compat && flux__android_activity_compat.clazz != NULL) (*env)->DeleteGlobalRef(env, flux__android_activity_compat.clazz);\n");
    out.push_str("    memset(&flux__android_activity_compat, 0, sizeof(flux__android_activity_compat));\n    flux__android_activity_compat.vm = vm;\n    flux__android_activity_compat.env = env;\n    flux__android_activity_compat.clazz = global_activity;\n    flux__android_activity = &flux__android_activity_compat;\n");
    if let Some(function) = application_metadata_function(application, "on_restore_state") {
        out.push_str("    if (restored_state != NULL) {\n        const char *restored = (*env)->GetStringUTFChars(env, restored_state, NULL);\n        if (restored != NULL) {\n");
        out.push_str(&format!(
            "            {}(restored);\n",
            function_c_name(function)
        ));
        out.push_str("            (*env)->ReleaseStringUTFChars(env, restored_state, restored);\n        }\n    }\n");
    } else {
        out.push_str("    (void)restored_state;\n");
    }
    out.push_str("}\n\n");

    for (metadata, native_name) in [
        ("on_start", "nativeStart"),
        ("on_resume", "nativeResume"),
        ("on_pause", "nativePause"),
        ("on_stop", "nativeStop"),
        ("on_low_memory", "nativeLowMemory"),
        ("on_configuration_changed", "nativeConfigurationChanged"),
    ] {
        out.push_str(&format!(
            "JNIEXPORT void JNICALL Java_app_flux_runtime_FluxActivity_{native_name}(JNIEnv *env, jobject activity) {{\n    (void)env;\n    (void)activity;\n"
        ));
        if let Some(function) = application_metadata_function(application, metadata) {
            out.push_str(&format!("    {}();\n", function_c_name(function)));
        }
        out.push_str("}\n\n");
    }

    out.push_str("JNIEXPORT jstring JNICALL Java_app_flux_runtime_FluxActivity_nativeSaveState(JNIEnv *env, jobject activity) {\n    (void)activity;\n");
    if let Some(function) = application_metadata_function(application, "on_save_state") {
        out.push_str(&format!(
            "    const char *state = {}();\n    return state == NULL ? NULL : flux__android_utf8_string(env, state);\n",
            function_c_name(function)
        ));
    } else {
        out.push_str("    (void)env;\n    return NULL;\n");
    }
    out.push_str("}\n\n");

    out.push_str("JNIEXPORT void JNICALL Java_app_flux_runtime_FluxActivity_nativeDestroy(JNIEnv *env, jobject activity) {\n    (void)activity;\n");
    if let Some(function) = application_metadata_function(application, "on_exit") {
        out.push_str(&format!("    {}();\n", function_c_name(function)));
    }
    out.push_str("    if (flux__android_activity == &flux__android_activity_compat) {\n        jobject global_activity = flux__android_activity_compat.clazz;\n        flux__android_activity = NULL;\n        memset(&flux__android_activity_compat, 0, sizeof(flux__android_activity_compat));\n        if (global_activity != NULL) (*env)->DeleteGlobalRef(env, global_activity);\n    }\n}\n\n");

    for (metadata, callback) in [
        ("on_start", "start"),
        ("on_resume", "resume"),
        ("on_pause", "pause"),
        ("on_stop", "stop"),
        ("on_low_memory", "low_memory"),
    ] {
        if let Some(function) = application_metadata_function(application, metadata) {
            out.push_str(&format!(
                "static void flux__android_on_{callback}(ANativeActivity *activity) {{ (void)activity; {}(); }}\n",
                function_c_name(function),
            ));
        }
    }
    out.push_str(
        "static void flux__android_on_configuration_changed(ANativeActivity *activity) {\n",
    );
    if let Some(function) = application_metadata_function(application, "on_configuration_changed") {
        out.push_str(&format!("    {}();\n", function_c_name(function)));
    }
    out.push_str("    bool detach = false;\n    JNIEnv *env = flux__android_get_env(&detach);\n    if (env != NULL) Java_app_flux_runtime_FluxActivity_nativeBuildUi(env, activity->clazz);\n    flux__android_release_env(detach);\n}\n");
    if let Some(function) = application_metadata_function(application, "on_exit") {
        out.push_str(&format!(
            "static void flux__android_on_destroy(ANativeActivity *activity) {{ (void)activity; {}(); flux__android_activity = NULL; }}\n",
            function_c_name(function),
        ));
    } else {
        out.push_str("static void flux__android_on_destroy(ANativeActivity *activity) { (void)activity; flux__android_activity = NULL; }\n");
    }
    if let Some(function) = application_metadata_function(application, "on_save_state") {
        out.push_str(&format!(
            "static void *flux__android_on_save_instance_state(ANativeActivity *activity, size_t *out_size) {{\n    (void)activity;\n    if (out_size == NULL) return NULL;\n    *out_size = 0;\n    const char *state = {}();\n    if (state == NULL) return NULL;\n    size_t len = strlen(state);\n    if (len == SIZE_MAX) return NULL;\n    size_t size = len + 1;\n    char *copy = (char *)malloc(size);\n    if (copy == NULL) return NULL;\n    memcpy(copy, state, size);\n    *out_size = size;\n    return copy;\n}}\n",
            function_c_name(function),
        ));
    }
    out.push('\n');
    out.push_str("__attribute__((visibility(\"default\"))) void ANativeActivity_onCreate(ANativeActivity *activity, void *saved_state, size_t saved_state_size) {\n    flux__android_activity = activity;\n");
    out.push_str("    activity->callbacks->onDestroy = flux__android_on_destroy;\n");
    for (metadata, callback, field) in [
        ("on_start", "start", "onStart"),
        ("on_resume", "resume", "onResume"),
        ("on_pause", "pause", "onPause"),
        ("on_stop", "stop", "onStop"),
        ("on_low_memory", "low_memory", "onLowMemory"),
    ] {
        if application_metadata_function(application, metadata).is_some() {
            out.push_str(&format!(
                "    activity->callbacks->{field} = flux__android_on_{callback};\n"
            ));
        }
    }
    out.push_str("    activity->callbacks->onConfigurationChanged = flux__android_on_configuration_changed;\n");
    if application_metadata_function(application, "on_save_state").is_some() {
        out.push_str("    activity->callbacks->onSaveInstanceState = flux__android_on_save_instance_state;\n");
    }
    if let Some(function) = application_metadata_function(application, "on_restore_state") {
        out.push_str(&format!(
            "    if (saved_state != NULL && saved_state_size > 0 && saved_state_size < SIZE_MAX) {{\n        char *restored_state = (char *)malloc(saved_state_size + 1);\n        if (restored_state != NULL) {{\n            memcpy(restored_state, saved_state, saved_state_size);\n            restored_state[saved_state_size] = '\\0';\n            {}(restored_state);\n            free(restored_state);\n        }}\n    }}\n",
            function_c_name(function),
        ));
    } else {
        out.push_str("    (void)saved_state;\n    (void)saved_state_size;\n");
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
        "static void flux__ui_window_environment_changed(GObject *object, GParamSpec *pspec, gpointer data) {\n    (void)pspec;\n    (void)data;\n    int width = -1;\n    int height = -1;\n    gtk_window_get_default_size(GTK_WINDOW(object), &width, &height);\n    int scale = gtk_widget_get_scale_factor(GTK_WIDGET(object));\n    int64_t next_width = width > 0 ? (int64_t)width : flux__ui_window_width;\n    int64_t next_height = height > 0 ? (int64_t)height : flux__ui_window_height;\n    int64_t next_scale = scale > 0 ? (int64_t)scale : INT64_C(1);\n    if (next_width == flux__ui_window_width && next_height == flux__ui_window_height && next_scale == flux__ui_display_scale) return;\n    flux__ui_window_width = next_width;\n    flux__ui_window_height = next_height;\n    flux__ui_display_scale = next_scale;\n    flux__ui_refresh_changed(-2);\n}\n\n",
    );

    for element in &view.elements {
        if element.kind == "TextInput" {
            let multiline = match view_property(element, "multiline") {
                Some(property) => {
                    static_expr_bool(&property.value, signatures).ok_or_else(|| {
                        diag(
                            property.value.span,
                            "bootstrap Linux TextInput.multiline must be a compile-time bool value",
                        )
                    })?
                }
                None => false,
            };
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
                if multiline && property_name == "on_change" {
                    out.push_str(&format!(
                        "static void flux__ui_{callback_name}_{}(GtkTextBuffer *buffer, gpointer data) {{ (void)data; GtkTextIter start; GtkTextIter end; gtk_text_buffer_get_bounds(buffer, &start, &end); gchar *text = gtk_text_buffer_get_text(buffer, &start, &end, FALSE); {}(text); g_free(text); flux__ui_refresh(); }}\n",
                        element.name,
                        function_c_name(function),
                    ));
                } else if multiline {
                    out.push_str(&format!(
                        "static gboolean flux__ui_{callback_name}_{}(GtkEventControllerKey *controller, guint keyval, guint keycode, GdkModifierType state, gpointer data) {{ (void)keycode; (void)state; (void)data; if (keyval != GDK_KEY_Return && keyval != GDK_KEY_KP_Enter) return FALSE; GtkWidget *widget = gtk_event_controller_get_widget(GTK_EVENT_CONTROLLER(controller)); GtkTextBuffer *buffer = gtk_text_view_get_buffer(GTK_TEXT_VIEW(widget)); GtkTextIter start; GtkTextIter end; gtk_text_buffer_get_bounds(buffer, &start, &end); gchar *text = gtk_text_buffer_get_text(buffer, &start, &end, FALSE); {}(text); g_free(text); flux__ui_refresh(); return TRUE; }}\n",
                        element.name,
                        function_c_name(function),
                    ));
                } else {
                    out.push_str(&format!(
                        "static void flux__ui_{callback_name}_{}(GtkWidget *widget, gpointer data) {{ (void)data; {}(gtk_editable_get_text(GTK_EDITABLE(widget))); flux__ui_refresh(); }}\n",
                        element.name,
                        function_c_name(function),
                    ));
                }
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
            let state_index = ui_state_index(view, &transition.state);
            out.push_str(&format!(
                "static void flux__ui_click_{}(GtkWidget *widget, gpointer data) {{{active_guard} (void)widget; (void)data; {} = {next}; flux__ui_refresh_changed({state_index}); }}\n",
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
    if view
        .elements
        .iter()
        .any(|element| view_property(element, "on_key").is_some())
    {
        out.push_str(
            "static const char *flux__ui_key_name(guint keyval, char utf8[8]) {\n    switch (keyval) {\n        case GDK_KEY_Return:\n        case GDK_KEY_KP_Enter: return \"Enter\";\n        case GDK_KEY_Escape: return \"Escape\";\n        case GDK_KEY_Tab:\n        case GDK_KEY_ISO_Left_Tab: return \"Tab\";\n        case GDK_KEY_BackSpace: return \"Backspace\";\n        case GDK_KEY_Delete:\n        case GDK_KEY_KP_Delete: return \"Delete\";\n        case GDK_KEY_Left:\n        case GDK_KEY_KP_Left: return \"ArrowLeft\";\n        case GDK_KEY_Right:\n        case GDK_KEY_KP_Right: return \"ArrowRight\";\n        case GDK_KEY_Up:\n        case GDK_KEY_KP_Up: return \"ArrowUp\";\n        case GDK_KEY_Down:\n        case GDK_KEY_KP_Down: return \"ArrowDown\";\n        case GDK_KEY_Home:\n        case GDK_KEY_KP_Home: return \"Home\";\n        case GDK_KEY_End:\n        case GDK_KEY_KP_End: return \"End\";\n        case GDK_KEY_Page_Up:\n        case GDK_KEY_KP_Page_Up: return \"PageUp\";\n        case GDK_KEY_Page_Down:\n        case GDK_KEY_KP_Page_Down: return \"PageDown\";\n        default: break;\n    }\n    gunichar character = gdk_keyval_to_unicode(keyval);\n    if (character != 0 && !g_unichar_iscntrl(character)) {\n        int length = g_unichar_to_utf8(character, utf8);\n        utf8[length] = '\\0';\n        return utf8;\n    }\n    const char *name = gdk_keyval_name(keyval);\n    return name != NULL ? name : \"Unknown\";\n}\n"
        );
    }
    for element in &view.elements {
        if let Some(action) = view_property(element, "on_tap") {
            let body = ui_zero_arg_event_body(action, view, signatures)?;
            out.push_str(&format!(
                "static void flux__ui_tap_{}(GtkGestureClick *gesture, int n_press, double x, double y, gpointer data) {{ (void)gesture; (void)n_press; (void)x; (void)y; (void)data; {body} }}\n",
                element.name,
            ));
        }
        if let Some(action) = view_property(element, "on_long_press") {
            let body = ui_zero_arg_event_body(action, view, signatures)?;
            out.push_str(&format!(
                "static void flux__ui_long_press_{}(GtkGestureLongPress *gesture, double x, double y, gpointer data) {{ (void)gesture; (void)x; (void)y; (void)data; {body} }}\n",
                element.name,
            ));
        }
        if let Some(action) = view_property(element, "on_key") {
            let ExprKind::Var(function) = &action.value.kind else {
                return Err(diag(
                    action.value.span,
                    "bootstrap native onKey requires a named fn(str) -> void callback",
                ));
            };
            out.push_str(&format!(
                "static gboolean flux__ui_key_{}(GtkEventControllerKey *controller, guint keyval, guint keycode, GdkModifierType state, gpointer data) {{ (void)controller; (void)keycode; (void)state; (void)data; char utf8[8] = {{0}}; const char *key = flux__ui_key_name(keyval, utf8); {}(key); flux__ui_refresh(); return FALSE; }}\n",
                element.name,
                function_c_name(function),
            ));
        }
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
    let on_stop = application_metadata_function(application, "on_stop");
    let on_exit = application_metadata_function(application, "on_exit");
    if on_stop.is_some() || on_exit.is_some() {
        out.push_str("static void flux__ui_shutdown(GtkApplication *application, gpointer data) { (void)application; (void)data;\n");
        if let Some(function) = on_stop {
            out.push_str(&format!("    {}();\n", function_c_name(function)));
        }
        if let Some(function) = on_exit {
            out.push_str(&format!("    {}();\n", function_c_name(function)));
        }
        out.push_str("}\n\n");
    }
    let on_resume = application_metadata_function(application, "on_resume");
    let on_pause = application_metadata_function(application, "on_pause");
    if on_resume.is_some() || on_pause.is_some() {
        out.push_str("static void flux__ui_active_changed(GObject *object, GParamSpec *pspec, gpointer data) {\n    (void)pspec;\n    (void)data;\n    bool active = gtk_window_is_active(GTK_WINDOW(object));\n");
        if let Some(function) = on_resume {
            out.push_str(&format!(
                "    if (active) {}();\n",
                function_c_name(function)
            ));
        }
        if let Some(function) = on_pause {
            out.push_str(&format!(
                "    if (!active) {}();\n",
                function_c_name(function)
            ));
        }
        out.push_str("}\n\n");
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
    out.push_str("    GtkCssProvider *flux__theme_provider = gtk_css_provider_new();\n");
    let mut theme_css = String::new();
    for (token, css_name, fallback) in [
        ("surface", "surface", "@theme_bg_color"),
        ("surfaceRaised", "surface_raised", "@theme_base_color"),
        ("text", "text", "@theme_fg_color"),
        ("textMuted", "text_muted", "alpha(@theme_fg_color, 0.62)"),
        ("accent", "accent", "@theme_selected_bg_color"),
        ("onAccent", "on_accent", "@theme_selected_fg_color"),
        ("outline", "outline", "alpha(@theme_fg_color, 0.20)"),
        ("danger", "danger", "#dc2626"),
        ("success", "success", "#16a34a"),
        ("warning", "warning", "#d97706"),
        ("shadow", "shadow", "alpha(black, 0.24)"),
    ] {
        let value = application_theme_color(application, token, signatures)
            .unwrap_or_else(|| fallback.to_string());
        theme_css.push_str(&format!("@define-color flux_{css_name} {value}; "));
    }
    theme_css.push_str(".flux-root { background-color: @flux_surface; color: @flux_text; } .flux-text { color: @flux_text; } .flux-button { border-radius: 10px; padding: 8px 14px; font-weight: 600; } .flux-input { border-radius: 10px; padding: 8px 10px; } .flux-check { padding: 4px; } @media (prefers-contrast: more) { .flux-root { background-color: @theme_bg_color; color: @theme_fg_color; } .flux-text { color: @theme_fg_color; } .flux-button, .flux-input, .flux-check { outline: 2px solid @theme_fg_color; outline-offset: 1px; } }");
    out.push_str(&format!(
        "    gtk_css_provider_load_from_data(flux__theme_provider, {}, -1);\n",
        c_string(&theme_css)
    ));
    out.push_str("    gtk_style_context_add_provider_for_display(gtk_widget_get_display(window), GTK_STYLE_PROVIDER(flux__theme_provider), GTK_STYLE_PROVIDER_PRIORITY_THEME + 1);\n");
    out.push_str("    GtkSettings *flux__theme_settings = gtk_settings_get_default();\n    if (flux__theme_settings != NULL) g_object_bind_property(flux__theme_settings, \"gtk-interface-contrast\", flux__theme_provider, \"prefers-contrast\", G_BINDING_SYNC_CREATE);\n");
    out.push_str("    g_object_unref(flux__theme_provider);\n");
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
    if on_resume.is_some() || on_pause.is_some() {
        out.push_str("    g_signal_connect(window, \"notify::is-active\", G_CALLBACK(flux__ui_active_changed), NULL);\n");
    }
    if let Some(resizable) = application_metadata_bool(application, "resizable", signatures) {
        out.push_str(&format!(
            "    gtk_window_set_resizable(GTK_WINDOW(window), {});\n",
            if resizable { "TRUE" } else { "FALSE" }
        ));
    }
    out.push_str("    GtkWidget *grid = gtk_grid_new();\n    gtk_widget_add_css_class(grid, \"flux-root\");\n");
    if let Some(direction) =
        application_metadata_string(application, "layout_direction", signatures)
    {
        match direction.as_str() {
            "ltr" => out.push_str("    gtk_widget_set_direction(grid, GTK_TEXT_DIR_LTR);\n"),
            "rtl" => out.push_str("    gtk_widget_set_direction(grid, GTK_TEXT_DIR_RTL);\n"),
            "system" => {}
            _ => unreachable!("application layoutDirection validated by type checking"),
        }
    }
    let gap = view.grid.gap.unwrap_or(12);
    out.push_str(&format!(
        "    gtk_grid_set_column_spacing(GTK_GRID(grid), {gap});\n    gtk_grid_set_row_spacing(GTK_GRID(grid), {gap});\n"
    ));
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
                    "    gtk_widget_add_css_class({variable}, \"flux-text\");\n"
                ));
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
                let (default_size, default_bold, default_line_height_percent) =
                    text_semantic_typography(element, signatures)?;
                let size = view_property(element, "size");
                let bold = view_property(element, "bold");
                let italic = view_property(element, "italic");
                let underline = view_property(element, "underline");
                let strikethrough = view_property(element, "strikethrough");
                let font_family = view_property(element, "font_family");
                let letter_spacing = view_property(element, "letter_spacing");
                let line_height_percent = view_property(element, "line_height_percent");
                let color = view_property(element, "color");
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
                    } else {
                        out.push_str(&format!(
                            "    pango_attr_list_insert({attrs}, pango_attr_size_new({default_size} * PANGO_SCALE));\n"
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
                    } else if default_bold {
                        out.push_str(&format!(
                            "    pango_attr_list_insert({attrs}, pango_attr_weight_new(PANGO_WEIGHT_BOLD));\n"
                        ));
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
                    } else {
                        out.push_str(&format!(
                            "    pango_attr_list_insert({attrs}, pango_attr_line_height_new({:.4}));\n",
                            default_line_height_percent as f64 / 100.0
                        ));
                    }
                    if let Some(property) = color {
                        let Some(value) = static_expr_str(&property.value, signatures) else {
                            return Err(diag(
                                property.value.span,
                                "bootstrap Linux Text.color must be a compile-time str value",
                            ));
                        };
                        if let Some((red, green, blue, alpha)) = parse_hex_rgba(&value) {
                            out.push_str(&format!(
                                "    pango_attr_list_insert({attrs}, pango_attr_foreground_new({red}, {green}, {blue}));\n"
                            ));
                            if let Some(alpha) = alpha {
                                out.push_str(&format!(
                                    "    pango_attr_list_insert({attrs}, pango_attr_foreground_alpha_new({alpha}));\n"
                                ));
                            }
                        } else if !is_semantic_ui_color(&value) {
                            return Err(diag(
                                property.value.span,
                                "Text.color must use '#RRGGBB', '#RRGGBBAA', or a semantic Flux color token",
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
                let multiline = match view_property(element, "multiline") {
                    Some(property) => static_expr_bool(&property.value, signatures).ok_or_else(|| {
                        diag(
                            property.value.span,
                            "bootstrap Linux TextInput.multiline must be a compile-time bool value",
                        )
                    })?,
                    None => false,
                };
                let submit_on_enter = match view_property(element, "submit_on_enter") {
                    Some(property) => static_expr_bool(&property.value, signatures).ok_or_else(|| {
                        diag(
                            property.value.span,
                            "bootstrap Linux TextInput.submitOnEnter must be a compile-time bool value",
                        )
                    })?,
                    None => !multiline,
                };
                let text = match view_property(element, "text") {
                    None => c_string(""),
                    Some(property) => ui_expr_c(&property.value, view, signatures)?,
                };
                let multiline_buffer = format!("flux__ui_buffer_{}", element.name);
                if multiline {
                    if let Some(property) = view_property(element, "placeholder") {
                        return Err(diag(
                            property.value.span,
                            "TextInput.placeholder is not yet supported for multiline Linux GTK input",
                        ));
                    }
                    out.push_str(&format!("    {variable} = gtk_text_view_new();\n"));
                    out.push_str(&format!(
                        "    gtk_widget_add_css_class({variable}, \"flux-input\");\n"
                    ));
                    out.push_str(&format!(
                        "    GtkTextBuffer *{multiline_buffer} = gtk_text_view_get_buffer(GTK_TEXT_VIEW({variable}));\n"
                    ));
                    out.push_str(&format!(
                        "    gtk_text_buffer_set_text({multiline_buffer}, {text}, -1);\n"
                    ));
                    out.push_str(&format!(
                        "    gtk_text_view_set_wrap_mode(GTK_TEXT_VIEW({variable}), GTK_WRAP_WORD_CHAR);\n"
                    ));
                } else {
                    out.push_str(&format!("    {variable} = gtk_entry_new();\n"));
                    out.push_str(&format!(
                        "    gtk_widget_add_css_class({variable}, \"flux-input\");\n"
                    ));
                    out.push_str(&format!(
                        "    gtk_editable_set_text(GTK_EDITABLE({variable}), {text});\n"
                    ));
                    if let Some(property) = view_property(element, "placeholder") {
                        let placeholder = ui_expr_c(&property.value, view, signatures)?;
                        out.push_str(&format!(
                            "    gtk_entry_set_placeholder_text(GTK_ENTRY({variable}), {placeholder});\n"
                        ));
                    }
                }
                if let Some(property) = view_property(element, "enabled") {
                    let enabled = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "    gtk_widget_set_sensitive({variable}, {enabled});\n"
                    ));
                }
                if let Some(property) = view_property(element, "keyboard_type") {
                    let Some(keyboard_type) = static_expr_str(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Linux TextInput.keyboardType must be a compile-time string",
                        ));
                    };
                    let purpose = match keyboard_type.as_str() {
                        "text" => "GTK_INPUT_PURPOSE_FREE_FORM",
                        "email" => "GTK_INPUT_PURPOSE_EMAIL",
                        "number" => "GTK_INPUT_PURPOSE_DIGITS",
                        "decimal" => "GTK_INPUT_PURPOSE_NUMBER",
                        "phone" => "GTK_INPUT_PURPOSE_PHONE",
                        "url" => "GTK_INPUT_PURPOSE_URL",
                        _ => {
                            return Err(diag(
                                property.value.span,
                                "TextInput.keyboardType must be one of 'text', 'email', 'number', 'decimal', 'phone', or 'url'",
                            ));
                        }
                    };
                    if multiline {
                        out.push_str(&format!(
                            "    gtk_text_view_set_input_purpose(GTK_TEXT_VIEW({variable}), {purpose});\n"
                        ));
                    } else {
                        out.push_str(&format!(
                            "    gtk_entry_set_input_purpose(GTK_ENTRY({variable}), {purpose});\n"
                        ));
                    }
                }
                if let Some(property) = view_property(element, "password") {
                    let Some(password) = static_expr_bool(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Linux TextInput.password must be a compile-time bool value",
                        ));
                    };
                    if password && multiline {
                        return Err(diag(
                            property.value.span,
                            "TextInput.password and TextInput.multiline cannot both be true",
                        ));
                    }
                    if password {
                        out.push_str(&format!(
                            "    gtk_entry_set_visibility(GTK_ENTRY({variable}), FALSE);\n"
                        ));
                        if view_property(element, "keyboard_type").is_none() {
                            out.push_str(&format!(
                                "    gtk_entry_set_input_purpose(GTK_ENTRY({variable}), GTK_INPUT_PURPOSE_PASSWORD);\n"
                            ));
                        }
                    }
                }
                if let Some(property) = view_property(element, "max_length") {
                    let Some(max_length) = static_expr_i64(&property.value, signatures) else {
                        return Err(diag(
                            property.value.span,
                            "bootstrap Linux TextInput.maxLength must be a compile-time i64 value",
                        ));
                    };
                    if !(0..=i64::from(i32::MAX)).contains(&max_length) {
                        return Err(diag(
                            property.value.span,
                            "TextInput.maxLength must be between 0 and 2147483647",
                        ));
                    }
                    if multiline {
                        return Err(diag(
                            property.value.span,
                            "TextInput.maxLength is not yet supported for multiline Linux GTK input",
                        ));
                    }
                    out.push_str(&format!(
                        "    gtk_entry_set_max_length(GTK_ENTRY({variable}), {max_length});\n"
                    ));
                }
                if view_property(element, "on_change").is_some() {
                    if multiline {
                        out.push_str(&format!(
                            "    g_signal_connect({multiline_buffer}, \"changed\", G_CALLBACK(flux__ui_change_{}), NULL);\n",
                            element.name
                        ));
                    } else {
                        out.push_str(&format!(
                            "    g_signal_connect({variable}, \"changed\", G_CALLBACK(flux__ui_change_{}), NULL);\n",
                            element.name
                        ));
                    }
                }
                if view_property(element, "on_submit").is_some() && submit_on_enter {
                    if multiline {
                        let controller = format!("flux__submit_controller_{}", element.name);
                        out.push_str(&format!(
                            "    GtkEventController *{controller} = gtk_event_controller_key_new();\n    g_signal_connect({controller}, \"key-pressed\", G_CALLBACK(flux__ui_submit_{}), NULL);\n    gtk_widget_add_controller({variable}, {controller});\n",
                            element.name
                        ));
                    } else {
                        out.push_str(&format!(
                            "    g_signal_connect({variable}, \"activate\", G_CALLBACK(flux__ui_submit_{}), NULL);\n",
                            element.name
                        ));
                    }
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
                out.push_str(&format!(
                    "    gtk_widget_add_css_class({variable}, \"flux-check\");\n"
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
                out.push_str(&format!(
                    "    gtk_widget_add_css_class({variable}, \"flux-check\");\n"
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
                out.push_str(&format!(
                    "    gtk_widget_add_css_class({variable}, \"flux-button\");\n"
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
        if let Some(property) = view_property(element, "accessibility_hidden") {
            let hidden = ui_expr_c(&property.value, view, signatures)?;
            out.push_str(&format!(
                "    gtk_accessible_update_state(GTK_ACCESSIBLE({variable}), GTK_ACCESSIBLE_STATE_HIDDEN, {hidden}, -1);\n"
            ));
        }
        if let Some(property) = view_property(element, "focusable") {
            let focusable = ui_expr_c(&property.value, view, signatures)?;
            out.push_str(&format!(
                "    gtk_widget_set_focusable({variable}, {focusable});\n"
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
        if view_property(element, "on_tap").is_some() {
            let controller = format!("flux__tap_{}", element.name);
            out.push_str(&format!(
                "    GtkEventController *{controller} = GTK_EVENT_CONTROLLER(gtk_gesture_click_new());\n"
            ));
            out.push_str(&format!(
                "    g_signal_connect({controller}, \"released\", G_CALLBACK(flux__ui_tap_{}), NULL);\n",
                element.name
            ));
            out.push_str(&format!(
                "    gtk_widget_add_controller({variable}, {controller});\n"
            ));
        }
        if view_property(element, "on_long_press").is_some() {
            let controller = format!("flux__long_press_{}", element.name);
            out.push_str(&format!(
                "    GtkEventController *{controller} = GTK_EVENT_CONTROLLER(gtk_gesture_long_press_new());\n"
            ));
            out.push_str(&format!(
                "    g_signal_connect({controller}, \"pressed\", G_CALLBACK(flux__ui_long_press_{}), NULL);\n",
                element.name
            ));
            out.push_str(&format!(
                "    gtk_widget_add_controller({variable}, {controller});\n"
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
        if view_property(element, "on_key").is_some() {
            let controller = format!("flux__key_{}", element.name);
            if view_property(element, "focusable").is_none() {
                out.push_str(&format!(
                    "    gtk_widget_set_focusable({variable}, TRUE);\n"
                ));
            }
            out.push_str(&format!(
                "    GtkEventController *{controller} = gtk_event_controller_key_new();\n    gtk_event_controller_set_propagation_phase({controller}, GTK_PHASE_CAPTURE);\n    g_signal_connect({controller}, \"key-pressed\", G_CALLBACK(flux__ui_key_{}), NULL);\n    gtk_widget_add_controller({variable}, {controller});\n",
                element.name
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
    let accessibility_order = ordered_accessibility_elements(view, signatures)?;
    for pair in accessibility_order.windows(2) {
        let current = ui_widget_c_name(&pair[0].name);
        let next = ui_widget_c_name(&pair[1].name);
        out.push_str(&format!(
            "    gtk_accessible_update_relation(GTK_ACCESSIBLE({current}), GTK_ACCESSIBLE_RELATION_FLOW_TO, GTK_ACCESSIBLE({next}), -1);\n"
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
    if on_stop.is_some() || on_exit.is_some() {
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

const APPLICATION_THEME_COLOR_FIELDS: &[(&str, &str)] = &[
    ("surface", "surface_color"),
    ("surfaceRaised", "surface_raised_color"),
    ("text", "text_color"),
    ("textMuted", "text_muted_color"),
    ("accent", "accent_color"),
    ("onAccent", "on_accent_color"),
    ("outline", "outline_color"),
    ("danger", "danger_color"),
    ("success", "success_color"),
    ("warning", "warning_color"),
    ("shadow", "shadow_color"),
];

fn application_theme_color(
    application: &crate::ast::ApplicationDef,
    token: &str,
    signatures: &Signatures,
) -> Option<String> {
    let (_, metadata_name) = APPLICATION_THEME_COLOR_FIELDS
        .iter()
        .find(|(candidate, _)| *candidate == token)?;
    application_metadata_string(application, metadata_name, signatures)
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

fn ordered_accessibility_elements<'a>(
    view: &'a crate::ast::ViewDef,
    signatures: &Signatures,
) -> Result<Vec<&'a crate::ast::ViewElement>, Diagnostic> {
    let mut ordered = Vec::new();
    for element in &view.elements {
        let Some(property) = view_property(element, "accessibility_order") else {
            continue;
        };
        let Some(order) = static_expr_i64(&property.value, signatures) else {
            return Err(diag(
                property.value.span,
                "accessibilityOrder must be a compile-time i64 value",
            ));
        };
        if order < 0 {
            return Err(diag(
                property.value.span,
                "accessibilityOrder must be non-negative",
            ));
        }
        ordered.push((order, element));
    }
    ordered.sort_by_key(|(order, _)| *order);
    for pair in ordered.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err(diag(
                pair[1].1.span,
                &format!("duplicate accessibilityOrder {}", pair[1].0),
            ));
        }
    }
    Ok(ordered.into_iter().map(|(_, element)| element).collect())
}

#[derive(Debug, Clone, Default)]
struct UiRefreshDependencies {
    states: HashSet<usize>,
    environment: bool,
}

fn ui_runtime_dependency_map(view: &crate::ast::ViewDef) -> HashMap<String, UiRefreshDependencies> {
    let mut dependencies = HashMap::new();
    for (index, state) in view.states.iter().enumerate() {
        let mut state_dependencies = UiRefreshDependencies::default();
        state_dependencies.states.insert(index);
        dependencies.insert(state.name.clone(), state_dependencies);
    }
    for derived in &view.derived {
        let mut reads = HashSet::new();
        typecheck::collect_expr_reads(&derived.value, &mut reads);
        let mut derived_dependencies = UiRefreshDependencies::default();
        for name in reads {
            if typecheck::view_environment_type(&name).is_some() {
                derived_dependencies.environment = true;
            }
            if let Some(read_dependencies) = dependencies.get(&name) {
                derived_dependencies
                    .states
                    .extend(read_dependencies.states.iter().copied());
                derived_dependencies.environment |= read_dependencies.environment;
            }
        }
        dependencies.insert(derived.name.clone(), derived_dependencies);
    }
    dependencies
}

fn ui_expr_refresh_dependencies(
    expr: &Expr,
    runtime_dependencies: &HashMap<String, UiRefreshDependencies>,
) -> UiRefreshDependencies {
    let mut reads = HashSet::new();
    typecheck::collect_expr_reads(expr, &mut reads);
    let mut dependencies = UiRefreshDependencies::default();
    for name in reads {
        if typecheck::view_environment_type(&name).is_some() {
            dependencies.environment = true;
        }
        if let Some(read_dependencies) = runtime_dependencies.get(&name) {
            dependencies
                .states
                .extend(read_dependencies.states.iter().copied());
            dependencies.environment |= read_dependencies.environment;
        }
    }
    dependencies
}

fn ui_property_is_refreshable(element_kind: &str, property_name: &str) -> bool {
    if matches!(
        property_name,
        "visible"
            | "enabled"
            | "clip"
            | "tooltip"
            | "accessibility_label"
            | "accessibility_description"
            | "accessibility_hidden"
            | "translate_x"
            | "translate_y"
            | "rotate_degrees"
            | "scale_percent"
            | "scale_x_percent"
            | "scale_y_percent"
            | "skew_x_degrees"
            | "skew_y_degrees"
            | "transform_origin_x_percent"
            | "transform_origin_y_percent"
    ) {
        return true;
    }
    matches!(
        (element_kind, property_name),
        ("Text", "text")
            | ("Text", "selectable")
            | ("Text", "wrap")
            | ("Button", "text")
            | ("TextInput", "placeholder")
            | ("Image", "source")
            | ("Image", "alt")
            | ("Image", "can_shrink")
            | ("Toggle", "label")
            | ("Toggle", "checked")
            | ("Radio", "label")
            | ("Radio", "selected")
    )
}

fn ui_element_refresh_dependencies(
    element: &crate::ast::ViewElement,
    runtime_dependencies: &HashMap<String, UiRefreshDependencies>,
) -> UiRefreshDependencies {
    let mut dependencies = UiRefreshDependencies::default();
    for property in &element.properties {
        if !ui_property_is_refreshable(&element.kind, &property.name) {
            continue;
        }
        let property_dependencies =
            ui_expr_refresh_dependencies(&property.value, runtime_dependencies);
        dependencies
            .states
            .extend(property_dependencies.states.iter().copied());
        dependencies.environment |= property_dependencies.environment;
    }
    dependencies
}

fn ui_refresh_condition(dependencies: &UiRefreshDependencies) -> String {
    let mut states = dependencies.states.iter().copied().collect::<Vec<_>>();
    states.sort_unstable();
    let mut clauses = Vec::new();
    if dependencies.environment {
        clauses.push("changed_state < 0".to_string());
    } else {
        clauses.push("changed_state == -1".to_string());
    }
    clauses.extend(
        states
            .into_iter()
            .map(|state| format!("changed_state == {state}")),
    );
    clauses.join(" || ")
}

fn ui_state_index(view: &crate::ast::ViewDef, state_name: &str) -> usize {
    view.states
        .iter()
        .position(|state| state.name == state_name)
        .expect("validated UI transition state exists in its declaring view")
}

fn android_ui_runtime_value_names(view: &crate::ast::ViewDef) -> HashSet<String> {
    let mut runtime_names = view
        .states
        .iter()
        .map(|state| state.name.clone())
        .collect::<HashSet<_>>();
    for derived in &view.derived {
        let mut reads = HashSet::new();
        typecheck::collect_expr_reads(&derived.value, &mut reads);
        if reads.iter().any(|name| {
            runtime_names.contains(name) || typecheck::view_environment_type(name).is_some()
        }) {
            runtime_names.insert(derived.name.clone());
        }
    }
    runtime_names
}

fn android_ui_expr_needs_refresh(expr: &Expr, runtime_names: &HashSet<String>) -> bool {
    let mut reads = HashSet::new();
    typecheck::collect_expr_reads(expr, &mut reads);
    reads.iter().any(|name| {
        runtime_names.contains(name) || typecheck::view_environment_type(name).is_some()
    })
}

fn android_ui_property_needs_refresh(
    element: &crate::ast::ViewElement,
    property_name: &str,
    runtime_names: &HashSet<String>,
) -> bool {
    view_property(element, property_name)
        .is_some_and(|property| android_ui_expr_needs_refresh(&property.value, runtime_names))
}

fn android_ui_element_needs_refresh(
    element: &crate::ast::ViewElement,
    runtime_names: &HashSet<String>,
) -> bool {
    let common = [
        "visible",
        "enabled",
        "focusable",
        "tooltip",
        "accessibility_label",
        "accessibility_description",
        "accessibility_hidden",
        "translate_x",
        "translate_y",
        "rotate_degrees",
        "scale_percent",
        "scale_x_percent",
        "scale_y_percent",
        "transform_origin_x_percent",
        "transform_origin_y_percent",
    ];
    let specific: &[&str] = match element.kind.as_str() {
        "Text" => &["text", "selectable", "wrap"],
        "Button" => &["text"],
        "TextInput" => &["placeholder"],
        "Image" => &["source", "alt", "can_shrink"],
        "Toggle" => &["label", "checked"],
        "Radio" => &["label", "selected"],
        _ => &[],
    };
    common
        .iter()
        .chain(specific.iter())
        .any(|name| android_ui_property_needs_refresh(element, name, runtime_names))
}

fn emit_android_ui_refresh(
    out: &mut String,
    view: &crate::ast::ViewDef,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    let runtime_names = android_ui_runtime_value_names(view);
    let runtime_dependencies = ui_runtime_dependency_map(view);
    out.push_str("static void flux__android_ui_refresh(JNIEnv *env, jobject activity, int changed_state) {\n");
    for derived in &view.derived {
        if !runtime_names.contains(&derived.name) {
            continue;
        }
        let value = ui_expr_c(&derived.value, view, signatures)?;
        let dependencies = runtime_dependencies
            .get(&derived.name)
            .expect("derived runtime dependency metadata exists");
        let condition = ui_refresh_condition(dependencies);
        out.push_str(&format!(
            "    if ({condition}) {} = {value};\n",
            ui_derived_c_name(&derived.name)
        ));
    }
    out.push_str("    jclass activity_class = (*env)->GetObjectClass(env, activity);\n");
    out.push_str("    if (activity_class == NULL) return;\n");
    out.push_str("    jmethodID find_view = (*env)->GetMethodID(env, activity_class, \"findViewById\", \"(I)Landroid/view/View;\");\n");
    out.push_str(
        "    if (find_view == NULL) { (*env)->DeleteLocalRef(env, activity_class); return; }\n",
    );

    for element in &view.elements {
        if !android_ui_element_needs_refresh(element, &runtime_names) {
            continue;
        }
        let dependencies = ui_element_refresh_dependencies(element, &runtime_dependencies);
        let condition = ui_refresh_condition(&dependencies);
        let element_id = stable_android_element_id(&view.name, &element.name);
        out.push_str(&format!("    if ({condition}) {{\n"));
        out.push_str(&format!(
            "        jobject child = (*env)->CallObjectMethod(env, activity, find_view, (jint){element_id});\n"
        ));
        out.push_str("        if (child != NULL && !(*env)->ExceptionCheck(env)) {\n");
        out.push_str("            jclass child_class = (*env)->GetObjectClass(env, child);\n");
        out.push_str("            if (child_class != NULL) {\n");

        if android_ui_property_needs_refresh(element, "visible", &runtime_names)
            && let Some(property) = view_property(element, "visible")
        {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str("                jmethodID refresh_visibility = (*env)->GetMethodID(env, child_class, \"setVisibility\", \"(I)V\");\n");
            out.push_str(&format!(
                "                if (refresh_visibility != NULL) (*env)->CallVoidMethod(env, child, refresh_visibility, (jint)(({value}) ? 0 : 8));\n"
            ));
        }
        if android_ui_property_needs_refresh(element, "enabled", &runtime_names)
            && let Some(property) = view_property(element, "enabled")
        {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str("                jmethodID refresh_enabled = (*env)->GetMethodID(env, child_class, \"setEnabled\", \"(Z)V\");\n");
            out.push_str(&format!(
                "                if (refresh_enabled != NULL) (*env)->CallVoidMethod(env, child, refresh_enabled, (jboolean)({value}));\n"
            ));
        }
        if android_ui_property_needs_refresh(element, "tooltip", &runtime_names)
            && let Some(property) = view_property(element, "tooltip")
        {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str(&format!(
                "                jstring refresh_tooltip_text = flux__android_utf8_string(env, {value});\n"
            ));
            out.push_str("                if (refresh_tooltip_text != NULL) {\n");
            out.push_str("                    jmethodID refresh_tooltip = (*env)->GetMethodID(env, activity_class, \"setTooltip\", \"(Landroid/view/View;Ljava/lang/String;)V\");\n");
            out.push_str("                    if (refresh_tooltip != NULL) (*env)->CallVoidMethod(env, activity, refresh_tooltip, child, refresh_tooltip_text);\n");
            out.push_str(
                "                    (*env)->DeleteLocalRef(env, refresh_tooltip_text);\n",
            );
            out.push_str("                }\n");
        }
        let runtime_transform = [
            "translate_x",
            "translate_y",
            "rotate_degrees",
            "scale_percent",
            "scale_x_percent",
            "scale_y_percent",
            "transform_origin_x_percent",
            "transform_origin_y_percent",
        ]
        .iter()
        .any(|name| android_ui_property_needs_refresh(element, name, &runtime_names));
        if runtime_transform {
            let transform_value =
                |property_name: &str, fallback: &str| -> Result<String, Diagnostic> {
                    view_property(element, property_name)
                        .map(|property| ui_expr_c(&property.value, view, signatures))
                        .unwrap_or_else(|| Ok(fallback.to_string()))
                };
            let translate_x = transform_value("translate_x", "0")?;
            let translate_y = transform_value("translate_y", "0")?;
            let rotate_degrees = transform_value("rotate_degrees", "0")?;
            let scale_percent = transform_value("scale_percent", "100")?;
            let scale_x_percent = view_property(element, "scale_x_percent")
                .map(|property| ui_expr_c(&property.value, view, signatures))
                .unwrap_or_else(|| Ok(scale_percent.clone()))?;
            let scale_y_percent = view_property(element, "scale_y_percent")
                .map(|property| ui_expr_c(&property.value, view, signatures))
                .unwrap_or_else(|| Ok(scale_percent.clone()))?;
            let origin_x = transform_value("transform_origin_x_percent", "50")?;
            let origin_y = transform_value("transform_origin_y_percent", "50")?;
            out.push_str("                jmethodID refresh_transform = (*env)->GetMethodID(env, activity_class, \"transformView\", \"(Landroid/view/View;FFFFFFF)V\");\n");
            out.push_str(&format!(
                "                if (refresh_transform != NULL) (*env)->CallVoidMethod(env, activity, refresh_transform, child, (jfloat)(({translate_x}) * flux__ui_density), (jfloat)(({translate_y}) * flux__ui_density), (jfloat)({rotate_degrees}), (jfloat)(({scale_x_percent}) / 100.0f), (jfloat)(({scale_y_percent}) / 100.0f), (jfloat)({origin_x}), (jfloat)({origin_y}));\n"
            ));
        }

        match element.kind.as_str() {
            "Text" | "Button" | "Toggle" | "Radio" => {
                let text_property = if matches!(element.kind.as_str(), "Toggle" | "Radio") {
                    "label"
                } else {
                    "text"
                };
                if android_ui_property_needs_refresh(element, text_property, &runtime_names)
                    && let Some(property) = view_property(element, text_property)
                {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "                jstring refresh_text_value = flux__android_utf8_string(env, {value});\n"
                    ));
                    out.push_str("                if (refresh_text_value != NULL) {\n");
                    out.push_str("                    jmethodID refresh_text = (*env)->GetMethodID(env, child_class, \"setText\", \"(Ljava/lang/CharSequence;)V\");\n");
                    out.push_str("                    if (refresh_text != NULL) (*env)->CallVoidMethod(env, child, refresh_text, refresh_text_value);\n");
                    out.push_str(
                        "                    (*env)->DeleteLocalRef(env, refresh_text_value);\n",
                    );
                    out.push_str("                }\n");
                }
                if element.kind == "Text" {
                    if android_ui_property_needs_refresh(element, "selectable", &runtime_names)
                        && let Some(property) = view_property(element, "selectable")
                    {
                        let value = ui_expr_c(&property.value, view, signatures)?;
                        out.push_str("                jmethodID refresh_selectable = (*env)->GetMethodID(env, child_class, \"setTextIsSelectable\", \"(Z)V\");\n");
                        out.push_str(&format!(
                            "                if (refresh_selectable != NULL) (*env)->CallVoidMethod(env, child, refresh_selectable, (jboolean)({value}));\n"
                        ));
                    }
                    if android_ui_property_needs_refresh(element, "wrap", &runtime_names)
                        && let Some(property) = view_property(element, "wrap")
                    {
                        let value = ui_expr_c(&property.value, view, signatures)?;
                        out.push_str("                jmethodID refresh_single_line = (*env)->GetMethodID(env, child_class, \"setSingleLine\", \"(Z)V\");\n");
                        out.push_str(&format!(
                            "                if (refresh_single_line != NULL) (*env)->CallVoidMethod(env, child, refresh_single_line, (jboolean)(!({value})));\n"
                        ));
                    }
                }
                if matches!(element.kind.as_str(), "Toggle" | "Radio") {
                    let checked_property = if element.kind == "Toggle" {
                        "checked"
                    } else {
                        "selected"
                    };
                    if android_ui_property_needs_refresh(element, checked_property, &runtime_names)
                        && let Some(property) = view_property(element, checked_property)
                    {
                        let value = ui_expr_c(&property.value, view, signatures)?;
                        out.push_str("                jmethodID refresh_checked = (*env)->GetMethodID(env, activity_class, \"setCheckedSilently\", \"(Landroid/widget/CompoundButton;Z)V\");\n");
                        out.push_str(&format!(
                            "                if (refresh_checked != NULL) (*env)->CallVoidMethod(env, activity, refresh_checked, child, (jboolean)({value}));\n"
                        ));
                    }
                }
            }
            "TextInput" => {
                if android_ui_property_needs_refresh(element, "placeholder", &runtime_names)
                    && let Some(property) = view_property(element, "placeholder")
                {
                    let value = ui_expr_c(&property.value, view, signatures)?;
                    out.push_str(&format!(
                        "                jstring refresh_hint_value = flux__android_utf8_string(env, {value});\n"
                    ));
                    out.push_str("                if (refresh_hint_value != NULL) {\n");
                    out.push_str("                    jmethodID refresh_hint = (*env)->GetMethodID(env, child_class, \"setHint\", \"(Ljava/lang/CharSequence;)V\");\n");
                    out.push_str("                    if (refresh_hint != NULL) (*env)->CallVoidMethod(env, child, refresh_hint, refresh_hint_value);\n");
                    out.push_str(
                        "                    (*env)->DeleteLocalRef(env, refresh_hint_value);\n",
                    );
                    out.push_str("                }\n");
                }
            }
            "Image" => {
                let source_dynamic =
                    android_ui_property_needs_refresh(element, "source", &runtime_names);
                let alt_dynamic = android_ui_property_needs_refresh(element, "alt", &runtime_names);
                let can_shrink_dynamic =
                    android_ui_property_needs_refresh(element, "can_shrink", &runtime_names);
                if source_dynamic {
                    let source = view_property(element, "source")
                        .map(|property| ui_expr_c(&property.value, view, signatures))
                        .transpose()?
                        .unwrap_or_else(|| c_string(""));
                    let alt = view_property(element, "alt")
                        .map(|property| ui_expr_c(&property.value, view, signatures))
                        .transpose()?
                        .unwrap_or_else(|| c_string(""));
                    let fit = view_property(element, "fit")
                        .and_then(|property| static_expr_str(&property.value, signatures))
                        .unwrap_or_else(|| "contain".to_string());
                    let can_shrink = view_property(element, "can_shrink")
                        .map(|property| ui_expr_c(&property.value, view, signatures))
                        .transpose()?
                        .unwrap_or_else(|| "false".to_string());
                    out.push_str(&format!(
                        "                jstring refresh_image_source = flux__android_utf8_string(env, {source});\n"
                    ));
                    out.push_str(&format!(
                        "                jstring refresh_image_alt = flux__android_utf8_string(env, {alt});\n"
                    ));
                    out.push_str(&format!(
                        "                jstring refresh_image_fit = flux__android_utf8_string(env, {});\n",
                        c_string(&fit)
                    ));
                    out.push_str("                if (refresh_image_source != NULL && refresh_image_alt != NULL && refresh_image_fit != NULL) {\n");
                    out.push_str("                    jmethodID refresh_image = (*env)->GetMethodID(env, activity_class, \"configureImage\", \"(Landroid/widget/ImageView;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Z)V\");\n");
                    out.push_str(&format!(
                        "                    if (refresh_image != NULL) (*env)->CallVoidMethod(env, activity, refresh_image, child, refresh_image_source, refresh_image_fit, refresh_image_alt, (jboolean)({can_shrink}));\n"
                    ));
                    out.push_str("                }\n");
                    out.push_str("                if (refresh_image_source != NULL) (*env)->DeleteLocalRef(env, refresh_image_source);\n");
                    out.push_str("                if (refresh_image_alt != NULL) (*env)->DeleteLocalRef(env, refresh_image_alt);\n");
                    out.push_str("                if (refresh_image_fit != NULL) (*env)->DeleteLocalRef(env, refresh_image_fit);\n");
                } else {
                    if can_shrink_dynamic
                        && let Some(property) = view_property(element, "can_shrink")
                    {
                        let value = ui_expr_c(&property.value, view, signatures)?;
                        out.push_str("                jmethodID refresh_image_can_shrink = (*env)->GetMethodID(env, child_class, \"setAdjustViewBounds\", \"(Z)V\");\n");
                        out.push_str(&format!(
                            "                if (refresh_image_can_shrink != NULL) (*env)->CallVoidMethod(env, child, refresh_image_can_shrink, (jboolean)({value}));\n"
                        ));
                    }
                    if alt_dynamic && let Some(property) = view_property(element, "alt") {
                        let value = ui_expr_c(&property.value, view, signatures)?;
                        out.push_str(&format!(
                            "                jstring refresh_image_alt = flux__android_utf8_string(env, {value});\n"
                        ));
                        out.push_str("                if (refresh_image_alt != NULL) {\n");
                        out.push_str("                    jmethodID refresh_image_content_description = (*env)->GetMethodID(env, child_class, \"setContentDescription\", \"(Ljava/lang/CharSequence;)V\");\n");
                        out.push_str("                    if (refresh_image_content_description != NULL) (*env)->CallVoidMethod(env, child, refresh_image_content_description, refresh_image_alt);\n");
                        out.push_str(
                            "                    (*env)->DeleteLocalRef(env, refresh_image_alt);\n",
                        );
                        out.push_str("                }\n");
                    }
                }
            }
            _ => {}
        }

        let accessibility_needs_refresh =
            android_ui_property_needs_refresh(element, "accessibility_label", &runtime_names)
                || android_ui_property_needs_refresh(
                    element,
                    "accessibility_description",
                    &runtime_names,
                );
        if accessibility_needs_refresh {
            if let Some(property) = view_property(element, "accessibility_label") {
                let value = ui_expr_c(&property.value, view, signatures)?;
                out.push_str(&format!(
                    "                jstring refresh_accessibility_label = flux__android_utf8_string(env, {value});\n"
                ));
            } else {
                out.push_str("                jstring refresh_accessibility_label = NULL;\n");
            }
            if let Some(property) = view_property(element, "accessibility_description") {
                let value = ui_expr_c(&property.value, view, signatures)?;
                out.push_str(&format!(
                    "                jstring refresh_accessibility_description = flux__android_utf8_string(env, {value});\n"
                ));
            } else {
                out.push_str("                jstring refresh_accessibility_description = NULL;\n");
            }
            out.push_str("                jmethodID refresh_accessibility = (*env)->GetMethodID(env, activity_class, \"setAccessibility\", \"(Landroid/view/View;Ljava/lang/String;Ljava/lang/String;)V\");\n");
            out.push_str("                if (refresh_accessibility != NULL) (*env)->CallVoidMethod(env, activity, refresh_accessibility, child, refresh_accessibility_label, refresh_accessibility_description);\n");
            out.push_str("                if (refresh_accessibility_label != NULL) (*env)->DeleteLocalRef(env, refresh_accessibility_label);\n");
            out.push_str("                if (refresh_accessibility_description != NULL) (*env)->DeleteLocalRef(env, refresh_accessibility_description);\n");
        }
        if android_ui_property_needs_refresh(element, "accessibility_hidden", &runtime_names)
            && let Some(property) = view_property(element, "accessibility_hidden")
        {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str("                jmethodID refresh_accessibility_hidden = (*env)->GetMethodID(env, activity_class, \"setAccessibilityHidden\", \"(Landroid/view/View;Z)V\");\n");
            out.push_str(&format!(
                "                if (refresh_accessibility_hidden != NULL) (*env)->CallVoidMethod(env, activity, refresh_accessibility_hidden, child, (jboolean)({value}));\n"
            ));
        }
        if android_ui_property_needs_refresh(element, "focusable", &runtime_names)
            && let Some(property) = view_property(element, "focusable")
        {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str("                jmethodID refresh_focusable = (*env)->GetMethodID(env, child_class, \"setFocusable\", \"(Z)V\");\n");
            out.push_str("                jmethodID refresh_focusable_in_touch_mode = (*env)->GetMethodID(env, child_class, \"setFocusableInTouchMode\", \"(Z)V\");\n");
            out.push_str(&format!(
                "                if (refresh_focusable != NULL) (*env)->CallVoidMethod(env, child, refresh_focusable, (jboolean)({value}));\n"
            ));
            out.push_str(&format!(
                "                if (refresh_focusable_in_touch_mode != NULL) (*env)->CallVoidMethod(env, child, refresh_focusable_in_touch_mode, (jboolean)({value}));\n"
            ));
        }

        out.push_str("                (*env)->DeleteLocalRef(env, child_class);\n");
        out.push_str("            }\n");
        out.push_str("            (*env)->DeleteLocalRef(env, child);\n");
        out.push_str("        } else if ((*env)->ExceptionCheck(env)) {\n");
        out.push_str("            (*env)->ExceptionClear(env);\n");
        out.push_str("        }\n");
        out.push_str("    }\n");
    }
    out.push_str("    (*env)->DeleteLocalRef(env, activity_class);\n");
    out.push_str("}\n\n");
    out.push_str("JNIEXPORT void JNICALL Java_app_flux_runtime_FluxActivity_nativeRefreshUi(JNIEnv *env, jobject activity) { flux__android_ui_refresh(env, activity, -1); }\n\n");
    Ok(())
}

fn android_ui_zero_arg_event_body(
    action: &crate::ast::ViewProperty,
    view: &crate::ast::ViewDef,
    signatures: &Signatures,
) -> Result<String, Diagnostic> {
    let refresh = "if (flux__android_activity != NULL) Java_app_flux_runtime_FluxActivity_nativeRefreshUi(env, flux__android_activity->clazz);";
    if let Some(transition) = &action.transition {
        let next = ui_expr_c(&action.value, view, signatures)?;
        let state_index = ui_state_index(view, &transition.state);
        return Ok(format!(
            "{} = {next}; if (flux__android_activity != NULL) flux__android_ui_refresh(env, flux__android_activity->clazz, {state_index});",
            ui_state_c_name(&transition.state)
        ));
    }
    let ExprKind::Var(function) = &action.value.kind else {
        return Err(diag(
            action.value.span,
            "bootstrap Android UI event lowering requires a named fn() -> void callback or state transition",
        ));
    };
    Ok(format!("{}(); {refresh}", function_c_name(function)))
}

fn ui_zero_arg_event_body(
    action: &crate::ast::ViewProperty,
    view: &crate::ast::ViewDef,
    signatures: &Signatures,
) -> Result<String, Diagnostic> {
    if let Some(transition) = &action.transition {
        let next = ui_expr_c(&action.value, view, signatures)?;
        let state_index = ui_state_index(view, &transition.state);
        return Ok(format!(
            "{} = {next}; flux__ui_refresh_changed({state_index});",
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

fn fold_ui_primitive_expr(
    expr: &Expr,
    signatures: &Signatures,
) -> Result<Option<ConstantValue>, Diagnostic> {
    match &expr.kind {
        ExprKind::Var(name) => {
            if let Some(value) = typecheck::semantic_ui_i64_token(name) {
                Ok(Some(ConstantValue::I64(value)))
            } else {
                fold_primitive_expr(expr, &HashMap::new(), signatures)
            }
        }
        ExprKind::Unary { op, expr: inner } => {
            let Some(inner) = fold_ui_primitive_expr(inner, signatures)? else {
                return Ok(None);
            };
            Ok(Some(match (op, inner) {
                (UnaryOp::Neg, ConstantValue::I64(value)) => {
                    ConstantValue::I64(value.checked_neg().ok_or_else(|| {
                        diag(expr.span, "constant integer negation overflows i64")
                    })?)
                }
                (UnaryOp::Not, ConstantValue::Bool(value)) => ConstantValue::Bool(!value),
                _ => return Ok(None),
            }))
        }
        ExprKind::Binary { left, op, right } => {
            let Some(left) = fold_ui_primitive_expr(left, signatures)? else {
                return Ok(None);
            };
            if matches!(op, BinOp::And) && left == ConstantValue::Bool(false) {
                return Ok(Some(ConstantValue::Bool(false)));
            }
            if matches!(op, BinOp::Or) && left == ConstantValue::Bool(true) {
                return Ok(Some(ConstantValue::Bool(true)));
            }
            let Some(right) = fold_ui_primitive_expr(right, signatures)? else {
                return Ok(None);
            };
            Ok(Some(fold_primitive_binary(expr.span, *op, left, right)?))
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            let Some(condition) = fold_ui_primitive_expr(cond, signatures)? else {
                return Ok(None);
            };
            match condition {
                ConstantValue::Bool(true) => fold_ui_primitive_expr(then_expr, signatures),
                ConstantValue::Bool(false) => fold_ui_primitive_expr(else_expr, signatures),
                _ => Ok(None),
            }
        }
        _ => fold_primitive_expr(expr, &HashMap::new(), signatures),
    }
}

fn static_expr_i64(expr: &Expr, signatures: &Signatures) -> Option<i64> {
    match fold_ui_primitive_expr(expr, signatures).ok().flatten()? {
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

fn is_semantic_ui_color(value: &str) -> bool {
    crate::typecheck::SEMANTIC_UI_COLOR_TOKENS.contains(&value)
}

fn valid_ui_color(value: &str) -> bool {
    crate::typecheck::valid_ui_color(value)
}

fn gtk_ui_color_css(value: &str) -> Option<&str> {
    match value {
        "surface" => Some("@flux_surface"),
        "surfaceRaised" => Some("@flux_surface_raised"),
        "text" => Some("@flux_text"),
        "textMuted" => Some("@flux_text_muted"),
        "accent" => Some("@flux_accent"),
        "onAccent" => Some("@flux_on_accent"),
        "outline" => Some("@flux_outline"),
        "danger" => Some("@flux_danger"),
        "success" => Some("@flux_success"),
        "warning" => Some("@flux_warning"),
        "shadow" => Some("@flux_shadow"),
        _ => None,
    }
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
    match fold_ui_primitive_expr(expr, signatures).ok().flatten()? {
        ConstantValue::Bool(value) => Some(value),
        _ => None,
    }
}

fn ui_expr_c(
    expr: &Expr,
    view: &crate::ast::ViewDef,
    signatures: &Signatures,
) -> Result<String, Diagnostic> {
    if let Some(value) = fold_ui_primitive_expr(expr, signatures)? {
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
            if let Some(value) = typecheck::semantic_ui_i64_token(name) {
                return Ok(format!("INT64_C({value})"));
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
                    "bootstrap dynamic UI expression may reference only view environment, view state, derived view values, or compile-time constants",
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
            "bootstrap dynamic UI expression currently supports primitive literals, view environment/state/constants, primitive operators, and conditional expressions",
        )),
    }
}

fn emit_ui_refresh(
    out: &mut String,
    view: &crate::ast::ViewDef,
    signatures: &Signatures,
) -> Result<(), Diagnostic> {
    let runtime_dependencies = ui_runtime_dependency_map(view);
    out.push_str("static void flux__ui_refresh_changed(int changed_state) {\n");
    for derived in &view.derived {
        let value = ui_expr_c(&derived.value, view, signatures)?;
        let dependencies = runtime_dependencies
            .get(&derived.name)
            .expect("derived runtime dependency metadata exists");
        let condition = ui_refresh_condition(dependencies);
        out.push_str(&format!(
            "    if ({condition}) {} = {value};\n",
            ui_derived_c_name(&derived.name)
        ));
    }
    for element in &view.elements {
        let dependencies = ui_element_refresh_dependencies(element, &runtime_dependencies);
        let condition = ui_refresh_condition(&dependencies);
        out.push_str(&format!("    if ({condition}) {{\n"));
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
        if let Some(property) = view_property(element, "accessibility_hidden") {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str(&format!(
                "    if ({widget} != NULL) gtk_accessible_update_state(GTK_ACCESSIBLE({widget}), GTK_ACCESSIBLE_STATE_HIDDEN, {value}, -1);\n"
            ));
        }
        if let Some(property) = view_property(element, "focusable") {
            let value = ui_expr_c(&property.value, view, signatures)?;
            out.push_str(&format!(
                "    if ({widget} != NULL) gtk_widget_set_focusable({widget}, {value});\n"
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
        out.push_str("    }\n");
    }
    out.push_str("}\n\n");
    out.push_str("static inline void flux__ui_refresh(void) { flux__ui_refresh_changed(-1); }\n\n");
    Ok(())
}

fn text_semantic_typography(
    element: &crate::ast::ViewElement,
    signatures: &Signatures,
) -> Result<(i64, bool, i64), Diagnostic> {
    let variant = match view_property(element, "variant") {
        Some(property) => static_expr_str(&property.value, signatures).ok_or_else(|| {
            diag(
                property.value.span,
                "Text.variant must be a compile-time string",
            )
        })?,
        None => "body".to_string(),
    };
    match variant.as_str() {
        "body" => Ok((16, false, 140)),
        "caption" => Ok((13, false, 135)),
        "heading" => Ok((20, true, 125)),
        "title" => Ok((28, true, 120)),
        "display" => Ok((36, true, 115)),
        _ => Err(diag(
            view_property(element, "variant")
                .expect("non-default variant has a source property")
                .value
                .span,
            "Text.variant must be one of 'body', 'caption', 'heading', 'title', or 'display'",
        )),
    }
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
    let mut color_properties = vec![
        ("background_color", "background-color"),
        ("border_color", "border-color"),
        ("border_top_color", "border-top-color"),
        ("border_bottom_color", "border-bottom-color"),
        ("border_start_color", "border-left-color"),
        ("border_end_color", "border-right-color"),
    ];
    if element.kind == "Text" {
        color_properties.push(("color", "color"));
    }
    for (property_name, css_name) in color_properties {
        let Some(property) = view_property(element, property_name) else {
            continue;
        };
        let Some(value) = static_expr_str(&property.value, signatures) else {
            return Err(diag(
                property.value.span,
                &format!("{property_name} must be a compile-time string"),
            ));
        };
        if !valid_ui_color(&value) {
            return Err(diag(
                property.value.span,
                &format!(
                    "{property_name} must use '#RRGGBB', '#RRGGBBAA', or a semantic Flux color token"
                ),
            ));
        }
        let css_value = gtk_ui_color_css(&value).unwrap_or(value.as_str());
        declarations.push(format!("{css_name}: {css_value};"));
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
            if !valid_ui_color(&value) {
                return Err(diag(
                    property.value.span,
                    "shadow_color must use '#RRGGBB', '#RRGGBBAA', or a semantic Flux color token",
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
            gtk_ui_color_css(shadow_color.as_deref().unwrap_or("shadow"))
                .unwrap_or_else(|| shadow_color.as_deref().unwrap_or("#00000080"))
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
    let min_height = static_minimum_size(element, "min_height", signatures)?.or_else(|| {
        (fixed_height.is_none()
            && matches!(
                element.kind.as_str(),
                "Button" | "TextInput" | "Toggle" | "Radio"
            ))
        .then_some(40)
    });
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
                        let mut pattern_conditions = Vec::new();
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
                                MatchPattern::Relational(_) | MatchPattern::Logical { .. } => {
                                    if let Some(condition) = emit_match_pattern_condition(
                                        pattern,
                                        &payload_access,
                                        payload_ty,
                                        &nested,
                                        signatures,
                                    )? {
                                        pattern_conditions.push(condition);
                                    }
                                }
                            }
                        }
                        if let Some(guard) = &arm.guard {
                            let guard = emit_expr(guard, &nested, signatures)?;
                            pattern_conditions.push(c_condition(&guard.code));
                        }
                        if pattern_conditions.is_empty() {
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
                        } else {
                            out.push_str(&format!(
                                "{pad}            if ({}) {{\n",
                                pattern_conditions.join(" && ")
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
            let mut pattern_conditions = Vec::new();
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
                    MatchPattern::Relational(_) | MatchPattern::Logical { .. } => {
                        if let Some(condition) = emit_match_pattern_condition(
                            pattern,
                            &payload_access,
                            payload_ty,
                            &nested,
                            signatures,
                        )? {
                            pattern_conditions.push(condition);
                        }
                    }
                }
            }
            if let Some(guard) = &arm.guard {
                let guard = emit_expr(guard, &nested, signatures)?;
                pattern_conditions.push(c_condition(&guard.code));
            }
            if pattern_conditions.is_empty() {
                let arm_value = emit_expr(&arm.value, &nested, signatures)?;
                out.push_str(&format!(
                    "{pad}            {target} = {};\n",
                    arm_value.code
                ));
                out.push_str(&format!("{pad}            break;\n"));
            } else {
                out.push_str(&format!(
                    "{pad}            if ({}) {{\n",
                    pattern_conditions.join(" && ")
                ));
                let arm_value = emit_expr(&arm.value, &nested, signatures)?;
                out.push_str(&format!(
                    "{pad}                {target} = {};\n",
                    arm_value.code
                ));
                out.push_str(&format!("{pad}                break;\n"));
                out.push_str(&format!("{pad}            }}\n"));
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
    if namespace == "process" {
        if !named_args.is_empty() {
            return Err(diag(span, "invalid process call reached code generation"));
        }
        match name {
            "pid" | "parentPid" => {
                if !args.is_empty() {
                    return Err(diag(span, "invalid process call reached code generation"));
                }
                let helper = if name == "pid" {
                    "flux__process_pid"
                } else {
                    "flux__process_parent_pid"
                };
                return Ok((format!("{helper}()"), vec![Type::I64], None));
            }
            "terminationRequested" => {
                if !args.is_empty() {
                    return Err(diag(span, "invalid process call reached code generation"));
                }
                return Ok((
                    "flux__process_termination_requested()".to_string(),
                    vec![Type::Bool],
                    None,
                ));
            }
            "exit" => {
                if args.len() != 1 {
                    return Err(diag(span, "invalid process call reached code generation"));
                }
                let code = emit_expr(&args[0], env, signatures)?;
                return Ok((
                    format!("flux__process_exit({})", code.code),
                    Vec::new(),
                    None,
                ));
            }
            "hasEnv" => {
                if args.len() != 1 {
                    return Err(diag(span, "invalid process call reached code generation"));
                }
                let value = emit_expr(&args[0], env, signatures)?;
                return Ok((
                    format!("flux__process_has_env({})", value.code),
                    vec![Type::Bool],
                    None,
                ));
            }
            "env" => {
                if args.len() != 2 {
                    return Err(diag(span, "invalid process call reached code generation"));
                }
                let name = emit_expr(&args[0], env, signatures)?;
                let fallback = emit_expr(&args[1], env, signatures)?;
                return Ok((
                    format!("flux__process_env({}, {})", name.code, fallback.code),
                    vec![Type::Str],
                    None,
                ));
            }
            _ => {
                return Err(diag(span, "unknown process call reached code generation"));
            }
        }
    }
    if namespace == "locale" {
        if !named_args.is_empty() || !args.is_empty() {
            return Err(diag(span, "invalid locale call reached code generation"));
        }
        let helper = match name {
            "language" => "flux__locale_language",
            "region" => "flux__locale_region",
            _ => return Err(diag(span, "unknown locale call reached code generation")),
        };
        return Ok((format!("{helper}()"), vec![Type::Str], None));
    }
    if namespace == "time" {
        if !named_args.is_empty() {
            return Err(diag(span, "invalid time call reached code generation"));
        }
        match name {
            "unixMillis" | "monotonicMillis" => {
                if !args.is_empty() {
                    return Err(diag(span, "invalid time call reached code generation"));
                }
                let helper = if name == "unixMillis" {
                    "flux__time_unix_millis"
                } else {
                    "flux__time_monotonic_millis"
                };
                return Ok((format!("{helper}()"), vec![Type::I64], None));
            }
            "sleepMillis" => {
                if args.len() != 1 {
                    return Err(diag(span, "invalid time call reached code generation"));
                }
                let duration = emit_expr(&args[0], env, signatures)?;
                return Ok((
                    format!("flux__time_sleep_millis({})", duration.code),
                    Vec::new(),
                    None,
                ));
            }
            "utcYear" | "utcMonth" | "utcDay" | "utcHour" | "utcMinute" | "utcSecond"
            | "utcMillisecond" | "utcWeekday" | "utcDayOfYear" => {
                if args.len() != 1 {
                    return Err(diag(span, "invalid time call reached code generation"));
                }
                let unix_millis = emit_expr(&args[0], env, signatures)?;
                let part = match name {
                    "utcYear" => 0,
                    "utcMonth" => 1,
                    "utcDay" => 2,
                    "utcHour" => 3,
                    "utcMinute" => 4,
                    "utcSecond" => 5,
                    "utcMillisecond" => 6,
                    "utcWeekday" => 7,
                    "utcDayOfYear" => 8,
                    _ => unreachable!(),
                };
                return Ok((
                    format!("flux__time_utc_part({}, {part})", unix_millis.code),
                    vec![Type::I64],
                    None,
                ));
            }
            _ => return Err(diag(span, "unknown time call reached code generation")),
        }
    }
    if namespace == "fs" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                "invalid filesystem call reached code generation",
            ));
        }
        match name {
            "exists" | "isFile" | "isDirectory" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        "invalid filesystem call reached code generation",
                    ));
                }
                let path = emit_expr(&args[0], env, signatures)?;
                let helper = match name {
                    "exists" => "flux__fs_exists",
                    "isFile" => "flux__fs_is_file",
                    _ => "flux__fs_is_directory",
                };
                return Ok((format!("{helper}({})", path.code), vec![Type::Bool], None));
            }
            "createDirectory" | "removeFile" | "removeDirectory" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        "invalid filesystem call reached code generation",
                    ));
                }
                let path = emit_expr(&args[0], env, signatures)?;
                let helper = match name {
                    "createDirectory" => "flux__fs_create_directory",
                    "removeFile" => "flux__fs_remove_file",
                    _ => "flux__fs_remove_directory",
                };
                return Ok((format!("{helper}({})", path.code), vec![Type::Error], None));
            }
            "writeText" | "appendText" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        "invalid filesystem call reached code generation",
                    ));
                }
                let path = emit_expr(&args[0], env, signatures)?;
                let text = emit_expr(&args[1], env, signatures)?;
                let helper = if name == "writeText" {
                    "flux__fs_write_text"
                } else {
                    "flux__fs_append_text"
                };
                return Ok((
                    format!("{helper}({}, {})", path.code, text.code),
                    vec![Type::Error],
                    None,
                ));
            }
            "rename" | "copyFile" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        "invalid filesystem call reached code generation",
                    ));
                }
                let source = emit_expr(&args[0], env, signatures)?;
                let destination = emit_expr(&args[1], env, signatures)?;
                let helper = if name == "rename" {
                    "flux__fs_rename"
                } else {
                    "flux__fs_copy_file"
                };
                return Ok((
                    format!("{helper}({}, {})", source.code, destination.code),
                    vec![Type::Error],
                    None,
                ));
            }
            _ => {
                return Err(diag(
                    span,
                    "unknown filesystem call reached code generation",
                ));
            }
        }
    }
    if namespace == "android" {
        if !named_args.is_empty() {
            return Err(diag(
                span,
                "invalid android platform call reached code generation",
            ));
        }
        match name {
            "sdkInt" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                return Ok(("flux__android_sdk_int()".to_string(), vec![Type::I64], None));
            }
            "permissionGranted" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                let value = emit_expr(&args[0], env, signatures)?;
                return Ok((
                    format!("flux__android_permission_granted({})", value.code),
                    vec![Type::Bool],
                    None,
                ));
            }
            "requestPermission" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                let value = emit_expr(&args[0], env, signatures)?;
                return Ok((
                    format!("flux__android_request_permission({})", value.code),
                    Vec::new(),
                    None,
                ));
            }
            "notificationPermissionGranted" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                return Ok((
                    "flux__android_notification_permission_granted()".to_string(),
                    vec![Type::Bool],
                    None,
                ));
            }
            "requestNotificationPermission" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                return Ok((
                    "flux__android_request_notification_permission()".to_string(),
                    Vec::new(),
                    None,
                ));
            }
            "showKeyboard" | "hideKeyboard" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                let runtime_name = if name == "showKeyboard" {
                    "show_keyboard"
                } else {
                    "hide_keyboard"
                };
                return Ok((format!("flux__android_{runtime_name}()"), Vec::new(), None));
            }
            "focusNext" | "focusPrevious" => {
                if args.len() > 1 {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                let runtime_name = if name == "focusNext" {
                    "focus_next"
                } else {
                    "focus_previous"
                };
                if let Some(wrap) = args.first() {
                    let wrap = emit_expr(wrap, env, signatures)?;
                    return Ok((
                        format!("flux__android_{runtime_name}_wrap({})", wrap.code),
                        Vec::new(),
                        None,
                    ));
                }
                return Ok((format!("flux__android_{runtime_name}()"), Vec::new(), None));
            }
            "focusFirst" | "focusLast" | "clearFocus" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                let runtime_name = match name {
                    "focusFirst" => "focus_first",
                    "focusLast" => "focus_last",
                    "clearFocus" => "clear_focus",
                    _ => unreachable!(),
                };
                return Ok((format!("flux__android_{runtime_name}()"), Vec::new(), None));
            }
            "selectionStart" | "selectionEnd" => {
                if !args.is_empty() {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                let runtime_name = if name == "selectionStart" {
                    "selection_start"
                } else {
                    "selection_end"
                };
                return Ok((
                    format!("flux__android_{runtime_name}()"),
                    vec![Type::I64],
                    None,
                ));
            }
            "setCaret" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                let position = emit_expr(&args[0], env, signatures)?;
                return Ok((
                    format!("flux__android_set_caret({})", position.code),
                    vec![Type::Bool],
                    None,
                ));
            }
            "setSelection" => {
                if args.len() != 2 {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                let start = emit_expr(&args[0], env, signatures)?;
                let end = emit_expr(&args[1], env, signatures)?;
                return Ok((
                    format!("flux__android_set_selection({}, {})", start.code, end.code),
                    vec![Type::Bool],
                    None,
                ));
            }
            "vibrate" | "openUrl" | "share" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                let value = emit_expr(&args[0], env, signatures)?;
                let code = match name {
                    "vibrate" => format!("flux__android_vibrate({})", value.code),
                    "openUrl" => format!("flux__android_open_url({})", value.code),
                    "share" => format!("flux__android_share({})", value.code),
                    _ => unreachable!(),
                };
                return Ok((code, Vec::new(), None));
            }
            "createNotificationChannel" => {
                if args.len() != 3 {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                let values = args
                    .iter()
                    .map(|arg| emit_expr(arg, env, signatures).map(|value| value.code))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok((
                    format!(
                        "flux__android_create_notification_channel({}, {}, {})",
                        values[0], values[1], values[2]
                    ),
                    Vec::new(),
                    None,
                ));
            }
            "notify" => {
                if args.len() != 4 {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                let values = args
                    .iter()
                    .map(|arg| emit_expr(arg, env, signatures).map(|value| value.code))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok((
                    format!(
                        "flux__android_notify({}, {}, {}, {})",
                        values[0], values[1], values[2], values[3]
                    ),
                    Vec::new(),
                    None,
                ));
            }
            "notifyUrlAction" => {
                if args.len() != 6 {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                let values = args
                    .iter()
                    .map(|arg| emit_expr(arg, env, signatures).map(|value| value.code))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok((
                    format!(
                        "flux__android_notify_url_action({}, {}, {}, {}, {}, {})",
                        values[0], values[1], values[2], values[3], values[4], values[5]
                    ),
                    Vec::new(),
                    None,
                ));
            }
            "cancelNotification" => {
                if args.len() != 1 {
                    return Err(diag(
                        span,
                        "invalid android platform call reached code generation",
                    ));
                }
                let value = emit_expr(&args[0], env, signatures)?;
                return Ok((
                    format!("flux__android_cancel_notification({})", value.code),
                    Vec::new(),
                    None,
                ));
            }
            _ => {
                return Err(diag(
                    span,
                    "unknown android platform call reached code generation",
                ));
            }
        }
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

fn emit_match_pattern_condition(
    pattern: &MatchPattern,
    payload_access: &str,
    payload_ty: &Type,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<Option<String>, Diagnostic> {
    match pattern {
        MatchPattern::Binding(_) | MatchPattern::Struct(_) => Ok(None),
        MatchPattern::Relational(pattern) => {
            let right = emit_expr(&pattern.value, env, signatures)?;
            let payload_ty = signatures.canonical_type(payload_ty);
            let condition =
                if payload_ty == Type::Str && matches!(pattern.op, BinOp::Eq | BinOp::Ne) {
                    let comparator = if pattern.op == BinOp::Eq { "==" } else { "!=" };
                    format!("(strcmp({payload_access}, {}) {comparator} 0)", right.code)
                } else {
                    format!(
                        "({payload_access} {} {})",
                        c_operator(pattern.op),
                        right.code
                    )
                };
            Ok(Some(condition))
        }
        MatchPattern::Logical {
            left, op, right, ..
        } => {
            let left =
                emit_match_pattern_condition(left, payload_access, payload_ty, env, signatures)?
                    .expect("logical match pattern sides are relational");
            let right =
                emit_match_pattern_condition(right, payload_access, payload_ty, env, signatures)?
                    .expect("logical match pattern sides are relational");
            let op = match op {
                PatternLogicalOp::And => "&&",
                PatternLogicalOp::Or => "||",
            };
            Ok(Some(format!("({left} {op} {right})")))
        }
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
