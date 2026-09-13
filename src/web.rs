use std::collections::HashSet;

use crate::ast::{BinOp, Expr, ExprKind, GridTrack, Program, UnaryOp, ViewDef, ViewElement};
use crate::diagnostic::{Diagnostic, DiagnosticStage};

pub fn emit_html(program: &Program) -> Result<String, Diagnostic> {
    let application = program.application.as_ref().ok_or_else(|| {
        Diagnostic::global(
            DiagnosticStage::Codegen,
            "web builds require an `app` declaration",
        )
    })?;
    let view = program
        .views
        .iter()
        .find(|view| view.name == application.view_name)
        .ok_or_else(|| {
            Diagnostic::global(
                DiagnosticStage::Codegen,
                format!(
                    "web app root view '{}' was not found",
                    application.view_name
                ),
            )
        })?;
    if !view.params.is_empty() {
        return Err(Diagnostic::global(
            DiagnosticStage::Codegen,
            "web app root views cannot require parameters",
        ));
    }

    let title = application
        .metadata
        .iter()
        .find(|field| field.name == "title")
        .and_then(|field| match &field.value.kind {
            ExprKind::Str(value) => Some(value.as_str()),
            _ => None,
        })
        .unwrap_or("Flux application");

    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str(
        "<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n<title>",
    );
    out.push_str(&escape_html(title));
    out.push_str("</title>\n<style>\n");
    out.push_str("html,body{margin:0;min-height:100%;font-family:system-ui,-apple-system,BlinkMacSystemFont,\"Segoe UI\",sans-serif;color:CanvasText;background:Canvas}*{box-sizing:border-box}.flux-root{display:grid;min-height:100vh;width:100%;align-content:start}.flux-text{white-space:pre-wrap}.flux-image{max-width:100%;height:auto}.flux-toggle{display:flex;align-items:center;gap:.5rem}.flux-hidden{display:none!important}\n");
    out.push_str("</style>\n</head>\n<body>\n<main id=\"flux-root\" class=\"flux-root\" style=\"");
    out.push_str(&grid_style(view));
    out.push_str("\">\n");

    for element in &view.elements {
        emit_element(&mut out, element)?;
    }
    out.push_str("</main>\n<script>\n'use strict';\n");
    emit_runtime(&mut out, view)?;
    out.push_str("</script>\n</body>\n</html>\n");
    Ok(out)
}

fn grid_style(view: &ViewDef) -> String {
    let mut style = String::new();
    if !view.grid.columns.is_empty() {
        style.push_str("grid-template-columns:");
        style.push_str(&grid_tracks(&view.grid.columns));
        style.push(';');
    }
    if !view.grid.rows.is_empty() {
        style.push_str("grid-template-rows:");
        style.push_str(&grid_tracks(&view.grid.rows));
        style.push(';');
    }
    if let Some(gap) = view.grid.gap {
        style.push_str(&format!("gap:{gap}px;"));
    }
    if let Some(padding) = view.grid.padding {
        style.push_str(&format!("padding:{padding}px;"));
    }
    if view.grid.scroll == Some(true) {
        style.push_str("overflow:auto;");
    }
    style
}

