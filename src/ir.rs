use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use crate::ast::{BinOp, Expr, ExprKind, Function, RecordTypeField, Stmt, StmtKind, Type, UnaryOp};
use crate::diagnostic::{SourceId, SourceSpan};
use crate::typecheck::{self, ConstantValue, Signatures};

fn is_non_copy_collection_type(ty: &Type) -> bool {
    matches!(ty, Type::List(_) | Type::Set(_) | Type::Map(_, _))
        || matches!(ty, Type::Optional(inner) if matches!(inner.as_ref(), Type::List(_) | Type::Set(_) | Type::Map(_, _)))
}

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
pub struct ControlFlowStructPattern {
    pub name: String,
    pub fields: Vec<ControlFlowStructPatternField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFlowStructPatternField {
    pub field: String,
    pub binding: String,
    pub nested: Option<Box<ControlFlowStructPattern>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlowPatternLogicalOp {
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlFlowMatchPattern {
    Binding {
        name: String,
    },
    Struct(ControlFlowStructPattern),
    Relational {
        op: BinOp,
        value: ConstantValue,
    },
    Logical {
        left: Box<ControlFlowMatchPattern>,
        op: ControlFlowPatternLogicalOp,
        right: Box<ControlFlowMatchPattern>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFlowMatchArmPattern {
    pub enum_name: String,
    pub variant: String,
    pub patterns: Vec<ControlFlowMatchPattern>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFlowListRestPattern {
    pub binding: String,
    pub index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFlowMapPatternEntry {
    pub key: ConstantValue,
    pub binding: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlFlowListMatchPattern {
    List {
        bindings: Vec<String>,
        rest: Option<ControlFlowListRestPattern>,
    },
    Wildcard,
    Map {
        entries: Vec<ControlFlowMapPatternEntry>,
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
        params: Vec<(String, Type)>,
        return_type: Option<Type>,
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
    NamedCall {
        callee: String,
        arguments: Vec<ControlFlowValueId>,
        argument_names: Vec<Option<String>>,
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
        optional: bool,
    },
    ListOptional {
        value: ControlFlowValueId,
    },
    ListIf {
        condition: ControlFlowValueId,
        binding: Option<String>,
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
        binding: String,
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
    NamedQualifiedCall {
        namespace: String,
        name: String,
        arguments: Vec<ControlFlowValueId>,
        argument_names: Vec<Option<String>>,
    },
    InterfaceDispatch {
        interface: String,
        capability: String,
        target: Option<String>,
        mapped_function: Option<String>,
        arguments: Vec<ControlFlowValueId>,
    },
    NamedInterfaceDispatch {
        interface: String,
        capability: String,
        target: Option<String>,
        mapped_function: Option<String>,
        arguments: Vec<ControlFlowValueId>,
        argument_names: Vec<Option<String>>,
    },
    Field {
        base: ControlFlowValueId,
        name: String,
        optional: bool,
    },
    Match {
        value: ControlFlowValueId,
        guards: Vec<Option<ControlFlowValueId>>,
        arms: Vec<ControlFlowValueId>,
        arm_patterns: Vec<ControlFlowMatchArmPattern>,
    },
    ListMatch {
        value: ControlFlowValueId,
        guards: Vec<Option<ControlFlowValueId>>,
        arms: Vec<ControlFlowValueId>,
        arm_patterns: Vec<ControlFlowListMatchPattern>,
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
    EffectOnly,
}

impl ControlFlowValueOwnership {
    pub const fn is_copy(self) -> bool {
        matches!(self, Self::Copy)
    }

    pub const fn is_borrow(self) -> bool {
        matches!(self, Self::ImmutableBorrow)
    }

    pub const fn is_effect_only(self) -> bool {
        matches!(self, Self::EffectOnly)
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
    /// Named aggregate projection for each argument, when the argument is a
    /// field chain rooted in a value. Keeping this parallel to `arguments`
    /// lets future consuming-call analysis distinguish `consume(value)` from
    /// `consume(value.field)` without rebuilding the expression tree.
    pub argument_projections: Vec<Vec<String>>,
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

    /// Returns the named aggregate projection for one argument, if present.
    pub fn argument_projection_at(&self, index: usize) -> &[String] {
        self.argument_projections
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

    /// Return the normalized ownership facts for one immutable-borrow argument.
    ///
    /// Keeping the projected path, exact borrowed source definitions, and typed
    /// value identity together gives future partial-move validation one stable
    /// boundary for overlap checks without indexing parallel call vectors or
    /// rebuilding the checked expression tree.
    pub fn borrowed_argument_at(
        &self,
        index: usize,
    ) -> Option<(
        &[String],
        &[ControlFlowDefinitionId],
        Option<ControlFlowValueId>,
    )> {
        if self.argument_kind(index) != Some(OwnershipCallArgumentKind::ImmutableBorrow) {
            return None;
        }
        Some((
            self.argument_projection_at(index),
            self.borrowed_argument_definitions_at(index),
            self.arguments.get(index).copied(),
        ))
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

    /// Return the normalized ownership facts for one consuming argument.
    ///
    /// The returned tuple keeps the projected path and exact reaching
    /// definitions together.  Consumers of future owned aggregates can use
    /// this boundary without independently indexing the parallel call
    /// vectors (and without accidentally treating an immutable borrow as a
    /// consuming projection).
    pub fn consuming_argument_at(
        &self,
        index: usize,
    ) -> Option<(
        &[String],
        &[ControlFlowDefinitionId],
        Option<ControlFlowValueId>,
    )> {
        if self.argument_kind(index) != Some(OwnershipCallArgumentKind::Consuming) {
            return None;
        }
        Some((
            self.argument_projection_at(index),
            self.consuming_argument_definitions_at(index),
            self.arguments.get(index).copied(),
        ))
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

/// A normalized immutable-borrow boundary on a CFG edge.
///
/// Borrow starts and ends are edge facts rather than node-local evaluations.
/// Keeping them in one borrowed view lets ownership consumers follow the
/// complete lifetime boundary stream without independently correlating the
/// start/end vectors or rebuilding borrow liveness from source syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlowBorrowBoundary<'a> {
    Start(&'a OwnershipBorrowStart),
    End(&'a OwnershipBorrowEnd),
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

const PERSISTED_IR_MAGIC: &[u8] = b"FLUXIR3\0";
const PERSISTED_IR_MAX_ITEMS: usize = 1_000_000;
const PERSISTED_IR_MAX_STRING_BYTES: usize = 16 * 1024 * 1024;

struct PersistedIrReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> PersistedIrReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read_exact(&mut self, length: usize) -> Option<&'a [u8]> {
        let end = self.offset.checked_add(length)?;
        let value = self.bytes.get(self.offset..end)?;
        self.offset = end;
        Some(value)
    }

    fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

trait PersistedIrCodec: Sized {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>);
    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self>;
}

impl PersistedIrCodec for u8 {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        bytes.push(*self);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        reader
            .read_exact(1)
            .and_then(|bytes| bytes.first().copied())
    }
}

impl PersistedIrCodec for bool {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        u8::from(*self).encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}

impl PersistedIrCodec for usize {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&(*self as u64).to_le_bytes());
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        let raw: [u8; 8] = reader.read_exact(8)?.try_into().ok()?;
        usize::try_from(u64::from_le_bytes(raw)).ok()
    }
}

impl PersistedIrCodec for u32 {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&self.to_le_bytes());
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        let raw: [u8; 4] = reader.read_exact(4)?.try_into().ok()?;
        Some(u32::from_le_bytes(raw))
    }
}

impl PersistedIrCodec for i64 {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&self.to_le_bytes());
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        let raw: [u8; 8] = reader.read_exact(8)?.try_into().ok()?;
        Some(i64::from_le_bytes(raw))
    }
}

impl PersistedIrCodec for String {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.len().encode_cache_value(bytes);
        bytes.extend_from_slice(self.as_bytes());
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        let length = usize::decode_cache_value(reader)?;
        if length > PERSISTED_IR_MAX_STRING_BYTES {
            return None;
        }
        String::from_utf8(reader.read_exact(length)?.to_vec()).ok()
    }
}

impl<T: PersistedIrCodec> PersistedIrCodec for Option<T> {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        match self {
            Some(value) => {
                1u8.encode_cache_value(bytes);
                value.encode_cache_value(bytes);
            }
            None => 0u8.encode_cache_value(bytes),
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(None),
            1 => Some(Some(T::decode_cache_value(reader)?)),
            _ => None,
        }
    }
}

impl<T: PersistedIrCodec> PersistedIrCodec for Vec<T> {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.len().encode_cache_value(bytes);
        for value in self {
            value.encode_cache_value(bytes);
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        let length = usize::decode_cache_value(reader)?;
        if length > PERSISTED_IR_MAX_ITEMS {
            return None;
        }
        let mut values = Vec::with_capacity(length);
        for _ in 0..length {
            values.push(T::decode_cache_value(reader)?);
        }
        Some(values)
    }
}

impl<T: PersistedIrCodec> PersistedIrCodec for Box<T> {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.as_ref().encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Box::new(T::decode_cache_value(reader)?))
    }
}

impl<T> PersistedIrCodec for BTreeSet<T>
where
    T: PersistedIrCodec + Ord,
{
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.len().encode_cache_value(bytes);
        for value in self {
            value.encode_cache_value(bytes);
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        let length = usize::decode_cache_value(reader)?;
        if length > PERSISTED_IR_MAX_ITEMS {
            return None;
        }
        let mut values = BTreeSet::new();
        for _ in 0..length {
            if !values.insert(T::decode_cache_value(reader)?) {
                return None;
            }
        }
        Some(values)
    }
}

impl<K, V> PersistedIrCodec for BTreeMap<K, V>
where
    K: PersistedIrCodec + Ord,
    V: PersistedIrCodec,
{
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.len().encode_cache_value(bytes);
        for (key, value) in self {
            key.encode_cache_value(bytes);
            value.encode_cache_value(bytes);
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        let length = usize::decode_cache_value(reader)?;
        if length > PERSISTED_IR_MAX_ITEMS {
            return None;
        }
        let mut values = BTreeMap::new();
        for _ in 0..length {
            let key = K::decode_cache_value(reader)?;
            let value = V::decode_cache_value(reader)?;
            if values.insert(key, value).is_some() {
                return None;
            }
        }
        Some(values)
    }
}

impl<A: PersistedIrCodec, B: PersistedIrCodec> PersistedIrCodec for (A, B) {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.0.encode_cache_value(bytes);
        self.1.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some((
            A::decode_cache_value(reader)?,
            B::decode_cache_value(reader)?,
        ))
    }
}

impl PersistedIrCodec for RecordTypeField {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.name.encode_cache_value(bytes);
        self.ty.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            name: Option::<String>::decode_cache_value(reader)?,
            ty: Type::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for Type {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        match self {
            Type::I64 => 0u8.encode_cache_value(bytes),
            Type::Bool => 1u8.encode_cache_value(bytes),
            Type::Str => 2u8.encode_cache_value(bytes),
            Type::Error => 3u8.encode_cache_value(bytes),
            Type::Void => 4u8.encode_cache_value(bytes),
            Type::Named(name) => {
                5u8.encode_cache_value(bytes);
                name.encode_cache_value(bytes);
            }
            Type::List(inner) => {
                6u8.encode_cache_value(bytes);
                inner.encode_cache_value(bytes);
            }
            Type::Set(inner) => {
                7u8.encode_cache_value(bytes);
                inner.encode_cache_value(bytes);
            }
            Type::Map(key, value) => {
                8u8.encode_cache_value(bytes);
                key.encode_cache_value(bytes);
                value.encode_cache_value(bytes);
            }
            Type::Optional(inner) => {
                9u8.encode_cache_value(bytes);
                inner.encode_cache_value(bytes);
            }
            Type::Record(fields) => {
                10u8.encode_cache_value(bytes);
                fields.encode_cache_value(bytes);
            }
            Type::Function { params, returns } => {
                11u8.encode_cache_value(bytes);
                params.encode_cache_value(bytes);
                returns.encode_cache_value(bytes);
            }
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(Type::I64),
            1 => Some(Type::Bool),
            2 => Some(Type::Str),
            3 => Some(Type::Error),
            4 => Some(Type::Void),
            5 => Some(Type::Named(String::decode_cache_value(reader)?)),
            6 => Some(Type::List(Box::<Type>::decode_cache_value(reader)?)),
            7 => Some(Type::Set(Box::<Type>::decode_cache_value(reader)?)),
            8 => Some(Type::Map(
                Box::<Type>::decode_cache_value(reader)?,
                Box::<Type>::decode_cache_value(reader)?,
            )),
            9 => Some(Type::Optional(Box::<Type>::decode_cache_value(reader)?)),
            10 => Some(Type::Record(Vec::<RecordTypeField>::decode_cache_value(
                reader,
            )?)),
            11 => Some(Type::Function {
                params: Vec::<Type>::decode_cache_value(reader)?,
                returns: Vec::<Type>::decode_cache_value(reader)?,
            }),
            _ => None,
        }
    }
}

impl PersistedIrCodec for SourceSpan {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.source_id.value().encode_cache_value(bytes);
        self.line.encode_cache_value(bytes);
        self.column.encode_cache_value(bytes);
        self.length.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            source_id: SourceId::new(u32::decode_cache_value(reader)?),
            line: usize::decode_cache_value(reader)?,
            column: usize::decode_cache_value(reader)?,
            length: usize::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ConstantValue {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        match self {
            ConstantValue::I64(value) => {
                0u8.encode_cache_value(bytes);
                value.encode_cache_value(bytes);
            }
            ConstantValue::Bool(value) => {
                1u8.encode_cache_value(bytes);
                value.encode_cache_value(bytes);
            }
            ConstantValue::Str(value) => {
                2u8.encode_cache_value(bytes);
                value.encode_cache_value(bytes);
            }
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(ConstantValue::I64(i64::decode_cache_value(reader)?)),
            1 => Some(ConstantValue::Bool(bool::decode_cache_value(reader)?)),
            2 => Some(ConstantValue::Str(String::decode_cache_value(reader)?)),
            _ => None,
        }
    }
}

impl PersistedIrCodec for UnaryOp {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        let tag: u8 = match self {
            UnaryOp::Neg => 0,
            UnaryOp::Not => 1,
            UnaryOp::Borrow => 2,
        };
        tag.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(UnaryOp::Neg),
            1 => Some(UnaryOp::Not),
            2 => Some(UnaryOp::Borrow),
            _ => None,
        }
    }
}

impl PersistedIrCodec for BinOp {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        let tag: u8 = match self {
            BinOp::Add => 0,
            BinOp::Sub => 1,
            BinOp::Mul => 2,
            BinOp::Div => 3,
            BinOp::Eq => 4,
            BinOp::Ne => 5,
            BinOp::Lt => 6,
            BinOp::Le => 7,
            BinOp::Gt => 8,
            BinOp::Ge => 9,
            BinOp::And => 10,
            BinOp::Or => 11,
            BinOp::Coalesce => 12,
        };
        tag.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(BinOp::Add),
            1 => Some(BinOp::Sub),
            2 => Some(BinOp::Mul),
            3 => Some(BinOp::Div),
            4 => Some(BinOp::Eq),
            5 => Some(BinOp::Ne),
            6 => Some(BinOp::Lt),
            7 => Some(BinOp::Le),
            8 => Some(BinOp::Gt),
            9 => Some(BinOp::Ge),
            10 => Some(BinOp::And),
            11 => Some(BinOp::Or),
            12 => Some(BinOp::Coalesce),
            _ => None,
        }
    }
}

