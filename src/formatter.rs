use std::collections::HashMap;

use crate::ast::{
    BinOp, Expr, ExprKind, Function, GridTrack, ListMatchPattern, MatchPattern, ShellRedirectMode,
    Stmt, StmtKind, StructPatternField, Type, UnaryOp,
};
use crate::diagnostic::Diagnostic;
use crate::parser;

/// Compatibility version of Flux's canonical source formatter.
///
/// A change that intentionally rewrites already-canonical source differently must
/// increment this value so editor/CI integrations can pin formatter behavior.
pub const FORMATTER_VERSION: u32 = 1;

pub fn format_source(source: &str) -> Result<String, Vec<Diagnostic>> {
    let program = parser::parse_all(source)?;
    let mut formatted = HashMap::new();
    for import in &program.imports {
        formatted.insert(import.line, format!("import {:?}", import.path));
    }
    if let Some(application) = &program.application {
        let metadata = application
            .metadata
            .iter()
            .map(|field| format!("{}: {}", field.name, format_expr(&field.value, 0)))
            .collect::<Vec<_>>();
        let suffix = if metadata.is_empty() {
            String::new()
        } else {
            format!("({})", metadata.join(", "))
        };
        formatted.insert(
            application.line,
            format!("app {}{suffix}", application.view_name),
        );
    }
    for alias in &program.aliases {
        let visibility = if alias.public { "pub " } else { "" };
        formatted.insert(
            alias.line,
            format!("{visibility}type {} = {}", alias.name, alias.target.name()),
        );
    }
    for definition in &program.interfaces {
        format_interface(definition, &mut formatted);
    }
    for implementation in &program.implementations {
        format_interface_implementation(implementation, &mut formatted);
    }
    for constant in &program.constants {
        let visibility = if constant.public { "pub " } else { "" };
        formatted.insert(
            constant.line,
            format!(
                "{visibility}const {}: {} = {}",
                constant.name,
                constant.ty.name(),
                format_expr(&constant.value, 0)
            ),
        );
    }
    for definition in &program.enums {
        format_enum(definition, &mut formatted);
    }
    for definition in &program.structs {
        format_struct(definition, &mut formatted);
    }
    for view in &program.views {
        format_view(view, &mut formatted);
    }
    for function in &program.functions {
        format_function(function, &mut formatted);
    }

    let raw_lines = source.lines().collect::<Vec<_>>();
    let mut output = Vec::with_capacity(raw_lines.len());
    let mut blank_pending = false;
    let mut preserve_multiline_continuation = false;

    for (index, raw) in raw_lines.iter().enumerate() {
        if preserve_multiline_continuation {
            output.push(raw.trim_end().to_string());
            if unescaped_triple_quote_count(raw) % 2 == 1 {
                preserve_multiline_continuation = false;
            }
            continue;
        }

        let line_number = index + 1;
        let trimmed = raw.trim();
        let starts_multiline = unescaped_triple_quote_count(raw) % 2 == 1;
        if trimmed.is_empty() {
            blank_pending = !output.is_empty();
            continue;
        }

        if blank_pending {
            output.push(String::new());
            blank_pending = false;
        }

        if starts_multiline {
            output.push(raw.trim_end().to_string());
            preserve_multiline_continuation = true;
            continue;
        }

        if let Some(code) = formatted.get(&line_number) {
            output.push(code.clone());
            continue;
        }

        if trimmed == "}" {
            output.push("}".to_string());
            continue;
        }

        output.push(raw.trim_end().to_string());
    }

    while output.last().is_some_and(String::is_empty) {
        output.pop();
    }

    Ok(format!("{}\n", output.join("\n")))
}

