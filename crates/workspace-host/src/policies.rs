use crate::*;

const UNPINNED: &str = "This agent has no pinned model. Choose one from its model menu; Wiffletree never substitutes a model.";
/// Why nothing starts on a store whose human has not saved a setup yet.
pub(crate) const NO_SETUP: &str = "No model setup is saved on this machine yet. Open Models, review the suggested setup and press Save. Agents cannot start until a setup is saved.";
/// The one-time note the upgrade to the Who runs what table leaves on the Models page.
pub(crate) const MIGRATED_NOTE: &str = "Set up from your previous settings. Check it.";
use std::collections::{BTreeMap, BTreeSet};

/// What the keeper's triggers need from a table and review settings about to be saved.
pub(crate) fn check_keeper_needs(
    keeper: &KeeperSettings,
    selection: &ModelSelection,
    verification: &VerificationSettings,
) -> Result<()> {
    if keeper.comments {
        check_sorting_model(selection.keeper())?;
    }
    check_tries(keeper, selection.keeper(), verification)
}

/// A reset time as reasons write it, such as "Mon 09:00", in this machine's time zone.
pub(crate) fn reset_time(at: i64) -> String {
    chrono::DateTime::from_timestamp_millis(at).map_or_else(String::new, |at| {
        at.with_timezone(&chrono::Local)
            .format("%a %H:%M")
            .to_string()
    })
}

/// Why `quotas` say `provider`'s limit is reached at `at`, whatever the threshold: a window
/// reported rejected or full, whose reset is unknown or still ahead.
pub(crate) fn limit_reached(quotas: &[QuotaReading], provider: Provider, at: i64) -> Option<Spent> {
    let limits = Limits {
        quotas,
        unusable: &BTreeSet::new(),
        switch_at: 100,
        now: at,
    };
    limits.spent(provider)
}

