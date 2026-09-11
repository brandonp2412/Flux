use crate::ast::{
    ApplicationDef, ApplicationMetadataField, BinOp, Binding, ConstantDef, EnumDef, EnumPayload,
    EnumVariant, Expr, ExprKind, FlowDirection, Function, GridLayout, GridTrack, ImportDef,
    InterfaceDef, InterfaceFunction, InterfaceImpl, InterfaceImplMapping, InterfaceParent,
    ListMatchArm, ListMatchExprArm, ListMatchPattern, ListRestPattern, MatchArm, MatchExprArm,
    MatchPattern, NamedArg, Param, PatternBinding, PatternLogicalOp, Program, RelationalPattern,
    ShellRedirect, ShellRedirectMode, Stmt, StmtKind, StructDef, StructField, StructLiteralField,
    StructPattern, StructPatternField, Type, TypeAlias, UnaryOp, ViewDef, ViewDerived, ViewElement,
    ViewProperty, ViewState, ViewStateTransition,
};
use crate::diagnostic::{Diagnostic, DiagnosticStage, SourceId, SourceSpan};

pub const GRAMMAR_VERSION: u32 = 1;

#[derive(Debug, Clone)]
struct Line {
    number: usize,
    indent: usize,
    text: String,
}

#[derive(Debug, Clone)]
struct ParsedListDestructure {
    bindings: Vec<PatternBinding>,
    rest: Option<ListRestPattern>,
}

impl Line {
    fn span(&self) -> SourceSpan {
        SourceSpan::new(self.number, self.indent + 1, self.text.len().max(1))
    }
}

#[derive(Debug, Clone)]
pub struct ParseSnapshot {
    pub source_id: SourceId,
    pub fingerprint: u64,
    pub program: Option<Program>,
    pub diagnostics: Vec<Diagnostic>,
}

impl ParseSnapshot {
    pub fn is_valid(&self) -> bool {
        self.diagnostics.is_empty() && self.program.is_some()
    }
}

pub fn parse_snapshot(source: &str, source_id: SourceId) -> ParseSnapshot {
    let fingerprint = source_fingerprint(source);
    match parse_all_with_source(source, source_id) {
        Ok(program) => ParseSnapshot {
            source_id,
            fingerprint,
            program: Some(program),
            diagnostics: Vec::new(),
        },
        Err(diagnostics) => ParseSnapshot {
            source_id,
            fingerprint,
            program: None,
            diagnostics,
        },
    }
}

pub fn reparse_snapshot(previous: &ParseSnapshot, source: &str) -> ParseSnapshot {
    let fingerprint = source_fingerprint(source);
    if fingerprint == previous.fingerprint {
        return previous.clone();
    }
    parse_snapshot(source, previous.source_id)
}

pub fn parse(source: &str) -> Result<Program, Diagnostic> {
    parse_all(source).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .next()
            .expect("parse_all always returns at least one diagnostic on failure")
    })
}

pub fn parse_with_source(source: &str, source_id: SourceId) -> Result<Program, Diagnostic> {
    parse_all_with_source(source, source_id).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .next()
            .expect("parse_all_with_source always returns diagnostics on failure")
    })
}

pub fn parse_all_with_source(
    source: &str,
    source_id: SourceId,
) -> Result<Program, Vec<Diagnostic>> {
    match parse_all(source) {
        Ok(mut program) => {
            attach_program_source(&mut program, source_id);
            Ok(program)
        }
        Err(diagnostics) => Err(diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.with_source(source_id))
            .collect()),
    }
}

pub fn parse_all(source: &str) -> Result<Program, Vec<Diagnostic>> {
    let (lines, mut diagnostics) = preprocess(source);
    let mut imports = Vec::new();
    let mut aliases = Vec::new();
    let mut interfaces = Vec::new();
    let mut implementations = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut constants = Vec::new();
    let mut application = None;
    let mut views = Vec::new();
    let mut functions = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = &lines[index];
        if line.indent != 0 {
            diagnostics.push(diag(
                line.number,
                "top-level declarations must not be indented",
            ));
            index += 1;
            continue;
        }

        let (declaration_text, public, visibility_offset) = split_visibility(&line.text);

        if declaration_text.starts_with("import ") {
            if public {
                diagnostics.push(diag(line.number, "imports cannot be declared pub"));
                index += 1;
                continue;
            }
            match parse_import(line) {
                Ok(import) => imports.push(import),
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
            index += 1;
            continue;
        }

        if declaration_text.starts_with("app ") {
            if public {
                diagnostics.push(diag(line.number, "application declarations cannot be pub"));
                index += 1;
                continue;
            }
            match parse_application(line) {
                Ok(definition) if application.is_none() => application = Some(definition),
                Ok(_) => diagnostics.push(diag(line.number, "program may declare only one app")),
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
            index += 1;
            continue;
        }

        if declaration_text.starts_with("type ") {
            match parse_type_alias(line) {
                Ok(alias) => aliases.push(alias),
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
            index += 1;
            continue;
        }

        if declaration_text.starts_with("const ") {
            match parse_constant(line) {
                Ok(constant) => constants.push(constant),
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
            index += 1;
            continue;
        }

        if declaration_text.starts_with("interface ") {
            match parse_interface_declaration(&lines, &mut index) {
                Ok(definition) => interfaces.push(definition),
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    index = recover_after_malformed_declaration(&lines, index.saturating_add(1));
                }
            }
            continue;
        }

        if declaration_text.starts_with("impl ") {
            if public {
                diagnostics.push(diag(
                    line.number,
                    "interface implementations cannot be declared pub",
                ));
                index = recover_after_malformed_declaration(&lines, index.saturating_add(1));
                continue;
            }
            match parse_interface_implementation(&lines, &mut index) {
                Ok(implementation) => implementations.push(implementation),
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    index = recover_after_malformed_declaration(&lines, index.saturating_add(1));
                }
            }
            continue;
        }

        if declaration_text.starts_with("struct ") {
            match parse_struct_declaration(&lines, &mut index) {
                Ok(definition) => structs.push(definition),
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    index = recover_after_malformed_declaration(&lines, index.saturating_add(1));
                }
            }
            continue;
        }

        if declaration_text.starts_with("enum ") {
            match parse_enum_declaration(&lines, &mut index) {
                Ok(definition) => enums.push(definition),
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    index = recover_after_malformed_declaration(&lines, index.saturating_add(1));
                }
            }
            continue;
        }

        if declaration_text.starts_with("view ") {
            match parse_view_declaration(&lines, &mut index) {
                Ok(definition) => views.push(definition),
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    index = recover_after_malformed_declaration(&lines, index.saturating_add(1));
                }
            }
            continue;
        }

        match parse_single_expression_function(line) {
            Ok(Some(function)) => {
                functions.push(function);
                index += 1;
                continue;
            }
            Ok(None) => {}
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                index = recover_after_malformed_declaration(&lines, index + 1);
                continue;
            }
        }

        let header = match parse_function_header(declaration_text, line.number) {
            Ok(mut header) => {
                shift_function_header(&mut header, visibility_offset);
                header
            }
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                index = recover_after_malformed_declaration(&lines, index + 1);
                continue;
            }
        };
        let FunctionHeader {
            name,
            name_span,
            params,
            returns,
            return_span,
            return_type_spans,
        } = header;
        let function_line = line.number;
        let function_span = line.span();
        index += 1;

        let body = if index < lines.len() && lines[index].indent > 0 {
            let body_indent = lines[index].indent;
            match parse_block(&lines, &mut index, body_indent) {
                Ok(body) => body,
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    index = recover_after_malformed_declaration(&lines, index);
                    continue;
                }
            }
        } else {
            Vec::new()
        };

        if index >= lines.len() || lines[index].indent != 0 || lines[index].text != "}" {
            diagnostics.push(diag(
                function_line,
                "function body must end with a top-level '}'",
            ));
            index = recover_after_malformed_declaration(&lines, index);
            continue;
        }
        index += 1;

        functions.push(Function {
            public,
            name,
            name_span,
            keyword_span: SourceSpan::new(function_line, 1 + visibility_offset, 2),
            params,
            returns,
            return_span,
            return_type_spans,
            body,
            expression_body: false,
            line: function_line,
            span: function_span,
        });
    }

    if diagnostics.is_empty() {
        Ok(Program {
            imports,
            aliases,
            interfaces,
            implementations,
            structs,
            enums,
            constants,
            application,
            views,
            functions,
        })
    } else {
        Err(diagnostics)
    }
}

