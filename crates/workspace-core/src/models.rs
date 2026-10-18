//! The models a provider's CLI offers on this machine, and the lineup this app knows without
//! asking it: the catalog shown until a CLI lists its own, each provider's usual efforts and the
//! models the first-run setup suggests.
use crate::{AllowedModel, Provider, ProviderStatus};
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

/// A model this app knows without asking its CLI, and the efforts the first-run setup allows
/// it at, none when the setup leaves it out.
struct Known {
    provider: Provider,
    model: &'static str,
    label: &'static str,
    suggested_efforts: &'static [&'static str],
}
const LINEUP: [Known; 4] = [
    Known {
        provider: Provider::Claude,
        model: "opus",
        label: "Opus · latest",
        suggested_efforts: &["high", "medium"],
    },
    Known {
        provider: Provider::Claude,
        model: "sonnet",
        label: "Sonnet · latest",
        suggested_efforts: &["high", "medium"],
    },
    Known {
        provider: Provider::Codex,
        model: "gpt-6.1-sol",
        label: "GPT 6.1 Sol",
        suggested_efforts: &["high", "medium"],
    },
    Known {
        provider: Provider::Codex,
        model: "gpt-6-astra",
        label: "GPT 6 Astra",
        suggested_efforts: &[],
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

/// The models and efforts the first-run setup allows for `provider`, in the order it lists them.
pub fn suggested_models(provider: Provider) -> Vec<AllowedModel> {
    (LINEUP.iter())
        .filter(|known| known.provider == provider)
        .flat_map(|known| {
            (known.suggested_efforts.iter()).map(|effort| AllowedModel {
                model: known.model.into(),
                effort: (*effort).into(),
            })
        })
        .collect()
}