impl Host {
    /// Opens a store with both providers counted as signed in and, when it has no saved setup,
    /// saves the one suggested then, without a PR keeper model, which only a store that uses
    /// one chooses. Tests, benchmarks and scripted stores start from it; the app detects its
    /// CLIs and only ever saves the setup the human pressed Save on.
    pub fn open_set_up(home: impl AsRef<Path>) -> Result<Self> {
        let mut host = Self::open(home)?;
        let signed_in = ProviderStatus {
            installed: true,
            signed_in: true,
            plan: None,
        };
        for provider in PROVIDERS {
            host.record_detection(provider, &signed_in);
        }
        if !host.model_selection()?.is_saved() {
            let detection = PROVIDERS.map(|p| (p, signed_in.clone())).into();
            let mut setup = suggest(&detection, &fallback_catalog());
            setup.selection.set_keeper(None);
            host.set_model_selection(&setup.selection)?;
        }
        Ok(host)
    }
    /// This machine's table, with the shared guide and threshold, fitted to the configured
    /// verifiers.
    pub fn model_selection(&self) -> Result<ModelSelection> {
        let shared = self.shared();
        let mut selection = ModelSelection {
            guide: shared.guide().unwrap_or_default(),
            switch_at: shared.switch_at(),
            ..self.saved_selection()?.unwrap_or_default()
        };
        selection.fit(&shared.review());
        Ok(selection)
    }
    /// The database row, whose guide is the one saved before the shared folder.
    pub(crate) fn saved_selection(&self) -> Result<Option<ModelSelection>> {
        Ok(self
            .db
            .query_row("SELECT data FROM model_selection", [], |r| decode(r, 0))
            .optional()?)
    }
    fn save_selection_row(&self, row: &ModelSelection) -> Result<()> {
        self.db.execute(
            "INSERT INTO model_selection VALUES (1,?1) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            [encode(row)?],
        )?;
        Ok(())
    }
    /// Saves the table at the next revision, the guide and the threshold in the shared folder,
    /// all or none. The saved one-time notes stay. Existing agents keep their models.
    pub fn set_model_selection(&mut self, selection: &ModelSelection) -> Result<ModelSelection> {
        selection.validate()?;
        let settings = self.settings();
        check_keeper_needs(&settings.keeper, selection, &settings.verification)?;
        let row = self.saved_selection()?.unwrap_or_default();
        let model_changed = row.sorting_model() != selection.sorting_model();
        let saved = ModelSelection {
            revision: row.revision + 1,
            notes: row.notes.clone(),
            copied: vec![],
            ..selection.clone()
        };
        let unchanged_guide = ModelSelection {
            guide: row.guide,
            ..saved.clone()
        };
        self.put_back_on_error(shared::GUIDE, |host| {
            host.put_back_on_error(shared::MODELS, |host| {
                host.shared().set_guide(&saved.guide)?;
                host.shared().set_switch_at(saved.switch_at)?;
                host.save_selection_row(&unchanged_guide)
            })
        })?;
        // A comments trigger held off for want of a keeper model starts afresh, as turning it on does.
        if !settings.keeper.comments && self.settings().keeper.comments {
            self.restart_comments()?;
        }
        if model_changed {
            self.forget_merges_waiting_for_a_model();
        }
        self.model_selection()
    }
    /// Clears the Models page's one-time notes; they change no resolution, so the revision stays.
    pub fn dismiss_model_notes(&mut self) -> Result<ModelSelection> {
        if let Some(mut row) = self.saved_selection()? {
            row.notes.clear();
            self.save_selection_row(&row)?;
        }
        self.model_selection()
    }
    /// Records what detection found for a provider's CLI; a missing or signed-out one is never
    /// chosen.
    pub fn record_detection(&mut self, provider: Provider, status: &ProviderStatus) {
        self.detected.insert(provider, status.usable());
    }
    /// What the latest readings, this machine's CLIs and the threshold say now. A provider
    /// detection has not found signed in, or not checked yet, is never chosen.
    pub(crate) fn limits(&self) -> Result<(Vec<QuotaReading>, BTreeSet<Provider>, u32)> {
        let unusable = PROVIDERS
            .into_iter()
            .filter(|provider| self.detected.get(provider) != Some(&true))
            .collect();
        Ok((self.quotas()?, unusable, self.shared().switch_at()))
    }
    /// Why `provider` counts as spent now, if it does.
    pub(crate) fn spent(&self, provider: Provider) -> Result<Option<Spent>> {
        let (quotas, unusable, switch_at) = self.limits()?;
        let limits = Limits {
            quotas: &quotas,
            unusable: &unusable,
            switch_at,
            now: now(),
        };
        Ok(limits.spent(provider))
    }
    /// What `job` starts on now, from its row, for small work when `small`. Only a verifier
    /// starting a review passes `may_skip`; otherwise a then of skip waits.
    pub(crate) fn resolve_job(&self, job: &Job, small: bool, may_skip: bool) -> Result<Pick> {
        let selection = self.model_selection()?;
        let (quotas, unusable, switch_at) = self.limits()?;
        let limits = Limits {
            quotas: &quotas,
            unusable: &unusable,
            switch_at,
            now: now(),
        };
        selection.resolve(job, small, may_skip, &limits, &reset_time)
    }
    /// Pins a session's model and records how it was chosen.
    pub(crate) fn pin(
        &mut self,
        session_id: &str,
        profile: ModelProfile,
        chosen_by: Chooser,
        reason: &str,
    ) -> Result<SessionRuntime> {
        let mut runtime = self.session_runtime(session_id)?;
        runtime.selection = Some(Selection {
            chosen_by,
            reason: reason.into(),
            revision: self.model_selection()?.revision,
            at: now(),
        });
        runtime.profile = Some(profile);
        runtime.waiting = false;
        self.save_runtime(&runtime)?;
        Ok(runtime)
    }
    /// Pins what Wiffletree resolved for a session, which waits when every choice was spent,
    /// and logs it.
    pub(crate) fn pin_pick(&mut self, session: &Session, pick: &Pick) -> Result<()> {
        let mut runtime = self.pin(
            &session.id,
            pick.profile.clone(),
            Chooser::Wiffletree,
            &pick.reason,
        )?;
        runtime.waiting = pick.waits;
        self.save_runtime(&runtime)?;
        Self::event(
            &self.db,
            &session.project_id,
            Some(&session.id),
            "model_selected",
            &format!("{}. {}", pick.profile, pick.reason),
        )
    }
    /// What the human's picker starts on for an agent of `role`: what its row would use now.
    pub(crate) fn prefill(&self, role: Role, focus: Option<&str>) -> Option<ModelProfile> {
        let job = Job::for_role(role, focus, &self.settings().verification)?;
        Some(self.resolve_job(&job, false, false).ok()?.profile)
    }
    /// Pins a session the human created: "From Who runs what" when it matches the picker's
    /// pre-fill.
    pub(crate) fn pin_human_choice(
        &mut self,
        session: &Session,
        profile: ModelProfile,
    ) -> Result<()> {
        let focus = self.session_runtime(&session.id)?.focus;
        let (chosen_by, reason) =
            if self.prefill(session.role, focus.as_deref()).as_ref() == Some(&profile) {
                (Chooser::Default, "From Who runs what")
            } else {
                (Chooser::Human, "Chosen by you")
            };
        self.pin(&session.id, profile, chosen_by, reason)?;
        Ok(())
    }
    /// The model a session's next turn uses: its pinned profile. Only an unpinned main
    /// coordinator, from before pins, takes its row's first choice on its own provider; any
    /// other agent was given a model when it was created, so a missing one is never replaced.
    pub fn turn_profile(&self, session: &Session) -> Result<ModelProfile> {
        if let Some(profile) = self.session_runtime(&session.id)?.profile {
            return Ok(profile);
        }
        ensure!(session.parent_id.is_none(), "{UNPINNED}");
        let selection = self.model_selection()?;
        let row = selection.row(&Job::Coordinator);
        row.and_then(|row| {
            (row.main.choices()).find(|profile| profile.provider == session.provider)
        })
        .cloned()
        .with_context(|| {
            format!(
                "Choose a model for this agent; the Coordinator row names no {} model",
                session.provider.label()
            )
        })
    }
    /// Why a session may not run here: it has no pinned model, or it was pinned while every
    /// choice of its job was spent and its provider still is.
    pub fn hold_reason(&self, session: &Session) -> Result<Option<String>> {
        let runtime = self.session_runtime(&session.id)?;
        let Some(profile) = runtime.profile else {
            return Ok(session.parent_id.is_some().then(|| UNPINNED.to_owned()));
        };
        if !runtime.waiting {
            return Ok(None);
        }
        let switch_at = self.shared().switch_at();
        Ok(self
            .spent(profile.provider)?
            .map(|spent| spent.waiting(switch_at, &reset_time)))
    }
    /// Why the latest readings say `provider`'s limit is reached at `at`; see `limit_reached`.
    pub(crate) fn limit_reached(&self, provider: Provider, at: i64) -> Result<Option<Spent>> {
        Ok(limit_reached(&self.quotas()?, provider, at))
    }
    /// Whether any session waits for a reset, so a new quota reading is worth a scheduling pass.
    pub(crate) fn any_waiting(&self) -> Result<bool> {
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM runtimes WHERE json_extract(data,'$.waiting')=1 OR json_extract(data,'$.limit_retry') IS NOT NULL)",
            [],
            |r| r.get(0),
        )?)
    }
    /// When a waiting session's provider is due to stop counting as spent, if known.
    pub(crate) fn wait_until(&self, session: &Session) -> Result<Option<i64>> {
        let runtime = self.session_runtime(&session.id)?;
        match runtime.profile.filter(|_| runtime.waiting) {
            Some(profile) => Ok(self.spent(profile.provider)?.and_then(|s| s.until)),
            None => Ok(None),
        }
    }
    pub(crate) fn migrate_opus_defaults(&mut self) -> Result<()> {
        if self.migrated("opus-defaults")? {
            return Ok(());
        }
        let sessions = self.sessions()?;
        let runtimes = self.runtimes()?;
        let tx = self.db.transaction()?;
        for policy in legacy::builtins()
            .into_iter()
            .filter(|p| p.default.provider == Provider::Claude)
        {
            let saved = legacy::load(&tx, policy.role)?;
            if saved != Some(legacy::codex_fixed(policy.role)) {
                continue;
            }
            // Freeze existing agents before changing defaults for future agents.
            for session in sessions
                .iter()
                .filter(|s| s.role == policy.role && s.provider == Provider::Codex)
            {
                let mut runtime = runtimes
                    .iter()
                    .find(|r| r.session_id == session.id)
                    .cloned()
                    .unwrap_or(SessionRuntime {
                        session_id: session.id.clone(),
                        ..Default::default()
                    });
                if runtime.profile.is_none() {
                    runtime.profile = Some(legacy::codex_fixed(policy.role).default);
                    tx.execute("INSERT INTO runtimes VALUES (?1,?2) ON CONFLICT(session_id) DO UPDATE SET data=excluded.data", params![session.id, encode(&runtime)?])?;
                }
            }
            tx.execute(
                "UPDATE policies SET data=?2 WHERE role=?1",
                params![tag(&policy.role)?, encode(&policy)?],
            )?;
        }
        tx.execute("INSERT INTO policy_migrations VALUES ('opus-defaults')", [])?;
        tx.commit()?;
        Ok(())
    }
    /// Turns tiered role policies into the machine's model selection. Unpinned sessions are
    /// pinned first, so no agent changes model. A fresh store gets no selection: the human
    /// saves the first one from the Models page. The legacy rows stay as they were; only this
    /// migration reads them.
    pub(crate) fn migrate_model_selection(&mut self) -> Result<()> {
        if self.migrated("model-selection")? {
            return Ok(());
        }
        let sessions = self.sessions()?;
        let runtimes = self.runtimes()?;
        let verifier_sizes = legacy::verifier_sizes(&self.home);
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let tx = self.db.transaction()?;
        let mut saved = Vec::new();
        for role in Role::ALL {
            if let Some(policy) = legacy::load(&tx, role)? {
                saved.push(policy);
            }
        }
        if !saved.is_empty() {
            for session in &sessions {
                let runtime = runtimes.iter().find(|r| r.session_id == session.id);
                if runtime.is_some_and(|r| r.profile.is_some()) {
                    continue;
                }
                let Some(profile) = saved
                    .iter()
                    .find(|p| p.role == session.role)
                    .and_then(|p| p.standard(session.provider))
                else {
                    continue;
                };
                let mut runtime = runtime.cloned().unwrap_or(SessionRuntime {
                    session_id: session.id.clone(),
                    ..Default::default()
                });
                runtime.profile = Some(profile);
                runtime.selection = Some(Selection {
                    chosen_by: Chooser::Default,
                    reason: "Kept the model it used before".into(),
                    revision: 1,
                    at: now(),
                });
                tx.execute("INSERT INTO runtimes VALUES (?1,?2) ON CONFLICT(session_id) DO UPDATE SET data=excluded.data", params![session.id, encode(&runtime)?])?;
            }
            let selection = legacy::convert(&saved, legacy::guide(&saved, &verifier_sizes, &date));
            tx.execute(
                "INSERT INTO model_selection VALUES (1,?1) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
                [encode(&selection)?],
            )?;
        }
        tx.execute(
            "INSERT INTO policy_migrations VALUES ('model-selection')",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Turns the providers, allowed models, providers per role and PR keeper model into the
    /// Who runs what table, seeded from the approved rows on the providers whose CLI `detect`
    /// finds signed in, or on the ones that were enabled when none is, with each model the
    /// closest one its CLI lists. A store with no saved setup is left alone and `detect` is not
    /// called. The guide loses the section the earlier migration wrote, which moves into a
    /// one-time note with everything else the table does not hold. Pinned agents keep their
    /// models.
    pub(crate) fn migrate_who_runs_what(
        &mut self,
        detect: &dyn Fn(Provider) -> ProviderReport,
    ) -> Result<()> {
        if self.migrated("who-runs-what")? {
            return Ok(());
        }
        let row: Option<Value> = self
            .db
            .query_row("SELECT data FROM model_selection", [], |r| decode(r, 0))
            .optional()?;
        if let Some(row) = row.filter(|row| row.get("jobs").is_none()) {
            let old: legacy::Selection = serde_json::from_value(row)?;
            let reports = PROVIDERS.map(|provider| (provider, detect(provider)));
            let signed_in: Vec<Provider> = (reports.iter())
                .filter(|(_, report)| report.status.usable())
                .map(|(provider, _)| *provider)
                .collect();
            let usable: Vec<Provider> = match signed_in.is_empty() {
                true => PROVIDERS
                    .into_iter()
                    .filter(|p| old.is_enabled(*p))
                    .collect(),
                false => signed_in,
            };
            let available: Vec<ModelOption> = (reports.into_iter())
                .flat_map(|(_, report)| report.models)
                .collect();
            let verification = self.shared().review();
            let lenses = legacy::verifier_providers(&self.home);
            let verifiers: Vec<(VerifierConfig, Option<Provider>)> = (verification.verifiers)
                .into_iter()
                .map(|verifier| {
                    let lens = lenses.get(&verifier.focus.trim().to_lowercase()).copied();
                    (verifier, lens)
                })
                .collect();
            let mut jobs = seed_jobs(&verifiers, old.keeper.is_some(), &usable, &available);
            let keeper = jobs.iter_mut().find(|row| row.job == Job::Keeper);
            if let (Some(model), Some(row)) = (&old.keeper, keeper) {
                let then = (row.main.choices()).find(|p| p.provider != model.provider);
                row.main.then = then.cloned().map_or(Then::Wait, Then::Model);
                // As every seeded model: the closest one its CLI lists; the note keeps the old.
                row.main.first = closest(model.provider, &model.model, &model.effort, &available);
            }
            let (guide, mut previous) = legacy::without_previous_settings(&old.guide);
            if let Some(shared) = self.shared().guide() {
                let (stripped, section) = legacy::without_previous_settings(&shared);
                if let Some(section) = section {
                    self.shared().set_guide(&stripped)?;
                    previous = Some(section);
                }
            }
            let section = previous.iter().flat_map(|section| {
                (section.lines())
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(|line| format!("From the guide: {line}"))
            });
            let selection = ModelSelection {
                revision: old.revision + 1,
                jobs,
                guide,
                notes: std::iter::once(MIGRATED_NOTE.to_owned())
                    .chain(old.replaced())
                    .chain(section)
                    .collect(),
                ..ModelSelection::default()
            };
            self.save_selection_row(&selection)?;
        }
        self.db
            .execute("INSERT INTO policy_migrations VALUES ('who-runs-what')", [])?;
        Ok(())
    }
    /// Replaces a model guide that is still an earlier starter text with the current one; a guide
    /// anyone edited, however slightly, is left as written.
    pub(crate) fn refresh_starter_guide(&mut self) -> Result<()> {
        let Some(guide) = self.shared().guide() else {
            return Ok(());
        };
        let unedited = |retired: &String| retired.trim() == guide.trim();
        if retired_starter_guides().iter().any(unedited) {
            self.shared().set_guide(STARTER_GUIDE)?;
        }
        Ok(())
    }
    fn migrated(&self, id: &str) -> Result<bool> {
        self.db
            .execute_batch("CREATE TABLE IF NOT EXISTS policy_migrations (id TEXT PRIMARY KEY)")?;
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM policy_migrations WHERE id=?1)",
            [id],
            |r| r.get::<_, bool>(0),
        )?)
    }
}

