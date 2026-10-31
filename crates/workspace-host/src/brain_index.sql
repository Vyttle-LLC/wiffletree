-- One brain's index (OpenSpec change `brain-browser`): a rebuildable projection of `main` in the
-- host's checkout. Deleting the file rebuilds it on the brain's next pass.
PRAGMA journal_mode=WAL;

-- Every indexed file of the brain folder, keyed by its path there, with its Git blob id as the
-- content hash. `class` is `status`, `index`, `queue`, `note` or `entry`.
CREATE TABLE IF NOT EXISTS files (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  hash TEXT NOT NULL,
  class TEXT NOT NULL,
  -- A note's `kind` or an entry's `type`: `decision` or `observation`.
  kind TEXT,
  -- An entry's repository folder, author, source, id and `ref`.
  repository TEXT COLLATE NOCASE,
  author TEXT COLLATE NOCASE,
  source TEXT,
  entry_id TEXT,
  reference TEXT,
  -- A note's `updated` or an entry's date.
  date TEXT,
  title TEXT NOT NULL,
  -- What a wikilink names a note by: its file name without `.md`, in lower case.
  name TEXT,
  -- The text after the frontmatter.
  body TEXT NOT NULL,
  -- An entry's `ref` checked on this machine, its reason, and the PR of the commit that added
  -- it with that commit's time.
  verification TEXT,
  verification_reason TEXT,
  pull_request INTEGER,
  added_at INTEGER
);
CREATE INDEX IF NOT EXISTS entries_by_id ON files(class, entry_id);
CREATE INDEX IF NOT EXISTS entries_by_repository ON files(class, repository, entry_id);
CREATE INDEX IF NOT EXISTS notes_by_name ON files(class, name);

-- The `[[wikilinks]]` of notes and STATUS, by their target in lower case.
CREATE TABLE IF NOT EXISTS links (
  file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
  target TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS links_by_target ON links(target);
CREATE INDEX IF NOT EXISTS links_by_file ON links(file_id);

-- Each note's sources, from its frontmatter: the repository folder a source names in lower case,
-- or, for one naming only an entry id, its entry's folder once that entry is indexed.
CREATE TABLE IF NOT EXISTS sources (
  file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
  repository TEXT COLLATE NOCASE,
  entry_id TEXT
);
CREATE INDEX IF NOT EXISTS sources_by_file ON sources(file_id);

-- `key` is `schema`, `commit`, `last_compile` and `pending`.
CREATE TABLE IF NOT EXISTS state (key TEXT PRIMARY KEY, value TEXT NOT NULL);

-- Full text over titles and bodies. `_`, `-` and `/` separate tokens, so identifiers and paths
-- match by their parts.
CREATE VIRTUAL TABLE IF NOT EXISTS search USING fts5(
  title, body, content='files', content_rowid='id',
  tokenize="unicode61 separators '_-/'"
);
CREATE TRIGGER IF NOT EXISTS files_added AFTER INSERT ON files BEGIN
  INSERT INTO search(rowid, title, body) VALUES (new.id, new.title, new.body);
END;
CREATE TRIGGER IF NOT EXISTS files_removed AFTER DELETE ON files BEGIN
  INSERT INTO search(search, rowid, title, body) VALUES ('delete', old.id, old.title, old.body);
END;
