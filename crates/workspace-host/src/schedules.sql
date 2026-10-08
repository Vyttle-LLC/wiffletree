CREATE TABLE IF NOT EXISTS schedules (id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id), session_id TEXT NOT NULL REFERENCES sessions(id), label TEXT NOT NULL, prompt TEXT NOT NULL, first_at INTEGER NOT NULL, every_ms INTEGER, until INTEGER, next_fire_at INTEGER, created_at INTEGER NOT NULL, stopped TEXT);
CREATE INDEX IF NOT EXISTS schedules_due ON schedules(next_fire_at) WHERE next_fire_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS schedules_session ON schedules(session_id);
CREATE TABLE IF NOT EXISTS schedule_fires (message_id TEXT PRIMARY KEY REFERENCES messages(id), schedule_id TEXT NOT NULL REFERENCES schedules(id), due_at INTEGER NOT NULL, missed INTEGER NOT NULL, late INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS schedule_fires_schedule ON schedule_fires(schedule_id);
