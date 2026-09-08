use crate::diagnostic::{Diagnostic, DiagnosticFix, DiagnosticLabel, SourceId, SourceSpan};

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_CYAN: &str = "\x1b[36m";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticSource {
    pub source_id: SourceId,
    pub name: String,
    pub text: String,
}

impl DiagnosticSource {
    pub fn new(source_id: SourceId, name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            source_id,
            name: name.into(),
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalRenderOptions {
    pub width: usize,
    pub color: bool,
}

impl Default for TerminalRenderOptions {
    fn default() -> Self {
        Self {
            width: 100,
            color: false,
        }
    }
}

pub fn render_diagnostics(
    diagnostics: &[Diagnostic],
    sources: &[DiagnosticSource],
    options: TerminalRenderOptions,
) -> String {
    let width = options.width.clamp(24, 240);
    let mut out = String::new();
    for (index, diagnostic) in diagnostics.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        render_diagnostic(&mut out, diagnostic, sources, width, options.color);
    }
    out
}

fn render_diagnostic(
    out: &mut String,
    diagnostic: &Diagnostic,
    sources: &[DiagnosticSource],
    width: usize,
    color: bool,
) {
    let kind = format!("error[{}]", diagnostic.stage.name());
    render_header(out, &kind, &diagnostic.message, width, color);

    if let Some(span) = diagnostic.span {
        render_span_block(out, span, None, sources, width, color, MarkerKind::Error);
    }

    for label in diagnostic.labels.iter() {
        render_label(out, label, diagnostic.span, sources, width, color);
    }

    for note in diagnostic.notes.iter() {
        render_prefixed_wrapped(out, "note", note, width, ANSI_CYAN, color);
    }

    for fix in diagnostic.fixes.iter() {
        render_fix(out, fix, diagnostic.span, sources, width, color);
    }
}

fn render_label(
    out: &mut String,
    label: &DiagnosticLabel,
    primary: Option<SourceSpan>,
    sources: &[DiagnosticSource],
    width: usize,
    color: bool,
) {
    if primary == Some(label.span) {
        render_prefixed_wrapped(out, "label", &label.message, width, ANSI_RED, color);
        return;
    }
    render_span_block(
        out,
        label.span,
        Some(&label.message),
        sources,
        width,
        color,
        MarkerKind::Label,
    );
}

fn render_fix(
    out: &mut String,
    fix: &DiagnosticFix,
    primary: Option<SourceSpan>,
    sources: &[DiagnosticSource],
    width: usize,
    color: bool,
) {
    let replacement = visible_replacement(&fix.replacement, fix.span.length);
    let message = if replacement.is_empty() {
        fix.message.clone()
    } else {
        format!("{}: {replacement}", fix.message)
    };
    render_prefixed_wrapped(out, "help", &message, width, ANSI_GREEN, color);
    if primary != Some(fix.span) {
        render_span_block(out, fix.span, None, sources, width, color, MarkerKind::Fix);
    }
}

#[derive(Debug, Clone, Copy)]
enum MarkerKind {
    Error,
    Label,
    Fix,
}

fn render_span_block(
    out: &mut String,
    span: SourceSpan,
    message: Option<&str>,
    sources: &[DiagnosticSource],
    width: usize,
    color: bool,
    kind: MarkerKind,
) {
    let source = find_source(span.source_id, sources);
    let name = source
        .map(|source| source.name.as_str())
        .unwrap_or("<source>");
    let location = compact_location(&format!("{name}:{}:{}", span.line, span.column), width);
    out.push_str(&paint(&location, ANSI_CYAN, false, color));
    out.push('\n');

    let Some(source) = source else {
        if let Some(message) = message {
            render_prefixed_wrapped(out, "label", message, width, ANSI_RED, color);
        }
        return;
    };
    let lines = source.text.lines().collect::<Vec<_>>();
    if span.line == 0 || span.line > lines.len() {
        if let Some(message) = message {
            render_prefixed_wrapped(out, "label", message, width, ANSI_RED, color);
        }
        return;
    }

    let number_width = span.line.to_string().len().max(1);
    let line = lines[span.line - 1];
    let gutter = format!("{:>number_width$} | ", span.line);
    let available = width.saturating_sub(gutter.chars().count()).max(1);
    let excerpt = crop_line(line, span.column, span.length, available);

    out.push_str(&paint(
        &format!("{:>number_width$} | ", span.line),
        ANSI_DIM,
        false,
        color,
    ));
    out.push_str(&excerpt.text);
    out.push('\n');

    let marker_prefix = format!("{} | ", " ".repeat(number_width));
    out.push_str(&paint(&marker_prefix, ANSI_DIM, false, color));
    out.push_str(&" ".repeat(excerpt.marker_offset));
    let marker_len = excerpt.marker_len.max(1);
    let marker = match kind {
        MarkerKind::Fix if span.length == 0 => "+",
        _ => "^",
    };
    let marker_text = marker.repeat(marker_len);
    let marker_color = match kind {
        MarkerKind::Fix => ANSI_GREEN,
        MarkerKind::Error | MarkerKind::Label => ANSI_RED,
    };
    out.push_str(&paint(&marker_text, marker_color, true, color));
    if let Some(message) = message {
        let used = marker_prefix.chars().count() + excerpt.marker_offset + marker_len + 1;
        if used + message.chars().count() <= width {
            out.push(' ');
            out.push_str(&paint(message, marker_color, false, color));
            out.push('\n');
        } else {
            out.push('\n');
            render_prefixed_wrapped(out, "label", message, width, marker_color, color);
        }
    } else {
        out.push('\n');
    }
}

fn render_header(out: &mut String, kind: &str, message: &str, width: usize, color: bool) {
    let prefix = format!("{kind}: ");
    let available = width.saturating_sub(prefix.chars().count()).max(1);
    let wrapped = wrap_words(message, available);
    for (index, line) in wrapped.iter().enumerate() {
        if index == 0 {
            out.push_str(&paint(kind, ANSI_RED, true, color));
            out.push_str(": ");
        } else {
            out.push_str(&" ".repeat(prefix.chars().count()));
        }
        out.push_str(&paint(line, ANSI_BOLD, false, color));
        out.push('\n');
    }
}

fn compact_location(location: &str, width: usize) -> String {
    let prefix = "--> ";
    let available = width.saturating_sub(prefix.len());
    let chars = location.chars().collect::<Vec<_>>();
    if chars.len() <= available {
        return format!("{prefix}{location}");
    }
    if available <= 1 {
        return prefix.chars().take(width).collect();
    }
    let tail: String = chars[chars.len() - (available - 1)..].iter().collect();
    format!("{prefix}…{tail}")
}

struct CroppedLine {
    text: String,
    marker_offset: usize,
    marker_len: usize,
}

fn crop_line(line: &str, column: usize, length: usize, available: usize) -> CroppedLine {
    let chars = line.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return CroppedLine {
            text: String::new(),
            marker_offset: 0,
            marker_len: 1,
        };
    }

