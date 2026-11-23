//! UI evaluator runs (OpenSpec change `ui-evaluators`). A coordinator starts one with a run
//! folder and an exact model; the human decides per project whether evaluators may run. Each
//! admitted run gets a fresh session of its own, outside every agent tree, so nothing it says
//! reaches the coordinator but the message that it ended. Full-desktop runs take turns at the
//! screen through one queue for every project.
use crate::*;

/// Where an evaluator's Codex keeps its home, under the data folder.
const CODEX_HOME: &str = "evaluators/codex-home";
const PROMPT_FILE: &str = "prompt.md";
/// Where each app run's test copy writes its output, under the data folder, for the human.
const TARGET_LOGS: &str = "evaluators/logs";
/// Why a run waits before the human has chosen for its project.
const ASKED: &str = "Waiting for the human to allow UI evaluators in this project.";
/// Why a full-desktop run at the front of the queue waits: nothing touches the screen before.
const AWAITING_START: &str = "Waiting for the human to press Start.";
/// The Start card's answers.
const START_NOW: &str = "Start now";
const LATER: &str = "Later";
const SKIP: &str = "Skip these runs";
pub(crate) const SKIPPED: &str = "Skipped by you.";
/// Why a full-desktop run that reached its provider's limit ended: going on after the reset
/// would take the screen without a fresh Start.
pub(crate) const DESKTOP_AT_LIMIT: &str = "Stopped at the provider's usage limit. A full-desktop run never goes on without your Start; the coordinator can start it again.";
/// The most apps one full-desktop run may name.
const MOST_APPS: usize = 10;
/// Why a run the human stopped ended.
pub(crate) const STOPPED_BY_YOU: &str = "Stopped by you.";
pub(crate) const STOPPED_BY_COORDINATOR: &str = "Stopped by its coordinator.";
/// Why a run whose turn the host's crash, kill or quit cut short ended.
const INTERRUPTED: &str = "Wiffletree stopped during the run.";
pub(crate) const PROJECT_CLOSED: &str = "Stopped: the project was closed.";

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS evaluations (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, session_id TEXT, created_at INTEGER NOT NULL, data TEXT NOT NULL);
         CREATE INDEX IF NOT EXISTS evaluations_by_project ON evaluations(project_id, created_at);
         CREATE TABLE IF NOT EXISTS evaluator_permissions (project_id TEXT PRIMARY KEY, permission TEXT NOT NULL);",
    )?;
    Ok(())
}

/// "41 min", as the end-of-run message says how long a run took.
fn duration(run: &Evaluation) -> String {
    let from = run.started_at.unwrap_or(run.created_at);
    let minutes = (run.ended_at.unwrap_or(from) - from) / 60_000;
    match minutes {
        0 => "under 1 min".into(),
        minutes => format!("{minutes} min"),
    }
}

/// The message that wakes a run's coordinator: its id, state, folder and duration, and nothing
/// the run said.
pub(crate) fn ended_message(run: &Evaluation) -> String {
    let model = run.reported_model.as_ref().unwrap_or(&run.profile.model);
    format!(
        "Run {} {} · model {model} · folder {} · {}",
        run.id,
        run.state.name(),
        run.folder,
        duration(run)
    )
}

/// What the coordinator sees of a run: never its transcript or a failure's reason.
fn summary(run: &Evaluation) -> Value {
    let waiting = (run.state == EvaluationState::Waiting).then(|| match run.session_id {
        None if run.holds_screen => AWAITING_START.to_owned(),
        None => ASKED.to_owned(),
        Some(_) => match run.resets_at {
            Some(at) => format!(
                "Its pinned model is at its limit until {}.",
                policies::reset_time(at)
            ),
            None => "Its pinned model is at its limit until it resets.".into(),
        },
    });
    json!({
        "run_id": run.id,
        "state": run.state.name(),
        "mode": run.mode,
        "provider": run.profile.provider,
        "model": run.profile.model,
        "reported_model": run.reported_model,
        "effort": run.profile.effort,
        "folder": run.folder,
        "apps": run.apps,
        "created_at": run.created_at,
        "started_at": run.started_at,
        "ended_at": run.ended_at,
        "waiting": waiting,
    })
}

impl Host {
    /// Records what a provider's CLI said about itself: its sign-in, and the models it lists,
    /// which `start_evaluation` checks a run's model against.
    pub(crate) fn record_report(&mut self, provider: Provider, report: &ProviderReport) {
        self.record_detection(provider, &report.status);
        if !report.models.is_empty() {
            self.listed_models.insert(provider, report.models.clone());
        }
    }

    /// What the human allows in `project`; `None` until they have chosen.
    pub(crate) fn evaluator_permission(
        &self,
        project: &str,
    ) -> Result<Option<EvaluatorPermission>> {
        let permission: Option<String> = self
            .db
            .query_row(
                "SELECT permission FROM evaluator_permissions WHERE project_id=?1",
                [project],
                |r| r.get(0),
            )
            .optional()?;
        Ok(match permission {
            Some(permission) => Some(serde_json::from_value(json!(permission))?),
            None => None,
        })
    }

    /// The human's choice for `project`: runs held for it start or stop, and so does its Inbox
    /// card. A narrower choice stops the runs it no longer allows.
    pub fn set_evaluator_permission(
        &mut self,
        project: &str,
        permission: EvaluatorPermission,
    ) -> Result<()> {
        self.project(project)?;
        self.atomically(|host| {
            host.db.execute(
                "INSERT INTO evaluator_permissions VALUES (?1,?2) ON CONFLICT(project_id) DO UPDATE SET permission=excluded.permission",
                params![project, serde_json::to_value(permission)?.as_str()],
            )?;
            for run in host.evaluations(project)? {
                if run.state.ended() {
                    continue;
                }
                // A run in its turn is left to the service, which stops the turn.
                if !permission.allows(run.mode) {
                    let reason = not_allowed(run.mode);
                    host.stop_evaluation_with(run, &reason)?;
                } else if run.session_id.is_none() && run.state == EvaluationState::Waiting {
                    let mut run = run;
                    run.state = EvaluationState::Queued;
                    run.reason = None;
                    host.save_evaluation(&run)?;
                }
            }
            for item in host.open_attention(project)? {
                if item.operation_id.starts_with(&card_prefix(project)) {
                    host.resolve_attention(&item.id, permission_label(permission))?;
                }
            }
            Ok(())
        })?;
        self.admit_evaluations()
    }

