-- Per-turn token usage metering. One row per completed model request that
-- reports usage, keyed by thread and turn, so cost can be attributed per turn
-- and aggregated over time windows.
CREATE TABLE turn_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    cached_input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_output_tokens INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_turn_usage_thread_created
    ON turn_usage(thread_id, created_at DESC);
CREATE INDEX idx_turn_usage_created
    ON turn_usage(created_at DESC);
CREATE INDEX idx_turn_usage_model_created
    ON turn_usage(model, created_at DESC);
