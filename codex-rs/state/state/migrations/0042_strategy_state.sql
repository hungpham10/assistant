CREATE TABLE strategy_state (
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    strategy_name TEXT NOT NULL,
    model_json TEXT NOT NULL,
    captured_at INTEGER NOT NULL,
    turn_id TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
