use workspace_core::{ModelProfile, Provider, ProviderProfiles, Role, RoleDefault, RolePolicy};
use workspace_host::runtime::ModelOption;

/// Roles with editable defaults. Coordinator covers main and repository coordinators.
pub const ROLES: [(Role, &str); 4] = [
    (Role::ProjectOrchestrator, "Coordinator"),
    (Role::Implementer, "Implementer"),
    (Role::Tester, "Tester"),
    (Role::Reviewer, "Reviewer"),
];
pub const PROVIDERS: [Provider; 2] = [Provider::Claude, Provider::Codex];

fn builtin(provider: Provider, small: bool) -> ModelProfile {
    let (model, effort) = match (provider, small) {
        (Provider::Claude, false) => ("opus", "high"),
        (Provider::Claude, true) => ("sonnet", "medium"),
        (Provider::Codex, false) => ("gpt-6.1-sol", "high"),
        (Provider::Codex, true) => ("gpt-6.1-sol", "medium"),
    };
    ModelProfile {
        provider,
        model: model.into(),
        effort: effort.into(),
    }
}

/// The editable Big/Small profiles for each role, derived from saved policies.
/// Legacy fixed policies contribute their default and small tiers for their own provider.
pub fn role_defaults(policies: &[RolePolicy]) -> Vec<RoleDefault> {
    ROLES
        .into_iter()
        .filter_map(|(role, _)| policies.iter().find(|policy| policy.role == role))
        .map(|policy| RoleDefault {
            role: policy.role,
            default_provider: policy.default.provider,
            profiles: PROVIDERS
                .into_iter()
                .map(|provider| {
                    let saved = policy
                        .provider_profiles
                        .iter()
                        .find(|profiles| profiles.provider == provider);
                    let legacy = |tier: &ModelProfile, small| {
                        if tier.provider == provider {
                            tier.clone()
                        } else {
                            builtin(provider, small)
                        }
                    };
                    saved.cloned().unwrap_or_else(|| ProviderProfiles {
                        provider,
                        big: legacy(&policy.default, false),
                        small: legacy(&policy.small, true),
                    })
                })
                .collect(),
        })
        .collect()
}

pub fn profile(defaults: &RoleDefault, provider: Provider, small: bool) -> Option<&ModelProfile> {
    let profiles = defaults.profiles.iter().find(|p| p.provider == provider)?;
    Some(if small {
        &profiles.small
    } else {
        &profiles.big
    })
}

pub fn profile_mut(
    defaults: &mut RoleDefault,
    provider: Provider,
    small: bool,
) -> Option<&mut ModelProfile> {
    let profiles = defaults
        .profiles
        .iter_mut()
        .find(|p| p.provider == provider)?;
    Some(if small {
        &mut profiles.small
    } else {
        &mut profiles.big
    })
}

pub fn fallback() -> Vec<ModelOption> {
    [
        (Provider::Claude, "opus", "Opus · latest"),
        (Provider::Claude, "sonnet", "Sonnet · latest"),
        (Provider::Codex, "gpt-6.1-sol", "GPT 6.1 Sol"),
        (Provider::Codex, "gpt-6-astra", "GPT 6 Astra"),
    ]
    .into_iter()
    .map(|(provider, model, label)| ModelOption {
        provider,
        model: model.into(),
        label: label.into(),
        efforts: vec![],
    })
    .collect()
}

pub fn choices(catalog: &[ModelOption], provider: Provider, saved: &[&str]) -> Vec<ModelOption> {
    let mut choices: Vec<_> = catalog
        .iter()
        .filter(|m| m.provider == provider)
        .cloned()
        .collect();
    for saved in saved {
        if !choices.iter().any(|m| m.model == *saved) {
            choices.push(ModelOption {
                provider,
                model: (*saved).into(),
                label: format!("Saved · {saved}"),
                efforts: vec![],
            });
        }
    }
    choices
}

pub fn effort_choices(
    catalog: &[ModelOption],
    provider: Provider,
    model: &str,
    saved: &str,
) -> Vec<String> {
    let mut efforts = catalog
        .iter()
        .find(|option| option.provider == provider && option.model == model)
        .map(|option| option.efforts.clone())
        .filter(|efforts| !efforts.is_empty())
        .unwrap_or_else(|| {
            let fallback = if provider == Provider::Claude {
                vec!["low", "medium", "high", "xhigh", "max"]
            } else {
                vec!["minimal", "low", "medium", "high", "xhigh", "max", "ultra"]
            };
            fallback.into_iter().map(str::to_owned).collect()
        });
    if !efforts.iter().any(|effort| effort == saved) {
        efforts.push(saved.into());
    }
    efforts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshed_catalog_preserves_saved_models_without_crossing_providers() {
        let options = choices(
            &fallback(),
            Provider::Codex,
            &["gpt-6.1-sol", "custom-codex-version"],
        );
        assert!(options.iter().all(|m| m.provider == Provider::Codex));
        assert!(options.iter().any(|m| m.model == "custom-codex-version"));
        assert_eq!(choices(&fallback(), Provider::Claude, &["opus"]).len(), 2);
    }
    #[test]
    fn legacy_policies_become_editable_profiles_for_both_providers() {
        let defaults = role_defaults(&workspace_core::default_policies());
        assert_eq!(defaults.len(), ROLES.len());
        let coordinator = &defaults[0];
        assert_eq!(coordinator.default_provider, Provider::Claude);
        assert_eq!(
            profile(coordinator, Provider::Claude, false)
                .unwrap()
                .effort,
            "high"
        );
        assert_eq!(
            profile(coordinator, Provider::Codex, true).unwrap().model,
            "gpt-6.1-sol"
        );
        let tester = &defaults[2];
        assert_eq!(tester.default_provider, Provider::Codex);
        assert_eq!(
            profile(tester, Provider::Claude, false).unwrap().model,
            "opus"
        );
    }
    #[test]
    fn efforts_follow_the_selected_model_and_preserve_saved_values() {
        let mut catalog = fallback();
        catalog[2].efforts = vec!["high".into(), "ultra".into()];
        assert_eq!(
            effort_choices(&catalog, Provider::Codex, "gpt-6.1-sol", "high"),
            ["high", "ultra"]
        );
        assert_eq!(
            effort_choices(&catalog, Provider::Codex, "gpt-6.1-sol", "custom-effort"),
            ["high", "ultra", "custom-effort"]
        );
        let claude = effort_choices(&catalog, Provider::Claude, "opus", "high");
        assert!(claude.contains(&"high".to_owned()));
        assert!(!claude.contains(&"ultra".to_owned()));
    }
}