impl PersistedIrCodec for ControlFlowStructPattern {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.name.encode_cache_value(bytes);
        self.fields.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            name: String::decode_cache_value(reader)?,
            fields: Vec::<ControlFlowStructPatternField>::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowStructPatternField {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.field.encode_cache_value(bytes);
        self.binding.encode_cache_value(bytes);
        self.nested.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            field: String::decode_cache_value(reader)?,
            binding: String::decode_cache_value(reader)?,
            nested: Option::<Box<ControlFlowStructPattern>>::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowPatternLogicalOp {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::And => 0u8,
            Self::Or => 1u8,
        }
        .encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(Self::And),
            1 => Some(Self::Or),
            _ => None,
        }
    }
}

impl PersistedIrCodec for ControlFlowMatchPattern {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::Binding { name } => {
                0u8.encode_cache_value(bytes);
                name.encode_cache_value(bytes);
            }
            Self::Struct(pattern) => {
                1u8.encode_cache_value(bytes);
                pattern.encode_cache_value(bytes);
            }
            Self::Relational { op, value } => {
                2u8.encode_cache_value(bytes);
                op.encode_cache_value(bytes);
                value.encode_cache_value(bytes);
            }
            Self::Logical { left, op, right } => {
                3u8.encode_cache_value(bytes);
                left.encode_cache_value(bytes);
                op.encode_cache_value(bytes);
                right.encode_cache_value(bytes);
            }
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(Self::Binding {
                name: String::decode_cache_value(reader)?,
            }),
            1 => Some(Self::Struct(ControlFlowStructPattern::decode_cache_value(
                reader,
            )?)),
            2 => Some(Self::Relational {
                op: BinOp::decode_cache_value(reader)?,
                value: ConstantValue::decode_cache_value(reader)?,
            }),
            3 => Some(Self::Logical {
                left: Box::<ControlFlowMatchPattern>::decode_cache_value(reader)?,
                op: ControlFlowPatternLogicalOp::decode_cache_value(reader)?,
                right: Box::<ControlFlowMatchPattern>::decode_cache_value(reader)?,
            }),
            _ => None,
        }
    }
}

impl PersistedIrCodec for ControlFlowMatchArmPattern {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.enum_name.encode_cache_value(bytes);
        self.variant.encode_cache_value(bytes);
        self.patterns.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            enum_name: String::decode_cache_value(reader)?,
            variant: String::decode_cache_value(reader)?,
            patterns: Vec::<ControlFlowMatchPattern>::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowListRestPattern {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.binding.encode_cache_value(bytes);
        self.index.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            binding: String::decode_cache_value(reader)?,
            index: usize::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowMapPatternEntry {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.key.encode_cache_value(bytes);
        self.binding.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            key: ConstantValue::decode_cache_value(reader)?,
            binding: String::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowListMatchPattern {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::List { bindings, rest } => {
                0u8.encode_cache_value(bytes);
                bindings.encode_cache_value(bytes);
                rest.encode_cache_value(bytes);
            }
            Self::Wildcard => 1u8.encode_cache_value(bytes),
            Self::Map { entries } => {
                2u8.encode_cache_value(bytes);
                entries.encode_cache_value(bytes);
            }
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(Self::List {
                bindings: Vec::<String>::decode_cache_value(reader)?,
                rest: Option::<ControlFlowListRestPattern>::decode_cache_value(reader)?,
            }),
            1 => Some(Self::Wildcard),
            2 => Some(Self::Map {
                entries: Vec::<ControlFlowMapPatternEntry>::decode_cache_value(reader)?,
            }),
            _ => None,
        }
    }
}

impl PersistedIrCodec for ControlFlowNodeId {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.0.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self(usize::decode_cache_value(reader)?))
    }
}

impl PersistedIrCodec for ControlFlowValueId {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.0.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self(usize::decode_cache_value(reader)?))
    }
}

impl PersistedIrCodec for ControlFlowValueRegionId {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.0.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self(usize::decode_cache_value(reader)?))
    }
}

impl PersistedIrCodec for ControlFlowDefinitionId {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::Parameter(index) => {
                0u8.encode_cache_value(bytes);
                index.encode_cache_value(bytes);
            }
            Self::Node { node, index } => {
                1u8.encode_cache_value(bytes);
                node.encode_cache_value(bytes);
                index.encode_cache_value(bytes);
            }
            Self::Scoped { node, index } => {
                2u8.encode_cache_value(bytes);
                node.encode_cache_value(bytes);
                index.encode_cache_value(bytes);
            }
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(Self::Parameter(usize::decode_cache_value(reader)?)),
            1 => Some(Self::Node {
                node: ControlFlowNodeId::decode_cache_value(reader)?,
                index: usize::decode_cache_value(reader)?,
            }),
            2 => Some(Self::Scoped {
                node: ControlFlowNodeId::decode_cache_value(reader)?,
                index: usize::decode_cache_value(reader)?,
            }),
            _ => None,
        }
    }
}

impl PersistedIrCodec for ControlFlowValueKind {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::Literal => 0u8.encode_cache_value(bytes),
            Self::NameRead { name, definitions } => {
                1u8.encode_cache_value(bytes);
                name.encode_cache_value(bytes);
                definitions.encode_cache_value(bytes);
            }
            Self::AnonymousFunction {
                body,
                params,
                return_type,
            } => {
                31u8.encode_cache_value(bytes);
                body.encode_cache_value(bytes);
                params.encode_cache_value(bytes);
                return_type.encode_cache_value(bytes);
            }
            Self::Await { value } => {
                3u8.encode_cache_value(bytes);
                value.encode_cache_value(bytes);
            }
            Self::Call { callee, arguments } => {
                4u8.encode_cache_value(bytes);
                callee.encode_cache_value(bytes);
                arguments.encode_cache_value(bytes);
            }
            Self::NamedCall {
                callee,
                arguments,
                argument_names,
            } => {
                28u8.encode_cache_value(bytes);
                callee.encode_cache_value(bytes);
                arguments.encode_cache_value(bytes);
                argument_names.encode_cache_value(bytes);
            }
            Self::OptionalCascadeCall {
                optional,
                callee,
                arguments,
            } => {
                5u8.encode_cache_value(bytes);
                optional.encode_cache_value(bytes);
                callee.encode_cache_value(bytes);
                arguments.encode_cache_value(bytes);
            }
            Self::InterfacePack {
                interface,
                target,
                value,
            } => {
                6u8.encode_cache_value(bytes);
                interface.encode_cache_value(bytes);
                target.encode_cache_value(bytes);
                value.encode_cache_value(bytes);
            }
            Self::List { items } => {
                7u8.encode_cache_value(bytes);
                items.encode_cache_value(bytes);
            }
            Self::Set { items } => {
                8u8.encode_cache_value(bytes);
                items.encode_cache_value(bytes);
            }
            Self::Map { entries } => {
                9u8.encode_cache_value(bytes);
                entries.encode_cache_value(bytes);
            }
            Self::ListSpread { value, optional } => {
                10u8.encode_cache_value(bytes);
                value.encode_cache_value(bytes);
                optional.encode_cache_value(bytes);
            }
            Self::ListOptional { value } => {
                11u8.encode_cache_value(bytes);
                value.encode_cache_value(bytes);
            }
            Self::ListIf {
                condition,
                binding,
                value,
                else_value,
            } => {
                12u8.encode_cache_value(bytes);
                condition.encode_cache_value(bytes);
                binding.encode_cache_value(bytes);
                value.encode_cache_value(bytes);
                else_value.encode_cache_value(bytes);
            }
            Self::Index {
                base,
                index,
                optional,
            } => {
                13u8.encode_cache_value(bytes);
                base.encode_cache_value(bytes);
                index.encode_cache_value(bytes);
                optional.encode_cache_value(bytes);
            }
            Self::Slice {
                base,
                start,
                end,
                step,
            } => {
                14u8.encode_cache_value(bytes);
                base.encode_cache_value(bytes);
                start.encode_cache_value(bytes);
                end.encode_cache_value(bytes);
                step.encode_cache_value(bytes);
            }
            Self::ListComprehension {
                binding,
                iterable,
                value,
                condition,
            } => {
                15u8.encode_cache_value(bytes);
                binding.encode_cache_value(bytes);
                iterable.encode_cache_value(bytes);
                value.encode_cache_value(bytes);
                condition.encode_cache_value(bytes);
            }
            Self::StructLiteral { name, base, fields } => {
                16u8.encode_cache_value(bytes);
                name.encode_cache_value(bytes);
                base.encode_cache_value(bytes);
                fields.encode_cache_value(bytes);
            }
            Self::RecordLiteral { fields } => {
                17u8.encode_cache_value(bytes);
                fields.encode_cache_value(bytes);
            }
            Self::QualifiedCall {
                namespace,
                name,
                arguments,
            } => {
                18u8.encode_cache_value(bytes);
                namespace.encode_cache_value(bytes);
                name.encode_cache_value(bytes);
                arguments.encode_cache_value(bytes);
            }
            Self::NamedQualifiedCall {
                namespace,
                name,
                arguments,
                argument_names,
            } => {
                30u8.encode_cache_value(bytes);
                namespace.encode_cache_value(bytes);
                name.encode_cache_value(bytes);
                arguments.encode_cache_value(bytes);
                argument_names.encode_cache_value(bytes);
            }
            Self::InterfaceDispatch {
                interface,
                capability,
                target,
                mapped_function,
                arguments,
            } => {
                19u8.encode_cache_value(bytes);
                interface.encode_cache_value(bytes);
                capability.encode_cache_value(bytes);
                target.encode_cache_value(bytes);
                mapped_function.encode_cache_value(bytes);
                arguments.encode_cache_value(bytes);
            }
            Self::NamedInterfaceDispatch {
                interface,
                capability,
                target,
                mapped_function,
                arguments,
                argument_names,
            } => {
                29u8.encode_cache_value(bytes);
                interface.encode_cache_value(bytes);
                capability.encode_cache_value(bytes);
                target.encode_cache_value(bytes);
                mapped_function.encode_cache_value(bytes);
                arguments.encode_cache_value(bytes);
                argument_names.encode_cache_value(bytes);
            }
            Self::Field {
                base,
                name,
                optional,
            } => {
                27u8.encode_cache_value(bytes);
                base.encode_cache_value(bytes);
                name.encode_cache_value(bytes);
                optional.encode_cache_value(bytes);
            }
            Self::Match {
                value,
                guards,
                arms,
                arm_patterns,
            } => {
                21u8.encode_cache_value(bytes);
                value.encode_cache_value(bytes);
                guards.encode_cache_value(bytes);
                arms.encode_cache_value(bytes);
                arm_patterns.encode_cache_value(bytes);
            }
            Self::ListMatch {
                value,
                guards,
                arms,
                arm_patterns,
            } => {
                22u8.encode_cache_value(bytes);
                value.encode_cache_value(bytes);
                guards.encode_cache_value(bytes);
                arms.encode_cache_value(bytes);
                arm_patterns.encode_cache_value(bytes);
            }
            Self::Conditional {
                condition,
                then_value,
                else_value,
            } => {
                23u8.encode_cache_value(bytes);
                condition.encode_cache_value(bytes);
                then_value.encode_cache_value(bytes);
                else_value.encode_cache_value(bytes);
            }
            Self::Unary { op, operand } => {
                24u8.encode_cache_value(bytes);
                op.encode_cache_value(bytes);
                operand.encode_cache_value(bytes);
            }
            Self::Binary { op, left, right } => {
                25u8.encode_cache_value(bytes);
                op.encode_cache_value(bytes);
                left.encode_cache_value(bytes);
                right.encode_cache_value(bytes);
            }
            Self::Opaque => 26u8.encode_cache_value(bytes),
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(Self::Literal),
            1 => Some(Self::NameRead {
                name: String::decode_cache_value(reader)?,
                definitions: Vec::<ControlFlowDefinitionId>::decode_cache_value(reader)?,
            }),
            2 => Some(Self::AnonymousFunction {
                body: ControlFlowValueId::decode_cache_value(reader)?,
                params: Vec::new(),
                return_type: None,
            }),
            3 => Some(Self::Await {
                value: ControlFlowValueId::decode_cache_value(reader)?,
            }),
            4 => Some(Self::Call {
                callee: String::decode_cache_value(reader)?,
                arguments: Vec::<ControlFlowValueId>::decode_cache_value(reader)?,
            }),
            5 => Some(Self::OptionalCascadeCall {
                optional: ControlFlowValueId::decode_cache_value(reader)?,
                callee: String::decode_cache_value(reader)?,
                arguments: Vec::<ControlFlowValueId>::decode_cache_value(reader)?,
            }),
            6 => Some(Self::InterfacePack {
                interface: String::decode_cache_value(reader)?,
                target: String::decode_cache_value(reader)?,
                value: ControlFlowValueId::decode_cache_value(reader)?,
            }),
            7 => Some(Self::List {
                items: Vec::<ControlFlowValueId>::decode_cache_value(reader)?,
            }),
            8 => Some(Self::Set {
                items: Vec::<ControlFlowValueId>::decode_cache_value(reader)?,
            }),
            9 => Some(Self::Map {
                entries: Vec::<(ControlFlowValueId, ControlFlowValueId)>::decode_cache_value(
                    reader,
                )?,
            }),
            10 => Some(Self::ListSpread {
                value: ControlFlowValueId::decode_cache_value(reader)?,
                optional: bool::decode_cache_value(reader)?,
            }),
            11 => Some(Self::ListOptional {
                value: ControlFlowValueId::decode_cache_value(reader)?,
            }),
            12 => Some(Self::ListIf {
                condition: ControlFlowValueId::decode_cache_value(reader)?,
                binding: Option::<String>::decode_cache_value(reader)?,
                value: ControlFlowValueId::decode_cache_value(reader)?,
                else_value: Option::<ControlFlowValueId>::decode_cache_value(reader)?,
            }),
            13 => Some(Self::Index {
                base: ControlFlowValueId::decode_cache_value(reader)?,
                index: ControlFlowValueId::decode_cache_value(reader)?,
                optional: bool::decode_cache_value(reader)?,
            }),
            14 => Some(Self::Slice {
                base: ControlFlowValueId::decode_cache_value(reader)?,
                start: Option::<ControlFlowValueId>::decode_cache_value(reader)?,
                end: Option::<ControlFlowValueId>::decode_cache_value(reader)?,
                step: Option::<ControlFlowValueId>::decode_cache_value(reader)?,
            }),
            15 => Some(Self::ListComprehension {
                binding: String::decode_cache_value(reader)?,
                iterable: ControlFlowValueId::decode_cache_value(reader)?,
                value: ControlFlowValueId::decode_cache_value(reader)?,
                condition: Option::<ControlFlowValueId>::decode_cache_value(reader)?,
            }),
            16 => Some(Self::StructLiteral {
                name: String::decode_cache_value(reader)?,
                base: Option::<ControlFlowValueId>::decode_cache_value(reader)?,
                fields: Vec::<(String, ControlFlowValueId)>::decode_cache_value(reader)?,
            }),
            17 => Some(Self::RecordLiteral {
                fields: Vec::<(Option<String>, ControlFlowValueId)>::decode_cache_value(reader)?,
            }),
            18 => Some(Self::QualifiedCall {
                namespace: String::decode_cache_value(reader)?,
                name: String::decode_cache_value(reader)?,
                arguments: Vec::<ControlFlowValueId>::decode_cache_value(reader)?,
            }),
            19 => Some(Self::InterfaceDispatch {
                interface: String::decode_cache_value(reader)?,
                capability: String::decode_cache_value(reader)?,
                target: Option::<String>::decode_cache_value(reader)?,
                mapped_function: Option::<String>::decode_cache_value(reader)?,
                arguments: Vec::<ControlFlowValueId>::decode_cache_value(reader)?,
            }),
            20 => Some(Self::Field {
                base: ControlFlowValueId::decode_cache_value(reader)?,
                name: String::decode_cache_value(reader)?,
                optional: false,
            }),
            21 => Some(Self::Match {
                value: ControlFlowValueId::decode_cache_value(reader)?,
                guards: Vec::<Option<ControlFlowValueId>>::decode_cache_value(reader)?,
                arms: Vec::<ControlFlowValueId>::decode_cache_value(reader)?,
                arm_patterns: Vec::<ControlFlowMatchArmPattern>::decode_cache_value(reader)?,
            }),
            22 => Some(Self::ListMatch {
                value: ControlFlowValueId::decode_cache_value(reader)?,
                guards: Vec::<Option<ControlFlowValueId>>::decode_cache_value(reader)?,
                arms: Vec::<ControlFlowValueId>::decode_cache_value(reader)?,
                arm_patterns: Vec::<ControlFlowListMatchPattern>::decode_cache_value(reader)?,
            }),
            23 => Some(Self::Conditional {
                condition: ControlFlowValueId::decode_cache_value(reader)?,
                then_value: ControlFlowValueId::decode_cache_value(reader)?,
                else_value: ControlFlowValueId::decode_cache_value(reader)?,
            }),
            24 => Some(Self::Unary {
                op: UnaryOp::decode_cache_value(reader)?,
                operand: ControlFlowValueId::decode_cache_value(reader)?,
            }),
            25 => Some(Self::Binary {
                op: BinOp::decode_cache_value(reader)?,
                left: ControlFlowValueId::decode_cache_value(reader)?,
                right: ControlFlowValueId::decode_cache_value(reader)?,
            }),
            26 => Some(Self::Opaque),
            27 => Some(Self::Field {
                base: ControlFlowValueId::decode_cache_value(reader)?,
                name: String::decode_cache_value(reader)?,
                optional: bool::decode_cache_value(reader)?,
            }),
            28 => Some(Self::NamedCall {
                callee: String::decode_cache_value(reader)?,
                arguments: Vec::<ControlFlowValueId>::decode_cache_value(reader)?,
                argument_names: Vec::<Option<String>>::decode_cache_value(reader)?,
            }),
            29 => Some(Self::NamedInterfaceDispatch {
                interface: String::decode_cache_value(reader)?,
                capability: String::decode_cache_value(reader)?,
                target: Option::<String>::decode_cache_value(reader)?,
                mapped_function: Option::<String>::decode_cache_value(reader)?,
                arguments: Vec::<ControlFlowValueId>::decode_cache_value(reader)?,
                argument_names: Vec::<Option<String>>::decode_cache_value(reader)?,
            }),
            30 => Some(Self::NamedQualifiedCall {
                namespace: String::decode_cache_value(reader)?,
                name: String::decode_cache_value(reader)?,
                arguments: Vec::<ControlFlowValueId>::decode_cache_value(reader)?,
                argument_names: Vec::<Option<String>>::decode_cache_value(reader)?,
            }),
            31 => Some(Self::AnonymousFunction {
                body: ControlFlowValueId::decode_cache_value(reader)?,
                params: Vec::<(String, Type)>::decode_cache_value(reader)?,
                return_type: Option::<Type>::decode_cache_value(reader)?,
            }),
            _ => None,
        }
    }
}

