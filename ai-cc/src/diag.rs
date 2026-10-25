use std::fmt;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Option<Span>,
}
impl Diagnostic {
    pub fn error<T: Into<String>>(message: T, span: Option<Span>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            span,
        }
    }
    pub fn warning<T: Into<String>>(message: T, span: Option<Span>) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            span,
        }
    }
}
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}",
            match self.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            },
            self.message
        )
    }
}
pub struct SourceMap {
    pub file_name: String,
    pub source: String,
}
impl SourceMap {
    pub fn new(file_name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            file_name: file_name.into(),
            source: source.into(),
        }
    }
    pub fn line_col(&self, at: usize) -> (usize, usize) {
        let mut end = at.min(self.source.len());
        while end > 0 && !self.source.is_char_boundary(end) {
            end -= 1;
        }
        let s = &self.source[..end];
        (
            s.bytes().filter(|b| *b == b'\n').count() + 1,
            s.rsplit('\n').next().unwrap_or("").chars().count() + 1,
        )
    }
}
pub trait DiagnosticEmitter {
    fn emit(&mut self, diagnostic: Diagnostic);
    fn has_errors(&self) -> bool;
    fn take_diagnostics(&mut self) -> Vec<Diagnostic>;
}
pub struct DiagnosticFormatter;
impl DiagnosticFormatter {
    pub fn format(diagnostic: &Diagnostic, source_map: &SourceMap) -> String {
        if let Some(s) = diagnostic.span {
            let (l, c) = source_map.line_col(s.start);
            let line_start = source_map.source[..s.start.min(source_map.source.len())]
                .rfind('\n')
                .map_or(0, |x| x + 1);
            let line_end = source_map.source[s.start.min(source_map.source.len())..]
                .find('\n')
                .map_or(source_map.source.len(), |x| s.start + x);
            let text = &source_map.source[line_start..line_end];
            let width = s.end.saturating_sub(s.start).max(1);
            format!(
                "{}:{}:{}: {}\n {:>4} | {}\n      | {}{}",
                source_map.file_name,
                l,
                c,
                diagnostic,
                l,
                text,
                " ".repeat(c.saturating_sub(1)),
                "^".repeat(width)
            )
        } else {
            format!("{}: {}", source_map.file_name, diagnostic)
        }
    }
}
