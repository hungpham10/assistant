use super::*;

/// Latest strategy-harness snapshot for a thread, derived from the rollout's
/// `RolloutItem::StrategyState` items. The rollout file is the source of
/// truth; this table is a queryable derived index (one row per thread).
#[derive(Debug, Clone, PartialEq)]
pub struct StrategyStateRecord {
    pub thread_id: String,
    pub strategy_name: String,
    /// Serialized `StrategyStateSnapshot::model` payload.
    pub model_json: String,
    /// Unix seconds when the snapshot was captured.
    pub captured_at: i64,
    pub turn_id: String,
    /// Unix seconds when the record was last refreshed.
    pub updated_at: i64,
}

impl StateRuntime {
    /// Stores (or replaces) the latest strategy snapshot for a thread.
    /// Best-effort by callers; this returns an error only for genuine DB
    /// failures.
    pub async fn upsert_strategy_state(&self, record: &StrategyStateRecord) -> anyhow::Result<()> {
        sqlx::query(
            r#"
INSERT INTO strategy_state (
    thread_id,
    strategy_name,
    model_json,
    captured_at,
    turn_id,
    updated_at
)
VALUES (?, ?, ?, ?, ?, ?)
ON CONFLICT(thread_id) DO UPDATE SET
    strategy_name = excluded.strategy_name,
    model_json = excluded.model_json,
    captured_at = excluded.captured_at,
    turn_id = excluded.turn_id,
    updated_at = excluded.updated_at
            "#,
        )
        .bind(&record.thread_id)
        .bind(&record.strategy_name)
        .bind(&record.model_json)
        .bind(record.captured_at)
        .bind(&record.turn_id)
        .bind(record.updated_at)
        .execute(self.pool.as_ref())
        .await?;
        Ok(())
    }

    /// Reads the latest strategy snapshot for a thread.
    pub async fn get_strategy_state(
        &self,
        thread_id: &str,
    ) -> anyhow::Result<Option<StrategyStateRecord>> {
        let row = sqlx::query(
            r#"
SELECT thread_id, strategy_name, model_json, captured_at, turn_id, updated_at
FROM strategy_state
WHERE thread_id = ?
            "#,
        )
        .bind(thread_id)
        .fetch_optional(self.pool.as_ref())
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(StrategyStateRecord {
            thread_id: row.try_get("thread_id")?,
            strategy_name: row.try_get("strategy_name")?,
            model_json: row.try_get("model_json")?,
            captured_at: row.try_get("captured_at")?,
            turn_id: row.try_get("turn_id")?,
            updated_at: row.try_get("updated_at")?,
        }))
    }

    /// Deletes the strategy snapshot for a thread. The table also cascades on
    /// thread deletion; this exists for explicit cleanup and tests.
    pub async fn delete_strategy_state(&self, thread_id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM strategy_state WHERE thread_id = ?")
            .bind(thread_id)
            .execute(self.pool.as_ref())
            .await?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "strategy_state_tests.rs"]
mod tests;
