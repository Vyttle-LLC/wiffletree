-- Sync's record, per file of the shared folder: the content this machine last synced and the
-- content the host last wrote, NULL for none. The row with an empty path holds, as `synced`,
-- the commit this machine last synced.
CREATE TABLE IF NOT EXISTS sync_record (path TEXT PRIMARY KEY, synced BLOB, written BLOB);
-- The files sync leaves alone until the human decides: why, as JSON, and the remote's content
-- the pause compares against, NULL for none.
CREATE TABLE IF NOT EXISTS sync_paused (path TEXT PRIMARY KEY, why TEXT NOT NULL, theirs BLOB);
