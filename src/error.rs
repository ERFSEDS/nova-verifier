use std::path::{Path, PathBuf};

use miette::{Diagnostic, SourceSpan};

type Result<'s, T> = std::result::Result<T, MutipleErrors<'s>>;

#[derive(Debug, PartialEq, Eq)]
pub enum CommandError {
    NoValues,
    TooManyValues(usize),
}

#[derive(Debug, PartialEq, Eq)]
pub enum StateCountError {
    NoStates,
    TooManyStates(usize),
}

#[derive(Debug, PartialEq, Eq)]
pub enum CheckConditionError {
    NoCondition,
    TooManyConditions(usize),
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("toml parse error {0}")]
    Toml(#[from] toml::de::Error),

    #[error("postcard error {0}")]
    Postcard(#[from] postcard::Error),

    #[error("state {0} not found")]
    StateNotFound(String),

    #[error("wrong number of states: {0:?}")]
    StateCount(StateCountError),

    #[error("command has wrong number of arguments: {0:?}")]
    Command(CommandError),

    #[error("check condition wrong: {0:?}")]
    CheckConditionError(CheckConditionError),

    #[error("no states declared\nconfig files require at least one state")]
    NoStates,

    #[error("{0:?}")]
    IO(#[from] std::io::Error),
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
}

/// Manages a single source file
pub struct SourceManager {
    file: PathBuf,
    src: String,
}

impl SourceManager {
    /// Opens a source manager for the given path
    pub fn open(path: impl AsRef<Path>) -> std::result::Result<Self, Error> {
        let file: PathBuf = path.as_ref().to_owned();
        let src = std::fs::read_to_string(&file)?;

        Ok(Self { file, src })
    }

    pub fn new_context<'s>(&'s self) -> Context<'s> {
        Context {
            src: &self.src,
            errors: MutipleErrors::new(),
        }
    }
}

/// Manages emission of error for a single toml source file.
/// NOTE: [`Self::finish`] must be called before `Self` is dropped, otherwise
pub struct Context<'s> {
    src: &'s str,
    errors: MutipleErrors<'s>,
}

impl<'s> Context<'s> {
    /// Emitts a span with a given error.
    ///
    /// Callers should always use the question mark here.
    /// This is because if `err` is a critical error that forces verification to stop,
    /// this function will return an Err(...) containing `err` along with the other errors emitted
    /// up to this point.
    /// Otherwise, `err` is added to the internal error list and will be emitted when
    /// [`Self::finish`] is called.
    pub fn emitt_span<T>(&self, span: toml::Spanned<T>, err: impl Into<Error>) -> Result<()> {
        Ok(())
    }

    /// Cleans up this context and prepares for displaying error.
    ///
    /// If any errors have been emitted, returns them inside the Err(...) variant.
    /// Otherwise Ok(())
    /// is returned
    pub fn finish(mut self) -> Result<'s, ()> {
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
}

impl Drop for Context<'_> {
    fn drop(&mut self) {
        panic!("Context dropped without calling finish!");
    }
}
