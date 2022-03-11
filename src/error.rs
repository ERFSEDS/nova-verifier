use codemap::CodeMap;
use codemap_diagnostic::{ColorConfig, Diagnostic, Emitter, Level, SpanLabel, SpanStyle};

#[must_use]
pub struct DiagnosticBuilder<'s, 'c> {
    diagnostic: Diagnostic,
    context: &'c mut Context<'s>,
    cancelled: bool,
}

impl<'s, 'c> DiagnosticBuilder<'s, 'c> {
    /// For internal use only, creates a new DiagnosticBuilder. For clients, the struct_* methods
    /// on a Session or Handler should be used instead.
    pub(crate) fn new(
        level: Level,
        message: impl Into<String>,
        context: &'c mut Context<'s>,
    ) -> Self {
        let diagnostic = Diagnostic {
            level,
            code: None,
            message: message.into(),
            spans: Vec::new(),
        };

        Self {
            diagnostic,
            cancelled: false,
            context,
        }
    }

    pub fn set_primary_span<T: Into<String>>(
        &mut self,
        span: impl Into<Span>,
        message: Option<T>,
    ) -> &mut Self {
        let span = span.into();
        self.diagnostic.spans.push(SpanLabel {
            span: self
                .context
                .span
                .subspan(span.start.into(), span.end.into()),
            label: message.map(|m| m.into()),
            style: SpanStyle::Primary,
        });

        self
    }

    /// Adds an addition label and span to this diagnostic
    pub fn span_label(&mut self, span: impl Into<Span>, label: impl Into<String>) -> &mut Self {
        self.diagnostic.spans.push(SpanLabel {
            span: self
                .context
                .span
                .subspan(span.start.into(), span.end.into()),
            label: Some(label.into()),
            style: SpanStyle::Secondary,
        });

        self
    }

    /*
    /// Adds a note message to the diagnostic
    pub fn note(&mut self, message: impl Into<String>) -> &mut Self {
        let subd = SubDiagnostic::new(Level::Note, message.into(), None);
        self.diagnostic.children.push(subd);

        self
    }

    /// Adds a note message with a separate span to the diagnostic
    pub fn span_note(&mut self, span: Span, message: impl Into<String>) -> &mut Self {
        let subd = SubDiagnostic::new(Level::Note, message.into(), Some(span));
        self.diagnostic.children.push(subd);

        self
    }

    /// Adds a help message to the diagnostic
    pub fn help(&mut self, message: impl Into<String>) -> &mut Self {
        let subd = SubDiagnostic::new(Level::Help, message.into(), None);
        self.diagnostic.children.push(subd);

        self
    }

    /// Adds a help message with a separate span to the diagnostic
    pub fn span_help(&mut self, span: Span, message: impl Into<String>) -> &mut Self {
        let subd = SubDiagnostic::new(Level::Help, message.into(), Some(span));
        self.diagnostic.children.push(subd);

        self
    }
    */

    /// Emits this diagnostic to the current session, consuming it
    pub fn emit(self) {
        self.context.session.add_diagnostic(self.diagnostic);
        self.cancel();
    }

    /// Sets this DiagnosticBuilder as cancelled, meaning that it is safe to be dropped
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// Returns true if this was cancelled, false otherwise
    pub fn cancelled(&self) -> bool {
        self.cancelled
    }
}

impl<'s, 'c> Drop for DiagnosticBuilder<'s, 'c> {
    fn drop(&mut self) {
        if !self.cancelled {
            panic!("Internal compiler bug. DiagnosticBuilder not emitted!");
        }
    }
}

/// The top level helper struct for opening and verifing toml files.
///
/// First, open a file with [`Self::open_file`], any errors generated when opening the file will be
/// saved into this session, and will be available when calling [`Self::end_phase`].
///
/// Once a file is open, phase processing can begin.
/// Verification/compilation works as usual, with the calling code doing as much work as possible
/// on a bad config file to give the most amount of diagnostics to the user.
/// At the end of each logical phase, call [`Self::end_phase`] to get the list of errors emitted
/// during that phase. Normal implementations should stop proceding through phases as soon as a
/// phase completes with errors.
pub struct Session {
    map: codemap::CodeMap,
    diagnostics: Vec<Diagnostic>,
}

