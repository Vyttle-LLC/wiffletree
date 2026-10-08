//! Pure helpers behind the Models page and the model pickers.
use workspace_core::{
    AllowedModel, Chooser, ModelProfile, ModelSelection, PROVIDERS, Project, Provider, Role,
    Selection, Session,
};
use workspace_host::runtime::ModelOption;

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

/// The model's reported efforts, else the provider's usual ones, plus any saved effort.
pub fn effort_choices(
    catalog: &[ModelOption],
    provider: Provider,
    model: &str,
    saved: &[&str],
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
    for saved in saved {
        if !efforts.iter().any(|effort| effort == saved) {
            efforts.push((*saved).into());
        }
    }
    efforts
}

pub fn model_label(catalog: &[ModelOption], provider: Provider, model: &str) -> String {
    catalog
        .iter()
        .find(|m| m.provider == provider && m.model == model)
        .map_or_else(|| model.to_owned(), |m| m.label.clone())
}

/// "Claude · Opus · latest · High", as pickers show a profile.
pub fn profile_label(catalog: &[ModelOption], profile: &ModelProfile) -> String {
    format!(
        "{} · {} · {}",
        profile.provider.label(),
        model_label(catalog, profile.provider, &profile.model),
        crate::ui::effort_label(&profile.effort)
    )
}

/// A model the Providers card offers to allow, with the efforts it supports.
#[derive(Clone)]
pub struct ModelRow {
    pub model: String,
    pub label: String,
    pub efforts: Vec<String>,
}
/// The catalog's models for a provider plus allowed ones the catalog no longer lists.
pub fn model_rows(
    catalog: &[ModelOption],
    provider: Provider,
    selection: &ModelSelection,
) -> Vec<ModelRow> {
    let allowed = selection
        .providers
        .get(&provider)
        .map_or(&[][..], |a| &a.models);
    let saved: Vec<&str> = allowed.iter().map(|m| m.model.as_str()).collect();
    choices(catalog, provider, &saved)
        .into_iter()
        .map(|option| {
            let saved: Vec<&str> = allowed
                .iter()
                .filter(|m| m.model == option.model)
                .map(|m| m.effort.as_str())
                .collect();
            ModelRow {
                efforts: effort_choices(catalog, provider, &option.model, &saved),
                model: option.model,
                label: option.label,
            }
        })
        .collect()
}
/// The provider's allowed models in first-listed order, each with its allowed efforts.
pub fn allowed_models(
    selection: &ModelSelection,
    provider: Provider,
) -> Vec<(String, Vec<String>)> {
    let mut grouped: Vec<(String, Vec<String>)> = Vec::new();
    for entry in selection
        .providers
        .get(&provider)
        .map_or(&[][..], |a| &a.models)
    {
        match grouped.iter_mut().find(|(model, _)| *model == entry.model) {
            Some((_, efforts)) => efforts.push(entry.effort.clone()),
            None => grouped.push((entry.model.clone(), vec![entry.effort.clone()])),
        }
    }
    grouped
}

/// Allows a model at its first effort; the catalog reports no default effort. New entries go
/// last, so pre-fill keeps its first model.
pub fn add_model(selection: &mut ModelSelection, provider: Provider, row: &ModelRow) {
    let Some(effort) = row.efforts.first() else {
        return;
    };
    if !allowed_models(selection, provider)
        .iter()
        .any(|(model, _)| *model == row.model)
    {
        toggle_effort(selection, provider, &row.model, effort);
    }
}
/// Ticks or unticks one effort. Unticking a model's last effort removes the model.
pub fn toggle_effort(
    selection: &mut ModelSelection,
    provider: Provider,
    model: &str,
    effort: &str,
) {
    let models = &mut selection.providers.entry(provider).or_default().models;
    match models
        .iter()
        .position(|m| m.model == model && m.effort == effort)
    {
        Some(index) => {
            models.remove(index);
        }
        None => models.push(AllowedModel {
            model: model.into(),
            effort: effort.into(),
        }),
    }
}
/// Removes every effort of a model.
pub fn remove_model(selection: &mut ModelSelection, provider: Provider, model: &str) {
    if let Some(access) = selection.providers.get_mut(&provider) {
        access.models.retain(|m| m.model != model);
    }
}
/// Disabling a provider keeps its allowed models but takes it out of every role.
pub fn set_enabled(selection: &mut ModelSelection, provider: Provider, enabled: bool) {
    selection.providers.entry(provider).or_default().enabled = enabled;
    if !enabled {
        for providers in selection.role_providers.values_mut() {
            providers.remove(&provider);
        }
    }
}
pub fn toggle_role_provider(selection: &mut ModelSelection, role: Role, provider: Provider) {
    let providers = selection.role_providers.entry(role).or_default();
    if !providers.remove(&provider) {
        providers.insert(provider);
    }
}

