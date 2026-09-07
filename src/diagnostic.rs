use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    pub line: usize,
    pub column: usize,
    pub length: usize,
}

impl SourceSpan {
    pub const fn new(line: usize, column: usize, length: usize) -> Self {
        Self {
            line,
            column,
            length,
        }
    }

    pub const fn line(line: usize) -> Self {
        Self::new(line, 1, 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticStage {
    Parse,
    Type,
    Codegen,
}

impl DiagnosticStage {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::Type => "type",
            Self::Codegen => "codegen",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub stage: DiagnosticStage,
    pub message: String,
    pub span: Option<SourceSpan>,
}

impl Diagnostic {
    pub fn new(stage: DiagnosticStage, span: SourceSpan, message: impl Into<String>) -> Self {
        Self {
            stage,
            message: message.into(),
            span: Some(span),
        }
    }

    pub fn global(stage: DiagnosticStage, message: impl Into<String>) -> Self {
        Self {
            stage,
            message: message.into(),
            span: None,
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.span {
            Some(span) => write!(
                formatter,
                "{} error at {}:{}: {}",
                self.stage.name(),
                span.line,
                span.column,
                self.message
            ),
            None => write!(formatter, "{} error: {}", self.stage.name(), self.message),
        }
    }
}

impl std::error::Error for Diagnostic {}
