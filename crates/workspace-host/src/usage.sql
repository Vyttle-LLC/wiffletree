CREATE TABLE IF NOT EXISTS usage_requests (
    provider TEXT NOT NULL, request_id TEXT NOT NULL, run_id TEXT NOT NULL REFERENCES provider_runs(id),
    session_id TEXT NOT NULL REFERENCES sessions(id), project_id TEXT NOT NULL REFERENCES projects(id),
    coordinator_id TEXT NOT NULL REFERENCES sessions(id), model TEXT NOT NULL, recorded_at INTEGER NOT NULL,
    input INTEGER NOT NULL, output INTEGER NOT NULL, cache_read INTEGER NOT NULL, cache_write INTEGER NOT NULL,
    reasoning INTEGER NOT NULL, complete INTEGER NOT NULL, native TEXT NOT NULL,
    PRIMARY KEY(provider, request_id)
);
CREATE INDEX IF NOT EXISTS usage_date ON usage_requests(recorded_at,provider);
CREATE INDEX IF NOT EXISTS usage_project_date ON usage_requests(project_id,recorded_at);
CREATE INDEX IF NOT EXISTS usage_run ON usage_requests(run_id);
CREATE TABLE IF NOT EXISTS quota_latest (provider TEXT PRIMARY KEY, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS quota_history (
    provider TEXT NOT NULL, account TEXT NOT NULL, window_key TEXT NOT NULL, observed_at INTEGER NOT NULL,
    resets_at INTEGER, used REAL NOT NULL, label TEXT NOT NULL,
    PRIMARY KEY(provider,account,window_key,observed_at)
);
CREATE INDEX IF NOT EXISTS quota_date ON quota_history(observed_at);
CREATE TABLE IF NOT EXISTS usage_meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
PRAGMA user_version = 3;
CREATE TABLE IF NOT EXISTS usage_counters (
    scope TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES provider_runs(id), counts TEXT NOT NULL
);
