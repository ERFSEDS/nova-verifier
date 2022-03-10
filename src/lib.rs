//! There must be at least one state that doesn't transition to so that we can serialize and
//! deserialize states. This prevents an infinite graph situation

pub mod lower;
pub mod upper;

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::{iter::IterNextOutput, prelude::*, PyIterProtocol};

create_exception!(verifier, TomlParseException, PyException);
create_exception!(verifier, VerifyException, PyException);

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

fn verify_inner(toml: &str) -> Result<Vec<u8>, Error> {
    let mid = upper::verify(toml)?;
    let lower = lower::verify(mid)?;
    Ok(postcard::to_stdvec(&lower)?)
}

#[pyfunction]
fn verify(toml: String) -> PyResult<Vec<u8>> {
    let result = verify_inner(toml.as_str());
    result.map_err(|e| TomlParseException::new_err(format!("{}", e)))
}

#[pymodule]
fn verifier(py: Python, m: &PyModule) -> PyResult<()> {
    m.add("TomlParseException", py.get_type::<TomlParseException>())?;

    m.add("VerifyException", py.get_type::<VerifyException>())?;

    m.add_function(wrap_pyfunction!(verify, m)?)?;

    Ok(())
}
