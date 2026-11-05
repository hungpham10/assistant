//! Object-safe facade over [`StateRuntime`] used by dyn-handle holders.
//! The forwarding
//! impl delegates every operation straight to the runtime; a remote store can
//! implement the trait without changing call sites.

use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;

use chrono::DateTime;
use chrono::Utc;
use codex_protocol::ThreadId;
use codex_protocol::protocol::RolloutItem;
use serde_json::Value;

use crate::StateRuntime;
use crate::{ThreadMetadataBuilder, AgentJob, AgentJobCreateParams, AgentJobItem, AgentJobItemCreateParams, AgentJobItemStatus, AgentJobProgress, Anchor, BackfillState, DirectionalThreadSpawnEdgeStatus, ExternalAgentConfigImportDetailsRecord, ExternalAgentConfigImportFailureRecord, ExternalAgentConfigImportHistoryRecord, ExternalAgentConfigImportSuccessRecord, GoalAccountingMode, GoalAccountingOutcome, GoalStore, GoalUpdate, LogEntry, LogQuery, LogRow, MemoryStore, Phase2JobClaimOutcome, RemoteControlEnrollmentRecord, SortKey, Stage1JobClaim, Stage1JobClaimOutcome, Stage1Output, Stage1StartupClaimParams, ThreadFilterOptions, TurnUsageByModel, TurnUsageRecord, TurnUsageSummary};

