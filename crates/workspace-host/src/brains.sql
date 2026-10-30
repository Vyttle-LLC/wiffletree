CREATE TABLE brains (repository TEXT PRIMARY KEY COLLATE NOCASE, granted INTEGER NOT NULL DEFAULT 0, data TEXT NOT NULL);
CREATE TABLE brain_batches (id TEXT PRIMARY KEY, repository TEXT NOT NULL COLLATE NOCASE, pushed TEXT, written TEXT, discard INTEGER NOT NULL DEFAULT 0, item_key TEXT, attention_id TEXT, items INTEGER NOT NULL DEFAULT 0, data TEXT NOT NULL);
CREATE TABLE brain_batch_checks (batch_id TEXT NOT NULL REFERENCES brain_batches(id) ON DELETE CASCADE, name TEXT NOT NULL, conclusion TEXT NOT NULL, annotations TEXT NOT NULL, PRIMARY KEY(batch_id,name));