/// Why `verify_ticket` would refuse in some projects: a round's verifiers all run at once, and
/// each project keeps one of its turns for its coordinator.
pub fn turn_limit_note(verifiers: usize, projects: &[Project]) -> Option<String> {
    let needed = verifiers + 1;
    let short: Vec<_> = projects
        .iter()
        .filter(|p| p.turn_limit < needed)
        .map(|p| format!("{} ({})", p.name, p.turn_limit))
        .collect();
    (!short.is_empty()).then(|| {
        format!(
            "{verifiers} verifiers need a project turn limit of at least {needed}. Lower now: {}.",
            short.join(", ")
        )
    })
}
/// A group of the human's model picker.
pub struct PickerGroup {
    pub heading: String,
    pub profiles: Vec<ModelProfile>,
}
/// The role's providers' allowed models first, then every other model the human may pick.
pub fn picker_groups(
    selection: &ModelSelection,
    catalog: &[ModelOption],
    role: Role,
) -> Vec<PickerGroup> {
    let configured = |p: &Provider| selection.providers_for(role).contains(p);
    let profile = |provider, m: &AllowedModel| ModelProfile {
        provider,
        model: m.model.clone(),
        effort: m.effort.clone(),
    };
    let mut groups: Vec<PickerGroup> = PROVIDERS
        .into_iter()
        .filter(|p| configured(p))
        .map(|provider| PickerGroup {
            heading: format!("{} · {}", role.label(), provider.label()),
            profiles: selection
                .allowed(provider)
                .iter()
                .map(|m| profile(provider, m))
                .collect(),
        })
        .filter(|g| !g.profiles.is_empty())
        .collect();
    let listed: Vec<ModelProfile> = groups.iter().flat_map(|g| g.profiles.clone()).collect();
    let mut others: Vec<ModelProfile> = PROVIDERS
        .into_iter()
        .flat_map(|p| selection.allowed(p).iter().map(move |m| (p, m)))
        .map(|(p, m)| profile(p, m))
        .collect();
    // Every catalog model at every effort: the human may pick anything.
    for option in catalog {
        for effort in effort_choices(catalog, option.provider, &option.model, &[]) {
            others.push(ModelProfile {
                provider: option.provider,
                model: option.model.clone(),
                effort,
            });
        }
    }
    let mut seen = listed.clone();
    others.retain(|p| {
        let new = !seen.contains(p);
        seen.push(p.clone());
        new
    });
    if !others.is_empty() {
        groups.push(PickerGroup {
            heading: "Other models".into(),
            profiles: others,
        });
    }
    groups
}

