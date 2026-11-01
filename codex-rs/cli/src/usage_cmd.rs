use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use clap::Args;
use codex_state::StateRuntime;
use codex_state::TurnUsageByModel;
use codex_state::TurnUsageSummary;
use codex_utils_absolute_path::AbsolutePathBuf;

use crate::find_codex_home;

#[derive(Debug, Args)]
pub struct UsageCommand {
    /// Only include usage from the last N days (default: 30).
    #[clap(long, default_value_t = 30)]
    pub days: u64,

    /// Print machine-readable JSON instead of a table.
    #[clap(long)]
    pub json: bool,
}

pub async fn run_usage_command(command: UsageCommand) -> anyhow::Result<()> {
    let codex_home = find_codex_home()?;
    let codex_home = AbsolutePathBuf::into_path_buf(codex_home);
    let runtime = StateRuntime::init(codex_home, String::new()).await?;
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(command.days.saturating_mul(24 * 60 * 60));
    let total = runtime
        .summarize_turn_usage(Some(since.try_into()?))
        .await?;
    let by_model = runtime
        .summarize_turn_usage_by_model(Some(since.try_into()?))
        .await?;

    if command.json {
        print_json(command.days, &total, &by_model);
        return Ok(());
    }
    print_table(command.days, &total, &by_model);
    Ok(())
}

fn print_json(days: u64, total: &TurnUsageSummary, by_model: &[TurnUsageByModel]) {
    let models: Vec<serde_json::Value> = by_model
        .iter()
        .map(|entry| {
            serde_json::json!({
                "model": entry.model,
                "request_count": entry.summary.request_count,
                "input_tokens": entry.summary.input_tokens,
                "cached_input_tokens": entry.summary.cached_input_tokens,
                "output_tokens": entry.summary.output_tokens,
                "reasoning_output_tokens": entry.summary.reasoning_output_tokens,
            })
        })
        .collect();
    let doc = serde_json::json!({
        "days": days,
        "request_count": total.request_count,
        "input_tokens": total.input_tokens,
        "cached_input_tokens": total.cached_input_tokens,
        "output_tokens": total.output_tokens,
        "reasoning_output_tokens": total.reasoning_output_tokens,
        "by_model": models,
    });
    println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
}

fn print_table(days: u64, total: &TurnUsageSummary, by_model: &[TurnUsageByModel]) {
    println!("Token usage for the last {days} day(s):");
    println!();
    println!(
        "  requests: {count}  input: {input}  cached: {cached}  output: {output}  reasoning: {reasoning}",
        count = total.request_count,
        input = total.input_tokens,
        cached = total.cached_input_tokens,
        output = total.output_tokens,
        reasoning = total.reasoning_output_tokens,
    );
    if by_model.is_empty() {
        println!();
        println!("  (no usage recorded yet; usage metering starts with this build)");
        return;
    }
    println!();
    println!("  By model:");
    for entry in by_model {
        println!(
            "    {model:<32} requests: {count:<8} in: {input:<10} cached: {cached:<10} out: {output:<10}",
            model = entry.model,
            count = entry.summary.request_count,
            input = entry.summary.input_tokens,
            cached = entry.summary.cached_input_tokens,
            output = entry.summary.output_tokens,
        );
    }
}
