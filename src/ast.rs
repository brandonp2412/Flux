use crate::diagnostic::SourceSpan;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    I64,
    Bool,
    Str,
    Error,
    Void,
}

impl Type {
    pub fn parse(input: &str) -> Option<Self> {
        match input.trim() {
            "i64" => Some(Self::I64),
            "bool" => Some(Self::Bool),
            "str" => Some(Self::Str),
            "error" => Some(Self::Error),
            "void" => Some(Self::Void),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::I64 => "i64",
            Self::Bool => "bool",
            Self::Str => "str",
            Self::Error => "error",
            Self::Void => "void",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Program {
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub returns: Vec<Type>,
    pub body: Vec<Stmt>,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub line: usize,
    pub span: SourceSpan,
    pub kind: StmtKind,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Let {
        name: String,
        ty: Type,
        expr: Expr,
    },
    LetDestructure {
        bindings: Vec<Binding>,
        expr: Expr,
    },
    Return(Vec<Expr>),
    Expr(Expr),
    If {
        cond: Expr,
        body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    ForRange {
        name: String,
        start: Expr,
        end: Expr,
        body: Vec<Stmt>,
    },
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