/// Future returned by async [`RolloutStore`] operations.
pub type RolloutStoreFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Object-safe facade over [`StateRuntime`] used by consumers of
/// dyn-object handles. Async methods return boxed futures so the handle can
pub trait RolloutStore: Send + Sync {
    fn codex_home(&self) -> &Path;
    fn thread_goals(&self) -> &GoalStore;
    fn memories(&self) -> &MemoryStore;
    fn close<'a>(&'a self) -> RolloutStoreFuture<'a, ()>;
    fn create_agent_job<'a>(&'a self, params: &'a AgentJobCreateParams, items: &'a [AgentJobItemCreateParams]) -> RolloutStoreFuture<'a, anyhow::Result<AgentJob>>;
    fn get_agent_job<'a>(&'a self, job_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<Option<AgentJob>>>;
    fn list_agent_job_items<'a>(&'a self, job_id: &'a str, status: Option<AgentJobItemStatus>, limit: Option<usize>) -> RolloutStoreFuture<'a, anyhow::Result<Vec<AgentJobItem>>>;
    fn get_agent_job_item<'a>(&'a self, job_id: &'a str, item_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<Option<AgentJobItem>>>;
    fn mark_agent_job_running<'a>(&'a self, job_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn mark_agent_job_completed<'a>(&'a self, job_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn mark_agent_job_failed<'a>(&'a self, job_id: &'a str, error_message: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn mark_agent_job_cancelled<'a>(&'a self, job_id: &'a str, reason: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn is_agent_job_cancelled<'a>(&'a self, job_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn mark_agent_job_item_running<'a>(&'a self, job_id: &'a str, item_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn mark_agent_job_item_running_with_thread<'a>(&'a self, job_id: &'a str, item_id: &'a str, thread_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn mark_agent_job_item_pending<'a>(&'a self, job_id: &'a str, item_id: &'a str, error_message: Option<&'a str>) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn set_agent_job_item_thread<'a>(&'a self, job_id: &'a str, item_id: &'a str, thread_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn report_agent_job_item_result<'a>(&'a self, job_id: &'a str, item_id: &'a str, reporting_thread_id: &'a str, result_json: &'a Value) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn mark_agent_job_item_completed<'a>(&'a self, job_id: &'a str, item_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn mark_agent_job_item_failed<'a>(&'a self, job_id: &'a str, item_id: &'a str, error_message: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn get_agent_job_progress<'a>(&'a self, job_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<AgentJobProgress>>;
    fn get_backfill_state<'a>(&'a self) -> RolloutStoreFuture<'a, anyhow::Result<crate::BackfillState>>;
    fn try_claim_backfill<'a>(&'a self, lease_seconds: i64) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn mark_backfill_running<'a>(&'a self) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn checkpoint_backfill<'a>(&'a self, watermark: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn mark_backfill_complete<'a>(&'a self, last_watermark: Option<&'a str>) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn record_external_agent_config_import_completed<'a>(&'a self, import_id: &'a str, successes: &'a [ExternalAgentConfigImportSuccessRecord], failures: &'a [ExternalAgentConfigImportFailureRecord]) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn external_agent_config_import_details_record<'a>(&'a self, import_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<Option<ExternalAgentConfigImportDetailsRecord>>>;
    fn external_agent_config_import_history_records<'a>(&'a self) -> RolloutStoreFuture<'a, anyhow::Result<Vec<ExternalAgentConfigImportHistoryRecord>>>;
    fn get_thread_goal<'a>(&'a self, thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadGoal>>>;
    fn replace_thread_goal<'a>(&'a self, thread_id: ThreadId, objective: &'a str, status: crate::ThreadGoalStatus, token_budget: Option<i64>) -> RolloutStoreFuture<'a, anyhow::Result<crate::ThreadGoal>>;
    fn insert_thread_goal<'a>(&'a self, thread_id: ThreadId, objective: &'a str, status: crate::ThreadGoalStatus, token_budget: Option<i64>) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadGoal>>>;
    fn update_thread_goal<'a>(&'a self, thread_id: ThreadId, update: GoalUpdate) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadGoal>>>;
    fn pause_active_thread_goal<'a>(&'a self, thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadGoal>>>;
    fn usage_limit_active_thread_goal<'a>(&'a self, thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadGoal>>>;
    fn delete_thread_goal<'a>(&'a self, thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadGoal>>>;
    fn account_thread_goal_usage<'a>(&'a self, thread_id: ThreadId, time_delta_seconds: i64, token_delta: i64, mode: GoalAccountingMode, expected_goal_id: Option<&'a str>) -> RolloutStoreFuture<'a, anyhow::Result<GoalAccountingOutcome>>;
    fn insert_log<'a>(&'a self, entry: &'a LogEntry) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn insert_logs<'a>(&'a self, entries: &'a [LogEntry]) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn query_logs<'a>(&'a self, query: &'a LogQuery) -> RolloutStoreFuture<'a, anyhow::Result<Vec<LogRow>>>;
    fn query_feedback_logs_for_threads<'a>(&'a self, thread_ids: &'a [&'a str]) -> RolloutStoreFuture<'a, anyhow::Result<Vec<u8>>>;
    fn query_feedback_logs<'a>(&'a self, thread_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<Vec<u8>>>;
    fn max_log_id<'a>(&'a self, query: &'a LogQuery) -> RolloutStoreFuture<'a, anyhow::Result<i64>>;
    fn clear_memory_data<'a>(&'a self) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn record_stage1_output_usage<'a>(&'a self, thread_ids: &'a [ThreadId]) -> RolloutStoreFuture<'a, anyhow::Result<usize>>;
    fn claim_stage1_jobs_for_startup<'a>(&'a self, current_thread_id: ThreadId, params: Stage1StartupClaimParams<'a>) -> RolloutStoreFuture<'a, anyhow::Result<Vec<Stage1JobClaim>>>;
    fn list_stage1_outputs_for_global<'a>(&'a self, n: usize) -> RolloutStoreFuture<'a, anyhow::Result<Vec<Stage1Output>>>;
    fn prune_stage1_outputs_for_retention<'a>(&'a self, max_unused_days: i64, limit: usize) -> RolloutStoreFuture<'a, anyhow::Result<usize>>;
    fn get_phase2_input_selection<'a>(&'a self, n: usize, max_unused_days: i64) -> RolloutStoreFuture<'a, anyhow::Result<Vec<Stage1Output>>>;
    fn mark_thread_memory_mode_polluted<'a>(&'a self, thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn try_claim_stage1_job<'a>(&'a self, thread_id: ThreadId, worker_id: ThreadId, source_updated_at: i64, lease_seconds: i64, max_running_jobs: usize) -> RolloutStoreFuture<'a, anyhow::Result<Stage1JobClaimOutcome>>;
    fn mark_stage1_job_succeeded<'a>(&'a self, thread_id: ThreadId, ownership_token: &'a str, source_updated_at: i64, raw_memory: &'a str, rollout_summary: &'a str, rollout_slug: Option<&'a str>) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn mark_stage1_job_succeeded_no_output<'a>(&'a self, thread_id: ThreadId, ownership_token: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn mark_stage1_job_failed<'a>(&'a self, thread_id: ThreadId, ownership_token: &'a str, failure_reason: &'a str, retry_delay_seconds: i64) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn enqueue_global_consolidation<'a>(&'a self, input_watermark: i64) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn try_claim_global_phase2_job<'a>(&'a self, worker_id: ThreadId, lease_seconds: i64) -> RolloutStoreFuture<'a, anyhow::Result<Phase2JobClaimOutcome>>;
    fn heartbeat_global_phase2_job<'a>(&'a self, ownership_token: &'a str, lease_seconds: i64) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn mark_global_phase2_job_succeeded<'a>(&'a self, ownership_token: &'a str, completed_watermark: i64, selected_outputs: &'a [Stage1Output]) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn mark_global_phase2_job_failed<'a>(&'a self, ownership_token: &'a str, failure_reason: &'a str, retry_delay_seconds: i64) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn mark_global_phase2_job_failed_if_unowned<'a>(&'a self, ownership_token: &'a str, failure_reason: &'a str, retry_delay_seconds: i64) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn get_remote_control_enrollment<'a>(&'a self, websocket_url: &'a str, account_id: &'a str, app_server_client_name: Option<&'a str>) -> RolloutStoreFuture<'a, anyhow::Result<Option<RemoteControlEnrollmentRecord>>>;
    fn upsert_remote_control_enrollment<'a>(&'a self, enrollment: &'a RemoteControlEnrollmentRecord) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn set_remote_control_enabled<'a>(&'a self, websocket_url: &'a str, account_id: &'a str, app_server_client_name: Option<&'a str>, remote_control_enabled: bool) -> RolloutStoreFuture<'a, anyhow::Result<u64>>;
    fn delete_remote_control_enrollment<'a>(&'a self, websocket_url: &'a str, account_id: &'a str, app_server_client_name: Option<&'a str>) -> RolloutStoreFuture<'a, anyhow::Result<u64>>;
    fn get_thread<'a>(&'a self, id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadMetadata>>>;
    fn get_thread_memory_mode<'a>(&'a self, id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Option<String>>>;
    fn set_thread_preview_if_empty<'a>(&'a self, thread_id: ThreadId, preview: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn upsert_thread_spawn_edge<'a>(&'a self, parent_thread_id: ThreadId, child_thread_id: ThreadId, status: crate::DirectionalThreadSpawnEdgeStatus) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn set_thread_spawn_edge_status<'a>(&'a self, child_thread_id: ThreadId, status: crate::DirectionalThreadSpawnEdgeStatus) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn list_thread_spawn_children_with_status<'a>(&'a self, parent_thread_id: ThreadId, status: crate::DirectionalThreadSpawnEdgeStatus) -> RolloutStoreFuture<'a, anyhow::Result<Vec<ThreadId>>>;
    fn list_thread_spawn_children<'a>(&'a self, parent_thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Vec<ThreadId>>>;
    fn list_thread_spawn_descendants_with_status<'a>(&'a self, root_thread_id: ThreadId, status: crate::DirectionalThreadSpawnEdgeStatus) -> RolloutStoreFuture<'a, anyhow::Result<Vec<ThreadId>>>;
    fn list_thread_spawn_descendants<'a>(&'a self, root_thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Vec<ThreadId>>>;
    fn find_thread_spawn_child_by_path<'a>(&'a self, parent_thread_id: ThreadId, agent_path: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<Option<ThreadId>>>;
    fn find_thread_spawn_descendant_by_path<'a>(&'a self, root_thread_id: ThreadId, agent_path: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<Option<ThreadId>>>;
    fn find_rollout_path_by_id<'a>(&'a self, id: ThreadId, archived_only: Option<bool>) -> RolloutStoreFuture<'a, anyhow::Result<Option<PathBuf>>>;
    fn find_thread_by_exact_title<'a>(&'a self, title: &'a str, allowed_sources: &'a [String], model_providers: Option<&'a [String]>, archived_only: bool, cwd: Option<&'a Path>) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadMetadata>>>;
    fn list_threads<'a>(&'a self, page_size: usize, filters: ThreadFilterOptions<'a>) -> RolloutStoreFuture<'a, anyhow::Result<crate::ThreadsPage>>;
    fn list_threads_by_parent<'a>(&'a self, page_size: usize, parent_thread_id: ThreadId, filters: ThreadFilterOptions<'a>) -> RolloutStoreFuture<'a, anyhow::Result<crate::ThreadsPage>>;
    fn list_threads_by_relation<'a>(&'a self, page_size: usize, relation_filter: crate::ThreadRelationFilter, filters: ThreadFilterOptions<'a>) -> RolloutStoreFuture<'a, anyhow::Result<crate::ThreadsPage>>;
    fn list_thread_ids<'a>(&'a self, limit: usize, anchor: Option<&'a crate::Anchor>, sort_key: crate::SortKey, allowed_sources: &'a [String], model_providers: Option<&'a [String]>, archived_only: bool) -> RolloutStoreFuture<'a, anyhow::Result<Vec<ThreadId>>>;
    fn upsert_thread<'a>(&'a self, metadata: &'a crate::ThreadMetadata) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn insert_thread_if_absent<'a>(&'a self, metadata: &'a crate::ThreadMetadata) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn set_thread_memory_mode<'a>(&'a self, thread_id: ThreadId, memory_mode: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn update_thread_title<'a>(&'a self, thread_id: ThreadId, title: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn touch_thread_updated_at<'a>(&'a self, thread_id: ThreadId, updated_at: DateTime<Utc>) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn touch_thread_recency_at<'a>(&'a self, thread_id: ThreadId, recency_at: DateTime<Utc>) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn update_thread_git_info<'a>(&'a self, thread_id: ThreadId, git_sha: Option<Option<&'a str>>, git_branch: Option<Option<&'a str>>, git_origin_url: Option<Option<&'a str>>) -> RolloutStoreFuture<'a, anyhow::Result<bool>>;
    fn apply_rollout_items<'a>(&'a self, builder: &'a ThreadMetadataBuilder, items: &'a [RolloutItem], new_thread_memory_mode: Option<&'a str>, updated_at_override: Option<DateTime<Utc>>) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn mark_archived<'a>(&'a self, thread_id: ThreadId, rollout_path: &'a Path, archived_at: DateTime<Utc>) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn mark_unarchived<'a>(&'a self, thread_id: ThreadId, rollout_path: &'a Path) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn delete_thread<'a>(&'a self, thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<u64>>;
    fn delete_threads_strict<'a>(&'a self, thread_ids: &'a [ThreadId]) -> RolloutStoreFuture<'a, anyhow::Result<u64>>;
    fn record_turn_usage<'a>(&'a self, record: &'a TurnUsageRecord) -> RolloutStoreFuture<'a, anyhow::Result<()>>;
    fn summarize_turn_usage<'a>(&'a self, since_epoch_seconds: Option<i64>) -> RolloutStoreFuture<'a, anyhow::Result<TurnUsageSummary>>;
    fn summarize_turn_usage_by_model<'a>(&'a self, since_epoch_seconds: Option<i64>) -> RolloutStoreFuture<'a, anyhow::Result<Vec<TurnUsageByModel>>>;
    fn prune_turn_usage<'a>(&'a self, cutoff_epoch_seconds: i64) -> RolloutStoreFuture<'a, anyhow::Result<u64>>;
}

