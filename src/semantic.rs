use crate::ast::{Expr, ExprKind, MatchPattern, Program, Stmt, StmtKind, StructPatternField, Type};
use crate::diagnostic::{Diagnostic, SourceId, SourceSpan};
use crate::parser;
use crate::typecheck::{self, Signature, Signatures};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    TypeAlias,
    Interface,
    InterfaceFunction,
    InterfaceImplementation,
    InterfaceImplementationMapping,
    Constant,
    Enum,
    EnumVariant,
    Struct,
    StructField,
    View,
    ViewState,
    ViewElement,
    ViewProperty,
    Function,
    Parameter,
    Binding,
    MutableBinding,
    PatternBinding,
    LoopVariable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub ty: Option<Type>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct SemanticDatabase {
    program: Program,
    signatures: Signatures,
    symbols: Vec<SemanticSymbol>,
}

impl SemanticDatabase {
    pub fn analyze(source: &str, source_id: SourceId) -> Result<Self, Vec<Diagnostic>> {
        let program = parser::parse_all_with_source(source, source_id)?;
        let signatures = typecheck::check_all(&program)?;
        Ok(Self::from_analyzed(program, signatures))
    }

    pub fn from_analyzed(program: Program, signatures: Signatures) -> Self {
        let mut symbols = Vec::new();
        for alias in &program.aliases {
            symbols.push(SemanticSymbol {
                name: alias.name.clone(),
                kind: SymbolKind::TypeAlias,
                ty: Some(alias.target.clone()),
                span: alias.name_span,
            });
        }
        for definition in &program.interfaces {
            symbols.push(SemanticSymbol {
                name: definition.name.clone(),
                kind: SymbolKind::Interface,
                ty: None,
                span: definition.name_span,
            });
            for function in &definition.functions {
                symbols.push(SemanticSymbol {
                    name: function.name.clone(),
                    kind: SymbolKind::InterfaceFunction,
                    ty: Some(Type::Function {
                        params: function
                            .params
                            .iter()
                            .map(|param| param.ty.clone())
                            .collect(),
                        returns: function.returns.clone(),
                    }),
                    span: function.name_span,
                });
                for param in &function.params {
                    symbols.push(SemanticSymbol {
                        name: param.name.clone(),
                        kind: SymbolKind::Parameter,
                        ty: Some(param.ty.clone()),
                        span: param.name_span,
                    });
                }
            }
        }
        for implementation in &program.implementations {
            symbols.push(SemanticSymbol {
                name: format!(
                    "{} for {}",
                    implementation.interface_name, implementation.target_name
                ),
                kind: SymbolKind::InterfaceImplementation,
                ty: None,
                span: implementation.span,
            });
            let interface = signatures.interface(&implementation.interface_name);
            for mapping in &implementation.mappings {
                let ty = interface
                    .and_then(|interface| interface.functions.get(&mapping.member))
                    .map(|signature| Type::Function {
                        params: signature.params.clone(),
                        returns: signature.returns.clone(),
                    });
                symbols.push(SemanticSymbol {
                    name: mapping.member.clone(),
                    kind: SymbolKind::InterfaceImplementationMapping,
                    ty,
                    span: mapping.member_span,
                });
            }
        }
        for constant in &program.constants {
            symbols.push(SemanticSymbol {
                name: constant.name.clone(),
                kind: SymbolKind::Constant,
                ty: Some(constant.ty.clone()),
                span: constant.name_span,
            });
        }
        for definition in &program.enums {
            symbols.push(SemanticSymbol {
                name: definition.name.clone(),
                kind: SymbolKind::Enum,
                ty: None,
                span: definition.name_span,
            });
            for variant in &definition.variants {
                symbols.push(SemanticSymbol {
                    name: variant.name.clone(),
                    kind: SymbolKind::EnumVariant,
                    ty: Some(Type::Named(definition.name.clone())),
                    span: variant.name_span,
                });
            }
        }
        for definition in &program.structs {
            symbols.push(SemanticSymbol {
                name: definition.name.clone(),
                kind: SymbolKind::Struct,
                ty: None,
                span: definition.name_span,
            });
            for field in &definition.fields {
                symbols.push(SemanticSymbol {
                    name: field.name.clone(),
                    kind: SymbolKind::StructField,
                    ty: Some(field.ty.clone()),
                    span: field.name_span,
                });
            }
        }
        for view in &program.views {
            symbols.push(SemanticSymbol {
                name: view.name.clone(),
                kind: SymbolKind::View,
                ty: None,
                span: view.name_span,
            });
            for param in &view.params {
                symbols.push(SemanticSymbol {
                    name: param.name.clone(),
                    kind: SymbolKind::Parameter,
                    ty: Some(param.ty.clone()),
                    span: param.name_span,
                });
            }
            for state in &view.states {
                symbols.push(SemanticSymbol {
                    name: state.name.clone(),
                    kind: SymbolKind::ViewState,
                    ty: Some(state.ty.clone()),
                    span: state.name_span,
                });
            }
            for element in &view.elements {
                symbols.push(SemanticSymbol {
                    name: element.name.clone(),
                    kind: SymbolKind::ViewElement,
                    ty: None,
                    span: element.name_span,
                });
                for property in &element.properties {
                    symbols.push(SemanticSymbol {
                        name: property.name.clone(),
                        kind: SymbolKind::ViewProperty,
                        ty: typecheck::view_element_property_type(
                            &program,
                            &signatures,
                            &element.kind,
                            &property.name,
                        ),
                        span: property.name_span,
                    });
                }
            }
        }
        for function in &program.functions {
            symbols.push(SemanticSymbol {
                name: function.name.clone(),
                kind: SymbolKind::Function,
                ty: None,
                span: function.name_span,
            });
            for param in &function.params {
                symbols.push(SemanticSymbol {
                    name: param.name.clone(),
                    kind: SymbolKind::Parameter,
                    ty: Some(param.ty.clone()),
                    span: param.name_span,
                });
            }
            collect_block_symbols(&function.body, &mut symbols, &signatures);
        }
        Self {
            program,
            signatures,
            symbols,
        }
    }

    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn signature(&self, name: &str) -> Option<&Signature> {
        self.signatures.get(name)
    }

