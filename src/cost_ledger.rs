use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEntry {
    pub ts: DateTime<Utc>,
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub agent: String,
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostSummary {
    pub total_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub by_provider: Vec<ProviderCost>,
    pub by_session: Vec<SessionCost>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderCost {
    pub provider: String,
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionCost {
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub agent: String,
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

static LEDGER: Mutex<Option<CostLedger>> = Mutex::new(None);

pub fn init() {
    if let Ok(mut guard) = LEDGER.lock() {
        if guard.is_none() {
            if let Ok(ledger) = CostLedger::open() {
                *guard = Some(ledger);
            }
        }
    }
}

pub fn record(
    session_id: &str,
    parent_session_id: Option<&str>,
    agent: &str,
    provider: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    cost_usd: f64,
) {
    if let Ok(mut guard) = LEDGER.lock() {
        if guard.is_none() {
            if let Ok(ledger) = CostLedger::open() {
                *guard = Some(ledger);
            }
        }
        if let Some(ref mut ledger) = *guard {
            let entry = CostEntry {
                ts: Utc::now(),
                session_id: session_id.to_string(),
                parent_session_id: parent_session_id.map(|s| s.to_string()),
                agent: agent.to_string(),
                provider: provider.to_string(),
                model: model.to_string(),
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                cost_usd,
            };
            if let Err(e) = ledger.append(&entry) {
                crate::logging::warn(&format!("cost_ledger write failed: {}", e));
            }
        }
    }
}

pub fn query_summary(since: Option<DateTime<Utc>>, session_filter: Option<&str>) -> CostSummary {
    let entries = read_entries().unwrap_or_default();
    let filtered: Vec<&CostEntry> = entries
        .iter()
        .filter(|e| since.map_or(true, |s| e.ts >= s))
        .filter(|e| {
            session_filter.map_or(true, |f| {
                e.session_id == f || e.parent_session_id.as_deref() == Some(f)
            })
        })
        .collect();

    let mut summary = CostSummary::default();
    let mut provider_map: std::collections::HashMap<String, ProviderCost> =
        std::collections::HashMap::new();
    let mut session_map: std::collections::HashMap<String, SessionCost> =
        std::collections::HashMap::new();

    for entry in &filtered {
        summary.total_usd += entry.cost_usd;
        summary.input_tokens += entry.input_tokens;
        summary.output_tokens += entry.output_tokens;
        summary.cache_read_tokens += entry.cache_read_tokens;
        summary.cache_write_tokens += entry.cache_write_tokens;

        let pc = provider_map.entry(entry.provider.clone()).or_default();
        pc.provider = entry.provider.clone();
        pc.cost_usd += entry.cost_usd;
        pc.input_tokens += entry.input_tokens;
        pc.output_tokens += entry.output_tokens;

        let sc = session_map.entry(entry.session_id.clone()).or_default();
        sc.session_id = entry.session_id.clone();
        sc.parent_session_id = entry.parent_session_id.clone();
        sc.agent = entry.agent.clone();
        sc.cost_usd += entry.cost_usd;
        sc.input_tokens += entry.input_tokens;
        sc.output_tokens += entry.output_tokens;
    }

    let mut providers: Vec<ProviderCost> = provider_map.into_values().collect();
    providers.sort_by(|a, b| b.cost_usd.partial_cmp(&a.cost_usd).unwrap_or(std::cmp::Ordering::Equal));
    summary.by_provider = providers;

    let mut sessions: Vec<SessionCost> = session_map.into_values().collect();
    sessions.sort_by(|a, b| b.cost_usd.partial_cmp(&a.cost_usd).unwrap_or(std::cmp::Ordering::Equal));
    summary.by_session = sessions;

    summary
}

pub fn today_cost() -> f64 {
    let today = Utc::now().date_naive();
    let entries = read_entries().unwrap_or_default();
    entries
        .iter()
        .filter(|e| e.ts.date_naive() == today)
        .map(|e| e.cost_usd)
        .sum()
}

pub fn week_cost() -> f64 {
    let since = Utc::now() - chrono::Duration::days(7);
    let entries = read_entries().unwrap_or_default();
    entries
        .iter()
        .filter(|e| e.ts >= since)
        .map(|e| e.cost_usd)
        .sum()
}

pub fn month_cost() -> f64 {
    let since = Utc::now() - chrono::Duration::days(30);
    let entries = read_entries().unwrap_or_default();
    entries
        .iter()
        .filter(|e| e.ts >= since)
        .map(|e| e.cost_usd)
        .sum()
}

struct CostLedger {
    file: File,
}

impl CostLedger {
    fn open() -> Result<Self> {
        let path = ledger_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self { file })
    }

    fn append(&mut self, entry: &CostEntry) -> Result<()> {
        let mut line = serde_json::to_string(entry)?;
        line.push('\n');
        self.file.write_all(line.as_bytes())?;
        Ok(())
    }
}

fn ledger_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".jcode").join("usage").join("costs.jsonl")
}

fn read_entries() -> Result<Vec<CostEntry>> {
    let path = ledger_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)?;
    let entries = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<CostEntry>(l).ok())
        .collect();
    Ok(entries)
}

