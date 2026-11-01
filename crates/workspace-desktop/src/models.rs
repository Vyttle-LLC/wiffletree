//! Pure helpers behind the Models page and the model pickers.
use std::collections::BTreeSet;
use workspace_core::{
    Chooser, Detection, Job, JobRow, Limits, Line, ModelOption, ModelProfile, ModelSelection,
    PROVIDERS, Pick, Provider, ProviderStatus, QuotaReading, Role, Selection, Setup, Space,
    SpaceTable, Then, VerificationSettings, WorkspaceIndex, resolve, suggest, usual_efforts,
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
/// What a cell holds: a first choice, a then choice's model (none means wait or skip), or for
/// a small-work first choice, none meaning the row has no small-work line.
pub(crate) fn cell(row: &JobRow, cell: Cell) -> Option<&ModelProfile> {
    let line = if cell.small {
        row.small.as_ref()?
    } else {
        &row.main
    };
    if cell.then {
        line.then.model()
    } else {
        Some(&line.first)
    }
}
/// Sets one cell of `job`'s row. `None` makes a then choice wait, removes a small-work line,
/// and for the first choice of the PR keeper's, the Job or the Brain compiler row removes the
/// row: no PR keeper model, jobs that follow the Assistant row, or no compiles. A first choice
/// on the then choice's subscription drops the then choice, which would be spent with it.
pub(crate) fn set_cell(
    selection: &mut ModelSelection,
    job: &Job,
    cell: Cell,
    profile: Option<ModelProfile>,
) {
    let optional_first =
        matches!(job, Job::Keeper | Job::Job | Job::Compiler) && !cell.small && !cell.then;
    if optional_first && (profile.is_none() || selection.row(job).is_none()) {
        let line = profile.map(|first| Line {
            first,
            then: Then::Wait,
        });
        match job {
            Job::Keeper => selection.set_keeper(line),
            Job::Compiler => selection.set_compiler_row(line),
            _ => selection.set_job_row(line),
        }
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
            None => {
                row.small = Some(Line {
                    first,
                    then: Then::Wait,
                })
            }
        },
        (false, false, Some(first)) => set_first(&mut row.main, first),
        (small, true, then) => set_then(row, small, then.map_or(Then::Wait, Then::Model)),
    }
}
/// Sets the then choice of a row's main or small-work line, when the row has that line.
pub(crate) fn set_then(row: &mut JobRow, small: bool, then: Then) {
    let line = if small {
        row.small.as_mut()
    } else {
        Some(&mut row.main)
    };
    if let Some(line) = line {
        line.then = then;
    }
}
fn set_first(line: &mut Line, first: ModelProfile) {
    if line
        .then
        .model()
        .is_some_and(|then| then.provider == first.provider)
    {
        line.then = Then::Wait;
    }
    line.first = first;
}