    let target = column.saturating_sub(1).min(chars.len());
    let requested_len = length.max(1);
    let target_end = target
        .saturating_add(requested_len)
        .min(chars.len().max(target + 1));
    let marker_len = target_end.saturating_sub(target).max(1);

    if chars.len() <= available {
        return CroppedLine {
            text: line.to_string(),
            marker_offset: target.min(chars.len()),
            marker_len,
        };
    }

    let ellipsis_width = 1usize;
    let inner = available.saturating_sub(2 * ellipsis_width).max(1);
    let desired_left_context = inner / 3;
    let mut start = target.saturating_sub(desired_left_context);
    if start + inner > chars.len() {
        start = chars.len().saturating_sub(inner);
    }
    let end = (start + inner).min(chars.len());
    let left_ellipsis = start > 0;
    let right_ellipsis = end < chars.len();

    let mut text = String::new();
    if left_ellipsis {
        text.push('…');
    }
    text.extend(chars[start..end].iter());
    if right_ellipsis {
        text.push('…');
    }

    let marker_offset = target
        .saturating_sub(start)
        .saturating_add(usize::from(left_ellipsis));
    let visible_target_end = target_end.min(end);
    let marker_len = visible_target_end.saturating_sub(target).max(1);

    CroppedLine {
        text,
        marker_offset,
        marker_len,
    }
}

fn find_source(source_id: SourceId, sources: &[DiagnosticSource]) -> Option<&DiagnosticSource> {
    if source_id == SourceId::UNKNOWN && sources.len() == 1 {
        return sources.first();
    }
    sources.iter().find(|source| source.source_id == source_id)
}