impl PersistedIrCodec for ControlFlowValueOwnership {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::Copy => 0u8.encode_cache_value(bytes),
            Self::ImmutableBorrow => 1u8.encode_cache_value(bytes),
            Self::EffectOnly => 2u8.encode_cache_value(bytes),
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(Self::Copy),
            1 => Some(Self::ImmutableBorrow),
            2 => Some(Self::EffectOnly),
            _ => None,
        }
    }
}

impl PersistedIrCodec for ControlFlowValue {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.id.encode_cache_value(bytes);
        self.producer.encode_cache_value(bytes);
        self.result_index.encode_cache_value(bytes);
        self.ty.encode_cache_value(bytes);
        self.ownership.encode_cache_value(bytes);
        self.span.encode_cache_value(bytes);
        self.kind.encode_cache_value(bytes);
        self.source_constant.encode_cache_value(bytes);
        self.constant.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            id: ControlFlowValueId::decode_cache_value(reader)?,
            producer: ControlFlowNodeId::decode_cache_value(reader)?,
            result_index: Option::<usize>::decode_cache_value(reader)?,
            ty: Type::decode_cache_value(reader)?,
            ownership: ControlFlowValueOwnership::decode_cache_value(reader)?,
            span: SourceSpan::decode_cache_value(reader)?,
            kind: ControlFlowValueKind::decode_cache_value(reader)?,
            source_constant: Option::<ConstantValue>::decode_cache_value(reader)?,
            constant: Option::<ConstantValue>::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowValueUseKind {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::Eager => 0u8.encode_cache_value(bytes),
            Self::ShortCircuitRight => 1u8.encode_cache_value(bytes),
            Self::OptionalPresent => 2u8.encode_cache_value(bytes),
            Self::BranchCondition => 3u8.encode_cache_value(bytes),
            Self::BranchThen => 4u8.encode_cache_value(bytes),
            Self::BranchElse => 5u8.encode_cache_value(bytes),
            Self::MatchValue => 6u8.encode_cache_value(bytes),
            Self::MatchGuard(arm) => {
                7u8.encode_cache_value(bytes);
                arm.encode_cache_value(bytes);
            }
            Self::MatchArm(arm) => {
                8u8.encode_cache_value(bytes);
                arm.encode_cache_value(bytes);
            }
            Self::LoopIterable => 9u8.encode_cache_value(bytes),
            Self::LoopCondition => 10u8.encode_cache_value(bytes),
            Self::LoopBody => 11u8.encode_cache_value(bytes),
            Self::DeferredBody => 12u8.encode_cache_value(bytes),
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(Self::Eager),
            1 => Some(Self::ShortCircuitRight),
            2 => Some(Self::OptionalPresent),
            3 => Some(Self::BranchCondition),
            4 => Some(Self::BranchThen),
            5 => Some(Self::BranchElse),
            6 => Some(Self::MatchValue),
            7 => Some(Self::MatchGuard(usize::decode_cache_value(reader)?)),
            8 => Some(Self::MatchArm(usize::decode_cache_value(reader)?)),
            9 => Some(Self::LoopIterable),
            10 => Some(Self::LoopCondition),
            11 => Some(Self::LoopBody),
            12 => Some(Self::DeferredBody),
            _ => None,
        }
    }
}

impl PersistedIrCodec for ControlFlowValueUse {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.user.encode_cache_value(bytes);
        self.value.encode_cache_value(bytes);
        self.kind.encode_cache_value(bytes);
        self.region.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            user: ControlFlowValueId::decode_cache_value(reader)?,
            value: ControlFlowValueId::decode_cache_value(reader)?,
            kind: ControlFlowValueUseKind::decode_cache_value(reader)?,
            region: Option::<ControlFlowValueRegionId>::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowValueRegionKind {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::ShortCircuitRight {
                condition,
                execute_when,
            } => {
                0u8.encode_cache_value(bytes);
                condition.encode_cache_value(bytes);
                execute_when.encode_cache_value(bytes);
            }
            Self::OptionalFallback { optional } => {
                1u8.encode_cache_value(bytes);
                optional.encode_cache_value(bytes);
            }
            Self::OptionalPresent { optional } => {
                2u8.encode_cache_value(bytes);
                optional.encode_cache_value(bytes);
            }
            Self::Branch {
                condition,
                selected_when,
            } => {
                3u8.encode_cache_value(bytes);
                condition.encode_cache_value(bytes);
                selected_when.encode_cache_value(bytes);
            }
            Self::MatchGuard { matched, arm } => {
                4u8.encode_cache_value(bytes);
                matched.encode_cache_value(bytes);
                arm.encode_cache_value(bytes);
            }
            Self::MatchArm {
                matched,
                guard,
                arm,
            } => {
                5u8.encode_cache_value(bytes);
                matched.encode_cache_value(bytes);
                guard.encode_cache_value(bytes);
                arm.encode_cache_value(bytes);
            }
            Self::LoopCondition { iterable } => {
                6u8.encode_cache_value(bytes);
                iterable.encode_cache_value(bytes);
            }
            Self::LoopBody {
                iterable,
                condition,
            } => {
                7u8.encode_cache_value(bytes);
                iterable.encode_cache_value(bytes);
                condition.encode_cache_value(bytes);
            }
            Self::DeferredBody => 8u8.encode_cache_value(bytes),
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(Self::ShortCircuitRight {
                condition: ControlFlowValueId::decode_cache_value(reader)?,
                execute_when: bool::decode_cache_value(reader)?,
            }),
            1 => Some(Self::OptionalFallback {
                optional: ControlFlowValueId::decode_cache_value(reader)?,
            }),
            2 => Some(Self::OptionalPresent {
                optional: ControlFlowValueId::decode_cache_value(reader)?,
            }),
            3 => Some(Self::Branch {
                condition: ControlFlowValueId::decode_cache_value(reader)?,
                selected_when: bool::decode_cache_value(reader)?,
            }),
            4 => Some(Self::MatchGuard {
                matched: ControlFlowValueId::decode_cache_value(reader)?,
                arm: usize::decode_cache_value(reader)?,
            }),
            5 => Some(Self::MatchArm {
                matched: ControlFlowValueId::decode_cache_value(reader)?,
                guard: Option::<ControlFlowValueId>::decode_cache_value(reader)?,
                arm: usize::decode_cache_value(reader)?,
            }),
            6 => Some(Self::LoopCondition {
                iterable: ControlFlowValueId::decode_cache_value(reader)?,
            }),
            7 => Some(Self::LoopBody {
                iterable: ControlFlowValueId::decode_cache_value(reader)?,
                condition: Option::<ControlFlowValueId>::decode_cache_value(reader)?,
            }),
            8 => Some(Self::DeferredBody),
            _ => None,
        }
    }
}

impl PersistedIrCodec for ControlFlowValueRegion {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.id.encode_cache_value(bytes);
        self.owner.encode_cache_value(bytes);
        self.root.encode_cache_value(bytes);
        self.span.encode_cache_value(bytes);
        self.kind.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            id: ControlFlowValueRegionId::decode_cache_value(reader)?,
            owner: ControlFlowValueId::decode_cache_value(reader)?,
            root: ControlFlowValueId::decode_cache_value(reader)?,
            span: SourceSpan::decode_cache_value(reader)?,
            kind: ControlFlowValueRegionKind::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowParameter {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.name.encode_cache_value(bytes);
        self.ty.encode_cache_value(bytes);
        self.span.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            name: String::decode_cache_value(reader)?,
            ty: Type::decode_cache_value(reader)?,
            span: SourceSpan::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowDefinition {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.name.encode_cache_value(bytes);
        self.ty.encode_cache_value(bytes);
        self.span.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            name: String::decode_cache_value(reader)?,
            ty: Type::decode_cache_value(reader)?,
            span: SourceSpan::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowEvaluationKind {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::BindingInitializer => 0u8.encode_cache_value(bytes),
            Self::AssignmentValue => 1u8.encode_cache_value(bytes),
            Self::DestructureValue => 2u8.encode_cache_value(bytes),
            Self::ReturnValue(index) => {
                3u8.encode_cache_value(bytes);
                index.encode_cache_value(bytes);
            }
            Self::ExpressionStatement => 4u8.encode_cache_value(bytes),
            Self::ShellValue => 5u8.encode_cache_value(bytes),
            Self::RedirectPath => 6u8.encode_cache_value(bytes),
            Self::Condition => 7u8.encode_cache_value(bytes),
            Self::RangeStart => 8u8.encode_cache_value(bytes),
            Self::RangeEnd => 9u8.encode_cache_value(bytes),
            Self::Iterable => 10u8.encode_cache_value(bytes),
            Self::MatchValue => 11u8.encode_cache_value(bytes),
            Self::MatchGuard(index) => {
                12u8.encode_cache_value(bytes);
                index.encode_cache_value(bytes);
            }
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(Self::BindingInitializer),
            1 => Some(Self::AssignmentValue),
            2 => Some(Self::DestructureValue),
            3 => Some(Self::ReturnValue(usize::decode_cache_value(reader)?)),
            4 => Some(Self::ExpressionStatement),
            5 => Some(Self::ShellValue),
            6 => Some(Self::RedirectPath),
            7 => Some(Self::Condition),
            8 => Some(Self::RangeStart),
            9 => Some(Self::RangeEnd),
            10 => Some(Self::Iterable),
            11 => Some(Self::MatchValue),
            12 => Some(Self::MatchGuard(usize::decode_cache_value(reader)?)),
            _ => None,
        }
    }
}

impl PersistedIrCodec for ControlFlowNodeKind {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::Entry => 0u8.encode_cache_value(bytes),
            Self::Exit => 1u8.encode_cache_value(bytes),
            Self::Evaluation(kind) => {
                2u8.encode_cache_value(bytes);
                kind.encode_cache_value(bytes);
            }
            Self::Binding { name, ty, mutable } => {
                3u8.encode_cache_value(bytes);
                name.encode_cache_value(bytes);
                ty.encode_cache_value(bytes);
                mutable.encode_cache_value(bytes);
            }
            Self::Assignment { name } => {
                4u8.encode_cache_value(bytes);
                name.encode_cache_value(bytes);
            }
            Self::Destructure { propagates_error } => {
                5u8.encode_cache_value(bytes);
                propagates_error.encode_cache_value(bytes);
            }
            Self::PatternBindings => 6u8.encode_cache_value(bytes),
            Self::Statement => 7u8.encode_cache_value(bytes),
            Self::Conditional => 8u8.encode_cache_value(bytes),
            Self::Loop => 9u8.encode_cache_value(bytes),
            Self::Match => 10u8.encode_cache_value(bytes),
            Self::Return => 11u8.encode_cache_value(bytes),
            Self::Break => 12u8.encode_cache_value(bytes),
            Self::Continue => 13u8.encode_cache_value(bytes),
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(Self::Entry),
            1 => Some(Self::Exit),
            2 => Some(Self::Evaluation(
                ControlFlowEvaluationKind::decode_cache_value(reader)?,
            )),
            3 => Some(Self::Binding {
                name: String::decode_cache_value(reader)?,
                ty: Type::decode_cache_value(reader)?,
                mutable: bool::decode_cache_value(reader)?,
            }),
            4 => Some(Self::Assignment {
                name: String::decode_cache_value(reader)?,
            }),
            5 => Some(Self::Destructure {
                propagates_error: bool::decode_cache_value(reader)?,
            }),
            6 => Some(Self::PatternBindings),
            7 => Some(Self::Statement),
            8 => Some(Self::Conditional),
            9 => Some(Self::Loop),
            10 => Some(Self::Match),
            11 => Some(Self::Return),
            12 => Some(Self::Break),
            13 => Some(Self::Continue),
            _ => None,
        }
    }
}

