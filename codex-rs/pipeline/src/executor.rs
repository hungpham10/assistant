use crate::command::CommandPlan;
use crate::command::VerbCall;
use crate::command::resolve_refs;
use crate::component::CommandResult;
use crate::registry::FamilyRegistry;
use serde_json::Value as JsonValue;

#[derive(Debug, Default)]
pub struct ChainOutcome {
    pub results: Vec<CommandResult>,
    pub hints: Vec<String>,
    pub truncated: bool,
}

pub async fn run_chain(registry: &FamilyRegistry, plan: &CommandPlan) -> ChainOutcome {
    let mut outcome = ChainOutcome::default();
    let mut results_vec: Vec<JsonValue> = Vec::new();

    for call in &plan.commands {
        let family = match registry.get(&call.namespace) {
            Some(f) => f,
            None => {
                outcome
                    .hints
                    .push(format!("unknown namespace: {}", call.namespace));
                break;
            }
        };

        let resolved_args = match resolve_refs(call.args.clone(), &results_vec) {
            Ok(args) => args,
            Err(e) => {
                outcome.hints.push(format!("ref resolution failed: {e}"));
                break;
            }
        };

        match family.run_verb(&call.verb, &resolved_args).await {
            Ok(output) => {
                results_vec.push(output.clone());
                outcome.results.push(CommandResult {
                    call: call.clone(),
                    output,
                });
            }
            Err(e) => {
                outcome.hints.push(format!("verb failed: {e}"));
                break;
            }
        }
    }

    outcome
}
