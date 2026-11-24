//! The UI evaluators' screens fixture: a store whose runs are in every state, for looking at
//! the app's Inbox cards, Evaluations group, run page and bands without running an evaluator.
//! Never opens a user's project store, and nothing in it starts a provider or takes the screen.
//!
//! `evaluator_screens seed <new folder> cards` builds a project that has not chosen yet, with
//! a run held on its "Allow UI evaluators" card, and a project allowed Full desktop whose run
//! waits on "Ready to use your screen" with another queued behind it.
//!
//! `evaluator_screens seed <new folder> runs` builds a project allowed Full desktop with a run
//! in each state: an app-only run and a full-desktop run running, one waiting for its model's
//! reset, one queued for the screen, and finished, stopped and failed ones. The app-only run
//! has stored steps, and its screenshot steps point at PNG files `<folder>/runs/…`; copy any
//! PNG there to see its thumbnails.
//!
//! Open the store with `workspace-desktop --data-dir <folder>/home` and the fake provider CLIs.
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, params};
use serde_json::json;
use std::path::Path;
use workspace_core::*;
use workspace_host::{Host, now};

const MINUTE: i64 = 60_000;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["seed", dir, "cards"] => seed(Path::new(dir), false),
        ["seed", dir, "runs"] => seed(Path::new(dir), true),
        _ => anyhow::bail!("Usage: evaluator_screens seed <new folder> cards|runs"),
    }
}

fn profile(provider: Provider, model: &str) -> ModelProfile {
    ModelProfile {
        provider,
        model: model.into(),
        effort: "high".into(),
    }
}

/// A run of `coordinator`'s project with its folder made under `runs`.
fn run(id: &str, coordinator: &Session, runs: &Path, state: EvaluationState) -> Result<Evaluation> {
    let folder = runs.join(id);
    std::fs::create_dir_all(folder.join("screenshots"))?;
    std::fs::write(folder.join("prompt.md"), "Open Models in dark mode.\n")?;
    Ok(Evaluation {
        id: id.into(),
        project_id: coordinator.project_id.clone(),
        coordinator_id: coordinator.id.clone(),
        session_id: None,
        profile: profile(Provider::Claude, "opus"),
        reported_model: None,
        mode: EvaluationMode::App,
        folder: folder.to_string_lossy().into_owned(),
        target: Some(EvaluationTarget {
            executable: "/Applications/Wiffletree.app/Contents/MacOS/workspace-desktop".into(),
            arguments: vec!["--data-dir".into(), "/tmp/wiffletree-ux-eval".into()],
            environment: Default::default(),
        }),
        apps: vec![],
        holds_screen: false,
        state,
        reason: None,
        resets_at: None,
        created_at: now() - 60 * MINUTE,
        started_at: None,
        ended_at: None,
    })
}

fn desktop(mut run: Evaluation, apps: &[&str]) -> Evaluation {
    run.mode = EvaluationMode::Desktop;
    run.target = None;
    run.apps = apps.iter().map(|app| (*app).to_owned()).collect();
    run
}

fn save(db: &Connection, run: &Evaluation) -> Result<()> {
    db.execute(
        "INSERT INTO evaluations VALUES (?1,?2,?3,?4,?5)",
        params![
            run.id,
            run.project_id,
            run.session_id,
            run.created_at,
            serde_json::to_string(run)?
        ],
    )?;
    Ok(())
}

