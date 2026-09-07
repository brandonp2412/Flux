use crate::diagnostic::SourceSpan;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    I64,
    Bool,
    Str,
    Error,
    Void,
    Named(String),
}

impl Type {
    pub fn parse(input: &str) -> Option<Self> {
        match input.trim() {
            "i64" => Some(Self::I64),
            "bool" => Some(Self::Bool),
            "str" => Some(Self::Str),
            "error" => Some(Self::Error),
            "void" => Some(Self::Void),
            name if is_type_identifier(name) => Some(Self::Named(name.to_string())),
            _ => None,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::I64 => "i64",
            Self::Bool => "bool",
            Self::Str => "str",
            Self::Error => "error",
            Self::Void => "void",
            Self::Named(name) => name,
        }
    }
}

fn is_type_identifier(input: &str) -> bool {
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

#[derive(Debug, Clone)]
pub struct Program {
    pub aliases: Vec<TypeAlias>,
    pub structs: Vec<StructDef>,
    pub enums: Vec<EnumDef>,
    pub constants: Vec<ConstantDef>,
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone)]
pub struct TypeAlias {
    pub name: String,
    pub name_span: SourceSpan,
    pub target: Type,
    pub target_span: SourceSpan,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct ConstantDef {
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
pub struct Function {
    pub name: String,
    pub name_span: SourceSpan,
    pub keyword_span: SourceSpan,
    pub params: Vec<Param>,
    pub returns: Vec<Type>,
    pub return_span: SourceSpan,
    pub return_type_spans: Vec<SourceSpan>,
    pub body: Vec<Stmt>,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub name_span: SourceSpan,
    pub ty: Type,
    pub type_span: SourceSpan,
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
    LetDestructure {
        bindings: Vec<Binding>,
        expr: Expr,
        else_return: bool,
    },
    Return(Vec<Expr>),
    Expr(Expr),
    If {
        cond: Expr,
        body: Vec<Stmt>,
        else_body: Vec<Stmt>,
        else_keyword_span: Option<SourceSpan>,
    },
    ForRange {
        name: String,
        name_span: SourceSpan,
        start: Expr,
        end: Expr,
        body: Vec<Stmt>,
    },
    Match {
        value: Expr,
        arms: Vec<MatchArm>,
    },
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub enum_name: String,
    pub enum_span: SourceSpan,
    pub variant: String,
    pub variant_span: SourceSpan,
    pub bindings: Vec<PatternBinding>,
    pub body: Vec<Stmt>,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct PatternBinding {
    pub name: String,
    pub span: SourceSpan,
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
    Var(String),
    Call {
        name: String,
        args: Vec<Expr>,
    },
    StructLiteral {
        name: String,
        name_span: SourceSpan,
        base: Option<Box<Expr>>,
        fields: Vec<StructLiteralField>,
    },
    EnumVariant {
        enum_name: String,
        enum_span: SourceSpan,
        variant: String,
        variant_span: SourceSpan,
        args: Vec<Expr>,
    },
    Field {
        base: Box<Expr>,
        name: String,
        name_span: SourceSpan,
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
pub struct StructLiteralField {
    pub name: String,
    pub name_span: SourceSpan,
    pub value: Expr,
}

#[derive(Debug, Clone, Copy)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy)]
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
}
