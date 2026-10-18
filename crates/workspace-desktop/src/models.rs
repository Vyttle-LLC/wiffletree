//! Pure helpers behind the Models page and the model pickers.
use std::collections::BTreeSet;
use workspace_core::{
    Chooser, Detection, Job, JobRow, Limits, Line, ModelOption, ModelProfile, ModelSelection,
    PROVIDERS, Pick, Provider, ProviderStatus, QuotaReading, Role, Selection, Setup,
    VerificationSettings, WorkspaceIndex, resolve, suggest, usual_efforts,
};

pub(crate) fn choices(
    catalog: &[ModelOption],
    provider: Provider,
    saved: &[&str],
) -> Vec<ModelOption> {
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
pub(crate) fn effort_choices(
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
            (usual_efforts(provider).iter())
                .map(|e| (*e).to_owned())
                .collect()
        });
    for saved in saved {
        if !efforts.iter().any(|effort| effort == saved) {
            efforts.push((*saved).into());
        }
    }
    efforts
}

pub(crate) fn model_label(catalog: &[ModelOption], provider: Provider, model: &str) -> String {
    catalog
        .iter()
        .find(|m| m.provider == provider && m.model == model)
        .map_or_else(|| model.to_owned(), |m| m.label.clone())
}

/// "Claude · Opus · latest · High", as pickers show a profile.
pub(crate) fn profile_label(catalog: &[ModelOption], profile: &ModelProfile) -> String {
    format!(
        "{} · {}",
        profile.provider.label(),
        cell_label(catalog, profile)
    )
}
/// "Opus · latest · High", as a table cell shows a profile beside its provider's dot.
pub(crate) fn cell_label(catalog: &[ModelOption], profile: &ModelProfile) -> String {
    format!(
        "{} · {}",
        model_label(catalog, profile.provider, &profile.model),
        crate::wording::effort_label(&profile.effort)
    )
}

/// Every profile a table dropdown offers on `provider`: each catalog model at each of its
/// efforts, then the table's own profiles the catalog no longer lists, so a saved choice stays
/// selectable.
pub(crate) fn profile_choices(
    catalog: &[ModelOption],
    selection: &ModelSelection,
    provider: Provider,
) -> Vec<ModelProfile> {
    let saved: Vec<&ModelProfile> = table_profiles(selection)
        .into_iter()
        .filter(|p| p.provider == provider)
        .collect();
    let models: Vec<&str> = saved.iter().map(|p| p.model.as_str()).collect();
    let mut profiles = vec![];
    for option in choices(catalog, provider, &models) {
        let efforts: Vec<&str> = (saved.iter())
            .filter(|p| p.model == option.model)
            .map(|p| p.effort.as_str())
            .collect();
        for effort in effort_choices(catalog, provider, &option.model, &efforts) {
            profiles.push(ModelProfile {
                provider,
                model: option.model.clone(),
                effort,
            });
        }
    }
    profiles
}
/// Every profile the table names, each once.
pub(crate) fn table_profiles(selection: &ModelSelection) -> Vec<&ModelProfile> {
    let mut profiles: Vec<&ModelProfile> = vec![];
    for row in &selection.jobs {
        for line in std::iter::once(&row.main).chain(&row.small) {
            for profile in line.choices() {
                if !profiles.contains(&profile) {
                    profiles.push(profile);
                }
            }
        }
    }
    profiles
}

/// Which cell of a row a dropdown edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cell {
    pub small: bool,
    pub then: bool,
}
/// What a cell holds: a first choice, a then choice (none means wait), or for a small-work
/// first choice, none meaning the row has no small-work line.
pub(crate) fn cell(row: &JobRow, cell: Cell) -> Option<&ModelProfile> {
    let line = if cell.small {
        row.small.as_ref()?
    } else {
        &row.main
    };
    if cell.then {
        line.then.as_ref()
    } else {
        Some(&line.first)
    }
}
/// Sets one cell of `job`'s row. `None` makes a then choice wait, removes a small-work line,
/// and for the PR keeper's first choice removes the row: no PR keeper model. A first choice
/// on the then choice's subscription drops the then choice, which would be spent with it.
pub(crate) fn set_cell(
    selection: &mut ModelSelection,
    job: &Job,
    cell: Cell,
    profile: Option<ModelProfile>,
) {
    let keeper_first = *job == Job::Keeper && !cell.small && !cell.then;
    if keeper_first && (profile.is_none() || selection.keeper().is_none()) {
        selection.set_keeper(profile.map(|first| Line { first, then: None }));
        return;
    }
    let Some(row) = selection.row_mut(job) else {
        return;
    };
    match (cell.small, cell.then, profile) {
        (false, false, None) => {}
        (true, false, None) => row.small = None,
        (true, false, Some(first)) => match &mut row.small {
            Some(line) => set_first(line, first),
            None => row.small = Some(Line { first, then: None }),
        },
        (false, false, Some(first)) => set_first(&mut row.main, first),
        (small, true, then) => {
            let line = if small {
                row.small.as_mut()
            } else {
                Some(&mut row.main)
            };
            if let Some(line) = line {
                line.then = then;
            }
        }
    }
}
fn set_first(line: &mut Line, first: ModelProfile) {
    if line
        .then
        .as_ref()
        .is_some_and(|then| then.provider == first.provider)
    {
        line.then = None;
    }
    line.first = first;
}

