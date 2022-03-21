use std::sync::Arc;

use codemap::CodeMap;
use codemap_diagnostic::{ColorConfig, Diagnostic, Emitter, Level, SpanLabel, SpanStyle};
use log::*;

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

    pub fn set_primary_span_no_msg(mut self, span: impl Into<Span>) -> Self {
        self.diagnostic.spans.push(SpanLabel {
            span: span.into().0,
            label: None,
            style: SpanStyle::Primary,
        });

        self
    }

    pub fn set_primary_span(mut self, span: impl Into<Span>, message: impl Into<String>) -> Self {
        let span = span.into();
        // SAFETY: Span is two u32, so there are no invalid bit patterns,
        // we need this to check if this is the "invalid span" from codemap, because trying to
        // display an invalid span will show at the top of the file, confusing the user
        let zero_span: Span = unsafe { std::mem::zeroed() };
        if span != zero_span {
            self.diagnostic.spans.push(SpanLabel {
                span: span.0,
                label: Some(message.into()),
                style: SpanStyle::Primary,
            });
        }

        self
    }

    /// Adds an addition label and span to this diagnostic
    pub fn span_label(mut self, span: impl Into<Span>, label: impl Into<String>) -> Self {
        let span = span.into();
        self.diagnostic.spans.push(SpanLabel {
            span: span.0,
            label: Some(label.into()),
            style: SpanStyle::Secondary,
        });

        self
    }

    pub fn add_span(mut self, span: impl Into<Span>) -> Self {
        let span = span.into();
        self.diagnostic.spans.push(SpanLabel {
            span: span.0,
            label: None,
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
    pub fn emit(mut self) {
        let empty = Diagnostic {
            level: Level::Bug,
            message: String::new(),
            code: None,
            spans: Vec::new(),
        };
        let diagnostic = std::mem::replace(&mut self.diagnostic, empty);
        self.context.session.add_diagnostic(diagnostic);
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
#[derive(Default, Debug)]
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

    pub fn open_file(&mut self, file_path: String) -> Result<Context<'_>, ()> {
        let data = match std::fs::read_to_string(&file_path) {
            Ok(t) => t,
            Err(e) => {
                self.diagnostics.push(Diagnostic {
                    level: Level::Error,
                    message: format!("Failed to open file `{file_path}`: {e}"),
                    code: None,
                    spans: Vec::new(),
                });
                return Err(());
            }
        };
        self.add_file(data, file_path)
    }

    pub fn add_file(&mut self, data: String, file_path: String) -> Result<Context<'_>, ()> {
        let file = self.map.add_file(file_path, data);
        let context = Context {
            session: self,
            file,
        };

        Ok(context)
    }

    #[cfg(test)]
    pub(crate) fn testing(&mut self, toml: &str) -> Context<'_> {
        let file = self.map.add_file("<anonymous>".to_owned(), toml.to_owned());
        let context = Context {
            session: self,
            file,
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

    /// Emits all diagnostics to stderr, and appends them to `to_add`
    pub fn emit_and_extend(self, to_add: &mut Vec<Diagnostic>) {
        if to_add.is_empty() {
            //Emitting an empty vec still causes newlines to be printed in `Emitter::emit()`
            return;
        }
        let mut emitter = Emitter::stderr(ColorConfig::Auto, Some(self.codemap));
        emitter.emit(&self.diagnostics);
        to_add.extend(self.diagnostics);
    }
}

pub struct Context<'session> {
    session: &'session mut Session,
    file: Arc<codemap::File>,
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

    /// Returns true if this phase contains errors
    pub fn has_error(&self) -> bool {
        for d in &self.session.diagnostics {
            if d.level == Level::Error {
                return true;
            }
        }
        false
    }

    pub fn span(&self) -> codemap::Span {
        self.file.span
    }

    pub fn source(&self) -> &str {
        self.file.source()
    }

    /// Ends the current phase, returning all diagnostics encountered in the process.
    /// If the current phase has diagnostics that are errors, Err(...) will be returned,
    /// otherwise Ok(...) will be returned contaiting errors and notes
    pub fn end_phase<'s>(&'s mut self) -> Result<Diagnostics<'s>, Diagnostics<'s>>
    where
        'session: 's,
    {
        let error = self.has_error();
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

    /// Ends the current phase, emitting all diagnostics, and returning them as a Vector.
    /// The value within the `Result` is the same, but Err(...) is used to convey that the current
    /// phase failed.
    pub fn end_phase_and_emit(&mut self) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
        let mut vec = Vec::new();
        match self.end_phase() {
            Ok(d) => {
                d.emit_and_extend(&mut vec);
                Ok(vec)
            }
            Err(d) => {
                d.emit_and_extend(&mut vec);
                Err(vec)
            }
        }
    }

    /// Returns a span representing a single character at 0 indexed `(row, col)` in the current
    /// file
    pub fn row_col_to_span(&self, row_col: (usize, usize)) -> Span {
        let line_num = row_col.0;
        let col_num = row_col.1;
        let line = self.file.line_span(line_num);
        let span = line.subspan(col_num as u64, col_num as u64 + 1);
        span.into()
    }
}

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct Span(codemap::Span);

impl Span {
    pub fn from_spanned<T>(context: &Context, span: &toml::Spanned<T>) -> Self {
        Self(
            context
                .file
                .span
                .subspan(span.start() as u64, span.end() as u64),
        )
    }
}

impl From<codemap::Span> for Span {
    fn from(span: codemap::Span) -> Self {
        Self(span)
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
    fn dropped_diagnostic_builder_panics() {
        let mut session = Session::new();
        let mut context = session.testing("");
        let _ = context.error("Test");
        // ^^^ Builder dropped here
    }

    #[test]
    fn basic1() {
        let mut session = Session::new();
        let mut context = session.testing("");
        let _ = context.error("Test").emit();
        let res = context.end_phase_and_emit();
        assert_eq!(res.unwrap_err().len(), 1);
    }
}