    pub fn signatures(&self) -> &Signatures {
        &self.signatures
    }

    pub fn symbols(&self) -> &[SemanticSymbol] {
        &self.symbols
    }

    pub fn symbols_named(&self, name: &str) -> impl Iterator<Item = &SemanticSymbol> {
        self.symbols
            .iter()
            .filter(move |symbol| symbol.name == name)
    }

    pub fn symbol_at(
        &self,
        source_id: SourceId,
        line: usize,
        column: usize,
    ) -> Option<&SemanticSymbol> {
        self.symbols.iter().find(|symbol| {
            symbol.span.source_id == source_id
                && symbol.span.line == line
                && column >= symbol.span.column
                && column < symbol.span.column + symbol.span.length
        })
    }
}

fn collect_struct_pattern_symbols(
    fields: &[StructPatternField],
    struct_name: &str,
    symbols: &mut Vec<SemanticSymbol>,
    signatures: &Signatures,
) {
    let Some(definition) = signatures.struct_type(struct_name) else {
        return;
    };
    for field in fields {
        let Some(field_signature) = definition.field(&field.field) else {
            continue;
        };
        if let Some(nested) = &field.nested {
            let Type::Named(nested_name) = signatures.canonical_type(&field_signature.ty) else {
                continue;
            };
            collect_struct_pattern_symbols(&nested.fields, &nested_name, symbols, signatures);
            continue;
        }
        if field.binding.name == "_" {
            continue;
        }
        symbols.push(SemanticSymbol {
            name: field.binding.name.clone(),
            kind: SymbolKind::PatternBinding,
            ty: Some(field_signature.ty.clone()),
            span: field.binding.span,
        });
    }
}

