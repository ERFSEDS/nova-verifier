use std::path::{Path, PathBuf};

use miette::{Diagnostic, SourceSpan};

enum ErrorSeverity {
    /// An unrecoverable error. Unrecoverable errors immediately bubble up the call stack
    Fatal,

    /// An error will eventually be emitted, but we should try to continue to give the user more
    /// diagnostics
    Recoverable,
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
            (IO(a), IO(b)) => false,// TODO: Find a better way to do this. std::io::Error doesnt support eq
            (Custom(a), Custom(b)) => a.eq(b),
        }
    }
}

#[derive(Diagnostic, Debug, thiserror::Error)]
#[error("oops")]
#[diagnostic()]
pub struct OuterError<'s> {
    inner: Error,

    #[label = "This is the highlight"]
    span: SourceSpan,

    #[source_code]
    src: &'s str,
}

impl<'s> OuterError<'s> {
    pub fn new(inner: Error, span: SourceSpan, src: &'s str) -> Self {
        Self { inner, span, src }
    }

    pub fn inner(&self) -> &Error {
        &self.inner
    }
}

#[derive(Diagnostic, Debug, thiserror::Error)]
#[error("oops2")]
pub struct MutipleErrors<'s> {
    #[related]
    inner: Vec<OuterError<'s>>,
}

impl<'s> MutipleErrors<'s> {
    pub(crate) fn new() -> Self {
        Self { inner: Vec::new() }
    }

    pub(crate) fn take(&mut self) -> Self {
        let inner = std::mem::take(&mut self.inner);
        Self { inner }
    }

    pub fn errors(&self) -> &[OuterError<'s>] {
        &self.inner
    }
}

/// Manages a single source file
pub struct SourceManager {
    file: PathBuf,
    src: String,
}

impl SourceManager {
    pub fn new(src: impl Into<String>) -> Self {
        Self {
            file: "<unknown>".into(),
            src: src.into(),
        }
    }

    /// Opens a source manager for the given path
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let file: PathBuf = path.as_ref().to_owned();
        let src = std::fs::read_to_string(&file)?;

        Ok(Self { file, src })
    }

    pub fn new_context<'s>(&'s self) -> Context<'s> {
        Context {
            errors: MutipleErrors::new(),
            manager: self,
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
pub struct Context<'s> {
    errors: MutipleErrors<'s>,
    manager: &'s SourceManager,
}

impl<'s> Context<'s> {

    fn emitt_span_severity(
        &self,
        span: impl Into<Span>,
        err: impl Into<Error>,
        severity: ErrorSeverity,
    ) -> Result<(), ()> {
        let err = err.into();
        let span = span.into();
        let single = OuterError {
            inner: err,
            span: span.into(),
            src: self.manager.source(),
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
    pub fn emitt_span(
        &self,
        span: impl Into<Span>,
        err: impl Into<Error>,
    ) -> Result<(), ()> {
        self.emitt_span_severity(span, err, ErrorSeverity::Recoverable)
    }

    /// Emitts a fatal error
    pub fn emitt_span_fatal<T>(
        &self,
        span: impl Into<Span>,
        err: impl Into<Error>,
    ) -> Result<T, ()> {
        self.emitt_span_severity(span, err, ErrorSeverity::Fatal)
            .map(|_| unreachable!())
    }

    /// Emitts the error with no span information
    ///
    /// See [`Self::emitt_span`]
    pub fn emitt(&self, span: impl Into<Span>, err: impl Into<Error>) -> Result<(), ()> {
        self.emitt_span(EMPTY_SPAN, err)
    }

    /// Cleans up this context and prepares for displaying error.
    ///
    /// If any errors have been emitted, returns them inside the Err(...) variant.
    /// Otherwise Ok(())
    /// is returned
    pub fn finish(mut self) -> Result<(), MutipleErrors<'s>> {
        let result = if !self.errors.inner.is_empty() {
            // Return real errors so that we forget an empty `MutipleErrors` later
            Err(self.errors.take())
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
            self.errors
                .inner
                .push(OuterError::new(err.into(), (0, 0).into(), ""));
            ()
        })
    }

    pub fn unreachable(&self) -> ! {
        let s = self.errors.to_string();
        eprintln!("{}", s);
        panic!("FATAL ERROR: Verifier entered unreachable code!");
    }
}

impl Drop for Context<'_> {
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
        let context = manager.new_context();
        context.emitt_span((0, 0), Error::NoStates);
    }
}
