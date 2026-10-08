-- The machine's model configuration: one row holding a `ModelSelection`.
CREATE TABLE IF NOT EXISTS model_selection (id INTEGER PRIMARY KEY CHECK (id = 1), data TEXT NOT NULL);