fn collect_expr_pattern_symbols(
    expr: &Expr,
    symbols: &mut Vec<SemanticSymbol>,
    signatures: &Signatures,
) {
    match &expr.kind {
        ExprKind::Match { value, arms } => {
            collect_expr_pattern_symbols(value, symbols, signatures);
            for arm in arms {
                let payloads = signatures
                    .enum_type(&arm.enum_name)
                    .and_then(|definition| definition.variant(&arm.variant))
                    .map(|variant| variant.payloads.as_slice())
                    .unwrap_or(&[]);
                for (index, pattern) in arm.patterns.iter().enumerate() {
                    match pattern {
                        MatchPattern::Binding(binding) => {
                            if binding.name == "_" {
                                continue;
                            }
                            symbols.push(SemanticSymbol {
                                name: binding.name.clone(),
                                kind: SymbolKind::PatternBinding,
                                ty: payloads.get(index).cloned(),
                                span: binding.span,
                            });
                        }
                        MatchPattern::Struct(pattern) => {
                            let Some(payload_ty) = payloads.get(index) else {
                                continue;
                            };
                            let Type::Named(struct_name) = signatures.canonical_type(payload_ty)
                            else {
                                continue;
                            };
                            collect_struct_pattern_symbols(
                                &pattern.fields,
                                &struct_name,
                                symbols,
                                signatures,
                            );
                        }
                    }
                }
                collect_expr_pattern_symbols(&arm.value, symbols, signatures);
            }
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            collect_expr_pattern_symbols(then_expr, symbols, signatures);
            collect_expr_pattern_symbols(cond, symbols, signatures);
            collect_expr_pattern_symbols(else_expr, symbols, signatures);
        }
        ExprKind::Field { base, .. } | ExprKind::Unary { expr: base, .. } => {
            collect_expr_pattern_symbols(base, symbols, signatures);
        }
        ExprKind::Binary { left, right, .. } => {
            collect_expr_pattern_symbols(left, symbols, signatures);
            collect_expr_pattern_symbols(right, symbols, signatures);
        }
        ExprKind::Call {
            args, named_args, ..
        } => {
            for arg in args {
                collect_expr_pattern_symbols(arg, symbols, signatures);
            }
            for arg in named_args {
                collect_expr_pattern_symbols(&arg.value, symbols, signatures);
            }
        }
        ExprKind::ShellCall { args, .. } => {
            for arg in args {
                collect_expr_pattern_symbols(arg, symbols, signatures);
            }
        }
        ExprKind::Pipe { input, args, .. } => {
            collect_expr_pattern_symbols(input, symbols, signatures);
            for arg in args {
                collect_expr_pattern_symbols(arg, symbols, signatures);
            }
        }
        ExprKind::List(items) => {
            for item in items {
                collect_expr_pattern_symbols(item, symbols, signatures);
            }
        }
        ExprKind::ListSpread { value, .. } => {
            collect_expr_pattern_symbols(value, symbols, signatures);
        }
        ExprKind::Index { base, index } => {
            collect_expr_pattern_symbols(base, symbols, signatures);
            collect_expr_pattern_symbols(index, symbols, signatures);
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            collect_expr_pattern_symbols(base, symbols, signatures);
            if let Some(start) = start {
                collect_expr_pattern_symbols(start, symbols, signatures);
            }
            if let Some(end) = end {
                collect_expr_pattern_symbols(end, symbols, signatures);
            }
            if let Some(step) = step {
                collect_expr_pattern_symbols(step, symbols, signatures);
            }
        }
        ExprKind::ListComprehension {
            value,
            iterable,
            condition,
            ..
        } => {
            collect_expr_pattern_symbols(iterable, symbols, signatures);
            collect_expr_pattern_symbols(value, symbols, signatures);
            if let Some(condition) = condition {
                collect_expr_pattern_symbols(condition, symbols, signatures);
            }
        }
        ExprKind::QualifiedCall {
            args, named_args, ..
        } => {
            for arg in args {
                collect_expr_pattern_symbols(arg, symbols, signatures);
            }
            for arg in named_args {
                collect_expr_pattern_symbols(&arg.value, symbols, signatures);
            }
        }
        ExprKind::StructLiteral { base, fields, .. } => {
            if let Some(base) = base {
                collect_expr_pattern_symbols(base, symbols, signatures);
            }
            for field in fields {
                collect_expr_pattern_symbols(&field.value, symbols, signatures);
            }
        }
        ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::Var(_) => {}
    }
}