impl RolloutStore for StateRuntime {
    fn codex_home(&self) -> &Path {
        StateRuntime::codex_home(self)
    }
    fn thread_goals(&self) -> &GoalStore {
        StateRuntime::thread_goals(self)
    }
    fn memories(&self) -> &MemoryStore {
        StateRuntime::memories(self)
    }
    fn close<'a>(&'a self) -> RolloutStoreFuture<'a, ()> {
        Box::pin(async move { StateRuntime::close(self).await })
    }
    fn create_agent_job<'a>(&'a self, params: &'a AgentJobCreateParams, items: &'a [AgentJobItemCreateParams]) -> RolloutStoreFuture<'a, anyhow::Result<AgentJob>> {
        Box::pin(async move { StateRuntime::create_agent_job(self, params, items).await })
    }
    fn get_agent_job<'a>(&'a self, job_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<Option<AgentJob>>> {
        Box::pin(async move { StateRuntime::get_agent_job(self, job_id).await })
    }
    fn list_agent_job_items<'a>(&'a self, job_id: &'a str, status: Option<AgentJobItemStatus>, limit: Option<usize>) -> RolloutStoreFuture<'a, anyhow::Result<Vec<AgentJobItem>>> {
        Box::pin(async move { StateRuntime::list_agent_job_items(self, job_id, status, limit).await })
    }
    fn get_agent_job_item<'a>(&'a self, job_id: &'a str, item_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<Option<AgentJobItem>>> {
        Box::pin(async move { StateRuntime::get_agent_job_item(self, job_id, item_id).await })
    }
    fn mark_agent_job_running<'a>(&'a self, job_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::mark_agent_job_running(self, job_id).await })
    }
    fn mark_agent_job_completed<'a>(&'a self, job_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::mark_agent_job_completed(self, job_id).await })
    }
    fn mark_agent_job_failed<'a>(&'a self, job_id: &'a str, error_message: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::mark_agent_job_failed(self, job_id, error_message).await })
    }
    fn mark_agent_job_cancelled<'a>(&'a self, job_id: &'a str, reason: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::mark_agent_job_cancelled(self, job_id, reason).await })
    }
    fn is_agent_job_cancelled<'a>(&'a self, job_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::is_agent_job_cancelled(self, job_id).await })
    }
    fn mark_agent_job_item_running<'a>(&'a self, job_id: &'a str, item_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::mark_agent_job_item_running(self, job_id, item_id).await })
    }
    fn mark_agent_job_item_running_with_thread<'a>(&'a self, job_id: &'a str, item_id: &'a str, thread_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::mark_agent_job_item_running_with_thread(self, job_id, item_id, thread_id).await })
    }
    fn mark_agent_job_item_pending<'a>(&'a self, job_id: &'a str, item_id: &'a str, error_message: Option<&'a str>) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::mark_agent_job_item_pending(self, job_id, item_id, error_message).await })
    }
    fn set_agent_job_item_thread<'a>(&'a self, job_id: &'a str, item_id: &'a str, thread_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::set_agent_job_item_thread(self, job_id, item_id, thread_id).await })
    }
    fn report_agent_job_item_result<'a>(&'a self, job_id: &'a str, item_id: &'a str, reporting_thread_id: &'a str, result_json: &'a Value) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::report_agent_job_item_result(self, job_id, item_id, reporting_thread_id, result_json).await })
    }
    fn mark_agent_job_item_completed<'a>(&'a self, job_id: &'a str, item_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::mark_agent_job_item_completed(self, job_id, item_id).await })
    }
    fn mark_agent_job_item_failed<'a>(&'a self, job_id: &'a str, item_id: &'a str, error_message: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::mark_agent_job_item_failed(self, job_id, item_id, error_message).await })
    }
    fn get_agent_job_progress<'a>(&'a self, job_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<AgentJobProgress>> {
        Box::pin(async move { StateRuntime::get_agent_job_progress(self, job_id).await })
    }
    fn get_backfill_state<'a>(&'a self) -> RolloutStoreFuture<'a, anyhow::Result<crate::BackfillState>> {
        Box::pin(async move { StateRuntime::get_backfill_state(self).await })
    }
    fn try_claim_backfill<'a>(&'a self, lease_seconds: i64) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::try_claim_backfill(self, lease_seconds).await })
    }
    fn mark_backfill_running<'a>(&'a self) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::mark_backfill_running(self).await })
    }
    fn checkpoint_backfill<'a>(&'a self, watermark: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::checkpoint_backfill(self, watermark).await })
    }
    fn mark_backfill_complete<'a>(&'a self, last_watermark: Option<&'a str>) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::mark_backfill_complete(self, last_watermark).await })
    }
    fn record_external_agent_config_import_completed<'a>(&'a self, import_id: &'a str, successes: &'a [ExternalAgentConfigImportSuccessRecord], failures: &'a [ExternalAgentConfigImportFailureRecord]) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::record_external_agent_config_import_completed(self, import_id, successes, failures).await })
    }
    fn external_agent_config_import_details_record<'a>(&'a self, import_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<Option<ExternalAgentConfigImportDetailsRecord>>> {
        Box::pin(async move { StateRuntime::external_agent_config_import_details_record(self, import_id).await })
    }
    fn external_agent_config_import_history_records<'a>(&'a self) -> RolloutStoreFuture<'a, anyhow::Result<Vec<ExternalAgentConfigImportHistoryRecord>>> {
        Box::pin(async move { StateRuntime::external_agent_config_import_history_records(self).await })
    }
    fn get_thread_goal<'a>(&'a self, thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadGoal>>> {
        Box::pin(async move { StateRuntime::get_thread_goal(self, thread_id).await })
    }
    fn replace_thread_goal<'a>(&'a self, thread_id: ThreadId, objective: &'a str, status: crate::ThreadGoalStatus, token_budget: Option<i64>) -> RolloutStoreFuture<'a, anyhow::Result<crate::ThreadGoal>> {
        Box::pin(async move { StateRuntime::replace_thread_goal(self, thread_id, objective, status, token_budget).await })
    }
    fn insert_thread_goal<'a>(&'a self, thread_id: ThreadId, objective: &'a str, status: crate::ThreadGoalStatus, token_budget: Option<i64>) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadGoal>>> {
        Box::pin(async move { StateRuntime::insert_thread_goal(self, thread_id, objective, status, token_budget).await })
    }
    fn update_thread_goal<'a>(&'a self, thread_id: ThreadId, update: GoalUpdate) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadGoal>>> {
        Box::pin(async move { StateRuntime::update_thread_goal(self, thread_id, update).await })
    }
    fn pause_active_thread_goal<'a>(&'a self, thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadGoal>>> {
        Box::pin(async move { StateRuntime::pause_active_thread_goal(self, thread_id).await })
    }
    fn usage_limit_active_thread_goal<'a>(&'a self, thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadGoal>>> {
        Box::pin(async move { StateRuntime::usage_limit_active_thread_goal(self, thread_id).await })
    }
    fn delete_thread_goal<'a>(&'a self, thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadGoal>>> {
        Box::pin(async move { StateRuntime::delete_thread_goal(self, thread_id).await })
    }
    fn account_thread_goal_usage<'a>(&'a self, thread_id: ThreadId, time_delta_seconds: i64, token_delta: i64, mode: GoalAccountingMode, expected_goal_id: Option<&'a str>) -> RolloutStoreFuture<'a, anyhow::Result<GoalAccountingOutcome>> {
        Box::pin(async move { StateRuntime::account_thread_goal_usage(self, thread_id, time_delta_seconds, token_delta, mode, expected_goal_id).await })
    }
    fn insert_log<'a>(&'a self, entry: &'a LogEntry) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::insert_log(self, entry).await })
    }
    fn insert_logs<'a>(&'a self, entries: &'a [LogEntry]) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::insert_logs(self, entries).await })
    }
    fn query_logs<'a>(&'a self, query: &'a LogQuery) -> RolloutStoreFuture<'a, anyhow::Result<Vec<LogRow>>> {
        Box::pin(async move { StateRuntime::query_logs(self, query).await })
    }
    fn query_feedback_logs_for_threads<'a>(&'a self, thread_ids: &'a [&'a str]) -> RolloutStoreFuture<'a, anyhow::Result<Vec<u8>>> {
        Box::pin(async move { StateRuntime::query_feedback_logs_for_threads(self, thread_ids).await })
    }
    fn query_feedback_logs<'a>(&'a self, thread_id: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<Vec<u8>>> {
        Box::pin(async move { StateRuntime::query_feedback_logs(self, thread_id).await })
    }
    fn max_log_id<'a>(&'a self, query: &'a LogQuery) -> RolloutStoreFuture<'a, anyhow::Result<i64>> {
        Box::pin(async move { StateRuntime::max_log_id(self, query).await })
    }
    fn clear_memory_data<'a>(&'a self) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::clear_memory_data(self).await })
    }
    fn record_stage1_output_usage<'a>(&'a self, thread_ids: &'a [ThreadId]) -> RolloutStoreFuture<'a, anyhow::Result<usize>> {
        Box::pin(async move { StateRuntime::record_stage1_output_usage(self, thread_ids).await })
    }
    fn claim_stage1_jobs_for_startup<'a>(&'a self, current_thread_id: ThreadId, params: Stage1StartupClaimParams<'a>) -> RolloutStoreFuture<'a, anyhow::Result<Vec<Stage1JobClaim>>> {
        Box::pin(async move { StateRuntime::claim_stage1_jobs_for_startup(self, current_thread_id, params).await })
    }
    fn list_stage1_outputs_for_global<'a>(&'a self, n: usize) -> RolloutStoreFuture<'a, anyhow::Result<Vec<Stage1Output>>> {
        Box::pin(async move { StateRuntime::list_stage1_outputs_for_global(self, n).await })
    }
    fn prune_stage1_outputs_for_retention<'a>(&'a self, max_unused_days: i64, limit: usize) -> RolloutStoreFuture<'a, anyhow::Result<usize>> {
        Box::pin(async move { StateRuntime::prune_stage1_outputs_for_retention(self, max_unused_days, limit).await })
    }
    fn get_phase2_input_selection<'a>(&'a self, n: usize, max_unused_days: i64) -> RolloutStoreFuture<'a, anyhow::Result<Vec<Stage1Output>>> {
        Box::pin(async move { StateRuntime::get_phase2_input_selection(self, n, max_unused_days).await })
    }
    fn mark_thread_memory_mode_polluted<'a>(&'a self, thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::mark_thread_memory_mode_polluted(self, thread_id).await })
    }
    fn try_claim_stage1_job<'a>(&'a self, thread_id: ThreadId, worker_id: ThreadId, source_updated_at: i64, lease_seconds: i64, max_running_jobs: usize) -> RolloutStoreFuture<'a, anyhow::Result<Stage1JobClaimOutcome>> {
        Box::pin(async move { StateRuntime::try_claim_stage1_job(self, thread_id, worker_id, source_updated_at, lease_seconds, max_running_jobs).await })
    }
    fn mark_stage1_job_succeeded<'a>(&'a self, thread_id: ThreadId, ownership_token: &'a str, source_updated_at: i64, raw_memory: &'a str, rollout_summary: &'a str, rollout_slug: Option<&'a str>) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::mark_stage1_job_succeeded(self, thread_id, ownership_token, source_updated_at, raw_memory, rollout_summary, rollout_slug).await })
    }
    fn mark_stage1_job_succeeded_no_output<'a>(&'a self, thread_id: ThreadId, ownership_token: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::mark_stage1_job_succeeded_no_output(self, thread_id, ownership_token).await })
    }
    fn mark_stage1_job_failed<'a>(&'a self, thread_id: ThreadId, ownership_token: &'a str, failure_reason: &'a str, retry_delay_seconds: i64) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::mark_stage1_job_failed(self, thread_id, ownership_token, failure_reason, retry_delay_seconds).await })
    }
    fn enqueue_global_consolidation<'a>(&'a self, input_watermark: i64) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::enqueue_global_consolidation(self, input_watermark).await })
    }
    fn try_claim_global_phase2_job<'a>(&'a self, worker_id: ThreadId, lease_seconds: i64) -> RolloutStoreFuture<'a, anyhow::Result<Phase2JobClaimOutcome>> {
        Box::pin(async move { StateRuntime::try_claim_global_phase2_job(self, worker_id, lease_seconds).await })
    }
    fn heartbeat_global_phase2_job<'a>(&'a self, ownership_token: &'a str, lease_seconds: i64) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::heartbeat_global_phase2_job(self, ownership_token, lease_seconds).await })
    }
    fn mark_global_phase2_job_succeeded<'a>(&'a self, ownership_token: &'a str, completed_watermark: i64, selected_outputs: &'a [Stage1Output]) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::mark_global_phase2_job_succeeded(self, ownership_token, completed_watermark, selected_outputs).await })
    }
    fn mark_global_phase2_job_failed<'a>(&'a self, ownership_token: &'a str, failure_reason: &'a str, retry_delay_seconds: i64) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::mark_global_phase2_job_failed(self, ownership_token, failure_reason, retry_delay_seconds).await })
    }
    fn mark_global_phase2_job_failed_if_unowned<'a>(&'a self, ownership_token: &'a str, failure_reason: &'a str, retry_delay_seconds: i64) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::mark_global_phase2_job_failed_if_unowned(self, ownership_token, failure_reason, retry_delay_seconds).await })
    }
    fn get_remote_control_enrollment<'a>(&'a self, websocket_url: &'a str, account_id: &'a str, app_server_client_name: Option<&'a str>) -> RolloutStoreFuture<'a, anyhow::Result<Option<RemoteControlEnrollmentRecord>>> {
        Box::pin(async move { StateRuntime::get_remote_control_enrollment(self, websocket_url, account_id, app_server_client_name).await })
    }
    fn upsert_remote_control_enrollment<'a>(&'a self, enrollment: &'a RemoteControlEnrollmentRecord) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::upsert_remote_control_enrollment(self, enrollment).await })
    }
    fn set_remote_control_enabled<'a>(&'a self, websocket_url: &'a str, account_id: &'a str, app_server_client_name: Option<&'a str>, remote_control_enabled: bool) -> RolloutStoreFuture<'a, anyhow::Result<u64>> {
        Box::pin(async move { StateRuntime::set_remote_control_enabled(self, websocket_url, account_id, app_server_client_name, remote_control_enabled).await })
    }
    fn delete_remote_control_enrollment<'a>(&'a self, websocket_url: &'a str, account_id: &'a str, app_server_client_name: Option<&'a str>) -> RolloutStoreFuture<'a, anyhow::Result<u64>> {
        Box::pin(async move { StateRuntime::delete_remote_control_enrollment(self, websocket_url, account_id, app_server_client_name).await })
    }
    fn get_thread<'a>(&'a self, id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadMetadata>>> {
        Box::pin(async move { StateRuntime::get_thread(self, id).await })
    }
    fn get_thread_memory_mode<'a>(&'a self, id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Option<String>>> {
        Box::pin(async move { StateRuntime::get_thread_memory_mode(self, id).await })
    }
    fn set_thread_preview_if_empty<'a>(&'a self, thread_id: ThreadId, preview: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::set_thread_preview_if_empty(self, thread_id, preview).await })
    }
    fn upsert_thread_spawn_edge<'a>(&'a self, parent_thread_id: ThreadId, child_thread_id: ThreadId, status: crate::DirectionalThreadSpawnEdgeStatus) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::upsert_thread_spawn_edge(self, parent_thread_id, child_thread_id, status).await })
    }
    fn set_thread_spawn_edge_status<'a>(&'a self, child_thread_id: ThreadId, status: crate::DirectionalThreadSpawnEdgeStatus) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::set_thread_spawn_edge_status(self, child_thread_id, status).await })
    }
    fn list_thread_spawn_children_with_status<'a>(&'a self, parent_thread_id: ThreadId, status: crate::DirectionalThreadSpawnEdgeStatus) -> RolloutStoreFuture<'a, anyhow::Result<Vec<ThreadId>>> {
        Box::pin(async move { StateRuntime::list_thread_spawn_children_with_status(self, parent_thread_id, status).await })
    }
    fn list_thread_spawn_children<'a>(&'a self, parent_thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Vec<ThreadId>>> {
        Box::pin(async move { StateRuntime::list_thread_spawn_children(self, parent_thread_id).await })
    }
    fn list_thread_spawn_descendants_with_status<'a>(&'a self, root_thread_id: ThreadId, status: crate::DirectionalThreadSpawnEdgeStatus) -> RolloutStoreFuture<'a, anyhow::Result<Vec<ThreadId>>> {
        Box::pin(async move { StateRuntime::list_thread_spawn_descendants_with_status(self, root_thread_id, status).await })
    }
    fn list_thread_spawn_descendants<'a>(&'a self, root_thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<Vec<ThreadId>>> {
        Box::pin(async move { StateRuntime::list_thread_spawn_descendants(self, root_thread_id).await })
    }
    fn find_thread_spawn_child_by_path<'a>(&'a self, parent_thread_id: ThreadId, agent_path: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<Option<ThreadId>>> {
        Box::pin(async move { StateRuntime::find_thread_spawn_child_by_path(self, parent_thread_id, agent_path).await })
    }
    fn find_thread_spawn_descendant_by_path<'a>(&'a self, root_thread_id: ThreadId, agent_path: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<Option<ThreadId>>> {
        Box::pin(async move { StateRuntime::find_thread_spawn_descendant_by_path(self, root_thread_id, agent_path).await })
    }
    fn find_rollout_path_by_id<'a>(&'a self, id: ThreadId, archived_only: Option<bool>) -> RolloutStoreFuture<'a, anyhow::Result<Option<PathBuf>>> {
        Box::pin(async move { StateRuntime::find_rollout_path_by_id(self, id, archived_only).await })
    }
    fn find_thread_by_exact_title<'a>(&'a self, title: &'a str, allowed_sources: &'a [String], model_providers: Option<&'a [String]>, archived_only: bool, cwd: Option<&'a Path>) -> RolloutStoreFuture<'a, anyhow::Result<Option<crate::ThreadMetadata>>> {
        Box::pin(async move { StateRuntime::find_thread_by_exact_title(self, title, allowed_sources, model_providers, archived_only, cwd).await })
    }
    fn list_threads<'a>(&'a self, page_size: usize, filters: ThreadFilterOptions<'a>) -> RolloutStoreFuture<'a, anyhow::Result<crate::ThreadsPage>> {
        Box::pin(async move { StateRuntime::list_threads(self, page_size, filters).await })
    }
    fn list_threads_by_parent<'a>(&'a self, page_size: usize, parent_thread_id: ThreadId, filters: ThreadFilterOptions<'a>) -> RolloutStoreFuture<'a, anyhow::Result<crate::ThreadsPage>> {
        Box::pin(async move { StateRuntime::list_threads_by_parent(self, page_size, parent_thread_id, filters).await })
    }
    fn list_threads_by_relation<'a>(&'a self, page_size: usize, relation_filter: crate::ThreadRelationFilter, filters: ThreadFilterOptions<'a>) -> RolloutStoreFuture<'a, anyhow::Result<crate::ThreadsPage>> {
        Box::pin(async move { StateRuntime::list_threads_by_relation(self, page_size, relation_filter, filters).await })
    }
    fn list_thread_ids<'a>(&'a self, limit: usize, anchor: Option<&'a crate::Anchor>, sort_key: crate::SortKey, allowed_sources: &'a [String], model_providers: Option<&'a [String]>, archived_only: bool) -> RolloutStoreFuture<'a, anyhow::Result<Vec<ThreadId>>> {
        Box::pin(async move { StateRuntime::list_thread_ids(self, limit, anchor, sort_key, allowed_sources, model_providers, archived_only).await })
    }
    fn upsert_thread<'a>(&'a self, metadata: &'a crate::ThreadMetadata) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::upsert_thread(self, metadata).await })
    }
    fn insert_thread_if_absent<'a>(&'a self, metadata: &'a crate::ThreadMetadata) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::insert_thread_if_absent(self, metadata).await })
    }
    fn set_thread_memory_mode<'a>(&'a self, thread_id: ThreadId, memory_mode: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::set_thread_memory_mode(self, thread_id, memory_mode).await })
    }
    fn update_thread_title<'a>(&'a self, thread_id: ThreadId, title: &'a str) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::update_thread_title(self, thread_id, title).await })
    }
    fn touch_thread_updated_at<'a>(&'a self, thread_id: ThreadId, updated_at: DateTime<Utc>) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::touch_thread_updated_at(self, thread_id, updated_at).await })
    }
    fn touch_thread_recency_at<'a>(&'a self, thread_id: ThreadId, recency_at: DateTime<Utc>) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::touch_thread_recency_at(self, thread_id, recency_at).await })
    }
    fn update_thread_git_info<'a>(&'a self, thread_id: ThreadId, git_sha: Option<Option<&'a str>>, git_branch: Option<Option<&'a str>>, git_origin_url: Option<Option<&'a str>>) -> RolloutStoreFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { StateRuntime::update_thread_git_info(self, thread_id, git_sha, git_branch, git_origin_url).await })
    }
    fn apply_rollout_items<'a>(&'a self, builder: &'a ThreadMetadataBuilder, items: &'a [RolloutItem], new_thread_memory_mode: Option<&'a str>, updated_at_override: Option<DateTime<Utc>>) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::apply_rollout_items(self, builder, items, new_thread_memory_mode, updated_at_override).await })
    }
    fn mark_archived<'a>(&'a self, thread_id: ThreadId, rollout_path: &'a Path, archived_at: DateTime<Utc>) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::mark_archived(self, thread_id, rollout_path, archived_at).await })
    }
    fn mark_unarchived<'a>(&'a self, thread_id: ThreadId, rollout_path: &'a Path) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::mark_unarchived(self, thread_id, rollout_path).await })
    }
    fn delete_thread<'a>(&'a self, thread_id: ThreadId) -> RolloutStoreFuture<'a, anyhow::Result<u64>> {
        Box::pin(async move { StateRuntime::delete_thread(self, thread_id).await })
    }
    fn delete_threads_strict<'a>(&'a self, thread_ids: &'a [ThreadId]) -> RolloutStoreFuture<'a, anyhow::Result<u64>> {
        Box::pin(async move { StateRuntime::delete_threads_strict(self, thread_ids).await })
    }
    fn record_turn_usage<'a>(&'a self, record: &'a TurnUsageRecord) -> RolloutStoreFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { StateRuntime::record_turn_usage(self, record).await })
    }
    fn summarize_turn_usage<'a>(&'a self, since_epoch_seconds: Option<i64>) -> RolloutStoreFuture<'a, anyhow::Result<TurnUsageSummary>> {
        Box::pin(async move { StateRuntime::summarize_turn_usage(self, since_epoch_seconds).await })
    }
    fn summarize_turn_usage_by_model<'a>(&'a self, since_epoch_seconds: Option<i64>) -> RolloutStoreFuture<'a, anyhow::Result<Vec<TurnUsageByModel>>> {
        Box::pin(async move { StateRuntime::summarize_turn_usage_by_model(self, since_epoch_seconds).await })
    }
    fn prune_turn_usage<'a>(&'a self, cutoff_epoch_seconds: i64) -> RolloutStoreFuture<'a, anyhow::Result<u64>> {
        Box::pin(async move { StateRuntime::prune_turn_usage(self, cutoff_epoch_seconds).await })
    }
}