impl PersistedIrCodec for OwnershipMove {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.source.encode_cache_value(bytes);
        self.destination.encode_cache_value(bytes);
        self.projection.encode_cache_value(bytes);
        self.value.encode_cache_value(bytes);
        self.source_definitions.encode_cache_value(bytes);
        self.span.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            source: String::decode_cache_value(reader)?,
            destination: String::decode_cache_value(reader)?,
            projection: Vec::<String>::decode_cache_value(reader)?,
            value: Option::<ControlFlowValueId>::decode_cache_value(reader)?,
            source_definitions: Vec::<ControlFlowDefinitionId>::decode_cache_value(reader)?,
            span: SourceSpan::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for OwnershipBorrowKind {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        0u8.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        (u8::decode_cache_value(reader)? == 0).then_some(Self::Immutable)
    }
}

impl PersistedIrCodec for OwnershipBorrow {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.source.encode_cache_value(bytes);
        self.kind.encode_cache_value(bytes);
        self.value.encode_cache_value(bytes);
        self.source_definitions.encode_cache_value(bytes);
        self.span.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            source: String::decode_cache_value(reader)?,
            kind: OwnershipBorrowKind::decode_cache_value(reader)?,
            value: Option::<ControlFlowValueId>::decode_cache_value(reader)?,
            source_definitions: Vec::<ControlFlowDefinitionId>::decode_cache_value(reader)?,
            span: SourceSpan::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for OwnershipCallArgumentKind {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        let tag: u8 = match self {
            Self::Copy => 0,
            Self::ImmutableBorrow => 1,
            Self::Consuming => 2,
        };
        tag.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(Self::Copy),
            1 => Some(Self::ImmutableBorrow),
            2 => Some(Self::Consuming),
            _ => None,
        }
    }
}