fn grid_tracks(tracks: &[GridTrack]) -> String {
    tracks
        .iter()
        .map(|track| match track {
            GridTrack::Units(value) => format!("{value}px"),
            GridTrack::Fraction(value) => format!("{value}fr"),
            GridTrack::Auto => "auto".to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn emit_element(out: &mut String, element: &ViewElement) -> Result<(), Diagnostic> {
    let tag = match element.kind.as_str() {
        "Text" => "div",
        "Button" => "button",
        "TextInput" => "input",
        "Image" => "img",
        "Toggle" => "label",
        "Radio" => "label",
        "Nav" => "nav",
        "Chart" => "figure",
        "Card" => "article",
        "Header" => "header",
        "Content" => "section",
        other => {
            return Err(Diagnostic::global(
                DiagnosticStage::Codegen,
                format!("web target does not yet support the native element '{other}'"),
            ));
        }
    };
    let id = element_id(element);
    out.push('<');
    out.push_str(tag);
    out.push_str(" id=\"");
    out.push_str(&id);
    out.push_str("\"");
    match element.kind.as_str() {
        "Text" => out.push_str(" class=\"flux-text\""),
        "Image" => out.push_str(" class=\"flux-image\""),
        "Toggle" | "Radio" => out.push_str(" class=\"flux-toggle\""),
        _ => {}
    }
    out.push_str(" style=\"");
    out.push_str(&format!(
        "grid-row:{} / span {};grid-column:{} / span {};",
        element.row, element.row_span, element.column, element.column_span
    ));
    out.push_str("\"");
    if matches!(element.kind.as_str(), "TextInput") {
        out.push_str(" type=\"text\"");
    }
    if matches!(element.kind.as_str(), "Image") {
        out.push_str(" alt=\"\"");
    }
    out.push('>');
    if matches!(element.kind.as_str(), "Toggle" | "Radio") {
        let input_type = if element.kind == "Radio" {
            "radio"
        } else {
            "checkbox"
        };
        out.push_str("<input data-flux-control=\"1\" type=\"");
        out.push_str(input_type);
        out.push_str("\"><span data-flux-label=\"1\"></span>");
    }
    if !matches!(element.kind.as_str(), "TextInput" | "Image") {
        out.push_str("</");
        out.push_str(tag);
        out.push_str(">");
    }
    out.push('\n');
    Ok(())
}

fn emit_runtime(out: &mut String, view: &ViewDef) -> Result<(), Diagnostic> {
    let state_names = view
        .states
        .iter()
        .map(|state| state.name.as_str())
        .collect::<HashSet<_>>();
    let derived_names = view
        .derived
        .iter()
        .map(|derived| derived.name.as_str())
        .collect::<HashSet<_>>();

    out.push_str("const state=Object.create(null);\n");
    for state in &view.states {
        out.push_str("state[");
        out.push_str(&js_string(&state.name));
        out.push_str("]=");
        out.push_str(&expr_js(&state.initial, &state_names, &derived_names)?);
        out.push_str(";\n");
    }
    for derived in &view.derived {
        out.push_str("function fluxDerived_");
        out.push_str(&js_ident(&derived.name));
        out.push_str("(){return ");
        out.push_str(&expr_js(&derived.value, &state_names, &derived_names)?);
        out.push_str(";}\n");
    }
    out.push_str("function fluxEnv(name){switch(name){case'windowWidth':return Math.trunc(window.innerWidth);case'windowHeight':return Math.trunc(window.innerHeight);case'windowIsLandscape':return window.innerWidth>=window.innerHeight;case'windowIsPortrait':return window.innerHeight>window.innerWidth;case'windowIsCompact':return window.innerWidth<600;case'windowIsMedium':return window.innerWidth>=600&&window.innerWidth<840;case'windowIsExpanded':return window.innerWidth>=840;case'displayScale':return Math.max(1,Math.trunc(window.devicePixelRatio||1));default:return undefined;}}\n");
    out.push_str("function fluxSetVisible(el,value){el.hidden=!value;}\n");
    out.push_str("function fluxSetText(el,value){el.textContent=String(value);}\n");
    out.push_str("function fluxRole(value){switch(String(value)){case'textBox':return'textbox';case'checkbox':return'checkbox';case'radio':return'radio';case'image':return'img';case'switch':return'switch';case'heading':return'heading';case'button':return'button';default:return null;}}\n");
    out.push_str(
        "function fluxBrowserPush(path){history.pushState(null,'',String(path));fluxRefresh();}\n",
    );
    out.push_str("function fluxBrowserReplace(path){history.replaceState(null,'',String(path));fluxRefresh();}\n");
    out.push_str("function fluxBrowserStore(key,value){try{localStorage.setItem(String(key),String(value));}catch(_){}}\n");
    out.push_str("function fluxBrowserLoad(key,fallback){try{const value=localStorage.getItem(String(key));return value===null?String(fallback):value;}catch(_){return String(fallback);}}\n");
    out.push_str(
        "function fluxBrowserErase(key){try{localStorage.removeItem(String(key));}catch(_){}}\n",
    );
    out.push_str("function fluxRefresh(){\n");
    for element in &view.elements {
        emit_refresh(out, element, &state_names, &derived_names)?;
    }
    out.push_str("}\n");

    for element in &view.elements {
        emit_events(out, element, &state_names, &derived_names)?;
    }
    out.push_str("window.addEventListener('resize',fluxRefresh,{passive:true});\nwindow.addEventListener('popstate',fluxRefresh,{passive:true});\nfluxRefresh();\n");
    Ok(())
}

fn emit_refresh(
    out: &mut String,
    element: &ViewElement,
    states: &HashSet<&str>,
    derived: &HashSet<&str>,
) -> Result<(), Diagnostic> {
    let id = element_id(element);
    out.push_str("{const el=document.getElementById(");
    out.push_str(&js_string(&id));
    out.push_str(");");
    for property in &element.properties {
        let value = expr_js(&property.value, states, derived)?;
        let property_name = crate::typecheck::source_name_to_internal(&property.name);
        match property_name.as_str() {
            "text" if element.kind == "Text" || element.kind == "Button" => {
                out.push_str("fluxSetText(el,");
                out.push_str(&value);
                out.push_str(");");
            }
            "text" if element.kind == "TextInput" => {
                out.push_str("if(el.value!==String(");
                out.push_str(&value);
                out.push_str("))el.value=String(");
                out.push_str(&value);
                out.push_str(");");
            }
            "label" if element.kind == "Toggle" || element.kind == "Radio" => {
                out.push_str("el.querySelector('[data-flux-label]').textContent=String(");
                out.push_str(&value);
                out.push_str(");");
            }
            "label" if matches!(element.kind.as_str(), "Nav" | "Chart" | "Content") => {
                out.push_str("fluxSetText(el,");
                out.push_str(&value);
                out.push_str(");");
            }
            "title" if element.kind == "Card" => {
                out.push_str("fluxSetText(el,");
                out.push_str(&value);
                out.push_str(");");
            }
            "text" if element.kind == "Header" => {
                out.push_str("fluxSetText(el,");
                out.push_str(&value);
                out.push_str(");");
            }
            "source" if element.kind == "Image" => {
                out.push_str("el.src=String(");
                out.push_str(&value);
                out.push_str(");");
            }
            "alt" if element.kind == "Image" => {
                out.push_str("el.alt=String(");
                out.push_str(&value);
                out.push_str(");");
            }
            "visible" => {
                out.push_str("fluxSetVisible(el,");
                out.push_str(&value);
                out.push_str(");");
            }
            "enabled" => {
                if element.kind == "Toggle" || element.kind == "Radio" {
                    out.push_str("el.querySelector('[data-flux-control]').disabled=!(");
                } else {
                    out.push_str("el.disabled=!(");
                }
                out.push_str(&value);
                out.push_str(");");
            }
            "checked" | "selected" if element.kind == "Toggle" || element.kind == "Radio" => {
                out.push_str("el.querySelector('[data-flux-control]').checked=!!(");
                out.push_str(&value);
                out.push_str(");");
            }
            "placeholder" if element.kind == "TextInput" => {
                out.push_str("el.placeholder=String(");
                out.push_str(&value);
                out.push_str(");");
            }
            "password" if element.kind == "TextInput" => {
                out.push_str("el.type=(");
                out.push_str(&value);
                out.push_str(")?'password':'text';");
            }
            "read_only" if element.kind == "TextInput" => {
                out.push_str("el.readOnly=!!(");
                out.push_str(&value);
                out.push_str(");");
            }
            "size" if element.kind == "Text" || element.kind == "Button" => {
                out.push_str("el.style.fontSize=String(");
                out.push_str(&value);
                out.push_str(")+'px';");
            }
            "bold" if element.kind == "Text" => {
                out.push_str("el.style.fontWeight=(");
                out.push_str(&value);
                out.push_str(")?'700':'';");
            }
            "italic" if element.kind == "Text" => {
                out.push_str("el.style.fontStyle=(");
                out.push_str(&value);
                out.push_str(")?'italic':'';");
            }
            "color" => {
                out.push_str("el.style.color=String(");
                out.push_str(&value);
                out.push_str(");");
            }
            "background" => {
                out.push_str("el.style.background=String(");
                out.push_str(&value);
                out.push_str(");");
            }
            "accessibility_label" => {
                out.push_str("el.setAttribute('aria-label',String(");
                out.push_str(&value);
                out.push_str("));");
            }
            "accessibility_description" => {
                out.push_str("el.setAttribute('aria-description',String(");
                out.push_str(&value);
                out.push_str("));");
            }
            "accessibility_hidden" => {
                out.push_str("el.setAttribute('aria-hidden',(");
                out.push_str(&value);
                out.push_str(")?'true':'false');");
            }
            "accessibility_role" => {
                out.push_str("{const role=fluxRole(");
                out.push_str(&value);
                out.push_str(
                    ");if(role)el.setAttribute('role',role);else el.removeAttribute('role');}",
                );
            }
            "on_press" | "on_change" | "on_submit" | "on_select" => {}
            _ => {}
        }
    }
    out.push_str("}\n");
    Ok(())
}

fn emit_events(
    out: &mut String,
    element: &ViewElement,
    states: &HashSet<&str>,
    derived: &HashSet<&str>,
) -> Result<(), Diagnostic> {
    let id = element_id(element);
    for property in &element.properties {
        let property_name = crate::typecheck::source_name_to_internal(&property.name);
        let event = match property_name.as_str() {
            "on_press" | "on_select" => "click",
            "on_change" if element.kind == "TextInput" => "input",
            "on_change" => "change",
            "on_submit" => "keydown",
            _ => continue,
        };
        out.push_str("document.getElementById(");
        out.push_str(&js_string(&id));
        if element.kind == "Toggle" || element.kind == "Radio" {
            out.push_str(").querySelector('[data-flux-control]')");
        } else {
            out.push(')');
        }
        out.push_str(".addEventListener('");
        out.push_str(event);
        out.push_str("',event=>{");
        if property_name == "on_submit" {
            out.push_str("if(event.key!=='Enter')return;");
        }
        if let Some(transition) = property.transition.as_ref() {
            if let Some(event_value) = transition.event_value.as_deref() {
                out.push_str("const ");
                out.push_str(&js_ident(event_value));
                out.push_str("=");
                if element.kind == "Toggle" || element.kind == "Radio" {
                    out.push_str("event.currentTarget.checked;");
                } else {
                    out.push_str("event.currentTarget.value;");
                }
            }
            out.push_str("state[");
            out.push_str(&js_string(&transition.state));
            out.push_str("]=");
            out.push_str(&expr_js(&property.value, states, derived)?);
            out.push_str(";fluxRefresh();});\n");
            continue;
        }
        let handler = expr_js(&property.value, states, derived)?;
        out.push('(');
        out.push_str(&handler);
        out.push_str(")(");
        if element.kind == "TextInput"
            && matches!(property_name.as_str(), "on_change" | "on_submit")
        {
            out.push_str("event.currentTarget.value");
        }
        out.push_str(");fluxRefresh();});\n");
    }
    Ok(())
}

fn expr_js(
    expr: &Expr,
    states: &HashSet<&str>,
    derived: &HashSet<&str>,
) -> Result<String, Diagnostic> {
    Ok(match &expr.kind {
        ExprKind::Int(value) => value.to_string(),
        ExprKind::Bool(value) => value.to_string(),
        ExprKind::Str(value) => js_string(value),
        ExprKind::InterpolatedString(parts) => {
            let mut rendered = String::from("`");
            for part in parts {
                match part {
                    crate::ast::InterpolatedStringPart::Text(text) => rendered.push_str(
                        &text
                            .replace('\\', "\\\\")
                            .replace('`', "\\`")
                            .replace("${", "\\${"),
                    ),
                    crate::ast::InterpolatedStringPart::Binding { name, .. } => {
                        rendered.push_str("${");
                        rendered.push_str(&var_js(name, states, derived));
                        rendered.push('}');
                    }
                }
            }
            rendered.push('`');
            rendered
        }
        ExprKind::Var(name) => var_js(name, states, derived),
        ExprKind::AnonymousFunction { params, body, .. } => {
            let params = params
                .iter()
                .map(|param| js_ident(&param.name))
                .collect::<Vec<_>>()
                .join(",");
            format!("({params})=>{}", expr_js(body, states, derived)?)
        }
        ExprKind::QualifiedCall {
            namespace,
            name,
            args,
            named_args,
            ..
        } if namespace == "browser" && named_args.is_empty() => {
            let rendered = args
                .iter()
                .map(|arg| expr_js(arg, states, derived))
                .collect::<Result<Vec<_>, _>>()?;
            match name.as_str() {
                "path" if rendered.is_empty() => "window.location.pathname".to_string(),
                "push" if rendered.len() == 1 => format!("fluxBrowserPush({})", rendered[0]),
                "replace" if rendered.len() == 1 => {
                    format!("fluxBrowserReplace({})", rendered[0])
                }
                "back" if rendered.is_empty() => "history.back()".to_string(),
                "forward" if rendered.is_empty() => "history.forward()".to_string(),
                "store" if rendered.len() == 2 => {
                    format!("fluxBrowserStore({},{})", rendered[0], rendered[1])
                }
                "load" if rendered.len() == 2 => {
                    format!("fluxBrowserLoad({},{})", rendered[0], rendered[1])
                }
                "erase" if rendered.len() == 1 => format!("fluxBrowserErase({})", rendered[0]),
                _ => {
                    return Err(Diagnostic::global(
                        DiagnosticStage::Codegen,
                        format!("unsupported browser operation 'browser.{name}' in web UI"),
                    ));
                }
            }
        }
        ExprKind::Unary { op, expr } => {
            let operator = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            };
            format!("({operator}{})", expr_js(expr, states, derived)?)
        }
        ExprKind::Binary { left, op, right } => {
            let operator = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Eq => "===",
                BinOp::Ne => "!==",
                BinOp::Lt => "<",
                BinOp::Le => "<=",
                BinOp::Gt => ">",
                BinOp::Ge => ">=",
                BinOp::And => "&&",
                BinOp::Or => "||",
                BinOp::Coalesce => "??",
            };
            format!(
                "({} {operator} {})",
                expr_js(left, states, derived)?,
                expr_js(right, states, derived)?
            )
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => format!(
            "({}?{}:{})",
            expr_js(cond, states, derived)?,
            expr_js(then_expr, states, derived)?,
            expr_js(else_expr, states, derived)?
        ),
        _ => {
            return Err(Diagnostic::global(
                DiagnosticStage::Codegen,
                "web UI expressions currently support primitive/state/derived values, browser primitives, anonymous event callbacks, interpolation, arithmetic, comparisons, boolean operators, and responsive environment bindings",
            ));
        }
    })
}

fn var_js(name: &str, states: &HashSet<&str>, derived: &HashSet<&str>) -> String {
    if states.contains(name) {
        return format!("state[{}]", js_string(name));
    }
    if derived.contains(name) {
        return format!("fluxDerived_{}()", js_ident(name));
    }
    match name {
        "windowWidth" | "windowHeight" | "windowIsLandscape" | "windowIsPortrait"
        | "windowIsCompact" | "windowIsMedium" | "windowIsExpanded" | "displayScale" => {
            format!("fluxEnv({})", js_string(name))
        }
        _ => js_ident(name),
    }
}

fn element_id(element: &ViewElement) -> String {
    format!("flux-{}", element.name.replace('_', "-"))
}

fn js_ident(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() || out.as_bytes()[0].is_ascii_digit() {
        out.insert(0, '_');
    }
    out
}

fn js_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029")
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::SourceId;
    use crate::semantic::SemanticDatabase;

    fn html(source: &str) -> String {
        let database = SemanticDatabase::analyze(source, SourceId::UNKNOWN)
            .expect("web fixture should type check");
        emit_html(database.program()).expect("web fixture should emit")
    }

    #[test]
    fn emits_native_dom_grid_and_responsive_environment() {
        let output = html(
            r#"
view Demo {
    grid columns: 1fr 240
    grid rows: auto auto
    grid gap: 12
    grid padding: 20

    Text title at 1,1 span columns 2
        text: "Wide"
        visible: windowIsExpanded

    Button action at 2,1
        text: "Action"
        enabled: windowWidth > 400
}

app Demo(title: "Web demo")
"#,
        );
        assert!(output.contains("<main id=\"flux-root\" class=\"flux-root\""));
        assert!(output.contains("grid-template-columns:1fr 240px"));
        assert!(output.contains("<button id=\"flux-action\""));
        assert!(output.contains("fluxEnv(\"windowIsExpanded\")"));
        assert!(output.contains("window.addEventListener('resize',fluxRefresh"));
        assert!(!output.contains("canvas"));
    }

    #[test]
    fn lowers_accessibility_to_browser_semantics() {
        let output = html(
            r#"
view Demo {
    grid columns: 1fr
    grid rows: auto

    Button save at 1,1
        text: "Save"
        accessibilityLabel: "Save document"
        accessibilityDescription: "Writes changes"
        accessibilityRole: "button"
        accessibilityHidden: false
}

app Demo
"#,
        );
        assert!(output.contains("setAttribute('aria-label'"));
        assert!(output.contains("setAttribute('aria-description'"));
        assert!(output.contains("setAttribute('aria-hidden'"));
        assert!(output.contains("fluxRole(\"button\")"));
    }

    #[test]
    fn lowers_state_transitions_without_framework_bridge() {
        let output = html(
            r#"
view Demo {
    grid columns: 1fr
    grid rows: auto auto
    state active: bool = false

    Text status at 1,1
        text: "Active"
        visible: active

    Button toggle at 2,1
        text: "Toggle"
        onPress: active => !active
}

app Demo
"#,
        );
        assert!(output.contains("state[\"active\"]=false"));
        assert!(output.contains("addEventListener('click'"));
        assert!(output.contains("state[\"active\"]="));
        assert!(output.contains("fluxRefresh();"));
        assert!(!output.contains("methodChannel"));
    }

    #[test]
    fn lowers_browser_history_and_storage_without_user_bridge() {
        let output = html(
            r#"
view Demo {
    grid columns: 1fr
    grid rows: auto auto auto auto auto auto auto auto

    Text location at 1,1
        text: browser.path()

    Text saved at 2,1
        text: browser.load("theme", "system")

    Button navigate at 3,1
        text: "Settings"
        onPress: fn() { browser.push("/settings") }

    Button replace at 4,1
        text: "Account"
        onPress: fn() { browser.replace("/account") }

    Button back at 5,1
        text: "Back"
        onPress: fn() { browser.back() }

    Button forward at 6,1
        text: "Forward"
        onPress: fn() { browser.forward() }

    Button remember at 7,1
        text: "Remember"
        onPress: fn() { browser.store("theme", "dark") }

    Button forget at 8,1
        text: "Forget"
        onPress: fn() { browser.erase("theme") }
}

app Demo
"#,
        );
        assert!(output.contains("window.location.pathname"));
        assert!(output.contains("history.pushState"));
        assert!(output.contains("window.addEventListener('popstate',fluxRefresh"));
        assert!(output.contains("localStorage.getItem"));
        assert!(output.contains("localStorage.setItem"));
        assert!(output.contains("fluxBrowserPush(\"/settings\")"));
        assert!(output.contains("fluxBrowserReplace(\"/account\")"));
        assert!(output.contains("history.back()"));
        assert!(output.contains("history.forward()"));
        assert!(output.contains("fluxBrowserStore(\"theme\",\"dark\")"));
        assert!(output.contains("fluxBrowserErase(\"theme\")"));
        assert!(!output.contains("MethodChannel"));
        assert!(!output.contains("PluginRegistry"));
    }
}
