use anyhow::Result;
use async_trait::async_trait;
use jcode_message_types::{Message, StreamEvent, ToolDefinition};
use jcode_provider_core::{
    EventStream, NativeCompactionResult, NativeToolResultSender, PremiumMode,
    Provider,
};
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::config;

static SUBAGENT_TPM_SEMAPHORE: std::sync::OnceLock<Option<Semaphore>> = std::sync::OnceLock::new();

fn tpm_semaphore() -> Option<&'static Semaphore> {
    SUBAGENT_TPM_SEMAPHORE
        .get_or_init(|| {
            let cfg = config::config();
            cfg.agents.subagent_tokens_per_minute.map(|limit| {
                let permits = (limit / 1000).max(1) as usize;
                Semaphore::new(permits)
            })
        })
        .as_ref()
}

static SUBAGENT_BUDGET_STATE: std::sync::OnceLock<SubagentBudgetState> = std::sync::OnceLock::new();

struct SubagentBudgetState {
    budget: Option<u64>,
    window_secs: u64,
    exceeded_policy: String,
}

fn budget_state() -> &'static SubagentBudgetState {
    SUBAGENT_BUDGET_STATE.get_or_init(|| {
        let cfg = config::config();
        SubagentBudgetState {
            budget: cfg.agents.subagent_rolling_token_budget,
            window_secs: parse_window(cfg.agents.subagent_rolling_token_window.as_deref()),
            exceeded_policy: cfg
                .agents
                .subagent_budget_exceeded
                .clone()
                .unwrap_or_else(|| "wait".to_string()),
        }
    })
}

fn parse_window(window: Option<&str>) -> u64 {
    match window {
        Some(s) if s.ends_with('h') => {
            s.trim_end_matches('h').parse::<u64>().unwrap_or(4) * 3600
        }
        Some(s) if s.ends_with('d') => {
            s.trim_end_matches('d').parse::<u64>().unwrap_or(1) * 86400
        }
        Some(s) if s.ends_with('m') => {
            s.trim_end_matches('m').parse::<u64>().unwrap_or(60) * 60
        }
        _ => 4 * 3600,
    }
}

pub struct SubagentRateLimitedProvider {
    inner: Arc<dyn Provider>,
}

impl SubagentRateLimitedProvider {
    pub fn wrap(inner: Arc<dyn Provider>) -> Arc<dyn Provider> {
        Arc::new(Self { inner })
    }
}

