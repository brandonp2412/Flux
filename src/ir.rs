use crate::ast::{Function, Stmt, StmtKind, Type};
use crate::diagnostic::SourceSpan;

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
pub struct ControlFlowNode {
    pub id: ControlFlowNodeId,
    pub kind: ControlFlowNodeKind,
    pub span: SourceSpan,
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
    pub fn from_function(function: &Function) -> Self {
        let mut builder = ControlFlowBuilder::new(function);
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

struct ControlFlowBuilder {
    function: String,
    parameters: Vec<ControlFlowParameter>,
    returns: Vec<Type>,
    entry: ControlFlowNodeId,
    exit: ControlFlowNodeId,
    nodes: Vec<ControlFlowNode>,
    edges: Vec<ControlFlowEdge>,
}

impl ControlFlowBuilder {
    fn new(function: &Function) -> Self {
        let mut builder = Self {
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
        let id = ControlFlowNodeId(self.nodes.len());
        self.nodes.push(ControlFlowNode { id, kind, span });
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
        match &stmt.kind {
            StmtKind::Let { name, ty, .. } => self.linear_node(
                ControlFlowNodeKind::Binding {
                    name: name.clone(),
                    ty: ty.clone(),
                    mutable: false,
                },
                stmt.span,
                successor,
            ),
            StmtKind::Var { name, ty, .. } => self.linear_node(
                ControlFlowNodeKind::Binding {
                    name: name.clone(),
                    ty: ty.clone(),
                    mutable: true,
                },
                stmt.span,
                successor,
            ),
            StmtKind::Assign { name, .. } => self.linear_node(
                ControlFlowNodeKind::Assignment { name: name.clone() },
                stmt.span,
                successor,
            ),
            StmtKind::LetDestructure { else_return, .. }
            | StmtKind::LetMultiDestructure { else_return, .. } => {
                let node = self.node(
                    ControlFlowNodeKind::Destructure {
                        propagates_error: *else_return,
                    },
                    stmt.span,
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
                let node = self.node(ControlFlowNodeKind::Return, stmt.span);
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
                let node = self.node(ControlFlowNodeKind::Conditional, stmt.span);
                let then_entry = self.build_block(body, successor, loop_targets);
                let else_entry = self.build_block(else_body, successor, loop_targets);
                self.edge(node, then_entry, ControlFlowEdgeKind::True);
                self.edge(node, else_entry, ControlFlowEdgeKind::False);
                node
            }
            StmtKind::ForRange { body, .. }
            | StmtKind::ForEach { body, .. }
            | StmtKind::While { body, .. } => {
                let node = self.node(ControlFlowNodeKind::Loop, stmt.span);
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
                let node = self.node(ControlFlowNodeKind::Match, stmt.span);
                for (index, arm) in arms.iter().enumerate() {
                    let arm_entry = self.build_block(&arm.body, successor, loop_targets);
                    self.edge(node, arm_entry, ControlFlowEdgeKind::MatchArm(index));
                }
                node
            }
            StmtKind::ListMatch { arms, .. } => {
                let node = self.node(ControlFlowNodeKind::Match, stmt.span);
                for (index, arm) in arms.iter().enumerate() {
                    let arm_entry = self.build_block(&arm.body, successor, loop_targets);
                    self.edge(node, arm_entry, ControlFlowEdgeKind::MatchArm(index));
                }
                node
            }
            StmtKind::LetListDestructure { .. }
            | StmtKind::LetStructDestructure { .. }
            | StmtKind::Expr(_)
            | StmtKind::Shell { .. } => {
                self.linear_node(ControlFlowNodeKind::Statement, stmt.span, successor)
            }
        }
    }

    fn linear_node(
        &mut self,
        kind: ControlFlowNodeKind,
        span: SourceSpan,
        successor: ControlFlowNodeId,
    ) -> ControlFlowNodeId {
        let node = self.node(kind, span);
        self.edge(node, successor, ControlFlowEdgeKind::Next);
        node
    }
}
