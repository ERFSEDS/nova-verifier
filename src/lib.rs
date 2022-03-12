//! There must be at least one state that doesn't transition to so that we can serialize and
//! deserialize states. This prevents an infinite graph situation

pub mod error;
pub mod lower;
pub mod upper;

use codemap_diagnostic::Diagnostic;
pub use error::*;

/// Verifies the given toml file and converts it to a postcard binary format sutiable for the
/// rocket.
///
/// Returns `Ok((bytes, diagnostics))` on success, or `Err(diagnostics)` on failure.
pub fn verify<'session>(
    session: &'session mut Session,
    toml: String,
    file_path: String,
) -> Result<(Vec<u8>, Vec<Diagnostic>), Vec<Diagnostic>> {
    let mut all_diagnostics: Vec<Diagnostic> = Vec::new();
    let mut context = session.add_file(toml, file_path).unwrap();

    let mid = upper::verify(&toml, &mut context);
    let warnings = context.end_phase_and_emit()?;
    let mid = mid.unwrap();
    all_diagnostics.extend(warnings);

    let lower = lower::verify(mid, &mut context);
    let warnings = context.end_phase_and_emit()?;
    let lower = lower.unwrap();
    all_diagnostics.extend(warnings);

    let bytes = postcard::to_stdvec(&lower);
    let warnings = context.end_phase_and_emit()?;
    let bytes = bytes.unwrap();
    all_diagnostics.extend(warnings);

    Ok((bytes, all_diagnostics))
}
