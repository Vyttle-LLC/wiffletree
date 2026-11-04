//! UI evaluator runs (OpenSpec change `ui-evaluators`). A coordinator starts one with a run
//! folder and an exact model; the human decides per project whether evaluators may run. Each
//! admitted run gets a fresh session of its own, outside every agent tree, so nothing it says
//! reaches the coordinator but the message that it ended. Full-desktop runs take turns at the
//! screen through one queue for every project.
use crate::*;

/// Where an evaluator's Codex keeps its home, under the data folder.
const CODEX_HOME: &str = "evaluators/codex-home";
const PROMPT_FILE: &str = "prompt.md";
/// Why a run waits before the human has chosen for its project.
const ASKED: &str = "Waiting for the human to allow UI evaluators in this project.";
/// Why a run the human stopped ended.
pub(crate) const STOPPED_BY_YOU: &str = "Stopped by you.";
pub(crate) const STOPPED_BY_COORDINATOR: &str = "Stopped by its coordinator.";
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
    format!(
        "Run {} {} · folder {} · {}",
        run.id,
        run.state.name(),
        run.folder,
        duration(run)
    )
}

/// What the coordinator sees of a run: never its transcript or a failure's reason.
fn summary(run: &Evaluation) -> Value {
    let waiting = (run.state == EvaluationState::Waiting).then(|| match run.session_id {
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
        "effort": run.profile.effort,
        "folder": run.folder,
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
        Ok(())
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
            &["run_folder", "provider", "model", "effort", "mode"],
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
        ensure!(
            mode == EvaluationMode::App,
            "Full desktop evaluators are not available yet. Start an app run instead."
        );
        let folder = self.run_folder(string("run_folder")?)?;
        self.check_model(&profile)?;
        let at = now();
        let run = Evaluation {
            id: format!("run-{}", &new_id()[..8]),
            project_id: caller.project_id.clone(),
            coordinator_id: caller.id.clone(),
            session_id: None,
            profile,
            mode,
            folder: folder.to_string_lossy().into_owned(),
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
            EvaluationState::Waiting if run.session_id.is_none() => {
                "The human decides whether this project allows UI evaluators. Finish your turn: Wiffletree messages you their answer, and when the run ends."
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

    /// Refuses a provider not signed in here, a model its CLI did not list and an effort the
    /// model does not offer. Until a provider has listed its models, Wiffletree's own lineup
    /// stands in.
    fn check_model(&self, profile: &ModelProfile) -> Result<()> {
        let provider = profile.provider;
        ensure!(
            self.detected.get(&provider) == Some(&true),
            "{} is not signed in on this Mac, or Wiffletree has not checked it yet. Ask the human.",
            provider.label()
        );
        let options: Vec<ModelOption> = match self.listed_models.get(&provider) {
            Some(listed) => listed.clone(),
            None => (fallback_catalog().into_iter())
                .filter(|option| option.provider == provider)
                .collect(),
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
        self.stop_evaluation_with(run, STOPPED_BY_YOU)
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

    /// Starts every queued run its project allows: each app run at once, and the oldest
    /// desktop run whenever no run holds the screen.
    pub(crate) fn admit_evaluations(&mut self) -> Result<()> {
        let queued: Vec<Evaluation> = self
            .db
            .prepare("SELECT data FROM evaluations WHERE json_extract(data,'$.state')='queued' ORDER BY created_at, rowid")?
            .query_map([], |r| decode(r, 0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut screen_taken = self.screen_holder()?.is_some();
        for run in queued {
            if run.mode == EvaluationMode::Desktop {
                if screen_taken {
                    continue;
                }
                screen_taken = true;
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

    /// The desktop run that holds the screen: the one admitted and not ended. One at a time.
    pub(crate) fn screen_holder(&self) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT id FROM evaluations WHERE json_extract(data,'$.mode')='desktop' AND session_id IS NOT NULL AND json_extract(data,'$.state') NOT IN ('finished','failed','stopped')",
                [],
                |r| r.get(0),
            )
            .optional()?)
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
        Ok(provider::EvaluatorLaunch {
            folder: fs::canonicalize(&run.folder)?,
            codex_home,
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

/// Why a run ended when what its project allows no longer allows it.
pub(crate) fn not_allowed(mode: EvaluationMode) -> String {
    format!("Stopped: {} is not allowed in this project.", mode.label())
}
/// The start of the operation ids of `project`'s Inbox cards; each ends with the run that asked.
fn card_prefix(project: &str) -> String {
    format!("{EVALUATION_PREFIX}allow:{project}:")
}
fn permission_label(permission: EvaluatorPermission) -> &'static str {
    (EvaluatorPermission::CHOICES.iter())
        .find(|(_, choice)| *choice == permission)
        .map_or("", |(label, _)| label)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROMPT: &str = "Open Models in dark mode at 1280 wide.\nWrite report.md.";

    struct Fixture {
        _home: tempfile::TempDir,
        runs: tempfile::TempDir,
        host: Host,
        coordinator: Session,
    }
    fn fixture() -> Fixture {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
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
            let args = json!({"run_folder":folder,"provider":"claude","model":"opus","effort":"high","mode":mode});
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
        /// A desktop run as `start_evaluation` will create one once ticket 4 allows it.
        fn queue_desktop(&mut self, name: &str) -> String {
            let id = self.app_run(name);
            let mut run = self.host.evaluation(&id).unwrap();
            run.id = format!("desktop-{id}");
            run.mode = EvaluationMode::Desktop;
            run.session_id = None;
            run.state = EvaluationState::Queued;
            run.ended_at = None;
            self.host.save_evaluation(&run).unwrap();
            run.id
        }
        /// An ended app run, as a source for `queue_desktop`.
        fn app_run(&mut self, name: &str) -> String {
            let folder = self.folder(name);
            let id = self.start(&folder, "app").unwrap()["run_id"]
                .as_str()
                .unwrap()
                .to_owned();
            let session = self.host.evaluation(&id).unwrap().session_id.unwrap();
            self.host
                .evaluation_turn_ended(&session, None, None, None)
                .unwrap();
            id
        }
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
        let args = |folder: &str, model: &str, effort: &str, mode: &str| json!({"run_folder":folder,"provider":"claude","model":model,"effort":effort,"mode":mode});
        // Desktop is not available until ticket 4, and raises no card.
        let desktop = refused(&mut f, args(&folder, "opus", "high", "desktop"));
        assert!(desktop.contains("not available yet"), "{desktop}");
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
        let signed_out = ProviderStatus::default();
        f.host.record_detection(Provider::Codex, &signed_out);
        let mut codex = args(&folder, "gpt-6.1-sol", "high", "app");
        codex["provider"] = json!("codex");
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
            format!("Run {id} finished · folder {} · under 1 min", run.folder)
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
        let first = f.queue_desktop("d1");
        let second = f.queue_desktop("d2");
        f.host.admit_evaluations().unwrap();
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
        let project = f.coordinator.project_id.clone();
        let running = (f.host.evaluations(&project).unwrap().into_iter())
            .filter(|run| run.state == EvaluationState::Running)
            .count();
        assert_eq!(running, 2);
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
