use std::collections::{BTreeMap, HashSet, VecDeque};

use crate::ast::{Expr, ExprKind, Function, Stmt, StmtKind, Type};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlowEvaluationKind {
    BindingInitializer,
    AssignmentValue,
    DestructureValue,
    ReturnValue(usize),
    ExpressionStatement,
    ShellValue,
    RedirectPath,
    Condition,
    RangeStart,
    RangeEnd,
    Iterable,
    MatchValue,
    MatchGuard(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlFlowNodeKind {
    Entry,
    Exit,
    Evaluation(ControlFlowEvaluationKind),
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
pub struct OwnershipMovedBinding {
    pub name: String,
    pub origin: SourceSpan,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlFlowMoveState {
    reachable: bool,
    moved: Vec<OwnershipMovedBinding>,
}

impl ControlFlowMoveState {
    pub fn reachable(&self) -> bool {
        self.reachable
    }

    pub fn moved(&self) -> &[OwnershipMovedBinding] {
        &self.moved
    }

    pub fn is_moved(&self, name: &str) -> bool {
        self.origin(name).is_some()
    }

    pub fn origin(&self, name: &str) -> Option<SourceSpan> {
        self.moved
            .iter()
            .find(|binding| binding.name == name)
            .map(|binding| binding.origin)
    }
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
    GuardTrue,
    GuardFalse,
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
    move_states_before: Vec<ControlFlowMoveState>,
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

    pub fn incoming(&self, id: ControlFlowNodeId) -> impl Iterator<Item = &ControlFlowEdge> {
        self.edges.iter().filter(move |edge| edge.to == id)
    }

    pub fn move_state_before(&self, id: ControlFlowNodeId) -> Option<&ControlFlowMoveState> {
        self.move_states_before.get(id.0)
    }

    pub fn is_reachable(&self, id: ControlFlowNodeId) -> bool {
        self.move_state_before(id)
            .is_some_and(ControlFlowMoveState::reachable)
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
        let move_states_before = compute_move_states(&self.nodes, &self.edges, self.entry);
        ControlFlowGraph {
            function: self.function,
            parameters: self.parameters,
            returns: self.returns,
            entry: self.entry,
            exit: self.exit,
            nodes: self.nodes,
            edges: self.edges,
            move_states_before,
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
        match &stmt.kind {
            StmtKind::Let { name, ty, expr, .. } => {
                let binding = self.linear_node(
                    ControlFlowNodeKind::Binding {
                        name: name.clone(),
                        ty: ty.clone(),
                        mutable: false,
                    },
                    stmt.span,
                    successor,
                    self.binding_move_ownership(name, ty, expr),
                );
                self.evaluation_node(ControlFlowEvaluationKind::BindingInitializer, expr, binding)
            }
            StmtKind::Var { name, ty, expr, .. } => {
                let binding = self.linear_node(
                    ControlFlowNodeKind::Binding {
                        name: name.clone(),
                        ty: ty.clone(),
                        mutable: true,
                    },
                    stmt.span,
                    successor,
                    ControlFlowOwnership::default(),
                );
                self.evaluation_node(ControlFlowEvaluationKind::BindingInitializer, expr, binding)
            }
            StmtKind::Assign { name, expr, .. } => {
                let assignment = self.linear_node(
                    ControlFlowNodeKind::Assignment { name: name.clone() },
                    stmt.span,
                    successor,
                    ControlFlowOwnership::default(),
                );
                self.evaluation_node(ControlFlowEvaluationKind::AssignmentValue, expr, assignment)
            }
            StmtKind::LetDestructure {
                expr, else_return, ..
            }
            | StmtKind::LetMultiDestructure {
                expr, else_return, ..
            } => {
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
                self.evaluation_node(ControlFlowEvaluationKind::DestructureValue, expr, node)
            }
            StmtKind::LetListDestructure { expr, .. }
            | StmtKind::LetStructDestructure { expr, .. } => {
                let node = self.linear_node(
                    ControlFlowNodeKind::Statement,
                    stmt.span,
                    successor,
                    ControlFlowOwnership::default(),
                );
                self.evaluation_node(ControlFlowEvaluationKind::DestructureValue, expr, node)
            }
            StmtKind::Return(values) => {
                let node = self.node(ControlFlowNodeKind::Return, stmt.span);
                self.edge(node, self.exit, ControlFlowEdgeKind::Return);
                let mut entry = node;
                for (index, value) in values.iter().enumerate().rev() {
                    entry = self.evaluation_node(
                        ControlFlowEvaluationKind::ReturnValue(index),
                        value,
                        entry,
                    );
                }
                entry
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
                cond,
                body,
                else_body,
                ..
            } => {
                let node = self.node(ControlFlowNodeKind::Conditional, stmt.span);
                let then_entry = self.build_block(body, successor, loop_targets);
                let else_entry = self.build_block(else_body, successor, loop_targets);
                match typecheck::constant_primitive_value(cond, self.signatures) {
                    Some(typecheck::ConstantValue::Bool(true)) => {
                        self.edge(node, then_entry, ControlFlowEdgeKind::True);
                    }
                    Some(typecheck::ConstantValue::Bool(false)) => {
                        self.edge(node, else_entry, ControlFlowEdgeKind::False);
                    }
                    _ => {
                        self.edge(node, then_entry, ControlFlowEdgeKind::True);
                        self.edge(node, else_entry, ControlFlowEdgeKind::False);
                    }
                }
                self.evaluation_node(ControlFlowEvaluationKind::Condition, cond, node)
            }
            StmtKind::ForRange {
                start,
                end,
                inclusive,
                body,
                ..
            } => {
                let node = self.node(ControlFlowNodeKind::Loop, stmt.span);
                let body_entry = self.build_block(
                    body,
                    node,
                    Some(LoopTargets {
                        break_target: successor,
                        continue_target: node,
                    }),
                );
                let statically_empty = match (
                    typecheck::constant_primitive_value(start, self.signatures),
                    typecheck::constant_primitive_value(end, self.signatures),
                ) {
                    (
                        Some(typecheck::ConstantValue::I64(start_value)),
                        Some(typecheck::ConstantValue::I64(end_value)),
                    ) => {
                        if *inclusive {
                            start_value > end_value
                        } else {
                            start_value >= end_value
                        }
                    }
                    _ => false,
                };
                if !statically_empty {
                    self.edge(node, body_entry, ControlFlowEdgeKind::True);
                }
                self.edge(node, successor, ControlFlowEdgeKind::False);
                let end_entry =
                    self.evaluation_node(ControlFlowEvaluationKind::RangeEnd, end, node);
                self.evaluation_node(ControlFlowEvaluationKind::RangeStart, start, end_entry)
            }
            StmtKind::ForEach { iterable, body, .. } => {
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
                self.evaluation_node(ControlFlowEvaluationKind::Iterable, iterable, node)
            }
            StmtKind::While { cond, body } => {
                let node = self.node(ControlFlowNodeKind::Loop, stmt.span);
                let condition = self.node_with_ownership(
                    ControlFlowNodeKind::Evaluation(ControlFlowEvaluationKind::Condition),
                    cond.span,
                    self.ownership_for_expr(cond),
                );
                self.edge(condition, node, ControlFlowEdgeKind::Next);
                let body_entry = self.build_block(
                    body,
                    condition,
                    Some(LoopTargets {
                        break_target: successor,
                        continue_target: condition,
                    }),
                );
                match typecheck::constant_primitive_value(cond, self.signatures) {
                    Some(typecheck::ConstantValue::Bool(true)) => {
                        self.edge(node, body_entry, ControlFlowEdgeKind::True);
                    }
                    Some(typecheck::ConstantValue::Bool(false)) => {
                        self.edge(node, successor, ControlFlowEdgeKind::False);
                    }
                    _ => {
                        self.edge(node, body_entry, ControlFlowEdgeKind::True);
                        self.edge(node, successor, ControlFlowEdgeKind::False);
                    }
                }
                condition
            }
            StmtKind::Match { value, arms } => {
                let node = self.node(ControlFlowNodeKind::Match, stmt.span);
                for (index, arm) in arms.iter().enumerate() {
                    let arm_entry = self.build_block(&arm.body, successor, loop_targets);
                    let dispatch_target = if let Some(guard) = &arm.guard {
                        let guard_node = self.node_with_ownership(
                            ControlFlowNodeKind::Evaluation(ControlFlowEvaluationKind::MatchGuard(
                                index,
                            )),
                            guard.span,
                            self.ownership_for_expr(guard),
                        );
                        self.edge(guard_node, arm_entry, ControlFlowEdgeKind::GuardTrue);
                        self.edge(guard_node, successor, ControlFlowEdgeKind::GuardFalse);
                        guard_node
                    } else {
                        arm_entry
                    };
                    self.edge(node, dispatch_target, ControlFlowEdgeKind::MatchArm(index));
                }
                self.evaluation_node(ControlFlowEvaluationKind::MatchValue, value, node)
            }
            StmtKind::ListMatch { value, arms } => {
                let node = self.node(ControlFlowNodeKind::Match, stmt.span);
                for (index, arm) in arms.iter().enumerate() {
                    let arm_entry = self.build_block(&arm.body, successor, loop_targets);
                    let dispatch_target = if let Some(guard) = &arm.guard {
                        let guard_node = self.node_with_ownership(
                            ControlFlowNodeKind::Evaluation(ControlFlowEvaluationKind::MatchGuard(
                                index,
                            )),
                            guard.span,
                            self.ownership_for_expr(guard),
                        );
                        self.edge(guard_node, arm_entry, ControlFlowEdgeKind::GuardTrue);
                        self.edge(guard_node, successor, ControlFlowEdgeKind::GuardFalse);
                        guard_node
                    } else {
                        arm_entry
                    };
                    self.edge(node, dispatch_target, ControlFlowEdgeKind::MatchArm(index));
                }
                self.evaluation_node(ControlFlowEvaluationKind::MatchValue, value, node)
            }
            StmtKind::Expr(expr) => {
                let node = self.linear_node(
                    ControlFlowNodeKind::Statement,
                    stmt.span,
                    successor,
                    ControlFlowOwnership::default(),
                );
                self.evaluation_node(ControlFlowEvaluationKind::ExpressionStatement, expr, node)
            }
            StmtKind::Shell { expr, redirect, .. } => {
                let node = self.linear_node(
                    ControlFlowNodeKind::Statement,
                    stmt.span,
                    successor,
                    ControlFlowOwnership::default(),
                );
                let redirect_entry = redirect.as_ref().map_or(node, |redirect| {
                    self.evaluation_node(
                        ControlFlowEvaluationKind::RedirectPath,
                        &redirect.path,
                        node,
                    )
                });
                self.evaluation_node(ControlFlowEvaluationKind::ShellValue, expr, redirect_entry)
            }
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

    fn evaluation_node(
        &mut self,
        kind: ControlFlowEvaluationKind,
        expr: &Expr,
        successor: ControlFlowNodeId,
    ) -> ControlFlowNodeId {
        let node = self.node_with_ownership(
            ControlFlowNodeKind::Evaluation(kind),
            expr.span,
            self.ownership_for_expr(expr),
        );
        self.edge(node, successor, ControlFlowEdgeKind::Next);
        node
    }

    fn ownership_for_expr(&self, expr: &Expr) -> ControlFlowOwnership {
        let mut reads = HashSet::new();
        typecheck::collect_expr_reads(expr, &mut reads);
        let mut reads = reads.into_iter().collect::<Vec<_>>();
        reads.sort();
        ControlFlowOwnership {
            reads,
            moves: Vec::new(),
        }
    }

    fn binding_move_ownership(&self, name: &str, ty: &Type, expr: &Expr) -> ControlFlowOwnership {
        let moves = if !self.signatures.is_copy_type(ty)
            && let ExprKind::Var(source) = &expr.kind
        {
            vec![OwnershipMove {
                source: source.clone(),
                destination: name.to_string(),
                span: expr.span,
            }]
        } else {
            Vec::new()
        };
        ControlFlowOwnership {
            reads: Vec::new(),
            moves,
        }
    }
}

fn compute_move_states(
    nodes: &[ControlFlowNode],
    edges: &[ControlFlowEdge],
    entry: ControlFlowNodeId,
) -> Vec<ControlFlowMoveState> {
    let mut states = vec![None::<BTreeMap<String, SourceSpan>>; nodes.len()];
    states[entry.0] = Some(BTreeMap::new());
    let mut queue = VecDeque::from([entry]);

    while let Some(id) = queue.pop_front() {
        let Some(mut outgoing_state) = states[id.0].clone() else {
            continue;
        };
        for movement in &nodes[id.0].ownership.moves {
            outgoing_state
                .entry(movement.source.clone())
                .and_modify(|origin| {
                    if span_key(movement.span) < span_key(*origin) {
                        *origin = movement.span;
                    }
                })
                .or_insert(movement.span);
        }

        for edge in edges.iter().filter(|edge| edge.from == id) {
            let target = edge.to.0;
            let changed = match states[target].as_mut() {
                Some(existing) => merge_move_state(existing, &outgoing_state),
                None => {
                    states[target] = Some(outgoing_state.clone());
                    true
                }
            };
            if changed {
                queue.push_back(edge.to);
            }
        }
    }

    states
        .into_iter()
        .map(|state| match state {
            Some(moved) => ControlFlowMoveState {
                reachable: true,
                moved: moved
                    .into_iter()
                    .map(|(name, origin)| OwnershipMovedBinding { name, origin })
                    .collect(),
            },
            None => ControlFlowMoveState::default(),
        })
        .collect()
}

fn merge_move_state(
    target: &mut BTreeMap<String, SourceSpan>,
    incoming: &BTreeMap<String, SourceSpan>,
) -> bool {
    let mut changed = false;
    for (name, origin) in incoming {
        match target.get_mut(name) {
            Some(existing) if span_key(*origin) < span_key(*existing) => {
                *existing = *origin;
                changed = true;
            }
            Some(_) => {}
            None => {
                target.insert(name.clone(), *origin);
                changed = true;
            }
        }
    }
    changed
}

fn span_key(span: SourceSpan) -> (u32, usize, usize, usize) {
    (span.source_id.value(), span.line, span.column, span.length)
}
