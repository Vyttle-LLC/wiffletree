CREATE TABLE IF NOT EXISTS live_projects (project_id TEXT PRIMARY KEY REFERENCES projects(id), enabled INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS runtimes (session_id TEXT PRIMARY KEY REFERENCES sessions(id), data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS tickets (id TEXT PRIMARY KEY, coordinator_id TEXT NOT NULL REFERENCES sessions(id), data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS provider_runs (id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id), messages TEXT NOT NULL, started_at INTEGER NOT NULL, finished_at INTEGER, outcome TEXT, detail TEXT);
CREATE INDEX IF NOT EXISTS tickets_coordinator ON tickets(coordinator_id);
-- Tickets whose worktree removal waits for an agent's turn to end. Only rows here are swept.
CREATE TABLE IF NOT EXISTS pending_worktree_removals (ticket_id TEXT PRIMARY KEY REFERENCES tickets(id), marked_at INTEGER NOT NULL);
PRAGMA user_version = 2;