impl Session {
    pub fn new() -> Self {
        Self {
            map: codemap::CodeMap::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn open_file<'s>(&'s mut self, file_path: String) -> Result<Context<'s>, ()> {
        let data = match std::fs::read_to_string(file_path) {
            Ok(t) => t,
            Err(v) => {
                self.diagnostics.push(Diagnostic {
                    level: Level::Error,
                    message: format!("Failed to open file `{file_path}`"),
                    code: None,
                    spans: Vec::new(),
                });
                return Err(());
            }
        };
        self.add_file(data, file_path)
    }

    pub fn add_file<'s>(&'s mut self, data: String, file_path: String) -> Result<Context<'s>, ()> {
        let file = self.map.add_file(file_path, data);
        let context = Context {
            session: self,
            span: file.span,
        };

        Ok(context)
    }

    pub(crate) fn testing<'s>(&'s mut self, toml: &str) -> Context<'s> {
        let file = self.map.add_file("<anonymous>".to_owned(), toml.to_owned());
        let context = Context {
            session: self,
            span: file.span,
        };

        context
    }

    /// Adds a diagnostic to this session.
    /// Most users should perfer the high level interface via [`DiagnosticBuilder`]
    pub fn add_diagnostic(&mut self, diagnostic: impl Into<Diagnostic>) {
        self.diagnostics.push(diagnostic.into());
    }
}

pub struct Diagnostics<'c> {
    diagnostics: Vec<Diagnostic>,
    codemap: &'c CodeMap,
}

impl<'c> Diagnostics<'c> {
    /// Emits all diagnostics to stderr
    pub fn emit(self) {
        let mut emitter = Emitter::stderr(ColorConfig::Auto, Some(self.codemap));
        emitter.emit(&self.diagnostics);
    }
}

pub struct Context<'session> {
    session: &'session mut Session,
    span: codemap::Span,
}

impl<'session> Context<'session> {
    pub fn error<'c>(&'c mut self, message: impl Into<String>) -> DiagnosticBuilder<'session, 'c> {
        DiagnosticBuilder::new(Level::Error, message.into(), self)
    }

    pub fn warn<'c>(&'c mut self, message: impl Into<String>) -> DiagnosticBuilder<'session, 'c> {
        DiagnosticBuilder::new(Level::Warning, message.into(), self)
    }

    pub fn note<'c>(&'c mut self, message: impl Into<String>) -> DiagnosticBuilder<'session, 'c> {
        DiagnosticBuilder::new(Level::Note, message.into(), self)
    }

    pub fn help<'c>(&'c mut self, message: impl Into<String>) -> DiagnosticBuilder<'session, 'c> {
        DiagnosticBuilder::new(Level::Help, message.into(), self)
    }

    /// Ends the current phase, returning all diagnostics encountered in the process.
    /// If the current phase has diagnostics that are errors, Err(...) will be returned,
    /// otherwise Ok(...) will be returned contaiting errors and notes
    pub fn end_phase<'s>(&'s mut self) -> Result<Diagnostics<'s>, Diagnostics<'s>>
    where
        'session: 's,
    {
        let mut error = false;
        for d in self.session.diagnostics {
            if d.level == Level::Error {
                error = true;
                break;
            }
        }
        let result = Diagnostics {
            diagnostics: std::mem::take(&mut self.session.diagnostics),
            codemap: &self.session.map,
        };
        if error {
            Err(result)
        } else {
            Ok(result)
        }
    }
}

pub struct Span {
    start: u32,
    end: u32,
}
const EMPTY_SPAN: Span = Span::new(0, 0);

impl Span {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }
}

impl<T> From<&toml::Spanned<T>> for Span {
    fn from(span: &toml::Spanned<T>) -> Self {
        Self::new(span.start() as u32, span.end() as u32)
    }
}

impl<T> From<toml::Spanned<T>> for Span {
    fn from(span: toml::Spanned<T>) -> Self {
        Self::new(span.start() as u32, span.end() as u32)
    }
}
/*
impl From<(usize, usize)> for Span {
    fn from(span: (usize, usize)) -> Self {
        Self::new(span.0, span.1)

    }
}
*/

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic]
    fn drop_context() {
        let manager = SourceManager::new("".to_owned());
        let _ = manager.new_context();
    }

    #[test]
    fn basic1() {
        let manager = SourceManager::new("".to_owned());
        let mut context = manager.new_context();
        let _ = context.emitt_span((0, 0), Error::NoStates);
        let errors = context.finish().unwrap_err();
        assert_eq!(errors.errors().len(), 1);
    }
}
