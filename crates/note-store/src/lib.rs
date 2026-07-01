//! note-store: the SQLite persistence layer for `note`.
//!
//! One file, one writer, one read pool. The single-writer actor ([`writer`])
//! owns the read-write connection on a dedicated thread; reads go through a
//! separate [`ReaderPool`]. FTS5, tags and links are kept in lockstep with the
//! `notes` table inside each write transaction (never indexed after the fact).

mod error;
mod mint;
mod model;
mod reader;
mod writer;

pub use error::{Result, StoreError};
pub use model::{ImportNote, ImportOutcome, Link, NewNote, NotePatch};
pub use reader::ReaderPool;
pub use writer::WriterHandle;

use rusqlite::Connection;
use std::path::Path;
use std::thread::JoinHandle;
use std::time::Duration;

mod embedded {
    refinery::embed_migrations!("migrations");
}

/// A handle to an open note database: the single writer plus the read pool.
/// Dropping the store closes the writer's command channel and joins its thread.
#[derive(Debug)]
pub struct Store {
    writer: Option<WriterHandle>,
    readers: ReaderPool,
    writer_thread: Option<JoinHandle<()>>,
}

impl Store {
    /// Open (creating if needed) the database at `path`, applying migrations and
    /// enabling WAL on the writer connection before serving any request.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let conn = init_writer_conn(path)?;
        let (writer, thread) = WriterHandle::spawn(conn).map_err(|_| StoreError::WriterGone)?;
        Ok(Self {
            writer: Some(writer),
            readers: ReaderPool::new(path),
            writer_thread: Some(thread),
        })
    }

    /// The single-writer handle for all mutations.
    #[must_use]
    pub fn writer(&self) -> &WriterHandle {
        self.writer
            .as_ref()
            .expect("writer handle present until drop")
    }

    /// The read pool for all queries.
    #[must_use]
    pub fn readers(&self) -> &ReaderPool {
        &self.readers
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        // Drop the only command sender so the writer loop ends, then join the
        // thread so it is reaped (a writer-thread panic is already printed by the
        // default panic hook; joining avoids leaking the thread).
        drop(self.writer.take());
        if let Some(handle) = self.writer_thread.take() {
            let _ = handle.join();
        }
    }
}

/// Highest migration version this binary embeds. Bump when adding a migration
/// (a `tests/` guard asserts it matches what refinery has embedded).
const EMBEDDED_MAX_VERSION: i64 = 1;

fn init_writer_conn(path: &Path) -> Result<Connection> {
    let mut conn = Connection::open(path)?;
    // WAL is persistent at the DB level; foreign_keys is per-connection (also set
    // on every reader). busy_timeout avoids spurious SQLITE_BUSY under contention.
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    refuse_if_newer(&conn)?;
    // Pin the flags that uphold invariant 13 explicitly rather than relying on
    // refinery's defaults: an applied-but-absent (future) or divergent migration
    // must abort, never silently proceed.
    embedded::migrations::runner()
        .set_abort_missing(true)
        .set_abort_divergent(true)
        .run(&mut conn)?;
    Ok(conn)
}

/// Invariant 13: refuse to open a DB migrated past what this binary embeds.
/// Checked before running migrations so a future DB gets an actionable
/// [`StoreError::DbTooNew`] instead of a cryptic refinery divergence error.
fn refuse_if_newer(conn: &Connection) -> Result<()> {
    let has_history: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'refinery_schema_history')",
        [],
        |r| r.get(0),
    )?;
    if !has_history {
        return Ok(()); // fresh DB
    }
    let found: Option<i64> = conn.query_row(
        "SELECT MAX(version) FROM refinery_schema_history",
        [],
        |r| r.get(0),
    )?;
    if let Some(found) = found
        && found > EMBEDDED_MAX_VERSION
    {
        return Err(StoreError::DbTooNew {
            found,
            supported: EMBEDDED_MAX_VERSION,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// The hand-maintained `EMBEDDED_MAX_VERSION` must track the migrations
    /// refinery actually embedded, or invariant-13 checking drifts silently.
    #[test]
    fn embedded_max_version_matches_refinery() {
        let max = super::embedded::migrations::runner()
            .get_migrations()
            .iter()
            .map(refinery::Migration::version)
            .max()
            .unwrap_or(0);
        assert_eq!(i64::from(max), super::EMBEDDED_MAX_VERSION);
    }
}
