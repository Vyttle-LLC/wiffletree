//! Pure helpers behind the Models page and the model pickers.
use workspace_core::{
    AllowedModel, Chooser, Detection, ModelProfile, ModelSelection, PROVIDERS, Provider,
    ProviderStatus, Role, Selection, Session, Setup, VerificationSettings, suggest,
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

/// "CLI found · signed in · Max plan", as the Providers card shows what detection found.
pub fn status_label(status: &ProviderStatus) -> String {
    if !status.installed {
        return "CLI not found".into();
    }
    if !status.signed_in {
        return "CLI found · not signed in".into();
    }
    let plan = status
        .plan
        .as_deref()
        .map(|plan| format!(" · {} plan", crate::ui::humanize(plan)));
    format!("CLI found · signed in{}", plan.unwrap_or_default())
}
/// The one-line fix for a provider a suggested setup left out, or `None` when it is usable.
pub fn fix_hint(provider: Provider, status: &ProviderStatus) -> Option<String> {
    fix(provider, status, ", then Refresh")
}
/// The same fix for the welcome, which has no Refresh to point at.
pub fn welcome_fix_hint(provider: Provider, status: &ProviderStatus) -> Option<String> {
    fix(provider, status, "")
}
/// `next` is what a signed-out CLI's fix says to do after signing in.
fn fix(provider: Provider, status: &ProviderStatus, next: &str) -> Option<String> {
    let (product, sign_in) = match provider {
        Provider::Claude => ("Claude Code", "claude auth login"),
        Provider::Codex => ("Codex", "codex login"),
    };
    match (status.installed, status.signed_in) {
        (true, true) => None,
        (true, false) => Some(format!("Run {sign_in}{next} to add it.")),
        (false, _) => Some(format!("Install {product} and run {sign_in}.")),
    }
}

/// A store with no saved setup cannot be edited or saved until its first suggestion is shown,
/// so an early edit is never overwritten and the first save always carries the suggested
/// verifiers.
pub fn editable(saved: &ModelSelection, suggested: Option<&Setup>) -> bool {
    saved.is_saved() || suggested.is_some()
}
/// Whether a provider carries the "Suggested" tag: the suggestion enabled it and the draft
/// still has it enabled.
pub fn tagged_suggested(
    suggested: Option<&Setup>,
    draft: &ModelSelection,
    provider: Provider,
) -> bool {
    draft.is_enabled(provider) && suggested.is_some_and(|s| s.selection.is_enabled(provider))
}
/// Whether the draft still is the suggested setup, verifiers included.
pub fn untouched(
    suggested: &Setup,
    selection: &ModelSelection,
    verification: Option<&VerificationSettings>,
) -> bool {
    *selection == suggested.selection && verification == Some(&suggested.verification)
}
/// A setup to suggest after a detection pass.
#[derive(Debug, PartialEq)]
pub struct Offer {
    pub suggested: Setup,
    /// The draft follows it: it was untouched, or nothing was suggested before.
    pub replaces_draft: bool,
}
/// What a detection pass suggests for a store with no saved setup; `None` on a saved store,
/// before every provider is detected, or when `shown` already is that suggestion. A draft the
/// human edited is kept.
pub fn offer(
    saved: &ModelSelection,
    shown: Option<&Setup>,
    selection: &ModelSelection,
    verification: Option<&VerificationSettings>,
    detection: &Detection,
) -> Option<Offer> {
    if saved.is_saved() || !PROVIDERS.iter().all(|p| detection.contains_key(p)) {
        return None;
    }
    let suggested = suggest(detection);
    if shown == Some(&suggested) {
        return None;
    }
    Some(Offer {
        replaces_draft: shown.is_none_or(|shown| untouched(shown, selection, verification)),
        suggested,
    })
}
/// Save needs an editable page and a valid draft that differs from what is saved; a suggested
/// setup is unsaved from the start, so it needs no edit first.
pub fn can_save(editable: bool, suggested: bool, edited: bool, selection: &ModelSelection) -> bool {
    editable && (suggested || edited) && selection.validate().is_ok()
}
/// The draft once the host has saved `sent`: it takes the saved revision unless the human
/// edited it meanwhile.
pub fn after_save(
    draft: &ModelSelection,
    sent: &ModelSelection,
    saved: &ModelSelection,
) -> ModelSelection {
    if draft == sent {
        saved.clone()
    } else {
        draft.clone()
    }
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
    fn provider_status_reads_as_found_signed_in_and_plan_with_a_fix_when_unusable() {
        let status = |installed, signed_in, plan: Option<&str>| ProviderStatus {
            installed,
            signed_in,
            plan: plan.map(Into::into),
        };
        let signed_in = status(true, true, Some("max"));
        assert_eq!(status_label(&signed_in), "CLI found · signed in · Max plan");
        assert_eq!(
            status_label(&status(true, true, Some("free_workspace"))),
            "CLI found · signed in · Free workspace plan"
        );
        assert_eq!(fix_hint(Provider::Claude, &signed_in), None);
        // An unknown plan is left out, never guessed.
        assert_eq!(
            status_label(&status(true, true, None)),
            "CLI found · signed in"
        );
        let signed_out = status(true, false, None);
        assert_eq!(status_label(&signed_out), "CLI found · not signed in");
        assert_eq!(
            fix_hint(Provider::Codex, &signed_out).unwrap(),
            "Run codex login, then Refresh to add it."
        );
        let missing = status(false, false, None);
        assert_eq!(status_label(&missing), "CLI not found");
        assert_eq!(
            fix_hint(Provider::Claude, &missing).unwrap(),
            "Install Claude Code and run claude auth login."
        );
        assert_eq!(
            fix_hint(Provider::Codex, &missing).unwrap(),
            "Install Codex and run codex login."
        );
        // The welcome has no Refresh, so its fix names none; the rest is the same text.
        assert_eq!(welcome_fix_hint(Provider::Claude, &signed_in), None);
        assert_eq!(
            welcome_fix_hint(Provider::Codex, &signed_out).unwrap(),
            "Run codex login to add it."
        );
        assert_eq!(
            welcome_fix_hint(Provider::Claude, &signed_out).unwrap(),
            "Run claude auth login to add it."
        );
        assert_eq!(
            welcome_fix_hint(Provider::Claude, &missing),
            fix_hint(Provider::Claude, &missing)
        );
    }
    fn detected(signed_in: &[Provider]) -> Detection {
        PROVIDERS
            .into_iter()
            .map(|provider| {
                let found = signed_in.contains(&provider);
                let status = ProviderStatus {
                    installed: found,
                    signed_in: found,
                    plan: None,
                };
                (provider, status)
            })
            .collect()
    }
    fn offered(
        saved: &ModelSelection,
        shown: Option<&Setup>,
        draft: &Setup,
        detection: &Detection,
    ) -> Option<Offer> {
        offer(
            saved,
            shown,
            &draft.selection,
            Some(&draft.verification),
            detection,
        )
    }
    #[test]
    fn an_untouched_suggestion_follows_refresh_and_an_edited_one_is_kept() {
        let unsaved = ModelSelection::default();
        let claude = suggest(&detected(&[Provider::Claude]));
        let both = suggest(&detected(&PROVIDERS));
        // The first detection fills the empty draft.
        assert_eq!(
            offer(
                &unsaved,
                None,
                &unsaved,
                None,
                &detected(&[Provider::Claude])
            ),
            Some(Offer {
                suggested: claude.clone(),
                replaces_draft: true
            })
        );
        // The same detection again offers nothing new.
        assert_eq!(
            offered(
                &unsaved,
                Some(&claude),
                &claude,
                &detected(&[Provider::Claude])
            ),
            None
        );
        // Signing in to Codex and refreshing: the untouched draft follows.
        assert_eq!(
            offered(&unsaved, Some(&claude), &claude, &detected(&PROVIDERS)),
            Some(Offer {
                suggested: both.clone(),
                replaces_draft: true
            })
        );
        // An edited selection or an edited verifier list is kept.
        let mut edited = claude.clone();
        edited.selection.guide.push_str("Mine.");
        let mut fewer = claude.clone();
        fewer.verification.verifiers.pop();
        for draft in [edited, fewer] {
            assert_eq!(
                offered(&unsaved, Some(&claude), &draft, &detected(&PROVIDERS)),
                Some(Offer {
                    suggested: both.clone(),
                    replaces_draft: false
                })
            );
        }
        // Half a detection pass suggests nothing yet.
        let half: Detection = detected(&PROVIDERS).into_iter().take(1).collect();
        assert_eq!(offer(&unsaved, None, &unsaved, None, &half), None);
    }
    #[test]
    fn a_saved_store_is_never_offered_a_suggestion() {
        let both = suggest(&detected(&PROVIDERS));
        // The host's answer to the first save carries revision 1; a detection result that lands
        // before the next snapshot must find the store saved.
        let saved = ModelSelection {
            revision: 1,
            ..both.selection.clone()
        };
        let draft = after_save(&both.selection, &both.selection, &saved);
        assert_eq!(draft, saved);
        assert_eq!(
            offer(
                &saved,
                None,
                &draft,
                Some(&both.verification),
                &detected(&[])
            ),
            None
        );
        // An edit made while the save was in flight stays, as unsaved changes.
        let mut edited = both.selection.clone();
        edited.guide.push_str("Mine.");
        assert_eq!(after_save(&edited, &both.selection, &saved), edited);
    }
    #[test]
    fn a_fresh_store_is_editable_only_once_its_first_suggestion_is_shown() {
        let unsaved = ModelSelection::default();
        let suggested = suggest(&detected(&PROVIDERS));
        assert!(!editable(&unsaved, None));
        assert!(editable(&unsaved, Some(&suggested)));
        // A saved store never waits for a suggestion.
        assert!(editable(&selection(), None));
    }
    #[test]
    fn save_needs_an_editable_page_and_a_valid_draft_and_no_edit_on_a_suggestion() {
        let both = suggest(&detected(&PROVIDERS)).selection;
        let neither = suggest(&detected(&[])).selection;
        assert!(can_save(true, true, false, &both));
        assert!(!can_save(true, true, false, &neither));
        assert!(!can_save(true, true, true, &neither));
        // A saved setup needs an edit first.
        assert!(!can_save(true, false, false, &selection()));
        assert!(can_save(true, false, true, &selection()));
        // Nothing saves before the first suggestion is shown.
        assert!(!can_save(false, false, true, &selection()));
    }
    #[test]
    fn the_suggested_tag_shows_only_while_the_provider_is_enabled_in_the_draft() {
        let suggested = suggest(&detected(&PROVIDERS));
        let mut draft = suggested.selection.clone();
        assert!(tagged_suggested(Some(&suggested), &draft, Provider::Codex));
        set_enabled(&mut draft, Provider::Codex, false);
        assert!(!tagged_suggested(Some(&suggested), &draft, Provider::Codex));
        assert!(tagged_suggested(Some(&suggested), &draft, Provider::Claude));
        set_enabled(&mut draft, Provider::Codex, true);
        assert!(tagged_suggested(Some(&suggested), &draft, Provider::Codex));
        // A provider the suggestion left out is not tagged when the human turns it on.
        let claude_only = suggest(&detected(&[Provider::Claude]));
        let mut draft = claude_only.selection.clone();
        set_enabled(&mut draft, Provider::Codex, true);
        assert!(!tagged_suggested(
            Some(&claude_only),
            &draft,
            Provider::Codex
        ));
        assert!(!tagged_suggested(None, &draft, Provider::Claude));
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
}