fn render_prefixed_wrapped(
    out: &mut String,
    prefix: &str,
    message: &str,
    width: usize,
    color_code: &str,
    color: bool,
) {
    let prefix_text = format!("{prefix}: ");
    let available = width.saturating_sub(prefix_text.len()).max(1);
    let lines = wrap_words(message, available);
    for (index, line) in lines.iter().enumerate() {
        if index == 0 {
            out.push_str(&paint(prefix, color_code, true, color));
            out.push_str(": ");
        } else {
            out.push_str(&" ".repeat(prefix_text.len()));
        }
        out.push_str(line);
        out.push('\n');
    }
}

fn wrap_words(input: &str, width: usize) -> Vec<String> {
    if input.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in input.split_whitespace() {
        if current.is_empty() {
            push_word_chunks(&mut lines, &mut current, word, width);
            continue;
        }
        if current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            push_word_chunks(&mut lines, &mut current, word, width);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn push_word_chunks(lines: &mut Vec<String>, current: &mut String, word: &str, width: usize) {
    let chars = word.chars().collect::<Vec<_>>();
    if chars.len() <= width {
        current.extend(chars);
        return;
    }
    let mut start = 0;
    while start + width < chars.len() {
        lines.push(chars[start..start + width].iter().collect());
        start += width;
    }
    current.extend(chars[start..].iter());
}

fn visible_replacement(replacement: &str, replaced_length: usize) -> String {
    if replacement.is_empty() {
        return "remove this text".to_string();
    }
    if replaced_length == 0 {
        format!("insert {}", quote_visible(replacement))
    } else {
        format!("replace with {}", quote_visible(replacement))
    }
}

fn quote_visible(input: &str) -> String {
    let escaped = input
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
        .replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn paint(text: &str, code: &str, bold: bool, enabled: bool) -> String {
    if !enabled {
        return text.to_string();
    }
    if bold && code != ANSI_BOLD {
        format!("{ANSI_BOLD}{code}{text}{ANSI_RESET}")
    } else {
        format!("{code}{text}{ANSI_RESET}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{DiagnosticStage, SourceSpan};

    #[test]
    fn renderer_crops_long_lines_around_the_error() {
        let source_id = SourceId::new(7);
        let source = DiagnosticSource::new(
            source_id,
            "main.flux",
            "fn main() -> i64 { this_name_is_far_to_the_right + false }\n",
        );
        let diagnostic = Diagnostic::new(
            DiagnosticStage::Type,
            SourceSpan::new(1, 50, 5).with_source(source_id),
            "expected i64, got bool",
        );
        let rendered = render_diagnostics(
            &[diagnostic],
            &[source],
            TerminalRenderOptions {
                width: 40,
                color: false,
            },
        );
        assert!(rendered.lines().all(|line| line.chars().count() <= 40));
        assert!(rendered.contains('…'));
        assert!(rendered.contains("^^^^^"));
    }

    #[test]
    fn renderer_wraps_headers_and_locations_to_terminal_width() {
        let source_id = SourceId::new(8);
        let source = DiagnosticSource::new(
            source_id,
            "/a/very/long/project/path/that/should/not/overflow/main.flux",
            "fn main() -> i64 { false }\n",
        );
        let diagnostic = Diagnostic::new(
            DiagnosticStage::Type,
            SourceSpan::new(1, 20, 5).with_source(source_id),
            "this deliberately long diagnostic message must wrap rather than overflow the terminal",
        )
        .with_label(
            SourceSpan::new(1, 4, 4).with_source(source_id),
            "this deliberately long label also needs width-aware wrapping",
        );
        let rendered = render_diagnostics(
            &[diagnostic],
            &[source],
            TerminalRenderOptions {
                width: 24,
                color: false,
            },
        );
        assert!(rendered.lines().all(|line| line.chars().count() <= 24));
        assert!(rendered.contains("--> …"));
    }

    #[test]
    fn renderer_emits_ansi_only_when_requested() {
        let diagnostic = Diagnostic::global(DiagnosticStage::Parse, "broken syntax");
        let plain = render_diagnostics(
            std::slice::from_ref(&diagnostic),
            &[],
            TerminalRenderOptions::default(),
        );
        assert!(!plain.contains("\x1b["));
        let colored = render_diagnostics(
            &[diagnostic],
            &[],
            TerminalRenderOptions {
                width: 80,
                color: true,
            },
        );
        assert!(colored.contains("\x1b[31m"));
    }
}
