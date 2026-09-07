use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SourceId(u32);

impl SourceId {
    pub const UNKNOWN: Self = Self(0);

    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u32 {
        self.0
    }

    pub fn from_name(name: &str) -> Self {
        let mut hash = 0x811c9dc5u32;
        for byte in name.as_bytes() {
            hash ^= u32::from(*byte);
            hash = hash.wrapping_mul(0x01000193);
        }
        if hash == 0 { Self(1) } else { Self(hash) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    pub source_id: SourceId,
    pub line: usize,
    pub column: usize,
    pub length: usize,
}

impl SourceSpan {
    pub const fn new(line: usize, column: usize, length: usize) -> Self {
        Self {
            source_id: SourceId::UNKNOWN,
            line,
            column,
            length,
        }
    }

    pub const fn line(line: usize) -> Self {
        Self::new(line, 1, 1)
    }

    pub const fn with_source(mut self, source_id: SourceId) -> Self {
        self.source_id = source_id;
        self
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

    pub fn with_source(mut self, source_id: SourceId) -> Self {
        self.span = self.span.map(|span| span.with_source(source_id));
        self
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
