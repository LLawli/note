use thiserror::Error;

/// Result alias for store operations.
pub type Result<T, E = StoreError> = core::result::Result<T, E>;

/// Anything that can go wrong talking to the SQLite store.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("migration error: {0}")]
    Migration(#[from] refinery::Error),

    /// A stored value failed to parse back into a domain type (e.g. a corrupt
    /// id string). Indicates DB corruption or an out-of-band edit.
    #[error("corrupt stored data: {0}")]
    Corrupt(String),

    /// The single-writer thread has gone away (panicked or shut down); no write
    /// can be serviced. Unrecoverable for the process.
    #[error("writer thread is no longer running")]
    WriterGone,
}
