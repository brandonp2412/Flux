use std::collections::HashSet;

use crate::ast::{ExprKind, Function, Stmt, StmtKind, Type};
use crate::diagnostic::SourceSpan;
use crate::typecheck::{self, Signatures};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ControlFlowNodeId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFlowParameter {
    pub name: String,
    pub ty: Type,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlFlowNodeKind {
    Entry,
    Exit,
    Binding {
        name: String,
        ty: Type,
        mutable: bool,
    },
    Assignment {
        name: String,
    },
    Destructure {
        propagates_error: bool,
    },
    Statement,
    Conditional,
    Loop,
    Match,
    Return,
    Break,
    Continue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipMove {
    pub source: String,
    pub destination: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlFlowOwnership {
    pub reads: Vec<String>,
    pub moves: Vec<OwnershipMove>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFlowNode {
    pub id: ControlFlowNodeId,
    pub kind: ControlFlowNodeKind,
    pub span: SourceSpan,
    pub ownership: ControlFlowOwnership,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlowEdgeKind {
    Next,
    True,
    False,
    MatchArm(usize),
    Success,
    Error,
    Return,
    Break,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlFlowEdge {
    pub from: ControlFlowNodeId,
    pub to: ControlFlowNodeId,
    pub kind: ControlFlowEdgeKind,
}

#[derive(Debug, Clone)]
pub struct ControlFlowGraph {
    function: String,
    parameters: Vec<ControlFlowParameter>,
    returns: Vec<Type>,
    entry: ControlFlowNodeId,
    exit: ControlFlowNodeId,
    nodes: Vec<ControlFlowNode>,
    edges: Vec<ControlFlowEdge>,
}

impl ControlFlowGraph {
    pub fn from_function(function: &Function, signatures: &Signatures) -> Self {
        let mut builder = ControlFlowBuilder::new(function, signatures);
        let body_entry = builder.build_block(&function.body, builder.exit, None);
        builder.edge(builder.entry, body_entry, ControlFlowEdgeKind::Next);
        builder.finish()
    }

    pub fn function(&self) -> &str {
        &self.function
    }

    pub fn parameters(&self) -> &[ControlFlowParameter] {
        &self.parameters
    }

    pub fn returns(&self) -> &[Type] {
        &self.returns
    }

    pub fn entry(&self) -> ControlFlowNodeId {
        self.entry
    }

    pub fn exit(&self) -> ControlFlowNodeId {
        self.exit
    }

    pub fn nodes(&self) -> &[ControlFlowNode] {
        &self.nodes
    }

    pub fn edges(&self) -> &[ControlFlowEdge] {
        &self.edges
    }

    pub fn node(&self, id: ControlFlowNodeId) -> Option<&ControlFlowNode> {
        self.nodes.get(id.0)
    }

    pub fn outgoing(&self, id: ControlFlowNodeId) -> impl Iterator<Item = &ControlFlowEdge> {
        self.edges.iter().filter(move |edge| edge.from == id)
    }
}

#[derive(Debug, Clone, Copy)]
struct LoopTargets {
    break_target: ControlFlowNodeId,
    continue_target: ControlFlowNodeId,
}

struct ControlFlowBuilder<'a> {
    signatures: &'a Signatures,
    function: String,
    parameters: Vec<ControlFlowParameter>,
    returns: Vec<Type>,
    entry: ControlFlowNodeId,
    exit: ControlFlowNodeId,
    nodes: Vec<ControlFlowNode>,
    edges: Vec<ControlFlowEdge>,
}

impl<'a> ControlFlowBuilder<'a> {
    fn new(function: &Function, signatures: &'a Signatures) -> Self {
        let mut builder = Self {
            signatures,
            function: function.name.clone(),
            parameters: function
                .params
                .iter()
                .map(|param| ControlFlowParameter {
                    name: param.name.clone(),
                    ty: param.ty.clone(),
                    span: param.name_span,
                })
                .collect(),
            returns: function.returns.clone(),
            entry: ControlFlowNodeId(0),
            exit: ControlFlowNodeId(0),
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        builder.entry = builder.node(ControlFlowNodeKind::Entry, function.keyword_span);
        builder.exit = builder.node(ControlFlowNodeKind::Exit, function.return_span);
        builder
    }

    fn finish(self) -> ControlFlowGraph {
        ControlFlowGraph {
            function: self.function,
            parameters: self.parameters,
            returns: self.returns,
            entry: self.entry,
            exit: self.exit,
            nodes: self.nodes,
            edges: self.edges,
        }
    }

    fn node(&mut self, kind: ControlFlowNodeKind, span: SourceSpan) -> ControlFlowNodeId {
        self.node_with_ownership(kind, span, ControlFlowOwnership::default())
    }

    fn node_with_ownership(
        &mut self,
        kind: ControlFlowNodeKind,
        span: SourceSpan,
        ownership: ControlFlowOwnership,
    ) -> ControlFlowNodeId {
        let id = ControlFlowNodeId(self.nodes.len());
        self.nodes.push(ControlFlowNode {
            id,
            kind,
            span,
            ownership,
        });
        id
    }

    fn edge(&mut self, from: ControlFlowNodeId, to: ControlFlowNodeId, kind: ControlFlowEdgeKind) {
        self.edges.push(ControlFlowEdge { from, to, kind });
    }

    fn build_block(
        &mut self,
        body: &[Stmt],
        successor: ControlFlowNodeId,
        loop_targets: Option<LoopTargets>,
    ) -> ControlFlowNodeId {
        let mut next = successor;
        for stmt in body.iter().rev() {
            next = self.build_stmt(stmt, next, loop_targets);
        }
        next
    }

    fn build_stmt(
        &mut self,
        stmt: &Stmt,
        successor: ControlFlowNodeId,
        loop_targets: Option<LoopTargets>,
    ) -> ControlFlowNodeId {
        let ownership = self.ownership_for_stmt(stmt);
        match &stmt.kind {
            StmtKind::Let { name, ty, .. } => self.linear_node(
                ControlFlowNodeKind::Binding {
                    name: name.clone(),
                    ty: ty.clone(),
                    mutable: false,
                },
                stmt.span,
                successor,
                ownership,
            ),
            StmtKind::Var { name, ty, .. } => self.linear_node(
                ControlFlowNodeKind::Binding {
                    name: name.clone(),
                    ty: ty.clone(),
                    mutable: true,
                },
                stmt.span,
                successor,
                ownership,
            ),
            StmtKind::Assign { name, .. } => self.linear_node(
                ControlFlowNodeKind::Assignment { name: name.clone() },
                stmt.span,
                successor,
                ownership,
            ),
            StmtKind::LetDestructure { else_return, .. }
            | StmtKind::LetMultiDestructure { else_return, .. } => {
                let node = self.node_with_ownership(
                    ControlFlowNodeKind::Destructure {
                        propagates_error: *else_return,
                    },
                    stmt.span,
                    ownership,
                );
                if *else_return {
                    self.edge(node, successor, ControlFlowEdgeKind::Success);
                    self.edge(node, self.exit, ControlFlowEdgeKind::Error);
                } else {
                    self.edge(node, successor, ControlFlowEdgeKind::Next);
                }
                node
            }
            StmtKind::Return(_) => {
                let node =
                    self.node_with_ownership(ControlFlowNodeKind::Return, stmt.span, ownership);
                self.edge(node, self.exit, ControlFlowEdgeKind::Return);
                node
            }
            StmtKind::Break => {
                let node = self.node(ControlFlowNodeKind::Break, stmt.span);
                let target = loop_targets
                    .map(|targets| targets.break_target)
                    .unwrap_or(successor);
                self.edge(node, target, ControlFlowEdgeKind::Break);
                node
            }
            StmtKind::Continue => {
                let node = self.node(ControlFlowNodeKind::Continue, stmt.span);
                let target = loop_targets
                    .map(|targets| targets.continue_target)
                    .unwrap_or(successor);
                self.edge(node, target, ControlFlowEdgeKind::Continue);
                node
            }
            StmtKind::If {
                body, else_body, ..
            } => {
                let node = self.node_with_ownership(
                    ControlFlowNodeKind::Conditional,
                    stmt.span,
                    ownership,
                );
                let then_entry = self.build_block(body, successor, loop_targets);
                let else_entry = self.build_block(else_body, successor, loop_targets);
                self.edge(node, then_entry, ControlFlowEdgeKind::True);
                self.edge(node, else_entry, ControlFlowEdgeKind::False);
                node
            }
            StmtKind::ForRange { body, .. }
            | StmtKind::ForEach { body, .. }
            | StmtKind::While { body, .. } => {
                let node =
                    self.node_with_ownership(ControlFlowNodeKind::Loop, stmt.span, ownership);
                let body_entry = self.build_block(
                    body,
                    node,
                    Some(LoopTargets {
                        break_target: successor,
                        continue_target: node,
                    }),
                );
                self.edge(node, body_entry, ControlFlowEdgeKind::True);
                self.edge(node, successor, ControlFlowEdgeKind::False);
                node
            }
            StmtKind::Match { arms, .. } => {
                let node =
                    self.node_with_ownership(ControlFlowNodeKind::Match, stmt.span, ownership);
                for (index, arm) in arms.iter().enumerate() {
                    let arm_entry = self.build_block(&arm.body, successor, loop_targets);
                    self.edge(node, arm_entry, ControlFlowEdgeKind::MatchArm(index));
                }
                node
            }
            StmtKind::ListMatch { arms, .. } => {
                let node =
                    self.node_with_ownership(ControlFlowNodeKind::Match, stmt.span, ownership);
                for (index, arm) in arms.iter().enumerate() {
                    let arm_entry = self.build_block(&arm.body, successor, loop_targets);
                    self.edge(node, arm_entry, ControlFlowEdgeKind::MatchArm(index));
                }
                node
            }
            StmtKind::LetListDestructure { .. }
            | StmtKind::LetStructDestructure { .. }
            | StmtKind::Expr(_)
            | StmtKind::Shell { .. } => self.linear_node(
                ControlFlowNodeKind::Statement,
                stmt.span,
                successor,
                ownership,
            ),
        }
    }

    fn linear_node(
        &mut self,
        kind: ControlFlowNodeKind,
        span: SourceSpan,
        successor: ControlFlowNodeId,
        ownership: ControlFlowOwnership,
    ) -> ControlFlowNodeId {
        let node = self.node_with_ownership(kind, span, ownership);
        self.edge(node, successor, ControlFlowEdgeKind::Next);
        node
    }

    fn ownership_for_stmt(&self, stmt: &Stmt) -> ControlFlowOwnership {
        let mut reads = HashSet::new();
        let mut moves = Vec::new();
        match &stmt.kind {
            StmtKind::Let { name, ty, expr, .. } => {
                typecheck::collect_expr_reads(expr, &mut reads);
                if !self.signatures.is_copy_type(ty)
                    && let ExprKind::Var(source) = &expr.kind
                {
                    moves.push(OwnershipMove {
                        source: source.clone(),
                        destination: name.clone(),
                        span: expr.span,
                    });
                }
            }
            StmtKind::Var { expr, .. }
            | StmtKind::Assign { expr, .. }
            | StmtKind::LetDestructure { expr, .. }
            | StmtKind::LetMultiDestructure { expr, .. }
            | StmtKind::LetListDestructure { expr, .. }
            | StmtKind::LetStructDestructure { expr, .. } => {
                typecheck::collect_expr_reads(expr, &mut reads);
            }
            StmtKind::Return(values) => {
                for value in values {
                    typecheck::collect_expr_reads(value, &mut reads);
                }
            }
            StmtKind::Expr(expr) => typecheck::collect_expr_reads(expr, &mut reads),
            StmtKind::Shell { expr, redirect, .. } => {
                typecheck::collect_expr_reads(expr, &mut reads);
                if let Some(redirect) = redirect {
                    typecheck::collect_expr_reads(&redirect.path, &mut reads);
                }
            }
            StmtKind::If { cond, .. } | StmtKind::While { cond, .. } => {
                typecheck::collect_expr_reads(cond, &mut reads);
            }
            StmtKind::ForRange { start, end, .. } => {
                typecheck::collect_expr_reads(start, &mut reads);
                typecheck::collect_expr_reads(end, &mut reads);
            }
            StmtKind::ForEach { iterable, .. } => {
                typecheck::collect_expr_reads(iterable, &mut reads);
            }
            StmtKind::Match { value, .. } | StmtKind::ListMatch { value, .. } => {
                typecheck::collect_expr_reads(value, &mut reads);
            }
            StmtKind::Break | StmtKind::Continue => {}
        }
        let mut reads = reads.into_iter().collect::<Vec<_>>();
        reads.sort();
        ControlFlowOwnership { reads, moves }
    }
}