fn collect_block_symbols(
    body: &[Stmt],
    symbols: &mut Vec<SemanticSymbol>,
    signatures: &Signatures,
) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Let {
                name,
                name_span,
                ty,
                expr,
                ..
            }
            | StmtKind::Var {
                name,
                name_span,
                ty,
                expr,
                ..
            } => {
                symbols.push(SemanticSymbol {
                    name: name.clone(),
                    kind: if matches!(stmt.kind, StmtKind::Var { .. }) {
                        SymbolKind::MutableBinding
                    } else {
                        SymbolKind::Binding
                    },
                    ty: Some(ty.clone()),
                    span: *name_span,
                });
                collect_expr_pattern_symbols(expr, symbols, signatures);
            }
            StmtKind::Assign { expr, .. } => {
                collect_expr_pattern_symbols(expr, symbols, signatures);
            }
            StmtKind::LetDestructure { bindings, .. } => {
                for binding in bindings {
                    symbols.push(SemanticSymbol {
                        name: binding.name.clone(),
                        kind: SymbolKind::Binding,
                        ty: Some(binding.ty.clone()),
                        span: binding.name_span,
                    });
                }
            }
            StmtKind::LetStructDestructure {
                struct_name,
                fields,
                ..
            } => {
                let definition = signatures.canonical_type(&Type::Named(struct_name.clone()));
                let concrete_name = match definition {
                    Type::Named(name) => name,
                    _ => struct_name.clone(),
                };
                collect_struct_pattern_symbols(fields, &concrete_name, symbols, signatures);
            }
            StmtKind::If {
                body, else_body, ..
            } => {
                collect_block_symbols(body, symbols, signatures);
                collect_block_symbols(else_body, symbols, signatures);
            }
            StmtKind::While { body, .. } => {
                collect_block_symbols(body, symbols, signatures);
            }
            StmtKind::ForRange {
                name,
                name_span,
                body,
                ..
            } => {
                symbols.push(SemanticSymbol {
                    name: name.clone(),
                    kind: SymbolKind::LoopVariable,
                    ty: Some(Type::I64),
                    span: *name_span,
                });
                collect_block_symbols(body, symbols, signatures);
            }
            StmtKind::ForEach {
                index_name,
                index_span,
                name,
                name_span,
                body,
                ..
            } => {
                if let (Some(index_name), Some(index_span)) = (index_name, index_span) {
                    symbols.push(SemanticSymbol {
                        name: index_name.clone(),
                        kind: SymbolKind::LoopVariable,
                        ty: Some(Type::I64),
                        span: *index_span,
                    });
                }
                symbols.push(SemanticSymbol {
                    name: name.clone(),
                    kind: SymbolKind::LoopVariable,
                    ty: None,
                    span: *name_span,
                });
                collect_block_symbols(body, symbols, signatures);
            }
            StmtKind::Match { arms, .. } => {
                for arm in arms {
                    let payloads = signatures
                        .enum_type(&arm.enum_name)
                        .and_then(|definition| definition.variant(&arm.variant))
                        .map(|variant| variant.payloads.as_slice())
                        .unwrap_or(&[]);
                    for (index, pattern) in arm.patterns.iter().enumerate() {
                        match pattern {
                            MatchPattern::Binding(binding) => {
                                if binding.name == "_" {
                                    continue;
                                }
                                symbols.push(SemanticSymbol {
                                    name: binding.name.clone(),
                                    kind: SymbolKind::PatternBinding,
                                    ty: payloads.get(index).cloned(),
                                    span: binding.span,
                                });
                            }
                            MatchPattern::Struct(pattern) => {
                                let Some(payload_ty) = payloads.get(index) else {
                                    continue;
                                };
                                let Type::Named(struct_name) =
                                    signatures.canonical_type(payload_ty)
                                else {
                                    continue;
                                };
                                collect_struct_pattern_symbols(
                                    &pattern.fields,
                                    &struct_name,
                                    symbols,
                                    signatures,
                                );
                            }
                        }
                    }
                    collect_block_symbols(&arm.body, symbols, signatures);
                }
            }
            StmtKind::Return(values) => {
                for value in values {
                    collect_expr_pattern_symbols(value, symbols, signatures);
                }
            }
            StmtKind::Expr(expr) => collect_expr_pattern_symbols(expr, symbols, signatures),
            StmtKind::Shell { expr, redirect, .. } => {
                collect_expr_pattern_symbols(expr, symbols, signatures);
                if let Some(redirect) = redirect {
                    collect_expr_pattern_symbols(&redirect.path, symbols, signatures);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
        }
    }
}