/// Providers detection found missing or signed out; none detected yet is usable.
pub(crate) fn unusable(detection: &Detection) -> BTreeSet<Provider> {
    (detection.iter())
        .filter(|(_, status)| !status.usable())
        .map(|(provider, _)| *provider)
        .collect()
}
/// What a row's line would start on now, as the host would resolve it.
pub(crate) fn right_now(
    selection: &ModelSelection,
    row: &JobRow,
    small: bool,
    quotas: &[QuotaReading],
    detection: &Detection,
    now: i64,
) -> Result<Pick, String> {
    let unusable = unusable(detection);
    let limits = Limits {
        quotas,
        unusable: &unusable,
        switch_at: selection.switch_at,
        now,
    };
    resolve(row, small, &limits, &reset_time).map_err(|e| e.to_string())
}
/// A reset time as reasons write it, "Mon 09:00", in this Mac's time zone.
pub(crate) fn reset_time(at: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(at)
        .single()
        .map_or_else(String::new, |at| at.format("%a %H:%M").to_string())
}
/// How the Right now column reads a pick.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Now {
    /// Starts on its first choice's provider.
    First(Provider),
    /// Starts on another choice.
    Moved(Provider),
    /// Every choice is spent: waits until this time, if known.
    Waits(Option<String>),
    Cannot,
}
pub(crate) fn now_of(row: &JobRow, small: bool, pick: &Result<Pick, String>) -> Now {
    let line = match (&row.small, small) {
        (Some(line), true) => line,
        _ => &row.main,
    };
    match pick {
        Ok(pick) if pick.waits => Now::Waits(pick.until.map(reset_time)),
        Ok(pick) if pick.profile == line.first => Now::First(pick.profile.provider),
        Ok(pick) => Now::Moved(pick.profile.provider),
        Err(_) => Now::Cannot,
    }
}

