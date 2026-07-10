/// Library error type. Consumers can match on variants directly; the host CLI stays
/// on `eyre` (the design's API split) while this crate stays `thiserror` internally.
///
/// Deliberately minimal in Phase 1 (scaffold only): Phase 3 (register/unregister/
/// status) and Phase 5 (bundle) add the variants their I/O paths need.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
