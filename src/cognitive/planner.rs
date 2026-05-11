use super::analyzer::{
    CognitiveRole, ContextAllocation, ExecutionStrategy, ModelRecommendation, ReasoningDepth,
    TaskAnalysisInput, TaskProfile,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub strategy: ExecutionStrategy,
    pub primary_model: String,
    pub primary_provider: String,
    pub validation_model: Option<String>,
    pub reasoning_depth: ReasoningDepth,
    pub context_allocation: ContextAllocation,
    pub token_budget: u64,
    pub max_turns: u32,
    pub use_compaction: bool,
    pub compaction_mode: Option<String>,
    pub use_subagents: bool,
    pub subagent_count_hint: u32,
    pub estimated_cost_usd: f64,
    pub estimated_latency_ms: u64,
    pub cost_optimized: bool,
}

pub struct CognitivePlanner;

impl CognitivePlanner {
    pub fn plan(profile: &TaskProfile, available_models: &[String]) -> ExecutionPlan {
        let (primary, validation) = Self::select_models(profile, available_models);
        let max_turns = Self::determine_max_turns(profile);
        let compaction = Self::determine_compaction(profile);
        let (use_subagents, subagent_count) = Self::determine_subagent_usage(profile);
        let estimated_cost = primary
            .as_ref()
            .map(|m| m.estimated_cost_usd)
            .unwrap_or(0.0)
            + validation
                .as_ref()
                .map(|m: &ModelRecommendation| m.estimated_cost_usd)
                .unwrap_or(0.0);

        let estimated_latency = primary
            .as_ref()
            .map(|m| m.estimated_latency_ms)
            .unwrap_or(5000);

        let (primary_model, primary_provider) = primary
            .map(|m| (m.model.clone(), m.provider.clone()))
            .unwrap_or_else(|| {
                let fallback = available_models.first().cloned().unwrap_or_default();
                (fallback, "unknown".to_string())
            });

        let cost_optimized = estimated_cost < Self::cost_threshold_for_strategy(&profile.execution_strategy);

        ExecutionPlan {
            strategy: profile.execution_strategy.clone(),
            primary_model,
            primary_provider,
            validation_model: validation.map(|m| m.model),
            reasoning_depth: profile.reasoning_depth.clone(),
            context_allocation: profile.context_allocation.clone(),
            token_budget: profile.estimated_token_budget,
            max_turns,
            use_compaction: compaction.0,
            compaction_mode: compaction.1,
            use_subagents,
            subagent_count_hint: subagent_count,
            estimated_cost_usd: estimated_cost,
            estimated_latency_ms: estimated_latency,
            cost_optimized,
        }
    }

    fn select_models(
        profile: &TaskProfile,
        available_models: &[String],
    ) -> (Option<ModelRecommendation>, Option<ModelRecommendation>) {
        if profile.recommended_models.is_empty() {
            return (None, None);
        }

        let primary = profile.recommended_models.first().cloned();

        let validation = match &profile.execution_strategy {
            ExecutionStrategy::SingleModelValidated
            | ExecutionStrategy::MultiStageValidation => {
                profile
                    .recommended_models
                    .iter()
                    .find(|m| m.role == CognitiveRole::Reasoning || m.role == CognitiveRole::Architecture)
                    .filter(|m| m.model != primary.as_ref().map(|p| p.model.as_str()).unwrap_or(""))
                    .cloned()
                    .or_else(|| profile.recommended_models.get(1).cloned())
            }
            _ => None,
        };

        (primary, validation)
    }

    fn determine_max_turns(profile: &TaskProfile) -> u32 {
        match profile.complexity_score {
            0..=25 => 5,
            26..=50 => 15,
            51..=75 => 30,
            _ => 50,
        }
    }

    fn determine_compaction(profile: &TaskProfile) -> (bool, Option<String>) {
        match profile.context_allocation {
            ContextAllocation::Minimal => (true, Some("proactive".to_string())),
            ContextAllocation::Standard => (true, Some("reactive".to_string())),
            ContextAllocation::Extended => (true, Some("proactive".to_string())),
            ContextAllocation::Full => (true, Some("semantic".to_string())),
        }
    }

    fn determine_subagent_usage(profile: &TaskProfile) -> (bool, u32) {
        if !profile.coordination_required {
            return (false, 0);
        }
        let count = match profile.complexity_score {
            0..=40 => 2,
            41..=70 => 3,
            _ => 5,
        };
        (true, count)
    }

    fn cost_threshold_for_strategy(strategy: &ExecutionStrategy) -> f64 {
        match strategy {
            ExecutionStrategy::Direct => 0.01,
            ExecutionStrategy::SingleModelValidated => 0.05,
            ExecutionStrategy::MultiStageValidation => 0.10,
            ExecutionStrategy::Decomposed => 0.25,
            ExecutionStrategy::ParallelReasoning => 0.15,
        }
    }

    pub fn should_degrade_model(
        profile: &TaskProfile,
        tokens_used: u64,
        _cost_used: f64,
        _turns_used: u32,
    ) -> Option<String> {
        let token_ratio = tokens_used as f64 / profile.estimated_token_budget.max(1) as f64;
        if token_ratio > 0.8 && !profile.recommended_models.is_empty() {
            let cheaper = profile
                .recommended_models
                .iter()
                .filter(|m| {
                    m.role == CognitiveRole::FastExecution
                        || m.role == CognitiveRole::Classification
                })
                .min_by(|a, b| a.estimated_cost_usd.partial_cmp(&b.estimated_cost_usd).unwrap_or(std::cmp::Ordering::Equal));
            return cheaper.map(|m| m.model.clone());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cognitive::analyzer::TaskAnalyzer;

    fn plan_for(prompt: &str) -> ExecutionPlan {
        let input = TaskAnalysisInput {
            prompt: prompt.to_string(),
            tool_count: 5,
            available_models: vec![
                "claude-opus-4-6".to_string(),
                "claude-sonnet-4-6".to_string(),
                "claude-haiku-4-5".to_string(),
                "gpt-5.4".to_string(),
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
        CognitivePlanner::plan(&profile, &input.available_models)
    }

    #[test]
    fn test_simple_task_uses_few_turns() {
        let plan = plan_for("list files in src/");
        assert!(plan.max_turns <= 10);
    }

    #[test]
    fn test_complex_task_uses_more_turns() {
        let plan = plan_for("architect and implement a distributed system");
        assert!(plan.max_turns >= 20);
    }

    #[test]
    fn test_compaction_always_enabled() {
        let plan = plan_for("do something");
        assert!(plan.use_compaction);
    }

    #[test]
    fn test_degradation_when_over_budget() {
        let input = TaskAnalysisInput {
            prompt: "complex architecture task".to_string(),
            tool_count: 5,
            available_models: vec![
                "claude-opus-4-6".to_string(),
                "claude-haiku-4-5".to_string(),
            ],
            has_codebase_context: true,
            estimated_input_tokens: 10000,
            session_turn_count: 3,
            is_subagent: false,
            cost_budget_remaining: Some(1.0),
            latency_sensitive: false,
            security_sensitive: false,
        };
        let profile = TaskAnalyzer::analyze(&input);
        let cheaper = CognitivePlanner::should_degrade_model(&profile, 9000, 0.05, 5);
        assert!(cheaper.is_some());
    }
}
