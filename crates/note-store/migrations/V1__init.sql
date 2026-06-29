-- M1 schema. notes carries an integer surrogate `rowid` purely so FTS5
-- external-content can key off it; the ULID `id` column is the real identity.

CREATE TABLE notes (
    rowid        INTEGER PRIMARY KEY,
    id           TEXT NOT NULL UNIQUE,
    title        TEXT,
    body         TEXT NOT NULL,
    content_kind TEXT NOT NULL DEFAULT 'markdown',
    created      INTEGER NOT NULL,
    updated      INTEGER NOT NULL
);

CREATE INDEX idx_notes_updated ON notes (updated DESC);

CREATE TABLE tags (
    note_id TEXT NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    tag     TEXT NOT NULL,
    PRIMARY KEY (note_id, tag)
);

CREATE INDEX idx_tags_tag ON tags (tag);

-- Link graph. Populated explicitly (M1) and by [[wikilink]] body extraction (M3).
-- target_kind is 'id' or 'title'; resolved_id is the NoteId a title/id resolved
-- to, or NULL when the link is dangling.
CREATE TABLE links (
    source_id    TEXT NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    target_kind  TEXT NOT NULL,
    target_value TEXT NOT NULL,
    display      TEXT,
    resolved_id  TEXT
);

CREATE INDEX idx_links_source ON links (source_id);
CREATE INDEX idx_links_resolved ON links (resolved_id);

-- External-content FTS5 over notes(title, body). Kept in sync by the triggers
-- below, which run synchronously inside the same transaction as the data write
-- (invariant: indexes never lag the data).
CREATE VIRTUAL TABLE notes_fts USING fts5 (
    title,
    body,
    content = 'notes',
    content_rowid = 'rowid'
);

CREATE TRIGGER notes_ai AFTER INSERT ON notes BEGIN
    INSERT INTO notes_fts (rowid, title, body) VALUES (new.rowid, new.title, new.body);
END;

CREATE TRIGGER notes_ad AFTER DELETE ON notes BEGIN
    INSERT INTO notes_fts (notes_fts, rowid, title, body)
    VALUES ('delete', old.rowid, old.title, old.body);
END;

CREATE TRIGGER notes_au AFTER UPDATE ON notes BEGIN
    INSERT INTO notes_fts (notes_fts, rowid, title, body)
    VALUES ('delete', old.rowid, old.title, old.body);
    INSERT INTO notes_fts (rowid, title, body) VALUES (new.rowid, new.title, new.body);
END;
