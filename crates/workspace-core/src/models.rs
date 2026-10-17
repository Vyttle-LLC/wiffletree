//! The models a provider's CLI offers on this machine.
use crate::{Provider, ProviderStatus};
use serde::{Deserialize, Serialize};

/// A model a provider offers, with the efforts it reports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelOption {
    pub provider: Provider,
    pub model: String,
    pub label: String,
    pub efforts: Vec<String>,
}

/// What a provider's CLI says about itself: whether it is installed and signed in, and the
/// models it lists, none when it could not list them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderReport {
    pub status: ProviderStatus,
    pub models: Vec<ModelOption>,
}