/// "CLI found · signed in · Max plan", as the Subscriptions card shows what detection found.
pub(crate) fn status_label(status: &ProviderStatus) -> String {
    if !status.installed {
        return "CLI not found".into();
    }
    if !status.signed_in {
        return "CLI found · not signed in".into();
    }
    let plan = status
        .plan
        .as_deref()
        .map(|plan| format!(" · {} plan", crate::wording::humanize(plan)));
    format!("CLI found · signed in{}", plan.unwrap_or_default())
}
/// The one-line fix for a provider a suggested setup left out, or `None` when it is usable.
pub(crate) fn fix_hint(provider: Provider, status: &ProviderStatus) -> Option<String> {
    fix(provider, status, ", then Refresh")
}
/// The same fix for the welcome, which has no Refresh to point at.
pub(crate) fn welcome_fix_hint(provider: Provider, status: &ProviderStatus) -> Option<String> {
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
pub(crate) fn editable(saved: &ModelSelection, suggested: Option<&Setup>) -> bool {
    saved.is_saved() || suggested.is_some()
}
/// Whether the draft still is the suggested setup, verifiers included.
pub(crate) fn untouched(
    suggested: &Setup,
    selection: &ModelSelection,
    verification: Option<&VerificationSettings>,
) -> bool {
    *selection == suggested.selection && verification == Some(&suggested.verification)
}
/// A setup to suggest after a detection pass.
#[derive(Debug, PartialEq)]
pub(crate) struct Offer {
    pub suggested: Setup,
    /// The draft follows it: it was untouched, or nothing was suggested before.
    pub replaces_draft: bool,
}
/// What a detection pass suggests for a store with no saved setup; `None` on a saved store,
/// before every provider is detected, or when `shown` already is that suggestion. A draft the
/// human edited is kept, and so is a guide already in the shared folder, even an empty one.
pub(crate) fn offer(
    saved: &ModelSelection,
    guide_saved: bool,
    shown: Option<&Setup>,
    draft: (&ModelSelection, Option<&VerificationSettings>),
    detection: &Detection,
    catalog: &[ModelOption],
) -> Option<Offer> {
    if saved.is_saved() || !PROVIDERS.iter().all(|p| detection.contains_key(p)) {
        return None;
    }
    let mut suggested = suggest(detection, catalog);
    if guide_saved {
        suggested.selection.guide = saved.guide.clone();
    }
    if shown == Some(&suggested) {
        return None;
    }
    let (selection, verification) = draft;
    Some(Offer {
        replaces_draft: shown.is_none_or(|shown| untouched(shown, selection, verification)),
        suggested,
    })
}
/// Save needs an editable page and a valid draft that differs from what is saved; a suggested
/// setup is unsaved from the start, so it needs no edit first.
pub(crate) fn can_save(
    editable: bool,
    suggested: bool,
    edited: bool,
    selection: &ModelSelection,
) -> bool {
    editable && (suggested || edited) && selection.validate().is_ok()
}
/// The draft once the host has saved `sent`: it takes the saved revision unless the human
/// edited it meanwhile.
pub(crate) fn after_save(
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

/// What the human's picker starts on for an agent of `role` with `focus`: what its row, the
/// one the host compares the pick with, would use now.
pub(crate) fn prefill(
    selection: &ModelSelection,
    verification: &VerificationSettings,
    (role, focus): (Role, Option<&str>),
    quotas: &[QuotaReading],
    detection: &Detection,
    now: i64,
) -> Option<ModelProfile> {
    let job = Job::for_role(role, focus, verification)?;
    let row = selection.row(&job)?;
    Some(
        right_now(selection, row, false, quotas, detection, now)
            .ok()?
            .profile,
    )
}
/// A group of the human's model picker.
pub(crate) struct PickerGroup {
    pub heading: String,
    pub profiles: Vec<ModelProfile>,
}
/// The row's choices first, then every other model the human may pick.
pub(crate) fn picker_groups(
    selection: &ModelSelection,
    verification: &VerificationSettings,
    catalog: &[ModelOption],
    (role, focus): (Role, Option<&str>),
) -> Vec<PickerGroup> {
    let row = Job::for_role(role, focus, verification).and_then(|job| selection.row(&job));
    let listed: Vec<ModelProfile> = row.map_or(vec![], |row| {
        let lines = std::iter::once(&row.main).chain(&row.small);
        let mut profiles: Vec<ModelProfile> = vec![];
        for profile in lines.flat_map(Line::choices) {
            if !profiles.contains(profile) {
                profiles.push(profile.clone());
            }
        }
        profiles
    });
    let mut groups = vec![];
    if !listed.is_empty() {
        groups.push(PickerGroup {
            heading: format!("{} · Who runs what", role.label()),
            profiles: listed.clone(),
        });
    }
    // Every catalog model at every effort: the human may pick anything.
    let others: Vec<ModelProfile> = PROVIDERS
        .into_iter()
        .flat_map(|provider| profile_choices(catalog, selection, provider))
        .filter(|profile| !listed.contains(profile))
        .collect();
    if !others.is_empty() {
        groups.push(PickerGroup {
            heading: "Other models".into(),
            profiles: others,
        });
    }
    groups
}

/// How an agent's model was chosen, as its chat shows it.
pub(crate) fn chosen_by(selection: &Selection, index: &WorkspaceIndex) -> String {
    match &selection.chosen_by {
        Chooser::Wiffletree => format!("Chosen by Wiffletree: {}", selection.reason),
        Chooser::Coordinator { session_id } => {
            let name = (index.session(session_id)).map_or("its coordinator", |s| s.name.as_str());
            format!("Chosen by {name}: {}", selection.reason)
        }
        Chooser::Human => "Chosen by you".into(),
        Chooser::Default => selection.reason.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workspace_core::{QuotaWindow, fallback_catalog};

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
    fn suggested() -> Setup {
        suggest(&detected(&PROVIDERS), &fallback_catalog())
    }
    fn profile(provider: Provider, model: &str, effort: &str) -> ModelProfile {
        ModelProfile {
            provider,
            model: model.into(),
            effort: effort.into(),
        }
    }
    fn claude_weekly(used: f64, now: i64) -> Vec<QuotaReading> {
        let window = QuotaWindow {
            key: "seven_day".into(),
            label: "Weekly".into(),
            used_percent: Some(used),
            observed_at: now,
            duration_mins: Some(10080),
            resets_at: Some(now + 3_600_000),
            status: None,
        };
        vec![QuotaReading {
            provider: Provider::Claude,
            account: None,
            observed_at: now,
            source: "test".into(),
            windows: vec![window],
            error: None,
        }]
    }
    fn offered(
        saved: &ModelSelection,
        shown: Option<&Setup>,
        draft: &Setup,
        detection: &Detection,
    ) -> Option<Offer> {
        let draft = (&draft.selection, Some(&draft.verification));
        offer(saved, false, shown, draft, detection, &fallback_catalog())
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
        assert_eq!(fix_hint(Provider::Claude, &signed_in), None);
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
        // The welcome has no Refresh, so its fix names none; the rest is the same text.
        assert_eq!(
            welcome_fix_hint(Provider::Codex, &signed_out).unwrap(),
            "Run codex login to add it."
        );
        assert_eq!(
            welcome_fix_hint(Provider::Claude, &missing),
            fix_hint(Provider::Claude, &missing)
        );
    }
    #[test]
    fn an_untouched_suggestion_follows_refresh_and_an_edited_one_is_kept() {
        let unsaved = ModelSelection::default();
        let claude = suggest(&detected(&[Provider::Claude]), &fallback_catalog());
        let both = suggested();
        let first = offer(
            &unsaved,
            false,
            None,
            (&unsaved, None),
            &detected(&[Provider::Claude]),
            &fallback_catalog(),
        );
        assert_eq!(
            first,
            Some(Offer {
                suggested: claude.clone(),
                replaces_draft: true
            })
        );
        // The same detection again offers nothing new.
        let again = offered(
            &unsaved,
            Some(&claude),
            &claude,
            &detected(&[Provider::Claude]),
        );
        assert_eq!(again, None);
        // Signing in to Codex and refreshing: the untouched draft follows.
        assert_eq!(
            offered(&unsaved, Some(&claude), &claude, &detected(&PROVIDERS)),
            Some(Offer {
                suggested: both.clone(),
                replaces_draft: true
            })
        );
        // An edited table or verifier list is kept.
        let mut edited = claude.clone();
        edited.selection.switch_at = 90;
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
    }
    #[test]
    fn a_saved_store_is_never_offered_a_suggestion_and_keeps_edits_made_while_saving() {
        let both = suggested();
        let saved = ModelSelection {
            revision: 1,
            ..both.selection.clone()
        };
        let draft = after_save(&both.selection, &both.selection, &saved);
        assert_eq!(draft, saved);
        assert_eq!(offered(&saved, None, &both, &detected(&[])), None);
        let mut edited = both.selection.clone();
        edited.guide.push_str("Mine.");
        assert_eq!(after_save(&edited, &both.selection, &saved), edited);
    }
    #[test]
    fn save_needs_an_editable_page_and_a_valid_draft_and_no_edit_on_a_suggestion() {
        let both = suggested().selection;
        let neither = suggest(&detected(&[]), &fallback_catalog()).selection;
        assert!(can_save(true, true, false, &both));
        assert!(!can_save(true, true, false, &neither));
        assert!(!can_save(true, false, false, &both));
        assert!(!can_save(false, false, true, &both));
        assert!(!editable(&ModelSelection::default(), None));
        assert!(editable(&ModelSelection::default(), Some(&suggested())));
    }
    #[test]
    fn a_cell_edit_keeps_each_line_on_two_subscriptions() {
        let mut selection = suggested().selection;
        let cell = |small, then| Cell { small, then };
        let opus = profile(Provider::Claude, "opus", "high");
        // The first choice moving onto the then choice's subscription drops the then choice.
        set_cell(
            &mut selection,
            &Job::Coordinator,
            cell(false, false),
            Some(profile(Provider::Codex, "gpt-6.1-sol", "medium")),
        );
        let row = selection.row(&Job::Coordinator).unwrap();
        assert_eq!(row.main.then, None);
        selection.validate().unwrap();
        // Then may be set to wait, and a small-work line added and removed.
        set_cell(&mut selection, &Job::Implementer, cell(false, true), None);
        set_cell(
            &mut selection,
            &Job::Tester,
            cell(true, false),
            Some(opus.clone()),
        );
        let tester = selection.row(&Job::Tester).unwrap();
        assert_eq!(cell_of(tester, true), Some(&opus));
        set_cell(&mut selection, &Job::Tester, cell(true, false), None);
        assert!(selection.row(&Job::Tester).unwrap().small.is_none());
        assert_eq!(selection.row(&Job::Implementer).unwrap().main.then, None);
        // The PR keeper's first choice of None removes the row; choosing one adds it back.
        set_cell(&mut selection, &Job::Keeper, cell(false, false), None);
        assert!(selection.keeper().is_none());
        set_cell(
            &mut selection,
            &Job::Keeper,
            cell(false, false),
            Some(opus.clone()),
        );
        assert_eq!(selection.keeper().unwrap().main.first, opus);
        selection.validate().unwrap();
    }
    fn cell_of(row: &JobRow, small: bool) -> Option<&ModelProfile> {
        cell(row, Cell { small, then: false })
    }
    #[test]
    fn right_now_follows_the_limits_and_the_sign_in() {
        let selection = suggested().selection;
        let now = 1_000_000;
        let row = |job: Job| selection.row(&job).unwrap().clone();
        let both = detected(&PROVIDERS);
        let pick = |job, quotas: &[QuotaReading], detection: &Detection| {
            let row = row(job);
            let pick = right_now(&selection, &row, false, quotas, detection, now);
            now_of(&row, false, &pick)
        };
        assert_eq!(
            pick(Job::Implementer, &[], &both),
            Now::First(Provider::Claude)
        );
        let spent = claude_weekly(96., now);
        assert_eq!(
            pick(Job::Implementer, &spent, &both),
            Now::Moved(Provider::Codex)
        );
        let correctness = Job::Verifier {
            focus: "Correctness".into(),
        };
        assert!(matches!(
            pick(correctness, &spent, &both),
            Now::Waits(Some(_))
        ));
        // A signed-out provider is never picked.
        let codex_only = detected(&[Provider::Codex]);
        assert_eq!(
            pick(Job::Coordinator, &[], &codex_only),
            Now::Moved(Provider::Codex)
        );
        assert_eq!(
            pick(
                Job::Verifier {
                    focus: "Correctness".into()
                },
                &[],
                &codex_only
            ),
            Now::Cannot
        );
    }
    #[test]
    fn the_picker_starts_on_the_rows_pick_and_still_offers_every_model() {
        let setup = suggested();
        let (selection, verification) = (&setup.selection, &setup.verification);
        let now = 1_000_000;
        let quotas = claude_weekly(96., now);
        let both = detected(&PROVIDERS);
        assert_eq!(
            prefill(
                selection,
                verification,
                (Role::Implementer, None),
                &[],
                &both,
                now
            ),
            Some(profile(Provider::Claude, "opus", "medium"))
        );
        assert_eq!(
            prefill(
                selection,
                verification,
                (Role::Implementer, None),
                &quotas,
                &both,
                now
            ),
            Some(profile(Provider::Codex, "gpt-6.1-sol", "high"))
        );
        // A reviewer is pre-filled from the first reviewer's row.
        assert_eq!(
            prefill(
                selection,
                verification,
                (Role::Reviewer, None),
                &[],
                &both,
                now
            ),
            Some(profile(Provider::Claude, "opus", "high"))
        );
        // A focus naming a configured verifier is pre-filled from that verifier's own row.
        assert_eq!(
            prefill(
                selection,
                verification,
                (Role::Reviewer, Some("Regressions")),
                &[],
                &both,
                now
            ),
            Some(profile(Provider::Codex, "gpt-6.1-sol", "high"))
        );
        let groups = picker_groups(
            selection,
            verification,
            &fallback_catalog(),
            (Role::Implementer, None),
        );
        assert_eq!(groups[0].heading, "Implementer · Who runs what");
        assert_eq!(
            groups[0].profiles[0],
            profile(Provider::Claude, "opus", "medium")
        );
        let others = &groups[1].profiles;
        for (model, effort) in [
            ("opus", "max"),
            ("sonnet", "low"),
            ("gpt-6-astra", "minimal"),
        ] {
            assert!(
                others
                    .iter()
                    .any(|p| p.model == model && p.effort == effort),
                "{model} · {effort}"
            );
        }
        assert!(!others.contains(&groups[0].profiles[0]));
    }
    #[test]
    fn dropdowns_keep_a_saved_model_the_catalog_no_longer_lists() {
        let mut selection = suggested().selection;
        let legacy = profile(Provider::Claude, "opus-legacy", "medium");
        selection.row_mut(&Job::Tester).unwrap().main.first = legacy.clone();
        let offered = profile_choices(&fallback_catalog(), &selection, Provider::Claude);
        assert!(offered.contains(&legacy));
        assert!(offered.iter().all(|p| p.provider == Provider::Claude));
        assert!(offered.contains(&profile(Provider::Claude, "sonnet", "xhigh")));
    }
}
