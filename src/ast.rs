use crate::diagnostic::SourceSpan;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    I64,
    Bool,
    Str,
    Error,
    Void,
    Named(String),
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
    LetDestructure {
        bindings: Vec<Binding>,
        expr: Expr,
        else_return: bool,
    },
    LetStructDestructure {
        struct_name: String,
        struct_span: SourceSpan,
        fields: Vec<StructPatternField>,
        expr: Expr,
    },
    Return(Vec<Expr>),
    Break,
    Continue,
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
    Var(String),
    Call {
        name: String,
        args: Vec<Expr>,
        named_args: Vec<NamedArg>,
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
