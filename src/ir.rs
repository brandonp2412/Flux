use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use crate::ast::{BinOp, Expr, ExprKind, Function, Stmt, StmtKind, Type, UnaryOp};
use crate::diagnostic::SourceSpan;
use crate::typecheck::{self, ConstantValue, Signatures};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ControlFlowNodeId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ControlFlowValueId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ControlFlowValueRegionId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ControlFlowDefinitionId {
    Parameter(usize),
    Node {
        node: ControlFlowNodeId,
        index: usize,
    },
    Scoped {
        node: ControlFlowNodeId,
        index: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlFlowValueKind {
    Literal,
    NameRead {
        name: String,
        definitions: Vec<ControlFlowDefinitionId>,
    },
    AnonymousFunction {
        body: ControlFlowValueId,
    },
    Call {
        callee: String,
        arguments: Vec<ControlFlowValueId>,
    },
    OptionalCascadeCall {
        optional: ControlFlowValueId,
        callee: String,
        arguments: Vec<ControlFlowValueId>,
    },
    InterfacePack {
        interface: String,
        target: String,
        value: ControlFlowValueId,
    },
    List {
        items: Vec<ControlFlowValueId>,
    },
    ListSpread {
        value: ControlFlowValueId,
    },
    ListOptional {
        value: ControlFlowValueId,
    },
    ListIf {
        condition: ControlFlowValueId,
        value: ControlFlowValueId,
        else_value: Option<ControlFlowValueId>,
    },
    Index {
        base: ControlFlowValueId,
        index: ControlFlowValueId,
    },
    Slice {
        base: ControlFlowValueId,
        start: Option<ControlFlowValueId>,
        end: Option<ControlFlowValueId>,
        step: Option<ControlFlowValueId>,
    },
    ListComprehension {
        iterable: ControlFlowValueId,
        value: ControlFlowValueId,
        condition: Option<ControlFlowValueId>,
    },
    StructLiteral {
        name: String,
        base: Option<ControlFlowValueId>,
        fields: Vec<(String, ControlFlowValueId)>,
    },
    QualifiedCall {
        namespace: String,
        name: String,
        arguments: Vec<ControlFlowValueId>,
    },
    InterfaceDispatch {
        interface: String,
        capability: String,
        target: Option<String>,
        mapped_function: Option<String>,
        arguments: Vec<ControlFlowValueId>,
    },
    Field {
        base: ControlFlowValueId,
        name: String,
    },
    Match {
        value: ControlFlowValueId,
        guards: Vec<Option<ControlFlowValueId>>,
        arms: Vec<ControlFlowValueId>,
    },
    ListMatch {
        value: ControlFlowValueId,
        guards: Vec<Option<ControlFlowValueId>>,
        arms: Vec<ControlFlowValueId>,
    },
    Conditional {
        condition: ControlFlowValueId,
        then_value: ControlFlowValueId,
        else_value: ControlFlowValueId,
    },
    Unary {
        op: UnaryOp,
        operand: ControlFlowValueId,
    },
    Binary {
        op: BinOp,
        left: ControlFlowValueId,
        right: ControlFlowValueId,
    },
    Opaque,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFlowValue {
    pub id: ControlFlowValueId,
    pub producer: ControlFlowNodeId,
    pub result_index: Option<usize>,
    pub ty: Type,
    pub span: SourceSpan,
    pub kind: ControlFlowValueKind,
    pub source_constant: Option<ConstantValue>,
    pub constant: Option<ConstantValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ControlFlowValueUseKind {
    Eager,
    ShortCircuitRight,
    OptionalPresent,
    BranchCondition,
    BranchThen,
    BranchElse,
    MatchValue,
    MatchGuard(usize),
    MatchArm(usize),
    LoopIterable,
    LoopCondition,
    LoopBody,
    DeferredBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlFlowValueUse {
    pub user: ControlFlowValueId,
    pub value: ControlFlowValueId,
    pub kind: ControlFlowValueUseKind,
    pub region: Option<ControlFlowValueRegionId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlowValueRegionKind {
    ShortCircuitRight {
        condition: ControlFlowValueId,
        execute_when: bool,
    },
    OptionalFallback {
        optional: ControlFlowValueId,
    },
    OptionalPresent {
        optional: ControlFlowValueId,
    },
    Branch {
        condition: ControlFlowValueId,
        selected_when: bool,
    },
    MatchGuard {
        matched: ControlFlowValueId,
        arm: usize,
    },
    MatchArm {
        matched: ControlFlowValueId,
        guard: Option<ControlFlowValueId>,
        arm: usize,
    },
    LoopCondition {
        iterable: ControlFlowValueId,
    },
    LoopBody {
        iterable: ControlFlowValueId,
        condition: Option<ControlFlowValueId>,
    },
    DeferredBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlFlowValueRegion {
    pub id: ControlFlowValueRegionId,
    pub owner: ControlFlowValueId,
    pub root: ControlFlowValueId,
    pub span: SourceSpan,
    pub kind: ControlFlowValueRegionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFlowParameter {
    pub name: String,
    pub ty: Type,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFlowDefinition {
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
    PatternBindings,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipBorrowKind {
    Immutable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipBorrow {
    pub source: String,
    pub kind: OwnershipBorrowKind,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlFlowOwnership {
    pub reads: Vec<String>,
    pub borrows: Vec<OwnershipBorrow>,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlFlowLiveState {
    live: Vec<String>,
}

impl ControlFlowLiveState {
    pub fn live(&self) -> &[String] {
        &self.live
    }

    pub fn contains(&self, name: &str) -> bool {
        self.live.iter().any(|binding| binding == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFlowNode {
    pub id: ControlFlowNodeId,
    pub kind: ControlFlowNodeKind,
    pub span: SourceSpan,
    pub value_types: Vec<Type>,
    pub values: Vec<ControlFlowValueId>,
    pub definitions: Vec<ControlFlowDefinition>,
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
    values: Vec<ControlFlowValue>,
    value_uses: Vec<ControlFlowValueUse>,
    value_regions: Vec<ControlFlowValueRegion>,
    reachable_values: BTreeSet<ControlFlowValueId>,
    scoped_definitions: Vec<Vec<ControlFlowDefinition>>,
    definition_values: BTreeMap<ControlFlowDefinitionId, ControlFlowValueId>,
    scoped_borrow_sources: BTreeMap<ControlFlowDefinitionId, ControlFlowValueId>,
    reaching_definitions_before: Vec<Option<ReachingDefinitionMap>>,
    move_states_before: Vec<ControlFlowMoveState>,
    live_before: Vec<ControlFlowLiveState>,
    live_after: Vec<ControlFlowLiveState>,
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

    pub fn values(&self) -> &[ControlFlowValue] {
        &self.values
    }

    pub fn value(&self, id: ControlFlowValueId) -> Option<&ControlFlowValue> {
        self.values.get(id.0)
    }

    pub fn value_uses(&self) -> &[ControlFlowValueUse] {
        &self.value_uses
    }

    pub fn value_regions(&self) -> &[ControlFlowValueRegion] {
        &self.value_regions
    }

    pub fn value_region(&self, id: ControlFlowValueRegionId) -> Option<&ControlFlowValueRegion> {
        self.value_regions.get(id.0)
    }

    pub fn regions_owned_by(
        &self,
        id: ControlFlowValueId,
    ) -> impl Iterator<Item = &ControlFlowValueRegion> {
        self.value_regions
            .iter()
            .filter(move |region| region.owner == id)
    }

    pub fn uses_from(&self, id: ControlFlowValueId) -> impl Iterator<Item = &ControlFlowValueUse> {
        self.value_uses.iter().filter(move |usage| usage.user == id)
    }

    pub fn uses_of(&self, id: ControlFlowValueId) -> impl Iterator<Item = &ControlFlowValueUse> {
        self.value_uses
            .iter()
            .filter(move |usage| usage.value == id)
    }

    pub fn is_value_reachable(&self, id: ControlFlowValueId) -> bool {
        self.reachable_values.contains(&id)
    }

    pub fn definition_name(&self, id: ControlFlowDefinitionId) -> Option<&str> {
        match id {
            ControlFlowDefinitionId::Parameter(index) => self
                .parameters
                .get(index)
                .map(|parameter| parameter.name.as_str()),
            ControlFlowDefinitionId::Node { node, index } => self
                .nodes
                .get(node.0)
                .and_then(|node| node.definitions.get(index))
                .map(|definition| definition.name.as_str()),
            ControlFlowDefinitionId::Scoped { node, index } => self
                .scoped_definitions
                .get(node.0)
                .and_then(|definitions| definitions.get(index))
                .map(|definition| definition.name.as_str()),
        }
    }

    pub fn definition_value(&self, id: ControlFlowDefinitionId) -> Option<ControlFlowValueId> {
        self.definition_values.get(&id).copied()
    }

    pub fn definition_borrow_source_value(
        &self,
        id: ControlFlowDefinitionId,
    ) -> Option<ControlFlowValueId> {
        definition_borrow_source_value_from_parts(
            &self.nodes,
            &self.edges,
            &self.scoped_borrow_sources,
            id,
        )
    }

    pub fn definition_span(&self, id: ControlFlowDefinitionId) -> Option<SourceSpan> {
        match id {
            ControlFlowDefinitionId::Parameter(index) => {
                self.parameters.get(index).map(|parameter| parameter.span)
            }
            ControlFlowDefinitionId::Node { node, index } => self
                .nodes
                .get(node.0)
                .and_then(|node| node.definitions.get(index))
                .map(|definition| definition.span),
            ControlFlowDefinitionId::Scoped { node, index } => self
                .scoped_definitions
                .get(node.0)
                .and_then(|definitions| definitions.get(index))
                .map(|definition| definition.span),
        }
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

    pub fn live_before(&self, id: ControlFlowNodeId) -> Option<&ControlFlowLiveState> {
        self.live_before.get(id.0)
    }

    pub fn live_after(&self, id: ControlFlowNodeId) -> Option<&ControlFlowLiveState> {
        self.live_after.get(id.0)
    }

    pub fn is_reachable(&self, id: ControlFlowNodeId) -> bool {
        self.move_state_before(id)
            .is_some_and(ControlFlowMoveState::reachable)
    }

    pub fn borrowed_definition_span(&self, name: &str, source: &str) -> Option<SourceSpan> {
        let mut visiting = HashSet::new();
        self.borrowed_definition_span_inner(name, source, &mut visiting)
    }

    pub fn borrowed_reaching_definition_span(
        &self,
        node: ControlFlowNodeId,
        name: &str,
        source: &str,
    ) -> Option<SourceSpan> {
        let definitions = self
            .reaching_definitions_before
            .get(node.0)?
            .as_ref()?
            .get(name)?;
        let mut visiting = HashSet::new();
        definitions.iter().find_map(|definition| {
            self.definition_id_borrows_from(*definition, source, &mut visiting)
                .then(|| self.definition_span(*definition))
                .flatten()
        })
    }

    fn borrowed_definition_span_inner(
        &self,
        name: &str,
        source: &str,
        visiting: &mut HashSet<String>,
    ) -> Option<SourceSpan> {
        if !visiting.insert(name.to_string()) {
            return None;
        }
        let result = self.nodes.iter().find_map(|node| {
            node.definitions
                .iter()
                .enumerate()
                .find_map(|(index, definition)| {
                    if definition.name != name || !matches!(definition.ty, Type::List(_)) {
                        return None;
                    }
                    let id = ControlFlowDefinitionId::Node {
                        node: node.id,
                        index,
                    };
                    let borrows_source = if let Some(value) = self.definition_value(id) {
                        self.definition_value_borrows_from(value, source, visiting)
                    } else if let Some(value) = self.definition_borrow_source_value(id) {
                        self.value_depends_on_borrow_source(value, source, visiting)
                    } else {
                        false
                    };
                    borrows_source.then_some(definition.span)
                })
        });
        visiting.remove(name);
        result
    }

    fn definition_id_borrows_from(
        &self,
        definition: ControlFlowDefinitionId,
        source: &str,
        visiting: &mut HashSet<String>,
    ) -> bool {
        self.definition_borrow_source_value(definition)
            .is_some_and(|value| self.value_depends_on_borrow_source(value, source, visiting))
            || self
                .definition_value(definition)
                .is_some_and(|value| self.definition_value_borrows_from(value, source, visiting))
    }

    fn definition_value_borrows_from(
        &self,
        value: ControlFlowValueId,
        source: &str,
        visiting: &mut HashSet<String>,
    ) -> bool {
        if !self.is_value_reachable(value) {
            return false;
        }
        let Some(value) = self.value(value) else {
            return false;
        };
        match &value.kind {
            ControlFlowValueKind::List { items } if matches!(&value.ty, Type::List(element) if matches!(element.as_ref(), Type::List(_))) => {
                items
                    .iter()
                    .any(|item| self.value_depends_on_borrow_source(*item, source, visiting))
            }
            ControlFlowValueKind::ListSpread { value } => {
                self.value_depends_on_borrow_source(*value, source, visiting)
            }
            ControlFlowValueKind::ListIf {
                value, else_value, ..
            } => {
                self.value_depends_on_borrow_source(*value, source, visiting)
                    || else_value.is_some_and(|value| {
                        self.value_depends_on_borrow_source(value, source, visiting)
                    })
            }
            ControlFlowValueKind::ListComprehension { value: body, .. } if matches!(&value.ty, Type::List(element) if matches!(element.as_ref(), Type::List(_))) => {
                self.value_depends_on_borrow_source(*body, source, visiting)
            }
            ControlFlowValueKind::Slice { base, .. } => {
                self.value_depends_on_borrow_source(*base, source, visiting)
            }
            ControlFlowValueKind::Index { base, .. } if matches!(value.ty, Type::List(_)) => {
                self.value_depends_on_borrow_source(*base, source, visiting)
            }
            ControlFlowValueKind::Field { base, .. } if matches!(value.ty, Type::List(_)) => {
                self.value_depends_on_borrow_source(*base, source, visiting)
            }
            ControlFlowValueKind::Call { callee, arguments }
                if matches!(callee.as_str(), "take" | "skip" | "chunked") =>
            {
                arguments.first().is_some_and(|argument| {
                    self.value_depends_on_borrow_source(*argument, source, visiting)
                })
            }
            ControlFlowValueKind::Call { callee, arguments }
                if matches!(&value.ty, Type::List(element) if matches!(element.as_ref(), Type::List(_)))
                    && matches!(callee.as_str(), "filter" | "where" | "flatten" | "concat") =>
            {
                let retained = if callee == "concat" {
                    arguments.iter().take(2)
                } else {
                    arguments.iter().take(1)
                };
                retained.into_iter().any(|argument| {
                    self.value_depends_on_borrow_source(*argument, source, visiting)
                })
            }
            ControlFlowValueKind::Match { arms, .. }
            | ControlFlowValueKind::ListMatch { arms, .. } => arms
                .iter()
                .any(|arm| self.value_depends_on_borrow_source(*arm, source, visiting)),
            ControlFlowValueKind::Conditional {
                then_value,
                else_value,
                ..
            } => {
                self.value_depends_on_borrow_source(*then_value, source, visiting)
                    || self.value_depends_on_borrow_source(*else_value, source, visiting)
            }
            ControlFlowValueKind::NameRead { definitions, .. } => definitions
                .iter()
                .any(|definition| self.definition_id_borrows_from(*definition, source, visiting)),
            _ => false,
        }
    }

    fn value_depends_on_borrow_source(
        &self,
        value: ControlFlowValueId,
        source: &str,
        visiting: &mut HashSet<String>,
    ) -> bool {
        if !self.is_value_reachable(value) {
            return false;
        }
        let Some(value) = self.value(value) else {
            return false;
        };
        match &value.kind {
            ControlFlowValueKind::List { items } if matches!(&value.ty, Type::List(element) if matches!(element.as_ref(), Type::List(_))) => {
                items
                    .iter()
                    .any(|item| self.value_depends_on_borrow_source(*item, source, visiting))
            }
            ControlFlowValueKind::ListSpread { value } => {
                self.value_depends_on_borrow_source(*value, source, visiting)
            }
            ControlFlowValueKind::ListIf {
                value, else_value, ..
            } => {
                self.value_depends_on_borrow_source(*value, source, visiting)
                    || else_value.is_some_and(|value| {
                        self.value_depends_on_borrow_source(value, source, visiting)
                    })
            }
            ControlFlowValueKind::ListComprehension { value: body, .. } if matches!(&value.ty, Type::List(element) if matches!(element.as_ref(), Type::List(_))) => {
                self.value_depends_on_borrow_source(*body, source, visiting)
            }
            ControlFlowValueKind::NameRead { name, definitions } => {
                name == source
                    || definitions.iter().any(|definition| {
                        self.definition_id_borrows_from(*definition, source, visiting)
                    })
            }
            ControlFlowValueKind::Slice { base, .. } => {
                self.value_depends_on_borrow_source(*base, source, visiting)
            }
            ControlFlowValueKind::Index { base, .. } if matches!(value.ty, Type::List(_)) => {
                self.value_depends_on_borrow_source(*base, source, visiting)
            }
            ControlFlowValueKind::Field { base, .. } if matches!(value.ty, Type::List(_)) => {
                self.value_depends_on_borrow_source(*base, source, visiting)
            }
            ControlFlowValueKind::Call { callee, arguments }
                if matches!(callee.as_str(), "take" | "skip" | "chunked") =>
            {
                arguments.first().is_some_and(|argument| {
                    self.value_depends_on_borrow_source(*argument, source, visiting)
                })
            }
            ControlFlowValueKind::Call { callee, arguments }
                if matches!(&value.ty, Type::List(element) if matches!(element.as_ref(), Type::List(_)))
                    && matches!(callee.as_str(), "filter" | "where" | "flatten" | "concat") =>
            {
                let retained = if callee == "concat" {
                    arguments.iter().take(2)
                } else {
                    arguments.iter().take(1)
                };
                retained.into_iter().any(|argument| {
                    self.value_depends_on_borrow_source(*argument, source, visiting)
                })
            }
            ControlFlowValueKind::Match { arms, .. }
            | ControlFlowValueKind::ListMatch { arms, .. } => arms
                .iter()
                .any(|arm| self.value_depends_on_borrow_source(*arm, source, visiting)),
            ControlFlowValueKind::Conditional {
                then_value,
                else_value,
                ..
            } => {
                self.value_depends_on_borrow_source(*then_value, source, visiting)
                    || self.value_depends_on_borrow_source(*else_value, source, visiting)
            }
            _ => false,
        }
    }
}

fn definition_borrow_source_value_from_parts(
    nodes: &[ControlFlowNode],
    edges: &[ControlFlowEdge],
    scoped_borrow_sources: &BTreeMap<ControlFlowDefinitionId, ControlFlowValueId>,
    id: ControlFlowDefinitionId,
) -> Option<ControlFlowValueId> {
    if let Some(value) = scoped_borrow_sources.get(&id) {
        return Some(*value);
    }
    let ControlFlowDefinitionId::Node { node, index } = id else {
        return None;
    };
    let definition = nodes.get(node.0)?.definitions.get(index)?;
    if !matches!(definition.ty, Type::List(_)) {
        return None;
    }
    let source_node = match nodes.get(node.0)?.kind {
        ControlFlowNodeKind::Destructure { .. } | ControlFlowNodeKind::Loop => node,
        ControlFlowNodeKind::PatternBindings => {
            edges
                .iter()
                .find(|edge| {
                    edge.to == node && matches!(edge.kind, ControlFlowEdgeKind::MatchArm(_))
                })?
                .from
        }
        _ => return None,
    };
    edges
        .iter()
        .filter(|edge| edge.to == source_node && edge.kind == ControlFlowEdgeKind::Next)
        .find_map(|edge| {
            let source = nodes.get(edge.from.0)?;
            matches!(
                source.kind,
                ControlFlowNodeKind::Evaluation(
                    ControlFlowEvaluationKind::DestructureValue
                        | ControlFlowEvaluationKind::AssignmentValue
                        | ControlFlowEvaluationKind::MatchValue
                        | ControlFlowEvaluationKind::Iterable
                )
            )
            .then(|| source.values.first().copied())
            .flatten()
        })
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
    values: Vec<ControlFlowValue>,
    scoped_definitions: Vec<Vec<ControlFlowDefinition>>,
    scoped_definition_stack: Vec<HashMap<String, ControlFlowDefinitionId>>,
    scoped_borrow_sources: BTreeMap<ControlFlowDefinitionId, ControlFlowValueId>,
    evaluation_types: Vec<(SourceSpan, Vec<Type>)>,
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
            values: Vec::new(),
            scoped_definitions: Vec::new(),
            scoped_definition_stack: Vec::new(),
            scoped_borrow_sources: BTreeMap::new(),
            evaluation_types: collect_evaluation_types(function, signatures),
        };
        builder.entry = builder.node(ControlFlowNodeKind::Entry, function.keyword_span);
        builder.exit = builder.node(ControlFlowNodeKind::Exit, function.return_span);
        builder
    }

    fn finish(mut self) -> ControlFlowGraph {
        let definition_values = compute_definition_values(&self.nodes, &self.edges);
        let reaching_definitions_before = loop {
            let reaching_definitions = compute_reaching_definitions(
                &self.nodes,
                &self.edges,
                &self.parameters,
                self.entry,
            );
            resolve_name_read_definitions(&mut self.values, &reaching_definitions);
            propagate_definition_constants(&mut self.values, &definition_values);
            if !prune_constant_control_edges(&self.nodes, &mut self.edges, &self.values) {
                break reaching_definitions;
            }
        };
        reclassify_borrowed_list_reborrows(
            &mut self.nodes,
            &self.edges,
            &self.values,
            &definition_values,
            &self.scoped_borrow_sources,
            &self.parameters,
            self.signatures,
        );
        let move_states_before = compute_move_states(&self.nodes, &self.edges, self.entry);
        let (live_before, live_after) =
            compute_liveness(&self.nodes, &self.edges, &self.parameters);
        let (value_uses, value_regions) = collect_value_uses(&self.values);
        let reachable_values = compute_reachable_values(
            &self.values,
            &value_uses,
            &value_regions,
            &move_states_before,
        );
        populate_immutable_borrows(
            &mut self.nodes,
            &self.values,
            &reachable_values,
            self.signatures,
        );
        ControlFlowGraph {
            function: self.function,
            parameters: self.parameters,
            returns: self.returns,
            entry: self.entry,
            exit: self.exit,
            nodes: self.nodes,
            edges: self.edges,
            values: self.values,
            value_uses,
            value_regions,
            reachable_values,
            scoped_definitions: self.scoped_definitions,
            definition_values,
            scoped_borrow_sources: self.scoped_borrow_sources,
            reaching_definitions_before,
            move_states_before,
            live_before,
            live_after,
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
            value_types: Vec::new(),
            values: Vec::new(),
            definitions: Vec::new(),
            ownership,
        });
        id
    }

    fn set_definitions(&mut self, id: ControlFlowNodeId, definitions: Vec<ControlFlowDefinition>) {
        self.nodes[id.0].definitions = definitions;
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
            StmtKind::Let {
                name,
                name_span,
                ty,
                expr,
                ..
            } => {
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
                self.set_definitions(binding, vec![self.definition(name, ty, *name_span)]);
                self.evaluation_node(ControlFlowEvaluationKind::BindingInitializer, expr, binding)
            }
            StmtKind::Var {
                name,
                name_span,
                ty,
                expr,
                ..
            } => {
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
                self.set_definitions(binding, vec![self.definition(name, ty, *name_span)]);
                self.evaluation_node(ControlFlowEvaluationKind::BindingInitializer, expr, binding)
            }
            StmtKind::Assign {
                name,
                name_span,
                expr,
                ..
            } => {
                let assignment = self.linear_node(
                    ControlFlowNodeKind::Assignment { name: name.clone() },
                    stmt.span,
                    successor,
                    ControlFlowOwnership::default(),
                );
                if let Some(ty) = self.scalar_expression_type(expr) {
                    self.set_definitions(
                        assignment,
                        vec![ControlFlowDefinition {
                            name: name.clone(),
                            ty,
                            span: *name_span,
                        }],
                    );
                }
                self.evaluation_node(ControlFlowEvaluationKind::AssignmentValue, expr, assignment)
            }
            StmtKind::AssignMultiDestructure { bindings, expr } => {
                let node = self.linear_node(
                    ControlFlowNodeKind::Destructure {
                        propagates_error: false,
                    },
                    stmt.span,
                    successor,
                    ControlFlowOwnership::default(),
                );
                let types = self.expression_types(expr);
                self.set_definitions(
                    node,
                    bindings
                        .iter()
                        .zip(types)
                        .filter(|(binding, _)| binding.name != "_")
                        .map(|(binding, ty)| ControlFlowDefinition {
                            name: binding.name.clone(),
                            ty,
                            span: binding.span,
                        })
                        .collect(),
                );
                self.evaluation_node(ControlFlowEvaluationKind::AssignmentValue, expr, node)
            }
            StmtKind::AssignListDestructure {
                bindings,
                rest,
                expr,
            } => {
                let node = self.linear_node(
                    ControlFlowNodeKind::Destructure {
                        propagates_error: false,
                    },
                    stmt.span,
                    successor,
                    ControlFlowOwnership::default(),
                );
                self.set_definitions(
                    node,
                    self.list_destructure_definitions(bindings, rest.as_ref(), expr),
                );
                self.evaluation_node(ControlFlowEvaluationKind::AssignmentValue, expr, node)
            }
            StmtKind::AssignStructDestructure {
                struct_name,
                fields,
                expr,
                ..
            } => {
                let node = self.linear_node(
                    ControlFlowNodeKind::Destructure {
                        propagates_error: false,
                    },
                    stmt.span,
                    successor,
                    ControlFlowOwnership::default(),
                );
                self.set_definitions(node, self.struct_pattern_definitions(fields, struct_name));
                self.evaluation_node(ControlFlowEvaluationKind::AssignmentValue, expr, node)
            }
            StmtKind::LetDestructure {
                bindings,
                expr,
                else_return,
                ..
            } => {
                let node = self.node(
                    ControlFlowNodeKind::Destructure {
                        propagates_error: *else_return,
                    },
                    stmt.span,
                );
                self.set_definitions(
                    node,
                    bindings
                        .iter()
                        .map(|binding| {
                            self.definition(&binding.name, &binding.ty, binding.name_span)
                        })
                        .collect(),
                );
                if *else_return {
                    self.edge(node, successor, ControlFlowEdgeKind::Success);
                    self.edge(node, self.exit, ControlFlowEdgeKind::Error);
                } else {
                    self.edge(node, successor, ControlFlowEdgeKind::Next);
                }
                self.evaluation_node(ControlFlowEvaluationKind::DestructureValue, expr, node)
            }
            StmtKind::LetMultiDestructure {
                bindings,
                expr,
                else_return,
                ..
            } => {
                let node = self.node(
                    ControlFlowNodeKind::Destructure {
                        propagates_error: *else_return,
                    },
                    stmt.span,
                );
                let types = self.expression_types(expr);
                self.set_definitions(
                    node,
                    bindings
                        .iter()
                        .zip(types)
                        .filter(|(binding, _)| binding.name != "_")
                        .map(|(binding, ty)| ControlFlowDefinition {
                            name: binding.name.clone(),
                            ty,
                            span: binding.span,
                        })
                        .collect(),
                );
                if *else_return {
                    self.edge(node, successor, ControlFlowEdgeKind::Success);
                    self.edge(node, self.exit, ControlFlowEdgeKind::Error);
                } else {
                    self.edge(node, successor, ControlFlowEdgeKind::Next);
                }
                self.evaluation_node(ControlFlowEvaluationKind::DestructureValue, expr, node)
            }
            StmtKind::LetListDestructure {
                bindings,
                rest,
                expr,
                ..
            } => {
                let node = self.linear_node(
                    ControlFlowNodeKind::Destructure {
                        propagates_error: false,
                    },
                    stmt.span,
                    successor,
                    ControlFlowOwnership::default(),
                );
                self.set_definitions(
                    node,
                    self.list_destructure_definitions(bindings, rest.as_ref(), expr),
                );
                self.evaluation_node(ControlFlowEvaluationKind::DestructureValue, expr, node)
            }
            StmtKind::LetStructDestructure {
                struct_name,
                fields,
                expr,
                ..
            } => {
                let node = self.linear_node(
                    ControlFlowNodeKind::Destructure {
                        propagates_error: false,
                    },
                    stmt.span,
                    successor,
                    ControlFlowOwnership::default(),
                );
                self.set_definitions(node, self.struct_pattern_definitions(fields, struct_name));
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
                binding,
                body,
                else_body,
                ..
            } => {
                let node = self.node(ControlFlowNodeKind::Conditional, stmt.span);
                let then_entry = self.build_block(body, successor, loop_targets);
                let then_entry = if let Some(binding) = binding {
                    let binding_node = self.linear_node(
                        ControlFlowNodeKind::PatternBindings,
                        binding.span,
                        then_entry,
                        ControlFlowOwnership::default(),
                    );
                    if binding.name != "_"
                        && let Some(Type::Optional(inner)) = self.scalar_expression_type(cond)
                    {
                        self.set_definitions(
                            binding_node,
                            vec![self.definition(&binding.name, &inner, binding.span)],
                        );
                    }
                    binding_node
                } else {
                    then_entry
                };
                let else_entry = self.build_block(else_body, successor, loop_targets);
                if binding.is_some() {
                    self.edge(node, then_entry, ControlFlowEdgeKind::True);
                    self.edge(node, else_entry, ControlFlowEdgeKind::False);
                } else {
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
                }
                self.evaluation_node(ControlFlowEvaluationKind::Condition, cond, node)
            }
            StmtKind::ForRange {
                name,
                name_span,
                start,
                end,
                inclusive,
                body,
            } => {
                let node = self.node(ControlFlowNodeKind::Loop, stmt.span);
                self.set_definitions(
                    node,
                    vec![ControlFlowDefinition {
                        name: name.clone(),
                        ty: Type::I64,
                        span: *name_span,
                    }],
                );
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
            StmtKind::ForEach {
                index_name,
                index_span,
                name,
                name_span,
                iterable,
                body,
            } => {
                let node = self.node(ControlFlowNodeKind::Loop, stmt.span);
                let mut definitions = Vec::new();
                if let Some(index_name) = index_name {
                    definitions.push(ControlFlowDefinition {
                        name: index_name.clone(),
                        ty: Type::I64,
                        span: index_span.unwrap_or(stmt.span),
                    });
                }
                if let Some(Type::List(element)) = self.scalar_expression_type(iterable) {
                    definitions.push(ControlFlowDefinition {
                        name: name.clone(),
                        ty: *element,
                        span: *name_span,
                    });
                }
                self.set_definitions(node, definitions);
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
                let condition =
                    self.raw_evaluation_node(ControlFlowEvaluationKind::Condition, cond);
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
                    let guard_entry = if let Some(guard) = &arm.guard {
                        let guard_node = self.raw_evaluation_node(
                            ControlFlowEvaluationKind::MatchGuard(index),
                            guard,
                        );
                        self.edge(guard_node, arm_entry, ControlFlowEdgeKind::GuardTrue);
                        self.edge(guard_node, successor, ControlFlowEdgeKind::GuardFalse);
                        guard_node
                    } else {
                        arm_entry
                    };
                    let bindings = self.node(ControlFlowNodeKind::PatternBindings, arm.span);
                    self.set_definitions(bindings, self.enum_match_definitions(value, arm));
                    self.edge(bindings, guard_entry, ControlFlowEdgeKind::Next);
                    self.edge(node, bindings, ControlFlowEdgeKind::MatchArm(index));
                }
                self.evaluation_node(ControlFlowEvaluationKind::MatchValue, value, node)
            }
            StmtKind::ListMatch { value, arms } => {
                let node = self.node(ControlFlowNodeKind::Match, stmt.span);
                for (index, arm) in arms.iter().enumerate() {
                    let arm_entry = self.build_block(&arm.body, successor, loop_targets);
                    let guard_entry = if let Some(guard) = &arm.guard {
                        let guard_node = self.raw_evaluation_node(
                            ControlFlowEvaluationKind::MatchGuard(index),
                            guard,
                        );
                        self.edge(guard_node, arm_entry, ControlFlowEdgeKind::GuardTrue);
                        self.edge(guard_node, successor, ControlFlowEdgeKind::GuardFalse);
                        guard_node
                    } else {
                        arm_entry
                    };
                    let bindings = self.node(ControlFlowNodeKind::PatternBindings, arm.span);
                    self.set_definitions(
                        bindings,
                        self.list_match_definitions(value, &arm.pattern),
                    );
                    self.edge(bindings, guard_entry, ControlFlowEdgeKind::Next);
                    self.edge(node, bindings, ControlFlowEdgeKind::MatchArm(index));
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
        let node = self.raw_evaluation_node(kind, expr);
        self.edge(node, successor, ControlFlowEdgeKind::Next);
        node
    }

    fn raw_evaluation_node(
        &mut self,
        kind: ControlFlowEvaluationKind,
        expr: &Expr,
    ) -> ControlFlowNodeId {
        let id = ControlFlowNodeId(self.nodes.len());
        let values = self.lower_expr_values(id, expr, true);
        let value_types = values
            .iter()
            .filter_map(|value| self.values.get(value.0))
            .map(|value| value.ty.clone())
            .collect();
        self.nodes.push(ControlFlowNode {
            id,
            kind: ControlFlowNodeKind::Evaluation(kind),
            span: expr.span,
            value_types,
            values,
            definitions: Vec::new(),
            ownership: self.ownership_for_expr(expr),
        });
        id
    }

    fn add_scoped_definition(
        &mut self,
        producer: ControlFlowNodeId,
        definition: ControlFlowDefinition,
    ) -> ControlFlowDefinitionId {
        if self.scoped_definitions.len() <= producer.0 {
            self.scoped_definitions
                .resize_with(producer.0 + 1, Vec::new);
        }
        let definitions = &mut self.scoped_definitions[producer.0];
        let index = definitions.len();
        definitions.push(definition);
        ControlFlowDefinitionId::Scoped {
            node: producer,
            index,
        }
    }

    fn add_scoped_definitions(
        &mut self,
        producer: ControlFlowNodeId,
        definitions: Vec<ControlFlowDefinition>,
    ) -> Vec<(String, ControlFlowDefinitionId)> {
        definitions
            .into_iter()
            .map(|definition| {
                let name = definition.name.clone();
                let id = self.add_scoped_definition(producer, definition);
                (name, id)
            })
            .collect()
    }

    fn record_scoped_borrow_sources(
        &mut self,
        definitions: &[(String, ControlFlowDefinitionId)],
        source: Option<ControlFlowValueId>,
    ) {
        let Some(source) = source else {
            return;
        };
        for (_, definition) in definitions {
            self.scoped_borrow_sources.insert(*definition, source);
        }
    }

    fn push_scoped_definitions(
        &mut self,
        definitions: impl IntoIterator<Item = (String, ControlFlowDefinitionId)>,
    ) {
        self.scoped_definition_stack
            .push(definitions.into_iter().collect());
    }

    fn scoped_definition_for(&self, name: &str) -> Option<ControlFlowDefinitionId> {
        self.scoped_definition_stack
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn lower_expr_values(
        &mut self,
        producer: ControlFlowNodeId,
        expr: &Expr,
        is_result: bool,
    ) -> Vec<ControlFlowValueId> {
        let kind = match &expr.kind {
            ExprKind::Int(_)
            | ExprKind::Bool(_)
            | ExprKind::Str(_)
            | ExprKind::Nil
            | ExprKind::None => ControlFlowValueKind::Literal,
            ExprKind::Var(name) => ControlFlowValueKind::NameRead {
                name: name.clone(),
                definitions: self.scoped_definition_for(name).into_iter().collect(),
            },
            ExprKind::AnonymousFunction { params, body, .. } => {
                let definitions = params
                    .iter()
                    .map(|param| {
                        let id = self.add_scoped_definition(
                            producer,
                            ControlFlowDefinition {
                                name: param.name.clone(),
                                ty: self.signatures.canonical_type(&param.ty),
                                span: param.name_span,
                            },
                        );
                        (param.name.clone(), id)
                    })
                    .collect::<Vec<_>>();
                self.push_scoped_definitions(definitions);
                let body = self.lower_scalar_expr(producer, body);
                self.scoped_definition_stack.pop();
                body.map_or(ControlFlowValueKind::Opaque, |body| {
                    ControlFlowValueKind::AnonymousFunction { body }
                })
            }
            ExprKind::Call {
                name,
                args,
                named_args,
            } => {
                let arguments = self.lower_call_arguments(producer, args, named_args);
                if self.signatures.interface(name).is_some()
                    && named_args.is_empty()
                    && arguments.len() == 1
                {
                    let value = arguments[0];
                    let target = self.values.get(value.0).and_then(|argument| {
                        let Type::Named(target) = self.signatures.canonical_type(&argument.ty)
                        else {
                            return None;
                        };
                        (target != *name && self.signatures.implementation(name, &target).is_some())
                            .then_some(target)
                    });
                    target.map_or(
                        ControlFlowValueKind::Call {
                            callee: name.clone(),
                            arguments,
                        },
                        |target| ControlFlowValueKind::InterfacePack {
                            interface: name.clone(),
                            target,
                            value,
                        },
                    )
                } else {
                    ControlFlowValueKind::Call {
                        callee: name.clone(),
                        arguments,
                    }
                }
            }
            ExprKind::ShellCall { name, args, .. } => ControlFlowValueKind::Call {
                callee: name.clone(),
                arguments: self.lower_expr_arguments(producer, args),
            },
            ExprKind::Pipe {
                input,
                name,
                args,
                optional,
                ..
            } => {
                if *optional {
                    let optional = self.lower_scalar_expr(producer, input);
                    let arguments = self.lower_expr_arguments(producer, args);
                    optional.map_or(ControlFlowValueKind::Opaque, |optional| {
                        ControlFlowValueKind::OptionalCascadeCall {
                            optional,
                            callee: name.clone(),
                            arguments,
                        }
                    })
                } else {
                    let mut arguments = self.lower_expr_values(producer, input, false);
                    arguments.extend(self.lower_expr_arguments(producer, args));
                    ControlFlowValueKind::Call {
                        callee: name.clone(),
                        arguments,
                    }
                }
            }
            ExprKind::List(items) => ControlFlowValueKind::List {
                items: self.lower_expr_arguments(producer, items),
            },
            ExprKind::ListSpread { value, .. } => self
                .lower_scalar_expr(producer, value)
                .map_or(ControlFlowValueKind::Opaque, |value| {
                    ControlFlowValueKind::ListSpread { value }
                }),
            ExprKind::ListOptional { value, .. } => self
                .lower_scalar_expr(producer, value)
                .map_or(ControlFlowValueKind::Opaque, |value| {
                    ControlFlowValueKind::ListOptional { value }
                }),
            ExprKind::ListIf {
                condition,
                binding,
                value,
                else_value,
                ..
            } => {
                let condition_value = self.lower_scalar_expr(producer, condition);
                let value = if let Some(binding) = binding {
                    let definitions = self
                        .scalar_expression_type(condition)
                        .and_then(|ty| match self.signatures.canonical_type(&ty) {
                            Type::Optional(inner)
                                if *inner != Type::Void && binding.name != "_" =>
                            {
                                Some(vec![self.definition(&binding.name, &inner, binding.span)])
                            }
                            _ => None,
                        })
                        .unwrap_or_default();
                    let definitions = self.add_scoped_definitions(producer, definitions);
                    self.push_scoped_definitions(definitions);
                    let lowered = self.lower_scalar_expr(producer, value);
                    self.scoped_definition_stack.pop();
                    lowered
                } else {
                    self.lower_scalar_expr(producer, value)
                };
                let else_value = else_value
                    .as_deref()
                    .and_then(|value| self.lower_scalar_expr(producer, value));
                match (condition_value, value) {
                    (Some(condition), Some(value)) => ControlFlowValueKind::ListIf {
                        condition,
                        value,
                        else_value,
                    },
                    _ => ControlFlowValueKind::Opaque,
                }
            }
            ExprKind::Index { base, index } => {
                let base = self.lower_scalar_expr(producer, base);
                let index = self.lower_scalar_expr(producer, index);
                match (base, index) {
                    (Some(base), Some(index)) => ControlFlowValueKind::Index { base, index },
                    _ => ControlFlowValueKind::Opaque,
                }
            }
            ExprKind::Slice {
                base,
                start,
                end,
                step,
            } => {
                let base = self.lower_scalar_expr(producer, base);
                let start = start
                    .as_deref()
                    .and_then(|value| self.lower_scalar_expr(producer, value));
                let end = end
                    .as_deref()
                    .and_then(|value| self.lower_scalar_expr(producer, value));
                let step = step
                    .as_deref()
                    .and_then(|value| self.lower_scalar_expr(producer, value));
                base.map_or(ControlFlowValueKind::Opaque, |base| {
                    ControlFlowValueKind::Slice {
                        base,
                        start,
                        end,
                        step,
                    }
                })
            }
            ExprKind::ListComprehension {
                value,
                binding,
                binding_span,
                iterable,
                condition,
            } => {
                let iterable_value = self.lower_scalar_expr(producer, iterable);
                let definitions = match self.scalar_expression_type(iterable) {
                    Some(Type::List(element)) => vec![ControlFlowDefinition {
                        name: binding.clone(),
                        ty: *element,
                        span: *binding_span,
                    }],
                    _ => Vec::new(),
                };
                let definitions = self.add_scoped_definitions(producer, definitions);
                self.record_scoped_borrow_sources(&definitions, iterable_value);
                self.push_scoped_definitions(definitions);
                let condition = condition
                    .as_deref()
                    .and_then(|condition| self.lower_scalar_expr(producer, condition));
                let value = self.lower_scalar_expr(producer, value);
                self.scoped_definition_stack.pop();
                match (iterable_value, value) {
                    (Some(iterable), Some(value)) => ControlFlowValueKind::ListComprehension {
                        iterable,
                        value,
                        condition,
                    },
                    _ => ControlFlowValueKind::Opaque,
                }
            }
            ExprKind::StructLiteral {
                name, base, fields, ..
            } => {
                let base = base
                    .as_deref()
                    .and_then(|value| self.lower_scalar_expr(producer, value));
                let fields = fields
                    .iter()
                    .filter_map(|field| {
                        self.lower_scalar_expr(producer, &field.value)
                            .map(|value| (field.name.clone(), value))
                    })
                    .collect();
                ControlFlowValueKind::StructLiteral {
                    name: name.clone(),
                    base,
                    fields,
                }
            }
            ExprKind::QualifiedCall {
                namespace,
                name,
                args,
                named_args,
                ..
            } => {
                let arguments = self.lower_call_arguments(producer, args, named_args);
                if self.signatures.interface(namespace).is_some() {
                    let concrete_target = arguments.first().and_then(|receiver| {
                        let receiver = self.values.get(receiver.0)?;
                        let Type::Named(target) = self.signatures.canonical_type(&receiver.ty)
                        else {
                            return None;
                        };
                        (target != *namespace
                            && self.signatures.implementation(namespace, &target).is_some())
                        .then_some(target)
                    });
                    let mapped_function = concrete_target.as_ref().and_then(|target| {
                        self.signatures
                            .implementation(namespace, target)
                            .and_then(|implementation| implementation.functions.get(name))
                            .cloned()
                    });
                    ControlFlowValueKind::InterfaceDispatch {
                        interface: namespace.clone(),
                        capability: name.clone(),
                        target: concrete_target,
                        mapped_function,
                        arguments,
                    }
                } else {
                    ControlFlowValueKind::QualifiedCall {
                        namespace: namespace.clone(),
                        name: name.clone(),
                        arguments,
                    }
                }
            }
            ExprKind::Field {
                base,
                name,
                optional,
                ..
            } if !*optional
                && matches!(&base.kind, ExprKind::Var(namespace) if namespace == "package")
                && self
                    .signatures
                    .package_constant(expr.span.source_id, name)
                    .is_some() =>
            {
                ControlFlowValueKind::Literal
            }
            ExprKind::Field { base, name, .. } => self.lower_scalar_expr(producer, base).map_or(
                ControlFlowValueKind::Opaque,
                |base| ControlFlowValueKind::Field {
                    base,
                    name: name.clone(),
                },
            ),
            ExprKind::Match { value, arms } => {
                let matched_value = self.lower_scalar_expr(producer, value);
                let mut guards = Vec::with_capacity(arms.len());
                let mut arm_values = Vec::with_capacity(arms.len());
                for arm in arms {
                    let definitions = self.enum_match_expr_definitions(value, arm);
                    let definitions = self.add_scoped_definitions(producer, definitions);
                    self.record_scoped_borrow_sources(&definitions, matched_value);
                    self.push_scoped_definitions(definitions);
                    guards.push(
                        arm.guard
                            .as_ref()
                            .and_then(|guard| self.lower_scalar_expr(producer, guard)),
                    );
                    if let Some(value) = self.lower_scalar_expr(producer, &arm.value) {
                        arm_values.push(value);
                    }
                    self.scoped_definition_stack.pop();
                }
                matched_value.map_or(ControlFlowValueKind::Opaque, |value| {
                    ControlFlowValueKind::Match {
                        value,
                        guards,
                        arms: arm_values,
                    }
                })
            }
            ExprKind::ListMatch { value, arms } => {
                let matched_value = self.lower_scalar_expr(producer, value);
                let mut guards = Vec::with_capacity(arms.len());
                let mut arm_values = Vec::with_capacity(arms.len());
                for arm in arms {
                    let definitions = self.list_match_definitions(value, &arm.pattern);
                    let definitions = self.add_scoped_definitions(producer, definitions);
                    self.record_scoped_borrow_sources(&definitions, matched_value);
                    self.push_scoped_definitions(definitions);
                    guards.push(
                        arm.guard
                            .as_ref()
                            .and_then(|guard| self.lower_scalar_expr(producer, guard)),
                    );
                    if let Some(value) = self.lower_scalar_expr(producer, &arm.value) {
                        arm_values.push(value);
                    }
                    self.scoped_definition_stack.pop();
                }
                matched_value.map_or(ControlFlowValueKind::Opaque, |value| {
                    ControlFlowValueKind::ListMatch {
                        value,
                        guards,
                        arms: arm_values,
                    }
                })
            }
            ExprKind::Conditional {
                then_expr,
                cond,
                else_expr,
            } => {
                let condition = self.lower_scalar_expr(producer, cond);
                let then_value = self.lower_scalar_expr(producer, then_expr);
                let else_value = self.lower_scalar_expr(producer, else_expr);
                match (condition, then_value, else_value) {
                    (Some(condition), Some(then_value), Some(else_value)) => {
                        ControlFlowValueKind::Conditional {
                            condition,
                            then_value,
                            else_value,
                        }
                    }
                    _ => ControlFlowValueKind::Opaque,
                }
            }
            ExprKind::Unary { op, expr: inner } => self
                .lower_scalar_expr(producer, inner)
                .map_or(ControlFlowValueKind::Opaque, |operand| {
                    ControlFlowValueKind::Unary { op: *op, operand }
                }),
            ExprKind::Binary { left, op, right } => {
                let left = self.lower_scalar_expr(producer, left);
                let right = self.lower_scalar_expr(producer, right);
                match (left, right) {
                    (Some(left), Some(right)) => ControlFlowValueKind::Binary {
                        op: *op,
                        left,
                        right,
                    },
                    _ => ControlFlowValueKind::Opaque,
                }
            }
        };
        let constant = typecheck::constant_primitive_value(expr, self.signatures);
        self.push_typed_values(producer, expr.span, kind, constant, is_result)
    }

    fn lower_call_arguments(
        &mut self,
        producer: ControlFlowNodeId,
        args: &[Expr],
        named_args: &[crate::ast::NamedArg],
    ) -> Vec<ControlFlowValueId> {
        let mut values = self.lower_expr_arguments(producer, args);
        values.extend(
            named_args
                .iter()
                .filter_map(|arg| self.lower_scalar_expr(producer, &arg.value)),
        );
        values
    }

    fn lower_expr_arguments(
        &mut self,
        producer: ControlFlowNodeId,
        expressions: &[Expr],
    ) -> Vec<ControlFlowValueId> {
        expressions
            .iter()
            .filter_map(|expr| self.lower_scalar_expr(producer, expr))
            .collect()
    }

    fn lower_scalar_expr(
        &mut self,
        producer: ControlFlowNodeId,
        expr: &Expr,
    ) -> Option<ControlFlowValueId> {
        self.lower_expr_values(producer, expr, false)
            .into_iter()
            .next()
    }

    fn push_typed_values(
        &mut self,
        producer: ControlFlowNodeId,
        span: SourceSpan,
        kind: ControlFlowValueKind,
        constant: Option<ConstantValue>,
        is_result: bool,
    ) -> Vec<ControlFlowValueId> {
        let types = self
            .evaluation_types
            .iter()
            .find(|(candidate, _)| *candidate == span)
            .map(|(_, types)| types.clone())
            .unwrap_or_default();
        let scalar = types.len() == 1;
        types
            .into_iter()
            .enumerate()
            .map(|(index, ty)| {
                let id = ControlFlowValueId(self.values.len());
                self.values.push(ControlFlowValue {
                    id,
                    producer,
                    result_index: is_result.then_some(index),
                    ty,
                    span,
                    kind: kind.clone(),
                    source_constant: scalar.then(|| constant.clone()).flatten(),
                    constant: scalar.then(|| constant.clone()).flatten(),
                });
                id
            })
            .collect()
    }

    fn expression_types(&self, expr: &Expr) -> Vec<Type> {
        self.evaluation_types
            .iter()
            .find(|(span, _)| *span == expr.span)
            .map(|(_, types)| {
                types
                    .iter()
                    .map(|ty| self.signatures.canonical_type(ty))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn scalar_expression_type(&self, expr: &Expr) -> Option<Type> {
        let types = self.expression_types(expr);
        (types.len() == 1).then(|| types[0].clone())
    }

    fn definition(&self, name: &str, ty: &Type, span: SourceSpan) -> ControlFlowDefinition {
        ControlFlowDefinition {
            name: name.to_string(),
            ty: self.signatures.canonical_type(ty),
            span,
        }
    }

    fn list_destructure_definitions(
        &self,
        bindings: &[crate::ast::PatternBinding],
        rest: Option<&crate::ast::ListRestPattern>,
        expr: &Expr,
    ) -> Vec<ControlFlowDefinition> {
        let Some(Type::List(element)) = self.scalar_expression_type(expr) else {
            return Vec::new();
        };
        let mut definitions = bindings
            .iter()
            .filter(|binding| binding.name != "_")
            .map(|binding| ControlFlowDefinition {
                name: binding.name.clone(),
                ty: (*element).clone(),
                span: binding.span,
            })
            .collect::<Vec<_>>();
        if let Some(rest) = rest
            && rest.binding.name != "_"
        {
            definitions.push(ControlFlowDefinition {
                name: rest.binding.name.clone(),
                ty: Type::List(element),
                span: rest.binding.span,
            });
        }
        definitions
    }

    fn struct_pattern_definitions(
        &self,
        fields: &[crate::ast::StructPatternField],
        struct_name: &str,
    ) -> Vec<ControlFlowDefinition> {
        let mut definitions = Vec::new();
        self.collect_struct_pattern_definitions(fields, struct_name, &mut definitions);
        definitions
    }

    fn collect_struct_pattern_definitions(
        &self,
        fields: &[crate::ast::StructPatternField],
        struct_name: &str,
        definitions: &mut Vec<ControlFlowDefinition>,
    ) {
        let Type::Named(concrete_name) = self
            .signatures
            .canonical_type(&Type::Named(struct_name.to_string()))
        else {
            return;
        };
        let Some(definition) = self.signatures.struct_type(&concrete_name) else {
            return;
        };
        for field in fields {
            let Some(field_signature) = definition.field(&field.field) else {
                continue;
            };
            if let Some(nested) = &field.nested {
                let Type::Named(nested_name) = self.signatures.canonical_type(&field_signature.ty)
                else {
                    continue;
                };
                self.collect_struct_pattern_definitions(&nested.fields, &nested_name, definitions);
            } else if field.binding.name != "_" {
                definitions.push(ControlFlowDefinition {
                    name: field.binding.name.clone(),
                    ty: self.signatures.canonical_type(&field_signature.ty),
                    span: field.binding.span,
                });
            }
        }
    }

    fn enum_match_definitions(
        &self,
        value: &Expr,
        arm: &crate::ast::MatchArm,
    ) -> Vec<ControlFlowDefinition> {
        self.enum_match_pattern_definitions(value, &arm.variant, &arm.patterns)
    }

    fn enum_match_expr_definitions(
        &self,
        value: &Expr,
        arm: &crate::ast::MatchExprArm,
    ) -> Vec<ControlFlowDefinition> {
        self.enum_match_pattern_definitions(value, &arm.variant, &arm.patterns)
    }

    fn enum_match_pattern_definitions(
        &self,
        value: &Expr,
        variant_name: &str,
        patterns: &[crate::ast::MatchPattern],
    ) -> Vec<ControlFlowDefinition> {
        let Some(Type::Named(enum_name)) = self.scalar_expression_type(value) else {
            return Vec::new();
        };
        let Some(definition) = self.signatures.enum_type(&enum_name) else {
            return Vec::new();
        };
        let Some(variant) = definition.variant(variant_name) else {
            return Vec::new();
        };
        let mut definitions = Vec::new();
        for (pattern, payload) in patterns.iter().zip(&variant.payloads) {
            match pattern {
                crate::ast::MatchPattern::Binding(binding) if binding.name != "_" => {
                    definitions.push(ControlFlowDefinition {
                        name: binding.name.clone(),
                        ty: self.signatures.canonical_type(payload),
                        span: binding.span,
                    });
                }
                crate::ast::MatchPattern::Struct(pattern) => self
                    .collect_struct_pattern_definitions(
                        &pattern.fields,
                        &pattern.struct_name,
                        &mut definitions,
                    ),
                crate::ast::MatchPattern::Binding(_)
                | crate::ast::MatchPattern::Relational(_)
                | crate::ast::MatchPattern::Logical { .. } => {}
            }
        }
        definitions
    }

    fn list_match_definitions(
        &self,
        value: &Expr,
        pattern: &crate::ast::ListMatchPattern,
    ) -> Vec<ControlFlowDefinition> {
        let Some(Type::List(element)) = self.scalar_expression_type(value) else {
            return Vec::new();
        };
        let crate::ast::ListMatchPattern::List { bindings, rest, .. } = pattern else {
            return Vec::new();
        };
        let mut definitions = bindings
            .iter()
            .filter(|binding| binding.name != "_")
            .map(|binding| ControlFlowDefinition {
                name: binding.name.clone(),
                ty: (*element).clone(),
                span: binding.span,
            })
            .collect::<Vec<_>>();
        if let Some(rest) = rest
            && rest.binding.name != "_"
        {
            definitions.push(ControlFlowDefinition {
                name: rest.binding.name.clone(),
                ty: Type::List(element),
                span: rest.binding.span,
            });
        }
        definitions
    }

    fn ownership_for_expr(&self, expr: &Expr) -> ControlFlowOwnership {
        let mut reads = HashSet::new();
        typecheck::collect_expr_reads(expr, &mut reads);
        let mut reads = reads.into_iter().collect::<Vec<_>>();
        reads.sort();
        ControlFlowOwnership {
            reads,
            borrows: Vec::new(),
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
            borrows: Vec::new(),
            moves,
        }
    }
}

fn collect_evaluation_types(
    function: &Function,
    signatures: &Signatures,
) -> Vec<(SourceSpan, Vec<Type>)> {
    let mut env = function
        .params
        .iter()
        .map(|param| (param.name.clone(), signatures.canonical_type(&param.ty)))
        .collect::<HashMap<_, _>>();
    let mut evaluations = Vec::new();
    collect_block_evaluation_types(&function.body, &mut env, signatures, &mut evaluations);
    evaluations
}

fn collect_block_evaluation_types(
    body: &[Stmt],
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
    evaluations: &mut Vec<(SourceSpan, Vec<Type>)>,
) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Let { name, ty, expr, .. } | StmtKind::Var { name, ty, expr, .. } => {
                record_evaluation_type(expr, env, signatures, evaluations);
                env.insert(name.clone(), signatures.canonical_type(ty));
            }
            StmtKind::Assign { expr, .. }
            | StmtKind::AssignMultiDestructure { expr, .. }
            | StmtKind::AssignListDestructure { expr, .. }
            | StmtKind::AssignStructDestructure { expr, .. }
            | StmtKind::Expr(expr) => {
                record_evaluation_type(expr, env, signatures, evaluations);
            }
            StmtKind::LetDestructure { bindings, expr, .. } => {
                record_evaluation_type(expr, env, signatures, evaluations);
                for binding in bindings {
                    env.insert(binding.name.clone(), signatures.canonical_type(&binding.ty));
                }
            }
            StmtKind::LetMultiDestructure { bindings, expr, .. } => {
                record_evaluation_type(expr, env, signatures, evaluations);
                if let Ok(types) = typecheck::value_types_of_expr(expr, env, signatures) {
                    for (binding, ty) in bindings.iter().zip(types) {
                        if binding.name != "_" {
                            env.insert(binding.name.clone(), signatures.canonical_type(&ty));
                        }
                    }
                }
            }
            StmtKind::LetListDestructure {
                bindings,
                rest,
                expr,
                ..
            } => {
                record_evaluation_type(expr, env, signatures, evaluations);
                if let Ok(ty) = typecheck::type_of_expr(expr, env, signatures)
                    && let Type::List(element) = signatures.canonical_type(&ty)
                {
                    for binding in bindings {
                        if binding.name != "_" {
                            env.insert(binding.name.clone(), (*element).clone());
                        }
                    }
                    if let Some(rest) = rest
                        && rest.binding.name != "_"
                    {
                        env.insert(rest.binding.name.clone(), Type::List(element));
                    }
                }
            }
            StmtKind::LetStructDestructure {
                struct_name,
                fields,
                expr,
                ..
            } => {
                record_evaluation_type(expr, env, signatures, evaluations);
                bind_struct_pattern_fields(fields, struct_name, env, signatures);
            }
            StmtKind::Return(values) => {
                for value in values {
                    record_evaluation_type(value, env, signatures, evaluations);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Shell { expr, redirect, .. } => {
                record_evaluation_type(expr, env, signatures, evaluations);
                if let Some(redirect) = redirect {
                    record_evaluation_type(&redirect.path, env, signatures, evaluations);
                }
            }
            StmtKind::If {
                cond,
                body,
                else_body,
                ..
            } => {
                record_evaluation_type(cond, env, signatures, evaluations);
                let mut then_env = env.clone();
                collect_block_evaluation_types(body, &mut then_env, signatures, evaluations);
                let mut else_env = env.clone();
                collect_block_evaluation_types(else_body, &mut else_env, signatures, evaluations);
            }
            StmtKind::ForRange {
                name,
                start,
                end,
                body,
                ..
            } => {
                record_evaluation_type(start, env, signatures, evaluations);
                record_evaluation_type(end, env, signatures, evaluations);
                let mut nested = env.clone();
                nested.insert(name.clone(), Type::I64);
                collect_block_evaluation_types(body, &mut nested, signatures, evaluations);
            }
            StmtKind::ForEach {
                index_name,
                name,
                iterable,
                body,
                ..
            } => {
                record_evaluation_type(iterable, env, signatures, evaluations);
                let mut nested = env.clone();
                if let Some(index_name) = index_name {
                    nested.insert(index_name.clone(), Type::I64);
                }
                if let Ok(ty) = typecheck::type_of_expr(iterable, env, signatures)
                    && let Type::List(element) = signatures.canonical_type(&ty)
                {
                    nested.insert(name.clone(), *element);
                }
                collect_block_evaluation_types(body, &mut nested, signatures, evaluations);
            }
            StmtKind::While { cond, body } => {
                record_evaluation_type(cond, env, signatures, evaluations);
                let mut nested = env.clone();
                collect_block_evaluation_types(body, &mut nested, signatures, evaluations);
            }
            StmtKind::Match { value, arms } => {
                record_evaluation_type(value, env, signatures, evaluations);
                let matched_ty = typecheck::type_of_expr(value, env, signatures)
                    .ok()
                    .map(|ty| signatures.canonical_type(&ty));
                for arm in arms {
                    let mut nested = env.clone();
                    if let Some(Type::Named(enum_name)) = matched_ty.as_ref()
                        && let Some(definition) = signatures.enum_type(enum_name)
                        && let Some(variant) = definition.variant(&arm.variant)
                    {
                        for (pattern, payload) in arm.patterns.iter().zip(&variant.payloads) {
                            bind_match_pattern(pattern, payload, &mut nested, signatures);
                        }
                    }
                    if let Some(guard) = &arm.guard {
                        record_evaluation_type(guard, &nested, signatures, evaluations);
                    }
                    collect_block_evaluation_types(&arm.body, &mut nested, signatures, evaluations);
                }
            }
            StmtKind::ListMatch { value, arms } => {
                record_evaluation_type(value, env, signatures, evaluations);
                let element_ty = typecheck::type_of_expr(value, env, signatures)
                    .ok()
                    .and_then(|ty| match signatures.canonical_type(&ty) {
                        Type::List(element) => Some(*element),
                        _ => None,
                    });
                for arm in arms {
                    let mut nested = env.clone();
                    if let Some(element_ty) = element_ty.as_ref() {
                        bind_list_match_pattern(&arm.pattern, element_ty, &mut nested);
                    }
                    if let Some(guard) = &arm.guard {
                        record_evaluation_type(guard, &nested, signatures, evaluations);
                    }
                    collect_block_evaluation_types(&arm.body, &mut nested, signatures, evaluations);
                }
            }
        }
    }
}

fn record_evaluation_type(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    evaluations: &mut Vec<(SourceSpan, Vec<Type>)>,
) {
    record_expr_types(expr, env, signatures, evaluations);
}

fn record_expr_types(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    evaluations: &mut Vec<(SourceSpan, Vec<Type>)>,
) {
    match &expr.kind {
        ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::None
        | ExprKind::Var(_) => {}
        ExprKind::AnonymousFunction { params, body, .. } => {
            let mut nested = env.clone();
            for param in params {
                nested.insert(param.name.clone(), signatures.canonical_type(&param.ty));
            }
            record_expr_types(body, &nested, signatures, evaluations);
        }
        ExprKind::Call {
            args, named_args, ..
        }
        | ExprKind::QualifiedCall {
            args, named_args, ..
        } => {
            for arg in args {
                record_expr_types(arg, env, signatures, evaluations);
            }
            for arg in named_args {
                record_expr_types(&arg.value, env, signatures, evaluations);
            }
        }
        ExprKind::ShellCall { args, .. } => {
            for arg in args {
                record_expr_types(arg, env, signatures, evaluations);
            }
        }
        ExprKind::Pipe { input, args, .. } => {
            record_expr_types(input, env, signatures, evaluations);
            for arg in args {
                record_expr_types(arg, env, signatures, evaluations);
            }
        }
        ExprKind::List(items) => {
            for item in items {
                record_expr_types(item, env, signatures, evaluations);
            }
        }
        ExprKind::ListSpread { value, .. } => {
            record_expr_types(value, env, signatures, evaluations);
            if let Ok(ty) = typecheck::type_of_expr(value, env, signatures)
                && let Type::List(element) = signatures.canonical_type(&ty)
            {
                evaluations.push((expr.span, vec![*element]));
            }
        }
        ExprKind::ListOptional { value, .. } => {
            record_expr_types(value, env, signatures, evaluations);
            if let Ok(ty) = typecheck::type_of_expr(value, env, signatures)
                && let Type::Optional(inner) = signatures.canonical_type(&ty)
                && !matches!(inner.as_ref(), Type::Void)
            {
                evaluations.push((expr.span, vec![*inner]));
            }
        }
        ExprKind::ListIf {
            condition,
            binding,
            value,
            else_value,
            ..
        } => {
            record_expr_types(condition, env, signatures, evaluations);
            let mut guarded_env = env.clone();
            if let Some(binding) = binding
                && binding.name != "_"
                && let Ok(Type::Optional(inner)) =
                    typecheck::type_of_expr(condition, env, signatures)
            {
                guarded_env.insert(binding.name.clone(), signatures.canonical_type(&inner));
            }
            record_expr_types(value, &guarded_env, signatures, evaluations);
            if let Some(else_value) = else_value {
                record_expr_types(else_value, env, signatures, evaluations);
            }
            if let Ok(ty) = typecheck::type_of_expr(value, &guarded_env, signatures) {
                evaluations.push((expr.span, vec![signatures.canonical_type(&ty)]));
            }
        }
        ExprKind::Index { base, index } => {
            record_expr_types(base, env, signatures, evaluations);
            record_expr_types(index, env, signatures, evaluations);
        }
        ExprKind::Slice {
            base,
            start,
            end,
            step,
        } => {
            record_expr_types(base, env, signatures, evaluations);
            for bound in [start.as_deref(), end.as_deref(), step.as_deref()]
                .into_iter()
                .flatten()
            {
                record_expr_types(bound, env, signatures, evaluations);
            }
        }
        ExprKind::ListComprehension {
            value,
            binding,
            iterable,
            condition,
            ..
        } => {
            record_expr_types(iterable, env, signatures, evaluations);
            let mut nested = env.clone();
            if let Ok(ty) = typecheck::type_of_expr(iterable, env, signatures)
                && let Type::List(element) = signatures.canonical_type(&ty)
            {
                nested.insert(binding.clone(), *element);
            }
            record_expr_types(value, &nested, signatures, evaluations);
            if let Some(condition) = condition {
                record_expr_types(condition, &nested, signatures, evaluations);
            }
        }
        ExprKind::StructLiteral { base, fields, .. } => {
            if let Some(base) = base {
                record_expr_types(base, env, signatures, evaluations);
            }
            for field in fields {
                record_expr_types(&field.value, env, signatures, evaluations);
            }
        }
        ExprKind::Field {
            base,
            name,
            optional,
            ..
        } => {
            if *optional
                || !matches!(&base.kind, ExprKind::Var(namespace) if namespace == "package")
                || signatures
                    .package_constant(expr.span.source_id, name)
                    .is_none()
            {
                record_expr_types(base, env, signatures, evaluations);
            }
        }
        ExprKind::Match { value, arms } => {
            record_expr_types(value, env, signatures, evaluations);
            let matched_ty = typecheck::type_of_expr(value, env, signatures)
                .ok()
                .map(|ty| signatures.canonical_type(&ty));
            for arm in arms {
                let mut nested = env.clone();
                if let Some(Type::Named(enum_name)) = matched_ty.as_ref()
                    && let Some(definition) = signatures.enum_type(enum_name)
                    && let Some(variant) = definition.variant(&arm.variant)
                {
                    for (pattern, payload) in arm.patterns.iter().zip(&variant.payloads) {
                        bind_match_pattern(pattern, payload, &mut nested, signatures);
                    }
                }
                if let Some(guard) = &arm.guard {
                    record_expr_types(guard, &nested, signatures, evaluations);
                }
                record_expr_types(&arm.value, &nested, signatures, evaluations);
            }
        }
        ExprKind::ListMatch { value, arms } => {
            record_expr_types(value, env, signatures, evaluations);
            let element_ty = typecheck::type_of_expr(value, env, signatures)
                .ok()
                .and_then(|ty| match signatures.canonical_type(&ty) {
                    Type::List(element) => Some(*element),
                    _ => None,
                });
            for arm in arms {
                let mut nested = env.clone();
                if let Some(element_ty) = element_ty.as_ref() {
                    bind_list_match_pattern(&arm.pattern, element_ty, &mut nested);
                }
                if let Some(guard) = &arm.guard {
                    record_expr_types(guard, &nested, signatures, evaluations);
                }
                record_expr_types(&arm.value, &nested, signatures, evaluations);
            }
        }
        ExprKind::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            record_expr_types(cond, env, signatures, evaluations);
            record_expr_types(then_expr, env, signatures, evaluations);
            record_expr_types(else_expr, env, signatures, evaluations);
        }
        ExprKind::Unary { expr, .. } => {
            record_expr_types(expr, env, signatures, evaluations);
        }
        ExprKind::Binary { left, right, .. } => {
            record_expr_types(left, env, signatures, evaluations);
            record_expr_types(right, env, signatures, evaluations);
        }
    }

    if let Ok(types) = typecheck::value_types_of_expr(expr, env, signatures) {
        evaluations.push((
            expr.span,
            types
                .into_iter()
                .map(|ty| signatures.canonical_type(&ty))
                .collect(),
        ));
    }
}

fn bind_match_pattern(
    pattern: &crate::ast::MatchPattern,
    ty: &Type,
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
) {
    match pattern {
        crate::ast::MatchPattern::Binding(binding) => {
            if binding.name != "_" {
                env.insert(binding.name.clone(), signatures.canonical_type(ty));
            }
        }
        crate::ast::MatchPattern::Struct(pattern) => {
            bind_struct_pattern_fields(&pattern.fields, &pattern.struct_name, env, signatures);
        }
        crate::ast::MatchPattern::Relational(_) | crate::ast::MatchPattern::Logical { .. } => {}
    }
}

fn bind_struct_pattern_fields(
    fields: &[crate::ast::StructPatternField],
    struct_name: &str,
    env: &mut HashMap<String, Type>,
    signatures: &Signatures,
) {
    let Type::Named(concrete_name) =
        signatures.canonical_type(&Type::Named(struct_name.to_string()))
    else {
        return;
    };
    let Some(definition) = signatures.struct_type(&concrete_name) else {
        return;
    };
    for field in fields {
        let Some(field_signature) = definition.field(&field.field) else {
            continue;
        };
        if let Some(nested) = &field.nested {
            bind_struct_pattern_fields(&nested.fields, &nested.struct_name, env, signatures);
        } else if field.binding.name != "_" {
            env.insert(
                field.binding.name.clone(),
                signatures.canonical_type(&field_signature.ty),
            );
        }
    }
}

fn bind_list_match_pattern(
    pattern: &crate::ast::ListMatchPattern,
    element_ty: &Type,
    env: &mut HashMap<String, Type>,
) {
    let crate::ast::ListMatchPattern::List { bindings, rest, .. } = pattern else {
        return;
    };
    for binding in bindings {
        if binding.name != "_" {
            env.insert(binding.name.clone(), element_ty.clone());
        }
    }
    if let Some(rest) = rest
        && rest.binding.name != "_"
    {
        env.insert(
            rest.binding.name.clone(),
            Type::List(Box::new(element_ty.clone())),
        );
    }
}

fn push_value_use(
    uses: &mut Vec<ControlFlowValueUse>,
    user: ControlFlowValueId,
    value: ControlFlowValueId,
    kind: ControlFlowValueUseKind,
) {
    uses.push(ControlFlowValueUse {
        user,
        value,
        kind,
        region: None,
    });
}

fn push_value_region_use(
    values: &[ControlFlowValue],
    uses: &mut Vec<ControlFlowValueUse>,
    regions: &mut Vec<ControlFlowValueRegion>,
    owner: &ControlFlowValue,
    root: ControlFlowValueId,
    use_kind: ControlFlowValueUseKind,
    region_kind: ControlFlowValueRegionKind,
) {
    let id = ControlFlowValueRegionId(regions.len());
    let span = values
        .get(root.0)
        .map(|value| value.span)
        .unwrap_or(owner.span);
    regions.push(ControlFlowValueRegion {
        id,
        owner: owner.id,
        root,
        span,
        kind: region_kind,
    });
    uses.push(ControlFlowValueUse {
        user: owner.id,
        value: root,
        kind: use_kind,
        region: Some(id),
    });
}

fn collect_value_uses(
    values: &[ControlFlowValue],
) -> (Vec<ControlFlowValueUse>, Vec<ControlFlowValueRegion>) {
    let mut uses = Vec::new();
    let mut regions = Vec::new();
    for value in values {
        match &value.kind {
            ControlFlowValueKind::Literal
            | ControlFlowValueKind::NameRead { .. }
            | ControlFlowValueKind::Opaque => {}
            ControlFlowValueKind::AnonymousFunction { body } => {
                push_value_region_use(
                    values,
                    &mut uses,
                    &mut regions,
                    value,
                    *body,
                    ControlFlowValueUseKind::DeferredBody,
                    ControlFlowValueRegionKind::DeferredBody,
                );
            }
            ControlFlowValueKind::Call { arguments, .. }
            | ControlFlowValueKind::QualifiedCall { arguments, .. }
            | ControlFlowValueKind::InterfaceDispatch { arguments, .. } => {
                for argument in arguments {
                    push_value_use(
                        &mut uses,
                        value.id,
                        *argument,
                        ControlFlowValueUseKind::Eager,
                    );
                }
            }
            ControlFlowValueKind::OptionalCascadeCall {
                optional,
                arguments,
                ..
            } => {
                push_value_use(
                    &mut uses,
                    value.id,
                    *optional,
                    ControlFlowValueUseKind::Eager,
                );
                for argument in arguments {
                    push_value_region_use(
                        values,
                        &mut uses,
                        &mut regions,
                        value,
                        *argument,
                        ControlFlowValueUseKind::OptionalPresent,
                        ControlFlowValueRegionKind::OptionalPresent {
                            optional: *optional,
                        },
                    );
                }
            }
            ControlFlowValueKind::InterfacePack { value: packed, .. }
            | ControlFlowValueKind::ListSpread { value: packed }
            | ControlFlowValueKind::ListOptional { value: packed }
            | ControlFlowValueKind::Field { base: packed, .. } => {
                push_value_use(&mut uses, value.id, *packed, ControlFlowValueUseKind::Eager);
            }
            ControlFlowValueKind::List { items } => {
                for item in items {
                    push_value_use(&mut uses, value.id, *item, ControlFlowValueUseKind::Eager);
                }
            }
            ControlFlowValueKind::ListIf {
                condition,
                value: then_value,
                else_value,
            } => {
                push_value_use(
                    &mut uses,
                    value.id,
                    *condition,
                    ControlFlowValueUseKind::BranchCondition,
                );
                push_value_region_use(
                    values,
                    &mut uses,
                    &mut regions,
                    value,
                    *then_value,
                    ControlFlowValueUseKind::BranchThen,
                    ControlFlowValueRegionKind::Branch {
                        condition: *condition,
                        selected_when: true,
                    },
                );
                if let Some(else_value) = else_value {
                    push_value_region_use(
                        values,
                        &mut uses,
                        &mut regions,
                        value,
                        *else_value,
                        ControlFlowValueUseKind::BranchElse,
                        ControlFlowValueRegionKind::Branch {
                            condition: *condition,
                            selected_when: false,
                        },
                    );
                }
            }
            ControlFlowValueKind::Index { base, index } => {
                push_value_use(&mut uses, value.id, *base, ControlFlowValueUseKind::Eager);
                push_value_use(&mut uses, value.id, *index, ControlFlowValueUseKind::Eager);
            }
            ControlFlowValueKind::Slice {
                base,
                start,
                end,
                step,
            } => {
                push_value_use(&mut uses, value.id, *base, ControlFlowValueUseKind::Eager);
                for bound in [start, end, step].into_iter().flatten() {
                    push_value_use(&mut uses, value.id, *bound, ControlFlowValueUseKind::Eager);
                }
            }
            ControlFlowValueKind::ListComprehension {
                iterable,
                value: body,
                condition,
            } => {
                push_value_use(
                    &mut uses,
                    value.id,
                    *iterable,
                    ControlFlowValueUseKind::LoopIterable,
                );
                if let Some(condition) = condition {
                    push_value_region_use(
                        values,
                        &mut uses,
                        &mut regions,
                        value,
                        *condition,
                        ControlFlowValueUseKind::LoopCondition,
                        ControlFlowValueRegionKind::LoopCondition {
                            iterable: *iterable,
                        },
                    );
                }
                push_value_region_use(
                    values,
                    &mut uses,
                    &mut regions,
                    value,
                    *body,
                    ControlFlowValueUseKind::LoopBody,
                    ControlFlowValueRegionKind::LoopBody {
                        iterable: *iterable,
                        condition: *condition,
                    },
                );
            }
            ControlFlowValueKind::StructLiteral { base, fields, .. } => {
                if let Some(base) = base {
                    push_value_use(&mut uses, value.id, *base, ControlFlowValueUseKind::Eager);
                }
                for (_, field) in fields {
                    push_value_use(&mut uses, value.id, *field, ControlFlowValueUseKind::Eager);
                }
            }
            ControlFlowValueKind::Match {
                value: matched,
                guards,
                arms,
            }
            | ControlFlowValueKind::ListMatch {
                value: matched,
                guards,
                arms,
            } => {
                push_value_use(
                    &mut uses,
                    value.id,
                    *matched,
                    ControlFlowValueUseKind::MatchValue,
                );
                for (index, guard) in guards.iter().enumerate() {
                    if let Some(guard) = guard {
                        push_value_region_use(
                            values,
                            &mut uses,
                            &mut regions,
                            value,
                            *guard,
                            ControlFlowValueUseKind::MatchGuard(index),
                            ControlFlowValueRegionKind::MatchGuard {
                                matched: *matched,
                                arm: index,
                            },
                        );
                    }
                }
                for (index, arm) in arms.iter().enumerate() {
                    push_value_region_use(
                        values,
                        &mut uses,
                        &mut regions,
                        value,
                        *arm,
                        ControlFlowValueUseKind::MatchArm(index),
                        ControlFlowValueRegionKind::MatchArm {
                            matched: *matched,
                            guard: guards.get(index).copied().flatten(),
                            arm: index,
                        },
                    );
                }
            }
            ControlFlowValueKind::Conditional {
                condition,
                then_value,
                else_value,
            } => {
                push_value_use(
                    &mut uses,
                    value.id,
                    *condition,
                    ControlFlowValueUseKind::BranchCondition,
                );
                push_value_region_use(
                    values,
                    &mut uses,
                    &mut regions,
                    value,
                    *then_value,
                    ControlFlowValueUseKind::BranchThen,
                    ControlFlowValueRegionKind::Branch {
                        condition: *condition,
                        selected_when: true,
                    },
                );
                push_value_region_use(
                    values,
                    &mut uses,
                    &mut regions,
                    value,
                    *else_value,
                    ControlFlowValueUseKind::BranchElse,
                    ControlFlowValueRegionKind::Branch {
                        condition: *condition,
                        selected_when: false,
                    },
                );
            }
            ControlFlowValueKind::Unary { operand, .. } => {
                push_value_use(
                    &mut uses,
                    value.id,
                    *operand,
                    ControlFlowValueUseKind::Eager,
                );
            }
            ControlFlowValueKind::Binary { op, left, right } => {
                push_value_use(&mut uses, value.id, *left, ControlFlowValueUseKind::Eager);
                if matches!(op, BinOp::And | BinOp::Or) {
                    push_value_region_use(
                        values,
                        &mut uses,
                        &mut regions,
                        value,
                        *right,
                        ControlFlowValueUseKind::ShortCircuitRight,
                        ControlFlowValueRegionKind::ShortCircuitRight {
                            condition: *left,
                            execute_when: matches!(op, BinOp::And),
                        },
                    );
                } else if matches!(op, BinOp::Coalesce) {
                    push_value_region_use(
                        values,
                        &mut uses,
                        &mut regions,
                        value,
                        *right,
                        ControlFlowValueUseKind::ShortCircuitRight,
                        ControlFlowValueRegionKind::OptionalFallback { optional: *left },
                    );
                } else {
                    push_value_use(&mut uses, value.id, *right, ControlFlowValueUseKind::Eager);
                }
            }
        }
    }
    (uses, regions)
}

fn reclassify_borrowed_list_reborrows(
    nodes: &mut [ControlFlowNode],
    edges: &[ControlFlowEdge],
    values: &[ControlFlowValue],
    definition_values: &BTreeMap<ControlFlowDefinitionId, ControlFlowValueId>,
    scoped_borrow_sources: &BTreeMap<ControlFlowDefinitionId, ControlFlowValueId>,
    parameters: &[ControlFlowParameter],
    signatures: &Signatures,
) {
    let mut borrowed_definitions = parameters
        .iter()
        .enumerate()
        .filter(|(_, parameter)| matches!(signatures.canonical_type(&parameter.ty), Type::List(_)))
        .map(|(index, _)| ControlFlowDefinitionId::Parameter(index))
        .collect::<BTreeSet<_>>();
    borrowed_definitions.extend(scoped_borrow_sources.keys().copied());
    for node in nodes.iter() {
        for (index, definition) in node.definitions.iter().enumerate() {
            if !matches!(signatures.canonical_type(&definition.ty), Type::List(_)) {
                continue;
            }
            let id = ControlFlowDefinitionId::Node {
                node: node.id,
                index,
            };
            if definition_borrow_source_value_from_parts(nodes, edges, scoped_borrow_sources, id)
                .is_some()
            {
                borrowed_definitions.insert(id);
            }
        }
    }

    loop {
        let mut changed = false;
        for node in nodes.iter() {
            for (index, definition) in node.definitions.iter().enumerate() {
                if !matches!(signatures.canonical_type(&definition.ty), Type::List(_)) {
                    continue;
                }
                let id = ControlFlowDefinitionId::Node {
                    node: node.id,
                    index,
                };
                if borrowed_definitions.contains(&id) {
                    continue;
                }
                let Some(value_id) = definition_values.get(&id) else {
                    continue;
                };
                let Some(ControlFlowValue {
                    kind: ControlFlowValueKind::NameRead { definitions, .. },
                    ..
                }) = values.get(value_id.0)
                else {
                    continue;
                };
                if !definitions.is_empty()
                    && definitions
                        .iter()
                        .all(|source| borrowed_definitions.contains(source))
                {
                    changed |= borrowed_definitions.insert(id);
                }
            }
        }
        if !changed {
            break;
        }
    }

    for node in nodes {
        node.ownership.moves.retain(|ownership_move| {
            !node
                .definitions
                .iter()
                .enumerate()
                .any(|(index, definition)| {
                    definition.name == ownership_move.destination
                        && borrowed_definitions.contains(&ControlFlowDefinitionId::Node {
                            node: node.id,
                            index,
                        })
                })
        });
    }
}

fn populate_immutable_borrows(
    nodes: &mut [ControlFlowNode],
    values: &[ControlFlowValue],
    reachable_values: &BTreeSet<ControlFlowValueId>,
    signatures: &Signatures,
) {
    for value in values {
        if !reachable_values.contains(&value.id) || signatures.is_copy_type(&value.ty) {
            continue;
        }
        let ControlFlowValueKind::NameRead { name, .. } = &value.kind else {
            continue;
        };
        let Some(node) = nodes.get_mut(value.producer.0) else {
            continue;
        };
        if node.ownership.borrows.iter().any(|borrow| {
            borrow.source == *name
                && borrow.kind == OwnershipBorrowKind::Immutable
                && borrow.span == value.span
        }) {
            continue;
        }
        node.ownership.borrows.push(OwnershipBorrow {
            source: name.clone(),
            kind: OwnershipBorrowKind::Immutable,
            span: value.span,
        });
    }
    for node in nodes {
        node.ownership.borrows.sort_by(|left, right| {
            left.source
                .cmp(&right.source)
                .then_with(|| span_key(left.span).cmp(&span_key(right.span)))
        });
    }
}

fn compute_reachable_values(
    values: &[ControlFlowValue],
    uses: &[ControlFlowValueUse],
    regions: &[ControlFlowValueRegion],
    move_states_before: &[ControlFlowMoveState],
) -> BTreeSet<ControlFlowValueId> {
    let mut reachable = BTreeSet::new();
    let mut pending = values
        .iter()
        .filter(|value| {
            value.result_index.is_some()
                && move_states_before
                    .get(value.producer.0)
                    .is_some_and(ControlFlowMoveState::reachable)
        })
        .map(|value| value.id)
        .collect::<Vec<_>>();

    while let Some(user_id) = pending.pop() {
        if !reachable.insert(user_id) {
            continue;
        }
        if values.get(user_id.0).is_none() {
            continue;
        }
        for usage in uses.iter().filter(|usage| usage.user == user_id) {
            if value_use_is_reachable(*usage, regions, values) {
                pending.push(usage.value);
            }
        }
    }

    reachable
}

fn value_use_is_reachable(
    usage: ControlFlowValueUse,
    regions: &[ControlFlowValueRegion],
    values: &[ControlFlowValue],
) -> bool {
    let Some(region) = usage.region.and_then(|id| regions.get(id.0)) else {
        return true;
    };
    match region.kind {
        ControlFlowValueRegionKind::ShortCircuitRight {
            condition,
            execute_when,
        }
        | ControlFlowValueRegionKind::Branch {
            condition,
            selected_when: execute_when,
        } => match values
            .get(condition.0)
            .and_then(|value| value.source_constant.as_ref())
        {
            Some(ConstantValue::Bool(condition)) => *condition == execute_when,
            _ => true,
        },
        ControlFlowValueRegionKind::MatchArm { guard, .. } => !matches!(
            guard
                .and_then(|guard| values.get(guard.0))
                .and_then(|guard| guard.source_constant.as_ref()),
            Some(ConstantValue::Bool(false))
        ),
        ControlFlowValueRegionKind::OptionalFallback { .. }
        | ControlFlowValueRegionKind::OptionalPresent { .. }
        | ControlFlowValueRegionKind::MatchGuard { .. }
        | ControlFlowValueRegionKind::LoopCondition { .. }
        | ControlFlowValueRegionKind::LoopBody { .. }
        | ControlFlowValueRegionKind::DeferredBody => true,
    }
}

fn compute_definition_values(
    nodes: &[ControlFlowNode],
    edges: &[ControlFlowEdge],
) -> BTreeMap<ControlFlowDefinitionId, ControlFlowValueId> {
    let mut values = BTreeMap::new();
    for node in nodes {
        if node.definitions.len() != 1
            || !matches!(
                node.kind,
                ControlFlowNodeKind::Binding { .. } | ControlFlowNodeKind::Assignment { .. }
            )
        {
            continue;
        }
        let source_value = edges
            .iter()
            .filter(|edge| edge.to == node.id && edge.kind == ControlFlowEdgeKind::Next)
            .find_map(|edge| {
                let source = nodes.get(edge.from.0)?;
                match source.kind {
                    ControlFlowNodeKind::Evaluation(
                        ControlFlowEvaluationKind::BindingInitializer
                        | ControlFlowEvaluationKind::AssignmentValue,
                    ) => source.values.first().copied(),
                    _ => None,
                }
            });
        if let Some(value) = source_value {
            values.insert(
                ControlFlowDefinitionId::Node {
                    node: node.id,
                    index: 0,
                },
                value,
            );
        }
    }
    values
}

fn propagate_definition_constants(
    values: &mut [ControlFlowValue],
    definition_values: &BTreeMap<ControlFlowDefinitionId, ControlFlowValueId>,
) {
    loop {
        let constants = values
            .iter()
            .map(|value| value.constant.clone())
            .collect::<Vec<_>>();
        let mut changed = false;
        for value in values.iter_mut().filter(|value| value.constant.is_none()) {
            if let Some(constant) = propagated_ir_constant(value, &constants, definition_values) {
                value.constant = Some(constant);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

fn propagated_ir_constant(
    value: &ControlFlowValue,
    constants: &[Option<ConstantValue>],
    definition_values: &BTreeMap<ControlFlowDefinitionId, ControlFlowValueId>,
) -> Option<ConstantValue> {
    match &value.kind {
        ControlFlowValueKind::NameRead { definitions, .. } => {
            let mut resolved = None::<ConstantValue>;
            if definitions.is_empty() {
                return None;
            }
            for definition in definitions {
                let source = definition_values.get(definition)?;
                let constant = constants.get(source.0)?.clone()?;
                if resolved
                    .as_ref()
                    .is_some_and(|existing| existing != &constant)
                {
                    return None;
                }
                resolved = Some(constant);
            }
            resolved
        }
        ControlFlowValueKind::Unary { op, operand } => {
            let operand = constants.get(operand.0)?.clone()?;
            match (op, operand) {
                (UnaryOp::Neg, ConstantValue::I64(value)) => {
                    value.checked_neg().map(ConstantValue::I64)
                }
                (UnaryOp::Not, ConstantValue::Bool(value)) => Some(ConstantValue::Bool(!value)),
                _ => None,
            }
        }
        ControlFlowValueKind::Binary { op, left, right } => {
            let left_constant = constants.get(left.0)?.clone();
            let right_constant = constants.get(right.0)?.clone();
            match (op, left_constant.as_ref(), right_constant.as_ref()) {
                (BinOp::And, Some(ConstantValue::Bool(false)), _)
                | (BinOp::And, _, Some(ConstantValue::Bool(false))) => {
                    Some(ConstantValue::Bool(false))
                }
                (BinOp::Or, Some(ConstantValue::Bool(true)), _)
                | (BinOp::Or, _, Some(ConstantValue::Bool(true))) => {
                    Some(ConstantValue::Bool(true))
                }
                _ => typecheck::evaluate_constant_binary(
                    value.span,
                    *op,
                    left_constant?,
                    right_constant?,
                )
                .ok(),
            }
        }
        ControlFlowValueKind::Conditional {
            condition,
            then_value,
            else_value,
        } => match constants.get(condition.0)?.as_ref()? {
            ConstantValue::Bool(true) => constants.get(then_value.0)?.clone(),
            ConstantValue::Bool(false) => constants.get(else_value.0)?.clone(),
            _ => None,
        },
        _ => None,
    }
}

fn prune_constant_control_edges(
    nodes: &[ControlFlowNode],
    edges: &mut Vec<ControlFlowEdge>,
    values: &[ControlFlowValue],
) -> bool {
    let mut decisions = BTreeMap::<ControlFlowNodeId, bool>::new();
    for evaluation in nodes {
        if !matches!(
            evaluation.kind,
            ControlFlowNodeKind::Evaluation(ControlFlowEvaluationKind::Condition)
        ) {
            continue;
        }
        let Some(ConstantValue::Bool(condition)) = evaluation
            .values
            .first()
            .and_then(|id| values.get(id.0))
            .and_then(|value| value.constant.as_ref())
        else {
            continue;
        };
        for edge in edges.iter().filter(|edge| edge.from == evaluation.id) {
            if nodes.get(edge.to.0).is_some_and(|node| {
                matches!(
                    node.kind,
                    ControlFlowNodeKind::Conditional | ControlFlowNodeKind::Loop
                )
            }) {
                decisions.insert(edge.to, *condition);
            }
        }
    }
    if decisions.is_empty() {
        return false;
    }
    let old_len = edges.len();
    edges.retain(|edge| {
        let Some(condition) = decisions.get(&edge.from) else {
            return true;
        };
        match edge.kind {
            ControlFlowEdgeKind::True => *condition,
            ControlFlowEdgeKind::False => !*condition,
            _ => true,
        }
    });
    edges.len() != old_len
}

type ReachingDefinitionMap = BTreeMap<String, BTreeSet<ControlFlowDefinitionId>>;

fn compute_reaching_definitions(
    nodes: &[ControlFlowNode],
    edges: &[ControlFlowEdge],
    parameters: &[ControlFlowParameter],
    entry: ControlFlowNodeId,
) -> Vec<Option<ReachingDefinitionMap>> {
    let mut initial = ReachingDefinitionMap::new();
    for (index, parameter) in parameters.iter().enumerate() {
        initial
            .entry(parameter.name.clone())
            .or_default()
            .insert(ControlFlowDefinitionId::Parameter(index));
    }

    let mut before = vec![None; nodes.len()];
    before[entry.0] = Some(initial);
    let mut queue = VecDeque::from([entry]);
    while let Some(id) = queue.pop_front() {
        let Some(mut outgoing) = before[id.0].clone() else {
            continue;
        };
        for (index, definition) in nodes[id.0].definitions.iter().enumerate() {
            outgoing.insert(
                definition.name.clone(),
                BTreeSet::from([ControlFlowDefinitionId::Node { node: id, index }]),
            );
        }
        for edge in edges.iter().filter(|edge| edge.from == id) {
            let target = edge.to.0;
            let changed = match before[target].as_mut() {
                Some(existing) => merge_reaching_definitions(existing, &outgoing),
                None => {
                    before[target] = Some(outgoing.clone());
                    true
                }
            };
            if changed {
                queue.push_back(edge.to);
            }
        }
    }
    before
}

fn merge_reaching_definitions(
    target: &mut ReachingDefinitionMap,
    incoming: &ReachingDefinitionMap,
) -> bool {
    let mut changed = false;
    for (name, definitions) in incoming {
        let target_definitions = target.entry(name.clone()).or_default();
        let old_len = target_definitions.len();
        target_definitions.extend(definitions.iter().copied());
        changed |= target_definitions.len() != old_len;
    }
    changed
}

fn resolve_name_read_definitions(
    values: &mut [ControlFlowValue],
    reaching_definitions: &[Option<ReachingDefinitionMap>],
) {
    for value in values {
        let ControlFlowValueKind::NameRead { name, definitions } = &mut value.kind else {
            continue;
        };
        if definitions
            .iter()
            .any(|definition| matches!(definition, ControlFlowDefinitionId::Scoped { .. }))
        {
            continue;
        }
        *definitions = reaching_definitions
            .get(value.producer.0)
            .and_then(Option::as_ref)
            .and_then(|state| state.get(name))
            .map(|definitions| definitions.iter().copied().collect())
            .unwrap_or_default();
    }
}

fn compute_liveness(
    nodes: &[ControlFlowNode],
    edges: &[ControlFlowEdge],
    parameters: &[ControlFlowParameter],
) -> (Vec<ControlFlowLiveState>, Vec<ControlFlowLiveState>) {
    let mut tracked = parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .collect::<BTreeSet<_>>();
    for node in nodes {
        tracked.extend(
            node.definitions
                .iter()
                .map(|definition| definition.name.clone()),
        );
    }

    let mut before = vec![BTreeSet::<String>::new(); nodes.len()];
    let mut after = vec![BTreeSet::<String>::new(); nodes.len()];
    loop {
        let mut changed = false;
        for node in nodes.iter().rev() {
            let mut next_after = BTreeSet::new();
            for edge in edges.iter().filter(|edge| edge.from == node.id) {
                next_after.extend(before[edge.to.0].iter().cloned());
            }

            let mut next_before = next_after.clone();
            for definition in &node.definitions {
                next_before.remove(&definition.name);
            }
            next_before.extend(
                node.ownership
                    .reads
                    .iter()
                    .filter(|name| tracked.contains(*name))
                    .cloned(),
            );

            if next_after != after[node.id.0] {
                after[node.id.0] = next_after;
                changed = true;
            }
            if next_before != before[node.id.0] {
                before[node.id.0] = next_before;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let into_state = |bindings: BTreeSet<String>| ControlFlowLiveState {
        live: bindings.into_iter().collect(),
    };
    (
        before.into_iter().map(into_state).collect(),
        after.into_iter().map(into_state).collect(),
    )
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
        for definition in &nodes[id.0].definitions {
            outgoing_state.remove(&definition.name);
        }
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