#[async_trait]
impl Provider for SubagentRateLimitedProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: &str,
        resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        self.acquire_budget(messages, tools, system, "").await?;
        self.inner
            .complete(messages, tools, system, resume_session_id)
            .await
    }

    async fn complete_split(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system_static: &str,
        system_dynamic: &str,
        resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        self.acquire_budget(messages, tools, system_static, system_dynamic)
            .await?;
        self.inner
            .complete_split(messages, tools, system_static, system_dynamic, resume_session_id)
            .await
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn model(&self) -> String {
        self.inner.model()
    }

    fn supports_image_input(&self) -> bool {
        self.inner.supports_image_input()
    }

    fn set_model(&self, model: &str) -> Result<()> {
        self.inner.set_model(model)
    }

    fn available_models(&self) -> Vec<&'static str> {
        self.inner.available_models()
    }

    fn available_models_display(&self) -> Vec<String> {
        self.inner.available_models_display()
    }

    fn available_models_for_switching(&self) -> Vec<String> {
        self.inner.available_models_for_switching()
    }

    fn available_providers_for_model(&self, model: &str) -> Vec<String> {
        self.inner.available_providers_for_model(model)
    }

    fn provider_details_for_model(&self, model: &str) -> Vec<(String, String)> {
        self.inner.provider_details_for_model(model)
    }

    fn preferred_provider(&self) -> Option<String> {
        self.inner.preferred_provider()
    }

    fn model_routes(&self) -> Vec<jcode_provider_core::ModelRoute> {
        self.inner.model_routes()
    }

    async fn prefetch_models(&self) -> Result<()> {
        self.inner.prefetch_models().await
    }

    async fn refresh_model_catalog(&self) -> Result<jcode_provider_core::ModelCatalogRefreshSummary> {
        self.inner.refresh_model_catalog().await
    }

    fn on_auth_changed(&self) {
        self.inner.on_auth_changed();
    }

    fn reasoning_effort(&self) -> Option<String> {
        self.inner.reasoning_effort()
    }

    fn set_reasoning_effort(&self, effort: &str) -> Result<()> {
        self.inner.set_reasoning_effort(effort)
    }

    fn available_efforts(&self) -> Vec<&'static str> {
        self.inner.available_efforts()
    }

    fn service_tier(&self) -> Option<String> {
        self.inner.service_tier()
    }

    fn set_service_tier(&self, service_tier: &str) -> Result<()> {
        self.inner.set_service_tier(service_tier)
    }

    fn available_service_tiers(&self) -> Vec<&'static str> {
        self.inner.available_service_tiers()
    }

    fn native_compaction_mode(&self) -> Option<String> {
        self.inner.native_compaction_mode()
    }

    fn native_compaction_threshold_tokens(&self) -> Option<usize> {
        self.inner.native_compaction_threshold_tokens()
    }

    fn transport(&self) -> Option<String> {
        self.inner.transport()
    }

    fn set_transport(&self, transport: &str) -> Result<()> {
        self.inner.set_transport(transport)
    }

    fn available_transports(&self) -> Vec<&'static str> {
        self.inner.available_transports()
    }

    fn handles_tools_internally(&self) -> bool {
        self.inner.handles_tools_internally()
    }

    async fn invalidate_credentials(&self) {
        self.inner.invalidate_credentials().await
    }

    fn set_premium_mode(&self, mode: PremiumMode) {
        self.inner.set_premium_mode(mode)
    }

    fn premium_mode(&self) -> PremiumMode {
        self.inner.premium_mode()
    }

    fn supports_compaction(&self) -> bool {
        self.inner.supports_compaction()
    }

    fn uses_jcode_compaction(&self) -> bool {
        self.inner.uses_jcode_compaction()
    }

    async fn native_compact(
        &self,
        messages: &[Message],
        existing_summary_text: Option<&str>,
        existing_openai_encrypted_content: Option<&str>,
    ) -> Result<NativeCompactionResult> {
        self.inner
            .native_compact(messages, existing_summary_text, existing_openai_encrypted_content)
            .await
    }

    fn context_window(&self) -> usize {
        self.inner.context_window()
    }

    fn fork(&self) -> Arc<dyn Provider> {
        let forked = self.inner.fork();
        Arc::new(Self { inner: forked })
    }

    fn native_result_sender(&self) -> Option<NativeToolResultSender> {
        self.inner.native_result_sender()
    }

    fn drain_startup_notices(&self) -> Vec<String> {
        self.inner.drain_startup_notices()
    }
}

impl SubagentRateLimitedProvider {
    async fn acquire_budget(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system_static: &str,
        _system_dynamic: &str,
    ) -> Result<()> {
        if let Some(sem) = tpm_semaphore() {
            let _permit = sem.acquire().await;
        }

        let state = budget_state();
        if let Some(budget) = state.budget {
            let summary = crate::cost_ledger::query_summary(
                Some(chrono::Utc::now() - chrono::Duration::seconds(state.window_secs as i64)),
                None,
            );
            let subagent_tokens = summary.input_tokens + summary.output_tokens;
            if subagent_tokens >= budget {
                match state.exceeded_policy.as_str() {
                    "reject" => {
                        return Err(anyhow::anyhow!(
                            "Sub-agent token budget exceeded: {} used of {} limit ({}h window)",
                            subagent_tokens,
                            budget,
                            state.window_secs / 3600
                        ));
                    }
                    "degrade" => {
                        crate::logging::info(&format!(
                            "Sub-agent budget near limit ({} / {}), but degrade policy allows continuation",
                            subagent_tokens, budget
                        ));
                    }
                    _ => {
                        let wait_tokens = subagent_tokens - budget;
                        let estimated_wait_secs = (wait_tokens as f64 / budget as f64
                            * state.window_secs as f64)
                            .min(300.0);
                        crate::logging::info(&format!(
                            "Sub-agent budget exceeded ({} / {}), waiting up to {:.0}s for window to roll",
                            subagent_tokens, budget, estimated_wait_secs
                        ));
                        tokio::time::sleep(std::time::Duration::from_secs(
                            estimated_wait_secs as u64
                        ))
                        .await;
                    }
                }
            }
        }

        Ok(())
    }
}
