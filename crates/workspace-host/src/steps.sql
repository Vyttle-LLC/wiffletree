CREATE TABLE IF NOT EXISTS steps (session_id TEXT NOT NULL REFERENCES sessions(id), run_id TEXT NOT NULL, id TEXT NOT NULL, revision INTEGER NOT NULL, data TEXT NOT NULL, PRIMARY KEY(run_id,id));
CREATE INDEX IF NOT EXISTS steps_session ON steps(session_id,revision);
