/// Library error type. Consumers can match on variants directly; the host CLI stays
/// on `eyre` (the design's API split) while this crate stays `thiserror` internally.
///
/// Grown per phase: Phase 2 (serve) adds the rmcp serve/waiting variants and the
/// log-path resolution error; Phase 3 (register/unregister/status) and Phase 5
/// (bundle) add the variants their I/O paths need.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("cannot resolve log path for {0}: neither $HOME nor $XDG_DATA_HOME is set")]
    LogPath(String),

    // Boxed: `ServerInitializeError` is ~528 bytes, which otherwise bloats the
    // whole `Error` enum (clippy `large_enum_variant`/`result_large_err`). No
    // `#[from]` because that would derive `From<Box<..>>`; the serve seam maps
    // the bare error into the box explicitly.
    #[error("mcp server initialize/transport error: {0}")]
    Serve(Box<rmcp::service::ServerInitializeError>),

    #[error("mcp serve task join error: {0}")]
    Join(#[from] tokio::task::JoinError),
}

pub type Result<T> = std::result::Result<T, Error>;
