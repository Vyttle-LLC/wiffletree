//! Repeatable application-overhead fixture. Never opens a user's project store.
use anyhow::Result;
use rusqlite::{Connection, params};
use serde_json::json;
use std::time::{Duration, Instant};
use workspace_core::*;
use workspace_host::{Host, now};

fn samples(
    name: &str,
    count: usize,
    mut work: impl FnMut(usize) -> Result<()>,
) -> Result<serde_json::Value> {
    let mut times = Vec::with_capacity(count);
    for i in 0..count {
        let start = Instant::now();
        work(i)?;
        times.push(start.elapsed());
    }
    times.sort();
    let ms = |duration: Duration| duration.as_secs_f64() * 1000.;
    Ok(
        json!({"measure":name,"samples":count,"p50_ms":ms(times[count/2]),"p95_ms":ms(times[count*95/100]),"max_ms":ms(times[count-1])}),
    )
}
/// An edit's diff at the stored limit: three hunks, `MAX_DIFF_LINES` lines of code in all.
fn edit_diff() -> FileDiff {
    let line = |sign: char, n: usize| {
        format!("{sign}    let value_{n} = compute(input, {n}).unwrap_or_default();")
    };
    let hunk = |start: u32, size: usize| DiffHunk {
        old_start: start,
        new_start: start,
        lines: (0..size)
            .map(|n| line([' ', '-', '+', '+'][n % 4], n))
            .collect(),
    };
    let sizes = [14, 13, MAX_DIFF_LINES - 27];
    let hunks: Vec<_> = sizes
        .iter()
        .enumerate()
        .map(|(i, &size)| hunk(40 + 120 * i as u32, size))
        .collect();
    let count = |sign: char| {
        hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|l| l.starts_with(sign))
            .count() as u32
    };
    FileDiff {
        path: "crates/workspace-host/src/stacks.rs".into(),
        created: false,
        added: count('+'),
        removed: count('-'),
        omitted_lines: 0,
        hunks,
    }
}

/// What the desktop waits for from its own host process: a request written to `host.sock`,
/// the host's actor and JSON, and the reply decoded. The host is the `workspace-host` built
/// beside this example, started as a child the way the app starts one, with no provider found.
fn over_the_socket(directory: &std::path::Path, session: &str) -> Result<Vec<serde_json::Value>> {
    let examples = std::env::current_exe()?;
    let binary = examples
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .join("workspace-host");
    anyhow::ensure!(
        binary.is_file(),
        "Build {} first: cargo build --release -p workspace-host --bin workspace-host",
        binary.display()
    );
    let launch = ChildLaunch::new(&binary, directory)
        .env("SHELL", "/usr/bin/false")
        .env("PATH", "/usr/bin:/bin")
        .env("WORKSPACE_CLAUDE_BIN", "/nonexistent/claude")
        .env("WORKSPACE_CODEX_BIN", "/nonexistent/codex");
    let client = Client {
        name: "benchmark".into(),
        kind: "script".into(),
    };
    let client = HostClient::connect(Box::new(launch), client, Box::new(|_| {}))?;
    let answer = |command: Command| -> Result<serde_json::Value> {
        let reply = client.request(command).map_err(anyhow::Error::msg)?;
        reply.recv_blocking()?.map_err(anyhow::Error::msg)
    };
    let results = vec![
        samples(
            "transcript page (100 rows) over host.sock, decoded",
            1000,
            |_| {
                let page = answer(Command::Messages {
                    session_id: session.into(),
                    before: None,
                    limit: 100,
                })?;
                std::hint::black_box(serde_json::from_value::<Vec<Message>>(page)?);
                Ok(())
            },
        )?,
        samples("workspace snapshot over host.sock, decoded", 200, |_| {
            let snapshot = answer(Command::Snapshot)?;
            std::hint::black_box(serde_json::from_value::<Snapshot>(snapshot)?);
            Ok(())
        })?,
    ];
    client.stop();
    Ok(results)
}

