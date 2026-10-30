-- Shared issues (`tracker`, `issues_sync`), keyed by storage repository as
-- `StorageRepository::key`. What this Mac last synced and last wrote of each tracker file is
-- sync's per-file record in `sync.sql`, which the repository's one engine keeps for every rule
-- set.
-- The index of each tracker's issues for the list and search, as `IssueListing` JSON, rebuilt
-- from the files when `issue_index_format` is missing or holds another format. `stamp` is the
-- issue folder's file names, sizes and times, so a change on disk is found without reading the
-- files; `version` is a digest of their content, for unread marks; `text` is the ID, title,
-- description and comments in lower case, for search.
CREATE TABLE IF NOT EXISTS issue_index (storage TEXT NOT NULL, id TEXT NOT NULL, state TEXT, data TEXT NOT NULL, stamp TEXT NOT NULL, version TEXT NOT NULL, text TEXT NOT NULL, PRIMARY KEY(storage, id));
CREATE TABLE IF NOT EXISTS issue_index_format (format INTEGER NOT NULL);
-- The `version` of each issue as the human last opened it on this Mac, or as the host last
-- wrote it for the human. An issue whose version differs is unread. Never written to a tracker.
CREATE TABLE IF NOT EXISTS issue_read (storage TEXT NOT NULL, id TEXT NOT NULL, version TEXT NOT NULL, PRIMARY KEY(storage, id));
-- The Space of the last issue filed on this Mac, one row at most, where New issue starts.
CREATE TABLE IF NOT EXISTS issue_last_filed (space TEXT NOT NULL);
-- The temporary IDs this Mac gave issues it filed, and the number each was pushed as.
CREATE TABLE IF NOT EXISTS issue_temporary (storage TEXT NOT NULL, temporary TEXT NOT NULL, number TEXT, PRIMARY KEY(storage, temporary));
-- Links between issues and this Mac's projects, or one of a project's tickets (`ticket` is ''
-- for the whole project). Never written to a tracker.
CREATE TABLE IF NOT EXISTS issue_links (storage TEXT NOT NULL, id TEXT NOT NULL, project TEXT NOT NULL, ticket TEXT NOT NULL, PRIMARY KEY(storage, id, project, ticket));
-- The state each linked issue's work last derived on this Mac. The issue follows its work when
-- this changes, so a manual move holds until then.
CREATE TABLE IF NOT EXISTS issue_derived (storage TEXT NOT NULL, id TEXT NOT NULL, state TEXT NOT NULL, PRIMARY KEY(storage, id));
-- The parts of an issue paused by the per-field merge, a front matter field or the description,
-- with the remote's value they compare against. Whole files pause in `sync_paused` until that
-- merge exists.
CREATE TABLE IF NOT EXISTS issue_paused_parts (storage TEXT NOT NULL, id TEXT NOT NULL, part TEXT NOT NULL, theirs TEXT, PRIMARY KEY(storage, id, part));