fn source_fingerprint(source: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in source.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn attach_program_source(program: &mut Program, source_id: SourceId) {
    for import in &mut program.imports {
        import.span = import.span.with_source(source_id);
        import.path_span = import.path_span.with_source(source_id);
    }
    if let Some(application) = &mut program.application {
        application.span = application.span.with_source(source_id);
        application.keyword_span = application.keyword_span.with_source(source_id);
        application.view_span = application.view_span.with_source(source_id);
        for field in &mut application.metadata {
            field.name_span = field.name_span.with_source(source_id);
            attach_expr_source(&mut field.value, source_id);
        }
    }
    for alias in &mut program.aliases {
        alias.span = alias.span.with_source(source_id);
        alias.name_span = alias.name_span.with_source(source_id);
        alias.target_span = alias.target_span.with_source(source_id);
    }
    for definition in &mut program.interfaces {
        definition.span = definition.span.with_source(source_id);
        definition.keyword_span = definition.keyword_span.with_source(source_id);
        definition.name_span = definition.name_span.with_source(source_id);
        for parent in &mut definition.parents {
            parent.span = parent.span.with_source(source_id);
        }
        for function in &mut definition.functions {
            function.span = function.span.with_source(source_id);
            function.keyword_span = function.keyword_span.with_source(source_id);
            function.name_span = function.name_span.with_source(source_id);
            function.return_span = function.return_span.with_source(source_id);
            for span in &mut function.return_type_spans {
                *span = span.with_source(source_id);
            }
            for param in &mut function.params {
                param.name_span = param.name_span.with_source(source_id);
                param.type_span = param.type_span.with_source(source_id);
            }
        }
    }
    for implementation in &mut program.implementations {
        implementation.span = implementation.span.with_source(source_id);
        implementation.keyword_span = implementation.keyword_span.with_source(source_id);
        implementation.interface_span = implementation.interface_span.with_source(source_id);
        implementation.target_span = implementation.target_span.with_source(source_id);
        for mapping in &mut implementation.mappings {
            mapping.member_span = mapping.member_span.with_source(source_id);
            mapping.function_span = mapping.function_span.with_source(source_id);
        }
    }
    for definition in &mut program.enums {
        definition.span = definition.span.with_source(source_id);
        definition.keyword_span = definition.keyword_span.with_source(source_id);
        definition.name_span = definition.name_span.with_source(source_id);
        for variant in &mut definition.variants {
            variant.name_span = variant.name_span.with_source(source_id);
            for payload in &mut variant.payloads {
                payload.type_span = payload.type_span.with_source(source_id);
            }
        }
    }
    for constant in &mut program.constants {
        constant.span = constant.span.with_source(source_id);
        constant.name_span = constant.name_span.with_source(source_id);
        constant.type_span = constant.type_span.with_source(source_id);
        attach_expr_source(&mut constant.value, source_id);
    }
    for definition in &mut program.structs {
        definition.span = definition.span.with_source(source_id);
        definition.keyword_span = definition.keyword_span.with_source(source_id);
        definition.name_span = definition.name_span.with_source(source_id);
        for field in &mut definition.fields {
            field.name_span = field.name_span.with_source(source_id);
            field.type_span = field.type_span.with_source(source_id);
        }
    }
    for view in &mut program.views {
        view.span = view.span.with_source(source_id);
        view.keyword_span = view.keyword_span.with_source(source_id);
        view.name_span = view.name_span.with_source(source_id);
        for param in &mut view.params {
            param.name_span = param.name_span.with_source(source_id);
            param.type_span = param.type_span.with_source(source_id);
            if let Some(default) = &mut param.default {
                attach_expr_source(default, source_id);
            }
        }
        for state in &mut view.states {
            state.span = state.span.with_source(source_id);
            state.name_span = state.name_span.with_source(source_id);
            state.type_span = state.type_span.with_source(source_id);
            attach_expr_source(&mut state.initial, source_id);
        }
        for derived in &mut view.derived {
            derived.span = derived.span.with_source(source_id);
            derived.name_span = derived.name_span.with_source(source_id);
            derived.type_span = derived.type_span.with_source(source_id);
            attach_expr_source(&mut derived.value, source_id);
        }
        for element in &mut view.elements {
            element.span = element.span.with_source(source_id);
            element.kind_span = element.kind_span.with_source(source_id);
            element.name_span = element.name_span.with_source(source_id);
            for property in &mut element.properties {
                property.span = property.span.with_source(source_id);
                property.name_span = property.name_span.with_source(source_id);
                if let Some(transition) = &mut property.transition {
                    transition.state_span = transition.state_span.with_source(source_id);
                }
                attach_expr_source(&mut property.value, source_id);
            }
        }
    }
    for function in &mut program.functions {
        function.span = function.span.with_source(source_id);
        function.keyword_span = function.keyword_span.with_source(source_id);
        function.name_span = function.name_span.with_source(source_id);
        function.return_span = function.return_span.with_source(source_id);
        for span in &mut function.return_type_spans {
            *span = span.with_source(source_id);
        }
        for param in &mut function.params {
            param.name_span = param.name_span.with_source(source_id);
            param.type_span = param.type_span.with_source(source_id);
            if let Some(default) = &mut param.default {
                attach_expr_source(default, source_id);
            }
        }
        attach_block_source(&mut function.body, source_id);
    }
}

fn attach_block_source(body: &mut [Stmt], source_id: SourceId) {
    for stmt in body {
        stmt.span = stmt.span.with_source(source_id);
        stmt.keyword_span = stmt.keyword_span.with_source(source_id);
        match &mut stmt.kind {
            StmtKind::Let {
                name_span,
                type_span,
                expr,
                ..
            }
            | StmtKind::Var {
                name_span,
                type_span,
                expr,
                ..
            } => {
                *name_span = name_span.with_source(source_id);
                *type_span = type_span.with_source(source_id);
                attach_expr_source(expr, source_id);
            }
            StmtKind::Assign {
                name_span, expr, ..
            } => {
                *name_span = name_span.with_source(source_id);
                attach_expr_source(expr, source_id);
            }
            StmtKind::AssignMultiDestructure { bindings, expr } => {
                for binding in bindings {
                    binding.span = binding.span.with_source(source_id);
                }
                attach_expr_source(expr, source_id);
            }
            StmtKind::AssignListDestructure {
                bindings,
                rest,
                expr,
            } => {
                for binding in bindings {
                    binding.span = binding.span.with_source(source_id);
                }
                if let Some(rest) = rest {
                    rest.binding.span = rest.binding.span.with_source(source_id);
                }
                attach_expr_source(expr, source_id);
            }
            StmtKind::AssignStructDestructure {
                struct_span,
                fields,
                expr,
                ..
            } => {
                *struct_span = struct_span.with_source(source_id);
                for field in fields {
                    attach_struct_pattern_field_source(field, source_id);
                }
                attach_expr_source(expr, source_id);
            }
            StmtKind::LetDestructure { bindings, expr, .. } => {
                for binding in bindings {
                    binding.name_span = binding.name_span.with_source(source_id);
                    binding.type_span = binding.type_span.with_source(source_id);
                }
                attach_expr_source(expr, source_id);
            }
            StmtKind::LetMultiDestructure { bindings, expr, .. } => {
                for binding in bindings {
                    binding.span = binding.span.with_source(source_id);
                }
                attach_expr_source(expr, source_id);
            }
            StmtKind::LetListDestructure {
                bindings,
                rest,
                expr,
                ..
            } => {
                for binding in bindings {
                    binding.span = binding.span.with_source(source_id);
                }
                if let Some(rest) = rest {
                    rest.binding.span = rest.binding.span.with_source(source_id);
                }
                attach_expr_source(expr, source_id);
            }
            StmtKind::LetStructDestructure {
                struct_span,
                fields,
                expr,
                ..
            } => {
                *struct_span = struct_span.with_source(source_id);
                for field in fields {
                    attach_struct_pattern_field_source(field, source_id);
                }
                attach_expr_source(expr, source_id);
            }
            StmtKind::Return(expressions) => {
                for expr in expressions {
                    attach_expr_source(expr, source_id);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Expr(expr) => attach_expr_source(expr, source_id),
            StmtKind::Shell { expr, redirect, .. } => {
                attach_expr_source(expr, source_id);
                if let Some(redirect) = redirect {
                    attach_expr_source(&mut redirect.path, source_id);
                }
            }
            StmtKind::If {
                cond,
                body,
                else_body,
                else_keyword_span,
            } => {
                if let Some(span) = else_keyword_span {
                    *span = span.with_source(source_id);
                }
                attach_expr_source(cond, source_id);
                attach_block_source(body, source_id);
                attach_block_source(else_body, source_id);
            }
            StmtKind::ForRange {
                name_span,
                start,
                end,
                body,
                ..
            } => {
                *name_span = name_span.with_source(source_id);
                attach_expr_source(start, source_id);
                attach_expr_source(end, source_id);
                attach_block_source(body, source_id);
            }
            StmtKind::ForEach {
                index_span,
                name_span,
                iterable,
                body,
                ..
            } => {
                if let Some(span) = index_span {
                    *span = span.with_source(source_id);
                }
                *name_span = name_span.with_source(source_id);
                attach_expr_source(iterable, source_id);
                attach_block_source(body, source_id);
            }
            StmtKind::While { cond, body } => {
                attach_expr_source(cond, source_id);
                attach_block_source(body, source_id);
            }
            StmtKind::Match { value, arms } => {
                attach_expr_source(value, source_id);
                for arm in arms {
                    arm.span = arm.span.with_source(source_id);
                    arm.enum_span = arm.enum_span.with_source(source_id);
                    arm.variant_span = arm.variant_span.with_source(source_id);
                    for pattern in &mut arm.patterns {
                        attach_match_pattern_source(pattern, source_id);
                    }
                    if let Some(guard) = &mut arm.guard {
                        attach_expr_source(guard, source_id);
                    }
                    attach_block_source(&mut arm.body, source_id);
                }
            }
            StmtKind::ListMatch { value, arms } => {
                attach_expr_source(value, source_id);
                for arm in arms {
                    arm.span = arm.span.with_source(source_id);
                    attach_list_match_pattern_source(&mut arm.pattern, source_id);
                    if let Some(guard) = &mut arm.guard {
                        attach_expr_source(guard, source_id);
                    }
                    attach_block_source(&mut arm.body, source_id);
                }
            }
        }
    }
}

fn attach_expr_source(expr: &mut Expr, source_id: SourceId) {
    expr.span = expr.span.with_source(source_id);
    match &mut expr.kind {
        ExprKind::AnonymousFunction { params, body, .. } => {
            for param in params {
                param.name_span = param.name_span.with_source(source_id);
                param.type_span = param.type_span.with_source(source_id);
            }
            attach_expr_source(body, source_id);
        }
        ExprKind::Call {
            args, named_args, ..
        } => {
            for arg in args {
                attach_expr_source(arg, source_id);
            }
            for arg in named_args {
                arg.name_span = arg.name_span.with_source(source_id);
                attach_expr_source(&mut arg.value, source_id);
            }
        }
        ExprKind::ShellCall {
            name_span, args, ..
        } => {
            *name_span = name_span.with_source(source_id);
            for arg in args {
                attach_expr_source(arg, source_id);
            }
        }
        ExprKind::Pipe {
            input,
            name_span,
            args,
            ..
        } => {
            attach_expr_source(input, source_id);
            *name_span = name_span.with_source(source_id);
            for arg in args {
                attach_expr_source(arg, source_id);
            }
        }
        ExprKind::List(items) => {
            for item in items {
                attach_expr_source(item, source_id);
            }
        }
        ExprKind::ListSpread { value, spread_span } => {
            *spread_span = spread_span.with_source(source_id);
            attach_expr_source(value, source_id);
        }
        ExprKind::ListIf {
            condition,
            value,
            else_value,
            if_span,
            else_span,
        } => {
            *if_span = if_span.with_source(source_id);
            *else_span = else_span.map(|span| span.with_source(source_id));
            attach_expr_source(condition, source_id);
            attach_expr_source(value, source_id);
            if let Some(else_value) = else_value {
                attach_expr_source(else_value, source_id);
            }
        }
        ExprKind::Index { base, index } => {
            attach_expr_source(base, source_id);
            attach_expr_source(index, source_id);
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            attach_expr_source(base, source_id);
            if let Some(start) = start {
                attach_expr_source(start, source_id);
            }
            if let Some(end) = end {
                attach_expr_source(end, source_id);
            }
            if let Some(step) = step {
                attach_expr_source(step, source_id);
            }
        }
        ExprKind::ListComprehension {
            value,
            binding_span,
            iterable,
            condition,
            ..
        } => {
            attach_expr_source(value, source_id);
            *binding_span = binding_span.with_source(source_id);
            attach_expr_source(iterable, source_id);
            if let Some(condition) = condition {
                attach_expr_source(condition, source_id);
            }
        }
        ExprKind::StructLiteral {
            name_span,
            base,
            fields,
            ..
        } => {
            *name_span = name_span.with_source(source_id);
            if let Some(base) = base {
                attach_expr_source(base, source_id);
            }
            for field in fields {
                field.name_span = field.name_span.with_source(source_id);
                attach_expr_source(&mut field.value, source_id);
            }
        }
        ExprKind::QualifiedCall {
            namespace_span,
            name_span,
            args,
            named_args,
            ..
        } => {
            *namespace_span = namespace_span.with_source(source_id);
            *name_span = name_span.with_source(source_id);
            for arg in args {
                attach_expr_source(arg, source_id);
            }
            for arg in named_args {
                arg.name_span = arg.name_span.with_source(source_id);
                attach_expr_source(&mut arg.value, source_id);
            }
        }
        ExprKind::Field {
            base, name_span, ..
        } => {
            *name_span = name_span.with_source(source_id);
            attach_expr_source(base, source_id);
        }
        ExprKind::Match { value, arms } => {
            attach_expr_source(value, source_id);
            for arm in arms {
                arm.span = arm.span.with_source(source_id);
                arm.enum_span = arm.enum_span.with_source(source_id);
                arm.variant_span = arm.variant_span.with_source(source_id);
                for pattern in &mut arm.patterns {
                    attach_match_pattern_source(pattern, source_id);
                }
                if let Some(guard) = &mut arm.guard {
                    attach_expr_source(guard, source_id);
                }
                attach_expr_source(&mut arm.value, source_id);
            }
        }
        ExprKind::ListMatch { value, arms } => {
            attach_expr_source(value, source_id);
            for arm in arms {
                arm.span = arm.span.with_source(source_id);
                attach_list_match_pattern_source(&mut arm.pattern, source_id);
                if let Some(guard) = &mut arm.guard {
                    attach_expr_source(guard, source_id);
                }
                attach_expr_source(&mut arm.value, source_id);
            }
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            attach_expr_source(then_expr, source_id);
            attach_expr_source(cond, source_id);
            attach_expr_source(else_expr, source_id);
        }
        ExprKind::Unary { expr, .. } => attach_expr_source(expr, source_id),
        ExprKind::Binary { left, right, .. } => {
            attach_expr_source(left, source_id);
            attach_expr_source(right, source_id);
        }
        ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::None
        | ExprKind::Var(_) => {}
    }
}

fn preprocess(source: &str) -> (Vec<Line>, Vec<Diagnostic>) {
    let mut lines = Vec::new();
    let mut diagnostics = Vec::new();
    let mut multiline: Option<Line> = None;

    for (index, raw) in source.lines().enumerate() {
        let number = index + 1;

        if let Some(line) = multiline.as_mut() {
            line.text.push('\n');
            line.text.push_str(raw);
            if unescaped_triple_quote_count(raw) % 2 == 1 {
                let completed = multiline.take().expect("multiline line exists");
                push_preprocessed_line(completed, &mut lines, &mut diagnostics);
            }
            continue;
        }

        if raw.contains('\t') {
            diagnostics.push(diag(
                number,
                "tabs are not allowed for indentation; use spaces",
            ));
            continue;
        }

        let indent = raw.bytes().take_while(|byte| *byte == b' ').count();
        let source_text = &raw[indent..];
        if unescaped_triple_quote_count(source_text) % 2 == 1 {
            multiline = Some(Line {
                number,
                indent,
                text: source_text.to_string(),
            });
            continue;
        }

        push_preprocessed_line(
            Line {
                number,
                indent,
                text: source_text.to_string(),
            },
            &mut lines,
            &mut diagnostics,
        );
    }

    if let Some(line) = multiline {
        diagnostics.push(Diagnostic::new(
            DiagnosticStage::Parse,
            SourceSpan::new(line.number, line.indent + 1, 3),
            "unterminated multiline string literal",
        ));
    }

    (lines, diagnostics)
}

fn push_preprocessed_line(
    mut line: Line,
    lines: &mut Vec<Line>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(offset) = comment_marker(&line.text) {
        diagnostics.push(Diagnostic::new(
            DiagnosticStage::Parse,
            SourceSpan::new(line.number, line.indent + offset + 1, 1),
            "comments are not part of Flux syntax; remove '#...' text",
        ));
        return;
    }
    line.text = line.text.trim_end().to_string();
    if line.text.trim().is_empty() {
        return;
    }
    lines.push(line);
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

fn split_visibility(input: &str) -> (&str, bool, usize) {
    input
        .strip_prefix("pub ")
        .map(|rest| (rest, true, 4))
        .unwrap_or((input, false, 0))
}

fn shift_expr_columns(expr: &mut Expr, offset: usize) {
    if offset == 0 {
        return;
    }
    expr.span.column += offset;
    match &mut expr.kind {
        ExprKind::AnonymousFunction { params, body, .. } => {
            for param in params {
                param.name_span.column += offset;
                param.type_span.column += offset;
            }
            shift_expr_columns(body, offset);
        }
        ExprKind::Call {
            args, named_args, ..
        } => {
            for arg in args {
                shift_expr_columns(arg, offset);
            }
            for arg in named_args {
                arg.name_span.column += offset;
                shift_expr_columns(&mut arg.value, offset);
            }
        }
        ExprKind::ShellCall {
            name_span, args, ..
        } => {
            name_span.column += offset;
            for arg in args {
                shift_expr_columns(arg, offset);
            }
        }
        ExprKind::Pipe {
            input,
            name_span,
            args,
            ..
        } => {
            shift_expr_columns(input, offset);
            name_span.column += offset;
            for arg in args {
                shift_expr_columns(arg, offset);
            }
        }
        ExprKind::List(items) => {
            for item in items {
                shift_expr_columns(item, offset);
            }
        }
        ExprKind::ListSpread { value, spread_span } => {
            spread_span.column += offset;
            shift_expr_columns(value, offset);
        }
        ExprKind::ListIf {
            condition,
            value,
            else_value,
            if_span,
            else_span,
        } => {
            if_span.column += offset;
            if let Some(else_span) = else_span {
                else_span.column += offset;
            }
            shift_expr_columns(condition, offset);
            shift_expr_columns(value, offset);
            if let Some(else_value) = else_value {
                shift_expr_columns(else_value, offset);
            }
        }
        ExprKind::Index { base, index } => {
            shift_expr_columns(base, offset);
            shift_expr_columns(index, offset);
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            shift_expr_columns(base, offset);
            if let Some(start) = start {
                shift_expr_columns(start, offset);
            }
            if let Some(end) = end {
                shift_expr_columns(end, offset);
            }
            if let Some(step) = step {
                shift_expr_columns(step, offset);
            }
        }
        ExprKind::ListComprehension {
            value,
            binding_span,
            iterable,
            condition,
            ..
        } => {
            shift_expr_columns(value, offset);
            binding_span.column += offset;
            shift_expr_columns(iterable, offset);
            if let Some(condition) = condition {
                shift_expr_columns(condition, offset);
            }
        }
        ExprKind::StructLiteral {
            name_span,
            base,
            fields,
            ..
        } => {
            name_span.column += offset;
            if let Some(base) = base {
                shift_expr_columns(base, offset);
            }
            for field in fields {
                field.name_span.column += offset;
                shift_expr_columns(&mut field.value, offset);
            }
        }
        ExprKind::QualifiedCall {
            namespace_span,
            name_span,
            args,
            named_args,
            ..
        } => {
            namespace_span.column += offset;
            name_span.column += offset;
            for arg in args {
                shift_expr_columns(arg, offset);
            }
            for arg in named_args {
                arg.name_span.column += offset;
                shift_expr_columns(&mut arg.value, offset);
            }
        }
        ExprKind::Field {
            base, name_span, ..
        } => {
            name_span.column += offset;
            shift_expr_columns(base, offset);
        }
        ExprKind::Match { value, arms } => {
            shift_expr_columns(value, offset);
            for arm in arms {
                arm.enum_span.column += offset;
                arm.variant_span.column += offset;
                arm.span.column += offset;
                for pattern in &mut arm.patterns {
                    shift_match_pattern_columns(pattern, offset);
                }
                if let Some(guard) = &mut arm.guard {
                    shift_expr_columns(guard, offset);
                }
                shift_expr_columns(&mut arm.value, offset);
            }
        }
        ExprKind::ListMatch { value, arms } => {
            shift_expr_columns(value, offset);
            for arm in arms {
                arm.span.column += offset;
                shift_list_match_pattern_columns(&mut arm.pattern, offset);
                if let Some(guard) = &mut arm.guard {
                    shift_expr_columns(guard, offset);
                }
                shift_expr_columns(&mut arm.value, offset);
            }
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            shift_expr_columns(then_expr, offset);
            shift_expr_columns(cond, offset);
            shift_expr_columns(else_expr, offset);
        }
        ExprKind::Unary { expr, .. } => shift_expr_columns(expr, offset),
        ExprKind::Binary { left, right, .. } => {
            shift_expr_columns(left, offset);
            shift_expr_columns(right, offset);
        }
        ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::None
        | ExprKind::Var(_) => {}
    }
}

fn shift_match_pattern_columns(pattern: &mut MatchPattern, offset: usize) {
    match pattern {
        MatchPattern::Binding(binding) => binding.span.column += offset,
        MatchPattern::Relational(pattern) => {
            pattern.span.column += offset;
            shift_expr_columns(&mut pattern.value, offset);
        }
        MatchPattern::Logical {
            left, right, span, ..
        } => {
            span.column += offset;
            shift_match_pattern_columns(left, offset);
            shift_match_pattern_columns(right, offset);
        }
        MatchPattern::Struct(pattern) => {
            pattern.struct_span.column += offset;
            for field in &mut pattern.fields {
                field.field_span.column += offset;
                field.binding.span.column += offset;
                if let Some(nested) = &mut field.nested {
                    nested.struct_span.column += offset;
                    for nested_field in &mut nested.fields {
                        shift_struct_pattern_field_columns(nested_field, offset);
                    }
                }
            }
        }
    }
}

fn shift_list_match_pattern_columns(pattern: &mut ListMatchPattern, offset: usize) {
    match pattern {
        ListMatchPattern::List {
            bindings,
            rest,
            span,
        } => {
            span.column += offset;
            for binding in bindings {
                binding.span.column += offset;
            }
            if let Some(rest) = rest {
                rest.binding.span.column += offset;
            }
        }
        ListMatchPattern::Wildcard { span } => span.column += offset,
    }
}

fn shift_struct_pattern_field_columns(field: &mut StructPatternField, offset: usize) {
    field.field_span.column += offset;
    field.binding.span.column += offset;
    if let Some(nested) = &mut field.nested {
        nested.struct_span.column += offset;
        for field in &mut nested.fields {
            shift_struct_pattern_field_columns(field, offset);
        }
    }
}

fn shift_function_header(header: &mut FunctionHeader, offset: usize) {
    if offset == 0 {
        return;
    }
    header.name_span.column += offset;
    header.return_span.column += offset;
    for span in &mut header.return_type_spans {
        span.column += offset;
    }
    for param in &mut header.params {
        param.name_span.column += offset;
        param.type_span.column += offset;
        if let Some(default) = &mut param.default {
            shift_expr_columns(default, offset);
        }
    }
}

fn recover_after_malformed_declaration(lines: &[Line], mut index: usize) -> usize {
    while index < lines.len() {
        let line = &lines[index];
        if line.indent == 0 {
            if line.text == "}" {
                return index + 1;
            }
            let (text, _, _) = split_visibility(&line.text);
            if text.starts_with("fn ")
                || text.starts_with("import ")
                || text.starts_with("interface ")
                || text.starts_with("impl ")
                || text.starts_with("struct ")
                || text.starts_with("enum ")
                || text.starts_with("app ")
                || text.starts_with("type ")
                || text.starts_with("const ")
                || text.starts_with("view ")
            {
                return index;
            }
        }
        index += 1;
    }
    index
}

fn parse_application(line: &Line) -> Result<ApplicationDef, Diagnostic> {
    let Some(raw) = line.text.strip_prefix("app ") else {
        return Err(diag(line.number, "expected application declaration"));
    };
    let raw = raw.trim();
    let (name, metadata_source, metadata_column) = if let Some(open) = raw.find('(') {
        let Some(inner) = raw.strip_suffix(')') else {
            return Err(diag(line.number, "application metadata must end with ')'"));
        };
        let name = raw[..open].trim();
        let metadata = &inner[open + 1..];
        let column = line.text.find('(').unwrap_or(4) + 2;
        (name, Some(metadata), column)
    } else {
        (raw, None, line.text.len() + 1)
    };
    validate_identifier(name, line.number)?;
    let name_offset = line.text.find(name).unwrap_or(4);
    let mut metadata = Vec::new();
    if let Some(source) = metadata_source
        && !source.trim().is_empty()
    {
        for (raw_field, offset) in split_top_level_commas_with_offsets(source) {
            let Some(colon) = raw_field.find(':') else {
                return Err(diag(
                    line.number,
                    "application metadata uses 'name: value' fields",
                ));
            };
            let (field_name, name_column) =
                trim_with_column(&raw_field[..colon], metadata_column + offset);
            validate_identifier(field_name, line.number)?;
            if !matches!(
                field_name,
                "id" | "title"
                    | "width"
                    | "height"
                    | "resizable"
                    | "theme"
                    | "layoutDirection"
                    | "surfaceColor"
                    | "surfaceRaisedColor"
                    | "textColor"
                    | "textMutedColor"
                    | "accentColor"
                    | "onAccentColor"
                    | "outlineColor"
                    | "dangerColor"
                    | "successColor"
                    | "warningColor"
                    | "shadowColor"
                    | "onStart"
                    | "onResume"
                    | "onPause"
                    | "onStop"
                    | "onExit"
                    | "onConfigurationChanged"
                    | "onLowMemory"
                    | "onBackgroundJob"
                    | "onSaveState"
                    | "onRestoreState"
                    | "onOpenUrl"
            ) {
                return Err(diag(
                    line.number,
                    &format!("unknown application metadata field '{field_name}'"),
                ));
            }
            if metadata
                .iter()
                .any(|field: &ApplicationMetadataField| field.name == field_name)
            {
                return Err(diag(
                    line.number,
                    &format!("duplicate application metadata field '{field_name}'"),
                ));
            }
            let raw_value = &raw_field[colon + 1..];
            let (value_source, value_column) =
                trim_with_column(raw_value, metadata_column + offset + colon + 1);
            if value_source.is_empty() {
                return Err(diag(
                    line.number,
                    &format!("application metadata '{field_name}' requires a value"),
                ));
            }
            metadata.push(ApplicationMetadataField {
                name: field_name.to_string(),
                name_span: SourceSpan::new(line.number, name_column, field_name.len()),
                value: parse_expression_at(value_source, line.number, value_column)?,
            });
        }
    }
    Ok(ApplicationDef {
        view_name: name.to_string(),
        view_span: SourceSpan::new(line.number, name_offset + 1, name.len()),
        keyword_span: SourceSpan::new(line.number, 1, 3),
        metadata,
        line: line.number,
        span: line.span(),
    })
}

fn parse_import(line: &Line) -> Result<ImportDef, Diagnostic> {
    let Some(rest) = line.text.strip_prefix("import ") else {
        return Err(diag(line.number, "expected import declaration"));
    };
    let leading = rest.len() - rest.trim_start().len();
    let source = rest.trim();
    let expression = parse_expression_at(source, line.number, 8 + leading)?;
    let ExprKind::Str(path) = expression.kind else {
        return Err(diag(
            line.number,
            "imports use a quoted module path: import \"file.flux\" or import \"pkg:dependency/module.flux\"",
        ));
    };
    if path.is_empty() {
        return Err(diag(line.number, "import path cannot be empty"));
    }
    Ok(ImportDef {
        path,
        path_span: expression.span,
        resolved_source_id: None,
        line: line.number,
        span: line.span(),
    })
}

fn parse_interface_declaration(
    lines: &[Line],
    index: &mut usize,
) -> Result<InterfaceDef, Diagnostic> {
    let header = &lines[*index];
    let (header_text, public, visibility_offset) = split_visibility(&header.text);
    let Some(rest) = header_text.strip_prefix("interface ") else {
        return Err(diag(header.number, "expected interface declaration"));
    };
    let Some(raw_name) = rest.strip_suffix('{') else {
        return Err(diag(
            header.number,
            "interface declarations must open their body with '{'",
        ));
    };
    let header_body = raw_name.trim();
    let (name, parents_source) = if let Some(colon_offset) = header_body.find(':') {
        (
            header_body[..colon_offset].trim(),
            Some(header_body[colon_offset + 1..].trim()),
        )
    } else {
        (header_body, None)
    };
    validate_identifier(name, header.number)?;
    let leading = raw_name.len() - raw_name.trim_start().len();
    let name_span = SourceSpan::new(header.number, visibility_offset + 11 + leading, name.len());
    let mut parents = Vec::new();
    if let Some(parents_source) = parents_source {
        if parents_source.is_empty() {
            return Err(diag(
                header.number,
                "interface composition requires at least one parent after ':'",
            ));
        }
        let parents_column = header_text
            .find(':')
            .expect("interface composition colon was parsed")
            + visibility_offset
            + 2;
        for (raw_parent, offset) in split_top_level_commas_with_offsets(parents_source) {
            let (parent, parent_column) = trim_with_column(raw_parent, parents_column + offset);
            validate_identifier(parent, header.number)?;
            if parents
                .iter()
                .any(|existing: &InterfaceParent| existing.name == parent)
            {
                return Err(diag(
                    header.number,
                    &format!("duplicate composed interface '{parent}'"),
                ));
            }
            parents.push(InterfaceParent {
                name: parent.to_string(),
                span: SourceSpan::new(header.number, parent_column, parent.len()),
            });
        }
    }
    let definition_span = header.span();
    *index += 1;

    let mut functions = Vec::new();
    if *index < lines.len() && lines[*index].indent > 0 {
        let member_indent = lines[*index].indent;
        while *index < lines.len() {
            let line = &lines[*index];
            if line.indent < member_indent {
                break;
            }
            if line.indent > member_indent {
                return Err(diag(
                    line.number,
                    "unexpected indentation in interface body",
                ));
            }
            if line.text == "}" {
                break;
            }
            if !line.text.starts_with("fn ") {
                return Err(diag(
                    line.number,
                    "interface bodies contain only function signatures",
                ));
            }
            if line.text.ends_with('{') {
                return Err(diag(
                    line.number,
                    "interface function signatures do not have bodies",
                ));
            }
            let synthetic = format!("{} {{", line.text);
            let mut parsed = parse_function_header(&synthetic, line.number)?;
            parsed.name_span.column += line.indent;
            parsed.return_span.column += line.indent;
            for span in &mut parsed.return_type_spans {
                span.column += line.indent;
            }
            for param in &mut parsed.params {
                param.name_span.column += line.indent;
                param.type_span.column += line.indent;
            }
            if parsed.params.iter().any(|param| param.default.is_some()) {
                return Err(diag(
                    line.number,
                    "interface function parameters cannot declare defaults",
                ));
            }
            if functions
                .iter()
                .any(|existing: &InterfaceFunction| existing.name == parsed.name)
            {
                return Err(diag(
                    line.number,
                    &format!("duplicate interface function '{}'", parsed.name),
                ));
            }
            functions.push(InterfaceFunction {
                name: parsed.name,
                name_span: parsed.name_span,
                keyword_span: SourceSpan::new(line.number, line.indent + 1, 2),
                params: parsed.params,
                returns: parsed.returns,
                return_span: parsed.return_span,
                return_type_spans: parsed.return_type_spans,
                line: line.number,
                span: line.span(),
            });
            *index += 1;
        }
    }

    if functions.is_empty() && parents.is_empty() {
        return Err(diag(
            header.number,
            "interface declarations require at least one function signature or composed interface",
        ));
    }
    if *index >= lines.len() || lines[*index].indent != 0 || lines[*index].text != "}" {
        return Err(diag(
            header.number,
            "interface body must end with a top-level '}'",
        ));
    }
    *index += 1;

    Ok(InterfaceDef {
        public,
        name: name.to_string(),
        name_span,
        keyword_span: SourceSpan::new(header.number, 1 + visibility_offset, 9),
        parents,
        functions,
        line: header.number,
        span: definition_span,
    })
}

fn parse_interface_implementation(
    lines: &[Line],
    index: &mut usize,
) -> Result<InterfaceImpl, Diagnostic> {
    let header = &lines[*index];
    let Some(rest) = header.text.strip_prefix("impl ") else {
        return Err(diag(header.number, "expected interface implementation"));
    };
    let Some(raw_header) = rest.strip_suffix('{') else {
        return Err(diag(
            header.number,
            "interface implementations use 'impl Interface for Type {'",
        ));
    };
    let raw_header = raw_header.trim_end();
    let Some(for_offset) = raw_header.find(" for ") else {
        return Err(diag(
            header.number,
            "interface implementations use 'impl Interface for Type {'",
        ));
    };
    let interface_name = raw_header[..for_offset].trim();
    let target_name = raw_header[for_offset + 5..].trim();
    validate_identifier(interface_name, header.number)?;
    validate_identifier(target_name, header.number)?;
    let interface_offset = rest.find(interface_name).unwrap_or(0);
    let target_offset = rest.find(target_name).unwrap_or(for_offset + 5);
    let definition_span = header.span();
    *index += 1;

    let mut mappings = Vec::new();
    if *index < lines.len() && lines[*index].indent > 0 {
        let mapping_indent = lines[*index].indent;
        while *index < lines.len() {
            let line = &lines[*index];
            if line.indent < mapping_indent {
                break;
            }
            if line.indent > mapping_indent {
                return Err(diag(
                    line.number,
                    "unexpected indentation in interface implementation",
                ));
            }
            let Some(colon_offset) = line.text.find(':') else {
                return Err(diag(
                    line.number,
                    "interface implementation mappings use 'member: function'",
                ));
            };
            let (member, member_column) =
                trim_with_column(&line.text[..colon_offset], line.indent + 1);
            let (function, function_column) = trim_with_column(
                &line.text[colon_offset + 1..],
                line.indent + 1 + colon_offset + 1,
            );
            validate_identifier(member, line.number)?;
            validate_identifier(function, line.number)?;
            if mappings
                .iter()
                .any(|mapping: &InterfaceImplMapping| mapping.member == member)
            {
                return Err(diag(
                    line.number,
                    &format!("duplicate interface implementation mapping '{member}'"),
                ));
            }
            mappings.push(InterfaceImplMapping {
                member: member.to_string(),
                member_span: SourceSpan::new(line.number, member_column, member.len()),
                function: function.to_string(),
                function_span: SourceSpan::new(line.number, function_column, function.len()),
            });
            *index += 1;
        }
    }

    if mappings.is_empty() {
        return Err(diag(
            header.number,
            "interface implementations require at least one function mapping",
        ));
    }
    if *index >= lines.len() || lines[*index].indent != 0 || lines[*index].text != "}" {
        return Err(diag(
            header.number,
            "interface implementation body must end with a top-level '}'",
        ));
    }
    *index += 1;

    Ok(InterfaceImpl {
        interface_name: interface_name.to_string(),
        interface_span: SourceSpan::new(header.number, 6 + interface_offset, interface_name.len()),
        target_name: target_name.to_string(),
        target_span: SourceSpan::new(header.number, 6 + target_offset, target_name.len()),
        keyword_span: SourceSpan::new(header.number, 1, 4),
        mappings,
        line: header.number,
        span: definition_span,
    })
}

fn parse_type_alias(line: &Line) -> Result<TypeAlias, Diagnostic> {
    let (text, public, visibility_offset) = split_visibility(&line.text);
    let Some(rest) = text.strip_prefix("type ") else {
        return Err(diag(line.number, "expected type alias declaration"));
    };
    let Some(eq_offset) = rest.find('=') else {
        return Err(diag(
            line.number,
            "type aliases use 'type Name = Target' syntax",
        ));
    };
    let raw_name = &rest[..eq_offset];
    let raw_target = &rest[eq_offset + 1..];
    let (name, name_column) = trim_with_column(raw_name, 6 + visibility_offset);
    validate_identifier(name, line.number)?;
    let (target_text, target_column) =
        trim_with_column(raw_target, 6 + visibility_offset + eq_offset + 1);
    let target = parse_type(target_text, line.number)?;
    if target == Type::Void {
        return Err(diag(line.number, "type aliases cannot target void"));
    }
    Ok(TypeAlias {
        public,
        name: name.to_string(),
        name_span: SourceSpan::new(line.number, name_column, name.len()),
        target,
        target_span: SourceSpan::new(line.number, target_column, target_text.len()),
        line: line.number,
        span: line.span(),
    })
}

fn parse_constant(line: &Line) -> Result<ConstantDef, Diagnostic> {
    let (text, public, visibility_offset) = split_visibility(&line.text);
    let Some(rest) = text.strip_prefix("const ") else {
        return Err(diag(line.number, "expected constant declaration"));
    };
    let Some(eq_offset) = rest.find('=') else {
        return Err(diag(
            line.number,
            "constants use 'const name: type = expression' syntax",
        ));
    };
    let binding_src = &rest[..eq_offset];
    let raw_value = &rest[eq_offset + 1..];
    let binding = parse_binding(binding_src.trim(), line.number, 7 + visibility_offset)?;
    let raw_value_column = 7 + visibility_offset + eq_offset + 1;
    let (value_src, value_column) = trim_with_column(raw_value, raw_value_column);
    let value = parse_expression_at(value_src, line.number, value_column)?;
    Ok(ConstantDef {
        public,
        name: binding.name,
        name_span: binding.name_span,
        ty: binding.ty,
        type_span: binding.type_span,
        value,
        line: line.number,
        span: line.span(),
    })
}

fn parse_enum_declaration(lines: &[Line], index: &mut usize) -> Result<EnumDef, Diagnostic> {
    let header = &lines[*index];
    let (header_text, public, visibility_offset) = split_visibility(&header.text);
    let Some(rest) = header_text.strip_prefix("enum ") else {
        return Err(diag(header.number, "expected enum declaration"));
    };
    let Some(raw_name) = rest.strip_suffix('{') else {
        return Err(diag(
            header.number,
            "enum declarations must open their body with '{'",
        ));
    };
    let name = raw_name.trim();
    validate_identifier(name, header.number)?;
    let leading = raw_name.len() - raw_name.trim_start().len();
    let name_span = SourceSpan::new(header.number, visibility_offset + 6 + leading, name.len());
    let definition_span = header.span();
    *index += 1;

    let mut variants = Vec::new();
    if *index < lines.len() && lines[*index].indent > 0 {
        let variant_indent = lines[*index].indent;
        while *index < lines.len() {
            let line = &lines[*index];
            if line.indent < variant_indent {
                break;
            }
            if line.indent > variant_indent {
                return Err(diag(line.number, "unexpected indentation in enum body"));
            }
            if line.text == "}" {
                break;
            }

            let text = line.text.trim();
            let (variant_name, payload_source, payload_column) = if let Some(open) = text.find('(')
            {
                let Some(inner) = text.strip_suffix(')') else {
                    return Err(diag(line.number, "enum variant payload must end with ')'"));
                };
                let variant_name = text[..open].trim();
                let payload_source = &inner[open + 1..];
                let payload_column = line.indent + 1 + open + 1;
                (variant_name, Some(payload_source), payload_column)
            } else {
                (text, None, line.indent + 1 + text.len())
            };
            validate_identifier(variant_name, line.number)?;
            if variants
                .iter()
                .any(|variant: &EnumVariant| variant.name == variant_name)
            {
                return Err(diag(
                    line.number,
                    &format!("duplicate enum variant '{variant_name}'"),
                ));
            }
            let variant_offset = line.text.find(variant_name).unwrap_or(0);
            let mut payloads = Vec::new();
            if let Some(payload_source) = payload_source
                && !payload_source.trim().is_empty()
            {
                for (raw_payload, offset) in split_top_level_commas_with_offsets(payload_source) {
                    let (type_text, type_column) =
                        trim_with_column(raw_payload, payload_column + offset);
                    let ty = parse_type(type_text, line.number)?;
                    if ty == Type::Void {
                        return Err(diag(line.number, "enum payloads cannot have type void"));
                    }
                    payloads.push(EnumPayload {
                        ty,
                        type_span: SourceSpan::new(line.number, type_column, type_text.len()),
                    });
                }
            }
            variants.push(EnumVariant {
                name: variant_name.to_string(),
                name_span: SourceSpan::new(
                    line.number,
                    line.indent + 1 + variant_offset,
                    variant_name.len(),
                ),
                payloads,
            });
            *index += 1;
        }
    }

    if variants.is_empty() {
        return Err(diag(
            header.number,
            "enum declarations require at least one variant",
        ));
    }
    if *index >= lines.len() || lines[*index].indent != 0 || lines[*index].text != "}" {
        return Err(diag(
            header.number,
            "enum body must end with a top-level '}'",
        ));
    }
    *index += 1;

    Ok(EnumDef {
        public,
        name: name.to_string(),
        name_span,
        keyword_span: SourceSpan::new(header.number, 1 + visibility_offset, 4),
        variants,
        line: header.number,
        span: definition_span,
    })
}

fn parse_view_declaration(lines: &[Line], index: &mut usize) -> Result<ViewDef, Diagnostic> {
    let header = &lines[*index];
    let (header_text, public, visibility_offset) = split_visibility(&header.text);
    let Some(rest) = header_text.strip_prefix("view ") else {
        return Err(diag(header.number, "expected view declaration"));
    };
    let Some(raw_header) = rest.strip_suffix('{') else {
        return Err(diag(
            header.number,
            "view declarations must open their body with '{'",
        ));
    };
    let header_body = raw_header.trim();
    let (name, name_span, params) = if header_body.contains('(') {
        let synthetic = format!("fn {header_body} -> void {{");
        let mut parsed = parse_function_header(&synthetic, header.number)?;
        shift_function_header(&mut parsed, visibility_offset + 2);
        (parsed.name, parsed.name_span, parsed.params)
    } else {
        validate_identifier(header_body, header.number)?;
        let leading = raw_header.len() - raw_header.trim_start().len();
        (
            header_body.to_string(),
            SourceSpan::new(
                header.number,
                visibility_offset + 6 + leading,
                header_body.len(),
            ),
            Vec::new(),
        )
    };
    let definition_span = header.span();
    *index += 1;

    let mut states = Vec::new();
    let mut derived = Vec::new();
    let mut grid = GridLayout::default();
    let mut elements = Vec::new();
    while *index < lines.len() && lines[*index].indent > 0 {
        let line = &lines[*index];
        if line.indent != 4 {
            return Err(diag(
                line.number,
                "view grid directives and elements use exactly four spaces of indentation",
            ));
        }

        if let Some(raw_state) = line.text.strip_prefix("state ") {
            let Some(eq_offset) = raw_state.find('=') else {
                return Err(diag(
                    line.number,
                    "view state uses 'state name: type = expression' syntax",
                ));
            };
            let binding_source = raw_state[..eq_offset].trim();
            let binding = parse_binding(binding_source, line.number, line.indent + 7)?;
            if states
                .iter()
                .any(|state: &ViewState| state.name == binding.name)
                || derived
                    .iter()
                    .any(|value: &ViewDerived| value.name == binding.name)
                || params.iter().any(|param| param.name == binding.name)
            {
                return Err(diag(
                    line.number,
                    &format!(
                        "duplicate view state, derived value, or parameter '{}'",
                        binding.name
                    ),
                ));
            }
            let raw_value = &raw_state[eq_offset + 1..];
            let value_column = line.indent + 7 + eq_offset + 1;
            let (value_source, value_column) = trim_with_column(raw_value, value_column);
            if value_source.is_empty() {
                return Err(diag(
                    line.number,
                    "view state initial value cannot be empty",
                ));
            }
            states.push(ViewState {
                name: binding.name,
                name_span: binding.name_span,
                ty: binding.ty,
                type_span: binding.type_span,
                initial: parse_expression_at(value_source, line.number, value_column)?,
                line: line.number,
                span: line.span(),
            });
            *index += 1;
            continue;
        }

        if let Some(raw_derived) = line.text.strip_prefix("derived ") {
            let Some(eq_offset) = raw_derived.find('=') else {
                return Err(diag(
                    line.number,
                    "derived view values use 'derived name: type = expression' syntax",
                ));
            };
            let binding_source = raw_derived[..eq_offset].trim();
            let binding = parse_binding(binding_source, line.number, line.indent + 9)?;
            if states
                .iter()
                .any(|state: &ViewState| state.name == binding.name)
                || derived
                    .iter()
                    .any(|value: &ViewDerived| value.name == binding.name)
                || params.iter().any(|param| param.name == binding.name)
            {
                return Err(diag(
                    line.number,
                    &format!(
                        "duplicate view state, derived value, or parameter '{}'",
                        binding.name
                    ),
                ));
            }
            let raw_value = &raw_derived[eq_offset + 1..];
            let value_column = line.indent + 9 + eq_offset + 1;
            let (value_source, value_column) = trim_with_column(raw_value, value_column);
            if value_source.is_empty() {
                return Err(diag(line.number, "derived view value cannot be empty"));
            }
            derived.push(ViewDerived {
                name: binding.name,
                name_span: binding.name_span,
                ty: binding.ty,
                type_span: binding.type_span,
                value: parse_expression_at(value_source, line.number, value_column)?,
                line: line.number,
                span: line.span(),
            });
            *index += 1;
            continue;
        }

        if let Some(value) = line.text.strip_prefix("flow:") {
            if grid.flow.is_some() {
                return Err(diag(line.number, "flow may only be declared once"));
            }
            grid.flow = Some(match value.trim() {
                "horizontal" => FlowDirection::Horizontal,
                "vertical" => FlowDirection::Vertical,
                _ => {
                    return Err(diag(line.number, "flow must be 'horizontal' or 'vertical'"));
                }
            });
            grid.flow_line = Some(line.number);
            *index += 1;
            continue;
        }

        if let Some(value) = line.text.strip_prefix("grid columns:") {
            if !grid.columns.is_empty() {
                return Err(diag(line.number, "grid columns may only be declared once"));
            }
            grid.columns = parse_grid_tracks(value.trim(), line.number)?;
            *index += 1;
            continue;
        }
        if let Some(value) = line.text.strip_prefix("grid rows:") {
            if !grid.rows.is_empty() {
                return Err(diag(line.number, "grid rows may only be declared once"));
            }
            grid.rows = parse_grid_tracks(value.trim(), line.number)?;
            *index += 1;
            continue;
        }
        if let Some((value, label)) = line
            .text
            .strip_prefix("grid gap:")
            .map(|value| (value, "grid gap"))
            .or_else(|| {
                line.text
                    .strip_prefix("flow gap:")
                    .map(|value| (value, "flow gap"))
            })
        {
            if grid.gap.is_some() {
                return Err(diag(
                    line.number,
                    &format!("{label} may only be declared once"),
                ));
            }
            grid.gap = Some(parse_positive_or_zero_u32(
                value.trim(),
                line.number,
                label,
            )?);
            *index += 1;
            continue;
        }

        if let Some((value, label)) = line
            .text
            .strip_prefix("grid padding:")
            .map(|value| (value, "grid padding"))
            .or_else(|| {
                line.text
                    .strip_prefix("flow padding:")
                    .map(|value| (value, "flow padding"))
            })
        {
            if grid.padding.is_some() {
                return Err(diag(
                    line.number,
                    &format!("{label} may only be declared once"),
                ));
            }
            grid.padding = Some(parse_positive_or_zero_u32(
                value.trim(),
                line.number,
                label,
            )?);
            grid.padding_line = Some(line.number);
            *index += 1;
            continue;
        }

        if let Some((value, label)) = line
            .text
            .strip_prefix("grid scroll:")
            .map(|value| (value, "grid scroll"))
            .or_else(|| {
                line.text
                    .strip_prefix("flow scroll:")
                    .map(|value| (value, "flow scroll"))
            })
        {
            if grid.scroll.is_some() {
                return Err(diag(
                    line.number,
                    &format!("{label} may only be declared once"),
                ));
            }
            grid.scroll = Some(match value.trim() {
                "true" => true,
                "false" => false,
                _ => {
                    return Err(diag(
                        line.number,
                        &format!("{label} must be the boolean literal true or false"),
                    ));
                }
            });
            grid.scroll_line = Some(line.number);
            *index += 1;
            continue;
        }

        if let Some(value) = line.text.strip_prefix("grid overlay:") {
            if grid.overlay.is_some() {
                return Err(diag(line.number, "grid overlay may only be declared once"));
            }
            grid.overlay = Some(match value.trim() {
                "true" => true,
                "false" => false,
                _ => {
                    return Err(diag(
                        line.number,
                        "grid overlay must be the boolean literal true or false",
                    ));
                }
            });
            grid.overlay_line = Some(line.number);
            *index += 1;
            continue;
        }

        let mut element = parse_view_element(line, grid.flow, elements.len())?;
        if elements
            .iter()
            .any(|existing: &ViewElement| existing.name == element.name)
        {
            return Err(diag(
                line.number,
                &format!("duplicate view element name '{}'", element.name),
            ));
        }
        *index += 1;
        while *index < lines.len() && lines[*index].indent > 4 {
            let property_line = &lines[*index];
            if property_line.indent != 8 {
                return Err(diag(
                    property_line.number,
                    "view element properties use exactly eight spaces of indentation",
                ));
            }
            let Some(colon) = property_line.text.find(':') else {
                return Err(diag(
                    property_line.number,
                    "view element properties use 'name: expression' syntax",
                ));
            };
            let property_name = property_line.text[..colon].trim();
            validate_identifier(property_name, property_line.number)?;
            if element
                .properties
                .iter()
                .any(|property| property.name == property_name)
            {
                return Err(diag(
                    property_line.number,
                    &format!("duplicate view property '{property_name}'"),
                ));
            }
            let raw_value = &property_line.text[colon + 1..];
            let (value_source, value_column) =
                trim_with_column(raw_value, property_line.indent + colon + 2);
            if value_source.is_empty() {
                return Err(diag(
                    property_line.number,
                    "view property value cannot be empty",
                ));
            }
            let (transition, expression_source, expression_column) = if matches!(
                property_name,
                "onPress"
                    | "onChange"
                    | "onSelect"
                    | "onTap"
                    | "onDoubleTap"
                    | "onLongPress"
                    | "onHover"
                    | "onLeave"
                    | "onFocus"
                    | "onBlur"
                    | "on_press"
                    | "on_change"
                    | "on_select"
                    | "on_double_tap"
                    | "on_hover"
                    | "on_leave"
                    | "on_focus"
                    | "on_blur"
            ) {
                if let Some((raw_state, _)) = value_source.split_once("=>") {
                    let state = raw_state.trim();
                    validate_identifier(state, property_line.number)?;
                    let state_offset = value_source.find(state).unwrap_or(0);
                    let expression_offset = value_source.find("=>").unwrap_or(0) + 2;
                    let (expression_source, expression_column) = trim_with_column(
                        &value_source[expression_offset..],
                        value_column + expression_offset,
                    );
                    if expression_source.is_empty() {
                        return Err(diag(
                            property_line.number,
                            "view state transition requires an expression after '=>'",
                        ));
                    }
                    (
                        Some(ViewStateTransition {
                            state: state.to_string(),
                            state_span: SourceSpan::new(
                                property_line.number,
                                value_column + state_offset,
                                state.len(),
                            ),
                        }),
                        expression_source,
                        expression_column,
                    )
                } else {
                    (None, value_source, value_column)
                }
            } else {
                (None, value_source, value_column)
            };
            let value =
                parse_expression_at(expression_source, property_line.number, expression_column)?;
            let name_offset = property_line.text.find(property_name).unwrap_or(0);
            element.properties.push(ViewProperty {
                name: property_name.to_string(),
                name_span: SourceSpan::new(
                    property_line.number,
                    property_line.indent + 1 + name_offset,
                    property_name.len(),
                ),
                value,
                transition,
                line: property_line.number,
                span: property_line.span(),
            });
            *index += 1;
        }
        elements.push(element);
    }

    if let Some(flow) = grid.flow {
        if !grid.columns.is_empty() || !grid.rows.is_empty() {
            return Err(diag(
                header.number,
                "flow layout cannot also declare explicit grid columns or rows",
            ));
        }
        if grid.overlay.is_some() {
            return Err(diag(
                header.number,
                "flow layout does not support grid overlay; use explicit grid placement for overlays",
            ));
        }
        let count = elements.len().max(1);
        match flow {
            FlowDirection::Horizontal => {
                grid.columns = vec![GridTrack::Auto; count];
                grid.rows = vec![GridTrack::Auto];
            }
            FlowDirection::Vertical => {
                grid.columns = vec![GridTrack::Auto];
                grid.rows = vec![GridTrack::Auto; count];
            }
        }
    } else if grid.columns.is_empty() || grid.rows.is_empty() {
        return Err(diag(
            header.number,
            "views require either 'flow: horizontal|vertical' or both 'grid columns:' and 'grid rows:' declarations",
        ));
    }
    if *index >= lines.len() || lines[*index].indent != 0 || lines[*index].text != "}" {
        return Err(diag(
            header.number,
            "view body must end with a top-level '}'",
        ));
    }
    *index += 1;

    Ok(ViewDef {
        public,
        name,
        name_span,
        keyword_span: SourceSpan::new(header.number, 1 + visibility_offset, 4),
        params,
        states,
        derived,
        grid,
        elements,
        line: header.number,
        span: definition_span,
    })
}

fn parse_grid_tracks(input: &str, line: usize) -> Result<Vec<GridTrack>, Diagnostic> {
    if input.is_empty() {
        return Err(diag(line, "grid track list cannot be empty"));
    }
    input
        .split_whitespace()
        .map(|track| {
            if track == "auto" {
                return Ok(GridTrack::Auto);
            }
            if let Some(value) = track.strip_suffix("fr") {
                let fraction = parse_positive_u32(value, line, "fractional grid track")?;
                return Ok(GridTrack::Fraction(fraction));
            }
            Ok(GridTrack::Units(parse_positive_u32(
                track,
                line,
                "fixed grid track",
            )?))
        })
        .collect()
}

fn parse_view_element(
    line: &Line,
    flow: Option<FlowDirection>,
    element_index: usize,
) -> Result<ViewElement, Diagnostic> {
    let tokens = line.text.split_whitespace().collect::<Vec<_>>();
    if let Some(flow) = flow {
        if tokens.len() != 2 {
            return Err(diag(
                line.number,
                "flow elements use 'Type name'; placement and spans are automatic",
            ));
        }
        let kind = tokens[0];
        let name = tokens[1];
        validate_identifier(kind, line.number)?;
        validate_identifier(name, line.number)?;
        let position = u32::try_from(element_index + 1)
            .map_err(|_| diag(line.number, "flow contains too many elements"))?;
        let (row, column) = match flow {
            FlowDirection::Horizontal => (1, position),
            FlowDirection::Vertical => (position, 1),
        };
        let kind_offset = line.text.find(kind).unwrap_or(0);
        let name_offset = line.text.find(name).unwrap_or(kind.len());
        return Ok(ViewElement {
            kind: kind.to_string(),
            kind_span: SourceSpan::new(line.number, line.indent + 1 + kind_offset, kind.len()),
            name: name.to_string(),
            name_span: SourceSpan::new(line.number, line.indent + 1 + name_offset, name.len()),
            row,
            column,
            row_span: 1,
            column_span: 1,
            properties: Vec::new(),
            line: line.number,
            span: line.span(),
        });
    }
    if tokens.len() < 4 || tokens[2] != "at" {
        return Err(diag(
            line.number,
            "grid elements use 'Type name at row,column' with optional 'span rows N'/'span columns N'",
        ));
    }
    let kind = tokens[0];
    let name = tokens[1];
    validate_identifier(kind, line.number)?;
    validate_identifier(name, line.number)?;
    let Some((row, column)) = tokens[3].split_once(',') else {
        return Err(diag(
            line.number,
            "view element placement uses 'row,column'",
        ));
    };
    let row = parse_positive_u32(row, line.number, "grid row")?;
    let column = parse_positive_u32(column, line.number, "grid column")?;
    let mut row_span = 1;
    let mut column_span = 1;
    let mut cursor = 4;
    while cursor < tokens.len() {
        if cursor + 2 >= tokens.len() || tokens[cursor] != "span" {
            return Err(diag(line.number, "invalid view element span syntax"));
        }
        let amount = parse_positive_u32(tokens[cursor + 2], line.number, "grid span")?;
        match tokens[cursor + 1] {
            "rows" if row_span == 1 => row_span = amount,
            "columns" if column_span == 1 => column_span = amount,
            "rows" | "columns" => {
                return Err(diag(line.number, "each span axis may be declared once"));
            }
            _ => return Err(diag(line.number, "span axis must be 'rows' or 'columns'")),
        }
        cursor += 3;
    }
    let kind_offset = line.text.find(kind).unwrap_or(0);
    let name_offset = line.text.find(name).unwrap_or(kind.len());
    Ok(ViewElement {
        kind: kind.to_string(),
        kind_span: SourceSpan::new(line.number, line.indent + 1 + kind_offset, kind.len()),
        name: name.to_string(),
        name_span: SourceSpan::new(line.number, line.indent + 1 + name_offset, name.len()),
        row,
        column,
        row_span,
        column_span,
        properties: Vec::new(),
        line: line.number,
        span: line.span(),
    })
}

fn parse_positive_u32(input: &str, line: usize, label: &str) -> Result<u32, Diagnostic> {
    let value = input
        .parse::<u32>()
        .map_err(|_| diag(line, &format!("{label} must be a positive integer")))?;
    if value == 0 {
        return Err(diag(line, &format!("{label} must be greater than zero")));
    }
    Ok(value)
}

fn parse_positive_or_zero_u32(input: &str, line: usize, label: &str) -> Result<u32, Diagnostic> {
    input
        .parse::<u32>()
        .map_err(|_| diag(line, &format!("{label} must be a non-negative integer")))
}

fn parse_struct_declaration(lines: &[Line], index: &mut usize) -> Result<StructDef, Diagnostic> {
    let header = &lines[*index];
    let (header_text, public, visibility_offset) = split_visibility(&header.text);
    let Some(rest) = header_text.strip_prefix("struct ") else {
        return Err(diag(header.number, "expected struct declaration"));
    };
    let Some(raw_name) = rest.strip_suffix('{') else {
        return Err(diag(
            header.number,
            "struct declarations must open their body with '{'",
        ));
    };
    let name = raw_name.trim();
    validate_identifier(name, header.number)?;
    let leading = raw_name.len() - raw_name.trim_start().len();
    let name_span = SourceSpan::new(header.number, visibility_offset + 8 + leading, name.len());
    let definition_span = header.span();
    *index += 1;

    let mut fields = Vec::new();
    if *index < lines.len() && lines[*index].indent > 0 {
        let field_indent = lines[*index].indent;
        while *index < lines.len() {
            let line = &lines[*index];
            if line.indent < field_indent {
                break;
            }
            if line.indent > field_indent {
                return Err(diag(line.number, "unexpected indentation in struct body"));
            }
            let Some(colon_offset) = line.text.find(':') else {
                return Err(diag(line.number, "struct fields use 'name: type' syntax"));
            };
            let raw_field_name = &line.text[..colon_offset];
            let raw_type = &line.text[colon_offset + 1..];
            let (field_name, name_column) = trim_with_column(raw_field_name, line.indent + 1);
            validate_identifier(field_name, line.number)?;
            let (type_text, type_column) =
                trim_with_column(raw_type, line.indent + 1 + colon_offset + 1);
            let ty = parse_type(type_text, line.number)?;
            if ty == Type::Void {
                return Err(diag(line.number, "struct fields cannot have type void"));
            }
            if fields
                .iter()
                .any(|field: &StructField| field.name == field_name)
            {
                return Err(diag(
                    line.number,
                    &format!("duplicate struct field '{field_name}'"),
                ));
            }
            fields.push(StructField {
                name: field_name.to_string(),
                name_span: SourceSpan::new(line.number, name_column, field_name.len()),
                ty,
                type_span: SourceSpan::new(line.number, type_column, type_text.len()),
            });
            *index += 1;
        }
    }

    if *index >= lines.len() || lines[*index].indent != 0 || lines[*index].text != "}" {
        return Err(diag(
            header.number,
            "struct body must end with a top-level '}'",
        ));
    }
    *index += 1;

    Ok(StructDef {
        public,
        name: name.to_string(),
        name_span,
        keyword_span: SourceSpan::new(header.number, 1 + visibility_offset, 6),
        fields,
        line: header.number,
        span: definition_span,
    })
}

fn comment_marker(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut index = 0usize;
    let mut in_string = false;
    let mut raw_string = false;
    let mut multiline_string = false;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if multiline_string {
                if bytes[index..].starts_with(b"\"\"\"") {
                    in_string = false;
                    multiline_string = false;
                    index += 3;
                    continue;
                }
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                }
            } else if raw_string {
                if byte == b'"' {
                    in_string = false;
                    raw_string = false;
                }
            } else if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }

        if bytes[index..].starts_with(b"\"\"\"") {
            in_string = true;
            multiline_string = true;
            index += 3;
            continue;
        }
        if byte == b'r' && bytes.get(index + 1) == Some(&b'"') {
            in_string = true;
            raw_string = true;
            index += 2;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            index += 1;
            continue;
        }
        if byte == b'#' {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn parse_single_expression_function(line: &Line) -> Result<Option<Function>, Diagnostic> {
    let (text, public, visibility_offset) = split_visibility(&line.text);
    if !text.starts_with("fn ") || !text.ends_with('}') {
        return Ok(None);
    }
    let Some(open_offset) = text.find('{') else {
        return Ok(None);
    };
    if open_offset + 1 >= text.len() {
        return Ok(None);
    }
    let header_source = text[..=open_offset].trim_end();
    let mut header = parse_function_header(header_source, line.number)?;
    shift_function_header(&mut header, visibility_offset);
    if header.returns.is_empty() {
        return Err(diag(
            line.number,
            "single-expression functions require a non-void return type",
        ));
    }
    let raw_expression = &text[open_offset + 1..text.len() - 1];
    let (expression_source, expression_column) = trim_with_column(
        raw_expression,
        line.indent + visibility_offset + open_offset + 2,
    );
    if expression_source.is_empty() {
        return Err(diag(
            line.number,
            "single-expression functions require an expression between '{' and '}'",
        ));
    }
    let expression = parse_expression_at(expression_source, line.number, expression_column)?;
    let expression_span = expression.span;
    Ok(Some(Function {
        public,
        name: header.name,
        name_span: header.name_span,
        keyword_span: SourceSpan::new(line.number, line.indent + 1 + visibility_offset, 2),
        params: header.params,
        returns: header.returns,
        return_span: header.return_span,
        return_type_spans: header.return_type_spans,
        body: vec![Stmt {
            line: line.number,
            span: expression_span,
            keyword_span: expression_span,
            kind: StmtKind::Return(vec![expression]),
        }],
        expression_body: true,
        line: line.number,
        span: line.span(),
    }))
}

struct FunctionHeader {
    name: String,
    name_span: SourceSpan,
    params: Vec<Param>,
    returns: Vec<Type>,
    return_span: SourceSpan,
    return_type_spans: Vec<SourceSpan>,
}

fn parse_function_header(input: &str, line: usize) -> Result<FunctionHeader, Diagnostic> {
    let Some(rest) = input.strip_prefix("fn ") else {
        return Err(diag(
            line,
            "expected function declaration starting with 'fn'",
        ));
    };
    let Some(open) = rest.find('(') else {
        return Err(diag(line, "expected '(' after function name"));
    };
    let raw_name = &rest[..open];
    let name = raw_name.trim();
    validate_identifier(name, line)?;
    let name_span = SourceSpan::new(line, 4 + raw_name.find(name).unwrap_or(0), name.len());

    let Some(close) = find_matching_paren(rest, open) else {
        return Err(diag(line, "expected ')' after function parameters"));
    };
    let params_src = &rest[open + 1..close];
    let suffix = rest[close + 1..].trim();
    let Some(ret_src) = suffix.strip_prefix("->") else {
        return Err(diag(
            line,
            "functions require an explicit return type with '->'",
        ));
    };
    let Some(ret_src) = ret_src.trim().strip_suffix('{') else {
        let insertion = SourceSpan::new(line, input.len() + 1, 0);
        return Err(Diagnostic::new(
            DiagnosticStage::Parse,
            insertion,
            "functions must open their body with '{'",
        )
        .with_fix(insertion, " {", "insert the function body opener"));
    };
    let return_text = ret_src.trim();
    let returns = parse_return_types(return_text, line)?;
    let arrow_offset = input
        .find("->")
        .expect("function return arrow was already parsed");
    let after_arrow = &input[arrow_offset + 2..];
    let return_leading = after_arrow.len() - after_arrow.trim_start().len();
    let return_column = arrow_offset + 2 + return_leading + 1;
    let return_span = SourceSpan::new(line, return_column, return_text.len());
    let return_type_spans = return_type_spans(return_text, return_span);

    let mut params = Vec::new();
    if !params_src.trim().is_empty() {
        let params_base_column = 4 + open + 1;
        let raw_params = split_top_level_commas_with_offsets(params_src);
        let mut named_only = false;
        let mut saw_named_marker = false;
        let mut saw_positional_default = false;
        for (raw_param, param_offset) in raw_params {
            let (param_src, param_column) =
                trim_with_column(raw_param, params_base_column + param_offset);
            if param_src == "*" {
                if saw_named_marker {
                    return Err(diag(
                        line,
                        "function parameters may contain only one '*' marker",
                    ));
                }
                saw_named_marker = true;
                named_only = true;
                continue;
            }

            let (binding_src, default_src) = split_parameter_default(param_src);
            let Some(colon_offset) = binding_src.find(':') else {
                return Err(diag(line, "parameters use 'name: type' syntax"));
            };
            let raw_name = &binding_src[..colon_offset];
            let raw_ty = &binding_src[colon_offset + 1..];
            let (param_name, name_column) = trim_with_column(raw_name, param_column);
            validate_identifier(param_name, line)?;
            let (type_text, type_column) =
                trim_with_column(raw_ty, param_column + colon_offset + 1);
            let ty = parse_type(type_text, line)?;
            if ty == Type::Void {
                return Err(diag(line, "parameters cannot have type void"));
            }
            if params.iter().any(|param: &Param| param.name == param_name) {
                return Err(diag(line, &format!("duplicate parameter '{param_name}'")));
            }

            let default = if let Some((raw_default, default_offset)) = default_src {
                let (default_src, default_column) =
                    trim_with_column(raw_default, param_column + default_offset);
                if default_src.is_empty() {
                    return Err(diag(line, "parameter default requires an expression"));
                }
                if !named_only {
                    saw_positional_default = true;
                }
                Some(parse_expression_at(default_src, line, default_column)?)
            } else {
                if !named_only && saw_positional_default {
                    return Err(diag(
                        line,
                        "required positional parameters cannot follow a positional parameter with a default",
                    ));
                }
                None
            };

            params.push(Param {
                name: param_name.to_string(),
                name_span: SourceSpan::new(line, name_column, param_name.len()),
                ty,
                type_span: SourceSpan::new(line, type_column, type_text.len()),
                named_only,
                default,
            });
        }
        if saw_named_marker && !params.iter().any(|param| param.named_only) {
            return Err(diag(
                line,
                "'*' must be followed by at least one named parameter",
            ));
        }
    }

    Ok(FunctionHeader {
        name: name.to_string(),
        name_span,
        params,
        returns,
        return_span,
        return_type_spans,
    })
}

fn find_matching_paren(input: &str, open: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().copied().enumerate().skip(open) {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' && in_string {
            escaped = true;
            continue;
        }
        if byte == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_parameter_default(input: &str) -> (&str, Option<(&str, usize)>) {
    let bytes = input.as_bytes();
    let mut in_string = false;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if byte == b'\\' && in_string {
            escaped = true;
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = !in_string;
            index += 1;
            continue;
        }
        if !in_string {
            match byte {
                b'(' | b'{' => depth += 1,
                b')' | b'}' => depth = depth.saturating_sub(1),
                b'=' if depth == 0
                    && bytes.get(index.wrapping_sub(1)) != Some(&b'!')
                    && bytes.get(index.wrapping_sub(1)) != Some(&b'<')
                    && bytes.get(index.wrapping_sub(1)) != Some(&b'>')
                    && bytes.get(index.wrapping_sub(1)) != Some(&b'=')
                    && bytes.get(index + 1) != Some(&b'=') =>
                {
                    return (&input[..index], Some((&input[index + 1..], index + 1)));
                }
                _ => {}
            }
        }
        index += 1;
    }
    (input, None)
}

fn return_type_spans(return_text: &str, return_span: SourceSpan) -> Vec<SourceSpan> {
    let Some(inner) = return_text
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
    else {
        return vec![return_span];
    };

    split_top_level_commas_with_offsets(inner)
        .into_iter()
        .map(|(raw, offset)| {
            let (ty, column) = trim_with_column(raw, return_span.column + 1 + offset);
            SourceSpan::new(return_span.line, column, ty.len())
        })
        .collect()
}

fn parse_block(lines: &[Line], index: &mut usize, indent: usize) -> Result<Vec<Stmt>, Diagnostic> {
    let mut body = Vec::new();

    while *index < lines.len() {
        let line = &lines[*index];
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            return Err(diag(line.number, "unexpected indentation"));
        }
        if line.text == "}" {
            break;
        }

        let stmt = if let Some(stmt) = parse_match_expression_statement(lines, index, indent)? {
            stmt
        } else if line.text.starts_with("match ") && line.text.ends_with(':') {
            parse_match_statement(lines, index, indent)?
        } else if line.text.starts_with("if ") && line.text.ends_with(':') {
            parse_if_statement(lines, index, indent)?
        } else if line.text.starts_with("elif ") {
            return Err(diag(
                line.number,
                "elif must immediately follow an if or elif block",
            ));
        } else if line.text == "else" || line.text == "else:" || line.text.starts_with("else ") {
            return Err(diag(
                line.number,
                "else must immediately follow an if or elif block",
            ));
        } else if line.text.starts_with("while ") && line.text.ends_with(':') {
            let stmt_line = line.number;
            let raw_cond = &line.text[6..line.text.len() - 1];
            let (cond_src, cond_column) = trim_with_column(raw_cond, line.indent + 7);
            if cond_src.is_empty() {
                return Err(diag(stmt_line, "while requires a condition"));
            }
            let cond = parse_expression_at(cond_src, stmt_line, cond_column)?;
            *index += 1;
            let nested = parse_nested_block(lines, index, indent, stmt_line, "while")?;
            Stmt {
                line: stmt_line,
                span: line.span(),
                keyword_span: SourceSpan::new(stmt_line, line.indent + 1, 5),
                kind: StmtKind::While { cond, body: nested },
            }
        } else if line.text.starts_with("for ") && line.text.ends_with(':') {
            let stmt_line = line.number;
            let raw_inner = &line.text[4..line.text.len() - 1];
            let inner_leading = raw_inner.len() - raw_inner.trim_start().len();
            let inner = raw_inner.trim();
            let inner_column = line.indent + 1 + 4 + inner_leading;
            let Some(in_offset) = inner.find(" in ") else {
                return Err(diag(
                    stmt_line,
                    "for loops use 'for name in source:' or 'for index, name in source:'",
                ));
            };
            let raw_bindings = &inner[..in_offset];
            let raw_source = &inner[in_offset + 4..];
            let mut bindings = Vec::new();
            for (raw_binding, offset) in split_top_level_commas_with_offsets(raw_bindings) {
                let (name, column) = trim_with_column(raw_binding, inner_column + offset);
                validate_identifier(name, stmt_line)?;
                bindings.push((
                    name.to_string(),
                    SourceSpan::new(stmt_line, column, name.len()),
                ));
            }
            if bindings.is_empty() || bindings.len() > 2 {
                return Err(diag(
                    stmt_line,
                    "for loops bind one value or an index and value",
                ));
            }
            if bindings.len() == 2 && bindings[0].0 == bindings[1].0 {
                return Err(diag(
                    stmt_line,
                    &format!("duplicate for-loop binding '{}'", bindings[0].0),
                ));
            }
            let (source_src, source_column) =
                trim_with_column(raw_source, inner_column + in_offset + 4);
            if source_src.is_empty() {
                return Err(diag(stmt_line, "for loop requires a source expression"));
            }
            if let Some((start_raw, end_raw, range_split, inclusive)) =
                split_range_with_offset(source_src)
            {
                if bindings.len() != 1 {
                    return Err(diag(
                        stmt_line,
                        "range loops bind exactly one loop variable",
                    ));
                }
                let (start_src, start_column) = trim_with_column(start_raw, source_column);
                let range_width = if inclusive { 3 } else { 2 };
                let (end_src, end_column) =
                    trim_with_column(end_raw, source_column + range_split + range_width);
                let start = parse_expression_at(start_src, stmt_line, start_column)?;
                let end = parse_expression_at(end_src, stmt_line, end_column)?;
                *index += 1;
                let nested = parse_nested_block(lines, index, indent, stmt_line, "for")?;
                let (name, name_span) = bindings.remove(0);
                Stmt {
                    line: stmt_line,
                    span: line.span(),
                    keyword_span: SourceSpan::new(stmt_line, line.indent + 1, 3),
                    kind: StmtKind::ForRange {
                        name,
                        name_span,
                        start,
                        end,
                        inclusive,
                        body: nested,
                    },
                }
            } else {
                let iterable = parse_expression_at(source_src, stmt_line, source_column)?;
                *index += 1;
                let nested = parse_nested_block(lines, index, indent, stmt_line, "for")?;
                let (index_name, index_span, name, name_span) = match bindings.as_slice() {
                    [(name, name_span)] => (None, None, name.clone(), *name_span),
                    [(index_name, index_span), (name, name_span)] => (
                        Some(index_name.clone()),
                        Some(*index_span),
                        name.clone(),
                        *name_span,
                    ),
                    _ => unreachable!(),
                };
                Stmt {
                    line: stmt_line,
                    span: line.span(),
                    keyword_span: SourceSpan::new(stmt_line, line.indent + 1, 3),
                    kind: StmtKind::ForEach {
                        index_name,
                        index_span,
                        name,
                        name_span,
                        iterable,
                        body: nested,
                    },
                }
            }
        } else {
            let stmt = parse_simple_statement(&line.text, line.span())?;
            *index += 1;
            stmt
        };

        body.push(stmt);
    }

    Ok(body)
}

fn parse_match_expression_statement(
    lines: &[Line],
    index: &mut usize,
    indent: usize,
) -> Result<Option<Stmt>, Diagnostic> {
    let line = &lines[*index];
    if !line.text.ends_with(':') {
        return Ok(None);
    }

    if let Some(rest) = line.text.strip_prefix("let ")
        && let Some(split) = rest.find(" = match ")
    {
        let binding_src = &rest[..split];
        let binding = parse_binding(binding_src.trim(), line.number, line.indent + 5)?;
        let raw_value = &rest[split + 9..rest.len() - 1];
        let value_column = line.indent + 5 + split + 9;
        let expr = parse_match_expression(lines, index, indent, raw_value, value_column)?;
        return Ok(Some(Stmt {
            line: line.number,
            span: line.span(),
            keyword_span: SourceSpan::new(line.number, line.indent + 1, 3),
            kind: StmtKind::Let {
                name: binding.name,
                name_span: binding.name_span,
                ty: binding.ty,
                type_span: binding.type_span,
                expr,
            },
        }));
    }

    if let Some(raw_value) = line.text.strip_prefix("return match ") {
        let raw_value = &raw_value[..raw_value.len() - 1];
        let expr = parse_match_expression(lines, index, indent, raw_value, line.indent + 14)?;
        return Ok(Some(Stmt {
            line: line.number,
            span: line.span(),
            keyword_span: SourceSpan::new(line.number, line.indent + 1, 6),
            kind: StmtKind::Return(vec![expr]),
        }));
    }

    Ok(None)
}

fn parse_match_expression(
    lines: &[Line],
    index: &mut usize,
    indent: usize,
    raw_value: &str,
    value_column: usize,
) -> Result<Expr, Diagnostic> {
    let header = &lines[*index];
    let (value_src, value_column) = trim_with_column(raw_value, value_column);
    if value_src.is_empty() {
        return Err(diag(header.number, "match expression requires a value"));
    }
    let value = parse_expression_at(value_src, header.number, value_column)?;
    let start_column = value.span.column.saturating_sub(6);
    *index += 1;
    if *index >= lines.len() || lines[*index].indent <= indent {
        return Err(diag(
            header.number,
            "match expression requires indented arms",
        ));
    }
    let arm_indent = lines[*index].indent;
    let span = SourceSpan::new(
        header.number,
        start_column,
        header
            .text
            .len()
            .saturating_sub(start_column.saturating_sub(header.indent + 1)),
    );
    if is_list_match_arm_line(&lines[*index]) {
        let mut arms = Vec::new();
        while *index < lines.len() && lines[*index].indent == arm_indent {
            arms.push(parse_list_match_expr_arm(&lines[*index])?);
            *index += 1;
        }
        return Ok(Expr {
            line: header.number,
            span,
            kind: ExprKind::ListMatch {
                value: Box::new(value),
                arms,
            },
        });
    }

    let mut arms = Vec::new();
    while *index < lines.len() && lines[*index].indent == arm_indent {
        arms.push(parse_match_expr_arm(&lines[*index])?);
        *index += 1;
    }
    Ok(Expr {
        line: header.number,
        span,
        kind: ExprKind::Match {
            value: Box::new(value),
            arms,
        },
    })
}

fn parse_match_expr_arm(line: &Line) -> Result<MatchExprArm, Diagnostic> {
    let Some(colon) = find_top_level_colon(&line.text) else {
        return Err(diag(
            line.number,
            "match expression arms use 'Enum.Variant(patterns): expression'",
        ));
    };
    let pattern_text = line.text[..colon].trim();
    let value_text = line.text[colon + 1..].trim();
    if value_text.is_empty() {
        return Err(diag(line.number, "match expression arm requires a value"));
    }
    let header_line = Line {
        number: line.number,
        indent: line.indent,
        text: format!("{pattern_text}:"),
    };
    let arm = parse_match_arm_header(&header_line)?;
    let value_offset = line.text[colon + 1..].find(value_text).unwrap_or(0);
    let value = parse_expression_at(
        value_text,
        line.number,
        line.indent + colon + 2 + value_offset,
    )?;
    Ok(MatchExprArm {
        enum_name: arm.enum_name,
        enum_span: arm.enum_span,
        variant: arm.variant,
        variant_span: arm.variant_span,
        patterns: arm.patterns,
        guard: arm.guard,
        value,
        line: line.number,
        span: line.span(),
    })
}

fn parse_list_match_expr_arm(line: &Line) -> Result<ListMatchExprArm, Diagnostic> {
    let Some(colon) = find_top_level_colon(&line.text) else {
        return Err(diag(
            line.number,
            "list match expression arms use '[pattern]: expression' or '_: expression'",
        ));
    };
    let pattern_text = line.text[..colon].trim();
    let value_text = line.text[colon + 1..].trim();
    if value_text.is_empty() {
        return Err(diag(line.number, "match expression arm requires a value"));
    }
    let pattern_offset = line.text[..colon].find(pattern_text).unwrap_or(0);
    let pattern_column = line.indent + 1 + pattern_offset;
    let (pattern_text, guard) = parse_match_guard(pattern_text, line.number, pattern_column)?;
    let pattern = parse_list_match_pattern(pattern_text, line.number, pattern_column)?;
    let value_offset = line.text[colon + 1..].find(value_text).unwrap_or(0);
    let value = parse_expression_at(
        value_text,
        line.number,
        line.indent + colon + 2 + value_offset,
    )?;
    Ok(ListMatchExprArm {
        pattern,
        guard,
        value,
        line: line.number,
        span: line.span(),
    })
}

fn is_list_match_arm_line(line: &Line) -> bool {
    let Some(colon) = find_top_level_colon(&line.text) else {
        return line.text.trim_start().starts_with('[');
    };
    let pattern = line.text[..colon].trim();
    pattern.starts_with('[') || pattern == "_" || pattern.starts_with("_ if ")
}

fn parse_match_guard(
    input: &str,
    line: usize,
    column: usize,
) -> Result<(&str, Option<Expr>), Diagnostic> {
    let bytes = input.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if byte == b'\\' && in_string {
            escaped = true;
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = !in_string;
            index += 1;
            continue;
        }
        if !in_string {
            match byte {
                b'(' | b'{' | b'[' => depth += 1,
                b')' | b'}' | b']' => depth = depth.saturating_sub(1),
                b' ' if depth == 0 && input[index..].starts_with(" if ") => {
                    let pattern = input[..index].trim_end();
                    let raw_guard = &input[index + 4..];
                    let (guard_source, guard_column) =
                        trim_with_column(raw_guard, column + index + 4);
                    if guard_source.is_empty() {
                        return Err(diag(line, "match guard requires a boolean expression"));
                    }
                    let guard = parse_expression_at(guard_source, line, guard_column)?;
                    return Ok((pattern, Some(guard)));
                }
                _ => {}
            }
        }
        index += 1;
    }
    Ok((input.trim(), None))
}

fn find_top_level_colon(input: &str) -> Option<usize> {
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in input.bytes().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' && in_string {
            escaped = true;
            continue;
        }
        if byte == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match byte {
            b'(' => paren += 1,
            b')' => paren = paren.saturating_sub(1),
            b'{' => brace += 1,
            b'}' => brace = brace.saturating_sub(1),
            b'[' => bracket += 1,
            b']' => bracket = bracket.saturating_sub(1),
            b':' if paren == 0 && brace == 0 && bracket == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn parse_match_statement(
    lines: &[Line],
    index: &mut usize,
    indent: usize,
) -> Result<Stmt, Diagnostic> {
    let line = &lines[*index];
    let raw_value = &line.text[6..line.text.len() - 1];
    let (value_src, value_column) = trim_with_column(raw_value, line.indent + 1 + 6);
    if value_src.is_empty() {
        return Err(diag(line.number, "match requires a value"));
    }
    let value = parse_expression_at(value_src, line.number, value_column)?;
    let stmt_line = line.number;
    let stmt_span = line.span();
    *index += 1;

    if *index >= lines.len() || lines[*index].indent <= indent {
        return Err(diag(stmt_line, "match requires at least one indented arm"));
    }
    let arm_indent = lines[*index].indent;
    if is_list_match_arm_line(&lines[*index]) {
        let mut arms = Vec::new();
        while *index < lines.len() && lines[*index].indent == arm_indent {
            let arm_line = &lines[*index];
            let (pattern, guard) = parse_list_match_arm_header(arm_line)?;
            let arm_line_number = arm_line.number;
            let arm_span = arm_line.span();
            *index += 1;
            let body = parse_nested_block(lines, index, arm_indent, arm_line_number, "match arm")?;
            arms.push(ListMatchArm {
                pattern,
                guard,
                body,
                line: arm_line_number,
                span: arm_span,
            });
        }
        return Ok(Stmt {
            line: stmt_line,
            span: stmt_span,
            keyword_span: SourceSpan::new(stmt_line, line.indent + 1, 5),
            kind: StmtKind::ListMatch { value, arms },
        });
    }

    let mut arms = Vec::new();
    while *index < lines.len() && lines[*index].indent == arm_indent {
        let arm_line = &lines[*index];
        let mut arm = parse_match_arm_header(arm_line)?;
        *index += 1;
        arm.body = parse_nested_block(lines, index, arm_indent, arm.line, "match arm")?;
        arms.push(arm);
    }

    Ok(Stmt {
        line: stmt_line,
        span: stmt_span,
        keyword_span: SourceSpan::new(stmt_line, line.indent + 1, 5),
        kind: StmtKind::Match { value, arms },
    })
}

fn parse_list_match_arm_header(
    line: &Line,
) -> Result<(ListMatchPattern, Option<Expr>), Diagnostic> {
    let Some(pattern) = line.text.strip_suffix(':') else {
        return Err(diag(line.number, "match arms must end with ':'"));
    };
    let pattern = pattern.trim();
    let offset = line.text.find(pattern).unwrap_or(0);
    let column = line.indent + 1 + offset;
    let (pattern, guard) = parse_match_guard(pattern, line.number, column)?;
    Ok((
        parse_list_match_pattern(pattern, line.number, column)?,
        guard,
    ))
}

fn parse_relational_match_pattern(
    input: &str,
    line: usize,
    column: usize,
) -> Result<Option<MatchPattern>, Diagnostic> {
    for (token, op) in [("||", PatternLogicalOp::Or), ("&&", PatternLogicalOp::And)] {
        if let Some(index) = find_top_level_pattern_operator(input, token) {
            let (left_source, left_column) = trim_with_column(&input[..index], column);
            let (right_source, right_column) =
                trim_with_column(&input[index + token.len()..], column + index + token.len());
            let Some(left) = parse_relational_match_pattern(left_source, line, left_column)? else {
                return Err(diag(
                    line,
                    "logical match patterns require relational patterns on both sides",
                ));
            };
            let Some(right) = parse_relational_match_pattern(right_source, line, right_column)?
            else {
                return Err(diag(
                    line,
                    "logical match patterns require relational patterns on both sides",
                ));
            };
            return Ok(Some(MatchPattern::Logical {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span: SourceSpan::new(line, column, input.len()),
            }));
        }
    }

    let (token, op) = if input.starts_with(">=") {
        (">=", BinOp::Ge)
    } else if input.starts_with("<=") {
        ("<=", BinOp::Le)
    } else if input.starts_with("==") {
        ("==", BinOp::Eq)
    } else if input.starts_with("!=") {
        ("!=", BinOp::Ne)
    } else if input.starts_with('>') {
        (">", BinOp::Gt)
    } else if input.starts_with('<') {
        ("<", BinOp::Lt)
    } else {
        return Ok(None);
    };
    let (value_source, value_column) =
        trim_with_column(&input[token.len()..], column + token.len());
    if value_source.is_empty() {
        return Err(diag(
            line,
            "relational match pattern requires a comparison value",
        ));
    }
    Ok(Some(MatchPattern::Relational(RelationalPattern {
        op,
        value: parse_expression_at(value_source, line, value_column)?,
        span: SourceSpan::new(line, column, input.len()),
    })))
}

fn find_top_level_pattern_operator(input: &str, token: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut index = 0usize;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    while index + token.len() <= bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if byte == b'\\' && in_string {
            escaped = true;
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = !in_string;
            index += 1;
            continue;
        }
        if !in_string {
            match byte {
                b'(' | b'{' | b'[' => depth += 1,
                b')' | b'}' | b']' => depth = depth.saturating_sub(1),
                _ if depth == 0 && input[index..].starts_with(token) => return Some(index),
                _ => {}
            }
        }
        index += 1;
    }
    None
}

fn parse_list_match_pattern(
    input: &str,
    line: usize,
    column: usize,
) -> Result<ListMatchPattern, Diagnostic> {
    let trimmed = input.trim();
    let leading = input.len() - input.trim_start().len();
    let span = SourceSpan::new(line, column + leading, trimmed.len());
    if trimmed == "_" {
        return Ok(ListMatchPattern::Wildcard { span });
    }
    let Some(pattern) = parse_list_destructure_pattern(input, line, column, true)? else {
        return Err(diag(
            line,
            "list match arms use '[pattern]:' or '_:' syntax",
        ));
    };
    Ok(ListMatchPattern::List {
        bindings: pattern.bindings,
        rest: pattern.rest,
        span,
    })
}

fn parse_match_arm_header(line: &Line) -> Result<MatchArm, Diagnostic> {
    let Some(pattern) = line.text.strip_suffix(':') else {
        return Err(diag(line.number, "match arms must end with ':'"));
    };
    let pattern = pattern.trim();
    let pattern_offset = line.text.find(pattern).unwrap_or(0);
    let (pattern, guard) =
        parse_match_guard(pattern, line.number, line.indent + 1 + pattern_offset)?;
    let Some(dot_offset) = pattern.find('.') else {
        return Err(diag(
            line.number,
            "enum match arms use 'Enum.Variant(bindings):' syntax",
        ));
    };
    let enum_name = pattern[..dot_offset].trim();
    validate_identifier(enum_name, line.number)?;
    let variant_source = &pattern[dot_offset + 1..];
    let Some(open_offset) = variant_source.find('(') else {
        return Err(diag(
            line.number,
            "enum match variants require parentheses, including payloadless variants",
        ));
    };
    let Some(inner) = variant_source.strip_suffix(')') else {
        return Err(diag(line.number, "match arm pattern must end with ')'"));
    };
    let variant = variant_source[..open_offset].trim();
    validate_identifier(variant, line.number)?;
    let bindings_source = &inner[open_offset + 1..];
    let enum_column = line.indent + 1 + pattern.find(enum_name).unwrap_or(0);
    let variant_column =
        line.indent + 1 + dot_offset + 1 + variant_source[..open_offset].find(variant).unwrap_or(0);
    let binding_base_column = line.indent + 1 + dot_offset + 1 + open_offset + 1;
    let mut patterns = Vec::new();
    if !bindings_source.trim().is_empty() {
        for (raw_pattern, offset) in split_top_level_commas_with_offsets(bindings_source) {
            let (pattern, column) = trim_with_column(raw_pattern, binding_base_column + offset);
            if let Some((struct_name, struct_span, fields)) =
                parse_struct_destructure_pattern(pattern, line.number, column)?
            {
                patterns.push(MatchPattern::Struct(StructPattern {
                    struct_name,
                    struct_span,
                    fields,
                }));
                continue;
            }
            if let Some(relational) = parse_relational_match_pattern(pattern, line.number, column)?
            {
                patterns.push(relational);
                continue;
            }
            validate_identifier(pattern, line.number)?;
            if pattern != "_"
                && patterns.iter().any(|existing| {
                    matches!(existing, MatchPattern::Binding(binding) if binding.name == pattern)
                })
            {
                return Err(diag(
                    line.number,
                    &format!("duplicate match binding '{pattern}'"),
                ));
            }
            patterns.push(MatchPattern::Binding(PatternBinding {
                name: pattern.to_string(),
                span: SourceSpan::new(line.number, column, pattern.len()),
            }));
        }
    }

    Ok(MatchArm {
        enum_name: enum_name.to_string(),
        enum_span: SourceSpan::new(line.number, enum_column, enum_name.len()),
        variant: variant.to_string(),
        variant_span: SourceSpan::new(line.number, variant_column, variant.len()),
        patterns,
        guard,
        body: Vec::new(),
        line: line.number,
        span: line.span(),
    })
}

fn parse_if_statement(
    lines: &[Line],
    index: &mut usize,
    indent: usize,
) -> Result<Stmt, Diagnostic> {
    let line = &lines[*index];
    let cond_src = line.text[3..line.text.len() - 1].trim();
    if cond_src.is_empty() {
        return Err(diag(line.number, "if requires a condition"));
    }
    let cond = parse_expression_at(
        cond_src,
        line.number,
        expression_column(&line.text, cond_src, line.indent + 1),
    )?;
    let stmt_line = line.number;
    let stmt_span = line.span();
    *index += 1;
    let body = parse_nested_block(lines, index, indent, stmt_line, "if")?;
    let (else_body, else_keyword_span) = parse_if_tail(lines, index, indent)?;

    Ok(Stmt {
        line: stmt_line,
        span: stmt_span,
        keyword_span: SourceSpan::new(stmt_line, line.indent + 1, 2),
        kind: StmtKind::If {
            cond,
            body,
            else_body,
            else_keyword_span,
        },
    })
}

fn parse_if_tail(
    lines: &[Line],
    index: &mut usize,
    indent: usize,
) -> Result<(Vec<Stmt>, Option<SourceSpan>), Diagnostic> {
    if *index >= lines.len() || lines[*index].indent != indent {
        return Ok((Vec::new(), None));
    }

    let line = &lines[*index];
    if line.text.starts_with("elif ") && line.text.ends_with(':') {
        let cond_src = line.text[5..line.text.len() - 1].trim();
        if cond_src.is_empty() {
            return Err(diag(line.number, "elif requires a condition"));
        }
        let cond = parse_expression_at(
            cond_src,
            line.number,
            expression_column(&line.text, cond_src, line.indent + 1),
        )?;
        let stmt_line = line.number;
        let stmt_span = line.span();
        *index += 1;
        let body = parse_nested_block(lines, index, indent, stmt_line, "elif")?;
        let (else_body, else_keyword_span) = parse_if_tail(lines, index, indent)?;
        return Ok((
            vec![Stmt {
                line: stmt_line,
                span: stmt_span,
                keyword_span: SourceSpan::new(stmt_line, line.indent + 1, 4),
                kind: StmtKind::If {
                    cond,
                    body,
                    else_body,
                    else_keyword_span,
                },
            }],
            Some(SourceSpan::new(stmt_line, line.indent + 1, 4)),
        ));
    }

    if line.text == "else:" {
        let stmt_line = line.number;
        let keyword_span = SourceSpan::new(stmt_line, line.indent + 1, 4);
        *index += 1;
        let body = parse_nested_block(lines, index, indent, stmt_line, "else")?;
        return Ok((body, Some(keyword_span)));
    }

    Ok((Vec::new(), None))
}

fn parse_nested_block(
    lines: &[Line],
    index: &mut usize,
    parent_indent: usize,
    parent_line: usize,
    kind: &str,
) -> Result<Vec<Stmt>, Diagnostic> {
    if *index >= lines.len() || lines[*index].indent <= parent_indent {
        return Err(diag(
            parent_line,
            &format!("{kind} requires an indented body"),
        ));
    }
    let nested_indent = lines[*index].indent;
    parse_block(lines, index, nested_indent)
}

fn parse_simple_statement(input: &str, span: SourceSpan) -> Result<Stmt, Diagnostic> {
    let line = span.line;
    if let Some(rest) = input.strip_prefix("var ") {
        let Some(eq_offset) = rest.find('=') else {
            return Err(diag(line, "var bindings require '= expression'"));
        };
        let binding_src = &rest[..eq_offset];
        let raw_expr_src = &rest[eq_offset + 1..];
        let (expr_src, expr_column) =
            trim_with_column(raw_expr_src, span.column + 4 + eq_offset + 1);
        let expr = parse_expression_at(expr_src, line, expr_column)?;

        if let Some(bindings) =
            parse_multi_value_destructure_pattern(binding_src, line, span.column + 4)?
        {
            return Ok(Stmt {
                line,
                span,
                keyword_span: SourceSpan::new(line, span.column, 3),
                kind: StmtKind::LetMultiDestructure {
                    bindings,
                    expr,
                    else_return: false,
                    mutable: true,
                },
            });
        }
        if let Some(list_pattern) =
            parse_list_destructure_pattern(binding_src, line, span.column + 4, false)?
        {
            return Ok(Stmt {
                line,
                span,
                keyword_span: SourceSpan::new(line, span.column, 3),
                kind: StmtKind::LetListDestructure {
                    bindings: list_pattern.bindings,
                    rest: list_pattern.rest,
                    expr,
                    mutable: true,
                },
            });
        }
        if let Some((struct_name, struct_span, fields)) =
            parse_struct_destructure_pattern(binding_src, line, span.column + 4)?
        {
            return Ok(Stmt {
                line,
                span,
                keyword_span: SourceSpan::new(line, span.column, 3),
                kind: StmtKind::LetStructDestructure {
                    struct_name,
                    struct_span,
                    fields,
                    expr,
                    mutable: true,
                },
            });
        }

        let raw_bindings = split_top_level_commas_with_offsets(binding_src);
        if raw_bindings.len() == 1 {
            let (raw_binding, binding_offset) = raw_bindings[0];
            let (raw_binding, binding_column) =
                trim_with_column(raw_binding, span.column + 4 + binding_offset);
            let binding = parse_binding(raw_binding, line, binding_column)?;
            return Ok(Stmt {
                line,
                span,
                keyword_span: SourceSpan::new(line, span.column, 3),
                kind: StmtKind::Var {
                    name: binding.name,
                    name_span: binding.name_span,
                    ty: binding.ty,
                    type_span: binding.type_span,
                    expr,
                },
            });
        }

        let mut bindings = Vec::with_capacity(raw_bindings.len());
        for (raw_binding, binding_offset) in raw_bindings {
            let (raw_binding, binding_column) =
                trim_with_column(raw_binding, span.column + 4 + binding_offset);
            let binding = parse_binding(raw_binding, line, binding_column)?;
            if bindings
                .iter()
                .any(|existing: &Binding| existing.name == binding.name)
            {
                return Err(diag(
                    line,
                    &format!("duplicate destructured binding '{}'", binding.name),
                ));
            }
            bindings.push(binding);
        }
        return Ok(Stmt {
            line,
            span,
            keyword_span: SourceSpan::new(line, span.column, 3),
            kind: StmtKind::LetDestructure {
                bindings,
                expr,
                else_return: false,
                mutable: true,
            },
        });
    }
    if let Some(rest) = input.strip_prefix("let ") {
        let Some(eq_offset) = rest.find('=') else {
            return Err(diag(line, "let bindings require '= expression'"));
        };
        let binding_src = &rest[..eq_offset];
        let raw_expr_src = &rest[eq_offset + 1..];
        let raw_expr_column = span.column + 4 + eq_offset + 1;
        let (trimmed_expr, expr_column) = trim_with_column(raw_expr_src, raw_expr_column);
        let (expr_src, else_return) =
            if let Some(expr_src) = trimmed_expr.strip_suffix(" else return") {
                (expr_src.trim_end(), true)
            } else {
                (trimmed_expr, false)
            };
        if let Some(bindings) =
            parse_multi_value_destructure_pattern(binding_src, line, span.column + 4)?
        {
            let expr = parse_expression_at(expr_src, line, expr_column)?;
            return Ok(Stmt {
                line,
                span,
                keyword_span: SourceSpan::new(line, span.column, 3),
                kind: StmtKind::LetMultiDestructure {
                    bindings,
                    expr,
                    else_return,
                    mutable: false,
                },
            });
        }
        if let Some(list_pattern) =
            parse_list_destructure_pattern(binding_src, line, span.column + 4, false)?
        {
            if else_return {
                return Err(diag(
                    line,
                    "'else return' is only valid on multi-value call destructuring",
                ));
            }
            let expr = parse_expression_at(expr_src, line, expr_column)?;
            return Ok(Stmt {
                line,
                span,
                keyword_span: SourceSpan::new(line, span.column, 3),
                kind: StmtKind::LetListDestructure {
                    bindings: list_pattern.bindings,
                    rest: list_pattern.rest,
                    expr,
                    mutable: false,
                },
            });
        }
        if let Some((struct_name, struct_span, fields)) =
            parse_struct_destructure_pattern(binding_src, line, span.column + 4)?
        {
            if else_return {
                return Err(diag(
                    line,
                    "'else return' is only valid on multi-value call destructuring",
                ));
            }
            let expr = parse_expression_at(expr_src, line, expr_column)?;
            return Ok(Stmt {
                line,
                span,
                keyword_span: SourceSpan::new(line, span.column, 3),
                kind: StmtKind::LetStructDestructure {
                    struct_name,
                    struct_span,
                    fields,
                    expr,
                    mutable: false,
                },
            });
        }

        let raw_bindings = split_top_level_commas_with_offsets(binding_src);
        if raw_bindings.len() == 1 {
            if else_return {
                return Err(diag(
                    line,
                    "'else return' requires a multi-value destructuring binding",
                ));
            }
            let (raw_binding, binding_offset) = raw_bindings[0];
            let (raw_binding, binding_column) =
                trim_with_column(raw_binding, span.column + 4 + binding_offset);
            let binding = parse_binding(raw_binding, line, binding_column)?;
            let expr = parse_expression_at(expr_src, line, expr_column)?;
            return Ok(Stmt {
                line,
                span,
                keyword_span: SourceSpan::new(line, span.column, 3),
                kind: StmtKind::Let {
                    name: binding.name,
                    name_span: binding.name_span,
                    ty: binding.ty,
                    type_span: binding.type_span,
                    expr,
                },
            });
        }

        let mut bindings = Vec::with_capacity(raw_bindings.len());
        for (raw_binding, binding_offset) in raw_bindings {
            let (raw_binding, binding_column) =
                trim_with_column(raw_binding, span.column + 4 + binding_offset);
            let binding = parse_binding(raw_binding, line, binding_column)?;
            if bindings
                .iter()
                .any(|existing: &Binding| existing.name == binding.name)
            {
                return Err(diag(
                    line,
                    &format!("duplicate destructured binding '{}'", binding.name),
                ));
            }
            bindings.push(binding);
        }
        let expr = parse_expression_at(expr_src, line, expr_column)?;
        return Ok(Stmt {
            line,
            span,
            keyword_span: SourceSpan::new(line, span.column, 3),
            kind: StmtKind::LetDestructure {
                bindings,
                expr,
                else_return,
                mutable: false,
            },
        });
    }

    if let Some(eq_offset) = find_assignment_operator(input) {
        let coalescing = eq_offset >= 2 && &input[eq_offset - 2..eq_offset] == "??";
        let target_end = if coalescing { eq_offset - 2 } else { eq_offset };
        let (target, target_column) = trim_with_column(&input[..target_end], span.column);
        let (expr_src, expr_column) =
            trim_with_column(&input[eq_offset + 1..], span.column + eq_offset + 1);
        if expr_src.is_empty() {
            return Err(diag(line, "assignment requires an expression"));
        }
        let rhs = parse_expression_at(expr_src, line, expr_column)?;
        let expr = if coalescing {
            validate_identifier(target, line)?;
            Expr {
                line,
                span,
                kind: ExprKind::Binary {
                    left: Box::new(Expr {
                        line,
                        span: SourceSpan::new(line, target_column, target.len()),
                        kind: ExprKind::Var(target.to_string()),
                    }),
                    op: BinOp::Coalesce,
                    right: Box::new(rhs),
                },
            }
        } else {
            rhs
        };
        if let Some(bindings) = parse_multi_value_destructure_pattern(target, line, target_column)?
        {
            return Ok(Stmt {
                line,
                span,
                keyword_span: SourceSpan::new(line, target_column, target.len()),
                kind: StmtKind::AssignMultiDestructure { bindings, expr },
            });
        }
        if let Some(list_pattern) =
            parse_list_destructure_pattern(target, line, target_column, false)?
        {
            return Ok(Stmt {
                line,
                span,
                keyword_span: SourceSpan::new(line, target_column, target.len()),
                kind: StmtKind::AssignListDestructure {
                    bindings: list_pattern.bindings,
                    rest: list_pattern.rest,
                    expr,
                },
            });
        }
        if let Some((struct_name, struct_span, fields)) =
            parse_struct_destructure_pattern(target, line, target_column)?
        {
            return Ok(Stmt {
                line,
                span,
                keyword_span: SourceSpan::new(line, target_column, target.len()),
                kind: StmtKind::AssignStructDestructure {
                    struct_name,
                    struct_span,
                    fields,
                    expr,
                },
            });
        }
        validate_identifier(target, line)?;
        return Ok(Stmt {
            line,
            span,
            keyword_span: SourceSpan::new(line, target_column, target.len()),
            kind: StmtKind::Assign {
                name: target.to_string(),
                name_span: SourceSpan::new(line, target_column, target.len()),
                expr,
                coalescing,
            },
        });
    }

    if input == "break" {
        return Ok(Stmt {
            line,
            span,
            keyword_span: SourceSpan::new(line, span.column, 5),
            kind: StmtKind::Break,
        });
    }
    if input == "continue" {
        return Ok(Stmt {
            line,
            span,
            keyword_span: SourceSpan::new(line, span.column, 8),
            kind: StmtKind::Continue,
        });
    }
    if input == "return" {
        return Ok(Stmt {
            line,
            span,
            keyword_span: SourceSpan::new(line, span.column, 6),
            kind: StmtKind::Return(Vec::new()),
        });
    }
    if let Some(expr_src) = input.strip_prefix("return ") {
        let expression_base_column = span.column + 7;
        let expressions = split_top_level_commas_with_offsets(expr_src)
            .into_iter()
            .map(|(raw_part, offset)| {
                let (part, column) = trim_with_column(raw_part, expression_base_column + offset);
                parse_expression_at(part, line, column)
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Stmt {
            line,
            span,
            keyword_span: SourceSpan::new(line, span.column, 6),
            kind: StmtKind::Return(expressions),
        });
    }

    if let Some(shell) = parse_shell_statement(input, span)? {
        return Ok(shell);
    }

    if validate_identifier(input, line).is_ok() {
        let expr = Expr {
            line,
            span,
            kind: ExprKind::ShellCall {
                name: input.to_string(),
                name_span: span,
                args: Vec::new(),
            },
        };
        return Ok(Stmt {
            line,
            span,
            keyword_span: span,
            kind: StmtKind::Expr(expr),
        });
    }

    let expr = parse_expression_at(input, line, span.column)?;
    Ok(Stmt {
        line,
        span,
        keyword_span: expr.span,
        kind: StmtKind::Expr(expr),
    })
}

fn parse_shell_statement(input: &str, span: SourceSpan) -> Result<Option<Stmt>, Diagnostic> {
    let line = span.line;
    let mut command = input.trim_end();
    let mut background = false;
    if let Some(prefix) = command.strip_suffix('&')
        && !prefix.ends_with('&')
        && prefix.chars().last().is_some_and(char::is_whitespace)
    {
        background = true;
        command = prefix.trim_end();
    }

    let mut redirect = None;
    if let Some((operator_offset, append)) = find_shell_redirect(command) {
        let operator_width = if append { 2 } else { 1 };
        let left = command[..operator_offset].trim_end();
        let raw_path = &command[operator_offset + operator_width..];
        let path_leading = raw_path.len() - raw_path.trim_start().len();
        let path_src = raw_path.trim();
        if left.is_empty() || path_src.is_empty() {
            return Err(diag(
                line,
                "redirection uses 'command > path' or 'command >> path'",
            ));
        }
        let path_column = span.column + operator_offset + operator_width + path_leading;
        redirect = Some(ShellRedirect {
            path: parse_expression_at(path_src, line, path_column)?,
            mode: if append {
                ShellRedirectMode::Append
            } else {
                ShellRedirectMode::Truncate
            },
        });
        command = left;
    }

    if !background && redirect.is_none() {
        return Ok(None);
    }
    let leading = command.len() - command.trim_start().len();
    let command = command.trim();
    if command.is_empty() {
        return Err(diag(
            line,
            "shell statement requires a function call or pipeline",
        ));
    }
    let command_column = span.column + leading;
    let expr = if split_top_level_shell_pipes(command).len() > 1 {
        parse_expression_at(command, line, command_column)?
    } else {
        match parse_shell_call_at(command, line, command_column, true)? {
            Some(expr) => expr,
            None => parse_expression_at(command, line, command_column)?,
        }
    };
    Ok(Some(Stmt {
        line,
        span,
        keyword_span: expr.span,
        kind: StmtKind::Shell {
            expr,
            redirect,
            background,
        },
    }))
}

fn find_shell_redirect(input: &str) -> Option<(usize, bool)> {
    let bytes = input.as_bytes();
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if byte == b'\\' && in_string {
            escaped = true;
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = !in_string;
            index += 1;
            continue;
        }
        if !in_string {
            match byte {
                b'(' => paren += 1,
                b')' => paren = paren.saturating_sub(1),
                b'{' => brace += 1,
                b'}' => brace = brace.saturating_sub(1),
                b'[' => bracket += 1,
                b']' => bracket = bracket.saturating_sub(1),
                b'>' if paren == 0 && brace == 0 && bracket == 0 => {
                    let before_space = index > 0 && bytes[index - 1].is_ascii_whitespace();
                    let append = bytes.get(index + 1) == Some(&b'>');
                    let after = index + if append { 2 } else { 1 };
                    let after_space = bytes.get(after).is_some_and(u8::is_ascii_whitespace);
                    if before_space && after_space {
                        return Some((index, append));
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

fn attach_match_pattern_source(pattern: &mut MatchPattern, source_id: SourceId) {
    match pattern {
        MatchPattern::Binding(binding) => binding.span = binding.span.with_source(source_id),
        MatchPattern::Struct(pattern) => {
            pattern.struct_span = pattern.struct_span.with_source(source_id);
            for field in &mut pattern.fields {
                attach_struct_pattern_field_source(field, source_id);
            }
        }
        MatchPattern::Relational(pattern) => {
            pattern.span = pattern.span.with_source(source_id);
            attach_expr_source(&mut pattern.value, source_id);
        }
        MatchPattern::Logical {
            left, right, span, ..
        } => {
            *span = span.with_source(source_id);
            attach_match_pattern_source(left, source_id);
            attach_match_pattern_source(right, source_id);
        }
    }
}

fn attach_list_match_pattern_source(pattern: &mut ListMatchPattern, source_id: SourceId) {
    match pattern {
        ListMatchPattern::List {
            bindings,
            rest,
            span,
        } => {
            *span = span.with_source(source_id);
            for binding in bindings {
                binding.span = binding.span.with_source(source_id);
            }
            if let Some(rest) = rest {
                rest.binding.span = rest.binding.span.with_source(source_id);
            }
        }
        ListMatchPattern::Wildcard { span } => *span = span.with_source(source_id),
    }
}

fn attach_struct_pattern_field_source(field: &mut StructPatternField, source_id: SourceId) {
    field.field_span = field.field_span.with_source(source_id);
    field.binding.span = field.binding.span.with_source(source_id);
    if let Some(nested) = &mut field.nested {
        nested.struct_span = nested.struct_span.with_source(source_id);
        for field in &mut nested.fields {
            attach_struct_pattern_field_source(field, source_id);
        }
    }
}

fn parse_multi_value_destructure_pattern(
    input: &str,
    line: usize,
    column: usize,
) -> Result<Option<Vec<PatternBinding>>, Diagnostic> {
    let trimmed = input.trim();
    if !trimmed.starts_with('(') {
        return Ok(None);
    }
    let Some(inner) = trimmed
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Err(diag(
            line,
            "multi-value destructuring uses 'let (first, second) = expression' syntax",
        ));
    };
    let entries = split_top_level_commas_with_offsets(inner);
    if entries.len() < 2 {
        return Err(diag(
            line,
            "multi-value destructuring requires at least two pattern positions",
        ));
    }
    let leading = input.len() - input.trim_start().len();
    let pattern_column = column + leading;
    let mut bindings = Vec::with_capacity(entries.len());
    for (raw_binding, offset) in entries {
        let (name, name_column) = trim_with_column(raw_binding, pattern_column + 1 + offset);
        if name.is_empty() {
            return Err(diag(
                line,
                "multi-value destructuring contains an empty pattern position",
            ));
        }
        validate_identifier(name, line)?;
        if name != "_"
            && bindings
                .iter()
                .any(|binding: &PatternBinding| binding.name == name)
        {
            return Err(diag(
                line,
                &format!("duplicate destructured binding '{name}'"),
            ));
        }
        bindings.push(PatternBinding {
            name: name.to_string(),
            span: SourceSpan::new(line, name_column, name.len()),
        });
    }
    Ok(Some(bindings))
}

fn parse_list_destructure_pattern(
    input: &str,
    line: usize,
    column: usize,
    allow_empty: bool,
) -> Result<Option<ParsedListDestructure>, Diagnostic> {
    let trimmed = input.trim();
    if !trimmed.starts_with('[') {
        return Ok(None);
    }
    let Some(inner) = trimmed
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    else {
        return Err(diag(
            line,
            "list destructuring uses 'let [first, second] = expression' syntax",
        ));
    };
    if inner.trim().is_empty() {
        if allow_empty {
            return Ok(Some(ParsedListDestructure {
                bindings: Vec::new(),
                rest: None,
            }));
        }
        return Err(diag(
            line,
            "list destructuring requires at least one binding",
        ));
    }
    let leading = input.len() - input.trim_start().len();
    let pattern_column = column + leading;
    let mut bindings = Vec::new();
    let mut rest = None;
    for (raw_binding, offset) in split_top_level_commas_with_offsets(inner) {
        let entry_column = pattern_column + 1 + offset;
        let (entry, entry_column) = trim_with_column(raw_binding, entry_column);
        if entry.is_empty() {
            return Err(diag(line, "list destructuring contains an empty binding"));
        }
        let (name, name_column, is_rest) = if let Some(name) = entry.strip_prefix("...") {
            let (name, name_column) = trim_with_column(name, entry_column + 3);
            if name.is_empty() {
                return Err(diag(
                    line,
                    "list rest patterns require a binding after '...'",
                ));
            }
            (name, name_column, true)
        } else {
            (entry, entry_column, false)
        };
        validate_identifier(name, line)?;
        let duplicate = name != "_"
            && (bindings
                .iter()
                .any(|existing: &PatternBinding| existing.name == name)
                || rest
                    .as_ref()
                    .is_some_and(|existing: &ListRestPattern| existing.binding.name == name));
        if duplicate {
            return Err(diag(
                line,
                &format!("duplicate list pattern binding '{name}'"),
            ));
        }
        let binding = PatternBinding {
            name: name.to_string(),
            span: SourceSpan::new(line, name_column, name.len()),
        };
        if is_rest {
            if rest.is_some() {
                return Err(diag(
                    line,
                    "list destructuring allows only one rest pattern",
                ));
            }
            rest = Some(ListRestPattern {
                binding,
                index: bindings.len(),
            });
        } else {
            bindings.push(binding);
        }
    }
    Ok(Some(ParsedListDestructure { bindings, rest }))
}

fn parse_struct_destructure_pattern(
    input: &str,
    line: usize,
    column: usize,
) -> Result<Option<(String, SourceSpan, Vec<StructPatternField>)>, Diagnostic> {
    let trimmed = input.trim();
    let leading = input.len() - input.trim_start().len();
    let pattern_column = column + leading;
    let Some(open_offset) = trimmed.find('{') else {
        return Ok(None);
    };
    let Some(inner) = trimmed.strip_suffix('}') else {
        return Ok(None);
    };
    let struct_name = trimmed[..open_offset].trim();
    if struct_name.is_empty() {
        return Ok(None);
    }
    validate_identifier(struct_name, line)?;
    let struct_offset = trimmed[..open_offset]
        .find(struct_name)
        .expect("trimmed struct pattern contains its name");
    let struct_span = SourceSpan::new(line, pattern_column + struct_offset, struct_name.len());
    let inner = &inner[open_offset + 1..];
    if inner.trim().is_empty() {
        return Err(diag(
            line,
            "struct destructuring requires at least one field",
        ));
    }

    let mut fields = Vec::new();
    for (raw_field, offset) in split_top_level_commas_with_offsets(inner) {
        let field_column = pattern_column + open_offset + 1 + offset;
        let (entry, entry_column) = trim_with_column(raw_field, field_column);
        if entry.is_empty() {
            return Err(diag(
                line,
                "struct destructuring contains an empty field pattern",
            ));
        }
        let (field_name, field_name_column, binding_name, binding_column) =
            if let Some(colon_offset) = entry.find(':') {
                let (field_name, field_name_column) =
                    trim_with_column(&entry[..colon_offset], entry_column);
                let (binding_name, binding_column) =
                    trim_with_column(&entry[colon_offset + 1..], entry_column + colon_offset + 1);
                (field_name, field_name_column, binding_name, binding_column)
            } else {
                (entry, entry_column, entry, entry_column)
            };
        validate_identifier(field_name, line)?;
        let nested = parse_struct_destructure_pattern(binding_name, line, binding_column)?.map(
            |(struct_name, struct_span, fields)| {
                Box::new(StructPattern {
                    struct_name,
                    struct_span,
                    fields,
                })
            },
        );
        if nested.is_none() && binding_name != "_" {
            validate_identifier(binding_name, line)?;
        }
        if fields
            .iter()
            .any(|existing: &StructPatternField| existing.field == field_name)
        {
            return Err(diag(
                line,
                &format!("duplicate struct pattern field '{field_name}'"),
            ));
        }
        if nested.is_none()
            && binding_name != "_"
            && fields.iter().any(|existing: &StructPatternField| {
                existing.nested.is_none() && existing.binding.name == binding_name
            })
        {
            return Err(diag(
                line,
                &format!("duplicate struct pattern binding '{binding_name}'"),
            ));
        }
        fields.push(StructPatternField {
            field: field_name.to_string(),
            field_span: SourceSpan::new(line, field_name_column, field_name.len()),
            binding: PatternBinding {
                name: if nested.is_some() {
                    "_".to_string()
                } else {
                    binding_name.to_string()
                },
                span: SourceSpan::new(line, binding_column, binding_name.len()),
            },
            nested,
        });
    }

    Ok(Some((struct_name.to_string(), struct_span, fields)))
}

fn parse_binding(input: &str, line: usize, column: usize) -> Result<Binding, Diagnostic> {
    let Some(colon_offset) = input.find(':') else {
        return Err(diag(
            line,
            "strict bindings require an explicit type: 'let name: type = value'",
        ));
    };
    let name_src = &input[..colon_offset];
    let ty_src = &input[colon_offset + 1..];
    let (name, name_column) = trim_with_column(name_src, column);
    validate_identifier(name, line)?;
    let (type_text, type_column) = trim_with_column(ty_src, column + colon_offset + 1);
    let ty = parse_type(type_text, line)?;
    if ty == Type::Void {
        return Err(diag(line, "variables cannot have type void"));
    }
    Ok(Binding {
        name: name.to_string(),
        name_span: SourceSpan::new(line, name_column, name.len()),
        ty,
        type_span: SourceSpan::new(line, type_column, type_text.len()),
    })
}

fn find_assignment_operator(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' && in_string {
            escaped = true;
            continue;
        }
        if byte == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match byte {
            b'(' | b'{' => depth += 1,
            b')' | b'}' => depth = depth.saturating_sub(1),
            b'=' if depth == 0
                && bytes.get(index.wrapping_sub(1)) != Some(&b'!')
                && bytes.get(index.wrapping_sub(1)) != Some(&b'<')
                && bytes.get(index.wrapping_sub(1)) != Some(&b'>')
                && bytes.get(index.wrapping_sub(1)) != Some(&b'=')
                && bytes.get(index + 1) != Some(&b'=') =>
            {
                return Some(index);
            }
            _ => {}
        }
    }
    None
}

fn split_top_level_commas(input: &str) -> Vec<&str> {
    split_top_level_commas_with_offsets(input)
        .into_iter()
        .map(|(part, _)| part)
        .collect()
}

fn trim_with_column(input: &str, column: usize) -> (&str, usize) {
    let leading = input.len() - input.trim_start().len();
    (input.trim(), column + leading)
}

fn split_top_level_commas_with_offsets(input: &str) -> Vec<(&str, usize)> {
    let bytes = input.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut depth = 0usize;

    for (index, byte) in bytes.iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' && in_string {
            escaped = true;
            continue;
        }
        if byte == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match byte {
            b'(' | b'{' | b'[' => depth += 1,
            b')' | b'}' | b']' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                parts.push((&input[start..index], start));
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push((&input[start..], start));
    parts
}

fn split_range_with_offset(input: &str) -> Option<(&str, &str, usize, bool)> {
    let bytes = input.as_bytes();
    let mut in_string = false;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut index = 0usize;
    while index + 1 < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if byte == b'\\' && in_string {
            escaped = true;
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = !in_string;
            index += 1;
            continue;
        }
        if !in_string {
            match byte {
                b'(' => depth += 1,
                b')' => depth = depth.saturating_sub(1),
                b'.' if depth == 0 && bytes[index + 1] == b'.' => {
                    let inclusive = bytes.get(index + 2) == Some(&b'=');
                    let end_start = index + if inclusive { 3 } else { 2 };
                    return Some((&input[..index], &input[end_start..], index, inclusive));
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

fn parse_return_types(input: &str, line: usize) -> Result<Vec<Type>, Diagnostic> {
    let input = input.trim();
    if input == "void" {
        return Ok(Vec::new());
    }
    if let Some(inner) = input
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
    {
        let parts = split_top_level_commas(inner);
        if parts.len() < 2 {
            return Err(diag(
                line,
                "multi-value return types require at least two types",
            ));
        }
        let mut returns = Vec::with_capacity(parts.len());
        for part in parts {
            let ty = parse_type(part.trim(), line)?;
            if ty == Type::Void {
                return Err(diag(line, "void cannot appear in a multi-value return"));
            }
            returns.push(ty);
        }
        return Ok(returns);
    }

    let ty = parse_type(input, line)?;
    if ty == Type::Void {
        Ok(Vec::new())
    } else {
        Ok(vec![ty])
    }
}

fn parse_type(input: &str, line: usize) -> Result<Type, Diagnostic> {
    Type::parse(input).ok_or_else(|| diag(line, &format!("unknown type '{input}'")))
}

fn validate_identifier(input: &str, line: usize) -> Result<(), Diagnostic> {
    if input.starts_with("flux__") {
        return Err(diag(
            line,
            "identifiers starting with 'flux__' are reserved for the compiler",
        ));
    }
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return Err(diag(line, "identifier cannot be empty"));
    };
    if !(first == '_' || first.is_ascii_alphabetic())
        || !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    {
        return Err(diag(line, &format!("invalid identifier '{input}'")));
    }
    if matches!(
        input,
        "fn" | "struct"
            | "enum"
            | "type"
            | "const"
            | "let"
            | "match"
            | "return"
            | "if"
            | "elif"
            | "else"
            | "for"
            | "while"
            | "in"
            | "var"
            | "break"
            | "continue"
            | "true"
            | "false"
            | "nil"
            | "none"
            | "error"
    ) {
        return Err(diag(line, &format!("'{input}' is reserved")));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Int(i64),
    Str(String),
    Ident(String),
    True,
    False,
    Nil,
    None,
    Fn,
    If,
    Else,
    For,
    In,
    Plus,
    Minus,
    Arrow,
    Star,
    Slash,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Colon,
    Dot,
    Comma,
    QuestionQuestion,
    Question,
}

#[derive(Debug, Clone, PartialEq)]
struct Token {
    kind: TokenKind,
    span: SourceSpan,
}

fn expression_column(haystack: &str, needle: &str, base_column: usize) -> usize {
    haystack
        .find(needle)
        .map(|offset| base_column + offset)
        .unwrap_or(base_column)
}

fn parse_expression_at(input: &str, line: usize, column: usize) -> Result<Expr, Diagnostic> {
    let cascade_parts = split_top_level_cascades(input);
    if cascade_parts.len() > 1 {
        let (first, first_offset) = cascade_parts[0];
        let (first, first_column) = trim_with_column(first, column + first_offset);
        if first.is_empty() {
            return Err(diag(line, "cascade requires a target before '..'"));
        }
        let mut expr = parse_expression_at(first, line, first_column)?;
        for (stage, offset) in cascade_parts.into_iter().skip(1) {
            let (stage, stage_column) = trim_with_column(stage, column + offset);
            if stage.is_empty() {
                return Err(diag(line, "cascade requires a function after '..'"));
            }
            let (name, name_span, args) = parse_pipe_stage(stage, line, stage_column, "cascade")?;
            let end = args
                .last()
                .map(|arg| arg.span.column + arg.span.length)
                .unwrap_or(name_span.column + name_span.length);
            let start = expr.span.column;
            expr = Expr {
                line,
                span: SourceSpan::new(line, start, end.saturating_sub(start)),
                kind: ExprKind::Pipe {
                    input: Box::new(expr),
                    name,
                    name_span,
                    args,
                },
            };
        }
        return Ok(expr);
    }

    let pipe_parts = split_top_level_shell_pipes(input);
    if pipe_parts.len() > 1 {
        let (first, first_offset) = pipe_parts[0];
        let (first, first_column) = trim_with_column(first, column + first_offset);
        let mut expr = match parse_shell_call_at(first, line, first_column, false)? {
            Some(expr) => expr,
            None => parse_regular_expression_at(first, line, first_column)?,
        };
        for (stage, offset) in pipe_parts.into_iter().skip(1) {
            let (stage, stage_column) = trim_with_column(stage, column + offset);
            if stage.is_empty() {
                return Err(diag(line, "pipeline requires a function after '|'"));
            }
            let (name, name_span, args) = parse_pipe_stage(stage, line, stage_column, "pipeline")?;
            let end = args
                .last()
                .map(|arg| arg.span.column + arg.span.length)
                .unwrap_or(name_span.column + name_span.length);
            let start = expr.span.column;
            expr = Expr {
                line,
                span: SourceSpan::new(line, start, end.saturating_sub(start)),
                kind: ExprKind::Pipe {
                    input: Box::new(expr),
                    name,
                    name_span,
                    args,
                },
            };
        }
        return Ok(expr);
    }
    if let Some(expr) = parse_shell_call_at(input, line, column, false)? {
        return Ok(expr);
    }
    parse_regular_expression_at(input, line, column)
}

fn parse_regular_expression_at(
    input: &str,
    line: usize,
    column: usize,
) -> Result<Expr, Diagnostic> {
    let tokens = lex_expression(input, line, column)?;
    let mut parser = ExprParser {
        tokens: &tokens,
        index: 0,
        line,
    };
    let expr = parser.parse_conditional()?;
    if parser.index != tokens.len() {
        let token = &tokens[parser.index];
        let message = if matches!(token.kind, TokenKind::If | TokenKind::Else) {
            "conditional/ternary expressions are not part of Flux; use an if statement or match"
        } else {
            "unexpected token after expression"
        };
        return Err(Diagnostic::new(DiagnosticStage::Parse, token.span, message));
    }
    Ok(expr)
}

fn parse_shell_call_at(
    input: &str,
    line: usize,
    column: usize,
    allow_bare: bool,
) -> Result<Option<Expr>, Diagnostic> {
    let parts = split_top_level_shell_words(input);
    if parts.is_empty() || (parts.len() == 1 && !allow_bare) {
        return Ok(None);
    }
    let (raw_name, name_offset) = parts[0];
    let name = raw_name.trim();
    if validate_identifier(name, line).is_err() {
        return Ok(None);
    }
    if parts.len() > 1 {
        let next = parts[1].0.trim_start();
        if next.starts_with(['+', '-', '*', '/', '<', '>', '=', '!', '&', '|', '?', '{']) {
            return Ok(None);
        }
    }
    let name_span = SourceSpan::new(line, column + name_offset, name.len());
    let mut args = Vec::new();
    for (raw_arg, offset) in parts.into_iter().skip(1) {
        let (arg, arg_column) = trim_with_column(raw_arg, column + offset);
        args.push(parse_regular_expression_at(arg, line, arg_column)?);
    }
    let end = args
        .last()
        .map(|arg| arg.span.column + arg.span.length)
        .unwrap_or(name_span.column + name_span.length);
    Ok(Some(Expr {
        line,
        span: SourceSpan::new(line, name_span.column, end - name_span.column),
        kind: ExprKind::ShellCall {
            name: name.to_string(),
            name_span,
            args,
        },
    }))
}

fn parse_pipe_stage(
    input: &str,
    line: usize,
    column: usize,
    construct: &str,
) -> Result<(String, SourceSpan, Vec<Expr>), Diagnostic> {
    if let Some(shell) = parse_shell_call_at(input, line, column, true)? {
        let ExprKind::ShellCall {
            name,
            name_span,
            args,
        } = shell.kind
        else {
            unreachable!()
        };
        return Ok((name, name_span, args));
    }
    let regular = parse_regular_expression_at(input, line, column)?;
    match regular.kind {
        ExprKind::Call {
            name,
            args,
            named_args,
        } if named_args.is_empty() => {
            let name_span = SourceSpan::new(line, regular.span.column, name.len());
            Ok((name, name_span, args))
        }
        ExprKind::Var(name) => Ok((name.clone(), regular.span, Vec::new())),
        _ => Err(Diagnostic::new(
            DiagnosticStage::Parse,
            regular.span,
            format!("{construct} stages must be function calls or function names"),
        )),
    }
}

fn split_top_level_cascades(input: &str) -> Vec<(&str, usize)> {
    let mut parts = Vec::new();
    let bytes = input.as_bytes();
    let mut start = 0usize;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if byte == b'\\' && in_string {
            escaped = true;
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = !in_string;
            index += 1;
            continue;
        }
        if !in_string {
            match byte {
                b'(' => paren += 1,
                b')' => paren = paren.saturating_sub(1),
                b'{' => brace += 1,
                b'}' => brace = brace.saturating_sub(1),
                b'[' => bracket += 1,
                b']' => bracket = bracket.saturating_sub(1),
                b'.' if paren == 0 && brace == 0 && bracket == 0 => {
                    let is_double_dot = bytes.get(index + 1) == Some(&b'.');
                    let prev_dot = index > 0 && bytes[index - 1] == b'.';
                    let third_dot = bytes.get(index + 2) == Some(&b'.');
                    let range_equal = bytes.get(index + 2) == Some(&b'=');
                    if is_double_dot && !prev_dot && !third_dot && !range_equal {
                        parts.push((&input[start..index], start));
                        start = index + 2;
                        index += 1;
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    parts.push((&input[start..], start));
    parts
}

fn split_top_level_shell_pipes(input: &str) -> Vec<(&str, usize)> {
    let mut parts = Vec::new();
    let bytes = input.as_bytes();
    let mut start = 0usize;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if byte == b'\\' && in_string {
            escaped = true;
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = !in_string;
            index += 1;
            continue;
        }
        if !in_string {
            match byte {
                b'(' => paren += 1,
                b')' => paren = paren.saturating_sub(1),
                b'{' => brace += 1,
                b'}' => brace = brace.saturating_sub(1),
                b'[' => bracket += 1,
                b']' => bracket = bracket.saturating_sub(1),
                b'|' if paren == 0 && brace == 0 && bracket == 0 => {
                    let prev_pipe = index > 0 && bytes[index - 1] == b'|';
                    let next_pipe = bytes.get(index + 1) == Some(&b'|');
                    if !prev_pipe && !next_pipe {
                        parts.push((&input[start..index], start));
                        start = index + 1;
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    parts.push((&input[start..], start));
    parts
}

fn split_top_level_shell_words(input: &str) -> Vec<(&str, usize)> {
    let bytes = input.as_bytes();
    let mut words = Vec::new();
    let mut start = None;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0usize;
    while index <= bytes.len() {
        let at_end = index == bytes.len();
        let byte = (!at_end).then(|| bytes[index]);
        let top_space = byte.is_some_and(|b| b.is_ascii_whitespace())
            && paren == 0
            && brace == 0
            && bracket == 0
            && !in_string;
        if at_end || top_space {
            if let Some(word_start) = start.take() {
                words.push((&input[word_start..index], word_start));
            }
            index += 1;
            continue;
        }
        if start.is_none() {
            start = Some(index);
        }
        let byte = byte.expect("not at end");
        if escaped {
            escaped = false;
        } else if byte == b'\\' && in_string {
            escaped = true;
        } else if byte == b'"' {
            in_string = !in_string;
        } else if !in_string {
            match byte {
                b'(' => paren += 1,
                b')' => paren = paren.saturating_sub(1),
                b'{' => brace += 1,
                b'}' => brace = brace.saturating_sub(1),
                b'[' => bracket += 1,
                b']' => bracket = bracket.saturating_sub(1),
                _ => {}
            }
        }
        index += 1;
    }
    words
}

fn normalize_multiline_string(mut value: String) -> String {
    if value.starts_with('\n') {
        value.remove(0);
    }
    if let Some(last_newline) = value.rfind('\n')
        && value[last_newline + 1..]
            .chars()
            .all(|ch| matches!(ch, ' ' | '\t'))
    {
        value.truncate(last_newline);
    }

    let common_indent = value
        .split('\n')
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.chars()
                .take_while(|ch| matches!(ch, ' ' | '\t'))
                .count()
        })
        .min()
        .unwrap_or(0);
    if common_indent == 0 {
        return value;
    }

    value
        .split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                line.chars().skip(common_indent).collect::<String>()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn lex_expression(input: &str, line: usize, column: usize) -> Result<Vec<Token>, Diagnostic> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0usize;

    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }

        let start = index;
        let kind = if bytes[index..].starts_with(b"\"\"\"") {
            index += 3;
            let mut value = String::new();
            let mut closed = false;
            while index < bytes.len() {
                if bytes[index..].starts_with(b"\"\"\"") {
                    index += 3;
                    closed = true;
                    break;
                }
                match bytes[index] {
                    b'\\' => {
                        index += 1;
                        if index >= bytes.len() {
                            break;
                        }
                        let escaped = match bytes[index] {
                            b'n' => '\n',
                            b'r' => '\r',
                            b't' => '\t',
                            b'"' => '"',
                            b'\\' => '\\',
                            other => {
                                return Err(Diagnostic::new(
                                    DiagnosticStage::Parse,
                                    SourceSpan::new(line, column + index, 1),
                                    format!("unsupported escape '\\\\{}'", other as char),
                                ));
                            }
                        };
                        value.push(escaped);
                        index += 1;
                    }
                    other if other.is_ascii() => {
                        value.push(other as char);
                        index += 1;
                    }
                    _ => {
                        let rest = &input[index..];
                        let ch = rest.chars().next().expect("non-empty unicode tail");
                        value.push(ch);
                        index += ch.len_utf8();
                    }
                }
            }
            if !closed {
                return Err(Diagnostic::new(
                    DiagnosticStage::Parse,
                    SourceSpan::new(line, column + start, input.len() - start),
                    "unterminated multiline string literal",
                ));
            }
            TokenKind::Str(normalize_multiline_string(value))
        } else if byte == b'r' && bytes.get(index + 1) == Some(&b'"') {
            index += 2;
            let mut value = String::new();
            let mut closed = false;
            while index < bytes.len() {
                match bytes[index] {
                    b'"' => {
                        index += 1;
                        closed = true;
                        break;
                    }
                    other if other.is_ascii() => {
                        value.push(other as char);
                        index += 1;
                    }
                    _ => {
                        let rest = &input[index..];
                        let ch = rest.chars().next().expect("non-empty unicode tail");
                        value.push(ch);
                        index += ch.len_utf8();
                    }
                }
            }
            if !closed {
                return Err(Diagnostic::new(
                    DiagnosticStage::Parse,
                    SourceSpan::new(line, column + start, input.len() - start),
                    "unterminated raw string literal",
                ));
            }
            TokenKind::Str(value)
        } else if byte.is_ascii_digit() {
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            let value = input[start..index].parse::<i64>().map_err(|_| {
                Diagnostic::new(
                    DiagnosticStage::Parse,
                    SourceSpan::new(line, column + start, index - start),
                    "integer literal is outside i64 range",
                )
            })?;
            TokenKind::Int(value)
        } else if byte == b'"' {
            index += 1;
            let mut value = String::new();
            let mut closed = false;
            while index < bytes.len() {
                match bytes[index] {
                    b'"' => {
                        index += 1;
                        closed = true;
                        break;
                    }
                    b'\\' => {
                        index += 1;
                        if index >= bytes.len() {
                            break;
                        }
                        let escaped = match bytes[index] {
                            b'n' => '\n',
                            b'r' => '\r',
                            b't' => '\t',
                            b'"' => '"',
                            b'\\' => '\\',
                            other => {
                                return Err(Diagnostic::new(
                                    DiagnosticStage::Parse,
                                    SourceSpan::new(line, column + index, 1),
                                    format!("unsupported escape '\\{}'", other as char),
                                ));
                            }
                        };
                        value.push(escaped);
                        index += 1;
                    }
                    other if other.is_ascii() => {
                        value.push(other as char);
                        index += 1;
                    }
                    _ => {
                        let rest = &input[index..];
                        let ch = rest.chars().next().expect("non-empty unicode tail");
                        value.push(ch);
                        index += ch.len_utf8();
                    }
                }
            }
            if !closed {
                return Err(Diagnostic::new(
                    DiagnosticStage::Parse,
                    SourceSpan::new(line, column + start, input.len() - start),
                    "unterminated string literal",
                ));
            }
            TokenKind::Str(value)
        } else if byte == b'_' || byte.is_ascii_alphabetic() {
            index += 1;
            while index < bytes.len()
                && (bytes[index] == b'_' || bytes[index].is_ascii_alphanumeric())
            {
                index += 1;
            }
            match &input[start..index] {
                "true" => TokenKind::True,
                "false" => TokenKind::False,
                "nil" => TokenKind::Nil,
                "none" => TokenKind::None,
                "fn" => TokenKind::Fn,
                "if" => TokenKind::If,
                "else" => TokenKind::Else,
                "for" => TokenKind::For,
                "in" => TokenKind::In,
                ident => TokenKind::Ident(ident.to_string()),
            }
        } else {
            let (kind, width) = match byte {
                b'+' => (TokenKind::Plus, 1),
                b'-' if bytes.get(index + 1) == Some(&b'>') => (TokenKind::Arrow, 2),
                b'-' => (TokenKind::Minus, 1),
                b'*' => (TokenKind::Star, 1),
                b'/' => (TokenKind::Slash, 1),
                b'(' => (TokenKind::LParen, 1),
                b')' => (TokenKind::RParen, 1),
                b'{' => (TokenKind::LBrace, 1),
                b'}' => (TokenKind::RBrace, 1),
                b'[' => (TokenKind::LBracket, 1),
                b']' => (TokenKind::RBracket, 1),
                b':' => (TokenKind::Colon, 1),
                b'.' => (TokenKind::Dot, 1),
                b',' => (TokenKind::Comma, 1),
                b'?' if bytes.get(index + 1) == Some(&b'?') => (TokenKind::QuestionQuestion, 2),
                b'?' => (TokenKind::Question, 1),
                b'!' if bytes.get(index + 1) == Some(&b'=') => (TokenKind::NotEq, 2),
                b'!' => (TokenKind::Bang, 1),
                b'=' if bytes.get(index + 1) == Some(&b'=') => (TokenKind::EqEq, 2),
                b'=' => {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        SourceSpan::new(line, column + index, 1),
                        "assignment is a statement, not a value-producing expression",
                    )
                    .with_note("declare mutable locals with 'var' and write assignment as its own statement"));
                }
                b'<' if bytes.get(index + 1) == Some(&b'=') => (TokenKind::Le, 2),
                b'<' => (TokenKind::Lt, 1),
                b'>' if bytes.get(index + 1) == Some(&b'=') => (TokenKind::Ge, 2),
                b'>' => (TokenKind::Gt, 1),
                b'&' if bytes.get(index + 1) == Some(&b'&') => (TokenKind::AndAnd, 2),
                b'|' if bytes.get(index + 1) == Some(&b'|') => (TokenKind::OrOr, 2),
                other => {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        SourceSpan::new(line, column + index, 1),
                        format!("unexpected character '{}' in expression", other as char),
                    ));
                }
            };
            index += width;
            kind
        };

        tokens.push(Token {
            kind,
            span: SourceSpan::new(line, column + start, index - start),
        });
    }

    if tokens.is_empty() {
        return Err(Diagnostic::new(
            DiagnosticStage::Parse,
            SourceSpan::new(line, column, 1),
            "expected expression",
        ));
    }
    Ok(tokens)
}

struct ExprParser<'a> {
    tokens: &'a [Token],
    index: usize,
    line: usize,
}

impl ExprParser<'_> {
    fn parse_conditional(&mut self) -> Result<Expr, Diagnostic> {
        self.parse_binary(1)
    }

    fn parse_binary(&mut self, min_precedence: u8) -> Result<Expr, Diagnostic> {
        let mut left = self.parse_unary()?;
        while let Some((op, precedence)) = self.peek_binary() {
            if precedence < min_precedence {
                break;
            }
            self.index += 1;
            let right = self.parse_binary(precedence + 1)?;
            let span = SourceSpan::new(
                self.line,
                left.span.column,
                right.span.column + right.span.length - left.span.column,
            );
            left = Expr {
                line: self.line,
                span,
                kind: ExprKind::Binary {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, Diagnostic> {
        if matches!(
            self.tokens.get(self.index).map(|token| &token.kind),
            Some(TokenKind::Minus)
        ) {
            let op_span = self.tokens[self.index].span;
            self.index += 1;
            let expr = self.parse_unary()?;
            let span = SourceSpan::new(
                self.line,
                op_span.column,
                expr.span.column + expr.span.length - op_span.column,
            );
            return Ok(Expr {
                line: self.line,
                span,
                kind: ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                },
            });
        }
        if matches!(
            self.tokens.get(self.index).map(|token| &token.kind),
            Some(TokenKind::Bang)
        ) {
            let op_span = self.tokens[self.index].span;
            self.index += 1;
            let expr = self.parse_unary()?;
            let span = SourceSpan::new(
                self.line,
                op_span.column,
                expr.span.column + expr.span.length - op_span.column,
            );
            return Ok(Expr {
                line: self.line,
                span,
                kind: ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                },
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, Diagnostic> {
        let mut expr = self.parse_atom()?;
        while matches!(
            self.tokens.get(self.index).map(|token| &token.kind),
            Some(TokenKind::Dot)
        ) {
            self.index += 1;
            let Some(field) = self.tokens.get(self.index).cloned() else {
                return Err(diag(self.line, "expected field name after '.'"));
            };
            let TokenKind::Ident(name) = field.kind else {
                return Err(Diagnostic::new(
                    DiagnosticStage::Parse,
                    field.span,
                    "expected field name after '.'",
                ));
            };
            self.index += 1;
            if matches!(
                self.tokens.get(self.index).map(|token| &token.kind),
                Some(TokenKind::LParen)
            ) {
                let ExprKind::Var(namespace) = &expr.kind else {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        field.span,
                        "qualified calls require a namespace name before '.'",
                    ));
                };
                let namespace = namespace.clone();
                let namespace_span = expr.span;
                self.index += 1;
                let mut args = Vec::new();
                let mut named_args = Vec::new();
                let mut saw_named = false;
                if !matches!(
                    self.tokens.get(self.index).map(|token| &token.kind),
                    Some(TokenKind::RParen)
                ) {
                    loop {
                        let named = match (
                            self.tokens.get(self.index),
                            self.tokens.get(self.index + 1).map(|token| &token.kind),
                        ) {
                            (Some(token), Some(TokenKind::Colon)) => {
                                if let TokenKind::Ident(name) = &token.kind {
                                    Some((name.clone(), token.span))
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        };
                        if let Some((arg_name, name_span)) = named {
                            saw_named = true;
                            self.index += 2;
                            if named_args.iter().any(|arg: &NamedArg| arg.name == arg_name) {
                                return Err(Diagnostic::new(
                                    DiagnosticStage::Parse,
                                    name_span,
                                    format!("duplicate named argument '{arg_name}'"),
                                ));
                            }
                            let value = self.parse_conditional()?;
                            named_args.push(NamedArg {
                                name: arg_name,
                                name_span,
                                value,
                            });
                        } else {
                            if saw_named {
                                let span = self
                                    .tokens
                                    .get(self.index)
                                    .map(|token| token.span)
                                    .unwrap_or(field.span);
                                return Err(Diagnostic::new(
                                    DiagnosticStage::Parse,
                                    span,
                                    "positional arguments cannot follow named arguments",
                                ));
                            }
                            args.push(self.parse_conditional()?);
                        }
                        match self.tokens.get(self.index).map(|token| &token.kind) {
                            Some(TokenKind::Comma) => self.index += 1,
                            Some(TokenKind::RParen) => break,
                            _ => {
                                return Err(diag(
                                    self.line,
                                    "expected ',' or ')' in qualified call arguments",
                                ));
                            }
                        }
                    }
                }
                let Some(close) = self.tokens.get(self.index) else {
                    return Err(diag(
                        self.line,
                        "expected ')' after qualified call arguments",
                    ));
                };
                if !matches!(close.kind, TokenKind::RParen) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        close.span,
                        "expected ')' after qualified call arguments",
                    ));
                }
                let close_span = close.span;
                self.index += 1;
                expr = Expr {
                    line: self.line,
                    span: SourceSpan::new(
                        self.line,
                        namespace_span.column,
                        close_span.column + close_span.length - namespace_span.column,
                    ),
                    kind: ExprKind::QualifiedCall {
                        namespace,
                        namespace_span,
                        name,
                        name_span: field.span,
                        args,
                        named_args,
                    },
                };
                continue;
            }
            let span = SourceSpan::new(
                self.line,
                expr.span.column,
                field.span.column + field.span.length - expr.span.column,
            );
            expr = Expr {
                line: self.line,
                span,
                kind: ExprKind::Field {
                    base: Box::new(expr),
                    name,
                    name_span: field.span,
                },
            };
        }
        while matches!(
            self.tokens.get(self.index).map(|token| &token.kind),
            Some(TokenKind::LBracket)
        ) {
            let open = self.tokens[self.index].span;
            self.index += 1;
            let start = if matches!(
                self.tokens.get(self.index).map(|token| &token.kind),
                Some(TokenKind::Colon)
            ) {
                None
            } else {
                Some(Box::new(self.parse_conditional()?))
            };
            if matches!(
                self.tokens.get(self.index).map(|token| &token.kind),
                Some(TokenKind::Colon)
            ) {
                self.index += 1;
                let end = if matches!(
                    self.tokens.get(self.index).map(|token| &token.kind),
                    Some(TokenKind::RBracket | TokenKind::Colon)
                ) {
                    None
                } else {
                    Some(Box::new(self.parse_conditional()?))
                };
                let step = if matches!(
                    self.tokens.get(self.index).map(|token| &token.kind),
                    Some(TokenKind::Colon)
                ) {
                    self.index += 1;
                    if matches!(
                        self.tokens.get(self.index).map(|token| &token.kind),
                        Some(TokenKind::RBracket)
                    ) {
                        None
                    } else {
                        Some(Box::new(self.parse_conditional()?))
                    }
                } else {
                    None
                };
                let Some(close) = self.tokens.get(self.index).cloned() else {
                    return Err(diag(self.line, "expected ']' after slice"));
                };
                if !matches!(close.kind, TokenKind::RBracket) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        close.span,
                        "expected ']' after slice",
                    ));
                }
                self.index += 1;
                let span = SourceSpan::new(
                    self.line,
                    expr.span.column,
                    close.span.column + close.span.length - expr.span.column,
                );
                expr = Expr {
                    line: self.line,
                    span,
                    kind: ExprKind::Slice {
                        base: Box::new(expr),
                        start,
                        end,
                        step,
                    },
                };
            } else {
                let Some(index) = start else {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        open,
                        "index expression cannot be empty",
                    ));
                };
                let Some(close) = self.tokens.get(self.index).cloned() else {
                    return Err(diag(self.line, "expected ']' after index"));
                };
                if !matches!(close.kind, TokenKind::RBracket) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        close.span,
                        "expected ']' after index",
                    ));
                }
                self.index += 1;
                let span = SourceSpan::new(
                    self.line,
                    expr.span.column,
                    close.span.column + close.span.length - expr.span.column,
                );
                expr = Expr {
                    line: self.line,
                    span,
                    kind: ExprKind::Index {
                        base: Box::new(expr),
                        index,
                    },
                };
            }
        }
        Ok(expr)
    }

    fn parse_type_annotation(&mut self) -> Result<(Type, SourceSpan), Diagnostic> {
        let Some(start) = self.tokens.get(self.index).cloned() else {
            return Err(diag(self.line, "expected type"));
        };
        let start_span = start.span;
        let mut ty = match start.kind {
            TokenKind::Ident(name) => {
                self.index += 1;
                Type::parse(&name).ok_or_else(|| {
                    Diagnostic::new(
                        DiagnosticStage::Parse,
                        start_span,
                        format!("unknown type syntax '{name}'"),
                    )
                })?
            }
            TokenKind::Fn => {
                self.index += 1;
                let Some(open) = self.tokens.get(self.index).cloned() else {
                    return Err(diag(self.line, "function types require '(' after 'fn'"));
                };
                if !matches!(open.kind, TokenKind::LParen) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        open.span,
                        "function types require '(' after 'fn'",
                    ));
                }
                self.index += 1;
                let mut params = Vec::new();
                if !matches!(
                    self.tokens.get(self.index).map(|token| &token.kind),
                    Some(TokenKind::RParen)
                ) {
                    loop {
                        let (param, span) = self.parse_type_annotation()?;
                        if param == Type::Void {
                            return Err(Diagnostic::new(
                                DiagnosticStage::Parse,
                                span,
                                "function parameters cannot have type void",
                            ));
                        }
                        params.push(param);
                        match self.tokens.get(self.index).map(|token| &token.kind) {
                            Some(TokenKind::Comma) => self.index += 1,
                            Some(TokenKind::RParen) => break,
                            _ => {
                                return Err(diag(
                                    self.line,
                                    "expected ',' or ')' in function type parameters",
                                ));
                            }
                        }
                    }
                }
                let Some(close) = self.tokens.get(self.index).cloned() else {
                    return Err(diag(self.line, "expected ')' in function type"));
                };
                if !matches!(close.kind, TokenKind::RParen) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        close.span,
                        "expected ')' in function type",
                    ));
                }
                self.index += 1;
                let Some(arrow) = self.tokens.get(self.index).cloned() else {
                    return Err(diag(self.line, "function types require '->' return type"));
                };
                if !matches!(arrow.kind, TokenKind::Arrow) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        arrow.span,
                        "function types require '->' return type",
                    ));
                }
                self.index += 1;
                let (return_ty, _) = self.parse_type_annotation()?;
                Type::Function {
                    params,
                    returns: if return_ty == Type::Void {
                        Vec::new()
                    } else {
                        vec![return_ty]
                    },
                }
            }
            _ => {
                return Err(Diagnostic::new(
                    DiagnosticStage::Parse,
                    start_span,
                    "expected type",
                ));
            }
        };

        loop {
            if matches!(
                self.tokens.get(self.index).map(|token| &token.kind),
                Some(TokenKind::LBracket)
            ) && matches!(
                self.tokens.get(self.index + 1).map(|token| &token.kind),
                Some(TokenKind::RBracket)
            ) {
                self.index += 2;
                ty = Type::List(Box::new(ty));
                continue;
            }
            if matches!(
                self.tokens.get(self.index).map(|token| &token.kind),
                Some(TokenKind::Question)
            ) {
                let question = self.tokens[self.index].span;
                if matches!(ty, Type::Void | Type::Optional(_)) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        question,
                        "optional types require a non-optional value type",
                    ));
                }
                self.index += 1;
                ty = Type::Optional(Box::new(ty));
                continue;
            }
            break;
        }
        let end = self.tokens[self.index.saturating_sub(1)].span;
        Ok((
            ty,
            SourceSpan::new(
                self.line,
                start_span.column,
                end.column + end.length - start_span.column,
            ),
        ))
    }

    fn parse_anonymous_function(&mut self, fn_span: SourceSpan) -> Result<Expr, Diagnostic> {
        let Some(open) = self.tokens.get(self.index).cloned() else {
            return Err(diag(
                self.line,
                "anonymous functions require '(' after 'fn'",
            ));
        };
        if !matches!(open.kind, TokenKind::LParen) {
            return Err(Diagnostic::new(
                DiagnosticStage::Parse,
                open.span,
                "anonymous functions require '(' after 'fn'",
            ));
        }
        self.index += 1;
        let mut params = Vec::new();
        if !matches!(
            self.tokens.get(self.index).map(|token| &token.kind),
            Some(TokenKind::RParen)
        ) {
            loop {
                let Some(name_token) = self.tokens.get(self.index).cloned() else {
                    return Err(diag(
                        self.line,
                        "anonymous function requires a parameter name",
                    ));
                };
                let TokenKind::Ident(name) = name_token.kind else {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        name_token.span,
                        "anonymous function parameters use 'name: type' syntax",
                    ));
                };
                validate_identifier(&name, self.line)?;
                if params.iter().any(|param: &Param| param.name == name) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        name_token.span,
                        format!("duplicate anonymous function parameter '{name}'"),
                    ));
                }
                self.index += 1;
                let Some(colon) = self.tokens.get(self.index).cloned() else {
                    return Err(diag(
                        self.line,
                        "anonymous function parameters use 'name: type' syntax",
                    ));
                };
                if !matches!(colon.kind, TokenKind::Colon) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        colon.span,
                        "anonymous function parameters use 'name: type' syntax",
                    ));
                }
                self.index += 1;
                let (ty, type_span) = self.parse_type_annotation()?;
                if ty == Type::Void {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        type_span,
                        "anonymous function parameters cannot have type void",
                    ));
                }
                params.push(Param {
                    name,
                    name_span: name_token.span,
                    ty,
                    type_span,
                    named_only: false,
                    default: None,
                });
                match self.tokens.get(self.index).map(|token| &token.kind) {
                    Some(TokenKind::Comma) => self.index += 1,
                    Some(TokenKind::RParen) => break,
                    _ => {
                        return Err(diag(
                            self.line,
                            "expected ',' or ')' after anonymous function parameter",
                        ));
                    }
                }
            }
        }
        let Some(close_params) = self.tokens.get(self.index).cloned() else {
            return Err(diag(
                self.line,
                "expected ')' after anonymous function parameters",
            ));
        };
        if !matches!(close_params.kind, TokenKind::RParen) {
            return Err(Diagnostic::new(
                DiagnosticStage::Parse,
                close_params.span,
                "expected ')' after anonymous function parameters",
            ));
        }
        self.index += 1;

        let return_type = if matches!(
            self.tokens.get(self.index).map(|token| &token.kind),
            Some(TokenKind::Arrow)
        ) {
            self.index += 1;
            let (ty, _) = self.parse_type_annotation()?;
            Some(ty)
        } else {
            None
        };

        let Some(open_body) = self.tokens.get(self.index).cloned() else {
            return Err(diag(
                self.line,
                "anonymous functions require a single-expression '{ ... }' body",
            ));
        };
        if !matches!(open_body.kind, TokenKind::LBrace) {
            return Err(Diagnostic::new(
                DiagnosticStage::Parse,
                open_body.span,
                "anonymous functions require a single-expression '{ ... }' body",
            ));
        }
        self.index += 1;
        if matches!(
            self.tokens.get(self.index).map(|token| &token.kind),
            Some(TokenKind::RBrace)
        ) {
            return Err(Diagnostic::new(
                DiagnosticStage::Parse,
                open_body.span,
                "anonymous function body requires an expression",
            ));
        }
        let body = self.parse_conditional()?;
        let Some(close_body) = self.tokens.get(self.index).cloned() else {
            return Err(diag(
                self.line,
                "expected '}' after anonymous function body",
            ));
        };
        if !matches!(close_body.kind, TokenKind::RBrace) {
            return Err(Diagnostic::new(
                DiagnosticStage::Parse,
                close_body.span,
                "expected '}' after anonymous function body",
            ));
        }
        self.index += 1;
        Ok(Expr {
            line: self.line,
            span: SourceSpan::new(
                self.line,
                fn_span.column,
                close_body.span.column + close_body.span.length - fn_span.column,
            ),
            kind: ExprKind::AnonymousFunction {
                params,
                return_type,
                body: Box::new(body),
            },
        })
    }

    fn parse_atom(&mut self) -> Result<Expr, Diagnostic> {
        let Some(token) = self.tokens.get(self.index).cloned() else {
            return Err(diag(self.line, "expected expression"));
        };
        self.index += 1;
        let token_span = token.span;

        match token.kind {
            TokenKind::Int(value) => Ok(Expr {
                line: self.line,
                span: token_span,
                kind: ExprKind::Int(value),
            }),
            TokenKind::Str(value) => Ok(Expr {
                line: self.line,
                span: token_span,
                kind: ExprKind::Str(value),
            }),
            TokenKind::True => Ok(Expr {
                line: self.line,
                span: token_span,
                kind: ExprKind::Bool(true),
            }),
            TokenKind::False => Ok(Expr {
                line: self.line,
                span: token_span,
                kind: ExprKind::Bool(false),
            }),
            TokenKind::Nil => Ok(Expr {
                line: self.line,
                span: token_span,
                kind: ExprKind::Nil,
            }),
            TokenKind::None => Ok(Expr {
                line: self.line,
                span: token_span,
                kind: ExprKind::None,
            }),
            TokenKind::Fn => self.parse_anonymous_function(token_span),
            TokenKind::LBracket => self.parse_list_literal(token_span),
            TokenKind::Ident(name) => {
                if matches!(
                    self.tokens.get(self.index).map(|token| &token.kind),
                    Some(TokenKind::LBrace)
                ) {
                    return self.parse_struct_literal(name, token_span);
                }
                if !matches!(
                    self.tokens.get(self.index).map(|token| &token.kind),
                    Some(TokenKind::LParen)
                ) {
                    return Ok(Expr {
                        line: self.line,
                        span: token_span,
                        kind: ExprKind::Var(name),
                    });
                }
                self.index += 1;
                let mut args = Vec::new();
                let mut named_args = Vec::new();
                let mut saw_named = false;
                if !matches!(
                    self.tokens.get(self.index).map(|token| &token.kind),
                    Some(TokenKind::RParen)
                ) {
                    loop {
                        let named = match (
                            self.tokens.get(self.index),
                            self.tokens.get(self.index + 1).map(|token| &token.kind),
                        ) {
                            (Some(token), Some(TokenKind::Colon)) => {
                                if let TokenKind::Ident(name) = &token.kind {
                                    Some((name.clone(), token.span))
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        };
                        if let Some((arg_name, name_span)) = named {
                            saw_named = true;
                            self.index += 2;
                            if named_args.iter().any(|arg: &NamedArg| arg.name == arg_name) {
                                return Err(Diagnostic::new(
                                    DiagnosticStage::Parse,
                                    name_span,
                                    format!("duplicate named argument '{arg_name}'"),
                                ));
                            }
                            let value = self.parse_conditional()?;
                            named_args.push(NamedArg {
                                name: arg_name,
                                name_span,
                                value,
                            });
                        } else {
                            if saw_named {
                                let span = self
                                    .tokens
                                    .get(self.index)
                                    .map(|token| token.span)
                                    .unwrap_or(token_span);
                                return Err(Diagnostic::new(
                                    DiagnosticStage::Parse,
                                    span,
                                    "positional arguments cannot follow named arguments",
                                ));
                            }
                            args.push(self.parse_conditional()?);
                        }
                        match self.tokens.get(self.index).map(|token| &token.kind) {
                            Some(TokenKind::Comma) => self.index += 1,
                            Some(TokenKind::RParen) => break,
                            _ => {
                                return Err(diag(
                                    self.line,
                                    "expected ',' or ')' in call arguments",
                                ));
                            }
                        }
                    }
                }
                let Some(close) = self.tokens.get(self.index) else {
                    return Err(diag(self.line, "expected ')' after call arguments"));
                };
                if !matches!(close.kind, TokenKind::RParen) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        close.span,
                        "expected ')' after call arguments",
                    ));
                }
                let close_span = close.span;
                self.index += 1;
                Ok(Expr {
                    line: self.line,
                    span: SourceSpan::new(
                        self.line,
                        token_span.column,
                        close_span.column + close_span.length - token_span.column,
                    ),
                    kind: ExprKind::Call {
                        name,
                        args,
                        named_args,
                    },
                })
            }
            TokenKind::LParen => {
                let mut expr = self.parse_conditional()?;
                let Some(close) = self.tokens.get(self.index) else {
                    return Err(diag(self.line, "expected ')'"));
                };
                if !matches!(close.kind, TokenKind::RParen) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        close.span,
                        "expected ')'",
                    ));
                }
                let close_span = close.span;
                self.index += 1;
                expr.span = SourceSpan::new(
                    self.line,
                    token_span.column,
                    close_span.column + close_span.length - token_span.column,
                );
                Ok(expr)
            }
            _ => Err(Diagnostic::new(
                DiagnosticStage::Parse,
                token_span,
                "expected expression",
            )),
        }
    }

    fn parse_list_item(&mut self) -> Result<Expr, Diagnostic> {
        if matches!(
            self.tokens.get(self.index).map(|token| &token.kind),
            Some(TokenKind::If)
        ) {
            let if_span = self.tokens[self.index].span;
            self.index += 1;
            let condition = self.parse_conditional()?;
            let Some(colon) = self.tokens.get(self.index).cloned() else {
                return Err(diag(
                    self.line,
                    "list 'if' item requires ':' before its value",
                ));
            };
            if !matches!(colon.kind, TokenKind::Colon) {
                return Err(Diagnostic::new(
                    DiagnosticStage::Parse,
                    colon.span,
                    "list 'if' item requires ':' before its value",
                ));
            }
            self.index += 1;
            let value = self.parse_conditional()?;
            let (else_value, else_span) = if matches!(
                self.tokens.get(self.index).map(|token| &token.kind),
                Some(TokenKind::Else)
            ) {
                let else_span = self.tokens[self.index].span;
                self.index += 1;
                let Some(colon) = self.tokens.get(self.index).cloned() else {
                    return Err(diag(
                        self.line,
                        "list 'else' item requires ':' before its value",
                    ));
                };
                if !matches!(colon.kind, TokenKind::Colon) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        colon.span,
                        "list 'else' item requires ':' before its value",
                    ));
                }
                self.index += 1;
                (Some(Box::new(self.parse_conditional()?)), Some(else_span))
            } else {
                (None, None)
            };
            let end = else_value.as_deref().unwrap_or(&value);
            return Ok(Expr {
                line: self.line,
                span: SourceSpan::new(
                    self.line,
                    if_span.column,
                    end.span.column + end.span.length - if_span.column,
                ),
                kind: ExprKind::ListIf {
                    condition: Box::new(condition),
                    value: Box::new(value),
                    else_value,
                    if_span,
                    else_span,
                },
            });
        }

        let is_spread = matches!(
            (
                self.tokens.get(self.index).map(|token| &token.kind),
                self.tokens.get(self.index + 1).map(|token| &token.kind),
                self.tokens.get(self.index + 2).map(|token| &token.kind),
            ),
            (
                Some(TokenKind::Dot),
                Some(TokenKind::Dot),
                Some(TokenKind::Dot)
            )
        );
        if !is_spread {
            return self.parse_conditional();
        }

        let spread_span = self.tokens[self.index].span;
        self.index += 3;
        let value = self.parse_conditional()?;
        Ok(Expr {
            line: self.line,
            span: SourceSpan::new(
                self.line,
                spread_span.column,
                value.span.column + value.span.length - spread_span.column,
            ),
            kind: ExprKind::ListSpread {
                value: Box::new(value),
                spread_span: SourceSpan::new(self.line, spread_span.column, 3),
            },
        })
    }

    fn parse_list_literal(&mut self, open_span: SourceSpan) -> Result<Expr, Diagnostic> {
        if matches!(
            self.tokens.get(self.index).map(|token| &token.kind),
            Some(TokenKind::RBracket)
        ) {
            let close = self.tokens[self.index].span;
            self.index += 1;
            return Ok(Expr {
                line: self.line,
                span: SourceSpan::new(
                    self.line,
                    open_span.column,
                    close.column + close.length - open_span.column,
                ),
                kind: ExprKind::List(Vec::new()),
            });
        }

        let first = self.parse_list_item()?;
        if !matches!(
            first.kind,
            ExprKind::ListSpread { .. } | ExprKind::ListIf { .. }
        ) && matches!(
            self.tokens.get(self.index).map(|token| &token.kind),
            Some(TokenKind::For)
        ) {
            self.index += 1;
            let Some(binding_token) = self.tokens.get(self.index).cloned() else {
                return Err(diag(
                    self.line,
                    "list comprehension requires a binding after 'for'",
                ));
            };
            let TokenKind::Ident(binding) = binding_token.kind else {
                return Err(Diagnostic::new(
                    DiagnosticStage::Parse,
                    binding_token.span,
                    "list comprehension requires a binding after 'for'",
                ));
            };
            self.index += 1;
            let Some(in_token) = self.tokens.get(self.index).cloned() else {
                return Err(diag(self.line, "list comprehension requires 'in'"));
            };
            if !matches!(in_token.kind, TokenKind::In) {
                return Err(Diagnostic::new(
                    DiagnosticStage::Parse,
                    in_token.span,
                    "list comprehension requires 'in'",
                ));
            }
            self.index += 1;
            let iterable = self.parse_conditional()?;
            let condition = if matches!(
                self.tokens.get(self.index).map(|token| &token.kind),
                Some(TokenKind::If)
            ) {
                self.index += 1;
                Some(Box::new(self.parse_conditional()?))
            } else {
                None
            };
            let Some(close) = self.tokens.get(self.index).cloned() else {
                return Err(diag(self.line, "expected ']' after list comprehension"));
            };
            if !matches!(close.kind, TokenKind::RBracket) {
                return Err(Diagnostic::new(
                    DiagnosticStage::Parse,
                    close.span,
                    "expected ']' after list comprehension",
                ));
            }
            self.index += 1;
            return Ok(Expr {
                line: self.line,
                span: SourceSpan::new(
                    self.line,
                    open_span.column,
                    close.span.column + close.span.length - open_span.column,
                ),
                kind: ExprKind::ListComprehension {
                    value: Box::new(first),
                    binding,
                    binding_span: binding_token.span,
                    iterable: Box::new(iterable),
                    condition,
                },
            });
        }

        let mut items = vec![first];
        loop {
            match self.tokens.get(self.index).map(|token| &token.kind) {
                Some(TokenKind::Comma) => {
                    self.index += 1;
                    if matches!(
                        self.tokens.get(self.index).map(|token| &token.kind),
                        Some(TokenKind::RBracket)
                    ) {
                        break;
                    }
                    items.push(self.parse_list_item()?);
                }
                Some(TokenKind::RBracket) => break,
                _ => return Err(diag(self.line, "expected ',' or ']' in list literal")),
            }
        }
        let close = self.tokens[self.index].span;
        self.index += 1;
        Ok(Expr {
            line: self.line,
            span: SourceSpan::new(
                self.line,
                open_span.column,
                close.column + close.length - open_span.column,
            ),
            kind: ExprKind::List(items),
        })
    }

    fn parse_struct_literal(
        &mut self,
        name: String,
        name_span: SourceSpan,
    ) -> Result<Expr, Diagnostic> {
        self.index += 1;
        let mut base = None;
        let mut fields = Vec::new();
        if matches!(
            (
                self.tokens.get(self.index).map(|token| &token.kind),
                self.tokens.get(self.index + 1).map(|token| &token.kind),
            ),
            (Some(TokenKind::Dot), Some(TokenKind::Dot))
        ) {
            self.index += 2;
            base = Some(Box::new(self.parse_conditional()?));
            match self.tokens.get(self.index).map(|token| &token.kind) {
                Some(TokenKind::Comma) => self.index += 1,
                Some(TokenKind::RBrace) => {}
                _ => {
                    return Err(diag(
                        self.line,
                        "expected ',' or '}' after struct update base",
                    ));
                }
            }
        }
        if !matches!(
            self.tokens.get(self.index).map(|token| &token.kind),
            Some(TokenKind::RBrace)
        ) {
            loop {
                let Some(field_token) = self.tokens.get(self.index).cloned() else {
                    return Err(diag(self.line, "expected struct field name"));
                };
                let TokenKind::Ident(field_name) = field_token.kind else {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        field_token.span,
                        "expected struct field name",
                    ));
                };
                self.index += 1;
                let Some(colon) = self.tokens.get(self.index) else {
                    return Err(diag(self.line, "expected ':' after struct field name"));
                };
                if !matches!(colon.kind, TokenKind::Colon) {
                    return Err(Diagnostic::new(
                        DiagnosticStage::Parse,
                        colon.span,
                        "expected ':' after struct field name",
                    ));
                }
                self.index += 1;
                let value = self.parse_conditional()?;
                fields.push(StructLiteralField {
                    name: field_name,
                    name_span: field_token.span,
                    value,
                });
                match self.tokens.get(self.index).map(|token| &token.kind) {
                    Some(TokenKind::Comma) => {
                        self.index += 1;
                        if matches!(
                            self.tokens.get(self.index).map(|token| &token.kind),
                            Some(TokenKind::RBrace)
                        ) {
                            break;
                        }
                    }
                    Some(TokenKind::RBrace) => break,
                    _ => return Err(diag(self.line, "expected ',' or '}' in struct literal")),
                }
            }
        }
        let Some(close) = self.tokens.get(self.index) else {
            return Err(diag(self.line, "expected '}' after struct literal"));
        };
        if !matches!(close.kind, TokenKind::RBrace) {
            return Err(Diagnostic::new(
                DiagnosticStage::Parse,
                close.span,
                "expected '}' after struct literal",
            ));
        }
        let close_span = close.span;
        self.index += 1;
        Ok(Expr {
            line: self.line,
            span: SourceSpan::new(
                self.line,
                name_span.column,
                close_span.column + close_span.length - name_span.column,
            ),
            kind: ExprKind::StructLiteral {
                name,
                name_span,
                base,
                fields,
            },
        })
    }

    fn peek_binary(&self) -> Option<(BinOp, u8)> {
        Some(match &self.tokens.get(self.index)?.kind {
            TokenKind::QuestionQuestion => (BinOp::Coalesce, 1),
            TokenKind::OrOr => (BinOp::Or, 2),
            TokenKind::AndAnd => (BinOp::And, 3),
            TokenKind::EqEq => (BinOp::Eq, 4),
            TokenKind::NotEq => (BinOp::Ne, 4),
            TokenKind::Lt => (BinOp::Lt, 5),
            TokenKind::Le => (BinOp::Le, 5),
            TokenKind::Gt => (BinOp::Gt, 5),
            TokenKind::Ge => (BinOp::Ge, 5),
            TokenKind::Plus => (BinOp::Add, 6),
            TokenKind::Minus => (BinOp::Sub, 6),
            TokenKind::Star => (BinOp::Mul, 7),
            TokenKind::Slash => (BinOp::Div, 7),
            _ => return None,
        })
    }
}

fn diag(line: usize, message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticStage::Parse, SourceSpan::line(line), message)
}