fn format_interface(definition: &crate::ast::InterfaceDef, lines: &mut HashMap<usize, String>) {
    let visibility = if definition.public { "pub " } else { "" };
    let parents = if definition.parents.is_empty() {
        String::new()
    } else {
        format!(
            ": {}",
            definition
                .parents
                .iter()
                .map(|parent| parent.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    lines.insert(
        definition.line,
        format!("{visibility}interface {}{parents} {{", definition.name),
    );
    for function in &definition.functions {
        let mut param_parts = Vec::new();
        let mut emitted_named_marker = false;
        for param in &function.params {
            if param.named_only && !emitted_named_marker {
                param_parts.push("*".to_string());
                emitted_named_marker = true;
            }
            param_parts.push(format!("{}: {}", param.name, param.ty.name()));
        }
        lines.insert(
            function.line,
            format!(
                "    fn {}({}) -> {}",
                function.name,
                param_parts.join(", "),
                format_return_types(&function.returns)
            ),
        );
    }
}

fn format_interface_implementation(
    implementation: &crate::ast::InterfaceImpl,
    lines: &mut HashMap<usize, String>,
) {
    lines.insert(
        implementation.line,
        format!(
            "impl {} for {} {{",
            implementation.interface_name, implementation.target_name
        ),
    );
    for mapping in &implementation.mappings {
        lines.insert(
            mapping.member_span.line,
            format!("    {}: {}", mapping.member, mapping.function),
        );
    }
}

fn format_enum(definition: &crate::ast::EnumDef, lines: &mut HashMap<usize, String>) {
    let visibility = if definition.public { "pub " } else { "" };
    lines.insert(
        definition.line,
        format!("{visibility}enum {} {{", definition.name),
    );
    for variant in &definition.variants {
        let payloads = variant
            .payloads
            .iter()
            .map(|payload| payload.ty.name())
            .collect::<Vec<_>>()
            .join(", ");
        let rendered = if payloads.is_empty() {
            variant.name.clone()
        } else {
            format!("{}({payloads})", variant.name)
        };
        lines.insert(variant.name_span.line, format!("    {rendered}"));
    }
}

fn format_struct(definition: &crate::ast::StructDef, lines: &mut HashMap<usize, String>) {
    let visibility = if definition.public { "pub " } else { "" };
    lines.insert(
        definition.line,
        format!("{visibility}struct {} {{", definition.name),
    );
    for field in &definition.fields {
        lines.insert(
            field.name_span.line,
            format!("    {}: {}", field.name, field.ty.name()),
        );
    }
}

fn format_view(view: &crate::ast::ViewDef, lines: &mut HashMap<usize, String>) {
    let visibility = if view.public { "pub " } else { "" };
    let mut param_parts = Vec::new();
    let mut emitted_named_marker = false;
    for param in &view.params {
        if param.named_only && !emitted_named_marker {
            param_parts.push("*".to_string());
            emitted_named_marker = true;
        }
        let default = param
            .default
            .as_ref()
            .map(|value| format!(" = {}", format_expr(value, 0)))
            .unwrap_or_default();
        param_parts.push(format!("{}: {}{default}", param.name, param.ty.name()));
    }
    let header = if param_parts.is_empty() {
        format!("{visibility}view {} {{", view.name)
    } else {
        format!(
            "{visibility}view {}({}) {{",
            view.name,
            param_parts.join(", ")
        )
    };
    lines.insert(view.line, header);
    lines.insert(
        view.line + 1,
        format!(
            "    grid columns: {}",
            format_grid_tracks(&view.grid.columns)
        ),
    );
    lines.insert(
        view.line + 2,
        format!("    grid rows: {}", format_grid_tracks(&view.grid.rows)),
    );
    if let Some(gap) = view.grid.gap {
        let gap_line = view
            .states
            .first()
            .map(|state| state.line.saturating_sub(1))
            .into_iter()
            .chain(
                view.derived
                    .first()
                    .map(|derived| derived.line.saturating_sub(1)),
            )
            .chain(
                view.elements
                    .first()
                    .map(|element| element.line.saturating_sub(1)),
            )
            .min()
            .unwrap_or(view.line + 3);
        lines.insert(gap_line, format!("    grid gap: {gap}"));
    }
    if let (Some(padding), Some(line)) = (view.grid.padding, view.grid.padding_line) {
        lines.insert(line, format!("    grid padding: {padding}"));
    }
    if let (Some(scroll), Some(line)) = (view.grid.scroll, view.grid.scroll_line) {
        lines.insert(line, format!("    grid scroll: {scroll}"));
    }
    for state in &view.states {
        lines.insert(
            state.line,
            format!(
                "    state {}: {} = {}",
                state.name,
                state.ty.name(),
                format_expr(&state.initial, 0)
            ),
        );
    }
    for derived in &view.derived {
        lines.insert(
            derived.line,
            format!(
                "    derived {}: {} = {}",
                derived.name,
                derived.ty.name(),
                format_expr(&derived.value, 0)
            ),
        );
    }
    for element in &view.elements {
        let mut placement = format!(
            "    {} {} at {},{}",
            element.kind, element.name, element.row, element.column
        );
        if element.row_span != 1 {
            placement.push_str(&format!(" span rows {}", element.row_span));
        }
        if element.column_span != 1 {
            placement.push_str(&format!(" span columns {}", element.column_span));
        }
        lines.insert(element.line, placement);
        for property in &element.properties {
            let value = match &property.transition {
                Some(transition) => format!(
                    "{} => {}",
                    transition.state,
                    format_expr(&property.value, 0)
                ),
                None => format_expr(&property.value, 0),
            };
            lines.insert(property.line, format!("        {}: {value}", property.name));
        }
    }
}

fn format_grid_tracks(tracks: &[GridTrack]) -> String {
    tracks
        .iter()
        .map(|track| match track {
            GridTrack::Units(value) => value.to_string(),
            GridTrack::Fraction(value) => format!("{value}fr"),
            GridTrack::Auto => "auto".to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_function(function: &Function, lines: &mut HashMap<usize, String>) {
    let visibility = if function.public { "pub " } else { "" };
    let mut param_parts = Vec::new();
    let mut emitted_named_marker = false;
    for param in &function.params {
        if param.named_only && !emitted_named_marker {
            param_parts.push("*".to_string());
            emitted_named_marker = true;
        }
        let default = param
            .default
            .as_ref()
            .map(|value| format!(" = {}", format_expr(value, 0)))
            .unwrap_or_default();
        param_parts.push(format!("{}: {}{default}", param.name, param.ty.name()));
    }
    let params = param_parts.join(", ");
    if function.expression_body {
        let expression = function
            .body
            .first()
            .and_then(|stmt| match &stmt.kind {
                StmtKind::Return(values) if values.len() == 1 => values.first(),
                _ => None,
            })
            .expect("single-expression functions contain one implicit return expression");
        lines.insert(
            function.line,
            format!(
                "{visibility}fn {}({params}) -> {} {{ {} }}",
                function.name,
                format_return_types(&function.returns),
                format_expr(expression, 0)
            ),
        );
        return;
    }
    lines.insert(
        function.line,
        format!(
            "{visibility}fn {}({params}) -> {} {{",
            function.name,
            format_return_types(&function.returns)
        ),
    );
    format_block(&function.body, 1, lines);
}

fn format_block(body: &[Stmt], depth: usize, lines: &mut HashMap<usize, String>) {
    for stmt in body {
        let pad = "    ".repeat(depth);
        match &stmt.kind {
            StmtKind::Let { name, ty, expr, .. } if matches!(expr.kind, ExprKind::Match { .. }) => {
                let ExprKind::Match { value, arms } = &expr.kind else {
                    unreachable!()
                };
                lines.insert(
                    stmt.line,
                    format!(
                        "{pad}let {name}: {} = match {}:",
                        ty.name(),
                        format_expr(value, 0)
                    ),
                );
                format_match_expr_arms(arms, depth + 1, lines);
            }
            StmtKind::Var { name, ty, expr, .. } if matches!(expr.kind, ExprKind::Match { .. }) => {
                let ExprKind::Match { value, arms } = &expr.kind else {
                    unreachable!()
                };
                lines.insert(
                    stmt.line,
                    format!(
                        "{pad}var {name}: {} = match {}:",
                        ty.name(),
                        format_expr(value, 0)
                    ),
                );
                format_match_expr_arms(arms, depth + 1, lines);
            }
            StmtKind::Let { name, ty, expr, .. }
                if matches!(expr.kind, ExprKind::ListMatch { .. }) =>
            {
                let ExprKind::ListMatch { value, arms } = &expr.kind else {
                    unreachable!()
                };
                lines.insert(
                    stmt.line,
                    format!(
                        "{pad}let {name}: {} = match {}:",
                        ty.name(),
                        format_expr(value, 0)
                    ),
                );
                format_list_match_expr_arms(arms, depth + 1, lines);
            }
            StmtKind::Var { name, ty, expr, .. }
                if matches!(expr.kind, ExprKind::ListMatch { .. }) =>
            {
                let ExprKind::ListMatch { value, arms } = &expr.kind else {
                    unreachable!()
                };
                lines.insert(
                    stmt.line,
                    format!(
                        "{pad}var {name}: {} = match {}:",
                        ty.name(),
                        format_expr(value, 0)
                    ),
                );
                format_list_match_expr_arms(arms, depth + 1, lines);
            }
            StmtKind::Let { name, ty, expr, .. } => {
                lines.insert(
                    stmt.line,
                    format!("{pad}let {name}: {} = {}", ty.name(), format_expr(expr, 0)),
                );
            }
            StmtKind::Var { name, ty, expr, .. } => {
                lines.insert(
                    stmt.line,
                    format!("{pad}var {name}: {} = {}", ty.name(), format_expr(expr, 0)),
                );
            }
            StmtKind::Assign { name, expr, .. } => {
                lines.insert(stmt.line, format!("{pad}{name} = {}", format_expr(expr, 0)));
            }
            StmtKind::AssignMultiDestructure { bindings, expr } => {
                let pattern = bindings
                    .iter()
                    .map(|binding| binding.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.insert(
                    stmt.line,
                    format!("{pad}({pattern}) = {}", format_expr(expr, 0)),
                );
            }
            StmtKind::AssignListDestructure {
                bindings,
                rest,
                expr,
            } => {
                let mut pattern = bindings
                    .iter()
                    .map(|binding| binding.name.clone())
                    .collect::<Vec<_>>();
                if let Some(rest) = rest {
                    pattern.insert(rest.index, format!("...{}", rest.binding.name));
                }
                lines.insert(
                    stmt.line,
                    format!("{pad}[{}] = {}", pattern.join(", "), format_expr(expr, 0)),
                );
            }
            StmtKind::AssignStructDestructure {
                struct_name,
                fields,
                expr,
                ..
            } => {
                let fields = fields
                    .iter()
                    .map(format_struct_pattern_field)
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.insert(
                    stmt.line,
                    format!(
                        "{pad}{struct_name} {{ {fields} }} = {}",
                        format_expr(expr, 0)
                    ),
                );
            }
            StmtKind::LetDestructure {
                bindings,
                expr,
                else_return,
                mutable,
            } => {
                let bindings = bindings
                    .iter()
                    .map(|binding| format!("{}: {}", binding.name, binding.ty.name()))
                    .collect::<Vec<_>>()
                    .join(", ");
                let suffix = if *else_return { " else return" } else { "" };
                let keyword = if *mutable { "var" } else { "let" };
                lines.insert(
                    stmt.line,
                    format!(
                        "{pad}{keyword} {bindings} = {}{suffix}",
                        format_expr(expr, 0)
                    ),
                );
            }
            StmtKind::LetMultiDestructure {
                bindings,
                expr,
                else_return,
                mutable,
            } => {
                let pattern = bindings
                    .iter()
                    .map(|binding| binding.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                let suffix = if *else_return { " else return" } else { "" };
                let keyword = if *mutable { "var" } else { "let" };
                lines.insert(
                    stmt.line,
                    format!(
                        "{pad}{keyword} ({pattern}) = {}{suffix}",
                        format_expr(expr, 0)
                    ),
                );
            }
            StmtKind::LetListDestructure {
                bindings,
                rest,
                expr,
                mutable,
            } => {
                let mut pattern = bindings
                    .iter()
                    .map(|binding| binding.name.clone())
                    .collect::<Vec<_>>();
                if let Some(rest) = rest {
                    pattern.insert(rest.index, format!("...{}", rest.binding.name));
                }
                let keyword = if *mutable { "var" } else { "let" };
                lines.insert(
                    stmt.line,
                    format!(
                        "{pad}{keyword} [{}] = {}",
                        pattern.join(", "),
                        format_expr(expr, 0)
                    ),
                );
            }
            StmtKind::LetStructDestructure {
                struct_name,
                fields,
                expr,
                mutable,
                ..
            } => {
                let fields = fields
                    .iter()
                    .map(format_struct_pattern_field)
                    .collect::<Vec<_>>()
                    .join(", ");
                let keyword = if *mutable { "var" } else { "let" };
                lines.insert(
                    stmt.line,
                    format!(
                        "{pad}{keyword} {struct_name} {{ {fields} }} = {}",
                        format_expr(expr, 0)
                    ),
                );
            }
            StmtKind::Return(values)
                if values.len() == 1 && matches!(values[0].kind, ExprKind::Match { .. }) =>
            {
                let ExprKind::Match { value, arms } = &values[0].kind else {
                    unreachable!()
                };
                lines.insert(
                    stmt.line,
                    format!("{pad}return match {}:", format_expr(value, 0)),
                );
                format_match_expr_arms(arms, depth + 1, lines);
            }
            StmtKind::Return(values)
                if values.len() == 1 && matches!(values[0].kind, ExprKind::ListMatch { .. }) =>
            {
                let ExprKind::ListMatch { value, arms } = &values[0].kind else {
                    unreachable!()
                };
                lines.insert(
                    stmt.line,
                    format!("{pad}return match {}:", format_expr(value, 0)),
                );
                format_list_match_expr_arms(arms, depth + 1, lines);
            }
            StmtKind::Return(values) => {
                if values.is_empty() {
                    lines.insert(stmt.line, format!("{pad}return"));
                } else {
                    let values = values
                        .iter()
                        .map(|expr| format_expr(expr, 0))
                        .collect::<Vec<_>>()
                        .join(", ");
                    lines.insert(stmt.line, format!("{pad}return {values}"));
                }
            }
            StmtKind::Break => {
                lines.insert(stmt.line, format!("{pad}break"));
            }
            StmtKind::Continue => {
                lines.insert(stmt.line, format!("{pad}continue"));
            }
            StmtKind::Expr(expr) => {
                lines.insert(stmt.line, format!("{pad}{}", format_expr(expr, 0)));
            }
            StmtKind::Shell {
                expr,
                redirect,
                background,
            } => {
                let mut text = format!("{pad}{}", format_expr(expr, 0));
                if let Some(redirect) = redirect {
                    text.push_str(match redirect.mode {
                        ShellRedirectMode::Truncate => " > ",
                        ShellRedirectMode::Append => " >> ",
                    });
                    text.push_str(&format_expr(&redirect.path, 0));
                }
                if *background {
                    text.push_str(" &");
                }
                lines.insert(stmt.line, text);
            }
            StmtKind::If {
                cond,
                body,
                else_body,
                else_keyword_span,
            } => {
                let keyword = if stmt.keyword_span.length == 4 {
                    "elif"
                } else {
                    "if"
                };
                lines.insert(
                    stmt.line,
                    format!("{pad}{keyword} {}:", format_expr(cond, 0)),
                );
                format_block(body, depth + 1, lines);
                if !else_body.is_empty() {
                    let nested_elif = else_keyword_span.is_some_and(|span| {
                        else_body.first().is_some_and(|stmt| {
                            stmt.line == span.line && stmt.keyword_span.length == 4
                        })
                    });
                    if let Some(span) = else_keyword_span
                        && !nested_elif
                    {
                        lines.insert(span.line, format!("{pad}else:"));
                    }
                    format_block(
                        else_body,
                        if nested_elif { depth } else { depth + 1 },
                        lines,
                    );
                }
            }
            StmtKind::While { cond, body } => {
                lines.insert(stmt.line, format!("{pad}while {}:", format_expr(cond, 0)));
                format_block(body, depth + 1, lines);
            }
            StmtKind::ForRange {
                name,
                start,
                end,
                inclusive,
                body,
                ..
            } => {
                let range = if *inclusive { "..=" } else { ".." };
                lines.insert(
                    stmt.line,
                    format!(
                        "{pad}for {name} in {}{range}{}:",
                        format_expr(start, 0),
                        format_expr(end, 0)
                    ),
                );
                format_block(body, depth + 1, lines);
            }
            StmtKind::ForEach {
                index_name,
                name,
                iterable,
                body,
                ..
            } => {
                let bindings = index_name
                    .as_ref()
                    .map(|index| format!("{index}, {name}"))
                    .unwrap_or_else(|| name.clone());
                lines.insert(
                    stmt.line,
                    format!("{pad}for {bindings} in {}:", format_expr(iterable, 0)),
                );
                format_block(body, depth + 1, lines);
            }
            StmtKind::Match { value, arms } => {
                lines.insert(stmt.line, format!("{pad}match {}:", format_expr(value, 0)));
                let arm_pad = "    ".repeat(depth + 1);
                for arm in arms {
                    let patterns = arm
                        .patterns
                        .iter()
                        .map(format_match_pattern)
                        .collect::<Vec<_>>()
                        .join(", ");
                    let guard = arm
                        .guard
                        .as_ref()
                        .map(|guard| format!(" if {}", format_expr(guard, 0)))
                        .unwrap_or_default();
                    lines.insert(
                        arm.line,
                        format!(
                            "{arm_pad}{}.{}({patterns}){guard}:",
                            arm.enum_name, arm.variant
                        ),
                    );
                    format_block(&arm.body, depth + 2, lines);
                }
            }
            StmtKind::ListMatch { value, arms } => {
                lines.insert(stmt.line, format!("{pad}match {}:", format_expr(value, 0)));
                let arm_pad = "    ".repeat(depth + 1);
                for arm in arms {
                    let guard = arm
                        .guard
                        .as_ref()
                        .map(|guard| format!(" if {}", format_expr(guard, 0)))
                        .unwrap_or_default();
                    lines.insert(
                        arm.line,
                        format!(
                            "{arm_pad}{}{guard}:",
                            format_list_match_pattern(&arm.pattern)
                        ),
                    );
                    format_block(&arm.body, depth + 2, lines);
                }
            }
        }
    }
}

fn format_match_expr_arms(
    arms: &[crate::ast::MatchExprArm],
    depth: usize,
    lines: &mut HashMap<usize, String>,
) {
    let pad = "    ".repeat(depth);
    for arm in arms {
        let patterns = arm
            .patterns
            .iter()
            .map(format_match_pattern)
            .collect::<Vec<_>>()
            .join(", ");
        let guard = arm
            .guard
            .as_ref()
            .map(|guard| format!(" if {}", format_expr(guard, 0)))
            .unwrap_or_default();
        lines.insert(
            arm.line,
            format!(
                "{pad}{}.{}({patterns}){guard}: {}",
                arm.enum_name,
                arm.variant,
                format_expr(&arm.value, 0)
            ),
        );
    }
}

fn format_list_match_expr_arms(
    arms: &[crate::ast::ListMatchExprArm],
    depth: usize,
    lines: &mut HashMap<usize, String>,
) {
    let pad = "    ".repeat(depth);
    for arm in arms {
        let guard = arm
            .guard
            .as_ref()
            .map(|guard| format!(" if {}", format_expr(guard, 0)))
            .unwrap_or_default();
        lines.insert(
            arm.line,
            format!(
                "{pad}{}{guard}: {}",
                format_list_match_pattern(&arm.pattern),
                format_expr(&arm.value, 0)
            ),
        );
    }
}

fn format_match_pattern(pattern: &MatchPattern) -> String {
    match pattern {
        MatchPattern::Binding(binding) => binding.name.clone(),
        MatchPattern::Struct(pattern) => {
            let fields = pattern
                .fields
                .iter()
                .map(format_struct_pattern_field)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} {{ {} }}", pattern.struct_name, fields)
        }
        MatchPattern::Relational(pattern) => {
            let op = match pattern.op {
                BinOp::Eq => "==",
                BinOp::Ne => "!=",
                BinOp::Lt => "<",
                BinOp::Le => "<=",
                BinOp::Gt => ">",
                BinOp::Ge => ">=",
                _ => unreachable!("relational match pattern stores only comparison operators"),
            };
            format!("{op} {}", format_expr(&pattern.value, 0))
        }
        MatchPattern::Logical {
            left, op, right, ..
        } => {
            let op = match op {
                crate::ast::PatternLogicalOp::And => "&&",
                crate::ast::PatternLogicalOp::Or => "||",
            };
            format!(
                "{} {op} {}",
                format_match_pattern(left),
                format_match_pattern(right)
            )
        }
    }
}

fn format_list_match_pattern(pattern: &ListMatchPattern) -> String {
    match pattern {
        ListMatchPattern::Wildcard { .. } => "_".to_string(),
        ListMatchPattern::List { bindings, rest, .. } => {
            let mut entries = bindings
                .iter()
                .map(|binding| binding.name.clone())
                .collect::<Vec<_>>();
            if let Some(rest) = rest {
                entries.insert(rest.index, format!("...{}", rest.binding.name));
            }
            format!("[{}]", entries.join(", "))
        }
    }
}

fn format_return_types(types: &[Type]) -> String {
    match types {
        [] => "void".to_string(),
        [ty] => ty.name().to_string(),
        _ => format!(
            "({})",
            types.iter().map(Type::name).collect::<Vec<_>>().join(", ")
        ),
    }
}

fn format_struct_pattern_field(field: &StructPatternField) -> String {
    if let Some(nested) = &field.nested {
        let fields = nested
            .fields
            .iter()
            .map(format_struct_pattern_field)
            .collect::<Vec<_>>()
            .join(", ");
        return format!("{}: {} {{ {} }}", field.field, nested.struct_name, fields);
    }
    if field.field == field.binding.name {
        field.field.clone()
    } else {
        format!("{}: {}", field.field, field.binding.name)
    }
}

fn format_expr(expr: &Expr, parent_precedence: u8) -> String {
    match &expr.kind {
        ExprKind::Int(value) => value.to_string(),
        ExprKind::Bool(value) => value.to_string(),
        ExprKind::Str(value) => format_string(value),
        ExprKind::Nil => "nil".to_string(),
        ExprKind::Var(name) => name.clone(),
        ExprKind::AnonymousFunction {
            params,
            return_type,
            body,
        } => {
            let params = params
                .iter()
                .map(|param| format!("{}: {}", param.name, param.ty.name()))
                .collect::<Vec<_>>()
                .join(", ");
            let return_type = return_type
                .as_ref()
                .map(|ty| format!(" -> {}", ty.name()))
                .unwrap_or_default();
            format!("fn({params}){return_type} {{ {} }}", format_expr(body, 0))
        }
        ExprKind::ShellCall { name, args, .. } => {
            if args.is_empty() {
                name.clone()
            } else {
                format!(
                    "{name} {}",
                    args.iter()
                        .map(|arg| format_expr(arg, 7))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            }
        }
        ExprKind::Pipe {
            input, name, args, ..
        } => {
            let mut text = format!("{} | {name}", format_expr(input, 0));
            if !args.is_empty() {
                text.push(' ');
                text.push_str(
                    &args
                        .iter()
                        .map(|arg| format_expr(arg, 7))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
            if parent_precedence > 0 {
                format!("({text})")
            } else {
                text
            }
        }
        ExprKind::List(items) => format!(
            "[{}]",
            items
                .iter()
                .map(|item| format_expr(item, 0))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExprKind::ListSpread { value, .. } => format!("...{}", format_expr(value, 0)),
        ExprKind::ListIf {
            condition,
            value,
            else_value,
            ..
        } => {
            let mut text = format!(
                "if {}: {}",
                format_expr(condition, 0),
                format_expr(value, 0)
            );
            if let Some(else_value) = else_value {
                text.push_str(&format!(" else: {}", format_expr(else_value, 0)));
            }
            text
        }
        ExprKind::Index { base, index } => {
            format!("{}[{}]", format_expr(base, 7), format_expr(index, 0))
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            let base = format_expr(base, 7);
            let start = start
                .as_deref()
                .map(|value| format_expr(value, 0))
                .unwrap_or_default();
            let end = end
                .as_deref()
                .map(|value| format_expr(value, 0))
                .unwrap_or_default();
            if let Some(step) = step {
                format!("{base}[{start}:{end}:{}]", format_expr(step, 0))
            } else {
                format!("{base}[{start}:{end}]")
            }
        }
        ExprKind::ListComprehension {
            value,
            binding,
            iterable,
            condition,
            ..
        } => {
            let filter = condition
                .as_deref()
                .map(|condition| format!(" if {}", format_expr(condition, 0)))
                .unwrap_or_default();
            format!(
                "[{} for {binding} in {}{filter}]",
                format_expr(value, 0),
                format_expr(iterable, 0)
            )
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } => {
            let mut rendered = args
                .iter()
                .map(|arg| format_expr(arg, 0))
                .collect::<Vec<_>>();
            rendered.extend(
                named_args
                    .iter()
                    .map(|arg| format!("{}: {}", arg.name, format_expr(&arg.value, 0))),
            );
            format!("{name}({})", rendered.join(", "))
        }
        ExprKind::QualifiedCall {
            namespace,
            name,
            args,
            named_args,
            ..
        } => {
            let mut rendered = args
                .iter()
                .map(|arg| format_expr(arg, 0))
                .collect::<Vec<_>>();
            rendered.extend(
                named_args
                    .iter()
                    .map(|arg| format!("{}: {}", arg.name, format_expr(&arg.value, 0))),
            );
            format!("{namespace}.{name}({})", rendered.join(", "))
        }
        ExprKind::StructLiteral {
            name, base, fields, ..
        } => {
            let mut parts = Vec::with_capacity(fields.len() + usize::from(base.is_some()));
            if let Some(base) = base {
                parts.push(format!("..{}", format_expr(base, 0)));
            }
            parts.extend(
                fields
                    .iter()
                    .map(|field| format!("{}: {}", field.name, format_expr(&field.value, 0))),
            );
            format!("{name} {{ {} }}", parts.join(", "))
        }
        ExprKind::Match { value, .. } | ExprKind::ListMatch { value, .. } => {
            format!("match {}:", format_expr(value, 0))
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            let text = format!(
                "{} if {} else {}",
                format_expr(then_expr, 0),
                format_expr(cond, 0),
                format_expr(else_expr, 0)
            );
            if parent_precedence > 0 {
                format!("({text})")
            } else {
                text
            }
        }
        ExprKind::Field { base, name, .. } => format!("{}.{name}", format_expr(base, 7)),
        ExprKind::Unary { op, expr } => {
            let operator = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            };
            format!("{operator}{}", format_expr(expr, 7))
        }
        ExprKind::Binary { left, op, right } => {
            let precedence = binary_precedence(*op);
            let text = format!(
                "{} {} {}",
                format_expr(left, precedence),
                binary_text(*op),
                format_expr(right, precedence + 1)
            );
            if precedence < parent_precedence {
                format!("({text})")
            } else {
                text
            }
        }
    }
}

fn binary_precedence(op: BinOp) -> u8 {
    match op {
        BinOp::Or => 1,
        BinOp::And => 2,
        BinOp::Eq | BinOp::Ne => 3,
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 4,
        BinOp::Add | BinOp::Sub => 5,
        BinOp::Mul | BinOp::Div => 6,
    }
}

fn binary_text(op: BinOp) -> &'static str {
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

fn unescaped_triple_quote_count(input: &str) -> usize {
    let bytes = input.as_bytes();
    let mut index = 0usize;
    let mut count = 0usize;
    while index + 2 < bytes.len() {
        if bytes[index..].starts_with(b"\"\"\"") {
            let mut slashes = 0usize;
            let mut cursor = index;
            while cursor > 0 && bytes[cursor - 1] == b'\\' {
                slashes += 1;
                cursor -= 1;
            }
            if slashes.is_multiple_of(2) {
                count += 1;
                index += 3;
                continue;
            }
        }
        index += 1;
    }
    count
}

fn format_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}
