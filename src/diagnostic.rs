use std::fmt;

pub const DIAGNOSTIC_JSON_SCHEMA_VERSION: u32 = 1;

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
pub struct DiagnosticLabel {
    pub span: SourceSpan,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticFix {
    pub span: SourceSpan,
    pub replacement: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub stage: DiagnosticStage,
    pub message: String,
    pub span: Option<SourceSpan>,
    pub labels: Box<Vec<DiagnosticLabel>>,
    pub notes: Box<Vec<String>>,
    pub fixes: Box<Vec<DiagnosticFix>>,
}

impl Diagnostic {
    pub fn new(stage: DiagnosticStage, span: SourceSpan, message: impl Into<String>) -> Self {
        Self {
            stage,
            message: message.into(),
            span: Some(span),
            labels: Box::default(),
            notes: Box::default(),
            fixes: Box::default(),
        }
    }

    pub fn global(stage: DiagnosticStage, message: impl Into<String>) -> Self {
        Self {
            stage,
            message: message.into(),
            span: None,
            labels: Box::default(),
            notes: Box::default(),
            fixes: Box::default(),
        }
    }

    pub fn with_source(mut self, source_id: SourceId) -> Self {
        self.span = self.span.map(|span| span.with_source(source_id));
        for label in self.labels.iter_mut() {
            label.span = label.span.with_source(source_id);
        }
        for fix in self.fixes.iter_mut() {
            fix.span = fix.span.with_source(source_id);
        }
        self
    }

    pub fn with_label(mut self, span: SourceSpan, message: impl Into<String>) -> Self {
        self.labels.push(DiagnosticLabel {
            span,
            message: message.into(),
        });
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_fix(
        mut self,
        span: SourceSpan,
        replacement: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.fixes.push(DiagnosticFix {
            span,
            replacement: replacement.into(),
            message: message.into(),
        });
        self
    }

    pub fn to_json(&self) -> String {
        let span = self
            .span
            .map(span_to_json)
            .unwrap_or_else(|| "null".to_string());
        let labels = self
            .labels
            .iter()
            .map(|label| {
                format!(
                    "{{\"span\":{},\"message\":{}}}",
                    span_to_json(label.span),
                    json_string(&label.message)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let notes = self
            .notes
            .iter()
            .map(|note| json_string(note))
            .collect::<Vec<_>>()
            .join(",");
        let fixes = self
            .fixes
            .iter()
            .map(|fix| {
                format!(
                    "{{\"span\":{},\"replacement\":{},\"message\":{}}}",
                    span_to_json(fix.span),
                    json_string(&fix.replacement),
                    json_string(&fix.message)
                )
            })
            .collect::<Vec<_>>()
            .join(",");

        format!(
            "{{\"stage\":{},\"message\":{},\"span\":{span},\"labels\":[{labels}],\"notes\":[{notes}],\"fixes\":[{fixes}]}}",
            json_string(self.stage.name()),
            json_string(&self.message)
        )
    }
}

pub fn diagnostics_to_json(diagnostics: &[Diagnostic]) -> String {
    format!(
        "[{}]",
        diagnostics
            .iter()
            .map(Diagnostic::to_json)
            .collect::<Vec<_>>()
            .join(",")
    )
}

pub fn diagnostics_envelope_to_json(
    ok: bool,
    source_id: SourceId,
    diagnostics: &[Diagnostic],
) -> String {
    format!(
        "{{\"schema_version\":{},\"ok\":{},\"source_id\":{},\"diagnostics\":{}}}",
        DIAGNOSTIC_JSON_SCHEMA_VERSION,
        ok,
        source_id.value(),
        diagnostics_to_json(diagnostics)
    )
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
            )?,
            None => write!(formatter, "{} error: {}", self.stage.name(), self.message)?,
        }
        for label in self.labels.iter() {
            write!(
                formatter,
                "\n  label at {}:{}: {}",
                label.span.line, label.span.column, label.message
            )?;
        }
        for note in self.notes.iter() {
            write!(formatter, "\n  note: {note}")?;
        }
        for fix in self.fixes.iter() {
            write!(
                formatter,
                "\n  help at {}:{}: {} (replace with {})",
                fix.span.line,
                fix.span.column,
                fix.message,
                json_string(&fix.replacement)
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for Diagnostic {}

fn span_to_json(span: SourceSpan) -> String {
    format!(
        "{{\"source_id\":{},\"line\":{},\"column\":{},\"length\":{}}}",
        span.source_id.value(),
        span.line,
        span.column,
        span.length
    )
}

fn json_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch < ' ' => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}
