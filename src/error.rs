use codemap::CodeMap;
use codemap_diagnostic::{ColorConfig, Diagnostic, Emitter, Level, SpanLabel};

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

    pub fn set_primary_span(&mut self, span: Span) -> &mut Self {
        self.diagnostic.spans.push(SpanLabel { span, label: (), style: () }

        self
    }

    pub fn span_label(&mut self, span: Span, label: impl Into<String>) -> &mut Self {
        self.diagnostic.spans.push(SpanLabel {
            span,
            label: label.into(),
            style: SpanStyle::A,
        });

        self
    }

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

    /// Sets this DiagnosticBuilder as cancelled, meaning that it is safe to be dropped
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// Returns true if this was cancelled, false otherwise
    pub fn cancelled(&self) -> bool {
        self.cancelled
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

    pub fn open_file<'s>(&'s mut self, path: String) -> Result<Context<'s>, ()> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(v) => {
                self.diagnostics.push(Diagnostic {
                    level: Level::Error,
                    message: format!("Failed to open file `{path}`"),
                    code: None,
                    spans: Vec::new(),
                });
                return Err(());
            }
        };
        let file = self.map.add_file(path, text);
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

    /// Ends the current phase, returning all diagnostics encountered in the process.
    /// If the current phase has diagnostics that are errors, Err(...) will be returned,
    /// otherwise Ok(...) will be returned contaiting errors and notes
    pub fn end_phase<'c>(&'c mut self) -> Result<Diagnostics<'c>, Diagnostics<'c>> {
        let mut error = false;
        for d in self.diagnostics {
            if d.level == Level::Error {
                error = true;
                break;
            }
        }
        let result = Diagnostics {
            diagnostics: std::mem::take(&mut self.diagnostics),
            codemap: &self.map,
        };
        if error {
            Err(result)
        } else {
            Ok(result)
        }
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

pub struct Context<'s> {
    session: &'s mut Session,
    span: codemap::Span,
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

/// Manages emission of error for a single toml source file.
/// NOTE: [`Self::finish`] must be called before `Self` is dropped, otherwise
pub struct Context {
    errors: MutipleErrors,
    manager: Arc<SourceManager>,
}

impl Context {
    fn emitt_span_severity(
        &mut self,
        span: impl Into<Span>,
        err: impl Into<Error>,
        severity: ErrorSeverity,
    ) -> Result<(), ()> {
        let err = err.into();
        let span = span.into();
        let single = OuterError {
            inner: err,
            span: span.into(),
            manager: Arc::clone(&self.manager),
        };
        self.errors.inner.push(single);
        match severity {
            ErrorSeverity::Recoverable => Ok(()),
            ErrorSeverity::Fatal => Err(()),
        }
    }

    /// Emitts a span with a given error.
    ///
    /// TODO: Fix docs
    /// Callers should always use the question mark on the result.
    /// This is because if `err` is a critical error that forces verification to stop,
    /// this function will return an Err(()).
    ///
    /// Otherwise, `err` is added to the internal error list and will be emitted when
    /// [`Self::finish`] is called.
    pub fn emitt_span(&mut self, span: impl Into<Span>, err: impl Into<Error>) -> Result<(), ()> {
        self.emitt_span_severity(span, err, ErrorSeverity::Recoverable)
    }

    /// Emitts a fatal error
    pub fn emitt_span_fatal<T>(
        &mut self,
        span: impl Into<Span>,
        err: impl Into<Error>,
    ) -> Result<T, ()> {
        self.emitt_span_severity(span, err, ErrorSeverity::Fatal)
            .map(|_| unreachable!())
    }

    /// Emitts the error with no span information
    ///
    /// See [`Self::emitt_span`]
    pub fn emitt(&mut self, err: impl Into<Error>) -> Result<(), ()> {
        self.emitt_span(EMPTY_SPAN, err)
    }

    /// Cleans up this context and prepares for displaying error.
    ///
    /// If any errors have been emitted, returns them inside the Err(...) variant.
    /// Otherwise Ok(())
    /// is returned
    pub fn finish(mut self) -> Result<(), MutipleErrors> {
        let result = if !self.errors.inner.is_empty() {
            // Return real errors so that we forget an empty `MutipleErrors` later
            let mut errors = self.errors.take();
            for e in errors.errors_mut() {}
            Err(errors)
        } else {
            Ok(())
        };
        // Prevent triggering debugging panic
        std::mem::forget(self);
        result
    }

    /// Checks that a result is not an error. If `res` is an error, it will be emmitted with no
    /// span or source information
    pub fn check<T, E: Into<Error>>(
        &mut self,
        res: std::result::Result<T, E>,
    ) -> std::result::Result<T, ()> {
        res.map_err(|err| {
            self.errors.inner.push(OuterError::new(
                err.into(),
                (0, 0).into(),
                Arc::clone(&self.manager),
            ));
            ()
        })
    }

    pub fn unreachable(&self) -> ! {
        let s = self.errors.to_string();
        eprintln!("{}", s);
        panic!("FATAL ERROR: Verifier entered unreachable code!");
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        panic!("Context dropped without calling finish!");
    }
}

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
