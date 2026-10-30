-- Sync's record, per repository (`repos::key`) and file: the content this machine last synced
-- and the content the host last wrote, NULL for none. The row with an empty path holds, as
-- `synced`, the commit this machine last synced. A store from before repositories were keyed is
-- moved to this layout by `repos::migrate_records`.
CREATE TABLE IF NOT EXISTS sync_record (repository TEXT NOT NULL, path TEXT NOT NULL, synced BLOB, written BLOB, PRIMARY KEY(repository, path));
-- The files sync leaves alone until the human decides: why, as JSON, and the remote's content
-- the pause compares against, NULL for none.
CREATE TABLE IF NOT EXISTS sync_paused (repository TEXT NOT NULL, path TEXT NOT NULL, why TEXT NOT NULL, theirs BLOB, PRIMARY KEY(repository, path));
-- The files the last pass held back from a push because a scan failed or could not run.
CREATE TABLE IF NOT EXISTS sync_held (repository TEXT NOT NULL, path TEXT NOT NULL, check_name TEXT NOT NULL, rule TEXT, reason TEXT, PRIMARY KEY(repository, path));
-- A clone's rename under `repos/`, recorded before it is made and removed after, so a start cut
-- short finishes it at the next start. Paths are relative to `repos/`.
CREATE TABLE IF NOT EXISTS repo_moves (from_path TEXT PRIMARY KEY, to_path TEXT NOT NULL);
-- The GitHub storage repositories on this Mac, by `repos::key`, as `SettingsRepository` JSON.
CREATE TABLE IF NOT EXISTS storage_repositories (key TEXT PRIMARY KEY, data TEXT NOT NULL);
