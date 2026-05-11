use super::analyzer::{ExecutionStrategy, ReasoningDepth, TaskProfile};
use super::planner::ExecutionPlan;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeState {
    pub turns_completed: u32,
    pub tokens_used: u64,
    pub cost_usd: f64,
    pub errors: u32,
    pub retries: u32,
    pub model_switches: u32,
    pub avg_turn_latency_ms: u64,
    pub confidence_score: f32,
    pub current_model: Option<String>,
    pub degradation_triggered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeAdjustment {
    pub switch_model_to: Option<String>,
    pub reduce_context: bool,
    pub increase_reasoning: bool,
    pub decrease_reasoning: bool,
    pub abort: bool,
    pub reason: String,
}

pub struct RuntimeOptimizer;

static RUNTIME_STATE: Mutex<Option<RuntimeState>> = Mutex::new(None);

pub fn init_state() {
    if let Ok(mut guard) = RUNTIME_STATE.lock() {
        if guard.is_none() {
            *guard = Some(RuntimeState::default());
        }
    }
}

pub fn record_turn(
    tokens: u64,
    cost_usd: f64,
    latency_ms: u64,
    success: bool,
    model: &str,
) {
    if let Ok(mut guard) = RUNTIME_STATE.lock() {
        if let Some(ref mut state) = *guard {
            state.turns_completed += 1;
            state.tokens_used += tokens;
            state.cost_usd += cost_usd;
            state.avg_turn_latency_ms = if state.turns_completed == 1 {
                latency_ms
            } else {
                (state.avg_turn_latency_ms * (state.turns_completed - 1) as u64 + latency_ms)
                    / state.turns_completed as u64
            };
            if !success {
                state.errors += 1;
            }
            state.current_model = Some(model.to_string());
            state.confidence_score = compute_confidence(state);
        }
    }
}

pub fn get_state() -> Option<RuntimeState> {
    RUNTIME_STATE.lock().ok()?.clone()
}

pub fn reset_state() {
    if let Ok(mut guard) = RUNTIME_STATE.lock() {
        *guard = Some(RuntimeState::default());
    }
}

impl RuntimeOptimizer {
    pub fn should_adjust(
        profile: &TaskProfile,
        plan: &ExecutionPlan,
        state: &RuntimeState,
    ) -> RuntimeAdjustment {
        let mut adjustment = RuntimeAdjustment {
            switch_model_to: None,
            reduce_context: false,
            increase_reasoning: false,
            decrease_reasoning: false,
            abort: false,
            reason: String::new(),
        };

        if state.errors > 3 {
            adjustment.abort = true;
            adjustment.reason = format!(
                "Too many errors ({}) — aborting to prevent waste",
                state.errors
            );
            return adjustment;
        }

        let token_budget = plan.token_budget.max(1);
        let token_ratio = state.tokens_used as f64 / token_budget as f64;

        if token_ratio > 0.9 {
            adjustment.reduce_context = true;
            adjustment.reason = format!(
                "Token usage at {:.0}% of budget — reducing context",
                token_ratio * 100.0
            );
        }

        if token_ratio > 0.8 && !state.degradation_triggered {
            let cheaper = super::planner::CognitivePlanner::should_degrade_model(
                profile,
                state.tokens_used,
                state.cost_usd,
                state.turns_completed,
            );
            if let Some(model) = cheaper {
                adjustment.switch_model_to = Some(model);
                adjustment.decrease_reasoning = true;
                adjustment.reason = format!(
                    "Token budget {:.0}% consumed — switching to cheaper model",
                    token_ratio * 100.0
                );
            }
        }

        if state.confidence_score < 0.3 && state.turns_completed > 3 {
            adjustment.increase_reasoning = true;
            if adjustment.reason.is_empty() {
                adjustment.reason = format!(
                    "Low confidence ({:.2}) — increasing reasoning depth",
                    state.confidence_score
                );
            }
        }

        if plan.max_turns > 0 && state.turns_completed >= plan.max_turns {
            adjustment.abort = true;
            adjustment.reason = format!(
                "Max turns ({}) reached — stopping",
                plan.max_turns
            );
        }

        adjustment
    }
}

fn compute_confidence(state: &RuntimeState) -> f32 {
    if state.turns_completed == 0 {
        return 0.5;
    }
    let success_rate = 1.0 - (state.errors as f32 / state.turns_completed as f32);
    let budget_efficiency = if state.tokens_used > 0 {
        let budget = 100_000.0f32;
        1.0 - (state.tokens_used as f32 / budget).min(1.0)
    } else {
        1.0
    };
    (success_rate * 0.7 + budget_efficiency * 0.3).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cognitive::analyzer::TaskAnalyzer;

    fn profile_and_plan(prompt: &str) -> (TaskProfile, ExecutionPlan) {
        let input = crate::cognitive::analyzer::TaskAnalysisInput {
            prompt: prompt.to_string(),
            tool_count: 5,
            available_models: vec![
                "claude-opus-4-6".to_string(),
                "claude-sonnet-4-6".to_string(),
                "claude-haiku-4-5".to_string(),
            ],
            has_codebase_context: true,
            estimated_input_tokens: 15000,
            session_turn_count: 3,
            is_subagent: false,
            cost_budget_remaining: Some(5.0),
            latency_sensitive: false,
            security_sensitive: false,
        };
        let profile = TaskAnalyzer::analyze(&input);
        let plan = crate::cognitive::planner::CognitivePlanner::plan(
            &profile,
            &input.available_models,
        );
        (profile, plan)
    }

    #[test]
    fn test_no_adjustment_on_healthy_run() {
        let (profile, plan) = profile_and_plan("implement a feature");
        let state = RuntimeState {
            turns_completed: 2,
            tokens_used: 5000,
            cost_usd: 0.01,
            errors: 0,
            ..Default::default()
        };
        let adj = RuntimeOptimizer::should_adjust(&profile, &plan, &state);
        assert!(!adj.abort);
    }

    #[test]
    fn test_abort_on_too_many_errors() {
        let (profile, plan) = profile_and_plan("do something");
        let state = RuntimeState {
            turns_completed: 5,
            tokens_used: 5000,
            cost_usd: 0.01,
            errors: 4,
            ..Default::default()
        };
        let adj = RuntimeOptimizer::should_adjust(&profile, &plan, &state);
        assert!(adj.abort);
    }

    #[test]
    fn test_context_reduction_near_budget() {
        let (profile, plan) = profile_and_plan("complex task");
        let state = RuntimeState {
            turns_completed: 5,
            tokens_used: plan.token_budget * 92 / 100,
            cost_usd: 0.1,
            errors: 0,
            ..Default::default()
        };
        let adj = RuntimeOptimizer::should_adjust(&profile, &plan, &state);
        assert!(adj.reduce_context);
    }
}
