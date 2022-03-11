//! There must be at least one state that doesn't transition to so that we can serialize and
//! deserialize states. This prevents an infinite graph situation

pub mod error;
pub mod lower;
pub mod upper;

pub use error::*;

pub fn verify<'session>(
    session: &'session mut Session,
    toml: String,
    file_path: String,
) -> Result<(Vec<u8>, Diagnostics<'session>), Diagnostics<'session>> {
    let mut context = session.add_file(toml, file_path).unwrap();
    let mid = upper::verify(&toml, &mut context);
    context.end_phase()?;
    let mid = mid.unwrap();

    let lower = lower::verify(mid, &mut context);
    context.end_phase()?;
    let lower = lower.unwrap();

    let bytes = postcard::to_stdvec(&lower);
    context.end_phase()?;
    let bytes = bytes.unwrap();

    Ok(bytes)
}
