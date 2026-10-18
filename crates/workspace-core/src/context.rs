//! What occupies a provider session's context window, as the host measures it.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextCategory {
    pub name: String,
    pub tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextUsage {
    pub model: Option<String>,
    pub used: u64,
    pub window: u64,
    /// Where the provider compacts the conversation, when it says.
    pub compacts_at: Option<u64>,
    pub categories: Vec<ContextCategory>,
    /// True when the categories are sized by this app rather than reported by the provider.
    pub estimated: bool,
    pub observed_at: i64,
}
