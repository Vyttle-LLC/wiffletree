//! The setup suggested on first run, from the provider CLIs signed in on this machine.
use crate::{
    AllowedModel, ModelSelection, PROVIDER_ROLES, PROVIDERS, Provider, ProviderAccess, Role,
    VerificationSettings,
};
use std::collections::{BTreeMap, BTreeSet};

/// What detection found for one provider's CLI.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderStatus {
    pub installed: bool,
    pub signed_in: bool,
    /// The subscription plan, when the CLI reports one. Shown, never acted on.
    pub plan: Option<String>,
}
impl ProviderStatus {
    pub fn usable(&self) -> bool {
        self.installed && self.signed_in
    }
}
pub type Detection = BTreeMap<Provider, ProviderStatus>;

/// Everything the Models page saves: the model selection and who verifies tickets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    pub selection: ModelSelection,
    pub verification: VerificationSettings,
}

fn models(provider: Provider) -> Vec<AllowedModel> {
    let pairs: &[(&str, &str)] = match provider {
        Provider::Claude => &[
            ("opus", "high"),
            ("opus", "medium"),
            ("sonnet", "high"),
            ("sonnet", "medium"),
        ],
        Provider::Codex => &[("gpt-6.1-sol", "high"), ("gpt-6.1-sol", "medium")],
    };
    pairs
        .iter()
        .map(|(model, effort)| AllowedModel {
            model: (*model).into(),
            effort: (*effort).into(),
        })
        .collect()
}

/// The starter guide, with a line for each of `providers`.
pub fn starter_guide(providers: &[Provider]) -> String {
    let lines: Vec<&str> = providers
        .iter()
        .map(|provider| match provider {
            Provider::Claude => {
                "Claude: small, contained change → Sonnet; large or cross-cutting → Opus."
            }
            Provider::Codex => "Codex: small → GPT 6.1 Sol at medium; large → GPT 6.1 Sol at high.",
        })
        .collect();
    format!(
        "Pick the model by the size and risk of the work. {} Raise effort for migrations, data or security changes.\n",
        lines.join(" ")
    )
}

/// A setup that uses only the providers found installed and signed in: Claude coordinates and
/// implements and Codex tests when both are, otherwise the one found does everything. With
/// neither, both providers are disabled and the setup cannot be saved.
pub fn suggest(detection: &Detection) -> Setup {
    let usable: Vec<Provider> = PROVIDERS
        .into_iter()
        .filter(|p| detection.get(p).is_some_and(ProviderStatus::usable))
        .collect();
    let providers = PROVIDERS
        .into_iter()
        .map(|provider| {
            let enabled = usable.contains(&provider);
            let models = if enabled { models(provider) } else { vec![] };
            (provider, ProviderAccess { enabled, models })
        })
        .collect();
    let role_providers: BTreeMap<Role, BTreeSet<Provider>> = PROVIDER_ROLES
        .into_iter()
        .filter_map(|role| {
            let preferred = if role == Role::Tester {
                Provider::Codex
            } else {
                Provider::Claude
            };
            let provider = usable
                .iter()
                .find(|&&p| p == preferred)
                .or(usable.first())?;
            Some((role, BTreeSet::from([*provider])))
        })
        .collect();
    let mut verification = VerificationSettings::default();
    verification
        .verifiers
        .retain(|v| v.provider.is_none_or(|p| usable.contains(&p)));
    Setup {
        selection: ModelSelection {
            revision: 0,
            providers,
            role_providers,
            guide: if usable.is_empty() {
                String::new()
            } else {
                starter_guide(&usable)
            },
        },
        verification,
    }
}
