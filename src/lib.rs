#![feature(never_type)]
// In many places errors are emitted to a context, so we return `Result<_, ()>`. This is fine
#![allow(clippy::result_unit_err)]

pub mod error;
pub mod lower;
pub mod upper;

use codemap_diagnostic::{Diagnostic, Level};
pub use error::*;
use log::*;

/// Verifies the given toml file and converts it to a postcard binary format sutiable for the
/// rocket.
///
/// Returns `Ok((bytes, diagnostics))` on success, or `Err(diagnostics)` on failure.
pub fn verify_inner(
    session: &mut Session,
    toml: String,
    file_path: String,
) -> Result<(Vec<u8>, Vec<Diagnostic>), Vec<Diagnostic>> {
    let mut all_diagnostics: Vec<Diagnostic> = Vec::new();
    let mut context = session.add_file(toml, file_path).unwrap();

    let mid = upper::verify(&mut context);
    let warnings = context.end_phase_and_emit()?;
    let mid = mid.unwrap();
    all_diagnostics.extend(warnings);
    trace!("Upper verify: {mid:#?}");

    //let s = toml::to_string(&mid).unwrap();
    //trace!("What toml would be: {s}");

    let lower = lower::verify(mid, &mut context);
    let warnings = context.end_phase_and_emit()?;
    let lower = lower.unwrap();
    all_diagnostics.extend(warnings);
    trace!("Lower verify: {lower:#?}");

    let bytes = postcard::to_stdvec(&lower);
    let warnings = context.end_phase_and_emit()?;
    let bytes = bytes.unwrap();
    all_diagnostics.extend(warnings);
    trace!("Postcard message is {} bytes", bytes.len());

    Ok((bytes, all_diagnostics))
}

/// Loads a toml file at the given path, verifies it, and writes the encoded contents to
/// `dst_path`, returning the diagnostics that the transformation produced.
///
/// Returns `Err(...)` if any step fails without writing to `dst_path`. If Ok(...) is returned
/// then the encoded config file has been written to `dst_path`, and all notes, warnings and helps
/// encountered while converting will be placed in the returned Vector.
pub fn verify_file(src_path: String, dst_path: String) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    let mut session = Session::new();
    let toml = match std::fs::read_to_string(&src_path) {
        Ok(t) => t,
        Err(err) => {
            return Err(vec![Diagnostic {
                level: Level::Error,
                message: format!("failed to read file `{src_path}`: {err:?}"),
                code: None,
                spans: vec![],
            }]);
        }
    };

    let (bytes, mut diags) = verify_inner(&mut session, toml, src_path)?;
    if let Err(err) = std::fs::write(&dst_path, bytes) {
        diags.push(Diagnostic {
            level: Level::Error,
            message: format!("failed to write to file `{dst_path}`: {err:?}"),
            code: None,
            spans: vec![],
        });
        return Err(diags);
    }
    Ok(diags)
}
