//! There must be at least one state that doesn't transition to so that we can serialize and
//! deserialize states. This prevents an infinite graph situation

pub mod lower;
pub mod upper;
pub mod error;
pub use error::*;

pub fn verify(manager: &SourceManager) -> Result<Vec<u8>, MutipleErrors> {

    let context = manager.new_context();
    let src = manager.source();
    let mid = upper::verify(src, &mut context);
    context.finish()?;
    let mid = mid.unwrap();

    let context = manager.new_context();
    let lower = lower::verify(mid, &mut context);
    context.finish()?;
    let lower = lower.unwrap();

    let context = manager.new_context();
    let res = postcard::to_stdvec(&lower);
    let bytes = context.check(res);
    context.finish()?;

    Ok(bytes.unwrap())
}
