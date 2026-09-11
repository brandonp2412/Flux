use crate::diagnostic::SourceSpan;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    I64,
    Bool,
    Str,
    Error,
    Void,
    Named(String),
    List(Box<Type>),
    Optional(Box<Type>),
    Function {
        params: Vec<Type>,
        returns: Vec<Type>,
    },
}

impl Type {
    pub fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        if let Some(rest) = input.strip_prefix("fn(") {
            let close = matching_type_paren(rest)?;
            let params_src = &rest[..close];
            let suffix = rest[close + 1..].trim();
            let returns_src = suffix.strip_prefix("->")?.trim();
            let params = if params_src.trim().is_empty() {
                Vec::new()
            } else {
                split_type_commas(params_src)
                    .into_iter()
                    .map(Self::parse)
                    .collect::<Option<Vec<_>>>()?
            };
            let returns = if returns_src == "void" {
                Vec::new()
            } else if let Some(inner) = returns_src
                .strip_prefix('(')
                .and_then(|value| value.strip_suffix(')'))
            {
                let values = split_type_commas(inner);
                if values.len() < 2 {
                    return None;
                }
                values
                    .into_iter()
                    .map(Self::parse)
                    .collect::<Option<Vec<_>>>()?
            } else {
                vec![Self::parse(returns_src)?]
            };
            return Some(Self::Function { params, returns });
        }
        if let Some(inner) = input.strip_suffix('?') {
            let inner = Self::parse(inner)?;
            if matches!(inner, Self::Void | Self::Optional(_)) {
                return None;
            }
            return Some(Self::Optional(Box::new(inner)));
        }
        if let Some(inner) = input.strip_suffix("[]") {
            return Some(Self::List(Box::new(Self::parse(inner)?)));
        }
        match input {
            "i64" => Some(Self::I64),
            "bool" => Some(Self::Bool),
            "str" => Some(Self::Str),
            "error" => Some(Self::Error),
            "void" => Some(Self::Void),
            name if is_type_identifier(name) => Some(Self::Named(name.to_string())),
            _ => None,
        }
    }

    pub fn name(&self) -> String {
        match self {
            Self::I64 => "i64".to_string(),
            Self::Bool => "bool".to_string(),
            Self::Str => "str".to_string(),
            Self::Error => "error".to_string(),
            Self::Void => "void".to_string(),
            Self::Named(name) => name.clone(),
            Self::List(element) => format!("{}[]", element.name()),
            Self::Optional(inner) => format!("{}?", inner.name()),
            Self::Function { params, returns } => {
                let params = params.iter().map(Type::name).collect::<Vec<_>>().join(", ");
                let returns = match returns.as_slice() {
                    [] => "void".to_string(),
                    [ty] => ty.name(),
                    _ => format!(
                        "({})",
                        returns
                            .iter()
                            .map(Type::name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                };
                format!("fn({params}) -> {returns}")
            }
        }
    }
}

fn matching_type_paren(input: &str) -> Option<usize> {
    let mut depth = 1usize;
    for (index, byte) in input.bytes().enumerate() {
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

fn split_type_commas(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, byte) in input.bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                parts.push(input[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(input[start..].trim());
    parts
}

fn is_type_identifier(input: &str) -> bool {
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

#[derive(Debug, Clone, Default)]
pub struct Program {
    pub imports: Vec<ImportDef>,
    pub aliases: Vec<TypeAlias>,
    pub interfaces: Vec<InterfaceDef>,
    pub implementations: Vec<InterfaceImpl>,
    pub structs: Vec<StructDef>,
    pub enums: Vec<EnumDef>,
    pub constants: Vec<ConstantDef>,
    pub application: Option<ApplicationDef>,
    pub views: Vec<ViewDef>,
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone)]
pub struct ImportDef {
    pub path: String,
    pub path_span: SourceSpan,
    pub resolved_source_id: Option<crate::diagnostic::SourceId>,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct InterfaceDef {
    pub public: bool,
    pub name: String,
    pub name_span: SourceSpan,
    pub keyword_span: SourceSpan,
    pub parents: Vec<InterfaceParent>,
    pub functions: Vec<InterfaceFunction>,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct InterfaceParent {
    pub name: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct InterfaceFunction {
    pub name: String,
    pub name_span: SourceSpan,
    pub keyword_span: SourceSpan,
    pub params: Vec<Param>,
    pub returns: Vec<Type>,
    pub return_span: SourceSpan,
    pub return_type_spans: Vec<SourceSpan>,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct InterfaceImpl {
    pub interface_name: String,
    pub interface_span: SourceSpan,
    pub target_name: String,
    pub target_span: SourceSpan,
    pub keyword_span: SourceSpan,
    pub mappings: Vec<InterfaceImplMapping>,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct InterfaceImplMapping {
    pub member: String,
    pub member_span: SourceSpan,
    pub function: String,
    pub function_span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct TypeAlias {
    pub public: bool,
    pub name: String,
    pub name_span: SourceSpan,
    pub target: Type,
    pub target_span: SourceSpan,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct ConstantDef {
    pub public: bool,
    pub name: String,
    pub name_span: SourceSpan,
    pub ty: Type,
    pub type_span: SourceSpan,
    pub value: Expr,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub public: bool,
    pub name: String,
    pub name_span: SourceSpan,
    pub keyword_span: SourceSpan,
    pub variants: Vec<EnumVariant>,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub name_span: SourceSpan,
    pub payloads: Vec<EnumPayload>,
}

#[derive(Debug, Clone)]
pub struct EnumPayload {
    pub ty: Type,
    pub type_span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub public: bool,
    pub name: String,
    pub name_span: SourceSpan,
    pub keyword_span: SourceSpan,
    pub fields: Vec<StructField>,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub name_span: SourceSpan,
    pub ty: Type,
    pub type_span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct ApplicationMetadataField {
    pub name: String,
    pub name_span: SourceSpan,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct ApplicationDef {
    pub view_name: String,
    pub view_span: SourceSpan,
    pub keyword_span: SourceSpan,
    pub metadata: Vec<ApplicationMetadataField>,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct ViewDef {
    pub public: bool,
    pub name: String,
    pub name_span: SourceSpan,
    pub keyword_span: SourceSpan,
    pub params: Vec<Param>,
    pub states: Vec<ViewState>,
    pub derived: Vec<ViewDerived>,
    pub grid: GridLayout,
    pub elements: Vec<ViewElement>,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct ViewState {
    pub name: String,
    pub name_span: SourceSpan,
    pub ty: Type,
    pub type_span: SourceSpan,
    pub initial: Expr,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct ViewDerived {
    pub name: String,
    pub name_span: SourceSpan,
    pub ty: Type,
    pub type_span: SourceSpan,
    pub value: Expr,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Default)]
pub struct GridLayout {
    pub columns: Vec<GridTrack>,
    pub rows: Vec<GridTrack>,
    pub flow: Option<FlowDirection>,
    pub flow_line: Option<usize>,
    pub gap: Option<u32>,
    pub padding: Option<u32>,
    pub padding_line: Option<usize>,
    pub scroll: Option<bool>,
    pub scroll_line: Option<usize>,
    pub overlay: Option<bool>,
    pub overlay_line: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GridTrack {
    Units(u32),
    Fraction(u32),
    Auto,
}

#[derive(Debug, Clone)]
pub struct ViewElement {
    pub kind: String,
    pub kind_span: SourceSpan,
    pub name: String,
    pub name_span: SourceSpan,
    pub row: u32,
    pub column: u32,
    pub row_span: u32,
    pub column_span: u32,
    pub properties: Vec<ViewProperty>,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct ViewProperty {
    pub name: String,
    pub name_span: SourceSpan,
    pub value: Expr,
    pub transition: Option<ViewStateTransition>,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct ViewStateTransition {
    pub state: String,
    pub state_span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub public: bool,
    pub name: String,
    pub name_span: SourceSpan,
    pub keyword_span: SourceSpan,
    pub params: Vec<Param>,
    pub returns: Vec<Type>,
    pub return_span: SourceSpan,
    pub return_type_spans: Vec<SourceSpan>,
    pub body: Vec<Stmt>,
    pub expression_body: bool,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub name_span: SourceSpan,
    pub ty: Type,
    pub type_span: SourceSpan,
    pub named_only: bool,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub name: String,
    pub name_span: SourceSpan,
    pub ty: Type,
    pub type_span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub line: usize,
    pub span: SourceSpan,
    pub keyword_span: SourceSpan,
    pub kind: StmtKind,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Let {
        name: String,
        name_span: SourceSpan,
        ty: Type,
        type_span: SourceSpan,
        expr: Expr,
    },
    Var {
        name: String,
        name_span: SourceSpan,
        ty: Type,
        type_span: SourceSpan,
        expr: Expr,
    },
    Assign {
        name: String,
        name_span: SourceSpan,
        expr: Expr,
        coalescing: bool,
    },
    AssignMultiDestructure {
        bindings: Vec<PatternBinding>,
        expr: Expr,
    },
    AssignListDestructure {
        bindings: Vec<PatternBinding>,
        rest: Option<ListRestPattern>,
        expr: Expr,
    },
    AssignStructDestructure {
        struct_name: String,
        struct_span: SourceSpan,
        fields: Vec<StructPatternField>,
        expr: Expr,
    },
    LetDestructure {
        bindings: Vec<Binding>,
        expr: Expr,
        else_return: bool,
        mutable: bool,
    },
    LetMultiDestructure {
        bindings: Vec<PatternBinding>,
        expr: Expr,
        else_return: bool,
        mutable: bool,
    },
    LetListDestructure {
        bindings: Vec<PatternBinding>,
        rest: Option<ListRestPattern>,
        expr: Expr,
        mutable: bool,
    },
    LetStructDestructure {
        struct_name: String,
        struct_span: SourceSpan,
        fields: Vec<StructPatternField>,
        expr: Expr,
        mutable: bool,
    },
    Return(Vec<Expr>),
    Break,
    Continue,
    Expr(Expr),
    Shell {
        expr: Expr,
        redirect: Option<ShellRedirect>,
        background: bool,
    },
    If {
        cond: Expr,
        binding: Option<PatternBinding>,
        body: Vec<Stmt>,
        else_body: Vec<Stmt>,
        else_keyword_span: Option<SourceSpan>,
    },
    ForRange {
        name: String,
        name_span: SourceSpan,
        start: Expr,
        end: Expr,
        inclusive: bool,
        body: Vec<Stmt>,
    },
    ForEach {
        index_name: Option<String>,
        index_span: Option<SourceSpan>,
        name: String,
        name_span: SourceSpan,
        iterable: Expr,
        body: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    Match {
        value: Expr,
        arms: Vec<MatchArm>,
    },
    ListMatch {
        value: Expr,
        arms: Vec<ListMatchArm>,
    },
}

#[derive(Debug, Clone)]
pub struct ShellRedirect {
    pub path: Expr,
    pub mode: ShellRedirectMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellRedirectMode {
    Truncate,
    Append,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub enum_name: String,
    pub enum_span: SourceSpan,
    pub variant: String,
    pub variant_span: SourceSpan,
    pub patterns: Vec<MatchPattern>,
    pub guard: Option<Expr>,
    pub body: Vec<Stmt>,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct MatchExprArm {
    pub enum_name: String,
    pub enum_span: SourceSpan,
    pub variant: String,
    pub variant_span: SourceSpan,
    pub patterns: Vec<MatchPattern>,
    pub guard: Option<Expr>,
    pub value: Expr,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct ListMatchArm {
    pub pattern: ListMatchPattern,
    pub guard: Option<Expr>,
    pub body: Vec<Stmt>,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct ListMatchExprArm {
    pub pattern: ListMatchPattern,
    pub guard: Option<Expr>,
    pub value: Expr,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub enum ListMatchPattern {
    List {
        bindings: Vec<PatternBinding>,
        rest: Option<ListRestPattern>,
        span: SourceSpan,
    },
    Wildcard {
        span: SourceSpan,
    },
}

#[derive(Debug, Clone)]
pub enum MatchPattern {
    Binding(PatternBinding),
    Struct(StructPattern),
    Relational(RelationalPattern),
    Logical {
        left: Box<MatchPattern>,
        op: PatternLogicalOp,
        right: Box<MatchPattern>,
        span: SourceSpan,
    },
}

#[derive(Debug, Clone)]
pub struct RelationalPattern {
    pub op: BinOp,
    pub value: Expr,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternLogicalOp {
    And,
    Or,
}

#[derive(Debug, Clone)]
pub struct PatternBinding {
    pub name: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct ListRestPattern {
    pub binding: PatternBinding,
    pub index: usize,
}

#[derive(Debug, Clone)]
pub struct StructPattern {
    pub struct_name: String,
    pub struct_span: SourceSpan,
    pub fields: Vec<StructPatternField>,
}

#[derive(Debug, Clone)]
pub struct StructPatternField {
    pub field: String,
    pub field_span: SourceSpan,
    pub binding: PatternBinding,
    pub nested: Option<Box<StructPattern>>,
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub line: usize,
    pub span: SourceSpan,
    pub kind: ExprKind,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    Bool(bool),
    Str(String),
    Nil,
    None,
    Var(String),
    AnonymousFunction {
        params: Vec<Param>,
        return_type: Option<Type>,
        body: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
        named_args: Vec<NamedArg>,
    },
    ShellCall {
        name: String,
        name_span: SourceSpan,
        args: Vec<Expr>,
    },
    Pipe {
        input: Box<Expr>,
        name: String,
        name_span: SourceSpan,
        args: Vec<Expr>,
        optional: bool,
    },
    List(Vec<Expr>),
    ListSpread {
        value: Box<Expr>,
        spread_span: SourceSpan,
    },
    ListOptional {
        value: Box<Expr>,
        question_span: SourceSpan,
    },
    ListIf {
        condition: Box<Expr>,
        binding: Option<PatternBinding>,
        value: Box<Expr>,
        else_value: Option<Box<Expr>>,
        if_span: SourceSpan,
        else_span: Option<SourceSpan>,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Slice {
        base: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        step: Option<Box<Expr>>,
    },
    ListComprehension {
        value: Box<Expr>,
        binding: String,
        binding_span: SourceSpan,
        iterable: Box<Expr>,
        condition: Option<Box<Expr>>,
    },
    StructLiteral {
        name: String,
        name_span: SourceSpan,
        base: Option<Box<Expr>>,
        fields: Vec<StructLiteralField>,
    },
    QualifiedCall {
        namespace: String,
        namespace_span: SourceSpan,
        name: String,
        name_span: SourceSpan,
        args: Vec<Expr>,
        named_args: Vec<NamedArg>,
    },
    Field {
        base: Box<Expr>,
        name: String,
        name_span: SourceSpan,
        optional: bool,
    },
    Match {
        value: Box<Expr>,
        arms: Vec<MatchExprArm>,
    },
    ListMatch {
        value: Box<Expr>,
        arms: Vec<ListMatchExprArm>,
    },
    Conditional {
        then_expr: Box<Expr>,
        cond: Box<Expr>,
        else_expr: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
    },
}

#[derive(Debug, Clone)]
pub struct NamedArg {
    pub name: String,
    pub name_span: SourceSpan,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct StructLiteralField {
    pub name: String,
    pub name_span: SourceSpan,
    pub value: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Coalesce,
}
