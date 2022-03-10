use std::path::{Path, PathBuf};
use std::rc::Arc;

enum ErrorSeverity {
    /// An unrecoverable error. Unrecoverable errors immediately bubble up the call stack
    Fatal,

    /// An error will eventually be emitted, but we should try to continue to give the user more
    /// diagnostics
    Recoverable,
}


pub struct DiagnosticBuilder<'a> {
    diagnostic: Diagnostic,
    handler: &'a Handler,
}

impl<'a> DiagnosticBuilder<'a> {
    /// For internal use only, creates a new DiagnosticBuilder. For clients, the struct_* methods
    /// on a Session or Handler should be used instead.
    pub(crate) fn new(handler: &'a Handler, level: Level, message: impl Into<String>) -> Self {
        let diagnostic = Diagnostic {
            level,
            message: message.into(),
            primary: None,
            spans: Vec::new(),
            children: Vec::new(),
        };

        Self {
            diagnostic,
            handler,
        }
    }

    pub fn set_primary_span(&mut self, span: Span) -> &mut Self {
        self.diagnostic.primary = Some(span);

        self
    }

    pub fn span_label(&mut self, span: Span, label: impl Into<String>) -> &mut Self {
        self.diagnostic.spans.push((span, label.into()));

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

    /// Queues this diagnostic to be emitted by the inner Handler/Emitter
    pub fn emit(&mut self) {
        if self.diagnostic.level == Level::Warning {
            self.handler.warn(self.diagnostic.clone());
        } else {
            self.handler.error(self.diagnostic.clone());
        }

        // Mark this as cancelled so that it can be safely dropped
        self.cancel();
    }

    /// Sets this DiagnosticBuilder as cancelled, meaning that it is safe to be dropped
    pub fn cancel(&mut self) {
        self.diagnostic.level = Level::Cancelled;
    }

    /// Returns true if this was cancelled, false otherwise
    pub fn cancelled(&self) -> bool {
        self.diagnostic.level == Level::Cancelled
    }
}



#[derive(Debug, PartialEq, Eq)]
pub enum StateCountError {
    NoStates,
    TooManyStates(usize),
}

#[derive(Debug, PartialEq, Eq)]
pub enum CheckError {
    NoCondition,
    TooManyConditions(Vec<SourceSpan>),
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommandError {
    NoValues,
    TooManyValues(Vec<SourceSpan>),
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Toml(#[from] toml::de::Error),

    #[error("postcard error: {0}")]
    Postcard(#[from] postcard::Error),

    #[error("state {0} not found")]
    StateNotFound(String),

    #[error("wrong number of states: {0:?}")]
    StateCount(StateCountError),

    #[error("command has wrong number of arguments: {0:?}")]
    Command(CommandError),

    #[error("check condition wrong: {0:?}")]
    CheckConditionError(CheckError),

    #[error("no states declared\nconfig files require at least one state")]
    NoStates,

    #[error("{0:?}")]
    IO(#[from] std::io::Error),

    #[error("{0:?}")]
    Custom(String),
}

impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        use Error::*;
        match (self, other) {
            (Toml(a), Toml(b)) => a.eq(b),
            (Postcard(a), Postcard(b)) => a.eq(b),
            (StateNotFound(a), StateNotFound(b)) => a.eq(b),
            (StateCount(a), StateCount(b)) => a.eq(b),
            (Command(a), Command(b)) => a.eq(b),
            (CheckConditionError(a), CheckConditionError(b)) => a.eq(b),
            (NoStates, NoStates) => true,
            (IO(_), IO(_)) => false, // TODO: Find a better way to do this. std::io::Error doesnt support eq
            (Custom(a), Custom(b)) => a.eq(b),
            _ => false,
        }
    }
}

#[derive(Diagnostic, Debug, thiserror::Error)]
#[error("oops")]
#[diagnostic()]
pub struct OuterError {
    inner: Error,

    #[label = "This is the highlight"]
    span: SourceSpan,

    manager: Arc<SourceManager>,
}

impl OuterError {
    pub fn new(inner: Error, span: SourceSpan, manager: Arc<SourceManager>) -> Self {
        Self {
            inner,
            span,
            manager,
        }
    }

    pub fn inner(&self) -> &Error {
        &self.inner
    }
}

#[derive(Diagnostic, Debug, thiserror::Error)]
#[error("oops2")]
pub struct MutipleErrors {
    #[related]
    inner: Vec<OuterError>,
}

impl MutipleErrors {
    pub(crate) fn new() -> Self {
        Self { inner: Vec::new() }
    }

    /// Creates a mutiple error collection from a single error with on span
    pub(crate) fn from_single(error: Error, manager: Arc<SourceManager>) -> Self {
        Self {
            inner: vec![OuterError::new(error, EMPTY_SPAN.into(), manager)],
        }
    }

    pub(crate) fn take(&mut self) -> Self {
        let inner = std::mem::take(&mut self.inner);
        Self { inner }
    }

    pub fn errors(&self) -> &[OuterError] {
        &self.inner
    }

    pub(crate) fn errors_mut(&mut self) -> &mut [OuterError] {
        &mut self.inner
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
    inner: codemap::CodeMap,
}

impl Session {
    pub fn open_file<'s>(&'s mut self, path: String) -> Result<Context<'s>, ()> {

    }
}

pub struct Context<'s> {
    session: &'s mut Session,
    span: codemap::Span,
}

/// Manages a single source file
#[derive(Debug)]
pub struct SourceManager {
}

impl SourceManager {
    pub fn new(src: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            file: "<unknown>".into(),
            src: src.into(),
        })
    }

    /// Opens a source manager for the given path
    pub fn open(path: impl AsRef<Path>) -> Result<Arc<Self>, Error> {
        let file: PathBuf = path.as_ref().to_owned();
        let src = std::fs::read_to_string(&file)?;

        Ok(Arc::new(Self { file, src }))
    }

    pub fn new_context(self: &Arc<Self>) -> Context {
        Context {
            errors: MutipleErrors::new(),
            manager: Arc::clone(self),
        }
    }

    pub fn source(&self) -> &str {
        self.src.as_str()
    }
}

pub struct Span {
    start: usize,
    len: usize,
}

const EMPTY_SPAN: Span = Span::new(0, 0);

impl Span {
    pub const fn new(start: usize, len: usize) -> Self {
        Self { start, len }
    }
}

impl<T> From<&toml::Spanned<T>> for Span {
    fn from(span: &toml::Spanned<T>) -> Self {
        Self {
            start: span.start(),
            len: span.end() - span.start(),
        }
    }
}

impl<T> From<toml::Spanned<T>> for Span {
    fn from(span: toml::Spanned<T>) -> Self {
        Self {
            start: span.start(),
            len: span.end() - span.start(),
        }
    }
}

impl From<(usize, usize)> for Span {
    fn from(span: (usize, usize)) -> Self {
        Self {
            start: span.0,
            len: span.1,
        }
    }
}

impl From<Span> for SourceSpan {
    fn from(span: Span) -> Self {
        SourceSpan::new(span.start.into(), span.len.into())
    }
}

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
            for e in errors.errors_mut() {
            }
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
