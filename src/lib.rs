//! There must be at least one state that doesn't transition to so that we can serialize and
//! deserialize states. This prevents an infinite graph situation

pub mod lower;
pub mod upper;
pub mod error;
pub use error::*;

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

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
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
}

pub fn verify(toml: &str) -> Result<Vec<u8>, Error> {
    let mid = upper::verify(toml)?;
    let lower = lower::verify(mid)?;
    Ok(postcard::to_stdvec(&lower)?)
}
