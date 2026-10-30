-- Shared issues (`tracker`, `issues_sync`), keyed by storage repository as
-- `StorageRepository::key`. What this Mac last synced and last wrote of each tracker file is
-- sync's per-file record in `sync.sql`, which the repository's one engine keeps for every rule
-- set.
-- The index of each tracker's issues for the list and search, as `IssueListing` JSON, rebuilt
-- from the files when `issue_index_format` is missing or holds another format.
CREATE TABLE IF NOT EXISTS issue_index (storage TEXT NOT NULL, id TEXT NOT NULL, state TEXT, data TEXT NOT NULL, PRIMARY KEY(storage, id));
CREATE TABLE IF NOT EXISTS issue_index_format (format INTEGER NOT NULL);
-- The temporary IDs this Mac gave issues it filed, and the number each was pushed as.
CREATE TABLE IF NOT EXISTS issue_temporary (storage TEXT NOT NULL, temporary TEXT NOT NULL, number TEXT, PRIMARY KEY(storage, temporary));
-- The parts of an issue paused by the per-field merge, a front matter field or the description,
-- with the remote's value they compare against. Whole files pause in `sync_paused` until that
-- merge exists.
CREATE TABLE IF NOT EXISTS issue_paused_parts (storage TEXT NOT NULL, id TEXT NOT NULL, part TEXT NOT NULL, theirs TEXT, PRIMARY KEY(storage, id, part));
