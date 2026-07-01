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

    /// The database was migrated by a newer `note` than this binary understands
    /// (invariant 13: refuse to open a DB from the future rather than mangle it).
    #[error(
        "database schema version {found} is newer than this build supports (max {supported}); upgrade `note`"
    )]
    DbTooNew { found: i64, supported: i64 },

    /// A stored value failed to parse back into a domain type (e.g. a corrupt
    /// id string). Indicates DB corruption or an out-of-band edit.
    #[error("corrupt stored data: {0}")]
    Corrupt(String),

    /// Refused to persist a note with neither a title nor any body content
    /// (invariant 5: the rule lives in the store, not only the CLI).
    #[error("refusing to store an empty note (no title and no body)")]
    EmptyNote,

    /// The single-writer thread has gone away (panicked or shut down); no write
    /// can be serviced. Unrecoverable for the process.
    #[error("writer thread is no longer running")]
    WriterGone,
}
