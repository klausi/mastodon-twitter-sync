pub use failure::{Error, ResultExt};
pub type Result<T> = ::std::result::Result<T, Error>;

use failure::bail;

// Helper function: Elefren returns errors that are not Sync. But the failure
// crate needs something Sync. We do this primitive conversion here, how could
// we do this better?
pub fn wrap_elefren_error<T>(result: std::result::Result<T, elefren::errors::Error>) -> Result<T> {
    match result {
        Ok(r) => Ok(r),
        Err(e) => bail!("Elefren error: {}", e),
    }
}
