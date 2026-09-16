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
    /// An explicit suspension boundary around the awaited value.  Keeping the
    /// inner call as a normal value preserves its argument provenance while
    /// making suspension visible to typed-IR consumers.
    Await {
        value: ControlFlowValueId,
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
    Set {
        items: Vec<ControlFlowValueId>,
    },
    Map {
        entries: Vec<(ControlFlowValueId, ControlFlowValueId)>,
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
        optional: bool,
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
    RecordLiteral {
        fields: Vec<(Option<String>, ControlFlowValueId)>,
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
    /// Ownership classification for the produced value.  Keeping this next
    /// to the typed value means ownership consumers do not need to rediscover
    /// whether a value is a copy or a borrowed collection descriptor from the
    /// source AST.  Owned values are intentionally not represented yet: the
    /// bootstrap type system rejects them until transfer and drop semantics
    /// are available.
    pub ownership: ControlFlowValueOwnership,
    pub span: SourceSpan,
    pub kind: ControlFlowValueKind,
    pub source_constant: Option<ConstantValue>,
    pub constant: Option<ConstantValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ControlFlowValueOwnership {
    Copy,
    ImmutableBorrow,
}

impl ControlFlowValueOwnership {
    pub const fn is_copy(self) -> bool {
        matches!(self, Self::Copy)
    }

    pub const fn is_borrow(self) -> bool {
        matches!(self, Self::ImmutableBorrow)
    }
}

/// The evaluation effect of a typed value.
///
/// This deliberately has only a conservative two-point lattice for now.  A
/// value is pure only when every operation needed to produce it is known to be
/// pure; calls, suspension, and opaque values are observable by default.  The
/// normalized query is useful to later explicit-effects checking and backend
/// scheduling without making either of them re-walk checked AST expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ControlFlowValueEffect {
    Pure,
    MayEffect,
}

/// The normalized direct effect summary for one function body.
///
/// This is deliberately not a transitive effect result yet: callees remain
/// named so a whole-program solver can handle recursion, function values, and
/// platform capabilities without making the per-function CFG rebuild another
/// AST-shaped call graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFlowEffectSummary {
    pub intrinsic_effect: bool,
    pub direct_callees: BTreeSet<String>,
}

impl ControlFlowEffectSummary {
    pub fn is_locally_pure(&self) -> bool {
        !self.intrinsic_effect && self.direct_callees.is_empty()
    }
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
    /// A non-empty path records a partial move from a value projection (for
    /// example `record.payload`).  The root `source` remains the stable name
    /// used by reaching-definition analysis; ownership consumers must retain
    /// the path so moving one field cannot be mistaken for moving the whole
    /// aggregate.
    pub projection: Vec<String>,
    /// The exact typed value consumed by this move boundary when the source
    /// is represented in the normalized value graph.  Keeping this alongside
    /// the source name and reaching definitions lets future partial-move and
    /// owned-value analysis follow projections without rebuilding an AST walk.
    pub value: Option<ControlFlowValueId>,
    /// Exact source definitions reaching this move boundary.  The source
    /// name is retained for diagnostics, but ownership consumers must use
    /// these identities so shadowed/re-executed bindings cannot be conflated.
    pub source_definitions: Vec<ControlFlowDefinitionId>,
    pub span: SourceSpan,
}

impl OwnershipMove {
    /// Whether this event transfers only a named projection of its source.
    ///
    /// Keeping this predicate on the normalized fact avoids making ownership
    /// consumers interpret an empty/non-empty path themselves, and gives
    /// future aggregate move checking one stable boundary to query.
    pub fn is_partial(&self) -> bool {
        !self.projection.is_empty()
    }

    /// Whether this move consumes the same whole value or an overlapping
    /// projection as `projection`.
    ///
    /// An empty path represents the complete value.  Keeping the prefix
    /// relation on the normalized ownership fact gives later owned-aggregate
    /// analyses one rule for whole and partial moves, including moves coming
    /// from different CFG paths.
    pub fn overlaps_projection(&self, projection: &[String]) -> bool {
        self.projection.is_empty()
            || projection.is_empty()
            || self.projection.starts_with(projection)
            || projection.starts_with(&self.projection)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipBorrowKind {
    Immutable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipBorrow {
    pub source: String,
    pub kind: OwnershipBorrowKind,
    /// The exact typed value whose evaluation created this borrow.  Keeping
    /// the value identity alongside the reaching source definitions lets
    /// ownership consumers correlate borrow provenance with projections and
    /// other normalized value facts without rebuilding an AST read.
    pub value: Option<ControlFlowValueId>,
    /// Exact source definitions reaching this borrow.  The source name is
    /// retained for diagnostics, but ownership consumers must use these
    /// identities so shadowed and re-executed bindings cannot be conflated.
    pub source_definitions: Vec<ControlFlowDefinitionId>,
    pub span: SourceSpan,
}

/// A typed call boundary recorded alongside normalized ownership facts.
///
/// The arguments deliberately remain value IDs rather than source names. This
/// keeps future consuming-call analysis tied to the same reaching-definition
/// and value provenance facts used by borrow checking instead of rebuilding a
/// second call walk from the checked AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipCall {
    pub callee: String,
    pub arguments: Vec<ControlFlowValueId>,
    /// Ownership mode for each argument at this call boundary. `Consuming`
    /// is reserved for a future owned-value ABI; bootstrap calls are either
    /// ordinary `Copy` values or immutable collection/view borrows.
    pub argument_kinds: Vec<OwnershipCallArgumentKind>,
    /// Source definitions reached by each argument at this call boundary.
    /// This remains parallel to `arguments` for future consuming-call checks.
    pub argument_definitions: Vec<Vec<ControlFlowDefinitionId>>,
    /// Definitions whose storage is immutably borrowed by each argument.
    /// Copy arguments deliberately stay empty; keeping this separate from
    /// `argument_definitions` lets future consuming-call checks distinguish a
    /// projected non-copy borrow from ordinary scalar provenance without
    /// re-walking the typed AST.
    pub borrowed_argument_definitions: Vec<Vec<ControlFlowDefinitionId>>,
    pub span: SourceSpan,
}

impl OwnershipCall {
    /// Returns the ownership mode for one typed argument boundary.
    ///
    /// Keeping this lookup on the normalized call fact prevents consumers from
    /// silently assuming that the parallel argument vectors have identical
    /// lengths. Malformed/synthetic graphs conservatively report no mode.
    pub fn argument_kind(&self, index: usize) -> Option<OwnershipCallArgumentKind> {
        self.argument_kinds.get(index).copied()
    }

    /// Returns the exact reaching definitions for one argument.
    pub fn argument_definitions_at(&self, index: usize) -> &[ControlFlowDefinitionId] {
        self.argument_definitions
            .get(index)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Returns the definitions borrowed by one argument, if it is an
    /// immutable non-copy boundary.
    pub fn borrowed_argument_definitions_at(&self, index: usize) -> &[ControlFlowDefinitionId] {
        self.borrowed_argument_definitions
            .get(index)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Returns the exact reaching definitions consumed by one argument, if
    /// this call boundary transfers ownership for that argument.
    ///
    /// Keeping the mode check next to the parallel ownership vectors prevents
    /// future consumers from accidentally treating an immutable borrow as a
    /// move merely because it has source-definition provenance.
    pub fn consuming_argument_definitions_at(&self, index: usize) -> &[ControlFlowDefinitionId] {
        (self.argument_kind(index) == Some(OwnershipCallArgumentKind::Consuming))
            .then(|| self.argument_definitions_at(index))
            .unwrap_or_default()
    }

    pub fn is_consuming(&self) -> bool {
        self.argument_kinds
            .iter()
            .any(|kind| *kind == OwnershipCallArgumentKind::Consuming)
    }
}

/// Ownership facts for a value crossing a function return boundary.
///
/// Return values are kept as typed value IDs, just like call arguments, so a
/// future owned-value ABI can validate transfers from the normalized value
/// graph.  In particular, a borrowed list result remains distinguishable from
/// an ordinary `Copy` result instead of requiring a second AST walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipReturn {
    pub value: ControlFlowValueId,
    pub kind: OwnershipCallArgumentKind,
    pub definitions: Vec<ControlFlowDefinitionId>,
    pub borrowed_definitions: Vec<ControlFlowDefinitionId>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipCallArgumentKind {
    Copy,
    ImmutableBorrow,
    Consuming,
}

/// A normalized point at which a non-copy definition's storage is no longer
/// needed by any reachable successor and may be released.
///
/// The bootstrap list representation does not own heap storage yet, so these
/// facts do not emit a native destructor call. Keeping the fact on the typed
/// CFG nevertheless makes the eventual drop/owned-resource lowering consume
/// the same reaching-definition and borrow regions as move validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipDrop {
    pub definition: ControlFlowDefinitionId,
    /// The normalized value produced for this definition, when one exists.
    /// Keeping the value identity here lets future destructors consume the
    /// same typed value graph as moves, calls, and returns instead of
    /// reconstructing a value from its source name.
    pub value: Option<ControlFlowValueId>,
    pub name: String,
    pub span: SourceSpan,
}

/// One normalized ownership event at a CFG node.
///
/// Consumers that need the complete ownership boundary for a node can use
/// this view instead of independently walking the parallel borrow/move/call/
/// return/drop vectors.  The references deliberately preserve the typed
/// event payloads; no source-AST reconstruction or lossy name-only event is
/// involved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlowOwnershipEvent<'a> {
    Borrow(&'a OwnershipBorrow),
    Move(&'a OwnershipMove),
    Call(&'a OwnershipCall),
    Return(&'a OwnershipReturn),
    Drop(&'a OwnershipDrop),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlFlowOwnership {
    pub reads: Vec<String>,
    pub borrows: Vec<OwnershipBorrow>,
    pub moves: Vec<OwnershipMove>,
    pub calls: Vec<OwnershipCall>,
    /// Typed ownership facts for values crossing a return boundary.
    pub returns: Vec<OwnershipReturn>,
    /// Definition-scoped releases whose boundary is this normalized node.
    ///
    /// These are populated after liveness/borrow analysis. Keeping them on
    /// the node makes ownership lowering consume the same normalized event
    /// stream as reads, moves, borrows, and calls instead of maintaining a
    /// second graph walk over the checked AST.
    pub drops: Vec<OwnershipDrop>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipMovedBinding {
    pub definition: ControlFlowDefinitionId,
    pub name: String,
    /// The named aggregate projection consumed by this move. An empty path
    /// means the complete value was consumed; a non-empty path means only
    /// that field projection was consumed.
    pub projection: Vec<String>,
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
            .filter(|binding| binding.name == name)
            .map(|binding| binding.origin)
            .min_by_key(|origin| span_key(*origin))
    }

    pub fn is_definition_moved(&self, definition: ControlFlowDefinitionId) -> bool {
        self.origin_for_definition(definition).is_some()
    }

    /// Returns whether a whole value or an overlapping projection of the
    /// definition has been consumed. A whole-value move overlaps every
    /// projection; two partial moves overlap when one path is a prefix of
    /// the other. This keeps future partial-move checking on the normalized
    /// state rather than forcing it to reconstruct field paths from syntax.
    pub fn is_definition_projection_moved(
        &self,
        definition: ControlFlowDefinitionId,
        projection: &[String],
    ) -> bool {
        self.moved.iter().any(|binding| {
            binding.definition == definition
                && moved_projection_overlaps(&binding.projection, projection)
        })
    }

    /// Return whether the complete value for `definition` has been consumed.
    ///
    /// This deliberately differs from `is_definition_projection_moved`: a
    /// partial field move must not be reported as a whole-value move, because
    /// future aggregate ownership checking may still permit unrelated fields
    /// to be read or transferred. Keeping the distinction in normalized move
    /// state prevents each ownership consumer from interpreting empty paths
    /// independently.
    pub fn is_definition_whole_moved(&self, definition: ControlFlowDefinitionId) -> bool {
        self.moved
            .iter()
            .any(|binding| binding.definition == definition && binding.projection.is_empty())
    }

    /// Return whether at least one partial projection of `definition` has
    /// been consumed, excluding a whole-value move.
    pub fn is_definition_partially_moved(&self, definition: ControlFlowDefinitionId) -> bool {
        self.moved
            .iter()
            .any(|binding| binding.definition == definition && !binding.projection.is_empty())
    }

    /// Return the exact consumed projections for one reaching definition.
    ///
    /// This is a borrowed view so ownership checking and later lowering can
    /// inspect the fixed-point result without allocating a second
    /// syntax-shaped representation.  The returned bindings retain their
    /// definition identity and source span for diagnostics.
    pub fn moved_projections_for_definition(
        &self,
        definition: ControlFlowDefinitionId,
    ) -> impl Iterator<Item = &OwnershipMovedBinding> {
        self.moved
            .iter()
            .filter(move |binding| binding.definition == definition)
    }

    pub fn origin_for_definition(&self, definition: ControlFlowDefinitionId) -> Option<SourceSpan> {
        self.moved
            .iter()
            .find(|binding| binding.definition == definition)
            .map(|binding| binding.origin)
    }
}

fn moved_projection_overlaps(left: &[String], right: &[String]) -> bool {
    left.is_empty() || right.is_empty() || left.starts_with(right) || right.starts_with(left)
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlFlowDefinitionLiveState {
    live: Vec<ControlFlowDefinitionId>,
}

impl ControlFlowDefinitionLiveState {
    pub fn live(&self) -> &[ControlFlowDefinitionId] {
        &self.live
    }

    pub fn contains(&self, definition: ControlFlowDefinitionId) -> bool {
        self.live.contains(&definition)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipBorrowedBinding {
    pub definition: ControlFlowDefinitionId,
    pub borrower: String,
    pub source_definition: ControlFlowDefinitionId,
    pub source: String,
    pub origin: SourceSpan,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlFlowBorrowState {
    borrows: Vec<OwnershipBorrowedBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipBorrowStart {
    pub from: ControlFlowNodeId,
    pub to: ControlFlowNodeId,
    pub definition: ControlFlowDefinitionId,
    pub borrower: String,
    pub source_definition: ControlFlowDefinitionId,
    pub source: String,
    pub origin: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipBorrowEnd {
    pub from: ControlFlowNodeId,
    pub to: ControlFlowNodeId,
    pub definition: ControlFlowDefinitionId,
    pub borrower: String,
    pub source_definition: ControlFlowDefinitionId,
    pub source: String,
    pub origin: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipBorrowLifetime {
    pub definition: ControlFlowDefinitionId,
    pub borrower: String,
    pub source_definition: ControlFlowDefinitionId,
    pub source: String,
    pub origin: SourceSpan,
    pub active_before: Vec<ControlFlowNodeId>,
    pub starts: Vec<OwnershipBorrowStart>,
    pub ends: Vec<OwnershipBorrowEnd>,
}

impl ControlFlowBorrowState {
    pub fn borrows(&self) -> &[OwnershipBorrowedBinding] {
        &self.borrows
    }

    pub fn contains(&self, borrower: &str, source: &str) -> bool {
        self.borrows
            .iter()
            .any(|borrow| borrow.borrower == borrower && borrow.source == source)
    }

    fn contains_definition(
        &self,
        definition: ControlFlowDefinitionId,
        source_definition: ControlFlowDefinitionId,
    ) -> bool {
        self.borrows.iter().any(|borrow| {
            borrow.definition == definition && borrow.source_definition == source_definition
        })
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
    escaping_values: BTreeSet<ControlFlowValueId>,
    scoped_definitions: Vec<Vec<ControlFlowDefinition>>,
    definition_values: BTreeMap<ControlFlowDefinitionId, ControlFlowValueId>,
    scoped_borrow_sources: BTreeMap<ControlFlowDefinitionId, ControlFlowValueId>,
    reaching_definitions_before: Vec<Option<ReachingDefinitionMap>>,
    move_states_before: Vec<ControlFlowMoveState>,
    live_before: Vec<ControlFlowLiveState>,
    live_after: Vec<ControlFlowLiveState>,
    definition_live_before: Vec<ControlFlowDefinitionLiveState>,
    definition_live_after: Vec<ControlFlowDefinitionLiveState>,
    borrow_states_before: Vec<ControlFlowBorrowState>,
    borrow_starts: Vec<OwnershipBorrowStart>,
    borrow_ends: Vec<OwnershipBorrowEnd>,
    borrow_lifetimes: Vec<OwnershipBorrowLifetime>,
    drops: Vec<(ControlFlowNodeId, OwnershipDrop)>,
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

    /// Return the normalized ownership class for a typed value.
    pub fn value_ownership(&self, id: ControlFlowValueId) -> Option<ControlFlowValueOwnership> {
        self.value(id).map(|value| value.ownership)
    }

    /// Return whether a reachable typed value is a pure scalar expression.
    ///
    /// This is deliberately a graph query rather than a codegen-side AST
    /// walk.  Backend consumers can therefore use the same value/dependency
    /// facts that drove reachability and constant propagation, including
    /// definition-scoped name reads and conditional expressions.
    pub fn is_pure_scalar_value(&self, id: ControlFlowValueId) -> bool {
        if !self.is_value_reachable(id) {
            return false;
        }
        fn visit(
            graph: &ControlFlowGraph,
            id: ControlFlowValueId,
            visiting: &mut BTreeSet<ControlFlowValueId>,
        ) -> bool {
            if !visiting.insert(id) {
                return false;
            }
            let Some(value) = graph.value(id) else {
                return false;
            };
            let pure = match &value.kind {
                ControlFlowValueKind::Literal => true,
                ControlFlowValueKind::NameRead { definitions, .. } => {
                    !definitions.is_empty()
                        && definitions.iter().all(|definition| {
                            graph
                                .definition_value(*definition)
                                .is_some_and(|value| visit(graph, value, visiting))
                        })
                }
                ControlFlowValueKind::Unary { operand, .. } => visit(graph, *operand, visiting),
                ControlFlowValueKind::Binary { left, right, .. } => {
                    visit(graph, *left, visiting) && visit(graph, *right, visiting)
                }
                ControlFlowValueKind::Conditional {
                    condition,
                    then_value,
                    else_value,
                } => {
                    visit(graph, *condition, visiting)
                        && visit(graph, *then_value, visiting)
                        && visit(graph, *else_value, visiting)
                }
                _ => false,
            };
            visiting.remove(&id);
            pure
        }

        visit(self, id, &mut BTreeSet::new())
    }

    /// Classify the evaluation of a reachable typed value using its graph
    /// dependencies.  Unknown or effectful producers are conservatively
    /// classified as `MayEffect`; unreachable values do not provide an
    /// optimization/effects proof.
    pub fn value_effect(&self, id: ControlFlowValueId) -> Option<ControlFlowValueEffect> {
        if !self.is_value_reachable(id) {
            return None;
        }

        fn visit(
            graph: &ControlFlowGraph,
            id: ControlFlowValueId,
            visiting: &mut BTreeSet<ControlFlowValueId>,
        ) -> ControlFlowValueEffect {
            if !visiting.insert(id) {
                return ControlFlowValueEffect::MayEffect;
            }
            let Some(value) = graph.value(id) else {
                return ControlFlowValueEffect::MayEffect;
            };
            fn all_pure(
                graph: &ControlFlowGraph,
                children: impl IntoIterator<Item = ControlFlowValueId>,
                visiting: &mut BTreeSet<ControlFlowValueId>,
            ) -> bool {
                children
                    .into_iter()
                    .all(|child| visit(graph, child, visiting) == ControlFlowValueEffect::Pure)
            }
            let effect = match &value.kind {
                ControlFlowValueKind::Literal | ControlFlowValueKind::AnonymousFunction { .. } => {
                    ControlFlowValueEffect::Pure
                }
                ControlFlowValueKind::NameRead { definitions, .. } => {
                    if definitions.is_empty() {
                        ControlFlowValueEffect::MayEffect
                    } else {
                        let values = definitions
                            .iter()
                            .filter_map(|definition| graph.definition_value(*definition))
                            .collect::<Vec<_>>();
                        if values.len() == definitions.len() && all_pure(graph, values, visiting) {
                            ControlFlowValueEffect::Pure
                        } else {
                            ControlFlowValueEffect::MayEffect
                        }
                    }
                }
                ControlFlowValueKind::Await { .. }
                | ControlFlowValueKind::Call { .. }
                | ControlFlowValueKind::OptionalCascadeCall { .. }
                | ControlFlowValueKind::QualifiedCall { .. }
                | ControlFlowValueKind::InterfaceDispatch { .. }
                | ControlFlowValueKind::Opaque => ControlFlowValueEffect::MayEffect,
                ControlFlowValueKind::InterfacePack { value, .. }
                | ControlFlowValueKind::ListSpread { value }
                | ControlFlowValueKind::ListOptional { value }
                | ControlFlowValueKind::Field { base: value, .. }
                | ControlFlowValueKind::Unary { operand: value, .. } => {
                    visit(graph, *value, visiting)
                }
                ControlFlowValueKind::List { items } => {
                    all_pure(graph, items.iter().copied(), visiting)
                        .then_some(ControlFlowValueEffect::Pure)
                        .unwrap_or(ControlFlowValueEffect::MayEffect)
                }
                ControlFlowValueKind::Set { items } => {
                    all_pure(graph, items.iter().copied(), visiting)
                        .then_some(ControlFlowValueEffect::Pure)
                        .unwrap_or(ControlFlowValueEffect::MayEffect)
                }
                ControlFlowValueKind::Map { entries } => all_pure(
                    graph,
                    entries.iter().flat_map(|(key, value)| [*key, *value]),
                    visiting,
                )
                .then_some(ControlFlowValueEffect::Pure)
                .unwrap_or(ControlFlowValueEffect::MayEffect),
                ControlFlowValueKind::ListIf {
                    condition,
                    value,
                    else_value,
                } => all_pure(
                    graph,
                    std::iter::once(*condition)
                        .chain(std::iter::once(*value))
                        .chain(else_value.iter().copied()),
                    visiting,
                )
                .then_some(ControlFlowValueEffect::Pure)
                .unwrap_or(ControlFlowValueEffect::MayEffect),
                ControlFlowValueKind::Index { base, index, .. } => {
                    if all_pure(graph, [*base, *index], visiting) {
                        ControlFlowValueEffect::Pure
                    } else {
                        ControlFlowValueEffect::MayEffect
                    }
                }
                ControlFlowValueKind::Slice {
                    base,
                    start,
                    end,
                    step,
                } => all_pure(
                    graph,
                    std::iter::once(*base)
                        .chain(start.iter().copied())
                        .chain(end.iter().copied())
                        .chain(step.iter().copied()),
                    visiting,
                )
                .then_some(ControlFlowValueEffect::Pure)
                .unwrap_or(ControlFlowValueEffect::MayEffect),
                ControlFlowValueKind::ListComprehension {
                    iterable,
                    value,
                    condition,
                } => all_pure(
                    graph,
                    std::iter::once(*iterable)
                        .chain(std::iter::once(*value))
                        .chain(condition.iter().copied()),
                    visiting,
                )
                .then_some(ControlFlowValueEffect::Pure)
                .unwrap_or(ControlFlowValueEffect::MayEffect),
                ControlFlowValueKind::StructLiteral { base, fields, .. } => all_pure(
                    graph,
                    base.iter()
                        .copied()
                        .chain(fields.iter().map(|(_, value)| *value)),
                    visiting,
                )
                .then_some(ControlFlowValueEffect::Pure)
                .unwrap_or(ControlFlowValueEffect::MayEffect),
                ControlFlowValueKind::RecordLiteral { fields } => {
                    all_pure(graph, fields.iter().map(|(_, value)| *value), visiting)
                        .then_some(ControlFlowValueEffect::Pure)
                        .unwrap_or(ControlFlowValueEffect::MayEffect)
                }
                ControlFlowValueKind::Match {
                    value,
                    guards,
                    arms,
                }
                | ControlFlowValueKind::ListMatch {
                    value,
                    guards,
                    arms,
                } => all_pure(
                    graph,
                    std::iter::once(*value)
                        .chain(guards.iter().flatten().copied())
                        .chain(arms.iter().copied()),
                    visiting,
                )
                .then_some(ControlFlowValueEffect::Pure)
                .unwrap_or(ControlFlowValueEffect::MayEffect),
                ControlFlowValueKind::Conditional {
                    condition,
                    then_value,
                    else_value,
                } => {
                    if all_pure(graph, [*condition, *then_value, *else_value], visiting) {
                        ControlFlowValueEffect::Pure
                    } else {
                        ControlFlowValueEffect::MayEffect
                    }
                }
                ControlFlowValueKind::Binary { left, right, .. } => {
                    if all_pure(graph, [*left, *right], visiting) {
                        ControlFlowValueEffect::Pure
                    } else {
                        ControlFlowValueEffect::MayEffect
                    }
                }
            };
            visiting.remove(&id);
            effect
        }

        Some(visit(self, id, &mut BTreeSet::new()))
    }

    /// Classify the deferred body of an anonymous function value.
    ///
    /// Creating a function value is pure even when invoking it may have
    /// observable effects.  This query lets effect-aware consumers inspect
    /// that deferred work without incorrectly treating closure construction
    /// itself as effectful or re-walking the checked AST.
    pub fn deferred_body_effect(&self, id: ControlFlowValueId) -> Option<ControlFlowValueEffect> {
        let ControlFlowValueKind::AnonymousFunction { body } = self.value(id)?.kind else {
            return None;
        };
        self.value_effect(body)
    }

    /// Return the direct named calls made by reachable typed values.
    ///
    /// This is intentionally a graph query rather than an AST walk. The
    /// result is deterministic and excludes calls in unreachable constant
    /// branches, which lets an effect checker build a call graph from the
    /// same reachability facts used by optimization. Anonymous function
    /// creation is not itself a call; its deferred body is represented by a
    /// separate value and can be queried with deferred_body_effect.
    pub fn direct_call_callees(&self) -> BTreeSet<String> {
        self.values
            .iter()
            .filter(|value| self.is_value_reachable(value.id))
            .flat_map(|value| match &value.kind {
                ControlFlowValueKind::Call { callee, .. }
                | ControlFlowValueKind::OptionalCascadeCall { callee, .. } => {
                    std::iter::once(callee.clone()).collect::<BTreeSet<_>>()
                }
                ControlFlowValueKind::QualifiedCall {
                    namespace, name, ..
                } => std::iter::once(format!("{namespace}.{name}")).collect(),
                ControlFlowValueKind::InterfaceDispatch {
                    interface,
                    capability,
                    ..
                } => std::iter::once(format!("{interface}.{capability}")).collect(),
                _ => BTreeSet::new(),
            })
            .collect()
    }

    /// Whether reachable evaluation in this function contains an effect that
    /// is intrinsic to the current value graph, independent of callees.
    ///
    /// Calls are reported through direct_call_callees so a future whole
    /// program effect solver can distinguish a pure user function from an
    /// effectful callee without treating every call as permanently opaque.
    pub fn has_intrinsic_effect(&self) -> bool {
        self.values.iter().any(|value| {
            self.is_value_reachable(value.id)
                && matches!(
                    value.kind,
                    ControlFlowValueKind::Await { .. }
                        | ControlFlowValueKind::Opaque
                        | ControlFlowValueKind::InterfaceDispatch { .. }
                )
        })
    }

    /// Summarize the direct effect boundary of this normalized function graph.
    pub fn effect_summary(&self) -> ControlFlowEffectSummary {
        ControlFlowEffectSummary {
            intrinsic_effect: self.has_intrinsic_effect(),
            direct_callees: self.direct_call_callees(),
        }
    }

    /// Return the constant proven safe for direct native emission, if any.
    ///
    /// Keeping reachability, scalar-shape, purity, and constant propagation
    /// in this query prevents a backend from accidentally inlining a value
    /// merely because its source expression happens to look constant.
    pub fn proven_scalar_constant(&self, id: ControlFlowValueId) -> Option<&ConstantValue> {
        let value = self.value(id)?;
        if !matches!(value.ty, Type::I64 | Type::Bool | Type::Str) || !self.is_pure_scalar_value(id)
        {
            return None;
        }
        value.constant.as_ref()
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

    pub fn value_escapes(&self, id: ControlFlowValueId) -> bool {
        self.escaping_values.contains(&id)
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

    pub fn definition_live_before(
        &self,
        id: ControlFlowNodeId,
    ) -> Option<&ControlFlowDefinitionLiveState> {
        self.definition_live_before.get(id.0)
    }

    pub fn definition_live_after(
        &self,
        id: ControlFlowNodeId,
    ) -> Option<&ControlFlowDefinitionLiveState> {
        self.definition_live_after.get(id.0)
    }

    pub fn borrow_state_before(&self, id: ControlFlowNodeId) -> Option<&ControlFlowBorrowState> {
        self.borrow_states_before.get(id.0)
    }

    /// Returns the normalized immutable-borrow events attached to one CFG
    /// evaluation node, including their exact reaching source definitions.
    /// This node-local query keeps ownership consumers on the typed IR event
    /// stream instead of making them scan the whole graph or re-resolve names.
    pub fn borrows_at(&self, id: ControlFlowNodeId) -> Option<&[OwnershipBorrow]> {
        self.node(id).map(|node| node.ownership.borrows.as_slice())
    }

    pub fn borrow_starts(&self) -> &[OwnershipBorrowStart] {
        &self.borrow_starts
    }

    pub fn borrow_ends(&self) -> &[OwnershipBorrowEnd] {
        &self.borrow_ends
    }

    pub fn borrow_lifetimes(&self) -> &[OwnershipBorrowLifetime] {
        &self.borrow_lifetimes
    }

    pub fn borrow_lifetimes_before(
        &self,
        id: ControlFlowNodeId,
    ) -> impl Iterator<Item = &OwnershipBorrowLifetime> {
        self.borrow_lifetimes
            .iter()
            .filter(move |lifetime| lifetime.active_before.contains(&id))
    }

    /// Return live immutable-borrow lifetimes rooted at one exact source
    /// definition before a CFG node.  Ownership consumers should prefer this
    /// identity-based query over matching the diagnostic source name: lexical
    /// shadowing and loop re-execution may give unrelated definitions the same
    /// spelling.
    pub fn borrow_lifetimes_before_source_definition(
        &self,
        id: ControlFlowNodeId,
        source_definition: ControlFlowDefinitionId,
    ) -> impl Iterator<Item = &OwnershipBorrowLifetime> {
        self.borrow_lifetimes_before(id)
            .filter(move |lifetime| lifetime.source_definition == source_definition)
    }

    /// Returns the normalized drop facts attached to a CFG evaluation node.
    /// Facts are ordered by node and definition identity for deterministic IR
    /// consumers.
    pub fn drops(&self) -> &[(ControlFlowNodeId, OwnershipDrop)] {
        &self.drops
    }

    /// Returns the normalized releases at one CFG node.
    pub fn drops_at(&self, id: ControlFlowNodeId) -> Option<&[OwnershipDrop]> {
        self.node(id).map(|node| node.ownership.drops.as_slice())
    }

    /// Return every typed ownership call boundary in normalized source order.
    /// Consumers that need both immutable borrows and consuming calls should
    /// use this complete event stream rather than filtering to transfers.
    pub fn ownership_calls(&self) -> impl Iterator<Item = (ControlFlowNodeId, &OwnershipCall)> {
        self.nodes
            .iter()
            .flat_map(|node| node.ownership.calls.iter().map(move |call| (node.id, call)))
    }

    /// Return every typed ownership call attached to one normalized node.
    pub fn ownership_calls_at(
        &self,
        id: ControlFlowNodeId,
    ) -> impl Iterator<Item = &OwnershipCall> {
        self.node(id)
            .into_iter()
            .flat_map(|node| node.ownership.calls.iter())
    }

    /// Return normalized whole-value and projected move events in source/CFG
    /// order.
    pub fn ownership_moves(&self) -> impl Iterator<Item = (ControlFlowNodeId, &OwnershipMove)> {
        self.nodes.iter().flat_map(|node| {
            node.ownership
                .moves
                .iter()
                .map(move |movement| (node.id, movement))
        })
    }

    /// Return all move events attached to one normalized CFG node.
    pub fn ownership_moves_at(
        &self,
        id: ControlFlowNodeId,
    ) -> impl Iterator<Item = &OwnershipMove> {
        self.node(id)
            .into_iter()
            .flat_map(|node| node.ownership.moves.iter())
    }

    /// Return consuming call boundaries in normalized source order.
    ///
    /// Ownership consumers can use this typed query instead of reconstructing
    /// consuming calls from the checked AST. Bootstrap currently has only
    /// `drop` as a consuming call, but the argument mode keeps this boundary
    /// extensible for future owned values and resources.
    pub fn consuming_calls(&self) -> impl Iterator<Item = (ControlFlowNodeId, &OwnershipCall)> {
        self.nodes.iter().flat_map(|node| {
            node.ownership
                .calls
                .iter()
                .filter(|call| call.is_consuming())
                .map(move |call| (node.id, call))
        })
    }

    /// Return consuming calls attached to one normalized evaluation node.
    pub fn consuming_calls_at(
        &self,
        id: ControlFlowNodeId,
    ) -> impl Iterator<Item = &OwnershipCall> {
        self.node(id)
            .into_iter()
            .flat_map(|node| node.ownership.calls.iter())
            .filter(|call| call.is_consuming())
    }

    /// Return normalized partial-move events in source/CFG order.
    ///
    /// Projection moves are currently provenance groundwork: the bootstrap
    /// checker still rejects non-copy aggregate fields before code generation.
    /// Exposing them separately ensures future owned aggregate checking can
    /// consume the typed projection path and reaching definitions directly.
    pub fn partial_moves(&self) -> impl Iterator<Item = (ControlFlowNodeId, &OwnershipMove)> {
        self.nodes.iter().flat_map(|node| {
            node.ownership
                .moves
                .iter()
                .filter(|movement| movement.is_partial())
                .map(move |movement| (node.id, movement))
        })
    }

    /// Return partial-move events attached to one normalized CFG node.
    pub fn partial_moves_at(&self, id: ControlFlowNodeId) -> impl Iterator<Item = &OwnershipMove> {
        self.node(id)
            .into_iter()
            .flat_map(|node| node.ownership.moves.iter())
            .filter(|movement| movement.is_partial())
    }

    /// Return values at normalized `return` evaluation nodes in source order.
    pub fn ownership_returns(&self) -> impl Iterator<Item = (ControlFlowNodeId, &OwnershipReturn)> {
        self.nodes.iter().flat_map(|node| {
            node.ownership
                .returns
                .iter()
                .map(move |value| (node.id, value))
        })
    }

    /// Return the typed ownership facts attached to one normalized return
    /// evaluation node.  Keeping the query node-local lets ownership passes
    /// inspect a specific control-flow boundary without filtering the whole
    /// graph or revisiting the checked AST.
    pub fn ownership_returns_at(&self, id: ControlFlowNodeId) -> &[OwnershipReturn] {
        self.node(id)
            .map_or(&[], |node| node.ownership.returns.as_slice())
    }

    /// Return every ownership event attached to one normalized node.
    ///
    /// Events are emitted in a stable category order (borrows, moves, calls,
    /// returns, drops), and each category retains the deterministic order
    /// established by CFG normalization.  This is intentionally a borrowed
    /// view, so ownership analyses can inspect the complete node boundary
    /// without allocating or copying event payloads.
    pub fn ownership_events_at(
        &self,
        id: ControlFlowNodeId,
    ) -> impl Iterator<Item = ControlFlowOwnershipEvent<'_>> {
        self.node(id).into_iter().flat_map(|node| {
            node.ownership
                .borrows
                .iter()
                .map(ControlFlowOwnershipEvent::Borrow)
                .chain(
                    node.ownership
                        .moves
                        .iter()
                        .map(ControlFlowOwnershipEvent::Move),
                )
                .chain(
                    node.ownership
                        .calls
                        .iter()
                        .map(ControlFlowOwnershipEvent::Call),
                )
                .chain(
                    node.ownership
                        .returns
                        .iter()
                        .map(ControlFlowOwnershipEvent::Return),
                )
                .chain(
                    node.ownership
                        .drops
                        .iter()
                        .map(ControlFlowOwnershipEvent::Drop),
                )
        })
    }

    /// Return the complete normalized ownership event stream in CFG node
    /// order.  The node id is retained alongside each borrowed event so a
    /// consumer can correlate a boundary with its control-flow position
    /// without scanning the graph again.  Events within a node use the same
    /// stable category order as [`Self::ownership_events_at`].
    pub fn ownership_events(
        &self,
    ) -> impl Iterator<Item = (ControlFlowNodeId, ControlFlowOwnershipEvent<'_>)> {
        self.nodes.iter().flat_map(|node| {
            let id = node.id;
            self.ownership_events_at(id).map(move |event| (id, event))
        })
    }

    pub fn is_reachable(&self, id: ControlFlowNodeId) -> bool {
        self.move_state_before(id)
            .is_some_and(ControlFlowMoveState::reachable)
    }

    pub fn definitions_reaching_before(
        &self,
        id: ControlFlowNodeId,
        name: &str,
    ) -> Option<&BTreeSet<ControlFlowDefinitionId>> {
        self.reaching_definitions_before
            .get(id.0)
            .and_then(Option::as_ref)
            .and_then(|reaching| reaching.get(name))
    }

    pub fn definition_reaches_before(
        &self,
        id: ControlFlowNodeId,
        name: &str,
        definition: ControlFlowDefinitionId,
    ) -> bool {
        self.definitions_reaching_before(id, name)
            .is_some_and(|definitions| definitions.contains(&definition))
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
                    if definition.name != name
                        || (!matches!(definition.ty, Type::List(_))
                            && !matches!(&definition.ty, Type::Optional(inner) if matches!(inner.as_ref(), Type::List(_))))
                    {
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
                if matches!(
                    crate::builtin_names::global_impl(callee),
                    "take" | "skip" | "chunked"
                ) =>
            {
                arguments.first().is_some_and(|argument| {
                    self.value_depends_on_borrow_source(*argument, source, visiting)
                })
            }
            ControlFlowValueKind::Call { callee, arguments }
                if matches!(&value.ty, Type::List(element) if matches!(element.as_ref(), Type::List(_)))
                    && matches!(
                        crate::builtin_names::global_impl(callee),
                        "filter" | "where" | "flatten" | "concat"
                    ) =>
            {
                let retained = if crate::builtin_names::global_impl(callee) == "concat" {
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
                if matches!(
                    crate::builtin_names::global_impl(callee),
                    "take" | "skip" | "chunked"
                ) =>
            {
                arguments.first().is_some_and(|argument| {
                    self.value_depends_on_borrow_source(*argument, source, visiting)
                })
            }
            ControlFlowValueKind::Call { callee, arguments }
                if matches!(&value.ty, Type::List(element) if matches!(element.as_ref(), Type::List(_)))
                    && matches!(
                        crate::builtin_names::global_impl(callee),
                        "filter" | "where" | "flatten" | "concat"
                    ) =>
            {
                let retained = if crate::builtin_names::global_impl(callee) == "concat" {
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
            ControlFlowValueKind::Unary {
                op: UnaryOp::Borrow,
                operand,
            } => self.value_depends_on_borrow_source(*operand, source, visiting),
            _ => false,
        }
    }

    fn definition_id_borrow_source_definitions(
        &self,
        definition: ControlFlowDefinitionId,
        source: &str,
        visiting: &mut HashSet<ControlFlowDefinitionId>,
    ) -> BTreeSet<ControlFlowDefinitionId> {
        if !visiting.insert(definition) {
            return BTreeSet::new();
        }
        let mut sources = BTreeSet::new();
        if let Some(value) = self.definition_borrow_source_value(definition) {
            self.collect_value_borrow_source_definitions(value, source, visiting, &mut sources);
        }
        if let Some(value) = self.definition_value(definition) {
            self.collect_value_borrow_source_definitions(value, source, visiting, &mut sources);
        }
        visiting.remove(&definition);
        sources
    }

    fn collect_value_borrow_source_definitions(
        &self,
        value: ControlFlowValueId,
        source: &str,
        visiting: &mut HashSet<ControlFlowDefinitionId>,
        sources: &mut BTreeSet<ControlFlowDefinitionId>,
    ) {
        if !self.is_value_reachable(value) {
            return;
        }
        let Some(value) = self.value(value) else {
            return;
        };
        let mut visit =
            |value| self.collect_value_borrow_source_definitions(value, source, visiting, sources);
        match &value.kind {
            ControlFlowValueKind::List { items } if matches!(&value.ty, Type::List(element) if matches!(element.as_ref(), Type::List(_))) => {
                for item in items {
                    visit(*item);
                }
            }
            ControlFlowValueKind::ListSpread { value } => visit(*value),
            ControlFlowValueKind::ListIf {
                value, else_value, ..
            } => {
                visit(*value);
                if let Some(else_value) = else_value {
                    visit(*else_value);
                }
            }
            ControlFlowValueKind::ListComprehension { value: body, .. } if matches!(&value.ty, Type::List(element) if matches!(element.as_ref(), Type::List(_))) =>
            {
                visit(*body);
            }
            ControlFlowValueKind::NameRead { name, definitions } => {
                if name == source {
                    sources.extend(definitions.iter().copied());
                }
                for definition in definitions {
                    sources.extend(self.definition_id_borrow_source_definitions(
                        *definition,
                        source,
                        visiting,
                    ));
                }
            }
            ControlFlowValueKind::Slice { base, .. }
            | ControlFlowValueKind::Index { base, .. }
            | ControlFlowValueKind::Field { base, .. }
                if matches!(value.ty, Type::List(_)) =>
            {
                visit(*base);
            }
            ControlFlowValueKind::Call { callee, arguments }
                if matches!(
                    crate::builtin_names::global_impl(callee),
                    "take" | "skip" | "chunked"
                ) =>
            {
                if let Some(argument) = arguments.first() {
                    visit(*argument);
                }
            }
            ControlFlowValueKind::Call { callee, arguments }
                if matches!(&value.ty, Type::List(element) if matches!(element.as_ref(), Type::List(_)))
                    && matches!(
                        crate::builtin_names::global_impl(callee),
                        "filter" | "where" | "flatten" | "concat"
                    ) =>
            {
                let retained = if crate::builtin_names::global_impl(callee) == "concat" {
                    arguments.iter().take(2)
                } else {
                    arguments.iter().take(1)
                };
                for argument in retained {
                    visit(*argument);
                }
            }
            ControlFlowValueKind::Match { arms, .. }
            | ControlFlowValueKind::ListMatch { arms, .. } => {
                for arm in arms {
                    visit(*arm);
                }
            }
            ControlFlowValueKind::Conditional {
                then_value,
                else_value,
                ..
            } => {
                visit(*then_value);
                visit(*else_value);
            }
            ControlFlowValueKind::Unary {
                op: UnaryOp::Borrow,
                operand,
            } => visit(*operand),
            _ => {}
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
    if !matches!(definition.ty, Type::List(_))
        && !matches!(&definition.ty, Type::Optional(inner) if matches!(inner.as_ref(), Type::List(_)))
    {
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
        // Resolve move provenance before the fixed-point move analysis.  The
        // move state must consume the same definition-scoped facts that were
        // attached to each normalized ownership event, rather than rebuilding
        // a name-based approximation while propagating through the CFG.
        populate_move_source_definitions_on_nodes(
            &mut self.nodes,
            &self.values,
            &reaching_definitions_before,
        );
        // Consuming-call modes and argument provenance are inputs to move
        // propagation, not a post-processing view of its result. Populate
        // them before the fixed point so normalized consuming boundaries can
        // actually invalidate their reaching definitions.
        populate_call_argument_definitions(&mut self.nodes, &self.values, self.signatures);
        let move_states_before = compute_move_states(&self.nodes, &self.edges, self.entry);
        let (live_before, live_after) =
            compute_liveness(&self.nodes, &self.edges, &self.parameters);
        let (definition_live_before, definition_live_after) =
            compute_definition_liveness(&self.nodes, &self.edges, &reaching_definitions_before);
        let (value_uses, value_regions) = collect_value_uses(&self.values);
        let reachable_values = compute_reachable_values(
            &self.nodes,
            &self.edges,
            &self.values,
            &value_uses,
            &value_regions,
            &move_states_before,
            &live_after,
            self.signatures,
        );
        let escaping_values = compute_escaping_values(
            &self.nodes,
            &self.values,
            &value_uses,
            &definition_values,
            &reachable_values,
        );
        populate_immutable_borrows(
            &mut self.nodes,
            &self.values,
            &reachable_values,
            self.signatures,
        );
        let mut graph = ControlFlowGraph {
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
            escaping_values,
            scoped_definitions: self.scoped_definitions,
            definition_values,
            scoped_borrow_sources: self.scoped_borrow_sources,
            reaching_definitions_before,
            move_states_before,
            live_before,
            live_after,
            definition_live_before,
            definition_live_after,
            borrow_states_before: Vec::new(),
            borrow_starts: Vec::new(),
            borrow_ends: Vec::new(),
            borrow_lifetimes: Vec::new(),
            drops: Vec::new(),
        };
        populate_return_ownership(&mut graph, self.signatures);
        populate_borrow_source_definitions(&mut graph);
        graph.borrow_states_before = compute_borrow_states(&graph);
        graph.borrow_starts = compute_borrow_starts(&graph);
        graph.borrow_ends = compute_borrow_ends(&graph);
        graph.borrow_lifetimes = compute_borrow_lifetimes(&graph);
        graph.drops = compute_drop_facts(&graph);
        for (node, drop) in &graph.drops {
            if let Some(control_flow_node) = graph.nodes.get_mut(node.0) {
                control_flow_node.ownership.drops.push(drop.clone());
            }
        }
        for node in &mut graph.nodes {
            node.ownership.drops.sort_by(|left, right| {
                left.definition
                    .cmp(&right.definition)
                    .then_with(|| left.name.cmp(&right.name))
                    .then_with(|| span_key(left.span).cmp(&span_key(right.span)))
            });
            node.ownership.drops.dedup();
        }
        graph
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
                let types = self.positional_destructure_expression_types(expr);
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
                let types = self.positional_destructure_expression_types(expr);
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
                let mut optional_binding_node = None;
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
                    optional_binding_node = Some(binding_node);
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
                let condition_node =
                    self.evaluation_node(ControlFlowEvaluationKind::Condition, cond, node);
                if let Some(binding_node) = optional_binding_node
                    && let Some(binding) = binding
                    && binding.name != "_"
                    && let Some(Type::Optional(inner)) = self.scalar_expression_type(cond)
                    && matches!(inner.as_ref(), Type::List(_))
                    && let Some(source) = self.nodes[condition_node.0].values.first().copied()
                {
                    self.scoped_borrow_sources.insert(
                        ControlFlowDefinitionId::Node {
                            node: binding_node,
                            index: 0,
                        },
                        source,
                    );
                }
                condition_node
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
                let iterable_types = self
                    .scalar_expression_type(iterable)
                    .map(|ty| match self.signatures.canonical_type(&ty) {
                        Type::List(element) | Type::Set(element) => (None, Some(*element)),
                        Type::Map(key, value) => (Some(*key), Some(*value)),
                        _ => (None, None),
                    })
                    .unwrap_or((None, None));
                if let Some(index_name) = index_name {
                    definitions.push(ControlFlowDefinition {
                        name: index_name.clone(),
                        ty: iterable_types.0.clone().unwrap_or(Type::I64),
                        span: index_span.unwrap_or(stmt.span),
                    });
                }
                if let Some(element) = iterable_types.1 {
                    definitions.push(ControlFlowDefinition {
                        name: name.clone(),
                        ty: element,
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
            ownership: self.ownership_for_expr(id, expr),
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
            | ExprKind::InterpolatedString(_)
            | ExprKind::Nil
            | ExprKind::None => ControlFlowValueKind::Literal,
            ExprKind::Var(name) => ControlFlowValueKind::NameRead {
                name: name.clone(),
                definitions: self.scoped_definition_for(name).into_iter().collect(),
            },
            ExprKind::Await(awaited) => {
                let kind = match &awaited.kind {
                    ExprKind::Call {
                        name,
                        args,
                        named_args,
                    } => ControlFlowValueKind::Call {
                        callee: name.clone(),
                        arguments: self.lower_call_arguments(producer, args, named_args),
                    },
                    _ => ControlFlowValueKind::Opaque,
                };
                let awaited_types = self.expression_types(expr);
                let awaited = self.push_typed_values_with_types(
                    producer,
                    awaited.span,
                    kind,
                    typecheck::constant_primitive_value(awaited, self.signatures),
                    false,
                    awaited_types,
                );
                awaited
                    .into_iter()
                    .next()
                    .map_or(ControlFlowValueKind::Opaque, |value| {
                        ControlFlowValueKind::Await { value }
                    })
            }
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
            ExprKind::Set(items) => ControlFlowValueKind::Set {
                items: self.lower_expr_arguments(producer, items),
            },
            ExprKind::Map(items) => ControlFlowValueKind::Map {
                entries: items
                    .chunks_exact(2)
                    .map(|pair| {
                        (
                            self.lower_scalar_expr(producer, &pair[0])
                                .unwrap_or(ControlFlowValueId(0)),
                            self.lower_scalar_expr(producer, &pair[1])
                                .unwrap_or(ControlFlowValueId(0)),
                        )
                    })
                    .collect(),
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
            ExprKind::Index {
                base,
                index,
                optional,
            } => {
                let base = self.lower_scalar_expr(producer, base);
                let index = self.lower_scalar_expr(producer, index);
                match (base, index) {
                    (Some(base), Some(index)) => ControlFlowValueKind::Index {
                        base,
                        index,
                        optional: *optional,
                    },
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
            ExprKind::RecordLiteral { fields } => ControlFlowValueKind::RecordLiteral {
                fields: fields
                    .iter()
                    .filter_map(|field| {
                        self.lower_scalar_expr(producer, &field.value)
                            .map(|value| (field.name.clone(), value))
                    })
                    .collect(),
            },
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
        self.push_typed_values_with_types(producer, span, kind, constant, is_result, types)
    }

    fn push_typed_values_with_types(
        &mut self,
        producer: ControlFlowNodeId,
        span: SourceSpan,
        kind: ControlFlowValueKind,
        constant: Option<ConstantValue>,
        is_result: bool,
        types: Vec<Type>,
    ) -> Vec<ControlFlowValueId> {
        let scalar = types.len() == 1;
        types
            .into_iter()
            .enumerate()
            .map(|(index, ty)| {
                let id = ControlFlowValueId(self.values.len());
                let ownership = value_ownership(&self.signatures, &ty);
                self.values.push(ControlFlowValue {
                    id,
                    producer,
                    result_index: is_result.then_some(index),
                    ty,
                    ownership,
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

    fn positional_destructure_expression_types(&self, expr: &Expr) -> Vec<Type> {
        let types = self.expression_types(expr);
        if types.len() == 1
            && let Type::Record(fields) = self.signatures.canonical_type(&types[0])
        {
            return fields
                .into_iter()
                .map(|field| self.signatures.canonical_type(&field.ty))
                .collect();
        }
        types
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

    fn ownership_for_expr(&self, node: ControlFlowNodeId, expr: &Expr) -> ControlFlowOwnership {
        let mut reads = HashSet::new();
        typecheck::collect_expr_reads(expr, &mut reads);
        let mut reads = reads.into_iter().collect::<Vec<_>>();
        reads.sort();
        let mut calls = self
            .values
            .iter()
            .filter(|value| value.producer == node)
            .filter_map(|value| {
                let (callee, arguments) = match &value.kind {
                    ControlFlowValueKind::Call { callee, arguments } => {
                        (callee.clone(), arguments.clone())
                    }
                    ControlFlowValueKind::QualifiedCall {
                        namespace,
                        name,
                        arguments,
                    } => (format!("{namespace}.{name}"), arguments.clone()),
                    ControlFlowValueKind::OptionalCascadeCall {
                        callee, arguments, ..
                    } => (callee.clone(), arguments.clone()),
                    ControlFlowValueKind::InterfaceDispatch {
                        interface,
                        capability,
                        arguments,
                        ..
                    } => (format!("{interface}.{capability}"), arguments.clone()),
                    _ => return None,
                };
                Some(OwnershipCall {
                    callee,
                    arguments,
                    argument_kinds: Vec::new(),
                    argument_definitions: Vec::new(),
                    borrowed_argument_definitions: Vec::new(),
                    span: value.span,
                })
            })
            .collect::<Vec<_>>();
        calls.sort_by(|left, right| {
            left.span
                .column
                .cmp(&right.span.column)
                .then_with(|| left.callee.cmp(&right.callee))
        });
        if calls.is_empty()
            && let Expr {
                span,
                kind:
                    ExprKind::Call {
                        name,
                        args,
                        named_args,
                    },
                ..
            } = expr
            && name == "drop"
            && args.len() == 1
            && named_args.is_empty()
            && let ExprKind::Var(source) = &args[0].kind
            && let Some(argument) = self.values.iter().rev().find(|value| {
                value.producer == node
                    && matches!(
                        &value.kind,
                        ControlFlowValueKind::NameRead { name, .. } if name == source
                    )
            })
        {
            calls.push(OwnershipCall {
                callee: "drop".to_string(),
                arguments: vec![argument.id],
                argument_kinds: Vec::new(),
                argument_definitions: Vec::new(),
                borrowed_argument_definitions: Vec::new(),
                span: *span,
            });
        }
        let moves = if expr_is_consuming_drop(expr)
            && let Expr {
                kind: ExprKind::Call { args, .. },
                ..
            } = expr
            && let Some(Expr {
                kind: ExprKind::Var(source),
                ..
            }) = args.first()
        {
            let value = self.values.iter().rev().find_map(|candidate| {
                (candidate.producer == node
                    && matches!(
                        &candidate.kind,
                        ControlFlowValueKind::NameRead { name, .. } if name == source
                    ))
                .then_some(candidate.id)
            });
            vec![OwnershipMove {
                source: source.clone(),
                destination: "<drop>".to_string(),
                projection: Vec::new(),
                value,
                source_definitions: Vec::new(),
                span: expr.span,
            }]
        } else {
            Vec::new()
        };
        ControlFlowOwnership {
            reads,
            borrows: Vec::new(),
            moves,
            calls,
            returns: Vec::new(),
            drops: Vec::new(),
        }
    }

    fn binding_move_ownership(&self, name: &str, ty: &Type, expr: &Expr) -> ControlFlowOwnership {
        let moves = if !self.signatures.is_copy_type(ty)
            && let Some((source, projection)) =
                non_copy_projection(expr, self.signatures, |value| {
                    self.scalar_expression_type(value)
                }) {
            vec![OwnershipMove {
                source,
                destination: name.to_string(),
                projection,
                value: None,
                source_definitions: Vec::new(),
                span: expr.span,
            }]
        } else {
            Vec::new()
        };
        ControlFlowOwnership {
            reads: Vec::new(),
            borrows: Vec::new(),
            moves,
            calls: Vec::new(),
            returns: Vec::new(),
            drops: Vec::new(),
        }
    }
}

/// Return the root binding and field path for a non-copy projection.  Keeping
/// this normalization in the IR builder means later partial-move checking can
/// operate on typed value provenance instead of reparsing field expressions.
fn non_copy_projection(
    expr: &Expr,
    signatures: &Signatures,
    type_of: impl Fn(&Expr) -> Option<Type>,
) -> Option<(String, Vec<String>)> {
    let mut path = Vec::new();
    let mut current = expr;
    loop {
        match &current.kind {
            ExprKind::Var(name) => {
                path.reverse();
                return Some((name.clone(), path));
            }
            ExprKind::Field {
                base,
                name,
                optional,
                ..
            } if !optional
                && matches!(
                    signatures.canonical_type(&type_of(base)?),
                    Type::Named(ref aggregate)
                        if signatures.struct_type(aggregate).is_some()
                            || signatures.enum_type(aggregate).is_some()
                ) =>
            {
                path.push(name.clone());
                current = base;
            }
            _ => return None,
        }
    }
}

fn expr_is_consuming_drop(expr: &Expr) -> bool {
    matches!(
        &expr.kind,
        ExprKind::Call {
            name,
            args,
            named_args,
        } if name == "drop" && args.len() == 1 && named_args.is_empty()
    )
}

/// Attach reaching-definition identity to move events after the CFG has been
/// normalized but before ownership fixed points run. Keeping this separate
/// from expression lowering means every ownership consumer observes the same
/// definition-scoped facts instead of rebuilding a name-based approximation.
fn populate_move_source_definitions_on_nodes(
    nodes: &mut [ControlFlowNode],
    values: &[ControlFlowValue],
    reaching_definitions_before: &[Option<ReachingDefinitionMap>],
) {
    for node in nodes {
        let mut moves = std::mem::take(&mut node.ownership.moves);
        for movement in &mut moves {
            let projected_value = movement.is_partial().then(|| {
                values
                    .iter()
                    .filter(|value| value.producer == node.id && value.span == movement.span)
                    .min_by_key(|value| value.id.0)
            });
            let value = projected_value.flatten().or_else(|| {
                values
                    .iter()
                    .filter(|value| {
                        matches!(
                            &value.kind,
                            ControlFlowValueKind::NameRead { name, .. } if name == &movement.source
                        )
                    })
                    .min_by_key(|value| {
                        (
                            if value.span == movement.span { 0 } else { 1 },
                            if value.producer == node.id { 0 } else { 1 },
                            value.span.column,
                            value.id.0,
                        )
                    })
            });
            movement.value = value.map(|value| value.id);
            movement.source_definitions = value
                .and_then(|value| match &value.kind {
                    ControlFlowValueKind::NameRead { definitions, .. } => Some(definitions.clone()),
                    _ => None,
                })
                .or_else(|| {
                    reaching_definitions_before
                        .get(node.id.0)
                        .and_then(Option::as_ref)
                        .and_then(|reaching| reaching.get(&movement.source))
                        .map(|definitions| definitions.iter().copied().collect())
                })
                .unwrap_or_default();
        }
        node.ownership.moves = moves;
    }
}

fn populate_call_argument_definitions(
    nodes: &mut [ControlFlowNode],
    values: &[ControlFlowValue],
    signatures: &Signatures,
) {
    for node in nodes {
        for call in &mut node.ownership.calls {
            call.argument_definitions = call
                .arguments
                .iter()
                .map(|argument| call_argument_definitions(*argument, values))
                .collect();
            call.argument_kinds = call
                .arguments
                .iter()
                .map(|argument| {
                    if call.callee == "drop" {
                        OwnershipCallArgumentKind::Consuming
                    } else {
                        values
                            .get(argument.0)
                            .map_or(OwnershipCallArgumentKind::Copy, |value| {
                                ownership_kind_for_value(value, signatures)
                            })
                    }
                })
                .collect();
            call.borrowed_argument_definitions = call
                .arguments
                .iter()
                .map(|argument| {
                    if call.callee == "drop" {
                        return Vec::new();
                    }
                    let Some(value) = values.get(argument.0) else {
                        return Vec::new();
                    };
                    if ownership_kind_for_value(value, signatures)
                        == OwnershipCallArgumentKind::ImmutableBorrow
                    {
                        call_argument_definitions(*argument, values)
                    } else {
                        Vec::new()
                    }
                })
                .collect();
        }
    }
}

fn ownership_kind_for_value(
    value: &ControlFlowValue,
    signatures: &Signatures,
) -> OwnershipCallArgumentKind {
    match signatures.canonical_type(&value.ty) {
        Type::List(_) | Type::Set(_) | Type::Map(_, _) => {
            OwnershipCallArgumentKind::ImmutableBorrow
        }
        Type::Optional(inner)
            if matches!(
                inner.as_ref(),
                Type::List(_) | Type::Set(_) | Type::Map(_, _)
            ) =>
        {
            OwnershipCallArgumentKind::ImmutableBorrow
        }
        _ => OwnershipCallArgumentKind::Copy,
    }
}

fn populate_return_ownership(graph: &mut ControlFlowGraph, signatures: &Signatures) {
    let values = graph.values.clone();
    for node in &mut graph.nodes {
        if !matches!(
            node.kind,
            ControlFlowNodeKind::Evaluation(ControlFlowEvaluationKind::ReturnValue(_))
        ) {
            continue;
        }
        node.ownership.returns = node
            .values
            .iter()
            .filter_map(|value_id| {
                let value = values.get(value_id.0)?;
                let kind = ownership_kind_for_value(value, signatures);
                let definitions = call_argument_definitions(*value_id, &values);
                let borrowed_definitions = if kind == OwnershipCallArgumentKind::ImmutableBorrow {
                    definitions.clone()
                } else {
                    Vec::new()
                };
                Some(OwnershipReturn {
                    value: *value_id,
                    kind,
                    definitions,
                    borrowed_definitions,
                    span: value.span,
                })
            })
            .collect();
    }
}

/// Resolve the reaching source definitions for a call argument without
/// re-walking the checked AST.  A projection still borrows its root value, so
/// fields, indexes, slices, and optional collection projections must retain
/// the concrete definition identity of their source owner at the call boundary.
/// Keeping this provenance on the typed value graph is important when a
/// future consuming-call contract is added: it must reject a move of the
/// owner while a projected argument is still live rather than treating the
/// projection as an unrelated expression.
fn call_argument_definitions(
    argument: ControlFlowValueId,
    values: &[ControlFlowValue],
) -> Vec<ControlFlowDefinitionId> {
    let mut definitions = Vec::new();
    let mut visited = BTreeSet::new();
    collect_call_argument_definitions(argument, values, &mut visited, &mut definitions);
    definitions.sort();
    definitions.dedup();
    definitions
}

/// Collect the concrete definitions that back a non-copy collection-valued
/// call argument.
///
/// A borrowed collection can be hidden behind a conditional/list-match arm, a
/// compiler-known view transform, or a nested list literal. Walking those
/// typed value nodes here keeps call-boundary provenance aligned with the
/// normalized value graph instead of silently reducing the argument to an
/// opaque temporary. Scalar conditions, indexes, and callback bodies are
/// deliberately not traversed because they do not own collection storage.
fn collect_call_argument_definitions(
    argument: ControlFlowValueId,
    values: &[ControlFlowValue],
    visited: &mut BTreeSet<ControlFlowValueId>,
    definitions: &mut Vec<ControlFlowDefinitionId>,
) {
    if !visited.insert(argument) {
        return;
    }
    let Some(value) = values.get(argument.0) else {
        return;
    };
    let is_non_copy_collection = |ty: &Type| {
        matches!(ty, Type::List(_) | Type::Set(_) | Type::Map(_, _))
            || matches!(ty, Type::Optional(inner) if matches!(inner.as_ref(), Type::List(_) | Type::Set(_) | Type::Map(_, _)))
    };
    if !is_non_copy_collection(&value.ty) {
        return;
    }
    match &value.kind {
        ControlFlowValueKind::NameRead {
            definitions: reached,
            ..
        } => {
            definitions.extend(reached.iter().copied());
        }
        ControlFlowValueKind::List { items } | ControlFlowValueKind::Set { items } => {
            for item in items {
                collect_call_argument_definitions(*item, values, visited, definitions);
            }
        }
        ControlFlowValueKind::ListIf {
            condition,
            value,
            else_value,
        } => {
            let known_condition = values
                .get(condition.0)
                .and_then(|condition| {
                    condition
                        .constant
                        .as_ref()
                        .or(condition.source_constant.as_ref())
                })
                .and_then(|constant| match constant {
                    ConstantValue::Bool(value) => Some(*value),
                    _ => None,
                });
            if known_condition != Some(false) {
                collect_call_argument_definitions(*value, values, visited, definitions);
            }
            if known_condition != Some(true)
                && let Some(else_value) = else_value
            {
                collect_call_argument_definitions(*else_value, values, visited, definitions);
            }
        }
        ControlFlowValueKind::ListComprehension { value, .. } => {
            collect_call_argument_definitions(*value, values, visited, definitions);
        }
        ControlFlowValueKind::ListSpread { value }
        | ControlFlowValueKind::ListOptional { value }
        | ControlFlowValueKind::Field { base: value, .. }
        | ControlFlowValueKind::Index { base: value, .. }
        | ControlFlowValueKind::Slice { base: value, .. } => {
            collect_call_argument_definitions(*value, values, visited, definitions);
        }
        ControlFlowValueKind::Call { callee, arguments } => {
            let count = if matches!(crate::builtin_names::global_impl(callee), "concat") {
                2
            } else {
                1
            };
            for argument in arguments.iter().take(count) {
                collect_call_argument_definitions(*argument, values, visited, definitions);
            }
        }
        ControlFlowValueKind::QualifiedCall { arguments, .. }
        | ControlFlowValueKind::InterfaceDispatch { arguments, .. } => {
            for argument in arguments {
                collect_call_argument_definitions(*argument, values, visited, definitions);
            }
        }
        ControlFlowValueKind::Match { arms, .. } | ControlFlowValueKind::ListMatch { arms, .. } => {
            for arm in arms {
                collect_call_argument_definitions(*arm, values, visited, definitions);
            }
        }
        ControlFlowValueKind::Conditional {
            then_value,
            else_value,
            ..
        } => {
            collect_call_argument_definitions(*then_value, values, visited, definitions);
            collect_call_argument_definitions(*else_value, values, visited, definitions);
        }
        ControlFlowValueKind::OptionalCascadeCall {
            optional,
            arguments,
            ..
        } => {
            collect_call_argument_definitions(*optional, values, visited, definitions);
            for argument in arguments {
                collect_call_argument_definitions(*argument, values, visited, definitions);
            }
        }
        ControlFlowValueKind::InterfacePack { value, .. } => {
            collect_call_argument_definitions(*value, values, visited, definitions);
        }
        ControlFlowValueKind::Await { value } => {
            collect_call_argument_definitions(*value, values, visited, definitions);
        }
        ControlFlowValueKind::Literal
        | ControlFlowValueKind::AnonymousFunction { .. }
        | ControlFlowValueKind::Map { .. }
        | ControlFlowValueKind::RecordLiteral { .. }
        | ControlFlowValueKind::StructLiteral { .. }
        | ControlFlowValueKind::Unary { .. }
        | ControlFlowValueKind::Binary { .. }
        | ControlFlowValueKind::Opaque => {}
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
    if function.asynchronous {
        env.insert("flux__async_context".to_string(), Type::Bool);
    }
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
                if let Ok((types, _)) =
                    typecheck::positional_destructure_types_of_expr(expr, env, signatures)
                {
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
                binding,
                body,
                else_body,
                ..
            } => {
                record_evaluation_type(cond, env, signatures, evaluations);
                let mut then_env = env.clone();
                if let Some(binding) = binding
                    && binding.name != "_"
                    && let Ok(Type::Optional(inner)) =
                        typecheck::type_of_expr(cond, env, signatures)
                {
                    then_env.insert(binding.name.clone(), *inner);
                }
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
                if let Ok(ty) = typecheck::type_of_expr(iterable, env, signatures) {
                    match signatures.canonical_type(&ty) {
                        Type::List(element) | Type::Set(element) => {
                            nested.insert(name.clone(), *element);
                        }
                        Type::Map(key, value) => {
                            if let Some(index_name) = index_name {
                                nested.insert(index_name.clone(), *key);
                            }
                            nested.insert(name.clone(), *value);
                        }
                        _ => {}
                    }
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

fn record_inline_sequence_callback_types(
    expr: &Expr,
    env: &HashMap<String, Type>,
    signatures: &Signatures,
    evaluations: &mut Vec<(SourceSpan, Vec<Type>)>,
) {
    let ExprKind::AnonymousFunction { params, body, .. } = &expr.kind else {
        record_expr_types(expr, env, signatures, evaluations);
        return;
    };
    let mut nested = env.clone();
    for param in params {
        nested.insert(param.name.clone(), signatures.canonical_type(&param.ty));
    }
    record_expr_types(body, &nested, signatures, evaluations);
    if let Ok(ty) = typecheck::type_of_sequence_callback(expr, env, signatures) {
        evaluations.push((expr.span, vec![signatures.canonical_type(&ty)]));
    }
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
        | ExprKind::InterpolatedString(_)
        | ExprKind::Nil
        | ExprKind::None
        | ExprKind::Var(_) => {}
        ExprKind::Await(awaited) => {
            record_expr_types(awaited, env, signatures, evaluations);
        }
        ExprKind::AnonymousFunction { params, body, .. } => {
            let mut nested = env.clone();
            for param in params {
                nested.insert(param.name.clone(), signatures.canonical_type(&param.ty));
            }
            record_expr_types(body, &nested, signatures, evaluations);
        }
        ExprKind::Call {
            name,
            args,
            named_args,
        } => {
            for (index, arg) in args.iter().enumerate() {
                let inline_sequence_callback = named_args.is_empty()
                    && matches!(arg.kind, ExprKind::AnonymousFunction { .. })
                    && match name.as_str() {
                        "map" | "filter" | "where" => args.len() == 2 && index == 1,
                        "fold" => args.len() == 3 && index == 2,
                        "reduce" => args.len() == 2 && index == 1,
                        _ => false,
                    };
                if inline_sequence_callback {
                    record_inline_sequence_callback_types(arg, env, signatures, evaluations);
                } else {
                    record_expr_types(arg, env, signatures, evaluations);
                }
            }
            for arg in named_args {
                record_expr_types(&arg.value, env, signatures, evaluations);
            }
        }
        ExprKind::QualifiedCall {
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
        ExprKind::Pipe {
            input, name, args, ..
        } => {
            record_expr_types(input, env, signatures, evaluations);
            for (index, arg) in args.iter().enumerate() {
                let inline_sequence_callback =
                    matches!(arg.kind, ExprKind::AnonymousFunction { .. })
                        && match name.as_str() {
                            "map" | "filter" | "where" => args.len() == 1 && index == 0,
                            "fold" => args.len() == 2 && index == 1,
                            "reduce" => args.len() == 1 && index == 0,
                            _ => false,
                        };
                if inline_sequence_callback {
                    record_inline_sequence_callback_types(arg, env, signatures, evaluations);
                } else {
                    record_expr_types(arg, env, signatures, evaluations);
                }
            }
        }
        ExprKind::List(items) | ExprKind::Set(items) | ExprKind::Map(items) => {
            for item in items {
                record_expr_types(item, env, signatures, evaluations);
            }
        }
        ExprKind::ListSpread {
            value, optional, ..
        } => {
            record_expr_types(value, env, signatures, evaluations);
            if let Ok(ty) = typecheck::type_of_expr(value, env, signatures) {
                let ty = signatures.canonical_type(&ty);
                let element = if *optional {
                    let Type::Optional(inner) = ty else {
                        return;
                    };
                    let Type::List(element) = signatures.canonical_type(&inner) else {
                        return;
                    };
                    element
                } else {
                    let Type::List(element) = ty else {
                        return;
                    };
                    element
                };
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
        ExprKind::Index { base, index, .. } => {
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
        ExprKind::RecordLiteral { fields } => {
            for field in fields {
                record_expr_types(&field.value, env, signatures, evaluations);
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
        let types = if types.is_empty() && matches!(expr.kind, ExprKind::Await(_)) {
            vec![Type::Void]
        } else {
            types
        };
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
            ControlFlowValueKind::Await { value: awaited } => {
                push_value_use(
                    &mut uses,
                    value.id,
                    *awaited,
                    ControlFlowValueUseKind::Eager,
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
            ControlFlowValueKind::List { items } | ControlFlowValueKind::Set { items } => {
                for item in items {
                    push_value_use(&mut uses, value.id, *item, ControlFlowValueUseKind::Eager);
                }
            }
            ControlFlowValueKind::Map { entries } => {
                for (key, mapped_value) in entries {
                    push_value_use(&mut uses, value.id, *key, ControlFlowValueUseKind::Eager);
                    push_value_use(
                        &mut uses,
                        value.id,
                        *mapped_value,
                        ControlFlowValueUseKind::Eager,
                    );
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
            ControlFlowValueKind::Index {
                base,
                index,
                optional,
            } => {
                push_value_use(&mut uses, value.id, *base, ControlFlowValueUseKind::Eager);
                if *optional {
                    push_value_region_use(
                        values,
                        &mut uses,
                        &mut regions,
                        value,
                        *index,
                        ControlFlowValueUseKind::BranchThen,
                        ControlFlowValueRegionKind::OptionalPresent { optional: *base },
                    );
                } else {
                    push_value_use(&mut uses, value.id, *index, ControlFlowValueUseKind::Eager);
                }
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
            ControlFlowValueKind::RecordLiteral { fields } => {
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
    let is_list_owner = |ty: &Type| {
        matches!(ty, Type::List(_))
            || matches!(ty, Type::Optional(inner) if matches!(inner.as_ref(), Type::List(_)))
    };
    let mut borrowed_definitions = parameters
        .iter()
        .enumerate()
        .filter(|(_, parameter)| is_list_owner(&signatures.canonical_type(&parameter.ty)))
        .map(|(index, _)| ControlFlowDefinitionId::Parameter(index))
        .collect::<BTreeSet<_>>();
    borrowed_definitions.extend(scoped_borrow_sources.keys().copied());
    for node in nodes.iter() {
        for (index, definition) in node.definitions.iter().enumerate() {
            if !is_list_owner(&signatures.canonical_type(&definition.ty)) {
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
                if !is_list_owner(&signatures.canonical_type(&definition.ty)) {
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
                && borrow.value == Some(value.id)
                && borrow.span == value.span
        }) {
            continue;
        }
        node.ownership.borrows.push(OwnershipBorrow {
            source: name.clone(),
            kind: OwnershipBorrowKind::Immutable,
            value: Some(value.id),
            source_definitions: Vec::new(),
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

/// Attach reaching-definition identity to normalized borrow events after the
/// CFG data-flow pass has converged.  Borrow events are initially discovered
/// from typed value reads because that is the only point where reachability is
/// known; resolving them here keeps the event itself useful to later ownership
/// consumers without making them repeat a name-based lookup.
fn populate_borrow_source_definitions(graph: &mut ControlFlowGraph) {
    for node in &mut graph.nodes {
        let reaching = graph
            .reaching_definitions_before
            .get(node.id.0)
            .and_then(Option::as_ref)
            .cloned();
        for borrow in &mut node.ownership.borrows {
            borrow.source_definitions = reaching
                .as_ref()
                .and_then(|reaching| reaching.get(&borrow.source))
                .map(|definitions| definitions.iter().copied().collect())
                .unwrap_or_default();
        }
        node.ownership.borrows.sort_by(|left, right| {
            left.source
                .cmp(&right.source)
                .then_with(|| left.source_definitions.cmp(&right.source_definitions))
                .then_with(|| span_key(left.span).cmp(&span_key(right.span)))
        });
    }
}

fn compute_reachable_values(
    nodes: &[ControlFlowNode],
    edges: &[ControlFlowEdge],
    values: &[ControlFlowValue],
    uses: &[ControlFlowValueUse],
    regions: &[ControlFlowValueRegion],
    move_states_before: &[ControlFlowMoveState],
    live_after: &[ControlFlowLiveState],
    signatures: &Signatures,
) -> BTreeSet<ControlFlowValueId> {
    let mut reachable = BTreeSet::new();
    let mut pending = values
        .iter()
        .filter(|value| {
            value.result_index.is_some()
                && move_states_before
                    .get(value.producer.0)
                    .is_some_and(ControlFlowMoveState::reachable)
                && value_root_is_observable(value.id, nodes, edges, values, live_after, signatures)
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

fn value_root_is_observable(
    id: ControlFlowValueId,
    nodes: &[ControlFlowNode],
    edges: &[ControlFlowEdge],
    values: &[ControlFlowValue],
    live_after: &[ControlFlowLiveState],
    signatures: &Signatures,
) -> bool {
    let Some(value) = values.get(id.0) else {
        return true;
    };
    let Some(node) = nodes.get(value.producer.0) else {
        return true;
    };
    let discardable = ir_value_is_discardable(id, values, signatures, &mut HashSet::new());
    match node.kind {
        ControlFlowNodeKind::Evaluation(ControlFlowEvaluationKind::ExpressionStatement) => {
            !discardable
        }
        ControlFlowNodeKind::Evaluation(
            ControlFlowEvaluationKind::BindingInitializer
            | ControlFlowEvaluationKind::AssignmentValue,
        ) if value.result_index == Some(0) => {
            let Some(target) = edges
                .iter()
                .find(|edge| edge.from == node.id && edge.kind == ControlFlowEdgeKind::Next)
                .and_then(|edge| nodes.get(edge.to.0))
            else {
                return true;
            };
            if target.definitions.len() != 1 {
                return true;
            }
            let definition = &target.definitions[0];
            let dead = live_after
                .get(target.id.0)
                .is_some_and(|state| !state.contains(&definition.name));
            !(dead && discardable)
        }
        _ => true,
    }
}

fn ir_value_is_discardable(
    id: ControlFlowValueId,
    values: &[ControlFlowValue],
    signatures: &Signatures,
    visiting: &mut HashSet<ControlFlowValueId>,
) -> bool {
    let Some(value) = values.get(id.0) else {
        return false;
    };
    if value.source_constant.is_some() {
        return true;
    }
    if !visiting.insert(id) {
        return false;
    }
    let child = |child, visiting: &mut HashSet<ControlFlowValueId>| {
        ir_value_is_discardable(child, values, signatures, visiting)
    };
    let result = match &value.kind {
        ControlFlowValueKind::Literal | ControlFlowValueKind::AnonymousFunction { .. } => true,
        ControlFlowValueKind::Await { .. } => false,
        ControlFlowValueKind::NameRead { .. } => signatures.is_copy_type(&value.ty),
        ControlFlowValueKind::List { items } | ControlFlowValueKind::Set { items } => {
            items.iter().all(|item| child(*item, visiting))
        }
        ControlFlowValueKind::Map { entries } => entries
            .iter()
            .all(|(key, value)| child(*key, visiting) && child(*value, visiting)),
        ControlFlowValueKind::ListSpread { value }
        | ControlFlowValueKind::ListOptional { value } => child(*value, visiting),
        ControlFlowValueKind::ListIf {
            condition,
            value,
            else_value,
        } => {
            child(*condition, visiting)
                && child(*value, visiting)
                && else_value.is_none_or(|value| child(value, visiting))
        }
        ControlFlowValueKind::StructLiteral { base, fields, .. } => {
            base.is_none_or(|value| child(value, visiting))
                && fields.iter().all(|(_, value)| child(*value, visiting))
        }
        ControlFlowValueKind::RecordLiteral { fields } => {
            fields.iter().all(|(_, value)| child(*value, visiting))
        }
        ControlFlowValueKind::Field { base, .. } => child(*base, visiting),
        ControlFlowValueKind::Conditional {
            condition,
            then_value,
            else_value,
        } => {
            child(*condition, visiting)
                && child(*then_value, visiting)
                && child(*else_value, visiting)
        }
        ControlFlowValueKind::Unary {
            op: UnaryOp::Not,
            operand,
        } => child(*operand, visiting),
        ControlFlowValueKind::Binary { op, left, right }
            if matches!(
                op,
                BinOp::Eq
                    | BinOp::Ne
                    | BinOp::Lt
                    | BinOp::Le
                    | BinOp::Gt
                    | BinOp::Ge
                    | BinOp::And
                    | BinOp::Or
            ) =>
        {
            child(*left, visiting) && child(*right, visiting)
        }
        _ => false,
    };
    visiting.remove(&id);
    result
}

fn compute_escaping_values(
    nodes: &[ControlFlowNode],
    values: &[ControlFlowValue],
    uses: &[ControlFlowValueUse],
    definition_values: &BTreeMap<ControlFlowDefinitionId, ControlFlowValueId>,
    reachable_values: &BTreeSet<ControlFlowValueId>,
) -> BTreeSet<ControlFlowValueId> {
    let mut escaping = BTreeSet::new();
    let mut pending = Vec::new();

    for node in nodes {
        if matches!(
            node.kind,
            ControlFlowNodeKind::Evaluation(ControlFlowEvaluationKind::ReturnValue(_))
        ) {
            pending.extend(
                node.values
                    .iter()
                    .copied()
                    .filter(|value| reachable_values.contains(value)),
            );
        }
    }
    for value in values
        .iter()
        .filter(|value| reachable_values.contains(&value.id))
    {
        match &value.kind {
            ControlFlowValueKind::Call { arguments, .. }
            | ControlFlowValueKind::QualifiedCall { arguments, .. }
            | ControlFlowValueKind::InterfaceDispatch { arguments, .. } => {
                pending.extend(arguments.iter().copied());
            }
            ControlFlowValueKind::OptionalCascadeCall {
                optional,
                arguments,
                ..
            } => {
                pending.push(*optional);
                pending.extend(arguments.iter().copied());
            }
            _ => {}
        }
    }

    while let Some(id) = pending.pop() {
        if !reachable_values.contains(&id) || !escaping.insert(id) {
            continue;
        }
        for usage in uses.iter().filter(|usage| usage.user == id) {
            if reachable_values.contains(&usage.value) {
                pending.push(usage.value);
            }
        }
        if let Some(ControlFlowValue {
            kind: ControlFlowValueKind::NameRead { definitions, .. },
            ..
        }) = values.get(id.0)
        {
            pending.extend(
                definitions
                    .iter()
                    .filter_map(|definition| definition_values.get(definition).copied()),
            );
        }
    }

    escaping
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

fn value_ownership(
    signatures: &Signatures,
    ty: &Type,
) -> ControlFlowValueOwnership {
    if signatures.is_copy_type(ty) {
        ControlFlowValueOwnership::Copy
    } else {
        // Every currently admitted non-Copy value is a borrowed collection
        // descriptor.  Keep this conservative fallback explicit so a future
        // owned value cannot accidentally inherit borrow semantics when its
        // type is added to the language.
        ControlFlowValueOwnership::ImmutableBorrow
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

fn compute_definition_liveness(
    nodes: &[ControlFlowNode],
    edges: &[ControlFlowEdge],
    reaching_definitions_before: &[Option<ReachingDefinitionMap>],
) -> (
    Vec<ControlFlowDefinitionLiveState>,
    Vec<ControlFlowDefinitionLiveState>,
) {
    let mut before = vec![BTreeSet::<ControlFlowDefinitionId>::new(); nodes.len()];
    let mut after = vec![BTreeSet::<ControlFlowDefinitionId>::new(); nodes.len()];
    loop {
        let mut changed = false;
        for node in nodes.iter().rev() {
            let mut next_after = BTreeSet::new();
            for edge in edges.iter().filter(|edge| edge.from == node.id) {
                next_after.extend(before[edge.to.0].iter().copied());
            }
            let mut next_before = next_after.clone();
            for (index, _) in node.definitions.iter().enumerate() {
                next_before.remove(&ControlFlowDefinitionId::Node {
                    node: node.id,
                    index,
                });
            }
            if let Some(reaching) = reaching_definitions_before
                .get(node.id.0)
                .and_then(Option::as_ref)
            {
                for name in &node.ownership.reads {
                    if let Some(definitions) = reaching.get(name) {
                        next_before.extend(definitions.iter().copied());
                    }
                }
            }
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
    let into_state = |bindings: BTreeSet<ControlFlowDefinitionId>| ControlFlowDefinitionLiveState {
        live: bindings.into_iter().collect(),
    };
    (
        before.into_iter().map(into_state).collect(),
        after.into_iter().map(into_state).collect(),
    )
}

fn compute_borrow_starts(graph: &ControlFlowGraph) -> Vec<OwnershipBorrowStart> {
    let mut starts = Vec::new();
    for edge in graph.edges() {
        if !graph.is_reachable(edge.from) || !graph.is_reachable(edge.to) {
            continue;
        }
        let Some(from_state) = graph.borrow_state_before(edge.from) else {
            continue;
        };
        let Some(to_state) = graph.borrow_state_before(edge.to) else {
            continue;
        };
        for borrow in to_state.borrows() {
            if !from_state.contains_definition(borrow.definition, borrow.source_definition) {
                starts.push(OwnershipBorrowStart {
                    from: edge.from,
                    to: edge.to,
                    definition: borrow.definition,
                    borrower: borrow.borrower.clone(),
                    source_definition: borrow.source_definition,
                    source: borrow.source.clone(),
                    origin: borrow.origin,
                });
            }
        }
    }
    starts.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then_with(|| left.to.cmp(&right.to))
            .then_with(|| left.borrower.cmp(&right.borrower))
            .then_with(|| left.source_definition.cmp(&right.source_definition))
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| span_key(left.origin).cmp(&span_key(right.origin)))
    });
    starts.dedup();
    starts
}

fn compute_borrow_ends(graph: &ControlFlowGraph) -> Vec<OwnershipBorrowEnd> {
    let mut ends = Vec::new();
    for edge in graph.edges() {
        if !graph.is_reachable(edge.from) || !graph.is_reachable(edge.to) {
            continue;
        }
        let Some(from_state) = graph.borrow_state_before(edge.from) else {
            continue;
        };
        let Some(to_state) = graph.borrow_state_before(edge.to) else {
            continue;
        };
        for borrow in from_state.borrows() {
            if !to_state.contains_definition(borrow.definition, borrow.source_definition) {
                ends.push(OwnershipBorrowEnd {
                    from: edge.from,
                    to: edge.to,
                    definition: borrow.definition,
                    borrower: borrow.borrower.clone(),
                    source_definition: borrow.source_definition,
                    source: borrow.source.clone(),
                    origin: borrow.origin,
                });
            }
        }
    }
    ends.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then_with(|| left.to.cmp(&right.to))
            .then_with(|| left.borrower.cmp(&right.borrower))
            .then_with(|| left.source_definition.cmp(&right.source_definition))
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| span_key(left.origin).cmp(&span_key(right.origin)))
    });
    ends.dedup();
    ends
}

fn compute_borrow_lifetimes(graph: &ControlFlowGraph) -> Vec<OwnershipBorrowLifetime> {
    let mut lifetimes = BTreeMap::<
        (ControlFlowDefinitionId, ControlFlowDefinitionId),
        OwnershipBorrowLifetime,
    >::new();

    for (node_index, state) in graph.borrow_states_before.iter().enumerate() {
        for borrow in state.borrows() {
            lifetimes
                .entry((borrow.definition, borrow.source_definition))
                .or_insert_with(|| OwnershipBorrowLifetime {
                    definition: borrow.definition,
                    borrower: borrow.borrower.clone(),
                    source_definition: borrow.source_definition,
                    source: borrow.source.clone(),
                    origin: borrow.origin,
                    active_before: Vec::new(),
                    starts: Vec::new(),
                    ends: Vec::new(),
                })
                .active_before
                .push(ControlFlowNodeId(node_index));
        }
    }

    for start in &graph.borrow_starts {
        if let Some(lifetime) = lifetimes.get_mut(&(start.definition, start.source_definition)) {
            lifetime.starts.push(start.clone());
        }
    }
    for end in &graph.borrow_ends {
        if let Some(lifetime) = lifetimes.get_mut(&(end.definition, end.source_definition)) {
            lifetime.ends.push(end.clone());
        }
    }

    lifetimes.into_values().collect()
}

fn compute_borrow_states(graph: &ControlFlowGraph) -> Vec<ControlFlowBorrowState> {
    let is_list_owner = |ty: &Type| {
        matches!(ty, Type::List(_))
            || matches!(ty, Type::Optional(inner) if matches!(inner.as_ref(), Type::List(_)))
    };
    let mut source_names = graph
        .parameters
        .iter()
        .filter(|parameter| is_list_owner(&parameter.ty))
        .map(|parameter| parameter.name.clone())
        .collect::<BTreeSet<_>>();
    for node in &graph.nodes {
        source_names.extend(
            node.definitions
                .iter()
                .filter(|definition| is_list_owner(&definition.ty))
                .map(|definition| definition.name.clone()),
        );
    }

    let mut sources_by_definition =
        BTreeMap::<ControlFlowDefinitionId, Vec<(ControlFlowDefinitionId, String)>>::new();
    for node in &graph.nodes {
        for (index, definition) in node.definitions.iter().enumerate() {
            if !is_list_owner(&definition.ty) {
                continue;
            }
            let id = ControlFlowDefinitionId::Node {
                node: node.id,
                index,
            };
            let mut sources = Vec::new();
            for source in &source_names {
                if source == &definition.name {
                    continue;
                }
                let mut visiting = HashSet::new();
                sources.extend(
                    graph
                        .definition_id_borrow_source_definitions(id, source, &mut visiting)
                        .into_iter()
                        .map(|source_definition| (source_definition, source.clone())),
                );
            }
            sources.sort();
            sources.dedup();
            if !sources.is_empty() {
                sources_by_definition.insert(id, sources);
            }
        }
    }

    graph
        .nodes
        .iter()
        .map(|node| {
            if !graph.is_reachable(node.id) {
                return ControlFlowBorrowState::default();
            }
            let Some(live) = graph.definition_live_before.get(node.id.0) else {
                return ControlFlowBorrowState::default();
            };

            let mut borrows = Vec::new();
            for definition in live.live() {
                let Some(origin) = graph.definition_span(*definition) else {
                    continue;
                };
                let Some(borrower) = graph.definition_name(*definition) else {
                    continue;
                };
                let Some(sources) = sources_by_definition.get(definition) else {
                    continue;
                };
                borrows.extend(sources.iter().map(|(source_definition, source)| {
                    OwnershipBorrowedBinding {
                        definition: *definition,
                        borrower: borrower.to_string(),
                        source_definition: *source_definition,
                        source: source.clone(),
                        origin,
                    }
                }));
            }
            borrows.sort_by(|left, right| {
                left.definition
                    .cmp(&right.definition)
                    .then_with(|| left.borrower.cmp(&right.borrower))
                    .then_with(|| left.source_definition.cmp(&right.source_definition))
                    .then_with(|| left.source.cmp(&right.source))
                    .then_with(|| span_key(left.origin).cmp(&span_key(right.origin)))
            });
            borrows.dedup();
            ControlFlowBorrowState { borrows }
        })
        .collect()
}

/// Whether a normalized ownership node consumes one exact source definition
/// through a typed call boundary. A consuming call may have no parallel
/// `OwnershipMove` event, so implicit destruction must query the call's
/// definition identity directly rather than falling back to the source name.
fn ownership_node_consumes_definition(
    ownership: &ControlFlowOwnership,
    definition: ControlFlowDefinitionId,
) -> bool {
    ownership.calls.iter().any(|call| {
        call.argument_kinds.iter().enumerate().any(|(index, kind)| {
            *kind == OwnershipCallArgumentKind::Consuming
                && call
                    .consuming_argument_definitions_at(index)
                    .contains(&definition)
        })
    })
}

fn compute_drop_facts(graph: &ControlFlowGraph) -> Vec<(ControlFlowNodeId, OwnershipDrop)> {
    let is_non_copy_storage = |ty: &Type| {
        matches!(ty, Type::List(_) | Type::Set(_) | Type::Map(_, _))
            || matches!(
                ty,
                Type::Optional(inner)
                    if matches!(
                        inner.as_ref(),
                        Type::List(_) | Type::Set(_) | Type::Map(_, _)
                    )
            )
    };
    let mut drops = Vec::new();

    for node in &graph.nodes {
        if !graph.is_reachable(node.id) {
            continue;
        }
        let Some(live_before) = graph.definition_live_before(node.id) else {
            continue;
        };
        let Some(live_after) = graph.definition_live_after(node.id) else {
            continue;
        };
        let moved = graph.move_state_before(node.id);

        let mut candidates = live_before.live().to_vec();
        candidates.sort();
        for definition in candidates {
            // A consuming boundary at this node transfers or releases the
            // definition before any implicit end-of-liveness release could
            // be emitted.  In particular, `drop(values)` records its move
            // on the same normalized evaluation node where `values` leaves
            // the live set; looking only at the incoming move state would
            // incorrectly manufacture a second drop for the consumed value.
            let consumed_by_move = node
                .ownership
                .moves
                .iter()
                .any(|movement| movement.source_definitions.contains(&definition));
            let consumed_by_call = ownership_node_consumes_definition(&node.ownership, definition);
            if consumed_by_move || consumed_by_call {
                continue;
            }
            if live_after.contains(definition)
                || moved.is_some_and(|state| state.is_definition_moved(definition))
            {
                continue;
            }

            let Some((name, ty, span)) = definition_details(graph, definition) else {
                continue;
            };
            if !is_non_copy_storage(&ty) {
                continue;
            }
            let consumed_on_reachable_path = graph.nodes.iter().any(|move_node| {
                graph.is_reachable(move_node.id)
                    && graph
                        .move_state_before(move_node.id)
                        .is_some_and(|state| state.is_definition_moved(definition))
                    && cfg_can_reach(graph, node.id, move_node.id)
            });
            if consumed_on_reachable_path {
                // The transfer consumes the reaching source at this node;
                // its destination, not the source definition, owns the next
                // eventual drop point.
                continue;
            }

            // A last use is not a drop if one of the reachable successor
            // states still borrows the definition. This is the same edge
            // boundary used by inferred borrow lifetimes and prevents a
            // future resource destructor from running before a view ends.
            let borrowed_by_successor = graph
                .outgoing(node.id)
                .filter(|edge| graph.is_reachable(edge.to))
                .filter_map(|edge| graph.borrow_state_before(edge.to))
                .any(|state| {
                    state
                        .borrows()
                        .iter()
                        .any(|borrow| borrow.source_definition == definition)
                });
            if borrowed_by_successor {
                continue;
            }

            drops.push((
                node.id,
                OwnershipDrop {
                    definition,
                    value: graph.definition_value(definition),
                    name,
                    span,
                },
            ));
        }
    }
    drops.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.definition.cmp(&right.1.definition))
    });
    drops.dedup();
    drops
}

fn cfg_can_reach(
    graph: &ControlFlowGraph,
    from: ControlFlowNodeId,
    target: ControlFlowNodeId,
) -> bool {
    if from == target {
        return true;
    }
    let mut pending = vec![from];
    let mut visited = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !visited.insert(current) {
            continue;
        }
        for edge in graph.outgoing(current) {
            if !graph.is_reachable(edge.to) {
                continue;
            }
            if edge.to == target {
                return true;
            }
            pending.push(edge.to);
        }
    }
    false
}

fn definition_details(
    graph: &ControlFlowGraph,
    definition: ControlFlowDefinitionId,
) -> Option<(String, Type, SourceSpan)> {
    match definition {
        ControlFlowDefinitionId::Parameter(index) => graph
            .parameters
            .get(index)
            .map(|parameter| (parameter.name.clone(), parameter.ty.clone(), parameter.span)),
        ControlFlowDefinitionId::Node { node, index } => graph
            .nodes
            .get(node.0)
            .and_then(|node| node.definitions.get(index))
            .map(|definition| {
                (
                    definition.name.clone(),
                    definition.ty.clone(),
                    definition.span,
                )
            }),
        ControlFlowDefinitionId::Scoped { node, index } => graph
            .scoped_definitions
            .get(node.0)
            .and_then(|definitions| definitions.get(index))
            .map(|definition| {
                (
                    definition.name.clone(),
                    definition.ty.clone(),
                    definition.span,
                )
            }),
    }
}

fn compute_move_states(
    nodes: &[ControlFlowNode],
    edges: &[ControlFlowEdge],
    entry: ControlFlowNodeId,
) -> Vec<ControlFlowMoveState> {
    let mut states =
        vec![
            None::<BTreeMap<ControlFlowDefinitionId, (String, Vec<Vec<String>>, SourceSpan)>>;
            nodes.len()
        ];
    states[entry.0] = Some(BTreeMap::new());
    let mut queue = VecDeque::from([entry]);

    while let Some(id) = queue.pop_front() {
        let Some(mut outgoing_state) = states[id.0].clone() else {
            continue;
        };
        for (index, _) in nodes[id.0].definitions.iter().enumerate() {
            outgoing_state.remove(&ControlFlowDefinitionId::Node { node: id, index });
        }
        for movement in &nodes[id.0].ownership.moves {
            if movement.source_definitions.is_empty() {
                // Internal CFG construction populates this field before
                // propagation. Keep incomplete synthetic facts harmless for
                // callers that construct graphs in isolation.
                continue;
            }
            for definition in &movement.source_definitions {
                outgoing_state
                    .entry(*definition)
                    .and_modify(|(_, projections, origin)| {
                        insert_move_projection(projections, &movement.projection);
                        if span_key(movement.span) < span_key(*origin) {
                            *origin = movement.span;
                        }
                    })
                    .or_insert((
                        movement.source.clone(),
                        vec![movement.projection.clone()],
                        movement.span,
                    ));
            }
        }
        // Consuming calls are ownership boundaries in their own right.  The
        // explicit `drop` lowering currently emits a matching OwnershipMove,
        // but future owned/resource calls may be represented only by their
        // typed call mode.  Consume the normalized argument definitions here
        // so move-state propagation cannot silently depend on a parallel
        // source-shaped event.
        for call in &nodes[id.0].ownership.calls {
            if !call.is_consuming() {
                continue;
            }
            for (index, kind) in call.argument_kinds.iter().enumerate() {
                if *kind != OwnershipCallArgumentKind::Consuming {
                    continue;
                }
                for definition in call.argument_definitions_at(index) {
                    outgoing_state
                        .entry(*definition)
                        .and_modify(|(_, projections, origin)| {
                            // A consuming boundary consumes the complete
                            // argument.  It must therefore subsume any
                            // projected move already recorded for this
                            // definition on the same CFG path, rather than
                            // being ignored by `or_insert`.
                            insert_move_projection(projections, &[]);
                            if span_key(call.span) < span_key(*origin) {
                                *origin = call.span;
                            }
                        })
                        .or_insert((call.callee.clone(), vec![Vec::new()], call.span));
                }
            }
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
                    .flat_map(|(definition, (name, projections, origin))| {
                        projections
                            .into_iter()
                            .map(move |projection| OwnershipMovedBinding {
                                definition,
                                name: name.clone(),
                                projection,
                                origin,
                            })
                    })
                    .collect(),
            },
            None => ControlFlowMoveState::default(),
        })
        .collect()
}

fn merge_move_state(
    target: &mut BTreeMap<ControlFlowDefinitionId, (String, Vec<Vec<String>>, SourceSpan)>,
    incoming: &BTreeMap<ControlFlowDefinitionId, (String, Vec<Vec<String>>, SourceSpan)>,
) -> bool {
    let mut changed = false;
    for (definition, (name, projections, origin)) in incoming {
        match target.get_mut(definition) {
            Some((_, existing_projections, existing)) => {
                for projection in projections {
                    changed |= insert_move_projection(existing_projections, projection);
                }
                if span_key(*origin) < span_key(*existing) {
                    *existing = *origin;
                    changed = true;
                }
            }
            None => {
                target.insert(*definition, (name.clone(), projections.clone(), *origin));
                changed = true;
            }
        }
    }
    changed
}

/// Insert one consumed projection while retaining the minimal prefix set.
/// An empty path denotes the whole value and therefore subsumes every other
/// path. A shorter path subsumes an existing longer projection; disjoint
/// projections remain independently queryable.
fn insert_move_projection(projections: &mut Vec<Vec<String>>, incoming: &[String]) -> bool {
    if projections.iter().any(Vec::is_empty) {
        return false;
    }
    if incoming.is_empty() {
        let changed = !projections.iter().any(Vec::is_empty);
        projections.clear();
        projections.push(Vec::new());
        return changed;
    }
    if projections
        .iter()
        .any(|existing| incoming.starts_with(existing))
    {
        return false;
    }
    projections.retain(|existing| !existing.starts_with(incoming));
    projections.push(incoming.to_vec());
    projections.sort();
    projections.dedup();
    true
}

fn span_key(span: SourceSpan) -> (u32, usize, usize, usize) {
    (span.source_id.value(), span.line, span.column, span.length)
}

#[cfg(test)]
mod tests {
    use super::{
        ControlFlowDefinitionId, ControlFlowEdge, ControlFlowEdgeKind, ControlFlowMoveState,
        ControlFlowNode, ControlFlowNodeId, ControlFlowNodeKind, ControlFlowOwnership,
        OwnershipCall, OwnershipCallArgumentKind, OwnershipMove, OwnershipMovedBinding,
        compute_move_states, insert_move_projection, ownership_node_consumes_definition,
    };
    use crate::diagnostic::SourceSpan;

    fn path(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    #[test]
    fn partial_move_projection_set_keeps_disjoint_paths_and_minimizes_prefixes() {
        let mut projections = Vec::new();
        assert!(insert_move_projection(
            &mut projections,
            &path(&["left", "value"])
        ));
        assert!(insert_move_projection(
            &mut projections,
            &path(&["right", "value"])
        ));
        assert_eq!(
            projections,
            vec![path(&["left", "value"]), path(&["right", "value"])]
        );
        assert!(insert_move_projection(&mut projections, &path(&["left"])));
        assert_eq!(
            projections,
            vec![path(&["left"]), path(&["right", "value"])]
        );
        assert!(!insert_move_projection(
            &mut projections,
            &path(&["left", "other"])
        ));
        assert!(insert_move_projection(&mut projections, &[]));
        assert_eq!(projections, vec![Vec::<String>::new()]);
        assert!(!insert_move_projection(&mut projections, &path(&["right"])));
    }

    #[test]
    fn whole_consuming_projection_subsumes_prior_partial_projection() {
        let definition = ControlFlowDefinitionId::Parameter(0);
        let mut state = std::collections::BTreeMap::new();
        state.insert(
            definition,
            (
                "value".to_string(),
                vec![path(&["left"])],
                SourceSpan::new(1, 10, 1),
            ),
        );

        assert!(insert_move_projection(
            &mut state.get_mut(&definition).unwrap().1,
            &[]
        ));
        assert_eq!(state[&definition].1, vec![Vec::<String>::new()]);
    }

    #[test]
    fn cfg_consuming_call_subsumes_prior_partial_move() {
        let definition = ControlFlowDefinitionId::Parameter(0);
        let span = SourceSpan::new(1, 1, 1);
        let nodes = vec![
            ControlFlowNode {
                id: ControlFlowNodeId(0),
                kind: ControlFlowNodeKind::Entry,
                span,
                value_types: Vec::new(),
                values: Vec::new(),
                definitions: Vec::new(),
                ownership: ControlFlowOwnership::default(),
            },
            ControlFlowNode {
                id: ControlFlowNodeId(1),
                kind: ControlFlowNodeKind::Statement,
                span,
                value_types: Vec::new(),
                values: Vec::new(),
                definitions: Vec::new(),
                ownership: ControlFlowOwnership {
                    moves: vec![OwnershipMove {
                        source: "value".to_string(),
                        destination: "field".to_string(),
                        projection: path(&["left"]),
                        value: None,
                        source_definitions: vec![definition],
                        span,
                    }],
                    calls: vec![OwnershipCall {
                        callee: "consume".to_string(),
                        arguments: vec![],
                        argument_kinds: vec![OwnershipCallArgumentKind::Consuming],
                        argument_definitions: vec![vec![definition]],
                        borrowed_argument_definitions: vec![Vec::new()],
                        span,
                    }],
                    ..ControlFlowOwnership::default()
                },
            },
            ControlFlowNode {
                id: ControlFlowNodeId(2),
                kind: ControlFlowNodeKind::Exit,
                span,
                value_types: Vec::new(),
                values: Vec::new(),
                definitions: Vec::new(),
                ownership: ControlFlowOwnership::default(),
            },
        ];
        let edges = vec![
            ControlFlowEdge {
                from: ControlFlowNodeId(0),
                to: ControlFlowNodeId(1),
                kind: ControlFlowEdgeKind::Next,
            },
            ControlFlowEdge {
                from: ControlFlowNodeId(1),
                to: ControlFlowNodeId(2),
                kind: ControlFlowEdgeKind::Next,
            },
        ];

        let states = compute_move_states(&nodes, &edges, ControlFlowNodeId(0));
        assert_eq!(
            states[2]
                .moved_projections_for_definition(definition)
                .map(|binding| binding.projection.clone())
                .collect::<Vec<_>>(),
            vec![Vec::<String>::new()]
        );
    }

    #[test]
    fn consuming_call_drop_suppression_uses_definition_identity() {
        let definition = ControlFlowDefinitionId::Parameter(3);
        let sibling = ControlFlowDefinitionId::Parameter(4);
        let span = SourceSpan::new(1, 1, 1);
        let ownership = ControlFlowOwnership {
            calls: vec![OwnershipCall {
                callee: "consume".to_string(),
                arguments: vec![],
                argument_kinds: vec![OwnershipCallArgumentKind::Consuming],
                argument_definitions: vec![vec![definition]],
                borrowed_argument_definitions: vec![Vec::new()],
                span,
            }],
            ..ControlFlowOwnership::default()
        };

        assert!(ownership_node_consumes_definition(&ownership, definition));
        assert!(!ownership_node_consumes_definition(&ownership, sibling));
    }

    #[test]
    fn normalized_move_queries_use_prefix_overlap_and_preserve_definition_identity() {
        let definition = ControlFlowDefinitionId::Node {
            node: super::ControlFlowNodeId(4),
            index: 2,
        };
        let sibling = ControlFlowDefinitionId::Node {
            node: super::ControlFlowNodeId(5),
            index: 2,
        };
        let move_event = OwnershipMove {
            source: "record".to_string(),
            destination: "field_value".to_string(),
            projection: path(&["payload", "bytes"]),
            value: None,
            source_definitions: vec![definition],
            span: SourceSpan::new(3, 4, 1),
        };
        assert!(move_event.overlaps_projection(&path(&["payload"])));
        assert!(move_event.overlaps_projection(&[]));
        assert!(!move_event.overlaps_projection(&path(&["other"])));

        let state = ControlFlowMoveState {
            reachable: true,
            moved: vec![
                OwnershipMovedBinding {
                    definition,
                    name: "record".to_string(),
                    projection: path(&["payload", "bytes"]),
                    origin: SourceSpan::new(3, 4, 1),
                },
                OwnershipMovedBinding {
                    definition: sibling,
                    name: "record".to_string(),
                    projection: path(&["other"]),
                    origin: SourceSpan::new(8, 4, 1),
                },
            ],
        };
        let projections = state
            .moved_projections_for_definition(definition)
            .map(|binding| binding.projection.clone())
            .collect::<Vec<_>>();
        assert_eq!(projections, vec![path(&["payload", "bytes"])]);
        assert!(!state.is_definition_whole_moved(definition));
        assert!(state.is_definition_partially_moved(definition));
        assert!(state.is_definition_partially_moved(sibling));
        assert!(!state.is_definition_projection_moved(definition, &path(&["other"])));
        assert!(state.is_definition_projection_moved(definition, &path(&["payload"])));
    }

    #[test]
    fn normalized_move_queries_distinguish_whole_value_consumption() {
        let definition = ControlFlowDefinitionId::Node {
            node: super::ControlFlowNodeId(7),
            index: 1,
        };
        let state = ControlFlowMoveState {
            reachable: true,
            moved: vec![OwnershipMovedBinding {
                definition,
                name: "value".to_string(),
                projection: Vec::new(),
                origin: SourceSpan::new(11, 2, 1),
            }],
        };
        assert!(state.is_definition_whole_moved(definition));
        assert!(!state.is_definition_partially_moved(definition));
        assert!(state.is_definition_projection_moved(definition, &["nested".to_string()]));
    }
}