/// Gives `run` a session of its own, as the host does when it starts, with one turn whose
/// steps are `steps`, each a title and the screenshot it saved, if any.
fn with_session(
    db: &Connection,
    run: &mut Evaluation,
    steps: &[(&str, Option<&str>)],
) -> Result<()> {
    let session = Session {
        id: format!("session-{}", run.id),
        project_id: String::new(),
        parent_id: None,
        repository_id: None,
        name: format!("UI evaluator · {}", run.id),
        role: Role::Evaluator,
        provider: run.profile.provider,
        status: Status::Working,
        archived: false,
        owner: Some(SessionOwner::Project(run.project_id.clone())),
    };
    db.execute(
        "INSERT INTO sessions VALUES (?1,NULL,NULL,'evaluator',?2)",
        params![session.id, serde_json::to_string(&session)?],
    )?;
    // A retry a week away keeps the host from ending the run as cut short when it starts,
    // and from starting a turn while the fixture is looked at: it has no input to retry.
    let runtime = SessionRuntime {
        session_id: session.id.clone(),
        profile: Some(run.profile.clone()),
        workdir: Some(run.folder.clone()),
        limit_retry: Some(LimitRetry {
            failed_at: now(),
            resets_at: Some(now() + 7 * 24 * 60 * MINUTE),
        }),
        ..Default::default()
    };
    db.execute(
        "INSERT INTO runtimes VALUES (?1,?2)",
        params![session.id, serde_json::to_string(&runtime)?],
    )?;
    let turn = format!("turn-{}", run.id);
    let started = run.started_at.unwrap_or(run.created_at);
    db.execute(
        "INSERT INTO provider_runs (id,session_id,messages,started_at) VALUES (?1,?2,'[]',?3)",
        params![turn, session.id, started],
    )?;
    for (seq, (title, shot)) in steps.iter().enumerate() {
        let at = started + seq as i64 * 7_000;
        let step = Step {
            run_id: turn.clone(),
            id: format!("step-{seq}"),
            parent_id: None,
            kind: StepKind::Tool,
            state: StepState::Succeeded,
            title: (*title).into(),
            note: None,
            detail: None,
            paths: shot.iter().map(|shot| (*shot).to_owned()).collect(),
            plan: None,
            diffs: vec![],
            omitted: 0,
            seq: seq as u32,
            revision: seq as u64 + 1,
            started_at: at,
            finished_at: Some(at + 1_000),
        };
        db.execute(
            "INSERT INTO steps VALUES (?1,?2,?3,?4,?5)",
            params![
                session.id,
                turn,
                step.id,
                step.revision as i64,
                serde_json::to_string(&step)?
            ],
        )?;
    }
    run.session_id = Some(session.id);
    Ok(())
}

/// An Inbox card of the host's for `run`, as it raises them.
fn card(
    db: &Connection,
    run: &Evaluation,
    operation: &str,
    prompt: &str,
    options: &[&str],
) -> Result<()> {
    let item = Attention {
        id: format!("card-{}", run.id),
        project_id: run.project_id.clone(),
        session_id: run.coordinator_id.clone(),
        host: "local".into(),
        operation_id: operation.into(),
        prompt: prompt.into(),
        options: options.iter().map(|option| (*option).to_owned()).collect(),
        answer: None,
        repository_request: None,
    };
    db.execute(
        "INSERT INTO attention VALUES (?1,?2,?3,?4,?5)",
        params![
            item.id,
            item.project_id,
            item.session_id,
            item.operation_id,
            serde_json::to_string(&item)?
        ],
    )?;
    Ok(())
}

