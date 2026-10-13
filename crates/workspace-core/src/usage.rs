use crate::Provider;
use serde::{Deserialize, Serialize};

/// A token count as the Context card prints it; whole numbers stay whole: `90k`, `4.2k`,
/// `1M`, `601`.
pub fn compact_tokens(tokens: u64) -> String {
    let scaled = |value: f64, unit: &str| {
        if value >= 10. || value.fract() < 0.05 {
            format!("{value:.0}{unit}")
        } else {
            format!("{value:.1}{unit}")
        }
    };
    if tokens >= 1_000_000 {
        scaled(tokens as f64 / 1e6, "M")
    } else if tokens >= 1000 {
        scaled(tokens as f64 / 1e3, "k")
    } else {
        tokens.to_string()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TokenTotals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
    pub requests: u64,
    pub partial_requests: u64,
}
impl TokenTotals {
    pub fn total(&self) -> u64 {
        self.input.saturating_add(self.output)
    }
    pub fn add(&mut self, other: &Self) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
        self.reasoning += other.reasoning;
        self.requests += other.requests;
        self.partial_requests += other.partial_requests;
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DailyTokens {
    pub date: String,
    pub provider: Provider,
    pub counts: TokenTotals,
    pub covered: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoordinatorUsage {
    pub id: String,
    pub name: String,
    pub project: String,
    pub own: TokenTotals,
    pub workers: TokenTotals,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuotaWindow {
    pub key: String,
    pub label: String,
    pub used_percent: Option<f64>,
    pub observed_at: i64,
    pub duration_mins: Option<u64>,
    pub resets_at: Option<i64>,
    pub status: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuotaReading {
    pub provider: Provider,
    pub account: Option<String>,
    pub observed_at: i64,
    pub source: String,
    pub windows: Vec<QuotaWindow>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DailyQuota {
    pub date: String,
    pub increase: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuotaTrend {
    pub provider: Provider,
    pub account: Option<String>,
    pub key: String,
    pub label: String,
    pub points_per_hour: Option<f64>,
    pub daily: Vec<DailyQuota>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageReport {
    pub timezone: String,
    pub start_date: String,
    pub end_date: String,
    pub tracking_since: i64,
    pub totals: TokenTotals,
    pub unreported_runs: u64,
    pub daily: Vec<DailyTokens>,
    pub coordinators: Vec<CoordinatorUsage>,
    pub quota_trends: Vec<QuotaTrend>,
}
