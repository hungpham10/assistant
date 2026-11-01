use super::*;

/// One recorded model request's token usage, attributed to a thread turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnUsageRecord {
    pub thread_id: String,
    pub turn_id: String,
    pub model: String,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    /// Unix seconds when the usage was observed.
    pub created_at: i64,
}

/// Aggregated usage over a time window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnUsageSummary {
    pub request_count: i64,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
}

/// Usage totals grouped by model slug over a time window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnUsageByModel {
    pub model: String,
    pub summary: TurnUsageSummary,
}

impl StateRuntime {
    /// Records one model request's token usage. Best-effort by callers; this
    /// returns an error only for genuine DB failures.
    pub async fn record_turn_usage(&self, record: &TurnUsageRecord) -> anyhow::Result<()> {
        sqlx::query(
            r#"
INSERT INTO turn_usage (
    thread_id,
    turn_id,
    model,
    input_tokens,
    cached_input_tokens,
    output_tokens,
    reasoning_output_tokens,
    created_at
)
VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&record.thread_id)
        .bind(&record.turn_id)
        .bind(&record.model)
        .bind(record.input_tokens)
        .bind(record.cached_input_tokens)
        .bind(record.output_tokens)
        .bind(record.reasoning_output_tokens)
        .bind(record.created_at)
        .execute(self.pool.as_ref())
        .await?;
        Ok(())
    }

    /// Aggregated usage since `since_epoch_seconds` (inclusive), across all models.
    pub async fn summarize_turn_usage(
        &self,
        since_epoch_seconds: Option<i64>,
    ) -> anyhow::Result<TurnUsageSummary> {
        let row = match since_epoch_seconds {
            Some(since) => {
                sqlx::query(
                    r#"
SELECT
    COUNT(*) AS request_count,
    COALESCE(SUM(input_tokens), 0) AS input_tokens,
    COALESCE(SUM(cached_input_tokens), 0) AS cached_input_tokens,
    COALESCE(SUM(output_tokens), 0) AS output_tokens,
    COALESCE(SUM(reasoning_output_tokens), 0) AS reasoning_output_tokens
FROM turn_usage
WHERE created_at >= ?
                    "#,
                )
                .bind(since)
                .fetch_one(self.pool.as_ref())
                .await?
            }
            None => {
                sqlx::query(
                    r#"
SELECT
    COUNT(*) AS request_count,
    COALESCE(SUM(input_tokens), 0) AS input_tokens,
    COALESCE(SUM(cached_input_tokens), 0) AS cached_input_tokens,
    COALESCE(SUM(output_tokens), 0) AS output_tokens,
    COALESCE(SUM(reasoning_output_tokens), 0) AS reasoning_output_tokens
FROM turn_usage
                    "#,
                )
                .fetch_one(self.pool.as_ref())
                .await?
            }
        };
        Ok(TurnUsageSummary {
            request_count: row.try_get("request_count")?,
            input_tokens: row.try_get("input_tokens")?,
            cached_input_tokens: row.try_get("cached_input_tokens")?,
            output_tokens: row.try_get("output_tokens")?,
            reasoning_output_tokens: row.try_get("reasoning_output_tokens")?,
        })
    }

    /// Aggregated usage grouped by model slug since `since_epoch_seconds`.
    pub async fn summarize_turn_usage_by_model(
        &self,
        since_epoch_seconds: Option<i64>,
    ) -> anyhow::Result<Vec<TurnUsageByModel>> {
        let rows = match since_epoch_seconds {
            Some(since) => {
                sqlx::query(
                    r#"
SELECT
    model,
    COUNT(*) AS request_count,
    COALESCE(SUM(input_tokens), 0) AS input_tokens,
    COALESCE(SUM(cached_input_tokens), 0) AS cached_input_tokens,
    COALESCE(SUM(output_tokens), 0) AS output_tokens,
    COALESCE(SUM(reasoning_output_tokens), 0) AS reasoning_output_tokens
FROM turn_usage
WHERE created_at >= ?
GROUP BY model
ORDER BY input_tokens + output_tokens DESC
                    "#,
                )
                .bind(since)
                .fetch_all(self.pool.as_ref())
                .await?
            }
            None => {
                sqlx::query(
                    r#"
SELECT
    model,
    COUNT(*) AS request_count,
    COALESCE(SUM(input_tokens), 0) AS input_tokens,
    COALESCE(SUM(cached_input_tokens), 0) AS cached_input_tokens,
    COALESCE(SUM(output_tokens), 0) AS output_tokens,
    COALESCE(SUM(reasoning_output_tokens), 0) AS reasoning_output_tokens
FROM turn_usage
GROUP BY model
ORDER BY input_tokens + output_tokens DESC
                    "#,
                )
                .fetch_all(self.pool.as_ref())
                .await?
            }
        };
        let mut grouped = Vec::with_capacity(rows.len());
        for row in rows {
            grouped.push(TurnUsageByModel {
                model: row.try_get("model")?,
                summary: TurnUsageSummary {
                    request_count: row.try_get("request_count")?,
                    input_tokens: row.try_get("input_tokens")?,
                    cached_input_tokens: row.try_get("cached_input_tokens")?,
                    output_tokens: row.try_get("output_tokens")?,
                    reasoning_output_tokens: row.try_get("reasoning_output_tokens")?,
                },
            });
        }
        Ok(grouped)
    }

    /// Deletes usage rows older than `cutoff_epoch_seconds`. Returns the number
    /// of rows removed so callers can report disk savings.
    pub async fn prune_turn_usage(&self, cutoff_epoch_seconds: i64) -> anyhow::Result<u64> {
        let result = sqlx::query("DELETE FROM turn_usage WHERE created_at < ?")
            .bind(cutoff_epoch_seconds)
            .execute(self.pool.as_ref())
            .await?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
#[path = "turn_usage_tests.rs"]
mod tests;
