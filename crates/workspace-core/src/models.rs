//! The models a provider's CLI offers on this machine, and the lineup this app knows without
//! asking it: the catalog shown until a CLI lists its own, each provider's usual efforts and the
//! short names reasons use.
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

/// A model this app knows without asking its CLI.
struct Known {
    provider: Provider,
    model: &'static str,
    label: &'static str,
    /// What reasons call it.
    name: &'static str,
}
const LINEUP: [Known; 4] = [
    Known {
        provider: Provider::Claude,
        model: "opus",
        label: "Opus · latest",
        name: "Opus",
    },
    Known {
        provider: Provider::Claude,
        model: "sonnet",
        label: "Sonnet · latest",
        name: "Sonnet",
    },
    Known {
        provider: Provider::Codex,
        model: "gpt-6.1-sol",
        label: "GPT 6.1 Sol",
        name: "GPT 6.1 Sol",
    },
    Known {
        provider: Provider::Codex,
        model: "gpt-6-astra",
        label: "GPT 6 Astra",
        name: "GPT 6 Astra",
    },
];

/// The catalog shown until a provider's CLI lists its own models. It reports no efforts, so
/// each model offers its provider's usual ones.
pub fn fallback_catalog() -> Vec<ModelOption> {
    (LINEUP.iter())
        .map(|known| ModelOption {
            provider: known.provider,
            model: known.model.into(),
            label: known.label.into(),
            efforts: vec![],
        })
        .collect()
}

/// The efforts a provider's models usually offer, for a model whose own are not known.
pub fn usual_efforts(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Claude => &["low", "medium", "high", "xhigh", "max"],
        Provider::Codex => &["minimal", "low", "medium", "high", "xhigh", "max", "ultra"],
    }
}

/// "Opus" for `opus`, as reasons name a model; a model this app does not know by its ID.
pub fn model_name(provider: Provider, model: &str) -> String {
    (LINEUP.iter())
        .find(|known| known.provider == provider && known.model == model)
        .map_or_else(|| model.to_owned(), |known| known.name.to_owned())
}