/// A finished store's shape: 140 tickets, ten still open, each with an implementer and four
/// verifiers, so 700 ticket agents, all archived but the open tickets' implementers. Every
/// agent has a runtime, and each open implementer has a question waiting for its coordinator.
fn seed_history(
    tx: &rusqlite::Transaction,
    roots: &[Session],
    repositories: &[(String, Repository)],
) -> Result<()> {
    const OPEN: [&str; 5] = [
        "assigned",
        "verifying",
        "ready_for_testing",
        "blocked",
        "passed",
    ];
    const VERIFIERS: [(Role, &str); 4] = [
        (Role::Tester, "Tests"),
        (Role::Reviewer, "Claude"),
        (Role::Reviewer, "Codex"),
        (Role::Reviewer, "Style"),
    ];
    let mut session = tx.prepare("INSERT INTO sessions VALUES (?1,?2,?3,?4,?5)")?;
    let mut runtime = tx.prepare("INSERT INTO runtimes VALUES (?1,?2)")?;
    let mut ticket_row = tx.prepare("INSERT INTO tickets VALUES (?1,?2,?3)")?;
    let mut question = tx.prepare("INSERT INTO messages(id,project_id,sender,recipient,body,receipt,created_at) VALUES (?1,?2,?3,?4,'[blocked] Which base?','completed',?5)")?;
    for i in 0..140 {
        let root = &roots[i % roots.len()];
        let repository = &repositories
            .iter()
            .find(|(p, _)| *p == root.project_id)
            .unwrap()
            .1;
        let open = i < 10;
        let state = if open {
            OPEN[i % OPEN.len()]
        } else if i % 2 == 0 {
            "accepted"
        } else {
            "closed"
        };
        let ticket_id = new_id();
        let worktree = format!("/benchmark/tasks/project-{}/ticket-{i}", i % roots.len());
        let agents: Vec<(Role, Option<&str>)> = std::iter::once((Role::Implementer, None))
            .chain(VERIFIERS.iter().map(|(role, focus)| (*role, Some(*focus))))
            .collect();
        let mut verifiers = vec![];
        for (role, focus) in agents {
            let id = new_id();
            let name = format!(
                "{} · {} · Fixture ticket {i}: a title as long as a real one",
                role.agent_label(focus),
                "Fixture"
            );
            let agent = Session {
                id: id.clone(),
                project_id: root.project_id.clone(),
                parent_id: Some(root.id.clone()),
                repository_id: Some(repository.id.clone()),
                name,
                role,
                provider: Provider::Claude,
                status: Status::Done,
                archived: !(open && role == Role::Implementer),
                owner: None,
            };
            session.execute(params![
                agent.id,
                agent.project_id,
                agent.parent_id,
                serde_json::to_value(role)?.as_str(),
                serde_json::to_string(&agent)?
            ])?;
            let record = json!({"session_id":id,"provider_session_id":new_id(),"profile":{"provider":"claude","model":"opus","effort":"high"},
                "ticket_id":ticket_id,"focus":focus,"workdir":worktree,"directory":worktree,"last_error":null,
                "selection":{"chosen_by":{"kind":"coordinator","session_id":root.id},"reason":"Large cross-crate change; the guide says raise effort","revision":3,"at":now()},
                "last_started_at":now(),"last_finished_at":now()});
            runtime.execute(params![
                id,
                serde_json::to_string(&serde_json::from_value::<SessionRuntime>(record)?)?
            ])?;
            if let Some(focus) = focus {
                verifiers.push(json!({"session_id":id,"role":role,"focus":focus,"message_id":format!("verify:{ticket_id}:1:1:{id}"),"result":"passed","report_id":format!("report:{id}:{}", new_id())}));
            } else if open {
                question.execute(params![
                    format!("report:{id}:{}", new_id()),
                    root.project_id,
                    id,
                    root.id,
                    now()
                ])?;
            }
        }
        let round = |n: u32| json!({"round":n,"commit":"0123456789abcdef0123456789abcdef01234567","verifiers":verifiers});
        let ledger: Vec<_> = (1..=if open { 4 } else { 18 }).map(|n| json!({"id":format!("F{n}"),"severity":"non_blocking","location":"crates/workspace-desktop/src/sidebar.rs:412",
            "summary":"The row label repeats the ticket title it sits under","trigger":"x".repeat(150),"evidence":"y".repeat(190),"focus":"Codex","cycle":1,"round":1,"status":"follow_up","reason":"z".repeat(110)})).collect();
        let pull_request = (state == "accepted").then(|| json!({"number":i,"url":format!("https://github.com/fixture/repo/pull/{i}"),"state":"merged","draft":false,"base":"main",
            "head":"0123456789abcdef0123456789abcdef01234567","merge_state":"clean","checks":"success","unresolved_threads":0,"comments":{"issue":null,"review":null,"inline":null}}));
        let ticket = json!({"id":ticket_id,"coordinator_id":root.id,"repository_id":repository.id,"title":format!("Fixture ticket {i}: a title as long as a real one"),
            "brief":"b".repeat(4400),"worktree":worktree,"branch":format!("wiffletree/fixture-ticket-{i}"),"state":state,
            "verification":{"cycle":2,"max_rounds":2,"outcome":"passed","rounds":[round(1)]},
            "previous_cycles":[{"cycle":1,"max_rounds":2,"outcome":"blocked","rounds":[round(1),round(2)]}],
            "ledger":ledger,"pull_request":pull_request});
        let ticket: Ticket = serde_json::from_value(ticket)?;
        ticket_row.execute(params![
            ticket.id,
            ticket.coordinator_id,
            serde_json::to_string(&ticket)?
        ])?;
    }
    Ok(())
}
fn main() -> Result<()> {
    // With a folder argument the fixture is built there and kept, for the desktop's tree bench.
    let kept = std::env::args().nth(1).map(std::path::PathBuf::from);
    let temporary = tempfile::tempdir()?;
    let directory = kept.as_deref().unwrap_or(temporary.path());
    let mut host = Host::open_set_up(directory)?;
    let mut projects = Vec::new();
    for i in 0..5 {
        projects.push(host.create_project(&format!("Fixture {i}"))?);
    }
    let roots = host.sessions()?;
    drop(host);
    let mut db = Connection::open(directory.join("workspace.sqlite3"))?;
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")?;
    let tx = db.transaction()?;
    let mut repositories = Vec::new();
    for (i, project) in projects.iter().enumerate() {
        for j in 0..2 {
            let repo = Repository {
                id: new_id(),
                name: format!("fixture-{i}-{j}"),
                path: format!("/benchmark/repo-{i}-{j}"),
                base: "fixture-base".into(),
                github: None,
                origin_problem: None,
                checks: None,
                protected_branches: vec![],
            };
            tx.execute(
                "INSERT INTO repositories VALUES (?1,?2,?3)",
                params![repo.id, repo.path, serde_json::to_string(&repo)?],
            )?;
            repositories.push((project.id.clone(), repo));
        }
    }
    tx.commit()?;
    drop(db);
    let mut host = Host::open_set_up(directory)?;
    // 45 agents across the projects' repositories, for 50 sessions with the coordinators.
    for i in 0..45 {
        let (project_id, repo) = &repositories[i % repositories.len()];
        let root = roots.iter().find(|r| &r.project_id == project_id).unwrap();
        host.create_session(
            project_id,
            &root.id,
            Some(&repo.id),
            &format!("Worker {i}"),
            Role::Implementer,
            Provider::Codex,
        )?;
    }
    let session = host.sessions()?.last().unwrap().clone();
    drop(host);
    let mut db = Connection::open(directory.join("workspace.sqlite3"))?;
    db.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")?;
    let tx = db.transaction()?;
    let seed_started = Instant::now();
    seed_history(&tx, &roots, &repositories)?;
    {
        let mut messages=tx.prepare("INSERT INTO messages(id,project_id,sender,recipient,body,receipt,created_at) VALUES (?1,?2,?3,?3,?4,'completed',?5)")?;
        let mut activity=tx.prepare("INSERT INTO activity(project_id,session_id,kind,detail,created_at) VALUES (?1,?2,'fixture','Historical activity',?3)")?;
        let mut logs=tx.prepare("INSERT INTO work_logs(id,project_id,session_id,milestone,repository,data) VALUES (?1,?2,?3,?4,?5,?6)")?;
        for i in 0..100_000 {
            messages.execute(params![
                format!("history-{i}"),
                session.project_id,
                session.id,
                "x".repeat(256),
                now()
            ])?;
            activity.execute(params![session.project_id, session.id, now()])?;
            let input = LogInput {
                session_id: session.id.clone(),
                milestone: Some(format!("milestone-{i}")),
                kind: LogKind::Observation,
                title: format!("Milestone {i}"),
                repository: None,
                ticket_id: None,
                changed: "Historical source entry".into(),
                why: "Performance fixture".into(),
                state: "Synthetic".into(),
            };
            let log = WorkLog {
                sequence: 0,
                id: format!("log-{i}"),
                project_id: session.project_id.clone(),
                task_id: session.parent_id.clone(),
                repository: "fixture".into(),
                created_at: now(),
                date: "2026-10-30".into(),
                input,
                reference: "—".into(),
                space_id: None,
                brain: None,
                path: None,
                batch_id: None,
                author: None,
                provider: None,
                model: None,
                machine: None,
                in_legacy_file: false,
                held: None,
                released: false,
            };
            logs.execute(params![
                log.id,
                log.project_id,
                log.input.session_id,
                log.input.milestone,
                log.repository,
                serde_json::to_string(&log)?
            ])?;
        }
    }
    // One long run at the step limit, and a page of finished runs for reply summaries.
    let long_run = "steps-long";
    let summary_runs: Vec<String> = (0..MAX_PAGE).map(|i| format!("steps-{i}")).collect();
    {
        let mut runs = tx.prepare("INSERT INTO provider_runs(id,session_id,messages,started_at,finished_at,detail) VALUES (?1,?2,'[]',?3,?3,'{}')")?;
        let mut steps = tx.prepare(
            "INSERT INTO steps(session_id,run_id,id,revision,data) VALUES (?1,?2,?3,?4,?5)",
        )?;
        let mut revision = 0;
        for run in summary_runs.iter().map(String::as_str).chain([long_run]) {
            runs.execute(params![run, session.id, now()])?;
            let count = if run == long_run { MAX_RUN_STEPS } else { 20 };
            for seq in 0..count {
                revision += 1;
                // Every fourth step is an edit with a full diff, as an implementer's run has.
                let edit = seq % 4 == 2;
                let step = Step {
                    run_id: run.into(),
                    id: format!("step-{seq}"),
                    parent_id: None,
                    kind: match seq % 4 {
                        0 => StepKind::Narration,
                        2 => StepKind::FileEdit,
                        _ => StepKind::Command,
                    },
                    state: StepState::Succeeded,
                    title: "cargo test --workspace".into(),
                    note: Some("exit 0".into()),
                    detail: (!edit).then(|| "x".repeat(512)),
                    paths: vec![],
                    plan: None,
                    diffs: if edit { vec![edit_diff()] } else { vec![] },
                    omitted: 0,
                    seq: seq as u32,
                    revision,
                    started_at: now(),
                    finished_at: Some(now()),
                };
                steps.execute(params![
                    session.id,
                    run,
                    step.id,
                    revision as i64,
                    serde_json::to_string(&step)?
                ])?;
            }
        }
    }
    tx.commit()?;
    let seed_ms = seed_started.elapsed().as_secs_f64() * 1000.;
    drop(db);
    let mut host = Host::open_set_up(directory)?;
    let snapshot_bytes = serde_json::to_vec(&host.snapshot()?)?.len();
    let archive_bytes = serde_json::to_vec(&host.archive()?)?.len();
    let (live, archive) = (host.snapshot()?, std::rc::Rc::new(host.archive()?));
    let results = vec![
        samples("indexed transcript page (100 rows)", 1000, |_| {
            std::hint::black_box(host.messages(&session.id, None, 100)?);
            Ok(())
        })?,
        samples(
            "workspace snapshot (750 sessions, 140 tickets)",
            200,
            |_| {
                std::hint::black_box(host.snapshot()?);
                Ok(())
            },
        )?,
        // What the desktop waits for: the host's build, its JSON value and the decode.
        samples("workspace snapshot with JSON round trip", 200, |_| {
            let value = serde_json::to_value(host.snapshot()?)?;
            std::hint::black_box(serde_json::from_value::<Snapshot>(value)?);
            Ok(())
        })?,
        // Read again only when its stamp moves: on archiving, accepting or closing.
        samples("archive with JSON round trip", 200, |_| {
            let value = serde_json::to_value(host.archive()?)?;
            std::hint::black_box(serde_json::from_value::<Archive>(value)?);
            Ok(())
        })?,
        // As the desktop joins them on each snapshot; the copy of the snapshot is timed too.
        samples("workspace index of snapshot and archive", 200, |_| {
            let index = WorkspaceIndex::new(live.clone(), archive.clone());
            std::hint::black_box(index);
            Ok(())
        })?,
        samples("indexed activity page (100 rows)", 1000, |_| {
            std::hint::black_box(host.activity(&session.project_id, None, 100)?);
            Ok(())
        })?,
        samples("indexed source-memory page (100 rows)", 1000, |_| {
            std::hint::black_box(host.logs(&session.project_id, None, 100)?);
            Ok(())
        })?,
        samples("step page, whole 2,000-step run", 1000, |_| {
            std::hint::black_box(host.steps(&session.id, Some(long_run), 0)?);
            Ok(())
        })?,
        samples("step page after cursor, nothing new", 1000, |_| {
            std::hint::black_box(host.steps(&session.id, Some(long_run), u32::MAX as u64)?);
            Ok(())
        })?,
        samples("reply summaries (100 runs)", 1000, |_| {
            std::hint::black_box(host.run_summaries(&session.id, &summary_runs)?);
            Ok(())
        })?,
        samples("durable 4 KiB enqueue (FULL sync)", 200, |i| {
            host.send(
                &format!("measured-{i}"),
                None,
                &session.id,
                &"x".repeat(4096),
            )?;
            Ok(())
        })?,
    ];
    drop(host);
    let reopen = samples("host restart and cached snapshot", 100, |_| {
        let host = Host::open_set_up(directory)?;
        std::hint::black_box(host.snapshot()?);
        Ok(())
    })?;
    let socket = over_the_socket(directory, &session.id)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"build":if cfg!(debug_assertions){"debug"}else{"release"},"durability":"SQLite WAL / synchronous=FULL","fixture":{"projects":5,"repositories":"10 metadata fixtures; no Git history benchmark","sessions":750,"tickets":140,"snapshot_bytes":snapshot_bytes,"archive_bytes":archive_bytes,"messages":100000,"activity":100000,"source_logs":100000,"step_runs":101,"steps":MAX_RUN_STEPS+MAX_PAGE*20},"seed_ms":seed_ms,"results":results,"recovery":reopen,"socket":socket,"excluded":"native UI, GPU, provider runtimes/inference, in-memory step streaming, filesystem import and long soak"})
        )?
    );
    Ok(())
}