/// How an agent's model was chosen, as its chat shows it.
pub fn chosen_by(selection: &Selection, sessions: &[Session]) -> String {
    match &selection.chosen_by {
        Chooser::Coordinator { session_id } => {
            let name = sessions
                .iter()
                .find(|s| s.id == *session_id)
                .map_or("its coordinator", |s| s.name.as_str());
            format!("Chosen by {name}: {}", selection.reason)
        }
        Chooser::Human => "Chosen by you".into(),
        Chooser::Default => selection.reason.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use workspace_core::{PROVIDER_ROLES, ProviderAccess};

    fn selection() -> ModelSelection {
        let access = |models: &[(&str, &str)]| ProviderAccess {
            enabled: true,
            models: models
                .iter()
                .map(|(model, effort)| AllowedModel {
                    model: (*model).into(),
                    effort: (*effort).into(),
                })
                .collect(),
        };
        ModelSelection {
            revision: 3,
            providers: [
                (
                    Provider::Claude,
                    access(&[("opus", "high"), ("sonnet", "high")]),
                ),
                (Provider::Codex, access(&[("gpt-6.1-sol", "high")])),
            ]
            .into(),
            role_providers: PROVIDER_ROLES
                .into_iter()
                .map(|r| (r, BTreeSet::from([Provider::Claude])))
                .collect(),
            guide: String::new(),
        }
    }

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
    fn efforts_follow_the_selected_model_and_preserve_saved_values() {
        let mut catalog = fallback();
        catalog[2].efforts = vec!["high".into(), "ultra".into()];
        assert_eq!(
            effort_choices(&catalog, Provider::Codex, "gpt-6.1-sol", &["high"]),
            ["high", "ultra"]
        );
        assert_eq!(
            effort_choices(&catalog, Provider::Codex, "gpt-6.1-sol", &["custom-effort"]),
            ["high", "ultra", "custom-effort"]
        );
        let claude = effort_choices(&catalog, Provider::Claude, "opus", &[]);
        assert!(claude.contains(&"high".to_owned()));
        assert!(!claude.contains(&"ultra".to_owned()));
    }
    #[test]
    fn models_are_added_once_and_grouped_with_their_efforts() {
        let mut selection = selection();
        let saved = AllowedModel {
            model: "opus-legacy".into(),
            effort: "medium".into(),
        };
        selection
            .providers
            .get_mut(&Provider::Claude)
            .unwrap()
            .models
            .push(saved);
        // Allowed models the catalog no longer lists stay offered with their saved effort.
        let rows = model_rows(&fallback(), Provider::Claude, &selection);
        let legacy = rows.iter().find(|r| r.model == "opus-legacy").unwrap();
        assert_eq!(legacy.label, "Saved · opus-legacy");
        assert!(legacy.efforts.contains(&"medium".to_owned()));
        toggle_effort(&mut selection, Provider::Claude, "opus-legacy", "high");
        // Adding a listed model changes nothing; a new one starts at its first effort.
        add_model(&mut selection, Provider::Claude, legacy);
        let sonnet = rows.iter().find(|r| r.model == "sonnet").unwrap();
        remove_model(&mut selection, Provider::Claude, "sonnet");
        add_model(&mut selection, Provider::Claude, sonnet);
        let grouped = allowed_models(&selection, Provider::Claude);
        assert_eq!(
            grouped[1..],
            [
                (
                    "opus-legacy".to_owned(),
                    vec!["medium".to_owned(), "high".to_owned()]
                ),
                ("sonnet".to_owned(), vec![sonnet.efforts[0].clone()]),
            ]
        );
        // Unticking the last effort removes the model.
        toggle_effort(
            &mut selection,
            Provider::Claude,
            "sonnet",
            &sonnet.efforts[0],
        );
        remove_model(&mut selection, Provider::Claude, "opus-legacy");
        assert_eq!(allowed_models(&selection, Provider::Claude).len(), 1);
    }
    #[test]
    fn disabling_a_provider_keeps_its_ticks_but_frees_roles() {
        let mut selection = selection();
        toggle_role_provider(&mut selection, Role::Implementer, Provider::Codex);
        set_enabled(&mut selection, Provider::Codex, false);
        assert_eq!(
            allowed_models(&selection, Provider::Codex),
            [("gpt-6.1-sol".to_owned(), vec!["high".to_owned()])]
        );
        assert_eq!(
            selection.role_providers[&Role::Implementer],
            BTreeSet::from([Provider::Claude])
        );
        selection.validate().unwrap();
        // The only provider gone: the role is left without one and Save must refuse.
        set_enabled(&mut selection, Provider::Claude, false);
        assert!(selection.validate().is_err());
    }
    #[test]
    fn the_picker_lists_the_roles_providers_first_then_everything_else() {
        let selection = selection();
        let groups = picker_groups(&selection, &fallback(), Role::Reviewer);
        let headings: Vec<_> = groups.iter().map(|g| g.heading.as_str()).collect();
        assert_eq!(
            headings,
            ["Reviewer · Claude", "Reviewer · Codex", "Other models"]
        );
        assert_eq!(
            groups[0].profiles[0],
            selection.prefill(Role::Reviewer).unwrap()
        );
        let implementer = picker_groups(&selection, &fallback(), Role::Implementer);
        assert_eq!(implementer[0].heading, "Implementer · Claude");
        let others = &implementer[1].profiles;
        assert_eq!(others[0].model, "gpt-6.1-sol");
        assert!(
            others
                .iter()
                .any(|p| p.model == "gpt-6-astra" && p.effort == "high")
        );
        assert!(
            !others
                .iter()
                .any(|p| p.model == "opus" && p.effort == "high")
        );
        // Unticked efforts of every catalog model stay available to the human.
        for (model, effort) in [
            ("opus", "low"),
            ("opus", "max"),
            ("sonnet", "xhigh"),
            ("gpt-6-astra", "minimal"),
        ] {
            assert!(
                others
                    .iter()
                    .any(|p| p.model == model && p.effort == effort),
                "{model} · {effort}"
            );
        }
    }
    #[test]
    fn the_review_card_names_projects_whose_turn_limit_is_too_low() {
        let project = |name: &str, turn_limit| Project {
            id: name.into(),
            name: name.into(),
            brain: None,
            turn_limit,
            repositories: None,
            home: None,
            slug: None,
        };
        let projects = [project("Small", 4), project("Large", 5)];
        assert_eq!(turn_limit_note(3, &projects), None);
        assert_eq!(
            turn_limit_note(4, &projects).unwrap(),
            "4 verifiers need a project turn limit of at least 5. Lower now: Small (4)."
        );
    }
}
