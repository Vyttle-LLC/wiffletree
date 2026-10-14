CREATE TABLE IF NOT EXISTS process_groups (id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id), run_id TEXT NOT NULL, process_group INTEGER NOT NULL, provider INTEGER NOT NULL DEFAULT 0, seen_at INTEGER NOT NULL, noticed_at INTEGER, ended_at INTEGER, ended TEXT);
CREATE UNIQUE INDEX IF NOT EXISTS process_groups_running ON process_groups(session_id, process_group) WHERE ended_at IS NULL;
CREATE INDEX IF NOT EXISTS process_groups_run ON process_groups(run_id);
CREATE TABLE IF NOT EXISTS process_group_members (group_id TEXT NOT NULL REFERENCES process_groups(id) ON DELETE CASCADE, pid INTEGER NOT NULL, started INTEGER NOT NULL, command TEXT NOT NULL, PRIMARY KEY (group_id, pid, started)) WITHOUT ROWID;
