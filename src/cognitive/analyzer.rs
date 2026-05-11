use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProfile {
    pub complexity_score: u8,
    pub uncertainty_score: u8,
    pub risk_score: u8,
    pub coordination_required: bool,
    pub recommended_models: Vec<ModelRecommendation>,
    pub estimated_token_budget: u64,
    pub execution_strategy: ExecutionStrategy,
    pub reasoning_depth: ReasoningDepth,
    pub context_allocation: ContextAllocation,
    pub confidence_requirement: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRecommendation {
    pub model: String,
    pub provider: String,
    pub role: CognitiveRole,
    pub estimated_cost_usd: f64,
    pub estimated_latency_ms: u64,
    pub suitability_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionStrategy {
    Direct,
    SingleModelValidated,
    MultiStageValidation,
    Decomposed,
    ParallelReasoning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReasoningDepth {
    Minimal,
    Standard,
    Deep,
    Extended,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ContextAllocation {
    Minimal,
    Standard,
    Extended,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CognitiveRole {
    Classification,
    FastExecution,
    Reasoning,
    Architecture,
    Implementation,
    Validation,
    Research,
    LongContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAnalysisInput {
    pub prompt: String,
    pub tool_count: usize,
    pub available_models: Vec<String>,
    pub has_codebase_context: bool,
    pub estimated_input_tokens: u64,
    pub session_turn_count: u32,
    pub is_subagent: bool,
    pub cost_budget_remaining: Option<f64>,
    pub latency_sensitive: bool,
    pub security_sensitive: bool,
}

pub struct TaskAnalyzer;

impl TaskAnalyzer {
    pub fn analyze(input: &TaskAnalysisInput) -> TaskProfile {
        let complexity = Self::score_complexity(input);
        let uncertainty = Self::score_uncertainty(input);
        let risk = Self::score_risk(input);
        let coordination = Self::needs_coordination(input, complexity);
        let strategy = Self::determine_strategy(complexity, uncertainty, coordination);
        let depth = Self::determine_reasoning_depth(complexity, risk);
        let context = Self::determine_context_allocation(complexity, input);
        let token_budget = Self::estimate_token_budget(complexity, input);
        let confidence = Self::determine_confidence_requirement(risk, complexity);
        let models = Self::recommend_models(input, complexity, &strategy);

        TaskProfile {
            complexity_score: complexity,
            uncertainty_score: uncertainty,
            risk_score: risk,
            coordination_required: coordination,
            recommended_models: models,
            estimated_token_budget: token_budget,
            execution_strategy: strategy,
            reasoning_depth: depth,
            context_allocation: context,
            confidence_requirement: confidence,
        }
    }

    fn score_complexity(input: &TaskAnalysisInput) -> u8 {
        let mut score: u8 = 10;

        let prompt_lower = input.prompt.to_lowercase();
        let prompt_len = input.prompt.len();

        if prompt_len > 2000 {
            score = score.saturating_add(15);
        } else if prompt_len > 500 {
            score = score.saturating_add(8);
        }

        let complexity_signals = [
            "architect", "design", "refactor", "migrate", "security",
            "optimize", "debug", "trace", "analyze", "investigate",
            "implement", "build", "create", "integrate", "coordinate",
        ];
        let simplicity_signals = [
            "list", "show", "read", "cat", "grep", "find", "what is",
            "echo", "print", "count", "check if",
        ];

        for signal in &complexity_signals {
            if prompt_lower.contains(signal) {
                score = score.saturating_add(8);
            }
        }
        for signal in &simplicity_signals {
            if prompt_lower.contains(signal) {
                score = score.saturating_sub(5);
            }
        }

        if input.tool_count > 5 {
            score = score.saturating_add(10);
        } else if input.tool_count > 2 {
            score = score.saturating_add(5);
        }

        if input.estimated_input_tokens > 50_000 {
            score = score.saturating_add(12);
        } else if input.estimated_input_tokens > 10_000 {
            score = score.saturating_add(6);
        }

        if input.has_codebase_context {
            score = score.saturating_add(5);
        }

        if prompt_lower.contains("subagent") || prompt_lower.contains("parallel") {
            score = score.saturating_add(10);
        }

        if prompt_lower.contains("explain") || prompt_lower.contains("summarize") {
            score = score.saturating_sub(8);
        }

        score.min(100)
    }

    fn score_uncertainty(input: &TaskAnalysisInput) -> u8 {
        let mut score: u8 = 20;

        let prompt_lower = input.prompt.to_lowercase();

        let uncertainty_signals = [
            "maybe", "might", "could be", "not sure", "unclear",
            "unknown", "figure out", "explore", "investigate",
            "what's causing", "why does", "don't understand",
        ];
        for signal in &uncertainty_signals {
            if prompt_lower.contains(signal) {
                score = score.saturating_add(15);
            }
        }

        let certainty_signals = [
            "exactly", "precisely", "change this to", "replace",
            "rename", "move", "delete", "add import",
        ];
        for signal in &certainty_signals {
            if prompt_lower.contains(signal) {
                score = score.saturating_sub(10);
            }
        }

        if !input.has_codebase_context {
            score = score.saturating_add(10);
        }

        if input.session_turn_count < 3 {
            score = score.saturating_add(5);
        }

        score.min(100)
    }

    fn score_risk(input: &TaskAnalysisInput) -> u8 {
        let mut score: u8 = 15;

        if input.security_sensitive {
            score = score.saturating_add(40);
        }

        let prompt_lower = input.prompt.to_lowercase();
        let risk_signals = [
            "production", "deploy", "database", "migration",
            "credentials", "secret", "password", "api key",
            "drop", "delete all", "truncate", "rm -rf",
            "security", "vulnerability", "exploit", "auth",
        ];
        for signal in &risk_signals {
            if prompt_lower.contains(signal) {
                score = score.saturating_add(10);
            }
        }

        let safe_signals = [
            "test", "spec", "mock", "fixture", "unit test",
            "read-only", "dry run", "preview",
        ];
        for signal in &safe_signals {
            if prompt_lower.contains(signal) {
                score = score.saturating_sub(10);
            }
        }

        score.min(100)
    }

    fn needs_coordination(input: &TaskAnalysisInput, complexity: u8) -> bool {
        if complexity > 70 {
            return true;
        }
        let prompt_lower = input.prompt.to_lowercase();
        prompt_lower.contains("subagent")
            || prompt_lower.contains("parallel")
            || prompt_lower.contains("multiple agents")
            || prompt_lower.contains("coordinate")
            || prompt_lower.contains("swarm")
    }

    fn determine_strategy(
        complexity: u8,
        uncertainty: u8,
        coordination: bool,
    ) -> ExecutionStrategy {
        if coordination && complexity > 70 {
            ExecutionStrategy::Decomposed
        } else if coordination {
            ExecutionStrategy::ParallelReasoning
        } else if complexity > 60 && uncertainty > 50 {
            ExecutionStrategy::MultiStageValidation
        } else if complexity > 40 || uncertainty > 40 {
            ExecutionStrategy::SingleModelValidated
        } else {
            ExecutionStrategy::Direct
        }
    }

    fn determine_reasoning_depth(complexity: u8, risk: u8) -> ReasoningDepth {
        let combined = (complexity as u16 + risk as u16) / 2;
        match combined {
            0..=25 => ReasoningDepth::Minimal,
            26..=50 => ReasoningDepth::Standard,
            51..=75 => ReasoningDepth::Deep,
            _ => ReasoningDepth::Extended,
        }
    }

    fn determine_context_allocation(complexity: u8, input: &TaskAnalysisInput) -> ContextAllocation {
        if input.estimated_input_tokens > 100_000 {
            ContextAllocation::Full
        } else if complexity > 70 {
            ContextAllocation::Extended
        } else if complexity > 40 {
            ContextAllocation::Standard
        } else {
            ContextAllocation::Minimal
        }
    }

    fn estimate_token_budget(complexity: u8, input: &TaskAnalysisInput) -> u64 {
        let base = input.estimated_input_tokens;
        let multiplier = match complexity {
            0..=25 => 1.2,
            26..=50 => 1.5,
            51..=75 => 2.0,
            _ => 3.0,
        };
        let budget = (base as f64 * multiplier) as u64;
        budget.max(4_000).min(200_000)
    }

    fn determine_confidence_requirement(risk: u8, complexity: u8) -> f32 {
        let base = 0.5 + (risk as f32 / 200.0) + (complexity as f32 / 300.0);
        base.min(0.99)
    }

    fn recommend_models(
        input: &TaskAnalysisInput,
        complexity: u8,
        strategy: &ExecutionStrategy,
    ) -> Vec<ModelRecommendation> {
        let mut recommendations = Vec::new();
        let prompt_lower = input.prompt.to_lowercase();

        let needs_reasoning = complexity > 50
            || prompt_lower.contains("architect")
            || prompt_lower.contains("design")
            || prompt_lower.contains("security")
            || prompt_lower.contains("debug");

        let needs_fast = complexity <= 30
            || prompt_lower.contains("list")
            || prompt_lower.contains("read")
            || prompt_lower.contains("grep")
            || prompt_lower.contains("find")
            || input.latency_sensitive;

        let needs_long_context = input.estimated_input_tokens > 80_000
            || prompt_lower.contains("entire codebase")
            || prompt_lower.contains("all files");

        let needs_implementation = prompt_lower.contains("implement")
            || prompt_lower.contains("write")
            || prompt_lower.contains("edit")
            || prompt_lower.contains("fix")
            || prompt_lower.contains("build");

        for model_id in &input.available_models {
            let model_lower = model_id.to_lowercase();

            let (role, suitability) = if needs_fast
                && (model_lower.contains("haiku")
                    || model_lower.contains("mini")
                    || model_lower.contains("flash")
                    || model_lower.contains("qwen")
                    || model_lower.contains("glm-4-flash"))
            {
                (CognitiveRole::FastExecution, 0.9)
            } else if needs_reasoning
                && (model_lower.contains("opus")
                    || model_lower.contains("o3")
                    || model_lower.contains("deepseek-r1")
                    || model_lower.contains("pro"))
            {
                (CognitiveRole::Reasoning, 0.95)
            } else if needs_long_context
                && (model_lower.contains("gemini") || model_lower.contains("[1m]"))
            {
                (CognitiveRole::LongContext, 0.85)
            } else if needs_implementation
                && (model_lower.contains("sonnet")
                    || model_lower.contains("gpt-5")
                    || model_lower.contains("deepseek-coder")
                    || model_lower.contains("codex"))
            {
                (CognitiveRole::Implementation, 0.9)
            } else if model_lower.contains("sonnet") || model_lower.contains("gpt-5") {
                (CognitiveRole::FastExecution, 0.7)
            } else {
                (CognitiveRole::Classification, 0.5)
            };

            let provider = Self::infer_provider(model_id);
            let estimated_cost = crate::cost_ledger::compute_cost(
                &provider,
                model_id,
                8000,
                2000,
                0,
                0,
            );
            let estimated_latency = Self::estimate_latency(model_id, complexity);

            recommendations.push(ModelRecommendation {
                model: model_id.clone(),
                provider,
                role,
                estimated_cost_usd: estimated_cost,
                estimated_latency_ms: estimated_latency,
                suitability_score: suitability,
            });
        }

        recommendations.sort_by(|a, b| {
            b.suitability_score
                .partial_cmp(&a.suitability_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        recommendations.truncate(5);
        recommendations
    }

    fn infer_provider(model_id: &str) -> String {
        let lower = model_id.to_lowercase();
        if lower.contains("claude") || lower.contains("opus") || lower.contains("sonnet") || lower.contains("haiku") {
            "anthropic".to_string()
        } else if lower.contains("gpt") || lower.contains("o3") || lower.contains("codex") {
            "openai".to_string()
        } else if lower.contains("gemini") {
            "google".to_string()
        } else if lower.contains("deepseek") {
            "deepseek".to_string()
        } else if lower.contains("qwen") || lower.contains("glm") {
            "openrouter".to_string()
        } else if lower.contains("copilot") {
            "copilot".to_string()
        } else if lower.contains("/") || lower.contains("@") {
            "openrouter".to_string()
        } else {
            "unknown".to_string()
        }
    }

    fn estimate_latency(model_id: &str, complexity: u8) -> u64 {
        let base = match complexity {
            0..=25 => 2000,
            26..=50 => 5000,
            51..=75 => 10000,
            _ => 20000,
        };
        let lower = model_id.to_lowercase();
        let multiplier = if lower.contains("haiku") || lower.contains("mini") || lower.contains("flash") {
            0.5
        } else if lower.contains("opus") || lower.contains("o3") {
            2.0
        } else {
            1.0
        };
        (base as f64 * multiplier) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_input(prompt: &str) -> TaskAnalysisInput {
        TaskAnalysisInput {
            prompt: prompt.to_string(),
            tool_count: 5,
            available_models: vec![
                "claude-opus-4-6".to_string(),
                "claude-sonnet-4-6".to_string(),
                "claude-haiku-4-5".to_string(),
                "gpt-5.4".to_string(),
                "gemini-2.5-pro".to_string(),
            ],
            has_codebase_context: true,
            estimated_input_tokens: 15000,
            session_turn_count: 5,
            is_subagent: false,
            cost_budget_remaining: Some(5.0),
            latency_sensitive: false,
            security_sensitive: false,
        }
    }

    #[test]
    fn test_simple_query_low_complexity() {
        let input = test_input("list the files in src/");
        let profile = TaskAnalyzer::analyze(&input);
        assert!(profile.complexity_score < 40);
        assert_eq!(profile.execution_strategy, ExecutionStrategy::Direct);
    }

    #[test]
    fn test_architecture_high_complexity() {
        let input = test_input("architect a new authentication system with OAuth2 and JWT tokens");
        let profile = TaskAnalyzer::analyze(&input);
        assert!(profile.complexity_score > 50);
        assert!(profile.reasoning_depth == ReasoningDepth::Deep || profile.reasoning_depth == ReasoningDepth::Extended);
    }

    #[test]
    fn test_security_sensitive_high_risk() {
        let mut input = test_input("review the security of our authentication");
        input.security_sensitive = true;
        let profile = TaskAnalyzer::analyze(&input);
        assert!(profile.risk_score > 50);
    }

    #[test]
    fn test_model_recommendations() {
        let input = test_input("read this file");
        let profile = TaskAnalyzer::analyze(&input);
        assert!(!profile.recommended_models.is_empty());
    }

    #[test]
    fn test_coordination_detection() {
        let input = test_input("use subagents to parallelize this refactoring across 5 modules");
        let profile = TaskAnalyzer::analyze(&input);
        assert!(profile.coordination_required);
    }

    #[test]
    fn test_token_budget_scales_with_complexity() {
        let simple = test_input("list files");
        let complex = test_input("architect and implement a distributed caching system with consistency guarantees");
        let simple_profile = TaskAnalyzer::analyze(&simple);
        let complex_profile = TaskAnalyzer::analyze(&complex);
        assert!(complex_profile.estimated_token_budget >= simple_profile.estimated_token_budget);
    }
}
