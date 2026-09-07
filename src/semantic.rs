use crate::ast::{Program, Stmt, StmtKind, Type};
use crate::diagnostic::{Diagnostic, SourceId, SourceSpan};
use crate::parser;
use crate::typecheck::{self, Signature, Signatures};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    TypeAlias,
    Struct,
    StructField,
    Function,
    Parameter,
    Binding,
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
            collect_block_symbols(&function.body, &mut symbols);
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

fn collect_block_symbols(body: &[Stmt], symbols: &mut Vec<SemanticSymbol>) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Let {
                name,
                name_span,
                ty,
                ..
            } => symbols.push(SemanticSymbol {
                name: name.clone(),
                kind: SymbolKind::Binding,
                ty: Some(ty.clone()),
                span: *name_span,
            }),
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
            StmtKind::If {
                body, else_body, ..
            } => {
                collect_block_symbols(body, symbols);
                collect_block_symbols(else_body, symbols);
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
                collect_block_symbols(body, symbols);
            }
            StmtKind::Return(_) | StmtKind::Expr(_) => {}
        }
    }
}