pub fn prune_older_than(days: u64) -> Result<u64> {
    let entries = read_entries()?;
    let cutoff = Utc::now() - chrono::Duration::days(days as i64);
    let (keep, remove): (Vec<_>, Vec<_>) = entries
        .into_iter()
        .partition(|e| e.ts >= cutoff);
    let removed = remove.len() as u64;
    if removed > 0 {
        let path = ledger_path();
        let mut file = File::create(&path)?;
        for entry in &keep {
            let mut line = serde_json::to_string(entry)?;
            line.push('\n');
            file.write_all(line.as_bytes())?;
        }
    }
    Ok(removed)
}

pub fn compute_cost(
    provider: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
) -> f64 {
    let (input_price, output_price, cache_read_price) = get_pricing(provider, model);

    let input_cost = (input_tokens as f64 / 1_000_000.0) * input_price;
    let output_cost = (output_tokens as f64 / 1_000_000.0) * output_price;
    let cache_read_cost = (cache_read_tokens as f64 / 1_000_000.0) * cache_read_price;
    let cache_write_cost = (cache_write_tokens as f64 / 1_000_000.0) * input_price * 1.25;

    input_cost + output_cost + cache_read_cost + cache_write_cost
}

fn get_pricing(provider: &str, model: &str) -> (f64, f64, f64) {
    let provider_lower = provider.to_lowercase();
    let model_lower = model.to_lowercase();

    if provider_lower.contains("anthropic") || provider_lower.contains("claude") {
        let base = model_lower.strip_suffix("[1m]").unwrap_or(&model_lower);
        match base {
            m if m.contains("opus-4-6") => (5.0, 25.0, 0.5),
            m if m.contains("sonnet-4-6") => (3.0, 15.0, 0.3),
            m if m.contains("haiku") => (1.0, 5.0, 0.1),
            m if m.contains("opus") => (5.0, 25.0, 0.5),
            m if m.contains("sonnet") => (3.0, 15.0, 0.3),
            _ => (3.0, 15.0, 0.3),
        }
    } else if provider_lower.contains("openai") || provider_lower.contains("copilot") {
        if model_lower.contains("gpt-5.5") || model_lower.contains("gpt-5.4") {
            (2.5, 10.0, 1.25)
        } else if model_lower.contains("o3") || model_lower.contains("o4") {
            (2.0, 8.0, 1.0)
        } else if model_lower.contains("gpt-4") {
            (30.0, 60.0, 15.0)
        } else if model_lower.contains("mini") {
            (0.15, 0.6, 0.075)
        } else {
            (2.5, 10.0, 1.25)
        }
    } else if provider_lower.contains("gemini") || provider_lower.contains("google") {
        if model_lower.contains("2.5-pro") {
            (1.25, 10.0, 0.315)
        } else if model_lower.contains("2.5-flash") {
            (0.15, 0.6, 0.0375)
        } else {
            (1.25, 5.0, 0.315)
        }
    } else if provider_lower.contains("openrouter") {
        (3.0, 15.0, 0.3)
    } else if provider_lower.contains("bedrock") {
        (3.0, 15.0, 0.3)
    } else {
        (3.0, 15.0, 0.3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_cost_anthropic_opus() {
        let cost = compute_cost("anthropic", "claude-opus-4-6", 1000, 500, 0, 0);
        assert!(cost > 0.0);
        let expected = (1000.0 / 1_000_000.0) * 5.0 + (500.0 / 1_000_000.0) * 25.0;
        assert!((cost - expected).abs() < 0.0001);
    }

    #[test]
    fn test_compute_cost_openai() {
        let cost = compute_cost("openai", "gpt-5.4", 1000, 500, 0, 0);
        assert!(cost > 0.0);
    }

    #[test]
    fn test_compute_cost_with_cache() {
        let cost_no_cache = compute_cost("anthropic", "claude-sonnet-4-6", 10000, 1000, 0, 0);
        let cost_with_cache = compute_cost("anthropic", "claude-sonnet-4-6", 1000, 1000, 9000, 0);
        assert!(cost_with_cache < cost_no_cache);
    }

    #[test]
    fn test_get_pricing_unknown_model() {
        let (i, o, c) = get_pricing("unknown", "unknown-model");
        assert!(i > 0.0 && o > 0.0);
    }
}
