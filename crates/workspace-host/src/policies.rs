use crate::*;

const UNPINNED: &str = "This agent has no pinned model. Choose one from its model menu; Wiffletree never substitutes a model.";
/// Why nothing starts on a store whose human has not saved a setup yet.
pub(crate) const NO_SETUP: &str = "No model setup is saved on this machine yet. Open Models, review the suggested setup and press Save. Agents cannot start until a setup is saved.";
use std::collections::{BTreeMap, BTreeSet};

/// What the keeper's triggers need from a selection and review settings about to be saved.
pub(crate) fn check_keeper_needs(
    keeper: &KeeperSettings,
    selection: &ModelSelection,
    verification: &VerificationSettings,
) -> Result<()> {
    if keeper.comments {
        check_sorting_model(selection.keeper.as_ref())?;
    }
    check_tries(keeper, selection.keeper.as_ref(), verification)
}

impl Host {
    /// Opens a store and, when it has no saved setup, saves the one suggested with both
    /// providers signed in. Tests, benchmarks and scripted stores start from it; the app only
    /// ever saves the setup the human pressed Save on.
    pub fn open_set_up(home: impl AsRef<Path>) -> Result<Self> {
        let mut host = Self::open(home)?;
        if !host.model_selection()?.is_saved() {
            let signed_in = ProviderStatus {
                installed: true,
                signed_in: true,
                plan: None,
            };
            let detection = PROVIDERS.map(|p| (p, signed_in.clone())).into();
            host.set_model_selection(&suggest(&detection).selection)?;
        }
        Ok(host)
    }
    /// This machine's selection with the shared guide.
    pub fn model_selection(&self) -> Result<ModelSelection> {
        Ok(ModelSelection {
            guide: self.shared().guide().unwrap_or_default(),
            ..self.saved_selection()?.unwrap_or_default()
        })
    }
    /// The database row, whose guide is the one saved before the shared folder.
    pub(crate) fn saved_selection(&self) -> Result<Option<ModelSelection>> {
        Ok(self
            .db
            .query_row("SELECT data FROM model_selection", [], |r| decode(r, 0))
            .optional()?)
    }
    /// Saves the whole configuration at the next revision, the guide in the shared folder.
    /// Existing agents keep their models.
    pub fn set_model_selection(&mut self, selection: &ModelSelection) -> Result<ModelSelection> {
        selection.validate()?;
        let settings = self.settings();
        check_keeper_needs(&settings.keeper, selection, &settings.verification)?;
        let row = self.saved_selection()?.unwrap_or_default();
        let saved = ModelSelection {
            revision: row.revision + 1,
            ..selection.clone()
        };
        let unchanged_guide = ModelSelection {
            guide: row.guide,
            ..saved.clone()
        };
        let shared = self.shared();
        shared.put_back_on_error(shared::GUIDE, || {
            shared.set_guide(&saved.guide)?;
            self.db.execute(
                "INSERT INTO model_selection VALUES (1,?1) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
                [encode(&unchanged_guide)?],
            )?;
            Ok(saved)
        })
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
        self.save_runtime(&runtime)?;
        Ok(runtime)
    }
    /// Pins a session the human created: "Role default" when it matches the picker's pre-fill.
    pub(crate) fn pin_human_choice(
        &mut self,
        session: &Session,
        profile: ModelProfile,
    ) -> Result<()> {
        let (chosen_by, reason) =
            if self.model_selection()?.prefill(session.role).as_ref() == Some(&profile) {
                (Chooser::Default, "Role default")
            } else {
                (Chooser::Human, "Chosen by you")
            };
        self.pin(&session.id, profile, chosen_by, reason)?;
        Ok(())
    }
    /// The model a session's next turn uses: its pinned profile. Only an unpinned main
    /// coordinator falls back to its provider's first allowed model; any other agent was given
    /// a model when it was created, so a missing one is never replaced.
    pub fn turn_profile(&self, session: &Session) -> Result<ModelProfile> {
        if let Some(profile) = self.session_runtime(&session.id)?.profile {
            return Ok(profile);
        }
        ensure!(session.parent_id.is_none(), "{UNPINNED}");
        let selection = self.model_selection()?;
        let model = selection
            .allowed(session.provider)
            .first()
            .with_context(|| {
                format!(
                    "Choose a model for this agent; {} is not enabled on this machine",
                    session.provider.label()
                )
            })?;
        Ok(ModelProfile {
            provider: session.provider,
            model: model.model.clone(),
            effort: model.effort.clone(),
        })
    }
    /// Why a session may not run here: it has no pinned model, or its coordinator chose a
    /// provider that has been disabled since.
    pub fn hold_reason(&self, session: &Session) -> Result<Option<String>> {
        let runtime = self.session_runtime(&session.id)?;
        let Some(profile) = runtime.profile else {
            return Ok(session.parent_id.is_some().then(|| UNPINNED.to_owned()));
        };
        let Some(selection) = runtime.selection else {
            return Ok(None);
        };
        if !matches!(selection.chosen_by, Chooser::Coordinator { .. })
            || self.model_selection()?.is_enabled(profile.provider)
        {
            return Ok(None);
        }
        Ok(Some(format!(
            "{} is disabled on this machine. Enable it in Models, or choose another model for this agent.",
            profile.provider.label()
        )))
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
            selection.validate()?;
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
    pub enum Mode {
        Fixed,
        Automatic,
    }
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Profiles {
        pub provider: Provider,
        pub big: ModelProfile,
        pub small: ModelProfile,
    }
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Policy {
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
        pub fn standard(&self, provider: Provider) -> Option<ModelProfile> {
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
    pub fn load(db: &Connection, role: Role) -> Result<Option<Policy>> {
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
    pub fn codex_fixed(role: Role) -> Policy {
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
    pub fn builtins() -> Vec<Policy> {
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
    /// Unions every role's allowed models per provider, defaults first; default providers become
    /// role providers. Reviewers take their providers from the verifiers instead.
    pub fn convert(policies: &[Policy], guide: String) -> ModelSelection {
        let mut providers: BTreeMap<Provider, ProviderAccess> = PROVIDERS
            .into_iter()
            .map(|p| (p, ProviderAccess::default()))
            .collect();
        // Defaults lead each provider's list, so pre-fill lands on today's defaults.
        for profile in policies
            .iter()
            .map(|p| &p.default)
            .chain(policies.iter().flat_map(|p| &p.allowed))
        {
            let access = providers.entry(profile.provider).or_default();
            let model = AllowedModel {
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
            .filter(|p| PROVIDER_ROLES.contains(&p.role))
            .map(|p| (p.role, BTreeSet::from([p.default.provider])))
            .collect();
        ModelSelection {
            revision: 1,
            providers,
            role_providers,
            guide,
            keeper: None,
        }
    }
    /// The verifiers saved with a Big or Small `size`, which model selection replaces with the
    /// coordinator's choice: (verifier, its provider, the size).
    pub fn verifier_sizes(home: &Path) -> Vec<(VerifierConfig, String)> {
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
            size: Option<String>,
        }
        fs::read(home.join("settings.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
            .and_then(|saved| saved.verification)
            .map(|v| {
                v.verifiers
                    .into_iter()
                    .filter_map(|s| Some((s.verifier, s.size?)))
                    .collect()
            })
            .unwrap_or_default()
    }
    /// The starter guide: the human's sizing intent, then every old setting as advice.
    pub fn guide(
        policies: &[Policy],
        verifier_sizes: &[(VerifierConfig, String)],
        date: &str,
    ) -> String {
        let mut out = format!(
            "{}\n## Previous settings (migrated {date})\n",
            starter_guide(&PROVIDERS)
        );
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
        for (verifier, size) in verifier_sizes {
            let role = policies.iter().find(|p| p.role == verifier.role);
            let provider = verifier
                .provider
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