impl PersistedIrCodec for OwnershipCall {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.callee.encode_cache_value(bytes);
        self.arguments.encode_cache_value(bytes);
        self.argument_projections.encode_cache_value(bytes);
        self.argument_kinds.encode_cache_value(bytes);
        self.argument_definitions.encode_cache_value(bytes);
        self.borrowed_argument_definitions.encode_cache_value(bytes);
        self.span.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            callee: String::decode_cache_value(reader)?,
            arguments: Vec::<ControlFlowValueId>::decode_cache_value(reader)?,
            argument_projections: Vec::<Vec<String>>::decode_cache_value(reader)?,
            argument_kinds: Vec::<OwnershipCallArgumentKind>::decode_cache_value(reader)?,
            argument_definitions: Vec::<Vec<ControlFlowDefinitionId>>::decode_cache_value(reader)?,
            borrowed_argument_definitions: Vec::<Vec<ControlFlowDefinitionId>>::decode_cache_value(
                reader,
            )?,
            span: SourceSpan::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for OwnershipReturn {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.value.encode_cache_value(bytes);
        self.kind.encode_cache_value(bytes);
        self.definitions.encode_cache_value(bytes);
        self.borrowed_definitions.encode_cache_value(bytes);
        self.span.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            value: ControlFlowValueId::decode_cache_value(reader)?,
            kind: OwnershipCallArgumentKind::decode_cache_value(reader)?,
            definitions: Vec::<ControlFlowDefinitionId>::decode_cache_value(reader)?,
            borrowed_definitions: Vec::<ControlFlowDefinitionId>::decode_cache_value(reader)?,
            span: SourceSpan::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for OwnershipDrop {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.definition.encode_cache_value(bytes);
        self.value.encode_cache_value(bytes);
        self.name.encode_cache_value(bytes);
        self.span.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            definition: ControlFlowDefinitionId::decode_cache_value(reader)?,
            value: Option::<ControlFlowValueId>::decode_cache_value(reader)?,
            name: String::decode_cache_value(reader)?,
            span: SourceSpan::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowOwnership {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.reads.encode_cache_value(bytes);
        self.borrows.encode_cache_value(bytes);
        self.moves.encode_cache_value(bytes);
        self.calls.encode_cache_value(bytes);
        self.returns.encode_cache_value(bytes);
        self.drops.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            reads: Vec::<String>::decode_cache_value(reader)?,
            borrows: Vec::<OwnershipBorrow>::decode_cache_value(reader)?,
            moves: Vec::<OwnershipMove>::decode_cache_value(reader)?,
            calls: Vec::<OwnershipCall>::decode_cache_value(reader)?,
            returns: Vec::<OwnershipReturn>::decode_cache_value(reader)?,
            drops: Vec::<OwnershipDrop>::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for OwnershipMovedBinding {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.definition.encode_cache_value(bytes);
        self.name.encode_cache_value(bytes);
        self.projection.encode_cache_value(bytes);
        self.origin.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            definition: ControlFlowDefinitionId::decode_cache_value(reader)?,
            name: String::decode_cache_value(reader)?,
            projection: Vec::<String>::decode_cache_value(reader)?,
            origin: SourceSpan::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowMoveState {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.reachable.encode_cache_value(bytes);
        self.moved.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            reachable: bool::decode_cache_value(reader)?,
            moved: Vec::<OwnershipMovedBinding>::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowLiveState {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.live.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            live: Vec::<String>::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowDefinitionLiveState {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.live.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            live: Vec::<ControlFlowDefinitionId>::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for OwnershipBorrowedBinding {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.definition.encode_cache_value(bytes);
        self.borrower.encode_cache_value(bytes);
        self.source_definition.encode_cache_value(bytes);
        self.source.encode_cache_value(bytes);
        self.origin.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            definition: ControlFlowDefinitionId::decode_cache_value(reader)?,
            borrower: String::decode_cache_value(reader)?,
            source_definition: ControlFlowDefinitionId::decode_cache_value(reader)?,
            source: String::decode_cache_value(reader)?,
            origin: SourceSpan::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowBorrowState {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.borrows.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            borrows: Vec::<OwnershipBorrowedBinding>::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for OwnershipBorrowStart {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.from.encode_cache_value(bytes);
        self.to.encode_cache_value(bytes);
        self.definition.encode_cache_value(bytes);
        self.borrower.encode_cache_value(bytes);
        self.source_definition.encode_cache_value(bytes);
        self.source.encode_cache_value(bytes);
        self.origin.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            from: ControlFlowNodeId::decode_cache_value(reader)?,
            to: ControlFlowNodeId::decode_cache_value(reader)?,
            definition: ControlFlowDefinitionId::decode_cache_value(reader)?,
            borrower: String::decode_cache_value(reader)?,
            source_definition: ControlFlowDefinitionId::decode_cache_value(reader)?,
            source: String::decode_cache_value(reader)?,
            origin: SourceSpan::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for OwnershipBorrowEnd {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.from.encode_cache_value(bytes);
        self.to.encode_cache_value(bytes);
        self.definition.encode_cache_value(bytes);
        self.borrower.encode_cache_value(bytes);
        self.source_definition.encode_cache_value(bytes);
        self.source.encode_cache_value(bytes);
        self.origin.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            from: ControlFlowNodeId::decode_cache_value(reader)?,
            to: ControlFlowNodeId::decode_cache_value(reader)?,
            definition: ControlFlowDefinitionId::decode_cache_value(reader)?,
            borrower: String::decode_cache_value(reader)?,
            source_definition: ControlFlowDefinitionId::decode_cache_value(reader)?,
            source: String::decode_cache_value(reader)?,
            origin: SourceSpan::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for OwnershipBorrowLifetime {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.definition.encode_cache_value(bytes);
        self.borrower.encode_cache_value(bytes);
        self.source_definition.encode_cache_value(bytes);
        self.source.encode_cache_value(bytes);
        self.origin.encode_cache_value(bytes);
        self.active_before.encode_cache_value(bytes);
        self.starts.encode_cache_value(bytes);
        self.ends.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            definition: ControlFlowDefinitionId::decode_cache_value(reader)?,
            borrower: String::decode_cache_value(reader)?,
            source_definition: ControlFlowDefinitionId::decode_cache_value(reader)?,
            source: String::decode_cache_value(reader)?,
            origin: SourceSpan::decode_cache_value(reader)?,
            active_before: Vec::<ControlFlowNodeId>::decode_cache_value(reader)?,
            starts: Vec::<OwnershipBorrowStart>::decode_cache_value(reader)?,
            ends: Vec::<OwnershipBorrowEnd>::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowNode {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.id.encode_cache_value(bytes);
        self.kind.encode_cache_value(bytes);
        self.span.encode_cache_value(bytes);
        self.value_types.encode_cache_value(bytes);
        self.values.encode_cache_value(bytes);
        self.definitions.encode_cache_value(bytes);
        self.ownership.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            id: ControlFlowNodeId::decode_cache_value(reader)?,
            kind: ControlFlowNodeKind::decode_cache_value(reader)?,
            span: SourceSpan::decode_cache_value(reader)?,
            value_types: Vec::<Type>::decode_cache_value(reader)?,
            values: Vec::<ControlFlowValueId>::decode_cache_value(reader)?,
            definitions: Vec::<ControlFlowDefinition>::decode_cache_value(reader)?,
            ownership: ControlFlowOwnership::decode_cache_value(reader)?,
        })
    }
}

impl PersistedIrCodec for ControlFlowEdgeKind {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::Next => 0u8.encode_cache_value(bytes),
            Self::True => 1u8.encode_cache_value(bytes),
            Self::False => 2u8.encode_cache_value(bytes),
            Self::GuardTrue => 3u8.encode_cache_value(bytes),
            Self::GuardFalse => 4u8.encode_cache_value(bytes),
            Self::MatchArm(index) => {
                5u8.encode_cache_value(bytes);
                index.encode_cache_value(bytes);
            }
            Self::Success => 6u8.encode_cache_value(bytes),
            Self::Error => 7u8.encode_cache_value(bytes),
            Self::Return => 8u8.encode_cache_value(bytes),
            Self::Break => 9u8.encode_cache_value(bytes),
            Self::Continue => 10u8.encode_cache_value(bytes),
        }
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        match u8::decode_cache_value(reader)? {
            0 => Some(Self::Next),
            1 => Some(Self::True),
            2 => Some(Self::False),
            3 => Some(Self::GuardTrue),
            4 => Some(Self::GuardFalse),
            5 => Some(Self::MatchArm(usize::decode_cache_value(reader)?)),
            6 => Some(Self::Success),
            7 => Some(Self::Error),
            8 => Some(Self::Return),
            9 => Some(Self::Break),
            10 => Some(Self::Continue),
            _ => None,
        }
    }
}

impl PersistedIrCodec for ControlFlowEdge {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.from.encode_cache_value(bytes);
        self.to.encode_cache_value(bytes);
        self.kind.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        Some(Self {
            from: ControlFlowNodeId::decode_cache_value(reader)?,
            to: ControlFlowNodeId::decode_cache_value(reader)?,
            kind: ControlFlowEdgeKind::decode_cache_value(reader)?,
        })
    }
}

fn persisted_value_kind_is_valid(kind: &ControlFlowValueKind, value_count: usize) -> bool {
    let valid = |value: ControlFlowValueId| value.0 < value_count;
    let valid_option = |value: &Option<ControlFlowValueId>| value.is_none_or(|value| valid(value));
    let valid_values = |values: &[ControlFlowValueId]| values.iter().copied().all(valid);
    match kind {
        ControlFlowValueKind::Literal | ControlFlowValueKind::Opaque => true,
        ControlFlowValueKind::NameRead { .. } => true,
        ControlFlowValueKind::AnonymousFunction { body, .. }
        | ControlFlowValueKind::Await { value: body }
        | ControlFlowValueKind::ListSpread { value: body, .. }
        | ControlFlowValueKind::ListOptional { value: body } => valid(*body),
        ControlFlowValueKind::Call { arguments, .. }
        | ControlFlowValueKind::QualifiedCall { arguments, .. } => valid_values(arguments),
        ControlFlowValueKind::NamedCall {
            arguments,
            argument_names,
            ..
        }
        | ControlFlowValueKind::NamedQualifiedCall {
            arguments,
            argument_names,
            ..
        } => arguments.len() == argument_names.len() && valid_values(arguments),
        ControlFlowValueKind::OptionalCascadeCall {
            optional,
            arguments,
            ..
        } => valid(*optional) && valid_values(arguments),
        ControlFlowValueKind::InterfacePack { value, .. } => valid(*value),
        ControlFlowValueKind::List { items } | ControlFlowValueKind::Set { items } => {
            valid_values(items)
        }
        ControlFlowValueKind::Map { entries } => entries
            .iter()
            .all(|(key, value)| valid(*key) && valid(*value)),
        ControlFlowValueKind::ListIf {
            condition,
            value,
            else_value,
            ..
        } => valid(*condition) && valid(*value) && valid_option(else_value),
        ControlFlowValueKind::Index { base, index, .. } => valid(*base) && valid(*index),
        ControlFlowValueKind::Slice {
            base,
            start,
            end,
            step,
        } => valid(*base) && valid_option(start) && valid_option(end) && valid_option(step),
        ControlFlowValueKind::ListComprehension {
            iterable,
            value,
            condition,
            ..
        } => valid(*iterable) && valid(*value) && valid_option(condition),
        ControlFlowValueKind::StructLiteral { base, fields, .. } => {
            valid_option(base) && fields.iter().all(|(_, value)| valid(*value))
        }
        ControlFlowValueKind::RecordLiteral { fields } => {
            fields.iter().all(|(_, value)| valid(*value))
        }
        ControlFlowValueKind::InterfaceDispatch { arguments, .. } => valid_values(arguments),
        ControlFlowValueKind::NamedInterfaceDispatch {
            arguments,
            argument_names,
            ..
        } => arguments.len() == argument_names.len() && valid_values(arguments),
        ControlFlowValueKind::Field { base, .. } => valid(*base),
        ControlFlowValueKind::Match {
            value,
            guards,
            arms,
            ..
        }
        | ControlFlowValueKind::ListMatch {
            value,
            guards,
            arms,
            ..
        } => valid(*value) && guards.iter().all(valid_option) && valid_values(arms),
        ControlFlowValueKind::Conditional {
            condition,
            then_value,
            else_value,
        } => valid(*condition) && valid(*then_value) && valid(*else_value),
        ControlFlowValueKind::Unary { operand, .. } => valid(*operand),
        ControlFlowValueKind::Binary { left, right, .. } => valid(*left) && valid(*right),
    }
}

fn persisted_value_region_kind_is_valid(
    kind: &ControlFlowValueRegionKind,
    value_count: usize,
) -> bool {
    let valid = |value: ControlFlowValueId| value.0 < value_count;
    match kind {
        ControlFlowValueRegionKind::ShortCircuitRight { condition, .. }
        | ControlFlowValueRegionKind::Branch { condition, .. } => valid(*condition),
        ControlFlowValueRegionKind::OptionalFallback { optional }
        | ControlFlowValueRegionKind::OptionalPresent { optional } => valid(*optional),
        ControlFlowValueRegionKind::MatchGuard { matched, .. } => valid(*matched),
        ControlFlowValueRegionKind::MatchArm { matched, guard, .. } => {
            valid(*matched) && guard.is_none_or(|guard| valid(guard))
        }
        ControlFlowValueRegionKind::LoopCondition { iterable } => valid(*iterable),
        ControlFlowValueRegionKind::LoopBody {
            iterable,
            condition,
        } => valid(*iterable) && condition.is_none_or(|condition| valid(condition)),
        ControlFlowValueRegionKind::DeferredBody => true,
    }
}

impl PersistedIrCodec for ControlFlowGraph {
    fn encode_cache_value(&self, bytes: &mut Vec<u8>) {
        self.function.encode_cache_value(bytes);
        self.parameters.encode_cache_value(bytes);
        self.returns.encode_cache_value(bytes);
        self.entry.encode_cache_value(bytes);
        self.exit.encode_cache_value(bytes);
        self.nodes.encode_cache_value(bytes);
        self.edges.encode_cache_value(bytes);
        self.values.encode_cache_value(bytes);
        self.value_uses.encode_cache_value(bytes);
        self.value_regions.encode_cache_value(bytes);
        self.reachable_values.encode_cache_value(bytes);
        self.escaping_values.encode_cache_value(bytes);
        self.scoped_definitions.encode_cache_value(bytes);
        self.definition_values.encode_cache_value(bytes);
        self.scoped_borrow_sources.encode_cache_value(bytes);
        self.reaching_definitions_before.encode_cache_value(bytes);
        self.move_states_before.encode_cache_value(bytes);
        self.live_before.encode_cache_value(bytes);
        self.live_after.encode_cache_value(bytes);
        self.definition_live_before.encode_cache_value(bytes);
        self.definition_live_after.encode_cache_value(bytes);
        self.borrow_states_before.encode_cache_value(bytes);
        self.borrow_starts.encode_cache_value(bytes);
        self.borrow_ends.encode_cache_value(bytes);
        self.borrow_lifetimes.encode_cache_value(bytes);
        self.drops.encode_cache_value(bytes);
    }

    fn decode_cache_value(reader: &mut PersistedIrReader<'_>) -> Option<Self> {
        let graph = Self {
            function: String::decode_cache_value(reader)?,
            parameters: Vec::<ControlFlowParameter>::decode_cache_value(reader)?,
            returns: Vec::<Type>::decode_cache_value(reader)?,
            entry: ControlFlowNodeId::decode_cache_value(reader)?,
            exit: ControlFlowNodeId::decode_cache_value(reader)?,
            nodes: Vec::<ControlFlowNode>::decode_cache_value(reader)?,
            edges: Vec::<ControlFlowEdge>::decode_cache_value(reader)?,
            values: Vec::<ControlFlowValue>::decode_cache_value(reader)?,
            value_uses: Vec::<ControlFlowValueUse>::decode_cache_value(reader)?,
            value_regions: Vec::<ControlFlowValueRegion>::decode_cache_value(reader)?,
            reachable_values: BTreeSet::<ControlFlowValueId>::decode_cache_value(reader)?,
            escaping_values: BTreeSet::<ControlFlowValueId>::decode_cache_value(reader)?,
            scoped_definitions: Vec::<Vec<ControlFlowDefinition>>::decode_cache_value(reader)?,
            definition_values:
                BTreeMap::<ControlFlowDefinitionId, ControlFlowValueId>::decode_cache_value(reader)?,
            scoped_borrow_sources:
                BTreeMap::<ControlFlowDefinitionId, ControlFlowValueId>::decode_cache_value(reader)?,
            reaching_definitions_before: Vec::<Option<ReachingDefinitionMap>>::decode_cache_value(
                reader,
            )?,
            move_states_before: Vec::<ControlFlowMoveState>::decode_cache_value(reader)?,
            live_before: Vec::<ControlFlowLiveState>::decode_cache_value(reader)?,
            live_after: Vec::<ControlFlowLiveState>::decode_cache_value(reader)?,
            definition_live_before: Vec::<ControlFlowDefinitionLiveState>::decode_cache_value(
                reader,
            )?,
            definition_live_after: Vec::<ControlFlowDefinitionLiveState>::decode_cache_value(
                reader,
            )?,
            borrow_states_before: Vec::<ControlFlowBorrowState>::decode_cache_value(reader)?,
            borrow_starts: Vec::<OwnershipBorrowStart>::decode_cache_value(reader)?,
            borrow_ends: Vec::<OwnershipBorrowEnd>::decode_cache_value(reader)?,
            borrow_lifetimes: Vec::<OwnershipBorrowLifetime>::decode_cache_value(reader)?,
            drops: Vec::<(ControlFlowNodeId, OwnershipDrop)>::decode_cache_value(reader)?,
        };
        graph.persisted_cache_is_valid().then_some(graph)
    }
}

impl ControlFlowGraph {
    pub(crate) fn encode_persisted(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(PERSISTED_IR_MAGIC);
        self.encode_cache_value(&mut bytes);
        bytes
    }

    pub(crate) fn decode_persisted(bytes: &[u8]) -> Option<Self> {
        if !bytes.starts_with(PERSISTED_IR_MAGIC) {
            return None;
        }
        let mut reader = PersistedIrReader::new(&bytes[PERSISTED_IR_MAGIC.len()..]);
        let graph = Self::decode_cache_value(&mut reader)?;
        reader.is_finished().then_some(graph)
    }

    fn persisted_cache_is_valid(&self) -> bool {
        let node_count = self.nodes.len();
        let value_count = self.values.len();
        let region_count = self.value_regions.len();
        if self.entry.0 >= node_count || self.exit.0 >= node_count {
            return false;
        }
        if self.nodes.iter().enumerate().any(|(index, node)| {
            node.id.0 != index || node.values.iter().any(|value| value.0 >= value_count)
        }) {
            return false;
        }
        if self
            .edges
            .iter()
            .any(|edge| edge.from.0 >= node_count || edge.to.0 >= node_count)
        {
            return false;
        }
        if self.values.iter().enumerate().any(|(index, value)| {
            value.id.0 != index
                || value.producer.0 >= node_count
                || !persisted_value_kind_is_valid(&value.kind, value_count)
        }) {
            return false;
        }
        if self.value_uses.iter().any(|value_use| {
            value_use.user.0 >= value_count
                || value_use.value.0 >= value_count
                || value_use
                    .region
                    .is_some_and(|region| region.0 >= region_count)
        }) {
            return false;
        }
        if self
            .value_regions
            .iter()
            .enumerate()
            .any(|(index, region)| {
                region.id.0 != index
                    || region.owner.0 >= value_count
                    || region.root.0 >= value_count
                    || !persisted_value_region_kind_is_valid(&region.kind, value_count)
            })
        {
            return false;
        }
        if self
            .reachable_values
            .iter()
            .chain(self.escaping_values.iter())
            .any(|value| value.0 >= value_count)
        {
            return false;
        }
        if self.scoped_definitions.len() > node_count
            || self.reaching_definitions_before.len() != node_count
            || self.move_states_before.len() != node_count
            || self.live_before.len() != node_count
            || self.live_after.len() != node_count
            || self.definition_live_before.len() != node_count
            || self.definition_live_after.len() != node_count
            || self.borrow_states_before.len() != node_count
        {
            return false;
        }
        let valid_definition = |definition: ControlFlowDefinitionId| match definition {
            ControlFlowDefinitionId::Parameter(index) => index < self.parameters.len(),
            ControlFlowDefinitionId::Node { node, index } => self
                .nodes
                .get(node.0)
                .is_some_and(|node| index < node.definitions.len()),
            ControlFlowDefinitionId::Scoped { node, index } => self
                .scoped_definitions
                .get(node.0)
                .is_some_and(|definitions| index < definitions.len()),
        };
        if self
            .definition_values
            .iter()
            .any(|(definition, value)| !valid_definition(*definition) || value.0 >= value_count)
            || self
                .scoped_borrow_sources
                .iter()
                .any(|(definition, value)| !valid_definition(*definition) || value.0 >= value_count)
        {
            return false;
        }
        if self
            .reaching_definitions_before
            .iter()
            .flatten()
            .flat_map(BTreeMap::values)
            .flatten()
            .any(|definition| !valid_definition(*definition))
        {
            return false;
        }
        if self
            .nodes
            .iter()
            .flat_map(|node| {
                node.ownership
                    .borrows
                    .iter()
                    .flat_map(|borrow| borrow.source_definitions.iter())
                    .chain(
                        node.ownership
                            .moves
                            .iter()
                            .flat_map(|moved| moved.source_definitions.iter()),
                    )
                    .chain(
                        node.ownership
                            .calls
                            .iter()
                            .flat_map(|call| call.argument_definitions.iter().flatten()),
                    )
                    .chain(
                        node.ownership
                            .calls
                            .iter()
                            .flat_map(|call| call.borrowed_argument_definitions.iter().flatten()),
                    )
                    .chain(
                        node.ownership
                            .returns
                            .iter()
                            .flat_map(|returned| returned.definitions.iter()),
                    )
                    .chain(
                        node.ownership
                            .returns
                            .iter()
                            .flat_map(|returned| returned.borrowed_definitions.iter()),
                    )
                    .chain(node.ownership.drops.iter().map(|drop| &drop.definition))
            })
            .any(|definition| !valid_definition(*definition))
        {
            return false;
        }
        true
    }

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
            // A value behind a pruned short-circuit/conditional region is not
            // evaluated by the source program.  Its producer may therefore be
            // effectful without making the reachable value impure.  Checking
            // reachability here keeps the backend's IR proof aligned with the
            // CFG's control-dependent evaluation facts instead of conservatively
            // rebuilding an AST-shaped dependency walk.
            if !graph.is_value_reachable(id) {
                return true;
            }
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
                        && definitions.iter().all(|definition| match definition {
                            ControlFlowDefinitionId::Parameter(_) => true,
                            _ => graph
                                .definition_value(*definition)
                                .is_some_and(|value| visit(graph, value, visiting)),
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
                | ControlFlowValueKind::NamedCall { .. }
                | ControlFlowValueKind::OptionalCascadeCall { .. }
                | ControlFlowValueKind::QualifiedCall { .. }
                | ControlFlowValueKind::NamedQualifiedCall { .. }
                | ControlFlowValueKind::InterfaceDispatch { .. }
                | ControlFlowValueKind::NamedInterfaceDispatch { .. }
                | ControlFlowValueKind::Opaque => ControlFlowValueEffect::MayEffect,
                ControlFlowValueKind::InterfacePack { value, .. }
                | ControlFlowValueKind::ListSpread { value, .. }
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
                    ..
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
                    ..
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
                    ..
                }
                | ControlFlowValueKind::ListMatch {
                    value,
                    guards,
                    arms,
                    ..
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
        let ControlFlowValueKind::AnonymousFunction { body, .. } = self.value(id)?.kind else {
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
                | ControlFlowValueKind::NamedCall { callee, .. }
                | ControlFlowValueKind::OptionalCascadeCall { callee, .. } => {
                    std::iter::once(callee.clone()).collect::<BTreeSet<_>>()
                }
                ControlFlowValueKind::QualifiedCall {
                    namespace, name, ..
                }
                | ControlFlowValueKind::NamedQualifiedCall {
                    namespace, name, ..
                } => std::iter::once(format!("{namespace}.{name}")).collect(),
                ControlFlowValueKind::InterfaceDispatch {
                    interface,
                    capability,
                    ..
                }
                | ControlFlowValueKind::NamedInterfaceDispatch {
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
                        | ControlFlowValueKind::NamedInterfaceDispatch { .. }
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

    /// Return all normalized immutable-borrow boundaries in deterministic
    /// order. Starts precede ends, and each underlying collection retains the
    /// graph's stable `(from, to, definition)` ordering. The view is borrowed
    /// and allocation-free, like the node-local ownership event stream.
    pub fn borrow_boundaries(&self) -> impl Iterator<Item = ControlFlowBorrowBoundary<'_>> {
        self.borrow_starts
            .iter()
            .map(ControlFlowBorrowBoundary::Start)
            .chain(self.borrow_ends.iter().map(ControlFlowBorrowBoundary::End))
    }

    /// Return borrow boundaries whose CFG edge remains executable.
    ///
    /// A lifetime is represented by edge facts, so filtering only node-local
    /// ownership events is insufficient for consumers that reconstruct active
    /// regions.  Both endpoints must be reachable: this excludes boundaries
    /// in statically dead branches and preserves the allocation-free borrowed
    /// view of the normalized facts.
    pub fn reachable_borrow_boundaries(
        &self,
    ) -> impl Iterator<Item = ControlFlowBorrowBoundary<'_>> {
        self.borrow_boundaries().filter(move |boundary| {
            let (from, to) = match boundary {
                ControlFlowBorrowBoundary::Start(start) => (start.from, start.to),
                ControlFlowBorrowBoundary::End(end) => (end.from, end.to),
            };
            self.is_reachable(from) && self.is_reachable(to)
        })
    }

    /// Return normalized borrow boundaries for one CFG edge.
    pub fn borrow_boundaries_on_edge(
        &self,
        from: ControlFlowNodeId,
        to: ControlFlowNodeId,
    ) -> impl Iterator<Item = ControlFlowBorrowBoundary<'_>> {
        self.borrow_starts
            .iter()
            .filter(move |boundary| boundary.from == from && boundary.to == to)
            .map(ControlFlowBorrowBoundary::Start)
            .chain(
                self.borrow_ends
                    .iter()
                    .filter(move |boundary| boundary.from == from && boundary.to == to)
                    .map(ControlFlowBorrowBoundary::End),
            )
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

    /// Return ownership events whose evaluation node is reachable after the
    /// normalized CFG has removed statically unselected control-flow edges.
    ///
    /// The complete [`Self::ownership_events`] stream is useful for source
    /// mapping and diagnostics, but ownership consumers that validate live
    /// moves/borrows must not accidentally treat facts from `if false`, an
    /// unreachable match arm, or a statically empty loop as executable.  The
    /// iterator borrows the graph and allocates no filtered event buffer.
    pub fn reachable_ownership_events(
        &self,
    ) -> impl Iterator<Item = (ControlFlowNodeId, ControlFlowOwnershipEvent<'_>)> {
        self.nodes
            .iter()
            .filter(|node| self.is_reachable(node.id))
            .flat_map(|node| {
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
                    if definition.name != name || !is_non_copy_collection_type(&definition.ty) {
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
            ControlFlowValueKind::ListSpread { value, .. } => {
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
            | ControlFlowValueKind::NamedCall {
                callee, arguments, ..
            } if matches!(
                crate::builtin_names::global_impl(callee),
                "take" | "skip" | "chunked"
            ) =>
            {
                arguments.first().is_some_and(|argument| {
                    self.value_depends_on_borrow_source(*argument, source, visiting)
                })
            }
            ControlFlowValueKind::Call { callee, arguments }
            | ControlFlowValueKind::NamedCall {
                callee, arguments, ..
            } if matches!(&value.ty, Type::List(element) if matches!(element.as_ref(), Type::List(_)))
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
            ControlFlowValueKind::ListSpread { value, .. } => {
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
            | ControlFlowValueKind::NamedCall {
                callee, arguments, ..
            } if matches!(
                crate::builtin_names::global_impl(callee),
                "take" | "skip" | "chunked"
            ) =>
            {
                arguments.first().is_some_and(|argument| {
                    self.value_depends_on_borrow_source(*argument, source, visiting)
                })
            }
            ControlFlowValueKind::Call { callee, arguments }
            | ControlFlowValueKind::NamedCall {
                callee, arguments, ..
            } if matches!(&value.ty, Type::List(element) if matches!(element.as_ref(), Type::List(_)))
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
            ControlFlowValueKind::ListSpread { value, .. } => visit(*value),
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
            | ControlFlowValueKind::NamedCall {
                callee, arguments, ..
            } if matches!(
                crate::builtin_names::global_impl(callee),
                "take" | "skip" | "chunked"
            ) =>
            {
                if let Some(argument) = arguments.first() {
                    visit(*argument);
                }
            }
            ControlFlowValueKind::Call { callee, arguments }
            | ControlFlowValueKind::NamedCall {
                callee, arguments, ..
            } if matches!(&value.ty, Type::List(element) if matches!(element.as_ref(), Type::List(_)))
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
    if !is_non_copy_collection_type(&definition.ty) {
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

fn complete_value_list(values: Vec<Option<ControlFlowValueId>>) -> Option<Vec<ControlFlowValueId>> {
    values.into_iter().collect()
}

fn complete_map_entry_values(
    values: Vec<Option<ControlFlowValueId>>,
) -> Option<Vec<(ControlFlowValueId, ControlFlowValueId)>> {
    if !values.len().is_multiple_of(2) {
        return None;
    }
    values
        .chunks_exact(2)
        .map(|pair| Some((pair[0]?, pair[1]?)))
        .collect()
}

fn complete_optional_value<T, U>(
    source: Option<T>,
    lower: impl FnOnce(T) -> Option<U>,
) -> Option<Option<U>> {
    match source {
        Some(value) => lower(value).map(Some),
        None => Some(None),
    }
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
        reclassify_borrowed_collection_reborrows(
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
            | ExprKind::Nil
            | ExprKind::None => ControlFlowValueKind::Literal,
            // A single local string interpolation is the allocation-free
            // identity form accepted by the type checker. Preserve its
            // read dependency in typed IR instead of collapsing it into a
            // literal: reaching-definition, liveness, and future ownership
            // consumers must see the source binding it borrows.
            ExprKind::InterpolatedString(parts) => {
                if let [crate::ast::InterpolatedStringPart::Binding { name, .. }] = parts.as_slice()
                    && self
                        .scalar_expression_type(expr)
                        .is_some_and(|ty| self.signatures.canonical_type(&ty) == Type::Str)
                {
                    ControlFlowValueKind::NameRead {
                        name: name.clone(),
                        definitions: self.scoped_definition_for(name).into_iter().collect(),
                    }
                } else {
                    ControlFlowValueKind::Literal
                }
            }
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
                    } => match self.lower_call_arguments(producer, args, named_args) {
                        Some(arguments) if named_args.is_empty() => ControlFlowValueKind::Call {
                            callee: name.clone(),
                            arguments,
                        },
                        Some(arguments) => {
                            let mut argument_names = vec![None; args.len()];
                            argument_names.extend(
                                named_args
                                    .iter()
                                    .map(|argument| Some(argument.name.clone())),
                            );
                            ControlFlowValueKind::NamedCall {
                                callee: name.clone(),
                                arguments,
                                argument_names,
                            }
                        }
                        None => ControlFlowValueKind::Opaque,
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
            ExprKind::AnonymousFunction {
                params,
                return_type,
                body,
            } => {
                let parameter_types = params
                    .iter()
                    .map(|param| {
                        (
                            param.name.clone(),
                            self.signatures.canonical_type(&param.ty),
                        )
                    })
                    .collect::<Vec<_>>();
                let return_type = return_type
                    .as_ref()
                    .map(|ty| self.signatures.canonical_type(ty));
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
                    ControlFlowValueKind::AnonymousFunction {
                        body,
                        params: parameter_types,
                        return_type,
                    }
                })
            }
            ExprKind::Call {
                name,
                args,
                named_args,
            } => match self.lower_call_arguments(producer, args, named_args) {
                Some(arguments)
                    if self.signatures.interface(name).is_some()
                        && named_args.is_empty()
                        && arguments.len() == 1 =>
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
                }
                Some(arguments) if named_args.is_empty() => ControlFlowValueKind::Call {
                    callee: name.clone(),
                    arguments,
                },
                Some(arguments) => {
                    let mut argument_names = vec![None; args.len()];
                    argument_names.extend(
                        named_args
                            .iter()
                            .map(|argument| Some(argument.name.clone())),
                    );
                    ControlFlowValueKind::NamedCall {
                        callee: name.clone(),
                        arguments,
                        argument_names,
                    }
                }
                None => ControlFlowValueKind::Opaque,
            },
            ExprKind::ShellCall { name, args, .. } => self
                .lower_expr_arguments(producer, args)
                .map_or(ControlFlowValueKind::Opaque, |arguments| {
                    ControlFlowValueKind::Call {
                        callee: name.clone(),
                        arguments,
                    }
                }),
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
                    match (optional, arguments) {
                        (Some(optional), Some(arguments)) => {
                            ControlFlowValueKind::OptionalCascadeCall {
                                optional,
                                callee: name.clone(),
                                arguments,
                            }
                        }
                        _ => ControlFlowValueKind::Opaque,
                    }
                } else {
                    let mut input_values = self.lower_expr_values(producer, input, false);
                    match self.lower_expr_arguments(producer, args) {
                        Some(arguments) => {
                            input_values.extend(arguments);
                            ControlFlowValueKind::Call {
                                callee: name.clone(),
                                arguments: input_values,
                            }
                        }
                        None => ControlFlowValueKind::Opaque,
                    }
                }
            }
            ExprKind::List(items) => self
                .lower_expr_arguments(producer, items)
                .map_or(ControlFlowValueKind::Opaque, |items| {
                    ControlFlowValueKind::List { items }
                }),
            ExprKind::Set(items) => self
                .lower_expr_arguments(producer, items)
                .map_or(ControlFlowValueKind::Opaque, |items| {
                    ControlFlowValueKind::Set { items }
                }),
            ExprKind::Map(items) => {
                let values = items
                    .iter()
                    .map(|item| self.lower_scalar_expr(producer, item))
                    .collect();
                complete_map_entry_values(values).map_or(ControlFlowValueKind::Opaque, |entries| {
                    ControlFlowValueKind::Map { entries }
                })
            }
            ExprKind::ListSpread {
                value, optional, ..
            } => self.lower_scalar_expr(producer, value).map_or(
                ControlFlowValueKind::Opaque,
                |value| ControlFlowValueKind::ListSpread {
                    value,
                    optional: *optional,
                },
            ),
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
                let else_value = complete_optional_value(else_value.as_deref(), |value| {
                    self.lower_scalar_expr(producer, value)
                });
                match (condition_value, value, else_value) {
                    (Some(condition), Some(value), Some(else_value)) => {
                        ControlFlowValueKind::ListIf {
                            condition,
                            binding: binding.as_ref().map(|binding| binding.name.clone()),
                            value,
                            else_value,
                        }
                    }
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
                let start = complete_optional_value(start.as_deref(), |value| {
                    self.lower_scalar_expr(producer, value)
                });
                let end = complete_optional_value(end.as_deref(), |value| {
                    self.lower_scalar_expr(producer, value)
                });
                let step = complete_optional_value(step.as_deref(), |value| {
                    self.lower_scalar_expr(producer, value)
                });
                match (base, start, end, step) {
                    (Some(base), Some(start), Some(end), Some(step)) => {
                        ControlFlowValueKind::Slice {
                            base,
                            start,
                            end,
                            step,
                        }
                    }
                    _ => ControlFlowValueKind::Opaque,
                }
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
                let condition = complete_optional_value(condition.as_deref(), |condition| {
                    self.lower_scalar_expr(producer, condition)
                });
                let value = self.lower_scalar_expr(producer, value);
                self.scoped_definition_stack.pop();
                match (iterable_value, value, condition) {
                    (Some(iterable), Some(value), Some(condition)) => {
                        ControlFlowValueKind::ListComprehension {
                            binding: binding.clone(),
                            iterable,
                            value,
                            condition,
                        }
                    }
                    _ => ControlFlowValueKind::Opaque,
                }
            }
            ExprKind::RecordLiteral { fields } => fields
                .iter()
                .map(|field| {
                    self.lower_scalar_expr(producer, &field.value)
                        .map(|value| (field.name.clone(), value))
                })
                .collect::<Option<Vec<_>>>()
                .map_or(ControlFlowValueKind::Opaque, |fields| {
                    ControlFlowValueKind::RecordLiteral { fields }
                }),
            ExprKind::StructLiteral {
                name, base, fields, ..
            } => {
                let base = complete_optional_value(base.as_deref(), |value| {
                    self.lower_scalar_expr(producer, value)
                });
                let fields = fields
                    .iter()
                    .map(|field| {
                        self.lower_scalar_expr(producer, &field.value)
                            .map(|value| (field.name.clone(), value))
                    })
                    .collect::<Option<Vec<_>>>();
                match (base, fields) {
                    (Some(base), Some(fields)) => ControlFlowValueKind::StructLiteral {
                        name: name.clone(),
                        base,
                        fields,
                    },
                    _ => ControlFlowValueKind::Opaque,
                }
            }
            ExprKind::QualifiedCall {
                namespace,
                name,
                args,
                named_args,
                ..
            } => match self.lower_call_arguments(producer, args, named_args) {
                Some(arguments) if self.signatures.interface(namespace).is_some() => {
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
                    if named_args.is_empty() {
                        ControlFlowValueKind::InterfaceDispatch {
                            interface: namespace.clone(),
                            capability: name.clone(),
                            target: concrete_target,
                            mapped_function,
                            arguments,
                        }
                    } else {
                        let mut argument_names = vec![None; args.len()];
                        argument_names.extend(
                            named_args
                                .iter()
                                .map(|argument| Some(argument.name.clone())),
                        );
                        ControlFlowValueKind::NamedInterfaceDispatch {
                            interface: namespace.clone(),
                            capability: name.clone(),
                            target: concrete_target,
                            mapped_function,
                            arguments,
                            argument_names,
                        }
                    }
                }
                Some(arguments) if named_args.is_empty() => ControlFlowValueKind::QualifiedCall {
                    namespace: namespace.clone(),
                    name: name.clone(),
                    arguments,
                },
                Some(arguments) => {
                    let mut argument_names = vec![None; args.len()];
                    argument_names.extend(
                        named_args
                            .iter()
                            .map(|argument| Some(argument.name.clone())),
                    );
                    ControlFlowValueKind::NamedQualifiedCall {
                        namespace: namespace.clone(),
                        name: name.clone(),
                        arguments,
                        argument_names,
                    }
                }
                None => ControlFlowValueKind::Opaque,
            },
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
            ExprKind::Field {
                base,
                name,
                optional,
                ..
            } => self.lower_scalar_expr(producer, base).map_or(
                ControlFlowValueKind::Opaque,
                |base| ControlFlowValueKind::Field {
                    base,
                    name: name.clone(),
                    optional: *optional,
                },
            ),
            ExprKind::Match { value, arms } => {
                let matched_value = self.lower_scalar_expr(producer, value);
                let arm_patterns = arms
                    .iter()
                    .map(|arm| self.lower_match_arm_pattern(arm))
                    .collect::<Option<Vec<_>>>();
                let mut guards = Vec::with_capacity(arms.len());
                let mut arm_values = Vec::with_capacity(arms.len());
                let mut complete = true;
                for arm in arms {
                    let definitions = self.enum_match_expr_definitions(value, arm);
                    let definitions = self.add_scoped_definitions(producer, definitions);
                    self.record_scoped_borrow_sources(&definitions, matched_value);
                    self.push_scoped_definitions(definitions);
                    let guard = complete_optional_value(arm.guard.as_ref(), |guard| {
                        self.lower_scalar_expr(producer, guard)
                    });
                    let arm_value = self.lower_scalar_expr(producer, &arm.value);
                    self.scoped_definition_stack.pop();
                    match (guard, arm_value) {
                        (Some(guard), Some(arm_value)) => {
                            guards.push(guard);
                            arm_values.push(arm_value);
                        }
                        _ => complete = false,
                    }
                }
                match (matched_value, arm_patterns, complete) {
                    (Some(value), Some(arm_patterns), true)
                        if arm_patterns.len() == arms.len()
                            && guards.len() == arms.len()
                            && arm_values.len() == arms.len() =>
                    {
                        ControlFlowValueKind::Match {
                            value,
                            guards,
                            arms: arm_values,
                            arm_patterns,
                        }
                    }
                    _ => ControlFlowValueKind::Opaque,
                }
            }
            ExprKind::ListMatch { value, arms } => {
                let matched_value = self.lower_scalar_expr(producer, value);
                let arm_patterns = arms
                    .iter()
                    .map(|arm| self.lower_list_match_pattern(&arm.pattern))
                    .collect::<Option<Vec<_>>>();
                let mut guards = Vec::with_capacity(arms.len());
                let mut arm_values = Vec::with_capacity(arms.len());
                let mut complete = true;
                for arm in arms {
                    let definitions = self.list_match_definitions(value, &arm.pattern);
                    let definitions = self.add_scoped_definitions(producer, definitions);
                    self.record_scoped_borrow_sources(&definitions, matched_value);
                    self.push_scoped_definitions(definitions);
                    let guard = complete_optional_value(arm.guard.as_ref(), |guard| {
                        self.lower_scalar_expr(producer, guard)
                    });
                    let arm_value = self.lower_scalar_expr(producer, &arm.value);
                    self.scoped_definition_stack.pop();
                    match (guard, arm_value) {
                        (Some(guard), Some(arm_value)) => {
                            guards.push(guard);
                            arm_values.push(arm_value);
                        }
                        _ => complete = false,
                    }
                }
                match (matched_value, arm_patterns, complete) {
                    (Some(value), Some(arm_patterns), true)
                        if arm_patterns.len() == arms.len()
                            && guards.len() == arms.len()
                            && arm_values.len() == arms.len() =>
                    {
                        ControlFlowValueKind::ListMatch {
                            value,
                            guards,
                            arms: arm_values,
                            arm_patterns,
                        }
                    }
                    _ => ControlFlowValueKind::Opaque,
                }
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
    ) -> Option<Vec<ControlFlowValueId>> {
        let positional = self.lower_expr_arguments(producer, args);
        let named = complete_value_list(
            named_args
                .iter()
                .map(|arg| self.lower_scalar_expr(producer, &arg.value))
                .collect(),
        );
        match (positional, named) {
            (Some(mut positional), Some(named)) => {
                positional.extend(named);
                Some(positional)
            }
            _ => None,
        }
    }

    fn lower_expr_arguments(
        &mut self,
        producer: ControlFlowNodeId,
        expressions: &[Expr],
    ) -> Option<Vec<ControlFlowValueId>> {
        complete_value_list(
            expressions
                .iter()
                .map(|expr| self.lower_scalar_expr(producer, expr))
                .collect(),
        )
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
        let types = if types.is_empty()
            && matches!(
                &kind,
                ControlFlowValueKind::Await { .. }
                    | ControlFlowValueKind::Call { .. }
                    | ControlFlowValueKind::NamedCall { .. }
                    | ControlFlowValueKind::QualifiedCall { .. }
                    | ControlFlowValueKind::NamedQualifiedCall { .. }
                    | ControlFlowValueKind::InterfaceDispatch { .. }
                    | ControlFlowValueKind::NamedInterfaceDispatch { .. }
            ) {
            // Void calls still represent observable evaluation even though
            // they do not produce a source-level value. Preserve one typed
            // effect-only node so reachability/effects/backend consumers can
            // reconstruct that evaluation without consulting the checked AST.
            vec![Type::Void]
        } else {
            types
        };
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

    fn lower_match_arm_pattern(
        &self,
        arm: &crate::ast::MatchExprArm,
    ) -> Option<ControlFlowMatchArmPattern> {
        Some(ControlFlowMatchArmPattern {
            enum_name: arm.enum_name.clone(),
            variant: arm.variant.clone(),
            patterns: arm
                .patterns
                .iter()
                .map(|pattern| self.lower_match_pattern(pattern))
                .collect::<Option<Vec<_>>>()?,
        })
    }

    fn lower_match_pattern(
        &self,
        pattern: &crate::ast::MatchPattern,
    ) -> Option<ControlFlowMatchPattern> {
        match pattern {
            crate::ast::MatchPattern::Binding(binding) => Some(ControlFlowMatchPattern::Binding {
                name: binding.name.clone(),
            }),
            crate::ast::MatchPattern::Struct(pattern) => Some(ControlFlowMatchPattern::Struct(
                self.lower_struct_pattern(pattern),
            )),
            crate::ast::MatchPattern::Relational(pattern) => {
                Some(ControlFlowMatchPattern::Relational {
                    op: pattern.op,
                    value: typecheck::constant_primitive_value(&pattern.value, self.signatures)?,
                })
            }
            crate::ast::MatchPattern::Logical {
                left, op, right, ..
            } => Some(ControlFlowMatchPattern::Logical {
                left: Box::new(self.lower_match_pattern(left)?),
                op: match op {
                    crate::ast::PatternLogicalOp::And => ControlFlowPatternLogicalOp::And,
                    crate::ast::PatternLogicalOp::Or => ControlFlowPatternLogicalOp::Or,
                },
                right: Box::new(self.lower_match_pattern(right)?),
            }),
        }
    }

    fn lower_struct_pattern(
        &self,
        pattern: &crate::ast::StructPattern,
    ) -> ControlFlowStructPattern {
        ControlFlowStructPattern {
            name: pattern.struct_name.clone(),
            fields: pattern
                .fields
                .iter()
                .map(|field| ControlFlowStructPatternField {
                    field: field.field.clone(),
                    binding: field.binding.name.clone(),
                    nested: field
                        .nested
                        .as_deref()
                        .map(|nested| Box::new(self.lower_struct_pattern(nested))),
                })
                .collect(),
        }
    }

    fn lower_list_match_pattern(
        &self,
        pattern: &crate::ast::ListMatchPattern,
    ) -> Option<ControlFlowListMatchPattern> {
        match pattern {
            crate::ast::ListMatchPattern::List { bindings, rest, .. } => {
                Some(ControlFlowListMatchPattern::List {
                    bindings: bindings
                        .iter()
                        .map(|binding| binding.name.clone())
                        .collect(),
                    rest: rest.as_ref().map(|rest| ControlFlowListRestPattern {
                        binding: rest.binding.name.clone(),
                        index: rest.index,
                    }),
                })
            }
            crate::ast::ListMatchPattern::Wildcard { .. } => {
                Some(ControlFlowListMatchPattern::Wildcard)
            }
            crate::ast::ListMatchPattern::Map { entries, .. } => {
                Some(ControlFlowListMatchPattern::Map {
                    entries: entries
                        .iter()
                        .map(|entry| {
                            Some(ControlFlowMapPatternEntry {
                                key: typecheck::constant_primitive_value(
                                    &entry.key,
                                    self.signatures,
                                )?,
                                binding: entry.binding.name.clone(),
                            })
                        })
                        .collect::<Option<Vec<_>>>()?,
                })
            }
        }
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
        let Some(collection_ty) = self.scalar_expression_type(value) else {
            return Vec::new();
        };
        match (collection_ty, pattern) {
            (Type::List(element), crate::ast::ListMatchPattern::List { bindings, rest, .. }) => {
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
            (Type::Map(_, mapped_value), crate::ast::ListMatchPattern::Map { entries, .. }) => {
                entries
                    .iter()
                    .filter(|entry| entry.binding.name != "_")
                    .map(|entry| ControlFlowDefinition {
                        name: entry.binding.name.clone(),
                        ty: (*mapped_value).clone(),
                        span: entry.binding.span,
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
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
                    ControlFlowValueKind::Call { callee, arguments }
                    | ControlFlowValueKind::NamedCall {
                        callee, arguments, ..
                    } => (callee.clone(), arguments.clone()),
                    ControlFlowValueKind::QualifiedCall {
                        namespace,
                        name,
                        arguments,
                    }
                    | ControlFlowValueKind::NamedQualifiedCall {
                        namespace,
                        name,
                        arguments,
                        ..
                    } => (format!("{namespace}.{name}"), arguments.clone()),
                    ControlFlowValueKind::OptionalCascadeCall {
                        callee, arguments, ..
                    } => (callee.clone(), arguments.clone()),
                    ControlFlowValueKind::InterfaceDispatch {
                        interface,
                        capability,
                        arguments,
                        ..
                    }
                    | ControlFlowValueKind::NamedInterfaceDispatch {
                        interface,
                        capability,
                        arguments,
                        ..
                    } => (format!("{interface}.{capability}"), arguments.clone()),
                    _ => return None,
                };
                let argument_projections = arguments
                    .iter()
                    .map(|argument| self.value_projection(*argument))
                    .collect();
                Some(OwnershipCall {
                    callee,
                    arguments,
                    argument_projections,
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
                argument_projections: vec![Vec::new()],
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

    fn value_projection(&self, value: ControlFlowValueId) -> Vec<String> {
        let mut projection = Vec::new();
        let mut current = value;
        while let Some(value) = self.values.iter().find(|value| value.id == current) {
            match &value.kind {
                ControlFlowValueKind::Field { base, name, .. } => {
                    projection.push(name.clone());
                    current = *base;
                }
                _ => break,
            }
        }
        projection.reverse();
        projection
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
            ..
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
        ControlFlowValueKind::ListSpread { value, .. }
        | ControlFlowValueKind::ListOptional { value }
        | ControlFlowValueKind::Field { base: value, .. }
        | ControlFlowValueKind::Index { base: value, .. }
        | ControlFlowValueKind::Slice { base: value, .. } => {
            collect_call_argument_definitions(*value, values, visited, definitions);
        }
        ControlFlowValueKind::Call { callee, arguments }
        | ControlFlowValueKind::NamedCall {
            callee, arguments, ..
        } => {
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
        | ControlFlowValueKind::NamedQualifiedCall { arguments, .. }
        | ControlFlowValueKind::InterfaceDispatch { arguments, .. }
        | ControlFlowValueKind::NamedInterfaceDispatch { arguments, .. } => {
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
                    Type::List(element) | Type::Map(_, element) => Some(*element),
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
    match pattern {
        crate::ast::ListMatchPattern::List { bindings, rest, .. } => {
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
        crate::ast::ListMatchPattern::Map { entries, .. } => {
            for entry in entries {
                if entry.binding.name != "_" {
                    env.insert(entry.binding.name.clone(), element_ty.clone());
                }
            }
        }
        crate::ast::ListMatchPattern::Wildcard { .. } => {}
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
            ControlFlowValueKind::AnonymousFunction { body, .. } => {
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
            | ControlFlowValueKind::NamedCall { arguments, .. }
            | ControlFlowValueKind::QualifiedCall { arguments, .. }
            | ControlFlowValueKind::NamedQualifiedCall { arguments, .. }
            | ControlFlowValueKind::InterfaceDispatch { arguments, .. }
            | ControlFlowValueKind::NamedInterfaceDispatch { arguments, .. } => {
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
            | ControlFlowValueKind::ListSpread { value: packed, .. }
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
                ..
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
                ..
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
                ..
            }
            | ControlFlowValueKind::ListMatch {
                value: matched,
                guards,
                arms,
                ..
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

fn reclassify_borrowed_collection_reborrows(
    nodes: &mut [ControlFlowNode],
    edges: &[ControlFlowEdge],
    values: &[ControlFlowValue],
    definition_values: &BTreeMap<ControlFlowDefinitionId, ControlFlowValueId>,
    scoped_borrow_sources: &BTreeMap<ControlFlowDefinitionId, ControlFlowValueId>,
    parameters: &[ControlFlowParameter],
    signatures: &Signatures,
) {
    let is_collection_owner = is_non_copy_collection_type;
    let mut borrowed_definitions = parameters
        .iter()
        .enumerate()
        .filter(|(_, parameter)| is_collection_owner(&signatures.canonical_type(&parameter.ty)))
        .map(|(index, _)| ControlFlowDefinitionId::Parameter(index))
        .collect::<BTreeSet<_>>();
    borrowed_definitions.extend(scoped_borrow_sources.keys().copied());
    for node in nodes.iter() {
        for (index, definition) in node.definitions.iter().enumerate() {
            if !is_collection_owner(&signatures.canonical_type(&definition.ty)) {
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
                if !is_collection_owner(&signatures.canonical_type(&definition.ty)) {
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
        let consumed_by_move = node
            .ownership
            .moves
            .iter()
            .any(|movement| movement.value == Some(value.id));
        let consumed_by_call = node.ownership.calls.iter().any(|call| {
            call.arguments.iter().enumerate().any(|(index, argument)| {
                *argument == value.id
                    && call.argument_kind(index) == Some(OwnershipCallArgumentKind::Consuming)
            })
        });
        if consumed_by_move || consumed_by_call {
            continue;
        }
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
        ControlFlowValueKind::ListSpread { value, .. }
        | ControlFlowValueKind::ListOptional { value } => child(*value, visiting),
        ControlFlowValueKind::ListIf {
            condition,
            value,
            else_value,
            ..
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
            | ControlFlowValueKind::NamedCall { arguments, .. }
            | ControlFlowValueKind::QualifiedCall { arguments, .. }
            | ControlFlowValueKind::NamedQualifiedCall { arguments, .. }
            | ControlFlowValueKind::InterfaceDispatch { arguments, .. }
            | ControlFlowValueKind::NamedInterfaceDispatch { arguments, .. } => {
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

fn value_ownership(signatures: &Signatures, ty: &Type) -> ControlFlowValueOwnership {
    if signatures.canonical_type(ty) == Type::Void {
        ControlFlowValueOwnership::EffectOnly
    } else if signatures.is_copy_type(ty) {
        ControlFlowValueOwnership::Copy
    } else {
        // Every currently admitted value-bearing non-Copy value is a borrowed
        // collection descriptor. Keep this conservative fallback explicit so
        // a future owned value cannot accidentally inherit borrow semantics
        // when its type is added to the language.
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
        let name_reads = values
            .iter()
            .map(|value| match &value.kind {
                ControlFlowValueKind::NameRead { name, definitions } if !definitions.is_empty() => {
                    Some((name.clone(), definitions.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut changed = false;
        for value in values.iter_mut().filter(|value| value.constant.is_none()) {
            if let Some(constant) =
                propagated_ir_constant(value, &constants, definition_values, &name_reads)
            {
                value.constant = Some(constant);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

fn same_definition_scoped_name_read(
    left: ControlFlowValueId,
    right: ControlFlowValueId,
    name_reads: &[Option<(String, Vec<ControlFlowDefinitionId>)>],
) -> bool {
    matches!(
        (name_reads.get(left.0), name_reads.get(right.0)),
        (
            Some(Some((left_name, left_definitions))),
            Some(Some((right_name, right_definitions)))
        ) if left_name == right_name && left_definitions == right_definitions
    )
}

fn propagated_ir_constant(
    value: &ControlFlowValue,
    constants: &[Option<ConstantValue>],
    definition_values: &BTreeMap<ControlFlowDefinitionId, ControlFlowValueId>,
    name_reads: &[Option<(String, Vec<ControlFlowDefinitionId>)>],
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
            // Keep reflexive scalar comparison proofs in normalized IR instead of
            // relying on a backend AST reduction. Definition identity matters
            // here: same-spelled shadowed bindings are not interchangeable.
            if same_definition_scoped_name_read(*left, *right, name_reads) {
                match op {
                    BinOp::Eq | BinOp::Le | BinOp::Ge => {
                        return Some(ConstantValue::Bool(true));
                    }
                    BinOp::Ne | BinOp::Lt | BinOp::Gt => {
                        return Some(ConstantValue::Bool(false));
                    }
                    _ => {}
                }
            }
            let left_constant = constants.get(left.0)?.clone();
            // A short-circuiting left operand can determine the result even
            // when the right operand is dynamic.  Keep this proof in the
            // normalized value graph; `proven_scalar_constant` still checks
            // purity before a backend may substitute it, so an effectful
            // skipped branch is never silently removed.
            if matches!(
                (op, left_constant.as_ref()),
                (BinOp::And, Some(ConstantValue::Bool(false)))
                    | (BinOp::Or, Some(ConstantValue::Bool(true)))
            ) {
                return Some(match op {
                    BinOp::And => ConstantValue::Bool(false),
                    BinOp::Or => ConstantValue::Bool(true),
                    _ => unreachable!(),
                });
            }
            let right_constant = constants.get(right.0)?.clone();
            match (op, left_constant.as_ref(), right_constant.as_ref()) {
                (BinOp::And, _, Some(ConstantValue::Bool(false))) => {
                    Some(ConstantValue::Bool(false))
                }
                (BinOp::Or, _, Some(ConstantValue::Bool(true))) => Some(ConstantValue::Bool(true)),
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
    let is_collection_owner = is_non_copy_collection_type;
    let mut source_names = graph
        .parameters
        .iter()
        .filter(|parameter| is_collection_owner(&parameter.ty))
        .map(|parameter| parameter.name.clone())
        .collect::<BTreeSet<_>>();
    for node in &graph.nodes {
        source_names.extend(
            node.definitions
                .iter()
                .filter(|definition| is_collection_owner(&definition.ty))
                .map(|definition| definition.name.clone()),
        );
    }

    let mut sources_by_definition =
        BTreeMap::<ControlFlowDefinitionId, Vec<(ControlFlowDefinitionId, String)>>::new();
    for node in &graph.nodes {
        for (index, definition) in node.definitions.iter().enumerate() {
            if !is_collection_owner(&definition.ty) {
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
    fn incomplete_value_lists_fail_atomically() {
        let first = super::ControlFlowValueId(2);
        let second = super::ControlFlowValueId(3);
        assert_eq!(
            super::complete_value_list(vec![Some(first), Some(second)]),
            Some(vec![first, second])
        );
        assert_eq!(
            super::complete_value_list(vec![Some(first), None, Some(second)]),
            None
        );
    }

    #[test]
    fn incomplete_map_dependencies_do_not_fabricate_value_ids() {
        let first = super::ControlFlowValueId(4);
        let second = super::ControlFlowValueId(5);
        assert_eq!(
            super::complete_map_entry_values(vec![Some(first), Some(second)]),
            Some(vec![(first, second)])
        );
        assert_eq!(
            super::complete_map_entry_values(vec![Some(first), None]),
            None
        );
        assert_eq!(super::complete_map_entry_values(vec![Some(first)]), None);
    }

    #[test]
    fn optional_ir_children_preserve_source_presence() {
        let value = super::ControlFlowValueId(7);
        assert_eq!(
            super::complete_optional_value(Some("present"), |_| Some(value)),
            Some(Some(value))
        );
        assert_eq!(
            super::complete_optional_value(Some("present"), |_| None::<super::ControlFlowValueId>),
            None
        );
        assert_eq!(
            super::complete_optional_value(None::<&str>, |_| Some(value)),
            Some(None)
        );
    }

    #[test]
    fn normalized_cfg_persisted_cache_round_trips() {
        let database = crate::semantic::SemanticDatabase::analyze(
            "fn helper() -> i64 { 1 }\nfn main() -> i64 { helper() }\n",
            crate::diagnostic::SourceId::UNKNOWN,
        )
        .expect("persisted CFG fixture should analyze");
        let graph = database
            .control_flow_graph("main")
            .expect("main CFG should exist");

        assert!(
            graph.persisted_cache_is_valid(),
            "a freshly normalized CFG must satisfy persisted-cache invariants: nodes={} scoped={} reaching={} moves={} live_before={} live_after={} definition_before={} definition_after={} borrows={}",
            graph.nodes.len(),
            graph.scoped_definitions.len(),
            graph.reaching_definitions_before.len(),
            graph.move_states_before.len(),
            graph.live_before.len(),
            graph.live_after.len(),
            graph.definition_live_before.len(),
            graph.definition_live_after.len(),
            graph.borrow_states_before.len(),
        );
        let encoded = graph.encode_persisted();
        let decoded =
            super::ControlFlowGraph::decode_persisted(&encoded).expect("encoded CFG should decode");
        assert_eq!(&decoded, graph, "persisted CFG must round-trip exactly");

        let mut truncated = encoded;
        truncated.pop();
        assert!(
            super::ControlFlowGraph::decode_persisted(&truncated).is_none(),
            "truncated CFG payloads must be rejected"
        );
    }

    #[test]
    fn enum_match_patterns_preserve_codegen_metadata_through_persisted_ir() {
        let database = crate::semantic::SemanticDatabase::analyze(
            r#"const LIMIT: i64 = 10

enum Reading {
    Number(i64)
}

fn classify(reading: Reading) -> i64 {
    return match reading:
        Reading.Number(< LIMIT): 1
        Reading.Number(_): 2
}

fn main() -> i64 {
    return classify(Reading.Number(7))
}
"#,
            crate::diagnostic::SourceId::UNKNOWN,
        )
        .expect("enum-match IR fixture should analyze");
        let graph = database
            .control_flow_graph("classify")
            .expect("classify CFG should exist");

        let arm_patterns = graph
            .values()
            .iter()
            .find_map(|value| match &value.kind {
                super::ControlFlowValueKind::Match { arm_patterns, .. } => Some(arm_patterns),
                _ => None,
            })
            .expect("enum match should retain arm-pattern metadata");
        assert!(matches!(
            arm_patterns[0].patterns.as_slice(),
            [super::ControlFlowMatchPattern::Relational {
                op: crate::ast::BinOp::Lt,
                value: crate::typecheck::ConstantValue::I64(10),
            }]
        ));

        let encoded = graph.encode_persisted();
        let decoded = super::ControlFlowGraph::decode_persisted(&encoded)
            .expect("enum-match CFG should decode");
        assert_eq!(&decoded, graph);
    }

    #[test]
    fn list_match_patterns_preserve_codegen_metadata_through_persisted_ir() {
        let database = crate::semantic::SemanticDatabase::analyze(
            r#"fn choose(values: i64[]) -> i64 {
    return match values:
        []: 0
        [_]: 1
        [_, ...middle, _]: middle.length + 2
}

fn main() -> i64 {
    return choose([1, 2, 3])
}
"#,
            crate::diagnostic::SourceId::UNKNOWN,
        )
        .expect("list-match IR fixture should analyze");
        let graph = database
            .control_flow_graph("choose")
            .expect("choose CFG should exist");

        let arm_patterns = graph
            .values()
            .iter()
            .find_map(|value| match &value.kind {
                super::ControlFlowValueKind::ListMatch { arm_patterns, .. } => Some(arm_patterns),
                _ => None,
            })
            .expect("list match should retain arm-pattern metadata");
        assert_eq!(arm_patterns.len(), 3);
        assert!(matches!(
            &arm_patterns[2],
            super::ControlFlowListMatchPattern::List {
                bindings,
                rest: Some(rest),
            } if bindings == &["_".to_string(), "_".to_string()]
                && rest.binding == "middle"
                && rest.index == 1
        ));

        let encoded = graph.encode_persisted();
        let decoded = super::ControlFlowGraph::decode_persisted(&encoded)
            .expect("list-match CFG should decode");
        assert_eq!(&decoded, graph);
    }

    #[test]
    fn anonymous_function_metadata_preserves_codegen_shape_through_persisted_ir() {
        let database = crate::semantic::SemanticDatabase::analyze(
            r#"fn pick() -> fn(i64) -> i64 {
    return fn(value: i64) -> i64 { value + 1 }
}

fn main() -> i64 {
    let transform: fn(i64) -> i64 = pick()
    return transform(2)
}
"#,
            crate::diagnostic::SourceId::UNKNOWN,
        )
        .expect("anonymous-function IR fixture should analyze");
        let graph = database
            .control_flow_graph("pick")
            .expect("pick CFG should exist");

        let metadata = graph
            .values()
            .iter()
            .find_map(|value| match &value.kind {
                super::ControlFlowValueKind::AnonymousFunction {
                    params,
                    return_type,
                    ..
                } => Some((params.clone(), return_type.clone())),
                _ => None,
            })
            .expect("anonymous function should retain codegen metadata");
        assert_eq!(
            metadata,
            (
                vec![("value".to_string(), crate::ast::Type::I64)],
                Some(crate::ast::Type::I64),
            )
        );

        let encoded = graph.encode_persisted();
        let decoded = super::ControlFlowGraph::decode_persisted(&encoded)
            .expect("anonymous-function CFG should decode");
        assert_eq!(&decoded, graph);
    }

    #[test]
    fn legacy_anonymous_function_ir_decodes_without_codegen_metadata() {
        let mut encoded = Vec::new();
        super::PersistedIrCodec::encode_cache_value(&2u8, &mut encoded);
        super::PersistedIrCodec::encode_cache_value(&super::ControlFlowValueId(7), &mut encoded);
        let mut reader = super::PersistedIrReader::new(&encoded);
        let decoded = <super::ControlFlowValueKind as super::PersistedIrCodec>::decode_cache_value(
            &mut reader,
        )
        .expect("legacy anonymous-function value should decode");
        assert!(matches!(
            decoded,
            super::ControlFlowValueKind::AnonymousFunction {
                body: super::ControlFlowValueId(7),
                ref params,
                return_type: None,
            } if params.is_empty()
        ));
        assert!(reader.is_finished());
    }

    #[test]
    fn named_call_values_preserve_argument_names_through_persisted_ir() {
        let database = crate::semantic::SemanticDatabase::analyze(
            r#"fn adjust(value: i64, *, amount: i64) -> i64 {
    return value + amount
}

fn main() -> i64 {
    return adjust(2, amount: 3)
}
"#,
            crate::diagnostic::SourceId::UNKNOWN,
        )
        .expect("named-call IR fixture should analyze");
        let graph = database
            .control_flow_graph("main")
            .expect("main CFG should exist");

        let argument_names = graph
            .values()
            .iter()
            .find_map(|value| match &value.kind {
                super::ControlFlowValueKind::NamedCall {
                    callee,
                    argument_names,
                    ..
                } if callee == "adjust" => Some(argument_names.clone()),
                _ => None,
            })
            .expect("named call should retain argument names");
        assert_eq!(argument_names, vec![None, Some("amount".to_string())]);

        let encoded = graph.encode_persisted();
        let decoded = super::ControlFlowGraph::decode_persisted(&encoded)
            .expect("named-call CFG should decode");
        assert_eq!(&decoded, graph);
    }

    #[test]
    fn named_qualified_calls_preserve_argument_names_through_persisted_ir() {
        let value = super::ControlFlowValueKind::NamedQualifiedCall {
            namespace: "platform".to_string(),
            name: "operation".to_string(),
            arguments: vec![super::ControlFlowValueId(3), super::ControlFlowValueId(5)],
            argument_names: vec![None, Some("label".to_string())],
        };
        let mut encoded = Vec::new();
        super::PersistedIrCodec::encode_cache_value(&value, &mut encoded);
        let mut reader = super::PersistedIrReader::new(&encoded);
        let decoded = <super::ControlFlowValueKind as super::PersistedIrCodec>::decode_cache_value(
            &mut reader,
        )
        .expect("named qualified-call value should decode");
        assert_eq!(decoded, value);
        assert!(reader.is_finished());
    }

    #[test]
    fn named_interface_dispatch_preserves_argument_names_through_persisted_ir() {
        let database = crate::semantic::SemanticDatabase::analyze(
            r#"interface Measure {
    fn adjust(value: i64, *, delta: i64) -> i64
}

struct Offset {
    amount: i64
}

fn offsetAdjust(offset: Offset, value: i64, *, delta: i64) -> i64 {
    return offset.amount + value + delta
}

impl Measure for Offset {
    adjust: offsetAdjust
}

fn apply(offset: Offset, value: i64) -> i64 {
    return Measure.adjust(offset, value, delta: 3)
}

fn main() -> i64 {
    return 0
}
"#,
            crate::diagnostic::SourceId::UNKNOWN,
        )
        .expect("named interface-dispatch IR fixture should analyze");
        let graph = database
            .control_flow_graph("apply")
            .expect("apply CFG should exist");

        let argument_names = graph
            .values()
            .iter()
            .find_map(|value| match &value.kind {
                super::ControlFlowValueKind::NamedInterfaceDispatch {
                    interface,
                    capability,
                    argument_names,
                    ..
                } if interface == "Measure" && capability == "adjust" => {
                    Some(argument_names.clone())
                }
                _ => None,
            })
            .expect("named interface dispatch should retain argument names");
        assert_eq!(argument_names, vec![None, None, Some("delta".to_string())]);

        let encoded = graph.encode_persisted();
        let decoded = super::ControlFlowGraph::decode_persisted(&encoded)
            .expect("named interface-dispatch CFG should decode");
        assert_eq!(&decoded, graph);
    }

    #[test]
    fn field_values_preserve_optional_access_through_persisted_ir() {
        let database = crate::semantic::SemanticDatabase::analyze(
            r#"struct Point {
    x: i64
}

fn direct(point: Point) -> i64 {
    return point.x
}

fn optional(point: Point?) -> i64? {
    return point?.x
}

fn main() -> i64 {
    return 0
}
"#,
            crate::diagnostic::SourceId::UNKNOWN,
        )
        .expect("field IR fixture should analyze");

        for (function, expected_optional) in [("direct", false), ("optional", true)] {
            let graph = database
                .control_flow_graph(function)
                .expect("field CFG should exist");
            let field = graph
                .values()
                .iter()
                .find_map(|value| match &value.kind {
                    super::ControlFlowValueKind::Field { optional, .. } => Some(*optional),
                    _ => None,
                })
                .expect("typed IR should retain field projection");
            assert_eq!(field, expected_optional);

            let encoded = graph.encode_persisted();
            let decoded = super::ControlFlowGraph::decode_persisted(&encoded)
                .expect("field CFG should decode");
            assert_eq!(&decoded, graph);
        }
    }

    #[test]
    fn list_control_values_preserve_codegen_metadata_through_persisted_ir() {
        let database = crate::semantic::SemanticDatabase::analyze(
            r#"fn maybe(flag: bool) -> i64? {
    if flag:
        return 7
    return none
}

fn main() -> i64 {
    let present: i64[]? = [1, 2]
    let spread: i64[] = [...?present]
    let guarded: i64[] = [if let value = maybe(true): value else: 0]
    let source: i64[] = [1, 2, 3]
    let mapped: i64[] = [value * 2 for value in source if value > 1]
    return spread.length + guarded.length + mapped.length
}
"#,
            crate::diagnostic::SourceId::UNKNOWN,
        )
        .expect("list-control IR fixture should analyze");
        let graph = database
            .control_flow_graph("main")
            .expect("main CFG should exist");

        assert!(graph.values().iter().any(|value| {
            matches!(
                &value.kind,
                super::ControlFlowValueKind::ListSpread { optional: true, .. }
            )
        }));
        assert!(graph.values().iter().any(|value| {
            matches!(
                &value.kind,
                super::ControlFlowValueKind::ListIf {
                    binding: Some(binding),
                    ..
                } if binding == "value"
            )
        }));
        assert!(graph.values().iter().any(|value| {
            matches!(
                &value.kind,
                super::ControlFlowValueKind::ListComprehension { binding, .. }
                    if binding == "value"
            )
        }));

        let encoded = graph.encode_persisted();
        let decoded = super::ControlFlowGraph::decode_persisted(&encoded)
            .expect("list-control CFG should decode");
        assert_eq!(&decoded, graph);
    }

    #[test]
    fn scalar_comparison_constants_use_definition_identity() {
        let database = crate::semantic::SemanticDatabase::analyze(
            r#"fn lessSelf(value: i64) -> bool {
    return value < value
}

fn equalTextSelf(value: str) -> bool {
    return value == value
}

fn main() -> i64 {
    return 0
}
"#,
            crate::diagnostic::SourceId::UNKNOWN,
        )
        .expect("scalar identity fixture should analyze");

        let less = database
            .control_flow_graph("lessSelf")
            .expect("lessSelf CFG should exist");
        let less_value = less
            .values()
            .iter()
            .find(|value| {
                matches!(
                    value.kind,
                    super::ControlFlowValueKind::Binary {
                        op: crate::ast::BinOp::Lt,
                        ..
                    }
                )
            })
            .expect("typed IR should retain the comparison");
        assert_eq!(
            less.proven_scalar_constant(less_value.id),
            Some(&crate::typecheck::ConstantValue::Bool(false))
        );

        let equal = database
            .control_flow_graph("equalTextSelf")
            .expect("equalTextSelf CFG should exist");
        let equal_value = equal
            .values()
            .iter()
            .find(|value| {
                matches!(
                    value.kind,
                    super::ControlFlowValueKind::Binary {
                        op: crate::ast::BinOp::Eq,
                        ..
                    }
                )
            })
            .expect("typed IR should retain the equality");
        assert_eq!(
            equal.proven_scalar_constant(equal_value.id),
            Some(&crate::typecheck::ConstantValue::Bool(true))
        );

        let same_name_different_definitions = vec![
            Some((
                "value".to_string(),
                vec![super::ControlFlowDefinitionId::Parameter(0)],
            )),
            Some((
                "value".to_string(),
                vec![super::ControlFlowDefinitionId::Parameter(1)],
            )),
        ];
        assert!(!super::same_definition_scoped_name_read(
            super::ControlFlowValueId(0),
            super::ControlFlowValueId(1),
            &same_name_different_definitions,
        ));
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
                        argument_projections: vec![],
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
                argument_projections: vec![],
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