fn seed(dir: &Path, runs_shown: bool) -> Result<()> {
    ensure!(
        !dir.exists() || dir.read_dir()?.next().is_none(),
        "{} is not empty; the fixture needs a new folder",
        dir.display()
    );
    let home = dir.join("home");
    let runs = dir.join("runs");
    let mut host = Host::open_set_up(&home)?;
    let coordinator_of = |host: &Host, project: &Project| -> Result<Session> {
        (host.sessions()?.into_iter())
            .find(|s| s.project_id == project.id && s.parent_id.is_none())
            .context("coordinator")
    };
    let loop_project = host.create_project("UX eval-and-improve loop")?;
    let coordinator = coordinator_of(&host, &loop_project)?;
    let other = host.create_project("Theme files")?;
    let other_coordinator = coordinator_of(&host, &other)?;
    let (loop_id, other_id) = (loop_project.id.clone(), other.id.clone());
    if runs_shown {
        host.set_evaluator_permission(&loop_id, EvaluatorPermission::Desktop)?;
    } else {
        host.set_evaluator_permission(&other_id, EvaluatorPermission::Desktop)?;
    }
    drop(host);
    let db = Connection::open(home.join("workspace.sqlite3"))?;
    let at = now();
    if runs_shown {
        let mut app = run(
            "run-3c07d1a2",
            &coordinator,
            &runs,
            EvaluationState::Running,
        )?;
        app.started_at = Some(at - 12 * MINUTE);
        app.reported_model = Some("claude-opus-5-5".into());
        with_session(
            &db,
            &mut app,
            &[
                ("resize window to 1280 × 820", None),
                ("click 1182, 64", None),
                (
                    "screenshot · screenshots/models-light.png",
                    Some("screenshots/models-light.png"),
                ),
                ("key cmd-shift-d", None),
                (
                    "screenshot · screenshots/models-dark.png",
                    Some("screenshots/models-dark.png"),
                ),
            ],
        )?;
        save(&db, &app)?;
        let mut screen = desktop(
            run(
                "run-d4e1a7c9",
                &coordinator,
                &runs,
                EvaluationState::Running,
            )?,
            &["com.apple.calculator", "com.apple.TextEdit"],
        );
        screen.holds_screen = true;
        screen.started_at = Some(at - 4 * MINUTE);
        screen.reported_model = Some("claude-opus-5-5".into());
        with_session(&db, &mut screen, &[("open com.apple.calculator", None)])?;
        save(&db, &screen)?;
        let mut waiting = run(
            "run-8e41b6f0",
            &coordinator,
            &runs,
            EvaluationState::Waiting,
        )?;
        waiting.profile = profile(Provider::Codex, "gpt-6.1-sol");
        waiting.resets_at = Some(at + 48 * MINUTE);
        with_session(&db, &mut waiting, &[])?;
        save(&db, &waiting)?;
        let queued = desktop(
            run("run-a19f2c55", &coordinator, &runs, EvaluationState::Queued)?,
            &["com.apple.calculator"],
        );
        save(&db, &queued)?;
        let mut finished = run(
            "run-91fb906c",
            &coordinator,
            &runs,
            EvaluationState::Finished,
        )?;
        finished.created_at = at - 120 * MINUTE;
        finished.started_at = Some(at - 110 * MINUTE);
        finished.ended_at = Some(at - 69 * MINUTE);
        save(&db, &finished)?;
        let mut stopped = run(
            "run-5d2e0b9a",
            &coordinator,
            &runs,
            EvaluationState::Stopped,
        )?;
        stopped.profile = profile(Provider::Codex, "gpt-6.1-sol");
        stopped.created_at = at - 150 * MINUTE;
        stopped.ended_at = Some(at - 140 * MINUTE);
        stopped.reason = Some("Stopped by you.".into());
        save(&db, &stopped)?;
        let mut failed = desktop(
            run("run-0b7c2e14", &coordinator, &runs, EvaluationState::Failed)?,
            &["com.apple.calculator"],
        );
        failed.created_at = at - 180 * MINUTE;
        failed.ended_at = Some(at - 178 * MINUTE);
        failed.reason = Some("Stopped at the provider's usage limit. A full-desktop run never goes on without your Start; the coordinator can start it again.".into());
        save(&db, &failed)?;
    } else {
        let mut held = run(
            "run-91fb906c",
            &coordinator,
            &runs,
            EvaluationState::Waiting,
        )?;
        held.reason = Some("Waiting for the human to allow UI evaluators in this project.".into());
        save(&db, &held)?;
        let mut second = run(
            "run-8e41b6f0",
            &coordinator,
            &runs,
            EvaluationState::Waiting,
        )?;
        second.profile = profile(Provider::Codex, "gpt-6.1-sol");
        second.created_at += 1;
        save(&db, &second)?;
        card(
            &db,
            &held,
            &format!("{EVALUATION_PREFIX}allow:{loop_id}:{}", held.id),
            "Allow UI evaluators in this project?\nThe coordinator wants to start an evaluator.",
            &["App only", "Full desktop", "Not in this project"],
        )?;
        let mut front = desktop(
            run(
                "run-a19f2c55",
                &other_coordinator,
                &runs,
                EvaluationState::Waiting,
            )?,
            &["com.apple.calculator", "com.apple.Terminal"],
        );
        front.holds_screen = true;
        front.reason = Some("Waiting for the human to press Start.".into());
        save(&db, &front)?;
        let mut behind = desktop(
            run(
                "run-c3f0d812",
                &other_coordinator,
                &runs,
                EvaluationState::Queued,
            )?,
            &["com.apple.calculator"],
        );
        behind.created_at += 1;
        save(&db, &behind)?;
        card(
            &db,
            &front,
            &format!("{EVALUATION_PREFIX}start:{}", front.id),
            "Ready to use your screen\nUI evaluator run run-a19f2c55 is next for the screen.",
            &["Start now", "Later", "Skip these runs"],
        )?;
    }
    println!("{}", json!({"home": home, "runs": runs}));
    Ok(())
}
