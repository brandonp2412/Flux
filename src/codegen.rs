use std::collections::{HashMap, HashSet};

use crate::ast::{
    BinOp, EnumDef, Expr, ExprKind, Function, MatchPattern, NamedArg, Program, ShellRedirectMode,
    Stmt, StmtKind, StructDef, StructPatternField, Type, UnaryOp,
};
use crate::diagnostic::{Diagnostic, DiagnosticStage, SourceSpan};
use crate::typecheck::{ConstantValue, Signature, Signatures, type_of_expr};

pub fn emit_c(program: &Program, signatures: &Signatures) -> Result<String, Diagnostic> {
    let mut out = String::new();
    out.push_str("#include <stdbool.h>\n");
    out.push_str("#include <stdint.h>\n");
    out.push_str("#include <stddef.h>\n");
    out.push_str("#include <stdio.h>\n");
    out.push_str("#include <stdlib.h>\n");
    out.push_str("#include <string.h>\n");
    if program_uses_background(program) {
        out.push_str("#include <errno.h>\n");
        out.push_str("#include <sys/types.h>\n");
        out.push_str("#include <sys/wait.h>\n");
        out.push_str("#include <unistd.h>\n");
    }
    if program.application.is_some() {
        out.push_str("#include <gtk/gtk.h>\n");
    }
    out.push('\n');
    out.push_str("static inline void flux_print_i64(int64_t value) { printf(\"%lld\\n\", (long long)value); }\n");
    out.push_str(
        "static inline void flux_print_bool(bool value) { puts(value ? \"true\" : \"false\"); }\n",
    );
    out.push_str("static inline void flux_print_str(const char *value) { puts(value); }\n");
    out.push_str("static inline void flux_print_error(const char *value) { puts(value ? value : \"nil\"); }\n");
    out.push_str("static inline FILE *flux_open_redirect(const char *path, bool append) { FILE *file = fopen(path, append ? \"a\" : \"w\"); if (file == NULL) { perror(path); abort(); } return file; }\n");
    out.push_str("static inline void flux_redirect_i64(const char *path, bool append, int64_t value) { FILE *file = flux_open_redirect(path, append); fprintf(file, \"%lld\\n\", (long long)value); fclose(file); }\n");
    out.push_str("static inline void flux_redirect_bool(const char *path, bool append, bool value) { FILE *file = flux_open_redirect(path, append); fputs(value ? \"true\\n\" : \"false\\n\", file); fclose(file); }\n");
    out.push_str("static inline void flux_redirect_str(const char *path, bool append, const char *value) { FILE *file = flux_open_redirect(path, append); fputs(value, file); fputc('\\n', file); fclose(file); }\n");
    out.push_str("static inline void flux_redirect_error(const char *path, bool append, const char *value) { FILE *file = flux_open_redirect(path, append); fputs(value ? value : \"nil\", file); fputc('\\n', file); fclose(file); }\n");
    out.push_str("static inline bool flux_error_eq(const char *a, const char *b) { return a == NULL ? b == NULL : b != NULL && strcmp(a, b) == 0; }\n");
    out.push_str("struct flux__list { void *data; size_t len; ptrdiff_t stride; };\n");
    out.push_str("static inline ptrdiff_t flux_list_stride(struct flux__list list, size_t elem_size) { return list.stride == 0 ? (ptrdiff_t)elem_size : list.stride; }\n");
    out.push_str("static inline size_t flux_list_index(size_t len, int64_t index) { int64_t resolved = index; if (resolved < 0) resolved += (int64_t)len; if (resolved < 0 || (uint64_t)resolved >= (uint64_t)len) { fputs(\"Flux runtime error: list index out of range\\n\", stderr); abort(); } return (size_t)resolved; }\n");
    out.push_str("static inline void *flux_list_at(struct flux__list list, int64_t index, size_t elem_size) { ptrdiff_t stride = flux_list_stride(list, elem_size); return (void *)((char *)list.data + (ptrdiff_t)flux_list_index(list.len, index) * stride); }\n");
    out.push_str("static inline void *flux_list_single(struct flux__list list) { if (list.len != 1) { fputs(\"Flux runtime error: list.single requires exactly one element\\n\", stderr); abort(); } return list.data; }\n");
    out.push_str("static inline size_t flux_list_count(size_t len, int64_t count) { if (count < 0) { fputs(\"Flux runtime error: list count must be non-negative\\n\", stderr); abort(); } uint64_t value = (uint64_t)count; return value > (uint64_t)len ? len : (size_t)value; }\n");
    out.push_str("static inline struct flux__list flux_list_take(struct flux__list list, int64_t count) { list.len = flux_list_count(list.len, count); return list; }\n");
    out.push_str("static inline struct flux__list flux_list_skip(struct flux__list list, int64_t count, size_t elem_size) { size_t skipped = flux_list_count(list.len, count); ptrdiff_t stride = flux_list_stride(list, elem_size); list.data = (void *)((char *)list.data + (ptrdiff_t)skipped * stride); list.len -= skipped; list.stride = stride; return list; }\n");
    out.push_str("static inline bool flux_list_any_bool(struct flux__list list) { for (size_t i = 0; i < list.len; ++i) { if (*((bool *)flux_list_at(list, (int64_t)i, sizeof(bool)))) return true; } return false; }\n");
    out.push_str("static inline bool flux_list_every_bool(struct flux__list list) { for (size_t i = 0; i < list.len; ++i) { if (!*((bool *)flux_list_at(list, (int64_t)i, sizeof(bool)))) return false; } return true; }\n");
    out.push_str("static inline int64_t flux_slice_bound(size_t len, bool present, int64_t value, bool end, int64_t step) { if (step > 0) { if (!present) return end ? (int64_t)len : 0; int64_t resolved = value; if (resolved < 0) resolved += (int64_t)len; if (resolved < 0) return 0; if ((uint64_t)resolved > (uint64_t)len) return (int64_t)len; return resolved; } if (!present) return end ? -1 : (len == 0 ? -1 : (int64_t)len - 1); int64_t resolved = value; if (resolved < 0) resolved += (int64_t)len; if (resolved < 0) return -1; if ((uint64_t)resolved >= (uint64_t)len) return len == 0 ? -1 : (int64_t)len - 1; return resolved; }\n");
    out.push_str("static inline struct flux__list flux_list_slice(struct flux__list list, bool has_start, int64_t start, bool has_end, int64_t end, int64_t step, size_t elem_size) { if (step == 0) { fputs(\"Flux runtime error: list slice step cannot be zero\\n\", stderr); abort(); } if (step > (int64_t)PTRDIFF_MAX || step < (int64_t)PTRDIFF_MIN) { fputs(\"Flux runtime error: list slice step is too large\\n\", stderr); abort(); } int64_t first = flux_slice_bound(list.len, has_start, start, false, step); int64_t last = flux_slice_bound(list.len, has_end, end, true, step); size_t count = 0; if (step > 0 && first < last) { count = (size_t)(1 + (uint64_t)(last - 1 - first) / (uint64_t)step); } else if (step < 0 && first > last) { uint64_t magnitude = (uint64_t)(-(step + 1)) + 1; count = (size_t)(1 + (uint64_t)(first - 1 - last) / magnitude); } ptrdiff_t base_stride = flux_list_stride(list, elem_size); ptrdiff_t next_stride = 0; if (__builtin_mul_overflow(base_stride, (ptrdiff_t)step, &next_stride)) { fputs(\"Flux runtime error: list slice stride overflow\\n\", stderr); abort(); } void *data = list.data; if (count != 0) data = (void *)((char *)list.data + (ptrdiff_t)first * base_stride); struct flux__list result = { .data = data, .len = count, .stride = next_stride }; return result; }\n");
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
    for definition in &program.interfaces {
        out.push_str(&format!("struct {};\n", interface_c_name(&definition.name)));
    }
    if !program.structs.is_empty() || !program.enums.is_empty() || !program.interfaces.is_empty() {
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

    emit_interface_value_definitions(&mut out, program, signatures);
    out.push_str(&interface_pack_helpers(program, signatures));
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
    emit_interface_multi_return_structs(&mut out, program, signatures);

    for function in &program.functions {
        out.push_str(&function_prototype(function, signatures));
        out.push_str(";\n");
    }
    out.push('\n');
    emit_interface_dispatch_helpers(&mut out, program, signatures)?;

    let mut temp_counter = 0usize;
    for function in &program.functions {
        emit_function(&mut out, function, signatures, &mut temp_counter)?;
        out.push('\n');
    }

    if program.application.is_some() {
        emit_linux_gtk_application(&mut out, program, signatures)?;
    }

    Ok(out)
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
                        "word_char" => "PANGO_WRAP_WORD_CHAR",
                        _ => {
                            return Err(diag(
                                property.value.span,
                                "Text.wrap_mode must be one of 'word', 'char', or 'word_char'",
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
                        "scale_down" => "GTK_CONTENT_FIT_SCALE_DOWN",
                        _ => {
                            return Err(diag(
                                property.value.span,
                                "Image.fit must be one of 'fill', 'contain', 'cover', or 'scale_down'",
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

fn application_metadata_function<'a>(
    application: &'a crate::ast::ApplicationDef,
    name: &str,
) -> Option<&'a str> {
    let field = application
        .metadata
        .iter()
        .find(|field| field.name == name)?;
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
    let field = application
        .metadata
        .iter()
        .find(|field| field.name == name)?;
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
    let field = application
        .metadata
        .iter()
        .find(|field| field.name == name)?;
    static_expr_bool(&field.value, signatures)
}

fn application_metadata_i64(
    application: &crate::ast::ApplicationDef,
    name: &str,
    signatures: &Signatures,
) -> Option<i64> {
    let field = application
        .metadata
        .iter()
        .find(|field| field.name == name)?;
    static_expr_i64(&field.value, signatures)
}

fn view_property<'a>(
    element: &'a crate::ast::ViewElement,
    name: &str,
) -> Option<&'a crate::ast::ViewProperty> {
    element
        .properties
        .iter()
        .find(|property| property.name == name)
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

fn ui_widget_c_name(name: &str) -> String {
    format!("flux__ui_{name}")
}

fn static_expr_i64(expr: &Expr, signatures: &Signatures) -> Option<i64> {
    match &expr.kind {
        ExprKind::Int(value) => Some(*value),
        ExprKind::Var(name) => {
            signatures
                .constant(name)
                .and_then(|constant| match constant.value {
                    ConstantValue::I64(value) => Some(value),
                    _ => None,
                })
        }
        ExprKind::Unary {
            op: UnaryOp::Neg,
            expr,
        } => static_expr_i64(expr, signatures)?.checked_neg(),
        _ => None,
    }
}

fn static_expr_str(expr: &Expr, signatures: &Signatures) -> Option<String> {
    match &expr.kind {
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
    match &expr.kind {
        ExprKind::Bool(value) => Some(*value),
        ExprKind::Var(name) => {
            signatures
                .constant(name)
                .and_then(|constant| match constant.value {
                    ConstantValue::Bool(value) => Some(value),
                    _ => None,
                })
        }
        _ => None,
    }
}

fn ui_expr_c(
    expr: &Expr,
    view: &crate::ast::ViewDef,
    signatures: &Signatures,
) -> Result<String, Diagnostic> {
    match &expr.kind {
        ExprKind::Bool(value) => Ok(if *value { "true" } else { "false" }.to_string()),
        ExprKind::Int(value) => Ok(format!("INT64_C({value})")),
        ExprKind::Str(value) => Ok(c_string(value)),
        ExprKind::Var(name) => {
            let environment = match name.as_str() {
                "window_width" => Some("flux__ui_window_width"),
                "window_height" => Some("flux__ui_window_height"),
                "window_is_landscape" => Some("(flux__ui_window_width > flux__ui_window_height)"),
                "window_is_portrait" => Some("(flux__ui_window_height >= flux__ui_window_width)"),
                "display_scale" => Some("flux__ui_display_scale"),
                _ => None,
            };
            if let Some(environment) = environment {
                return Ok(environment.to_string());
            }
            if view.states.iter().any(|state| state.name == *name) {
                return Ok(ui_state_c_name(name));
            }
            let Some(constant) = signatures.constant(name) else {
                return Err(diag(
                    expr.span,
                    "bootstrap Linux dynamic UI expression may reference only view environment, view state, or compile-time constants",
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
                UnaryOp::Neg => format!("(-({inner}))"),
                UnaryOp::Not => format!("(!({inner}))"),
            })
        }
        ExprKind::Binary { left, op, right } => {
            let left = ui_expr_c(left, view, signatures)?;
            let right = ui_expr_c(right, view, signatures)?;
            if matches!(op, BinOp::Div) {
                Ok(format!("flux_div_i64({left}, {right})"))
            } else {
                Ok(format!("({left} {} {right})", c_operator(*op)))
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
                "ease_in" => "ease-in",
                "ease_out" => "ease-out",
                "ease_in_out" => "ease-in-out",
                _ => {
                    return Err(diag(
                        property.value.span,
                        "transition_easing must be one of 'linear', 'ease', 'ease_in', 'ease_out', or 'ease_in_out'",
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
                if matches!(expr.kind, ExprKind::Match { .. }) =>
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
            StmtKind::Return(values)
                if values.len() == 1 && matches!(values[0].kind, ExprKind::Match { .. }) =>
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
            StmtKind::While { cond, body } => {
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
                    current_function,
                )?;
                out.push_str(&format!("{pad}}}\n"));
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
                let index_c = index_name
                    .as_ref()
                    .map(|index| local_c_name(index))
                    .unwrap_or_else(|| {
                        let generated = format!("flux__iter_index_{}", *temp_counter);
                        *temp_counter += 1;
                        generated
                    });
                let item_c = local_c_name(name);
                let element_c = c_type(&element, signatures);
                out.push_str(&format!(
                    "{pad}struct flux__list {source_name} = {};\n",
                    source.code
                ));
                out.push_str(&format!(
                    "{pad}for (int64_t {index_c} = 0; {index_c} < (int64_t){source_name}.len; ++{index_c}) {{\n"
                ));
                out.push_str(&format!(
                    "{pad}    {element_c} {item_c} = *(({element_c} *)flux_list_at({source_name}, {index_c}, sizeof({element_c})));\n"
                ));
                let mut nested = env.clone();
                if let Some(index_name) = index_name {
                    nested.insert(index_name.clone(), Type::I64);
                }
                nested.insert(name.clone(), *element);
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

fn emit_match_expr_into(
    out: &mut String,
    expr: &Expr,
    target: &str,
    depth: usize,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    temp_counter: &mut usize,
) -> Result<(), Diagnostic> {
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
    for arm in arms {
        let variant = definition
            .variant(&arm.variant)
            .expect("type checking guarantees match expression variants exist");
        out.push_str(&format!(
            "{pad}    case {}: {{\n",
            enum_tag_value_name(enum_name, &arm.variant)
        ));
        let mut nested = env.clone();
        for (index, (pattern, payload_ty)) in arm.patterns.iter().zip(&variant.payloads).enumerate()
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
                    let Type::Named(struct_name) = signatures.canonical_type(payload_ty) else {
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
        let arm_value = emit_expr(&arm.value, &nested, signatures)?;
        out.push_str(&format!("{pad}        {target} = {};\n", arm_value.code));
        out.push_str(&format!("{pad}        break;\n"));
        out.push_str(&format!("{pad}    }}\n"));
    }
    out.push_str(&format!("{pad}}}\n"));
    Ok(())
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
    let list = emit_expr(list_expr, env, signatures)?;
    let Type::List(element) = signatures.canonical_type(&list.ty) else {
        return Err(diag(expr.span, "sequence reduction requires a list source"));
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
    let element_c = c_type(&element, signatures);
    out.push_str(&format!(
        "{pad}struct flux__list {source_name} = {};\n",
        list.code
    ));
    if reduce {
        out.push_str(&format!(
            "{pad}if ({source_name}.len == 0) {{ fputs(\"Flux runtime error: reduce requires a non-empty list\\n\", stderr); abort(); }}\n"
        ));
        out.push_str(&format!(
            "{pad}{} {target_name} = *(({element_c} *)flux_list_at({source_name}, INT64_C(0), sizeof({element_c})));\n",
            c_type(declared_ty, signatures)
        ));
        out.push_str(&format!(
            "{pad}for (size_t {index_name} = 1; {index_name} < {source_name}.len; ++{index_name}) {{\n"
        ));
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
        out.push_str(&format!(
            "{pad}for (size_t {index_name} = 0; {index_name} < {source_name}.len; ++{index_name}) {{\n"
        ));
    }
    out.push_str(&format!(
        "{pad}    {element_c} {item_name} = *(({element_c} *)flux_list_at({source_name}, (int64_t){index_name}, sizeof({element_c})));\n"
    ));
    out.push_str(&format!(
        "{pad}    {target_name} = {}({target_name}, {item_name});\n",
        reducer.code
    ));
    out.push_str(&format!("{pad}}}\n"));
    env.insert(name.to_string(), signatures.canonical_type(declared_ty));
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
        "{pad}    {} {binding_c} = *(({} *)flux_list_at({source_name}, (int64_t){index_name}, sizeof({})));\n",
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
        ExprKind::Index { base, index } => {
            let base = emit_expr(base, env, signatures)?;
            let index = emit_expr(index, env, signatures)?;
            let result_ty = type_of_expr(expr, env, signatures)?;
            let element_c = c_type(&result_ty, signatures);
            EmittedExpr {
                code: format!(
                    "(*(({element_c} *)flux_list_at({}, {}, sizeof({element_c}))))",
                    base.code, index.code
                ),
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
        } if name == "first_or" || name == "last_or" => {
            if !named_args.is_empty() || args.len() != 2 {
                return Err(diag(
                    expr.span,
                    "invalid safe list access call reached code generation",
                ));
            }
            let list = emit_expr(&args[0], env, signatures)?;
            let fallback = emit_expr(&args[1], env, signatures)?;
            let Type::List(element) = &list.ty else {
                return Err(diag(expr.span, "safe list access requires a list value"));
            };
            let element_c = c_type(element, signatures);
            let index = if name == "first_or" {
                "INT64_C(0)"
            } else {
                "INT64_C(-1)"
            };
            EmittedExpr {
                code: format!(
                    "(({}).len == 0 ? ({}) : (*(({element_c} *)flux_list_at({}, {index}, sizeof({element_c})))))",
                    list.code, fallback.code, list.code
                ),
                ty: (**element).clone(),
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
        ExprKind::Match { .. } => {
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
            let base = emit_expr(base, env, signatures)?;
            let result_ty = type_of_expr(expr, env, signatures)?;
            let code = if let Type::List(element) = &base.ty {
                let element_c = c_type(element, signatures);
                match name.as_str() {
                    "length" => format!("({}).len", base.code),
                    "is_empty" => format!("(({}).len == 0)", base.code),
                    "is_not_empty" => format!("(({}).len != 0)", base.code),
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

fn emit_qualified_call(
    span: SourceSpan,
    namespace: &str,
    name: &str,
    args: &[Expr],
    named_args: &[NamedArg],
    env: &HashMap<String, Type>,
    signatures: &Signatures,
) -> Result<(String, Vec<Type>, Option<String>), Diagnostic> {
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
) -> Result<(String, String), Diagnostic> {
    match &expr.kind {
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
            Ok((code, multi_struct))
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
        if !targets.contains(&target_name) {
            targets.push(target_name);
        }
    }
    targets
}

fn emit_interface_value_definitions(out: &mut String, program: &Program, signatures: &Signatures) {
    for definition in &program.interfaces {
        let targets = interface_targets(program, &definition.name, signatures);
        if !targets.is_empty() {
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
        out.push_str("    int32_t tag;\n");
        if !targets.is_empty() {
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
        out.push_str("};\n\n");
    }
}

fn interface_pack_helpers(program: &Program, signatures: &Signatures) -> String {
    let mut out = String::new();
    for definition in &program.interfaces {
        for target in interface_targets(program, &definition.name, signatures) {
            out.push_str(&format!(
                "static inline struct {} {}(struct {} value) {{\n",
                interface_c_name(&definition.name),
                interface_pack_helper_name(&definition.name, &target),
                struct_c_name(&target)
            ));
            out.push_str(&format!(
                "    struct {} result;\n",
                interface_c_name(&definition.name)
            ));
            out.push_str(&format!(
                "    result.tag = {};\n",
                interface_tag_name(&definition.name, &target)
            ));
            out.push_str(&format!(
                "    result.value.{} = value;\n",
                interface_value_member_name(&target)
            ));
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
) {
    let mut emitted = false;
    for definition in &program.interfaces {
        let Some(interface) = signatures.interface(&definition.name) else {
            continue;
        };
        let mut member_names = interface.functions.keys().collect::<Vec<_>>();
        member_names.sort();
        for member_name in member_names {
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
) -> Result<(), Diagnostic> {
    let mut emitted = false;
    for definition in &program.interfaces {
        let Some(interface) = signatures.interface(&definition.name) else {
            continue;
        };
        let targets = interface_targets(program, &definition.name, signatures);
        let mut member_names = interface.functions.keys().collect::<Vec<_>>();
        member_names.sort();
        for member_name in member_names {
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
                "static inline {return_type} {}(struct {} receiver",
                interface_dispatch_helper_name(&definition.name, member_name),
                interface_c_name(&definition.name)
            ));
            for (index, param) in member.param_details.iter().enumerate() {
                out.push_str(&format!(", {} arg_{index}", c_type(&param.ty, signatures)));
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

fn program_uses_background(program: &Program) -> bool {
    program
        .functions
        .iter()
        .any(|function| block_uses_background(&function.body))
}

fn block_uses_background(body: &[Stmt]) -> bool {
    body.iter().any(|stmt| match &stmt.kind {
        StmtKind::Shell { background, .. } => *background,
        StmtKind::If {
            body, else_body, ..
        } => block_uses_background(body) || block_uses_background(else_body),
        StmtKind::ForRange { body, .. }
        | StmtKind::ForEach { body, .. }
        | StmtKind::While { body, .. } => block_uses_background(body),
        StmtKind::Match { arms, .. } => arms.iter().any(|arm| block_uses_background(&arm.body)),
        _ => false,
    })
}

fn collect_function_types_from_block(
    body: &[Stmt],
    signatures: &Signatures,
    types: &mut HashSet<Type>,
) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Let { ty, .. } | StmtKind::Var { ty, .. } => {
                collect_function_type(ty, signatures, types)
            }
            StmtKind::LetDestructure { bindings, .. } => {
                for binding in bindings {
                    collect_function_type(&binding.ty, signatures, types);
                }
            }
            StmtKind::LetStructDestructure { .. }
            | StmtKind::Assign { .. }
            | StmtKind::Return(_)
            | StmtKind::Break
            | StmtKind::Continue
            | StmtKind::Expr(_)
            | StmtKind::Shell { .. } => {}
            StmtKind::If {
                body, else_body, ..
            } => {
                collect_function_types_from_block(body, signatures, types);
                collect_function_types_from_block(else_body, signatures, types);
            }
            StmtKind::ForRange { body, .. }
            | StmtKind::ForEach { body, .. }
            | StmtKind::While { body, .. } => {
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
            | StmtKind::Var { expr, .. }
            | StmtKind::Assign { expr, .. }
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
        ExprKind::Match { value, arms } => {
            collect_update_helpers_from_expr(value, signatures, emitted, helpers);
            for arm in arms {
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