    /// Answers the Inbox card with one of its choices, as `set_evaluator_permission` does, and
    /// tells the coordinator that asked. Any other answer is refused, so the card stays.
    pub(crate) fn answer_evaluator_card(&mut self, item: &Attention, answer: &str) -> Result<()> {
        if let Some(run) = item.operation_id.strip_prefix(START_CARD) {
            return self.answer_start_card(item, run, answer);
        }
        let permission = EvaluatorPermission::from_answer(answer).with_context(|| {
            format!(
                "Answer with one of: {}",
                (EvaluatorPermission::CHOICES.iter())
                    .map(|(label, _)| *label)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        self.set_evaluator_permission(&item.project_id, permission)?;
        let body = match permission {
            EvaluatorPermission::Off => {
                "The human chose not to allow UI evaluators in this project. Your waiting runs were stopped. Do not start more.".to_owned()
            }
            allowed => format!(
                "The human allowed UI evaluators in this project: {}. Your waiting runs that this allows have started; Wiffletree messages you when each ends.",
                permission_label(allowed)
            ),
        };
        let id = format!("{EVALUATION_PREFIX}answer:{}", item.id);
        if let Err(error) = self.send(&id, None, &item.session_id, &body) {
            eprintln!("Telling the coordinator about UI evaluators: {error:#}");
        }
        Ok(())
    }

    /// The Start card of the full-desktop run `id`, which holds the screen. Start now begins
    /// it, unless its provider is at its limit, when the card stays; Later leaves the card and
    /// the run first in line; Skip stops it and the project's desktop runs queued after it.
    fn answer_start_card(&mut self, item: &Attention, id: &str, answer: &str) -> Result<()> {
        let run = self.evaluation(id)?;
        if run.state.ended() || run.session_id.is_some() {
            self.resolve_attention(&item.id, answer)?;
            return Ok(());
        }
        match answer {
            START_NOW => {
                if let Some(spent) = self.limit_reached(run.profile.provider, now())? {
                    let until = match spent.until {
                        Some(at) => format!(" until {}", policies::reset_time(at)),
                        None => String::new(),
                    };
                    bail!(
                        "{} is at its usage limit{until}. Press Start now once it resets.",
                        run.profile.provider.label()
                    );
                }
                self.resolve_attention(&item.id, answer)?;
                if let Err(error) = self.begin_evaluation(run) {
                    let run = self.evaluation(id)?;
                    let reason = format!("Could not start: {error:#}");
                    self.end_evaluation(run, EvaluationState::Failed, &reason)?;
                    self.admit_evaluations()?;
                }
            }
            LATER => {}
            SKIP => {
                self.resolve_attention(&item.id, answer)?;
                let queued = (self.evaluations(&run.project_id)?.into_iter()).filter(|other| {
                    other.mode == EvaluationMode::Desktop && other.state == EvaluationState::Queued
                });
                for skipped in std::iter::once(run.clone()).chain(queued.collect::<Vec<_>>()) {
                    self.end_evaluation(skipped, EvaluationState::Stopped, SKIPPED)?;
                }
                self.admit_evaluations()?;
            }
            _ => bail!("Answer with one of: {START_NOW}, {LATER}, {SKIP}"),
        }
        Ok(())
    }

    /// Closing the project, by archiving its coordinator, turns evaluators off and stops the
    /// runs that have not ended; the service stops those in a turn (`evaluations_in_turn`).
    pub(crate) fn close_evaluations(&mut self, project: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO evaluator_permissions VALUES (?1,'off') ON CONFLICT(project_id) DO UPDATE SET permission='off'",
            [project],
        )?;
        for run in self.evaluations(project)? {
            self.stop_evaluation_with(run, PROJECT_CLOSED)?;
        }
        // A desktop run of another project may take the screen this project held.
        self.admit_evaluations()
    }
    /// The sessions in a turn of `project`'s runs that what it allows now no longer allows,
    /// each with its run's mode, for the service to stop.
    pub(crate) fn disallowed_in_turn(
        &self,
        project: &str,
    ) -> Result<Vec<(String, EvaluationMode)>> {
        let permission = (self.evaluator_permission(project)?).unwrap_or(EvaluatorPermission::Off);
        Ok((self.evaluations(project)?.into_iter())
            .filter(|run| !run.state.ended() && !permission.allows(run.mode))
            .filter_map(|run| Some((run.session_id?, run.mode)))
            .filter(|(session, _)| self.in_turn.contains(session))
            .collect())
    }

    pub(crate) fn evaluation(&self, id: &str) -> Result<Evaluation> {
        self.db
            .query_row("SELECT data FROM evaluations WHERE id=?1", [id], |r| {
                decode(r, 0)
            })
            .optional()?
            .with_context(|| format!("No UI evaluator run {id}"))
    }
    fn session_evaluation(&self, session: &str) -> Result<Evaluation> {
        self.db
            .query_row(
                "SELECT data FROM evaluations WHERE session_id=?1",
                [session],
                |r| decode(r, 0),
            )
            .optional()?
            .context("This session is no UI evaluator run")
    }
    fn save_evaluation(&self, run: &Evaluation) -> Result<()> {
        self.db.execute(
            "INSERT INTO evaluations VALUES (?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET session_id=excluded.session_id, data=excluded.data",
            params![
                run.id,
                run.project_id,
                run.session_id,
                run.created_at,
                encode(run)?
            ],
        )?;
        Ok(())
    }
    /// `project`'s runs, newest first.
    pub fn evaluations(&self, project: &str) -> Result<Vec<Evaluation>> {
        Ok(self
            .db
            .prepare("SELECT data FROM evaluations WHERE project_id=?1 ORDER BY created_at DESC, rowid DESC")?
            .query_map([project], |r| decode(r, 0))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// `start_evaluation`: checks everything a run needs, then creates it. A refusal creates
    /// nothing and says why.
    pub(crate) fn start_evaluation(&mut self, caller: &Session, args: &Value) -> Result<Value> {
        ensure!(
            caller.role == Role::ProjectOrchestrator,
            "Only the project coordinator starts UI evaluators"
        );
        service::only_arguments(
            "start_evaluation",
            args,
            &[
                "run_folder",
                "provider",
                "model",
                "effort",
                "mode",
                "target",
                "apps",
            ],
        )?;
        let string = |key: &str| -> Result<&str> {
            let value = args[key].as_str().map(str::trim).unwrap_or_default();
            ensure!(!value.is_empty(), "Give {key}");
            Ok(value)
        };
        let mode: EvaluationMode = serde_json::from_value(json!(string("mode")?))
            .context("Give mode as app or desktop")?;
        let provider: Provider = serde_json::from_value(json!(string("provider")?))
            .context("Give provider as claude or codex")?;
        let profile = ModelProfile {
            provider,
            model: string("model")?.into(),
            effort: string("effort")?.into(),
        };
        let permission = self.evaluator_permission(&caller.project_id)?;
        match permission {
            Some(EvaluatorPermission::Off) => {
                bail!("UI evaluators are not allowed in this project. Ask the human.")
            }
            Some(permission) if !permission.allows(mode) => {
                bail!(
                    "{} is not allowed in this project. Ask the human.",
                    mode.label()
                )
            }
            _ => {}
        }
        let folder = self.run_folder(string("run_folder")?)?;
        let (target, apps) = match mode {
            EvaluationMode::App => {
                ensure!(
                    args["apps"].is_null(),
                    "Leave apps out of an app run: it drives only its target"
                );
                (Some(self.app_target(&args["target"])?), vec![])
            }
            EvaluationMode::Desktop => {
                ensure!(
                    args["target"].is_null(),
                    "Leave target out of a desktop run: it uses the apps you name in apps"
                );
                (None, desktop_apps(&args["apps"])?)
            }
        };
        self.check_model(&profile)?;
        let at = now();
        let run = Evaluation {
            id: format!("run-{}", &new_id()[..8]),
            project_id: caller.project_id.clone(),
            coordinator_id: caller.id.clone(),
            session_id: None,
            profile,
            reported_model: None,
            mode,
            folder: folder.to_string_lossy().into_owned(),
            target,
            apps,
            holds_screen: false,
            state: match permission {
                Some(_) => EvaluationState::Queued,
                None => EvaluationState::Waiting,
            },
            reason: permission.is_none().then(|| ASKED.to_owned()),
            resets_at: None,
            created_at: at,
            started_at: None,
            ended_at: None,
        };
        ensure!(
            self.evaluation(&run.id).is_err(),
            "Wiffletree picked a run id that is taken. Call start_evaluation again"
        );
        self.atomically(|host| {
            host.save_evaluation(&run)?;
            if permission.is_none() {
                host.ask_for_evaluators(caller, &run)?;
            }
            Ok(())
        })?;
        self.admit_evaluations()?;
        let run = self.evaluation(&run.id)?;
        let next = match run.state {
            EvaluationState::Waiting if run.session_id.is_none() && !run.holds_screen => {
                "The human decides whether this project allows UI evaluators. Finish your turn: Wiffletree messages you their answer, and when the run ends."
            }
            _ if run.mode == EvaluationMode::Desktop => {
                "Full-desktop runs take turns at the screen, and each starts only when the human presses Start. Finish your turn: Wiffletree messages you when the run ends, also if the human skips it. Read its report from the folder then."
            }
            _ => {
                "Finish your turn or carry on: Wiffletree messages you when the run ends. Read its report from the folder then."
            }
        };
        let mut result = summary(&run);
        result["next"] = json!(next);
        Ok(result)
    }

    /// The Inbox card, one open at a time per project: later runs wait for the same answer.
    /// After the human dismissed one unanswered, the next run asks again.
    fn ask_for_evaluators(&mut self, caller: &Session, run: &Evaluation) -> Result<()> {
        let prefix = card_prefix(&caller.project_id);
        let open = self.open_attention(&caller.project_id)?;
        if open
            .iter()
            .any(|item| item.operation_id.starts_with(&prefix))
        {
            return Ok(());
        }
        let prompt = format!(
            "Allow UI evaluators in this project?\nThe coordinator wants to start an evaluator on {} · {}. It gets only its prompt and its own folder, {}.\nApp only drives just the app under test, a separate copy on a test profile; you can keep working. Full desktop can see your whole screen and click any app you approve. Not in this project tells the coordinator evaluators are off.\nOne choice per project, kept until you change it. Closing the project turns it off. A coordinator can never change it.",
            run.profile.provider.label(),
            run.profile.model,
            run.folder
        );
        let options: Vec<String> = (EvaluatorPermission::CHOICES.iter())
            .map(|(label, _)| (*label).to_owned())
            .collect();
        self.raise_attention(
            &caller.id,
            "local",
            &format!("{prefix}{}", run.id),
            &prompt,
            &options,
            None,
        )?;
        Ok(())
    }

    /// The run folder, canonical: an existing folder with a `prompt.md`, in no repository,
    /// worktree or Wiffletree data folder, and used by no other run that has not ended.
    fn run_folder(&self, given: &str) -> Result<PathBuf> {
        ensure!(
            Path::new(given).is_absolute(),
            "Give run_folder as an absolute path"
        );
        let folder = fs::canonicalize(given)
            .ok()
            .filter(|folder| folder.is_dir())
            .with_context(|| format!("The run folder {given} does not exist"))?;
        ensure!(
            folder.join(PROMPT_FILE).is_file(),
            "The run folder {given} has no {PROMPT_FILE}"
        );
        ensure!(
            !folder.starts_with(&self.home),
            "The run folder {given} is inside Wiffletree's data folder"
        );
        if let Some(repository) = folder.ancestors().find(|a| a.join(".git").exists()) {
            bail!(
                "The run folder {given} is inside the Git repository or worktree {}",
                repository.display()
            );
        }
        for repository in self.repositories()? {
            let path = fs::canonicalize(&repository.path).unwrap_or(repository.path.into());
            ensure!(
                !folder.starts_with(&path),
                "The run folder {given} is inside the repository {}",
                path.display()
            );
        }
        let folder_text = folder.to_string_lossy();
        let in_use: Option<String> = self
            .db
            .query_row(
                "SELECT id FROM evaluations WHERE json_extract(data,'$.folder')=?1 AND json_extract(data,'$.state') NOT IN ('finished','failed','stopped')",
                [folder_text.as_ref()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(run) = in_use {
            bail!("The run {run} still uses the folder {given}. Give each run its own folder");
        }
        Ok(folder)
    }

    /// An app run's test copy, as it will start: an absolute executable on a test data folder
    /// of its own, which no other run that has not ended uses, and given no Wiffletree session
    /// variable or credential.
    fn app_target(&self, given: &Value) -> Result<EvaluationTarget> {
        ensure!(
            given.is_object(),
            "Give target: the app under test's executable, its arguments with --data-dir <test data folder>, and the environment it needs"
        );
        service::only_arguments("target", given, &["executable", "arguments", "environment"])?;
        let mut target: EvaluationTarget = serde_json::from_value(given.clone()).context(
            "Give target as {executable, arguments: [strings], environment: {NAME: value}}",
        )?;
        let executable = Path::new(&target.executable);
        ensure!(
            executable.is_absolute() && executable.is_file(),
            "The app under test {} is not an absolute path to an executable",
            target.executable
        );
        ensure!(
            !target
                .arguments
                .iter()
                .any(|arg| arg == "--automation-socket"),
            "Leave --automation-socket out of the target's arguments: Wiffletree gives the app a socket of the run's own"
        );
        if let Some(name) = (target.environment.keys()).find(|name| session_variable(name)) {
            bail!(
                "The app under test may not be given {name}: no Wiffletree session variable or token reaches it"
            );
        }
        // The app reads the last --data-dir it is given; the checks read the only one.
        ensure!(
            !(target.arguments.iter()).any(|arg| arg.starts_with("--data-dir=")),
            "Give --data-dir and its folder as two arguments"
        );
        ensure!(
            (target.arguments.iter())
                .filter(|arg| *arg == "--data-dir")
                .count()
                <= 1,
            "Give the app under test one --data-dir"
        );
        let given_data = target
            .data_folder()
            .filter(|folder| Path::new(folder).is_absolute())
            .context("Give the app under test a test data folder of its own with --data-dir <absolute path>")?;
        // Canonical, so links and `..` cannot hide an overlap or a shared folder.
        let data = fs::canonicalize(given_data)
            .ok()
            .filter(|data| data.is_dir())
            .with_context(|| {
                format!("The test data folder {given_data} does not exist. Create it before starting the run")
            })?;
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        for own in [
            self.home.clone(),
            workspace_core::data_dir::default_directory(&home),
        ] {
            let own = fs::canonicalize(&own).unwrap_or(own);
            ensure!(
                !data.starts_with(&own) && !own.starts_with(&data),
                "The test data folder {given_data} overlaps Wiffletree's own data folder {}",
                own.display()
            );
        }
        let data_text = data.to_string_lossy().into_owned();
        for run in self.unended_evaluations()? {
            if run.target.as_ref().and_then(EvaluationTarget::data_folder) == Some(&data_text) {
                bail!(
                    "The run {} still drives a copy on the data folder {given_data}. Give each run its own data folder",
                    run.id
                );
            }
        }
        let at = target.arguments.iter().position(|arg| arg == "--data-dir");
        target.arguments[at.expect("data_folder found it") + 1] = data_text;
        Ok(target)
    }
    fn unended_evaluations(&self) -> Result<Vec<Evaluation>> {
        Ok(self
            .db
            .prepare("SELECT data FROM evaluations WHERE json_extract(data,'$.state') NOT IN ('finished','failed','stopped')")?
            .query_map([], |r| decode(r, 0))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Refuses a provider not signed in here, a model its CLI did not list, full IDs and aliases
    /// alike, and an effort the model does not offer. Until the CLI has listed its models
    /// nothing is checked against a guess: the run is refused, and the service reads the list.
    fn check_model(&self, profile: &ModelProfile) -> Result<()> {
        let provider = profile.provider;
        ensure!(
            self.detected.get(&provider) == Some(&true),
            "{} is not signed in on this Mac, or Wiffletree has not checked it yet. Ask the human.",
            provider.label()
        );
        let Some(options) = self.listed_models.get(&provider) else {
            bail!(
                "Wiffletree has not read {}'s model list on this Mac yet, so it cannot check {}. It is reading it now: call start_evaluation again in a minute.",
                provider.label(),
                profile.model
            );
        };
        let Some(option) = options.iter().find(|option| option.model == profile.model) else {
            bail!(
                "{} does not offer the model {}. It offers: {}",
                provider.label(),
                profile.model,
                (options.iter().map(|o| o.model.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        };
        let efforts: Vec<&str> = match option.efforts.is_empty() {
            true => usual_efforts(provider).to_vec(),
            false => option.efforts.iter().map(String::as_str).collect(),
        };
        ensure!(
            efforts.contains(&profile.effort.as_str()),
            "{} does not offer the effort {}. It offers: {}",
            profile.model,
            profile.effort,
            efforts.join(", ")
        );
        Ok(())
    }

    /// `list_evaluations`: the caller's project's runs, without anything they said.
    pub(crate) fn list_evaluations(&self, caller: &Session, args: &Value) -> Result<Value> {
        ensure!(
            caller.role == Role::ProjectOrchestrator,
            "Only the project coordinator lists UI evaluators"
        );
        service::only_arguments("list_evaluations", args, &[])?;
        let runs: Vec<Value> = self
            .evaluations(&caller.project_id)?
            .iter()
            .map(summary)
            .collect();
        Ok(json!({
            "permission": self.evaluator_permission(&caller.project_id)?,
            "runs": runs,
        }))
    }

    /// `stop_evaluation` for its coordinator's project. Returns the session to stop when the
    /// run is in a turn; its turn's end then ends the run.
    pub(crate) fn coordinator_stops_evaluation(
        &mut self,
        caller: &Session,
        args: &Value,
    ) -> Result<(Value, Option<String>)> {
        ensure!(
            caller.role == Role::ProjectOrchestrator,
            "Only the project coordinator stops UI evaluators"
        );
        service::only_arguments("stop_evaluation", args, &["run_id"])?;
        let id = args["run_id"].as_str().context("Missing run_id")?;
        let run = self.evaluation(id)?;
        ensure!(
            run.project_id == caller.project_id,
            "{id} is not a run of this project"
        );
        ensure!(
            !run.state.ended(),
            "The run {id} has already {}",
            run.state.name()
        );
        let in_turn = self.stop_evaluation_with(run, STOPPED_BY_COORDINATOR)?;
        self.admit_evaluations()?;
        let result = summary(&self.evaluation(id)?);
        Ok((result, in_turn))
    }

    /// The human's Stop. Returns the session to stop when the run is in a turn.
    pub(crate) fn stop_evaluation(&mut self, id: &str) -> Result<Option<String>> {
        let run = self.evaluation(id)?;
        ensure!(
            !run.state.ended(),
            "The run {id} has already {}",
            run.state.name()
        );
        let in_turn = self.stop_evaluation_with(run, STOPPED_BY_YOU)?;
        // A run stopped while it waited for Start frees the screen for the next.
        self.admit_evaluations()?;
        Ok(in_turn)
    }

    /// Ends a run that has not ended as stopped, or returns its session when it is in a turn,
    /// whose end then ends it.
    fn stop_evaluation_with(&mut self, run: Evaluation, reason: &str) -> Result<Option<String>> {
        if run.state.ended() {
            return Ok(None);
        }
        if let Some(session) = run.session_id.clone().filter(|s| self.in_turn.contains(s)) {
            return Ok(Some(session));
        }
        self.end_evaluation(run, EvaluationState::Stopped, reason)?;
        Ok(None)
    }

    /// Starts every queued run its project allows: each app run at once. The oldest desktop
    /// run takes the screen whenever no run holds it, and waits there for the human's Start.
    pub(crate) fn admit_evaluations(&mut self) -> Result<()> {
        let queued: Vec<Evaluation> = self
            .db
            .prepare("SELECT data FROM evaluations WHERE json_extract(data,'$.state')='queued' ORDER BY created_at, rowid")?
            .query_map([], |r| decode(r, 0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut screen_taken = self.screen_holder()?.is_some();
        for mut run in queued {
            if run.mode == EvaluationMode::Desktop {
                if screen_taken {
                    continue;
                }
                screen_taken = true;
                run.holds_screen = true;
                run.state = EvaluationState::Waiting;
                run.reason = Some(AWAITING_START.into());
                self.atomically(|host| {
                    host.save_evaluation(&run)?;
                    host.ask_to_start(&run)
                })?;
                continue;
            }
            let id = run.id.clone();
            // One run's failure to start must never stop the others.
            if let Err(error) = self.begin_evaluation(run) {
                let run = self.evaluation(&id)?;
                self.end_evaluation(
                    run,
                    EvaluationState::Failed,
                    &format!("Could not start: {error:#}"),
                )?;
            }
        }
        Ok(())
    }

    /// The desktop run that holds the screen, from the front of the queue until it ends,
    /// whether or not the human has pressed Start. One at a time.
    pub(crate) fn screen_holder(&self) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT id FROM evaluations WHERE json_extract(data,'$.holds_screen')=1 AND json_extract(data,'$.state') NOT IN ('finished','failed','stopped')",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// The Inbox card a desktop run at the front of the queue waits on. Nothing touches the
    /// screen without its Start now.
    fn ask_to_start(&mut self, run: &Evaluation) -> Result<()> {
        let apps: Vec<String> = (run.apps.iter())
            .map(|app| match sensitive(app) {
                Some(why) => format!("{app} ({why})"),
                None => app.clone(),
            })
            .collect();
        let prompt = format!(
            "Ready to use your screen\nUI evaluator run {} on {} · {} is next for the screen. It may see and use only {}; every other app stays out of its screenshots and its reach.\nDon't use this Mac until it finishes. Press Esc anywhere, or Stop in the menu bar, to stop it.\nNothing touches the screen until you press {START_NOW}. {LATER} keeps it first in line. {SKIP} stops it and this project's full-desktop runs queued after it, and tells the coordinator they didn't run.",
            run.id,
            run.profile.provider.label(),
            run.profile.model,
            apps.join(", ")
        );
        let options = [START_NOW, LATER, SKIP].map(String::from);
        self.raise_attention(
            &run.coordinator_id,
            "local",
            &format!("{START_CARD}{}", run.id),
            &prompt,
            &options,
            None,
        )?;
        Ok(())
    }

    /// Gives an admitted run its own session, pinned to its model, with `prompt.md` as its
    /// only message. A provider at its limit holds the turn until the reset.
    fn begin_evaluation(&mut self, mut run: Evaluation) -> Result<()> {
        let prompt = fs::read_to_string(Path::new(&run.folder).join(PROMPT_FILE))
            .context("Could not read prompt.md")?;
        ensure!(!prompt.trim().is_empty(), "prompt.md is empty");
        let session = Session {
            id: new_id(),
            project_id: String::new(),
            parent_id: None,
            repository_id: None,
            name: format!("UI evaluator · {}", run.id),
            role: Role::Evaluator,
            provider: run.profile.provider,
            status: Status::Ready,
            archived: false,
            owner: Some(SessionOwner::Project(run.project_id.clone())),
        };
        let spent = self.limit_reached(run.profile.provider, now())?;
        let waits = spent.is_some();
        run.session_id = Some(session.id.clone());
        run.state = match waits {
            true => EvaluationState::Waiting,
            false => EvaluationState::Running,
        };
        run.resets_at = spent.and_then(|spent| spent.until);
        run.reason = None;
        self.atomically(|host| {
            host.db.execute(
                "INSERT INTO sessions VALUES (?1,NULL,NULL,?2,?3)",
                params![session.id, tag(&session.role)?, encode(&session)?],
            )?;
            let mut runtime = host.pin(
                &session.id,
                run.profile.clone(),
                Chooser::Coordinator {
                    session_id: run.coordinator_id.clone(),
                },
                "Pinned by start_evaluation; never replaced",
            )?;
            runtime.waiting = waits;
            runtime.workdir = Some(run.folder.clone());
            host.save_runtime(&runtime)?;
            host.save_evaluation(&run)?;
            host.send(
                &format!("{EVALUATION_PREFIX}{}:prompt", run.id),
                None,
                &session.id,
                &prompt,
            )?;
            Ok(())
        })
    }

    /// The launch of a run's turn: its folder, and the Codex home that keeps the human's own out.
    pub(crate) fn evaluator_launch(&self, session: &Session) -> Result<provider::EvaluatorLaunch> {
        let run = self.session_evaluation(&session.id)?;
        ensure!(!run.state.ended(), "The run {} has ended", run.id);
        let codex_home = self.home.join(CODEX_HOME);
        if session.provider == Provider::Codex {
            provider::prepare_codex_home(&codex_home)?;
        }
        let app = run.target.map(|target| provider::AppLaunch {
            target,
            socket: std::env::temp_dir().join(format!("wiffletree-{}.sock", run.id)),
            log: self.home.join(TARGET_LOGS).join(format!("{}.log", run.id)),
        });
        let desktop = (run.mode == EvaluationMode::Desktop).then(|| provider::DesktopLaunch {
            helper: provider::helper_app(&self.home),
            socket: std::env::temp_dir().join(format!("wiffletree-{}-desktop.sock", run.id)),
            run: run.id.clone(),
            apps: run.apps.clone(),
        });
        Ok(provider::EvaluatorLaunch {
            folder: fs::canonicalize(&run.folder)?,
            codex_home,
            app,
            desktop,
        })
    }

    /// A run's turn has started: it runs, whether for the first time or after its limit reset.
    pub(crate) fn evaluation_turn_started(&mut self, session: &str) -> Result<()> {
        let mut run = self.session_evaluation(session)?;
        run.state = EvaluationState::Running;
        run.started_at.get_or_insert(now());
        run.resets_at = None;
        self.save_evaluation(&run)
    }

    /// Records the model a run's provider said it runs, which a later turn may say again.
    pub(crate) fn evaluation_model_reported(&mut self, session: &str, model: &str) -> Result<()> {
        let mut run = self.session_evaluation(session)?;
        run.reported_model = Some(model.chars().take(200).collect());
        self.save_evaluation(&run)
    }

    /// A run's turn has ended: at a usage limit it waits for the reset in the same session;
    /// otherwise the run ends as stopped, failed or finished.
    pub(crate) fn evaluation_turn_ended(
        &mut self,
        session: &str,
        stopped: Option<&str>,
        error: Option<&str>,
        waits_until: Option<Option<i64>>,
    ) -> Result<()> {
        let mut run = self.session_evaluation(session)?;
        if run.state.ended() {
            return Ok(());
        }
        if let (Some(resets_at), None) = (waits_until, stopped) {
            if run.mode == EvaluationMode::Desktop {
                self.end_evaluation(run, EvaluationState::Failed, DESKTOP_AT_LIMIT)?;
                return self.admit_evaluations();
            }
            run.state = EvaluationState::Waiting;
            run.resets_at = resets_at;
            return self.save_evaluation(&run);
        }
        let (state, reason) = match (stopped, error) {
            (Some(reason), _) => (EvaluationState::Stopped, reason),
            (None, Some(error)) => (EvaluationState::Failed, error),
            (None, None) => (EvaluationState::Finished, ""),
        };
        self.end_evaluation(run, state, reason)?;
        self.admit_evaluations()
    }

    /// At startup, ends as failed each run whose turn the host's stop cut short, and wakes its
    /// coordinator. A run whose prompt still waits to be delivered, or that waits for its
    /// limit's reset, goes on; one with no session yet waits for the human or the screen.
    pub(crate) fn end_interrupted_evaluations(&mut self) -> Result<()> {
        let unended: Vec<Evaluation> = self
            .db
            .prepare("SELECT data FROM evaluations WHERE session_id IS NOT NULL AND json_extract(data,'$.state') NOT IN ('finished','failed','stopped') ORDER BY created_at")?
            .query_map([], |r| decode(r, 0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for run in unended {
            let session = run.session_id.clone().unwrap_or_default();
            let queued: i64 = self.db.query_row(
                "SELECT count(*) FROM messages WHERE recipient=?1 AND receipt='queued'",
                [&session],
                |r| r.get(0),
            )?;
            if queued > 0 || self.session_runtime(&session)?.limit_retry.is_some() {
                continue;
            }
            self.end_evaluation(run, EvaluationState::Failed, INTERRUPTED)?;
        }
        self.admit_evaluations()
    }

    /// A run's session that could not start a turn: the run fails.
    pub(crate) fn evaluation_start_failed(&mut self, session: &str, error: &str) -> Result<()> {
        let run = self.session_evaluation(session)?;
        self.end_evaluation(
            run,
            EvaluationState::Failed,
            &format!("Could not start: {error}"),
        )?;
        self.admit_evaluations()
    }

    /// Ends a run: its session is archived, so nothing starts another turn in it, its queued
    /// input is settled, and its coordinator is told only the run's id, state, folder and
    /// duration.
    fn end_evaluation(
        &mut self,
        mut run: Evaluation,
        state: EvaluationState,
        reason: &str,
    ) -> Result<()> {
        run.state = state;
        run.reason = (!reason.is_empty()).then(|| reason.to_owned());
        run.ended_at = Some(now());
        self.atomically(|host| {
            host.save_evaluation(&run)?;
            // A desktop run that ends before its Start closes its card.
            let card: Option<String> = host
                .db
                .query_row(
                    "SELECT id FROM attention WHERE operation_id=?1 AND json_extract(data,'$.answer') IS NULL",
                    [format!("{START_CARD}{}", run.id)],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(card) = card {
                host.resolve_attention(&card, "The run ended")?;
            }
            if let Some(id) = &run.session_id {
                host.db.execute(
                    "UPDATE messages SET receipt='completed' WHERE recipient=?1 AND receipt IN ('queued','held')",
                    [id],
                )?;
                let mut session = host.session(id)?;
                session.archived = true;
                session.status = Status::Done;
                host.db.execute(
                    "UPDATE sessions SET data=?2 WHERE id=?1",
                    params![id, encode(&session)?],
                )?;
                let mut runtime = host.session_runtime(id)?;
                runtime.waiting = false;
                runtime.limit_retry = None;
                host.save_runtime(&runtime)?;
            }
            Ok(())
        })?;
        let id = format!("{EVALUATION_PREFIX}{}:ended", run.id);
        if let Err(error) = self.send(&id, None, &run.coordinator_id, &ended_message(&run)) {
            eprintln!(
                "Telling the coordinator that run {} ended: {error:#}",
                run.id
            );
        }
        Ok(())
    }
}

/// A variable that could carry a Wiffletree session or a credential into the test copy. Only
/// the provider stubs are passed among Wiffletree's own.
fn session_variable(name: &str) -> bool {
    name.ends_with("_TOKEN")
        || (name.starts_with("WORKSPACE_")
            && !["WORKSPACE_CLAUDE_BIN", "WORKSPACE_CODEX_BIN"].contains(&name))
}
/// Why a run ended when what its project allows no longer allows it.
pub(crate) fn not_allowed(mode: EvaluationMode) -> String {
    format!("Stopped: {} is not allowed in this project.", mode.label())
}
/// The start of the operation ids of `project`'s Inbox cards; each ends with the run that asked.
fn card_prefix(project: &str) -> String {
    format!("{EVALUATION_PREFIX}allow:{project}:")
}
/// The start of a Start card's operation id, which ends with its run.
const START_CARD: &str = "evaluation:start:";
/// Whether an Inbox item is a Start card, which only its answers close.
pub(crate) fn is_start_card(item: &Attention) -> bool {
    item.operation_id.starts_with(START_CARD)
}

/// A desktop run's apps: one to `MOST_APPS` bundle ids, never Wiffletree Evaluator itself.
/// Nothing is allowed that the coordinator does not name.
fn desktop_apps(given: &Value) -> Result<Vec<String>> {
    let apps: Vec<String> = serde_json::from_value(given.clone()).unwrap_or_default();
    ensure!(
        !apps.is_empty(),
        "Name the apps a desktop run may use in apps, by bundle id, such as [\"com.apple.calculator\"]. It sees and touches no other app."
    );
    ensure!(
        apps.len() <= MOST_APPS,
        "Name at most {MOST_APPS} apps for one run"
    );
    let mut named: Vec<String> = vec![];
    for app in apps {
        let app = app.trim().to_owned();
        ensure!(
            is_bundle_id(&app),
            "{app} is not a bundle id, such as com.apple.calculator"
        );
        ensure!(
            !app.eq_ignore_ascii_case(HELPER_BUNDLE_ID),
            "A run may not use Wiffletree Evaluator itself"
        );
        if !named.iter().any(|other| other.eq_ignore_ascii_case(&app)) {
            named.push(app);
        }
    }
    Ok(named)
}
/// The helper's bundle id, which the helper also refuses.
const HELPER_BUNDLE_ID: &str = "com.vyttle.wiffletree.evaluator";
/// A reverse-DNS bundle id: at least two parts of letters, digits, `-` and `_`, no wildcard.
fn is_bundle_id(text: &str) -> bool {
    text.len() <= 155
        && text.split('.').count() >= 2
        && text.split('.').all(|part| {
            !part.is_empty()
                && (part.chars()).all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}
/// Why the Start card calls out an app the coordinator named: these reach far beyond their own
/// windows.
fn sensitive(app: &str) -> Option<&'static str> {
    let app = app.to_ascii_lowercase();
    let terminals = [
        "com.apple.terminal",
        "com.googlecode.iterm2",
        "com.mitchellh.ghostty",
        "dev.warp.warp-stable",
        "net.kovidgoyal.kitty",
        "org.alacritty",
        "com.github.wez.wezterm",
    ];
    if app.starts_with("com.vyttle.wiffletree") {
        Some("Wiffletree itself: it can drive your agents")
    } else if terminals.contains(&app.as_str()) {
        Some("a terminal: it can run any command")
    } else if app == "com.apple.systempreferences" {
        Some("System Settings")
    } else if app == "com.apple.finder" {
        Some("Finder: it can open and move your files")
    } else {
        None
    }
}
fn permission_label(permission: EvaluatorPermission) -> &'static str {
    (EvaluatorPermission::CHOICES.iter())
        .find(|(_, choice)| *choice == permission)
        .map_or("", |(label, _)| label)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const PROMPT: &str = "Open Models in dark mode at 1280 wide.\nWrite report.md.";

    struct Fixture {
        _home: tempfile::TempDir,
        runs: tempfile::TempDir,
        host: Host,
        coordinator: Session,
    }
    /// Models as Claude's CLI lists them: an alias and the full ID it stands for.
    pub(crate) fn claude_listed() -> Vec<ModelOption> {
        ["opus", "claude-opus-5-5"]
            .map(|model| ModelOption {
                provider: Provider::Claude,
                model: model.into(),
                label: model.into(),
                efforts: vec!["low".into(), "high".into()],
            })
            .into()
    }
    fn fixture() -> Fixture {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.listed_models.insert(Provider::Claude, claude_listed());
        let project = host.create_project("UX loop").unwrap();
        let id = host.coordinator_of(&project.id).unwrap().unwrap();
        let coordinator = host.session(&id).unwrap();
        Fixture {
            _home: home,
            runs: tempfile::tempdir().unwrap(),
            host,
            coordinator,
        }
    }
    impl Fixture {
        /// A run folder holding `PROMPT`.
        fn folder(&self, name: &str) -> String {
            let folder = self.runs.path().join(name);
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join(PROMPT_FILE), PROMPT).unwrap();
            folder.to_string_lossy().into_owned()
        }
        fn start(&mut self, folder: &str, mode: &str) -> Result<Value> {
            let data = format!("{folder}-data");
            self.start_with(folder, mode, self.target(&data))
        }
        /// An app under test on the test data folder `data`.
        /// Creates `data`, as a coordinator does before it starts a run.
        fn target(&self, data: &str) -> Value {
            fs::create_dir_all(data).unwrap();
            json!({"executable":"/usr/bin/true","arguments":["--data-dir",data],"environment":{"WORKSPACE_CLAUDE_BIN":"/stubs/claude"}})
        }
        fn start_with(&mut self, folder: &str, mode: &str, target: Value) -> Result<Value> {
            let args = json!({"run_folder":folder,"provider":"claude","model":"opus","effort":"high","mode":mode,"target":target});
            let coordinator = self.coordinator.clone();
            self.host.start_evaluation(&coordinator, &args)
        }
        fn allow(&mut self, permission: EvaluatorPermission) {
            let project = self.coordinator.project_id.clone();
            self.host
                .set_evaluator_permission(&project, permission)
                .unwrap();
        }
        fn messages_to_coordinator(&self) -> Vec<Message> {
            (self.host.messages(&self.coordinator.id, None, 100).unwrap())
                .into_iter()
                .filter(|m| m.id.starts_with(EVALUATION_PREFIX))
                .collect()
        }
        /// A desktop run on Calculator, in a folder of its own.
        fn start_desktop(&mut self, name: &str) -> String {
            let folder = self.folder(name);
            let args = json!({"run_folder":folder,"provider":"claude","model":"opus","effort":"high","mode":"desktop","apps":["com.apple.calculator"]});
            let coordinator = self.coordinator.clone();
            let started = self.host.start_evaluation(&coordinator, &args).unwrap();
            started["run_id"].as_str().unwrap().to_owned()
        }
        /// The open Start card of `run`.
        fn start_card(&self, run: &str) -> Option<Attention> {
            (self
                .host
                .open_attention(&self.coordinator.project_id)
                .unwrap())
            .into_iter()
            .find(|item| item.operation_id == format!("{START_CARD}{run}"))
        }
        fn answer_start(&mut self, run: &str, answer: &str) -> Result<Attention> {
            let card = self.start_card(run).expect("the run's Start card is open");
            self.host.answer_attention(&card.id, answer)
        }
        fn state(&self, run: &str) -> EvaluationState {
            self.host.evaluation(run).unwrap().state
        }
    }

    #[test]
    fn an_app_run_needs_a_test_copy_on_a_data_folder_of_its_own_with_no_session_variable() {
        let mut f = fixture();
        f.allow(EvaluatorPermission::App);
        let folder = f.folder("run-a");
        let data = f.runs.path().join("profile").to_string_lossy().into_owned();
        let refused = |f: &mut Fixture, target: Value| {
            let error = f.start_with(&folder, "app", target).unwrap_err();
            assert!(
                f.host
                    .evaluations(&f.coordinator.project_id)
                    .unwrap()
                    .is_empty()
            );
            error.to_string()
        };
        assert!(refused(&mut f, Value::Null).contains("Give target"));
        let mut relative = f.target(&data);
        relative["executable"] = json!("workspace-desktop");
        assert!(refused(&mut f, relative).contains("absolute path to an executable"));
        let mut no_data = f.target(&data);
        no_data["arguments"] = json!(["--demo-repository", "/tmp/demo"]);
        assert!(refused(&mut f, no_data).contains("--data-dir"));
        let own = f.host.home.to_string_lossy().into_owned();
        let mut twice = f.target(&data);
        twice["arguments"] = json!(["--data-dir", data, "--data-dir", own]);
        assert!(refused(&mut f, twice).contains("one --data-dir"));
        let mut joined = f.target(&data);
        joined["arguments"] = json!([format!("--data-dir={data}"), "--data-dir", own]);
        assert!(refused(&mut f, joined).contains("two arguments"));
        let mut missing = f.target(&data);
        let absent = f.runs.path().join("not-created/profile");
        missing["arguments"] = json!(["--data-dir", absent]);
        assert!(refused(&mut f, missing).contains("does not exist"));
        let mut socket = f.target(&data);
        socket["arguments"] = json!(["--data-dir", data, "--automation-socket", "/tmp/s"]);
        assert!(refused(&mut f, socket).contains("--automation-socket"));
        for variable in [
            "WORKSPACE_TOKEN",
            "WORKSPACE_SOCKET",
            "GH_TOKEN",
            "GITHUB_TOKEN",
        ] {
            let mut secret = f.target(&data);
            secret["environment"][variable] = json!("x");
            assert!(refused(&mut f, secret).contains(variable), "{variable}");
        }
        let own = f
            .host
            .home
            .join("test-profile")
            .to_string_lossy()
            .into_owned();
        let own = f.target(&own);
        assert!(refused(&mut f, own).contains("Wiffletree's own data folder"));

        let started = f.start_with(&folder, "app", f.target(&data)).unwrap();
        let run = f
            .host
            .evaluation(started["run_id"].as_str().unwrap())
            .unwrap();
        let target = run.target.unwrap();
        assert_eq!(target.environment["WORKSPACE_CLAUDE_BIN"], "/stubs/claude");
        // Stored canonical, as the copy will use it.
        let canonical = fs::canonicalize(&data)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(target.data_folder(), Some(canonical.as_str()));
    }

    #[test]
    fn a_second_live_run_on_the_same_data_folder_is_refused() {
        let mut f = fixture();
        f.allow(EvaluatorPermission::App);
        let data = f.runs.path().join("profile").to_string_lossy().into_owned();
        let first = f.folder("run-a");
        let id = f.start_with(&first, "app", f.target(&data)).unwrap()["run_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let second = f.folder("run-b");
        let refused = f.start_with(&second, "app", f.target(&data)).unwrap_err();
        assert!(refused.to_string().contains(&id), "{refused}");
        // The same folder through another path is the same folder.
        let other_path = format!(
            "{}/../{}/profile",
            f.runs.path().display(),
            f.runs.path().file_name().unwrap().to_string_lossy()
        );
        assert!(f.start_with(&second, "app", f.target(&other_path)).is_err());
        // And through a link.
        let link = f.runs.path().join("profile-link");
        std::os::unix::fs::symlink(&data, &link).unwrap();
        let linked = f.target(&link.to_string_lossy());
        assert!(f.start_with(&second, "app", linked).is_err());
        let session = f.host.evaluation(&id).unwrap().session_id.unwrap();
        f.host
            .evaluation_turn_ended(&session, None, None, None)
            .unwrap();
        f.start_with(&second, "app", f.target(&data)).unwrap();
    }

    #[test]
    fn the_first_run_asks_the_human_and_their_answer_starts_it_with_only_its_prompt() {
        let mut f = fixture();
        let folder = f.folder("run-a");
        let started = f.start(&folder, "app").unwrap();
        assert_eq!(started["state"], "waiting");
        let run = f
            .host
            .evaluation(started["run_id"].as_str().unwrap())
            .unwrap();
        assert_eq!(run.session_id, None);
        let card = f.host.open_attention(&f.coordinator.project_id).unwrap();
        assert_eq!(card.len(), 1);
        assert_eq!(
            card[0].options,
            ["App only", "Full desktop", "Not in this project"]
        );
        // A second run waits for the same card.
        let second = f.folder("run-b");
        f.start(&second, "app").unwrap();
        assert_eq!(
            f.host
                .open_attention(&f.coordinator.project_id)
                .unwrap()
                .len(),
            1
        );
        // Free text is refused and the card stays.
        assert!(f.host.answer_attention(&card[0].id, "sure").is_err());
        f.host.answer_attention(&card[0].id, "App only").unwrap();
        assert!(
            f.host
                .open_attention(&f.coordinator.project_id)
                .unwrap()
                .is_empty()
        );
        let run = f.host.evaluation(&run.id).unwrap();
        assert_eq!(run.state, EvaluationState::Running);
        let session = f.host.session(run.session_id.as_deref().unwrap()).unwrap();
        assert_eq!(session.role, Role::Evaluator);
        assert_eq!(session.parent_id, None);
        assert_eq!(session.project_id, "");
        // Its one message is prompt.md as it is, and its model is pinned as given.
        let input = f.host.messages(&session.id, None, 10).unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0].body, PROMPT);
        let runtime = f.host.session_runtime(&session.id).unwrap();
        assert_eq!(runtime.profile, Some(run.profile.clone()));
        assert_eq!(runtime.workdir.as_deref(), Some(run.folder.as_str()));
        assert_eq!(f.host.turn_profile(&session).unwrap().model, "opus");
        let told = f.messages_to_coordinator();
        assert!(told[0].body.contains("App only"), "{}", told[0].body);
    }

    #[test]
    fn refusals_name_the_reason_and_create_nothing() {
        let mut f = fixture();
        let folder = f.folder("run-a");
        let refused = |f: &mut Fixture, args: Value| {
            let coordinator = f.coordinator.clone();
            format!(
                "{:#}",
                f.host.start_evaluation(&coordinator, &args).unwrap_err()
            )
        };
        let target = f.target(&format!("{folder}-data"));
        let args = |folder: &str, model: &str, effort: &str, mode: &str| json!({"run_folder":folder,"provider":"claude","model":model,"effort":effort,"mode":mode,"target":target});
        // A desktop run names its apps and no target, and raises no card when refused.
        let desktop = refused(&mut f, args(&folder, "opus", "high", "desktop"));
        assert!(desktop.contains("Leave target out"), "{desktop}");
        let unknown = refused(&mut f, args(&folder, "gpt-9", "high", "app"));
        assert!(
            unknown.contains("does not offer the model gpt-9"),
            "{unknown}"
        );
        let effort = refused(&mut f, args(&folder, "opus", "ultra", "app"));
        assert!(
            effort.contains("does not offer the effort ultra"),
            "{effort}"
        );
        // A list not read yet checks nothing against a guess.
        let mut codex = args(&folder, "gpt-6.1-sol", "high", "app");
        codex["provider"] = json!("codex");
        let unread = refused(&mut f, codex.clone());
        assert!(
            unread.contains("has not read Codex's model list"),
            "{unread}"
        );
        let signed_out = ProviderStatus::default();
        f.host.record_detection(Provider::Codex, &signed_out);
        let codex = refused(&mut f, codex);
        assert!(codex.contains("Codex is not signed in"), "{codex}");
        // Bad folders.
        let relative = refused(&mut f, args("runs/a", "opus", "high", "app"));
        assert!(relative.contains("absolute"), "{relative}");
        let missing = f.runs.path().join("missing");
        let missing = refused(
            &mut f,
            args(&missing.to_string_lossy(), "opus", "high", "app"),
        );
        assert!(missing.contains("does not exist"), "{missing}");
        let empty = f.runs.path().join("empty");
        fs::create_dir(&empty).unwrap();
        let empty = refused(
            &mut f,
            args(&empty.to_string_lossy(), "opus", "high", "app"),
        );
        assert!(empty.contains("no prompt.md"), "{empty}");
        let repository = f.folder("repo");
        crate::test_support::git(Path::new(&repository), &["init", "-q"]);
        let inside = Path::new(&repository).join("eval");
        fs::create_dir(&inside).unwrap();
        fs::write(inside.join(PROMPT_FILE), PROMPT).unwrap();
        let inside = refused(
            &mut f,
            args(&inside.to_string_lossy(), "opus", "high", "app"),
        );
        assert!(inside.contains("Git repository or worktree"), "{inside}");
        let data = f.host.home.join("eval");
        fs::create_dir(&data).unwrap();
        fs::write(data.join(PROMPT_FILE), PROMPT).unwrap();
        let data = refused(&mut f, args(&data.to_string_lossy(), "opus", "high", "app"));
        assert!(data.contains("data folder"), "{data}");
        // A worker may not start one.
        let worker = Session {
            role: Role::Implementer,
            ..f.coordinator.clone()
        };
        let good = args(&folder, "opus", "high", "app");
        assert!(f.host.start_evaluation(&worker, &good).is_err());
        assert!(
            f.host
                .evaluations(&f.coordinator.project_id)
                .unwrap()
                .is_empty()
        );
        assert!(
            f.host
                .open_attention(&f.coordinator.project_id)
                .unwrap()
                .is_empty()
        );
        // Off, and a mode the project does not allow.
        f.allow(EvaluatorPermission::Off);
        let off = refused(&mut f, good.clone());
        assert_eq!(
            off,
            "UI evaluators are not allowed in this project. Ask the human."
        );
        f.allow(EvaluatorPermission::App);
        let narrow = refused(&mut f, args(&folder, "opus", "high", "desktop"));
        assert_eq!(
            narrow,
            "Full desktop is not allowed in this project. Ask the human."
        );
        // One folder, one unfinished run.
        f.start(&folder, "app").unwrap();
        let reused = refused(&mut f, good);
        assert!(reused.contains("still uses the folder"), "{reused}");
    }

    #[test]
    fn at_a_usage_limit_the_run_waits_on_its_own_model_and_never_falls_back() {
        let mut f = fixture();
        f.allow(EvaluatorPermission::App);
        let at = now();
        let resets = at + 3_600_000;
        f.host
            .record_quota(QuotaReading {
                provider: Provider::Claude,
                account: None,
                observed_at: at,
                source: "Claude stream".into(),
                windows: vec![QuotaWindow {
                    key: "five_hour".into(),
                    label: "Session".into(),
                    used_percent: Some(100.),
                    observed_at: at,
                    duration_mins: Some(300),
                    resets_at: Some(resets),
                    status: None,
                }],
                error: None,
            })
            .unwrap();
        let folder = f.folder("run-a");
        let started = f.start(&folder, "app").unwrap();
        assert_eq!(started["state"], "waiting");
        assert!(
            started["waiting"]
                .as_str()
                .unwrap()
                .contains("at its limit")
        );
        let run = f
            .host
            .evaluation(started["run_id"].as_str().unwrap())
            .unwrap();
        assert_eq!(run.resets_at, Some(resets));
        let session = f.host.session(run.session_id.as_deref().unwrap()).unwrap();
        assert!(f.host.hold_reason(&session).unwrap().is_some());
        let runtime = f.host.session_runtime(&session.id).unwrap();
        assert_eq!(runtime.profile, Some(run.profile));
        // A turn that failed at the limit waits again in the same session.
        f.host
            .evaluation_turn_ended(&session.id, None, Some("limit"), Some(Some(resets)))
            .unwrap();
        let run = f.host.evaluation(&run.id).unwrap();
        assert_eq!(run.state, EvaluationState::Waiting);
        assert!(f.messages_to_coordinator().is_empty());
    }

    #[test]
    fn a_full_id_is_started_as_given_and_an_alias_records_the_model_it_stood_for() {
        let mut f = fixture();
        f.allow(EvaluatorPermission::App);
        let folder = f.folder("run-a");
        let target = f.target(&format!("{folder}-data"));
        let args = json!({"run_folder":folder,"provider":"claude","model":"claude-opus-5-5","effort":"high","mode":"app","target":target});
        let coordinator = f.coordinator.clone();
        let started = f.host.start_evaluation(&coordinator, &args).unwrap();
        assert_eq!(started["state"], "running");
        let run = f
            .host
            .evaluation(started["run_id"].as_str().unwrap())
            .unwrap();
        let session = f.host.session(run.session_id.as_deref().unwrap()).unwrap();
        assert_eq!(
            f.host.turn_profile(&session).unwrap().model,
            "claude-opus-5-5"
        );
        // An alias's run records the model Claude said it stood for, which the coordinator
        // may see, and its end names it.
        let other = f.folder("run-b");
        let id = f.start(&other, "app").unwrap()["run_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let session = f.host.evaluation(&id).unwrap().session_id.unwrap();
        f.host
            .evaluation_model_reported(&session, "claude-opus-5-5")
            .unwrap();
        let listed = f.host.list_evaluations(&coordinator, &json!({})).unwrap();
        assert_eq!(listed["runs"][0]["model"], "opus");
        assert_eq!(listed["runs"][0]["reported_model"], "claude-opus-5-5");
        f.host
            .evaluation_turn_ended(&session, None, None, None)
            .unwrap();
        let told = f.messages_to_coordinator();
        assert!(
            told[0].body.contains("· model claude-opus-5-5 ·"),
            "{}",
            told[0].body
        );
    }

    #[test]
    fn the_end_wakes_the_coordinator_with_only_the_id_state_folder_and_time() {
        let mut f = fixture();
        f.allow(EvaluatorPermission::App);
        let folder = f.folder("run-a");
        let id = f.start(&folder, "app").unwrap()["run_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let session = f.host.evaluation(&id).unwrap().session_id.unwrap();
        f.host.evaluation_turn_started(&session).unwrap();
        f.host
            .append_output(
                &f.host.session(&session).unwrap(),
                "out:1",
                "SECRET FINDINGS",
            )
            .unwrap();
        f.host
            .evaluation_turn_ended(&session, None, None, None)
            .unwrap();
        let run = f.host.evaluation(&id).unwrap();
        assert_eq!(run.state, EvaluationState::Finished);
        let told = f.messages_to_coordinator();
        assert_eq!(told.len(), 1);
        assert_eq!(
            told[0].body,
            format!(
                "Run {id} finished · model opus · folder {} · under 1 min",
                run.folder
            )
        );
        assert!(f.host.session(&session).unwrap().archived);
        // A failure's reason reaches neither the message nor the coordinator's list.
        let other = f.folder("run-b");
        let failed = f.start(&other, "app").unwrap()["run_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let session = f.host.evaluation(&failed).unwrap().session_id.unwrap();
        f.host
            .evaluation_turn_ended(&session, None, Some("SECRET model text"), None)
            .unwrap();
        let coordinator = f.coordinator.clone();
        let listed = f.host.list_evaluations(&coordinator, &json!({})).unwrap();
        assert!(!listed.to_string().contains("SECRET"), "{listed}");
        assert_eq!(listed["runs"][0]["state"], "failed");
        assert!(
            !f.messages_to_coordinator()
                .iter()
                .any(|m| m.body.contains("SECRET"))
        );
    }

    #[test]
    fn desktop_runs_take_turns_at_the_screen_while_app_runs_go_side_by_side() {
        let mut f = fixture();
        f.allow(EvaluatorPermission::Desktop);
        let first = f.start_desktop("d1");
        let second = f.start_desktop("d2");
        f.answer_start(&first, START_NOW).unwrap();
        let run = f.host.evaluation(&first).unwrap();
        assert_eq!(run.state, EvaluationState::Running, "{:?}", run.reason);
        assert_eq!(f.host.screen_holder().unwrap(), Some(first.clone()));
        assert_eq!(
            f.host.evaluation(&second).unwrap().state,
            EvaluationState::Queued
        );
        // App runs do not wait for the screen.
        let a1 = f.folder("a1");
        let a2 = f.folder("a2");
        assert_eq!(f.start(&a1, "app").unwrap()["state"], "running");
        assert_eq!(f.start(&a2, "app").unwrap()["state"], "running");
        let session = f.host.evaluation(&first).unwrap().session_id.unwrap();
        f.host
            .evaluation_turn_ended(&session, None, None, None)
            .unwrap();
        assert_eq!(f.host.screen_holder().unwrap(), Some(second.clone()));
        // Narrowing to app only stops the desktop run and frees the screen; app runs go on.
        f.allow(EvaluatorPermission::App);
        let stopped = f.host.evaluation(&second).unwrap();
        assert_eq!(stopped.state, EvaluationState::Stopped);
        assert_eq!(
            stopped.reason.as_deref(),
            Some(not_allowed(EvaluationMode::Desktop).as_str())
        );
        assert_eq!(f.host.screen_holder().unwrap(), None);
        assert!(f.start_card(&second).is_none(), "its Start card closed");
        let project = f.coordinator.project_id.clone();
        let running = (f.host.evaluations(&project).unwrap().into_iter())
            .filter(|run| run.state == EvaluationState::Running)
            .count();
        assert_eq!(running, 2);
    }

    #[test]
    fn a_desktop_run_names_the_apps_it_may_use_and_no_other() {
        let mut f = fixture();
        f.allow(EvaluatorPermission::Desktop);
        let folder = f.folder("d1");
        let mut refused = |apps: Value| {
            let args = json!({"run_folder":folder,"provider":"claude","model":"opus","effort":"high","mode":"desktop","apps":apps});
            let coordinator = f.coordinator.clone();
            let error = f.host.start_evaluation(&coordinator, &args).unwrap_err();
            assert!(
                f.host
                    .evaluations(&coordinator.project_id)
                    .unwrap()
                    .is_empty()
            );
            error.to_string()
        };
        assert!(refused(Value::Null).contains("Name the apps"));
        assert!(refused(json!([])).contains("Name the apps"));
        assert!(refused(json!(["Calculator"])).contains("not a bundle id"));
        assert!(refused(json!(["com.apple.*"])).contains("not a bundle id"));
        assert!(refused(json!(["com.vyttle.wiffletree.evaluator"])).contains("Evaluator itself"));
        let many: Vec<String> = (0..11).map(|n| format!("com.example.app{n}")).collect();
        assert!(refused(json!(many)).contains("at most"));
        // An app run names a target and no apps.
        let app_folder = f.folder("a1");
        let mut app = json!({"run_folder":app_folder,"provider":"claude","model":"opus","effort":"high","mode":"app","target":f.target(&format!("{app_folder}-data")),"apps":["com.apple.calculator"]});
        let coordinator = f.coordinator.clone();
        let error = f.host.start_evaluation(&coordinator, &app).unwrap_err();
        assert!(error.to_string().contains("Leave apps out"), "{error}");
        app["apps"] = Value::Null;
        f.host.start_evaluation(&coordinator, &app).unwrap();

        let id = f.start_desktop("d2");
        let run = f.host.evaluation(&id).unwrap();
        assert_eq!(run.apps, ["com.apple.calculator"]);
        assert_eq!(run.target, None);
        let listed = f.host.list_evaluations(&coordinator, &json!({})).unwrap();
        assert_eq!(listed["runs"][0]["apps"], json!(["com.apple.calculator"]));
    }

    #[test]
    fn nothing_touches_the_screen_until_the_human_presses_start() {
        let mut f = fixture();
        f.allow(EvaluatorPermission::Desktop);
        let folder = f.folder("d1");
        let args = json!({"run_folder":folder,"provider":"claude","model":"opus","effort":"high","mode":"desktop","apps":["com.apple.calculator","com.apple.Terminal"]});
        let coordinator = f.coordinator.clone();
        let started = f.host.start_evaluation(&coordinator, &args).unwrap();
        assert_eq!(started["state"], "waiting");
        assert_eq!(started["waiting"], AWAITING_START);
        assert!(started["next"].as_str().unwrap().contains("presses Start"));
        let first = started["run_id"].as_str().unwrap().to_owned();
        let second = f.start_desktop("d2");
        // The first holds the screen with no session; the second waits behind it.
        let run = f.host.evaluation(&first).unwrap();
        assert_eq!(
            (run.state, run.session_id.as_deref()),
            (EvaluationState::Waiting, None)
        );
        assert_eq!(f.host.screen_holder().unwrap(), Some(first.clone()));
        assert_eq!(f.state(&second), EvaluationState::Queued);
        let card = f.start_card(&first).unwrap();
        assert_eq!(card.options, [START_NOW, LATER, SKIP]);
        assert!(
            card.prompt.contains("com.apple.Terminal (a terminal"),
            "{}",
            card.prompt
        );
        assert!(f.start_card(&second).is_none());
        // The card cannot be dismissed or answered with anything else, and Later leaves it.
        assert!(f.host.dismiss_attention(&card.id).is_err());
        assert!(f.answer_start(&first, "sure").is_err());
        f.answer_start(&first, LATER).unwrap();
        assert!(f.start_card(&first).is_some());
        assert_eq!(f.host.evaluation(&first).unwrap().session_id, None);
        // Start now gives it its session, and only then.
        f.answer_start(&first, START_NOW).unwrap();
        assert!(f.start_card(&first).is_none());
        let run = f.host.evaluation(&first).unwrap();
        assert_eq!(run.state, EvaluationState::Running);
        assert!(run.session_id.is_some());
        // When it ends, the next one waits for its own Start.
        f.host
            .evaluation_turn_ended(run.session_id.as_deref().unwrap(), None, None, None)
            .unwrap();
        assert_eq!(f.host.screen_holder().unwrap(), Some(second.clone()));
        assert_eq!(f.state(&second), EvaluationState::Waiting);
        assert!(f.start_card(&second).is_some());
        assert_eq!(f.host.evaluation(&second).unwrap().session_id, None);
    }

    #[test]
    fn skip_stops_the_run_at_the_screen_and_the_projects_desktop_runs_behind_it() {
        let mut f = fixture();
        f.allow(EvaluatorPermission::Desktop);
        let first = f.start_desktop("d1");
        let second = f.start_desktop("d2");
        let app = f.folder("a1");
        f.start(&app, "app").unwrap();
        f.answer_start(&first, SKIP).unwrap();
        for run in [&first, &second] {
            let run = f.host.evaluation(run).unwrap();
            assert_eq!(run.state, EvaluationState::Stopped);
            assert_eq!(run.reason.as_deref(), Some(SKIPPED));
            assert_eq!(run.session_id, None);
        }
        assert_eq!(f.host.screen_holder().unwrap(), None);
        // The coordinator hears that each ended, and the app run goes on.
        let told = f.messages_to_coordinator();
        assert!(
            told.iter()
                .any(|m| m.body.starts_with(&format!("Run {first} stopped")))
        );
        assert!(
            told.iter()
                .any(|m| m.body.starts_with(&format!("Run {second} stopped")))
        );
        let project = f.coordinator.project_id.clone();
        let running = (f.host.evaluations(&project).unwrap().into_iter())
            .filter(|run| run.state == EvaluationState::Running)
            .count();
        assert_eq!(running, 1);
    }

    #[test]
    fn a_stop_while_it_waits_for_start_closes_the_card_and_passes_the_screen_on() {
        let mut f = fixture();
        f.allow(EvaluatorPermission::Desktop);
        let first = f.start_desktop("d1");
        let second = f.start_desktop("d2");
        assert_eq!(f.host.stop_evaluation(&first).unwrap(), None);
        assert_eq!(f.state(&first), EvaluationState::Stopped);
        assert!(f.start_card(&first).is_none());
        assert_eq!(f.host.screen_holder().unwrap(), Some(second.clone()));
        assert!(f.start_card(&second).is_some());
        let coordinator = f.coordinator.clone();
        f.host
            .coordinator_stops_evaluation(&coordinator, &json!({"run_id": second}))
            .unwrap();
        assert_eq!(f.host.screen_holder().unwrap(), None);
    }

    #[test]
    fn a_desktop_run_never_goes_on_after_a_limit_without_a_fresh_start() {
        let mut f = fixture();
        f.allow(EvaluatorPermission::Desktop);
        let first = f.start_desktop("d1");
        let second = f.start_desktop("d2");
        f.answer_start(&first, START_NOW).unwrap();
        // Its turn reached the limit: it fails, rather than wait to take the screen later.
        let session = f.host.evaluation(&first).unwrap().session_id.unwrap();
        f.host
            .evaluation_turn_ended(&session, None, Some("limit"), Some(None))
            .unwrap();
        let run = f.host.evaluation(&first).unwrap();
        assert_eq!(run.state, EvaluationState::Failed);
        assert_eq!(run.reason.as_deref(), Some(DESKTOP_AT_LIMIT));
        assert_eq!(f.host.screen_holder().unwrap(), Some(second.clone()));
        // At the limit, Start now is refused and the card stays.
        let at = now();
        f.host
            .record_quota(QuotaReading {
                provider: Provider::Claude,
                account: None,
                observed_at: at,
                source: "Claude stream".into(),
                windows: vec![QuotaWindow {
                    key: "five_hour".into(),
                    label: "Session".into(),
                    used_percent: Some(100.),
                    observed_at: at,
                    duration_mins: Some(300),
                    resets_at: Some(at + 3_600_000),
                    status: None,
                }],
                error: None,
            })
            .unwrap();
        let refused = f.answer_start(&second, START_NOW).unwrap_err();
        assert!(refused.to_string().contains("usage limit"), "{refused}");
        assert!(f.start_card(&second).is_some());
        assert_eq!(f.host.evaluation(&second).unwrap().session_id, None);
    }

    #[test]
    fn a_desktop_runs_turn_starts_its_helper_on_the_runs_apps() {
        let mut f = fixture();
        f.allow(EvaluatorPermission::Desktop);
        let id = f.start_desktop("d1");
        f.answer_start(&id, START_NOW).unwrap();
        let session = f.host.evaluation(&id).unwrap().session_id.unwrap();
        let launch = f
            .host
            .evaluator_launch(&f.host.session(&session).unwrap())
            .unwrap();
        assert!(launch.app.is_none());
        let desktop = launch.desktop.unwrap();
        assert_eq!(desktop.apps, ["com.apple.calculator"]);
        assert_eq!(desktop.run, id);
        assert!(
            desktop
                .helper
                .ends_with("evaluator/Wiffletree Evaluator.app")
                || std::env::var_os("WORKSPACE_EVALUATOR_APP").is_some()
        );
        assert!(
            desktop
                .socket
                .to_string_lossy()
                .ends_with(&format!("wiffletree-{id}-desktop.sock"))
        );
    }

    #[test]
    fn a_run_the_hosts_stop_cut_short_fails_at_the_next_start_and_wakes_its_coordinator() {
        let mut f = fixture();
        f.allow(EvaluatorPermission::App);
        let started = |f: &mut Fixture, name: &str| {
            let folder = f.folder(name);
            let id = f.start(&folder, "app").unwrap()["run_id"]
                .as_str()
                .unwrap()
                .to_owned();
            let session = f.host.evaluation(&id).unwrap().session_id.unwrap();
            (id, session)
        };
        // In its turn when the host went: its prompt was delivered.
        let (cut, session) = started(&mut f, "cut");
        f.host.evaluation_turn_started(&session).unwrap();
        f.host
            .db
            .execute(
                "UPDATE messages SET receipt='delivered' WHERE recipient=?1",
                [&session],
            )
            .unwrap();
        // Waiting for its limit's reset, with its prompt held for the retry.
        let (waiting, session) = started(&mut f, "waiting");
        let mut runtime = f.host.session_runtime(&session).unwrap();
        runtime.limit_retry = Some(LimitRetry {
            failed_at: now(),
            resets_at: None,
        });
        f.host.save_runtime(&runtime).unwrap();
        f.host
            .db
            .execute(
                "UPDATE messages SET receipt='held' WHERE recipient=?1",
                [&session],
            )
            .unwrap();
        // Admitted, its prompt not delivered yet.
        let (queued, _) = started(&mut f, "queued");

        let Fixture {
            _home: home,
            runs: _runs,
            host,
            coordinator,
        } = f;
        drop(host);
        let host = Host::open_set_up(home.path()).unwrap();

        let run = host.evaluation(&cut).unwrap();
        assert_eq!(run.state, EvaluationState::Failed);
        assert_eq!(run.reason.as_deref(), Some(INTERRUPTED));
        assert!(
            host.session(run.session_id.as_deref().unwrap())
                .unwrap()
                .archived
        );
        let told: Vec<Message> = (host.messages(&coordinator.id, None, 100).unwrap())
            .into_iter()
            .filter(|m| m.id.starts_with(EVALUATION_PREFIX))
            .collect();
        assert_eq!(told.len(), 1);
        assert_eq!(told[0].body, ended_message(&run));
        for going_on in [waiting, queued] {
            assert!(!host.evaluation(&going_on).unwrap().state.ended());
        }
    }

    #[test]
    fn the_human_stops_a_waiting_run_and_closing_the_project_turns_evaluators_off() {
        let mut f = fixture();
        let folder = f.folder("run-a");
        let id = f.start(&folder, "app").unwrap()["run_id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(f.host.stop_evaluation(&id).unwrap(), None);
        let run = f.host.evaluation(&id).unwrap();
        assert_eq!(run.state, EvaluationState::Stopped);
        assert!(f.host.stop_evaluation(&id).is_err());
        f.allow(EvaluatorPermission::App);
        let other = f.folder("run-b");
        let id = f.start(&other, "app").unwrap()["run_id"]
            .as_str()
            .unwrap()
            .to_owned();
        f.host.set_archived(&f.coordinator.id, true).unwrap();
        let project = f.coordinator.project_id.clone();
        assert_eq!(
            f.host.evaluator_permission(&project).unwrap(),
            Some(EvaluatorPermission::Off)
        );
        assert_eq!(
            f.host.evaluation(&id).unwrap().state,
            EvaluationState::Stopped
        );
    }
}
