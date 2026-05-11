use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelPerformanceRecord {
    pub model: String,
    pub provider: String,
    pub total_calls: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost_usd: f64,
    pub successes: u64,
    pub failures: u64,
    pub total_latency_ms: u64,
    pub tasks_by_complexity_bucket: [u64; 4],
    pub success_by_complexity_bucket: [u64; 4],
}

impl ModelPerformanceRecord {
    pub fn success_rate(&self) -> f32 {
        if self.total_calls == 0 {
            return 0.0;
        }
        self.successes as f32 / self.total_calls as f32
    }

    pub fn avg_latency_ms(&self) -> u64 {
        if self.total_calls == 0 {
            return 0;
        }
        self.total_latency_ms / self.total_calls
    }

    pub fn cost_per_success(&self) -> f64 {
        if self.successes == 0 {
            return f64::MAX;
        }
        self.total_cost_usd / self.successes as f64
    }

    pub fn tokens_per_dollar(&self) -> f64 {
        if self.total_cost_usd == 0.0 {
            return 0.0;
        }
        (self.total_input_tokens + self.total_output_tokens) as f64 / self.total_cost_usd
    }

    pub fn roi_score(&self) -> f64 {
        let success_rate = self.success_rate() as f64;
        let cost_efficiency = self.tokens_per_dollar();
        if cost_efficiency == 0.0 {
            return 0.0;
        }
        success_rate * (cost_efficiency / 1000.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EconomicsSnapshot {
    pub total_cost_usd: f64,
    pub total_tokens: u64,
    pub total_calls: u64,
    pub total_successes: u64,
    pub total_failures: u64,
    pub avg_cost_per_task: f64,
    pub best_model_by_roi: Option<String>,
    pub cheapest_model: Option<String>,
    pub fastest_model: Option<String>,
    pub most_reliable_model: Option<String>,
    pub model_performance: HashMap<String, ModelPerformanceRecord>,
}

pub struct EconomicsEngine;

impl EconomicsEngine {
    pub fn compute_snapshot(records: &[ModelPerformanceRecord]) -> EconomicsSnapshot {
        let mut snapshot = EconomicsSnapshot::default();
        let mut perf_map: HashMap<String, ModelPerformanceRecord> = HashMap::new();

        for record in records {
            snapshot.total_cost_usd += record.total_cost_usd;
            snapshot.total_tokens += record.total_input_tokens + record.total_output_tokens;
            snapshot.total_calls += record.total_calls;
            snapshot.total_successes += record.successes;
            snapshot.total_failures += record.failures;
            perf_map.insert(record.model.clone(), record.clone());
        }

        if snapshot.total_calls > 0 {
            snapshot.avg_cost_per_task = snapshot.total_cost_usd / snapshot.total_calls as f64;
        }

        snapshot.best_model_by_roi = Self::find_best_by(&records, |r| r.roi_score());
        snapshot.cheapest_model = Self::find_best_by(&records, |r| 1.0 / r.cost_per_success().max(0.001));
        snapshot.fastest_model = Self::find_best_by(&records, |r| 1.0 / r.avg_latency_ms().max(1) as f64);
        snapshot.most_reliable_model = Self::find_best_by(&records, |r| r.success_rate() as f64);

        snapshot.model_performance = perf_map;
        snapshot
    }

    fn find_best_by<F: Fn(&ModelPerformanceRecord) -> f64>(
        records: &[ModelPerformanceRecord],
        scorer: F,
    ) -> Option<String> {
        records
            .iter()
            .filter(|r| r.total_calls >= 1)
            .max_by(|a, b| scorer(a).partial_cmp(&scorer(b)).unwrap_or(std::cmp::Ordering::Equal))
            .map(|r| r.model.clone())
    }

    pub fn complexity_bucket(complexity: u8) -> usize {
        match complexity {
            0..=25 => 0,
            26..=50 => 1,
            51..=75 => 2,
            _ => 3,
        }
    }

    pub fn recommend_model_for_complexity(
        records: &[ModelPerformanceRecord],
        complexity: u8,
    ) -> Option<String> {
        let bucket = Self::complexity_bucket(complexity);
        records
            .iter()
            .filter(|r| r.tasks_by_complexity_bucket[bucket] >= 1)
            .max_by(|a, b| {
                let a_rate = a.success_by_complexity_bucket[bucket] as f64
                    / a.tasks_by_complexity_bucket[bucket].max(1) as f64;
                let b_rate = b.success_by_complexity_bucket[bucket] as f64
                    / b.tasks_by_complexity_bucket[bucket].max(1) as f64;
                a_rate.partial_cmp(&b_rate).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|r| r.model.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(model: &str, calls: u64, successes: u64, cost: f64, latency: u64) -> ModelPerformanceRecord {
        ModelPerformanceRecord {
            model: model.to_string(),
            provider: "test".to_string(),
            total_calls: calls,
            total_input_tokens: calls * 1000,
            total_output_tokens: calls * 200,
            total_cost_usd: cost,
            successes,
            failures: calls - successes,
            total_latency_ms: latency * calls,
            tasks_by_complexity_bucket: [calls / 4; 4],
            success_by_complexity_bucket: [successes / 4; 4],
        }
    }

    #[test]
    fn test_roi_higher_for_efficient_models() {
        let cheap_good = record("haiku", 100, 95, 0.05, 500);
        let expensive_good = record("opus", 100, 98, 5.0, 5000);
        assert!(cheap_good.roi_score() > expensive_good.roi_score());
    }

    #[test]
    fn test_snapshot_aggregation() {
        let records = vec![
            record("haiku", 50, 45, 0.02, 300),
            record("sonnet", 30, 28, 0.10, 1000),
        ];
        let snap = EconomicsEngine::compute_snapshot(&records);
        assert_eq!(snap.total_calls, 80);
        assert_eq!(snap.total_successes, 73);
        assert!(snap.total_cost_usd > 0.0);
    }

    #[test]
    fn test_best_model_selection() {
        let records = vec![
            record("haiku", 100, 90, 0.05, 300),
            record("opus", 10, 10, 5.0, 5000),
        ];
        let snap = EconomicsEngine::compute_snapshot(&records);
        assert_eq!(snap.fastest_model.as_deref(), Some("haiku"));
        assert_eq!(snap.most_reliable_model.as_deref(), Some("opus"));
    }
}