/// Role policies as they were saved before model selection. Read only by the migrations.
pub(crate) mod legacy {
    use super::*;
    use serde::Deserialize;

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub(crate) enum Mode {
        Fixed,
        Automatic,
    }
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub(crate) struct Profiles {
        pub provider: Provider,
        pub big: ModelProfile,
        pub small: ModelProfile,
    }
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub(crate) struct Policy {
        pub role: Role,
        pub mode: Mode,
        pub default: ModelProfile,
        pub allowed: Vec<ModelProfile>,
        pub small: ModelProfile,
        pub standard: ModelProfile,
        pub complex: ModelProfile,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub provider_profiles: Vec<Profiles>,
    }
    impl Policy {
        /// What the old router gave an unassigned session of this provider; `None` when it
        /// would have refused the provider.
        pub(crate) fn standard(&self, provider: Provider) -> Option<ModelProfile> {
            let profile = match self.mode {
                Mode::Fixed => self.default.clone(),
                Mode::Automatic if !self.provider_profiles.is_empty() => self
                    .provider_profiles
                    .iter()
                    .find(|p| p.provider == provider)?
                    .big
                    .clone(),
                Mode::Automatic => self.standard.clone(),
            };
            (profile.provider == provider).then_some(profile)
        }
    }
    /// A saved policy row, or `None` when there is none.
    pub(crate) fn load(db: &Connection, role: Role) -> Result<Option<Policy>> {
        let text: Option<String> = db
            .query_row(
                "SELECT data FROM policies WHERE role=?1",
                [tag(&role)?],
                |r| r.get(0),
            )
            .optional()?;
        Ok(text.and_then(|t| serde_json::from_str(&t).ok()))
    }
    fn profile(provider: Provider, model: &str, effort: &str) -> ModelProfile {
        ModelProfile {
            provider,
            model: model.into(),
            effort: effort.into(),
        }
    }
    /// The first built-in policy: fixed on Codex Sol for every role.
    pub(crate) fn codex_fixed(role: Role) -> Policy {
        let sol = profile(Provider::Codex, "gpt-6.1-sol", "medium");
        let small = profile(Provider::Codex, "gpt-6.1-sol", "low");
        let complex = profile(Provider::Codex, "gpt-6-astra", "high");
        Policy {
            role,
            mode: Mode::Fixed,
            default: sol.clone(),
            allowed: vec![small.clone(), sol.clone(), complex.clone()],
            small,
            standard: sol,
            complex,
            provider_profiles: Vec::new(),
        }
    }
    /// The last built-in policies: coordinators and implementers fixed on Opus.
    pub(crate) fn builtins() -> Vec<Policy> {
        Role::ALL
            .into_iter()
            .map(|role| {
                let mut policy = codex_fixed(role);
                if matches!(
                    role,
                    Role::ProjectOrchestrator | Role::TaskOrchestrator | Role::Implementer
                ) {
                    let effort = if role == Role::Implementer {
                        "medium"
                    } else {
                        "high"
                    };
                    policy.default = profile(Provider::Claude, "opus", effort);
                    policy.small = profile(Provider::Claude, "opus", "low");
                    policy.standard = profile(Provider::Claude, "opus", "medium");
                    policy.complex = profile(Provider::Claude, "opus", "high");
                    policy.allowed.extend([
                        policy.small.clone(),
                        policy.standard.clone(),
                        policy.complex.clone(),
                    ]);
                }
                policy
            })
            .collect()
    }
    /// The model selection as it was saved before the Who runs what table: providers with their
    /// allowed models, providers per role and the PR keeper's model.
    #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub(crate) struct Selection {
        pub revision: u64,
        pub providers: BTreeMap<Provider, Access>,
        pub role_providers: BTreeMap<Role, BTreeSet<Provider>>,
        pub guide: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub keeper: Option<ModelProfile>,
    }
    #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub(crate) struct Access {
        pub enabled: bool,
        pub models: Vec<Allowed>,
    }
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub(crate) struct Allowed {
        pub model: String,
        pub effort: String,
    }
    /// The roles whose providers the selection kept.
    const ROLES: [Role; 3] = [Role::ProjectOrchestrator, Role::Implementer, Role::Tester];
    impl Selection {
        pub(crate) fn is_enabled(&self, provider: Provider) -> bool {
            self.providers.get(&provider).is_some_and(|a| a.enabled)
        }
        /// Every provider's allowed models, each with its allowed efforts, in the order they
        /// were listed.
        fn allowed(&self) -> Vec<ModelOption> {
            let mut options: Vec<ModelOption> = vec![];
            for (provider, access) in &self.providers {
                for allowed in &access.models {
                    let known = (options.iter_mut())
                        .find(|o| o.provider == *provider && o.model == allowed.model);
                    match known {
                        Some(option) => option.efforts.push(allowed.effort.clone()),
                        None => options.push(ModelOption {
                            provider: *provider,
                            model: allowed.model.clone(),
                            label: allowed.model.clone(),
                            efforts: vec![allowed.effort.clone()],
                        }),
                    }
                }
            }
            options
        }
        /// What the table does not hold, one line each: the allowed models and the providers
        /// per role.
        pub(crate) fn replaced(&self) -> Vec<String> {
            let mut lines = vec![];
            for option in self.allowed() {
                let disabled = match self.is_enabled(option.provider) {
                    true => "",
                    false => ", then disabled",
                };
                lines.push(format!(
                    "Allowed before: {} {} at {}{disabled}.",
                    option.provider.label(),
                    option.model,
                    option.efforts.join(", ")
                ));
            }
            for role in ROLES {
                let providers: Vec<&str> = (self.role_providers.get(&role).into_iter())
                    .flatten()
                    .map(|p| p.label())
                    .collect();
                if !providers.is_empty() {
                    lines.push(format!(
                        "{} used {} before.",
                        role.label(),
                        or_list(&providers)
                    ));
                }
            }
            if let Some(keeper) = &self.keeper {
                lines.push(format!(
                    "The PR keeper used {} {} at {} before.",
                    keeper.provider.label(),
                    keeper.model,
                    keeper.effort
                ));
            }
            lines
        }
    }
    /// `guide` without the "## Previous settings (migrated …)" section the earlier migration
    /// wrote, and that section, if there was one.
    pub(crate) fn without_previous_settings(guide: &str) -> (String, Option<String>) {
        let Some(start) = guide.find("## Previous settings (migrated ") else {
            return (guide.to_owned(), None);
        };
        let after = &guide[start..];
        // The section ends at the next heading of its level or higher that it did not write.
        let end = after.find("\n## ").map_or(after.len(), |i| i + 1);
        let mut kept = guide[..start].trim_end().to_owned();
        let rest = after[end..].trim_start();
        if !rest.is_empty() {
            kept.push_str("\n\n");
            kept.push_str(rest);
        }
        if !kept.is_empty() {
            kept.push('\n');
        }
        (kept, Some(after[..end].to_owned()))
    }
    /// The provider each verifier in the shared review settings was limited to, by lowercase
    /// focus, as saved before the table, or the default reviewers' when none were saved.
    pub(crate) fn verifier_providers(home: &Path) -> BTreeMap<String, Provider> {
        let path = shared::folder(home).join(shared::REVIEW);
        // Never saved: the default reviewers, which were Claude's and Codex's.
        if !path.exists() {
            return BTreeMap::from([
                ("correctness".into(), Provider::Claude),
                ("regressions".into(), Provider::Codex),
            ]);
        }
        let review: Value = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        (review["verifiers"].as_array().into_iter().flatten())
            .filter_map(|v| {
                let focus = v["focus"].as_str()?.trim().to_lowercase();
                let provider = serde_json::from_value(v["provider"].clone()).ok()?;
                Some((focus, provider))
            })
            .collect()
    }
    /// Unions every role's allowed models per provider, defaults first; default providers become
    /// role providers. Reviewers take their providers from the verifiers instead.
    pub(crate) fn convert(policies: &[Policy], guide: String) -> Selection {
        let mut providers: BTreeMap<Provider, Access> = PROVIDERS
            .into_iter()
            .map(|p| (p, Access::default()))
            .collect();
        // Defaults lead each provider's list, so pre-fill lands on today's defaults.
        for profile in policies
            .iter()
            .map(|p| &p.default)
            .chain(policies.iter().flat_map(|p| &p.allowed))
        {
            let access = providers.entry(profile.provider).or_default();
            let model = Allowed {
                model: profile.model.clone(),
                effort: profile.effort.clone(),
            };
            if !access.models.contains(&model) {
                access.models.push(model);
            }
            access.enabled = true;
        }
        let role_providers = policies
            .iter()
            .filter(|p| ROLES.contains(&p.role))
            .map(|p| (p.role, BTreeSet::from([p.default.provider])))
            .collect();
        Selection {
            revision: 1,
            providers,
            role_providers,
            guide,
            keeper: None,
        }
    }
    /// The verifiers saved with a Big or Small `size`, which model selection replaces with the
    /// coordinator's choice: (verifier, its provider, the size).
    pub(crate) fn verifier_sizes(home: &Path) -> Vec<(VerifierConfig, Option<Provider>, String)> {
        #[derive(Deserialize)]
        struct Saved {
            #[serde(default)]
            verification: Option<Verification>,
        }
        #[derive(Deserialize)]
        struct Verification {
            verifiers: Vec<Sized>,
        }
        #[derive(Deserialize)]
        struct Sized {
            #[serde(flatten)]
            verifier: VerifierConfig,
            provider: Option<Provider>,
            size: Option<String>,
        }
        fs::read(home.join("settings.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
            .and_then(|saved| saved.verification)
            .map(|v| {
                v.verifiers
                    .into_iter()
                    .filter_map(|s| Some((s.verifier, s.provider, s.size?)))
                    .collect()
            })
            .unwrap_or_default()
    }
    /// The starter guide: the human's sizing intent, then every old setting as advice.
    pub(crate) fn guide(
        policies: &[Policy],
        verifier_sizes: &[(VerifierConfig, Option<Provider>, String)],
        date: &str,
    ) -> String {
        let mut out = format!("{STARTER_GUIDE}\n## Previous settings (migrated {date})\n");
        let same = |a: &Policy, b: &Policy| {
            Policy {
                role: b.role,
                ..a.clone()
            } == *b
        };
        let coordinators = policies
            .iter()
            .find(|p| p.role == Role::ProjectOrchestrator)
            .zip(policies.iter().find(|p| p.role == Role::TaskOrchestrator))
            .is_some_and(|(a, b)| same(a, b));
        for policy in policies {
            let name = match policy.role {
                Role::ProjectOrchestrator if coordinators => "Coordinators (main and repository)",
                Role::TaskOrchestrator if coordinators => continue,
                role => role.label(),
            };
            out.push_str(&format!("\n### {name}\n"));
            match policy.mode {
                Mode::Automatic => out.push_str(&format!(
                    "- Default: {} (coordinators could choose within its list).\n",
                    policy.default
                )),
                // Its other listed profiles are on the allowlist; repeating them could
                // outgrow the guide's limit.
                Mode::Fixed => out.push_str(&format!(
                    "- Was fixed to {}; coordinators could not choose another model.\n",
                    policy.default
                )),
            }
            out.push_str(&format!(
                "- Tiers: small work → {}; standard → {}; complex → {}.\n",
                policy.small, policy.standard, policy.complex
            ));
            for profiles in &policy.provider_profiles {
                out.push_str(&format!(
                    "- {}: large work → {} · {}; small work → {} · {}.\n",
                    profiles.provider.label(),
                    profiles.big.model,
                    profiles.big.effort,
                    profiles.small.model,
                    profiles.small.effort
                ));
            }
        }
        if !verifier_sizes.is_empty() {
            out.push_str("\n### Verifiers\n");
        }
        for (verifier, provider, size) in verifier_sizes {
            let role = policies.iter().find(|p| p.role == verifier.role);
            let provider = provider
                .or(role.map(|p| p.default.provider))
                .unwrap_or(Provider::Claude);
            let profile = role
                .and_then(|p| {
                    p.provider_profiles
                        .iter()
                        .find(|pp| pp.provider == provider)
                })
                .map(|pp| if size == "small" { &pp.small } else { &pp.big });
            let line = format!(
                "- {}: ran the {size} {} profile{}.\n",
                verifier.role.agent_label(Some(&verifier.focus)),
                provider.label(),
                profile.map_or(String::new(), |p| format!(", {} · {}", p.model, p.effort))
            );
            // The verifier list has no length limit; the guide does.
            if out.len() + line.len() > MAX_GUIDE_BYTES {
                break;
            }
            out.push_str(&line);
        }
        out
    }
}