/// Providers detection has not confirmed signed in, as the host counts them: one not checked
/// yet is not usable either.
pub(crate) fn unusable(detection: &Detection) -> BTreeSet<Provider> {
    PROVIDERS
        .into_iter()
        .filter(|provider| !detection.get(provider).is_some_and(ProviderStatus::usable))
        .collect()
}
fn all_checked(detection: &Detection) -> bool {
    PROVIDERS
        .iter()
        .all(|provider| detection.contains_key(provider))
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
    resolve(row, small, true, &limits, &reset_time).map_err(|e| e.to_string())
}
/// What `job`'s line would start on now as a Space's `table` resolves it, its reason naming the
/// Space's part.
pub(crate) fn space_right_now(
    table: &SpaceTable,
    job: &Job,
    small: bool,
    quotas: &[QuotaReading],
    detection: &Detection,
    now: i64,
) -> Result<Pick, String> {
    let unusable = unusable(detection);
    let limits = Limits {
        quotas,
        unusable: &unusable,
        switch_at: table.selection.switch_at,
        now,
    };
    (table.resolve(job, small, true, &limits, &reset_time)).map_err(|e| e.to_string())
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
    /// Its first choice is spent and its row says skip: skips reviews until this time, if known.
    Skips(Option<String>),
    Cannot,
    /// The router cannot run or would wait, so it asks you where each message goes.
    AsksYou,
    /// A provider's sign-in is still being checked, so the pick is not known.
    Checking,
}
pub(crate) fn now_of(
    row: &JobRow,
    small: bool,
    pick: &Result<Pick, String>,
    detection: &Detection,
) -> Now {
    if !all_checked(detection) {
        return Now::Checking;
    }
    let line = row.line(small);
    match pick {
        Ok(pick) if pick.waits && row.job == Job::Router => Now::AsksYou,
        Err(_) if row.job == Job::Router => Now::AsksYou,
        Ok(pick) if pick.waits => Now::Waits(pick.until.map(reset_time)),
        Ok(pick) if pick.skip.is_some() => Now::Skips(pick.until.map(reset_time)),
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

/// This Mac's table as `space` resolves it here with the Space's row overrides `rows`, which
/// may be the page's unsaved draft.
pub(crate) fn space_table(
    selection: &ModelSelection,
    space: Option<&Space>,
    rows: &[JobRow],
    detection: &Detection,
) -> SpaceTable {
    let name = space.map(|space| space.name.as_str());
    selection.for_space(name, rows, &unusable(detection))
}

/// The rows the Models page shows at a Space: this Mac's, each replaced by the Space's own where
/// it sets one, then the Space's rows for jobs this Mac has none for, each with whether the
/// Space sets it. A Space's row for a verifier this Mac does not configure is left out, as the
/// host leaves it out.
pub(crate) fn space_rows(selection: &ModelSelection, overrides: &[JobRow]) -> Vec<(JobRow, bool)> {
    let over = |job: &Job| overrides.iter().find(|row| row.job.is(job));
    let mut rows: Vec<(JobRow, bool)> = (selection.jobs.iter())
        .map(|row| match over(&row.job) {
            Some(space) => (space.clone(), true),
            None => (row.clone(), false),
        })
        .collect();
    rows.extend(
        (overrides.iter())
            .filter(|row| !matches!(row.job, Job::Verifier { .. }))
            .filter(|row| selection.row(&row.job).is_none())
            .map(|row| (row.clone(), true)),
    );
    rows
}

/// Sets one cell of `job`'s row at a Space: the Space's row, started as a copy of this Mac's
/// when it has none yet, as [`set_cell`] sets this Mac's.
pub(crate) fn set_space_cell(
    overrides: &mut Vec<JobRow>,
    selection: &ModelSelection,
    job: &Job,
    which: Cell,
    profile: Option<ModelProfile>,
) {
    let mut table = ModelSelection {
        jobs: std::mem::take(overrides),
        ..ModelSelection::default()
    };
    if table.row(job).is_none()
        && let Some(row) = selection.row(job)
    {
        table.jobs.push(row.clone());
    }
    set_cell(&mut table, job, which, profile);
    *overrides = table.jobs;
}

/// Drops the Space's row for `job`, so this Mac's row applies again.
pub(crate) fn reset_space_row(overrides: &mut Vec<JobRow>, job: &Job) {
    overrides.retain(|row| !row.job.is(job));
}

/// What each of a Space's rows this Mac cannot run falls back to, as the Models and Space pages
/// say it.
pub(crate) fn fallback_lines(table: &SpaceTable, space: &str) -> Vec<String> {
    let main = table.fallbacks(false).into_iter().map(|(row, why)| {
        format!(
            "{space} asks for {} for the {} row, but {why}, so it uses this Mac's row until then.",
            row.main.first.short(),
            row.job.name()
        )
    });
    let small = table.fallbacks(true).into_iter().filter_map(|(row, why)| {
        Some(format!(
            "{space} asks for {} for the {} row's small work, but {why}, so small work uses this Mac's row until then.",
            row.small.as_ref()?.first.short(),
            row.job.name()
        ))
    });
    main.chain(small).collect()
}

/// The model a new thread in `space` starts on when the user picks none: the first choice of
/// the Assistant row as the Space resolves it that this Mac can run. A provider not checked yet
/// counts as not usable, as the host counts it.
pub(crate) fn thread_model(
    selection: &ModelSelection,
    space: Option<&Space>,
    detection: &Detection,
) -> Option<ModelProfile> {
    let overrides = space.map_or(vec![], |space| space.models.overrides());
    let table = space_table(selection, space, &overrides, detection);
    let unusable = unusable(detection);
    let row = table.selection.row(&Job::Assistant)?;
    row.main
        .choices()
        .find(|profile| !unusable.contains(&profile.provider))
        .cloned()
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
        assert_eq!(row.main.then, Then::Wait);
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
        assert_eq!(
            selection.row(&Job::Implementer).unwrap().main.then,
            Then::Wait
        );
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
        // So does the Job row's, whose absence leaves jobs on the Assistant row.
        set_cell(
            &mut selection,
            &Job::Job,
            cell(false, false),
            Some(opus.clone()),
        );
        assert_eq!(selection.row(&Job::Job).unwrap().main.first, opus);
        set_cell(&mut selection, &Job::Job, cell(false, false), None);
        assert!(selection.row(&Job::Job).is_none());
        // And the Brain compiler's, whose absence leaves brains uncompiled.
        set_cell(
            &mut selection,
            &Job::Compiler,
            cell(false, false),
            Some(opus.clone()),
        );
        assert_eq!(selection.row(&Job::Compiler).unwrap().main.first, opus);
        selection.validate().unwrap();
        set_cell(&mut selection, &Job::Compiler, cell(false, false), None);
        assert!(selection.row(&Job::Compiler).is_none());
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
            now_of(&row, false, &pick, detection)
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
        // The router asks you rather than waiting, and on Codex alone it always asks.
        assert_eq!(pick(Job::Router, &[], &both), Now::First(Provider::Claude));
        assert_eq!(pick(Job::Router, &spent, &both), Now::AsksYou);
        let mut codex = row(Job::Router);
        codex.main.first = workspace_core::ModelProfile {
            provider: Provider::Codex,
            model: "gpt-6-luna".into(),
            effort: "low".into(),
        };
        let refused = right_now(&selection, &codex, false, &[], &both, now);
        assert_eq!(refused.clone().unwrap_err(), workspace_core::CODEX_ROUTER);
        assert_eq!(now_of(&codex, false, &refused, &both), Now::AsksYou);
        let correctness = Job::Verifier {
            focus: "Correctness".into(),
        };
        assert!(matches!(
            pick(correctness.clone(), &spent, &both),
            Now::Waits(Some(_))
        ));
        // A verifier whose row says skip skips until the reset instead of waiting.
        let mut skipping = row(correctness);
        set_then(&mut skipping, false, Then::Skip);
        let skips = right_now(&selection, &skipping, false, &spent, &both, now);
        assert!(matches!(
            now_of(&skipping, false, &skips, &both),
            Now::Skips(Some(_))
        ));
        assert_eq!(
            cell(
                &skipping,
                Cell {
                    small: false,
                    then: true
                }
            ),
            None
        );
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
    fn job_row(job: Job, first: &ModelProfile, then: Then) -> JobRow {
        JobRow {
            job,
            main: Line {
                first: first.clone(),
                then,
            },
            small: None,
        }
    }
    /// This Mac's table: the Implementer on Opus and the Assistant on Sonnet, each then Codex.
    fn this_macs() -> ModelSelection {
        let sol = Then::Model(profile(Provider::Codex, "gpt-6.1-sol", "medium"));
        ModelSelection {
            jobs: vec![
                job_row(
                    Job::Implementer,
                    &profile(Provider::Claude, "opus", "high"),
                    sol.clone(),
                ),
                job_row(
                    Job::Assistant,
                    &profile(Provider::Claude, "sonnet", "medium"),
                    sol,
                ),
            ],
            ..ModelSelection::default()
        }
    }
    fn sage(rows: Vec<JobRow>) -> Space {
        let mut space: Space = serde_json::from_value(serde_json::json!({
            "id": "sage", "name": "Sage",
            "storage": {"repository": {"kind": "local", "id": "s"}, "folder": "sage"}
        }))
        .unwrap();
        space.models.set_overrides(rows);
        space
    }
    #[test]
    fn at_a_space_an_edited_row_is_the_spaces_until_reset_restores_this_macs() {
        let table = this_macs();
        let (opus, sonnet) = (
            profile(Provider::Claude, "opus", "high"),
            profile(Provider::Claude, "sonnet", "medium"),
        );
        let first = Cell {
            small: false,
            then: false,
        };
        // At This Mac's scope, or a Space that sets nothing, every row is this Mac's.
        let mac_rows: Vec<(JobRow, bool)> =
            (table.jobs.iter()).map(|r| (r.clone(), false)).collect();
        assert_eq!(space_rows(&table, &[]), mac_rows);

        // An edit at a Space starts the Space's row as a copy of this Mac's and leaves the
        // table alone.
        let mut overrides = vec![];
        set_space_cell(
            &mut overrides,
            &table,
            &Job::Assistant,
            first,
            Some(opus.clone()),
        );
        let assistant = Line {
            first: opus.clone(),
            ..table.row(&Job::Assistant).unwrap().main.clone()
        };
        assert_eq!(overrides[0].main, assistant);
        assert_eq!(table.row(&Job::Assistant).unwrap().main.first, sonnet);
        let shown = space_rows(&table, &overrides);
        assert_eq!(shown[0], mac_rows[0]);
        assert_eq!(shown[1], (overrides[0].clone(), true));
        // Skip is for reviews only, so its then choice cannot be set to skip; a wait is kept.
        set_space_cell(
            &mut overrides,
            &table,
            &Job::Assistant,
            Cell {
                then: true,
                ..first
            },
            None,
        );
        assert_eq!(overrides[0].main.then, Then::Wait);

        reset_space_row(&mut overrides, &Job::Assistant);
        assert_eq!(space_rows(&table, &overrides), mac_rows);

        // A Space may add a row this Mac has none of, such as the Job row.
        set_space_cell(
            &mut overrides,
            &table,
            &Job::Job,
            first,
            Some(sonnet.clone()),
        );
        let job = job_row(Job::Job, &sonnet, Then::Wait);
        assert_eq!(space_rows(&table, &overrides).last(), Some(&(job, true)));
    }
    #[test]
    fn a_space_row_whose_provider_is_signed_out_falls_back_to_this_macs_row() {
        let table = this_macs();
        let opus = profile(Provider::Claude, "opus", "high");
        let sol = profile(Provider::Codex, "gpt-6.1-sol", "medium");
        let space = sage(vec![job_row(Job::Assistant, &opus, Then::Wait)]);
        let at = |detection: &Detection| {
            space_table(&table, Some(&space), &space.models.overrides(), detection)
        };
        let both = detected(&PROVIDERS);
        assert!(fallback_lines(&at(&both), "Sage").is_empty());
        assert_eq!(
            thread_model(&table, Some(&space), &both),
            Some(opus.clone())
        );
        // Outside a Space a thread starts on this Mac's row.
        assert_eq!(
            thread_model(&table, None, &both),
            Some(profile(Provider::Claude, "sonnet", "medium"))
        );

        let codex = detected(&[Provider::Codex]);
        assert_eq!(
            fallback_lines(&at(&codex), "Sage"),
            [
                "Sage asks for Opus · high for the Assistant row, but Claude is not signed in on this Mac, so it uses this Mac's row until then."
            ]
        );
        // The composer's label follows: this Mac's row, on the choice it can run.
        assert_eq!(
            thread_model(&table, Some(&space), &codex),
            Some(sol.clone())
        );

        // A provider not checked yet counts as unavailable, as the host counts it.
        let claude_unchecked: Detection = (codex.into_iter())
            .filter(|(provider, _)| *provider == Provider::Codex)
            .collect();
        assert_eq!(fallback_lines(&at(&claude_unchecked), "Sage").len(), 1);
        assert_eq!(
            thread_model(&table, Some(&space), &claude_unchecked),
            Some(sol.clone())
        );

        // A Space row whose small-work line alone cannot run falls back for small work only.
        let mut implementer = job_row(Job::Implementer, &opus, Then::Wait);
        implementer.small = Some(Line {
            first: sol,
            then: Then::Wait,
        });
        let space = sage(vec![implementer]);
        let claude = detected(&[Provider::Claude]);
        let table = space_table(&table, Some(&space), &space.models.overrides(), &claude);
        assert!(table.fallbacks(false).is_empty(), "its main line runs here");
        assert_eq!(table.fallbacks(true).len(), 1);
        assert_eq!(
            fallback_lines(&table, "Sage"),
            [
                "Sage asks for GPT 6.1 Sol · medium for the Implementer row's small work, but Codex is not signed in on this Mac, so small work uses this Mac's row until then."
            ]
        );
    }
    #[test]
    fn a_provider_not_checked_yet_is_unusable_and_shows_as_checking() {
        let selection = suggested().selection;
        let row = selection.row(&Job::Coordinator).unwrap();
        let nothing_yet = Detection::new();
        assert_eq!(unusable(&nothing_yet), PROVIDERS.into_iter().collect());
        let pick = right_now(&selection, row, false, &[], &nothing_yet, 1_000_000);
        assert!(pick.is_err());
        assert_eq!(now_of(row, false, &pick, &nothing_yet), Now::Checking);
        // Only Codex has answered, signed in: Claude is not assumed usable meanwhile.
        let mut codex_checked = nothing_yet.clone();
        codex_checked.extend(
            detected(&[Provider::Codex])
                .into_iter()
                .filter(|(p, _)| *p == Provider::Codex),
        );
        assert_eq!(unusable(&codex_checked), [Provider::Claude].into());
        let pick = right_now(&selection, row, false, &[], &codex_checked, 1_000_000);
        assert_eq!(pick.unwrap().profile.provider, Provider::Codex);
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
