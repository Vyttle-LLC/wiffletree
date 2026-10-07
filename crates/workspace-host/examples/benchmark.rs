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
fn main() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut host = Host::open(directory.path())?;
    let mut projects = Vec::new();
    for i in 0..5 {
        projects.push(host.create_project(&format!("Fixture {i}"))?);
    }
    let roots = host.sessions()?;
    drop(host);
    let mut db = Connection::open(directory.path().join("workspace.sqlite3"))?;
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
    let mut host = Host::open(directory.path())?;
    let mut tasks = Vec::new();
    for (i, (project_id, repo)) in repositories.iter().enumerate() {
        let root = roots.iter().find(|r| &r.project_id == project_id).unwrap();
        tasks.push(host.create_session(
            project_id,
            &root.id,
            Some(&repo.id),
            &format!("Task {i}"),
            Role::TaskOrchestrator,
            Provider::Codex,
        )?);
    }
    for i in 0..35 {
        let task = &tasks[i % 10];
        host.create_session(
            &task.project_id,
            &task.id,
            None,
            &format!("Worker {i}"),
            Role::Implementer,
            Provider::Codex,
        )?;
    }
    let session = host.sessions()?.last().unwrap().clone();
    drop(host);
    let mut db = Connection::open(directory.path().join("workspace.sqlite3"))?;
    db.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")?;
    let tx = db.transaction()?;
    let seed_started = Instant::now();
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
                milestone: format!("milestone-{i}"),
                kind: LogKind::Observation,
                title: format!("Milestone {i}"),
                reference: "—".into(),
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
                input,
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
    tx.commit()?;
    let seed_ms = seed_started.elapsed().as_secs_f64() * 1000.;
    drop(db);
    let mut host = Host::open(directory.path())?;
    let results = vec![
        samples("indexed transcript page (100 rows)", 1000, |_| {
            std::hint::black_box(host.messages(&session.id, None, 100)?);
            Ok(())
        })?,
        samples("workspace snapshot (50 sessions)", 1000, |_| {
            std::hint::black_box(host.snapshot()?);
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
        let host = Host::open(directory.path())?;
        std::hint::black_box(host.snapshot()?);
        Ok(())
    })?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"build":if cfg!(debug_assertions){"debug"}else{"release"},"durability":"SQLite WAL / synchronous=FULL","fixture":{"projects":5,"repositories":"10 metadata fixtures; no Git history benchmark","sessions":50,"messages":100000,"activity":100000,"source_logs":100000},"seed_ms":seed_ms,"results":results,"recovery":reopen,"excluded":"native UI, GPU, provider runtimes/inference, streaming, filesystem import and long soak"})
        )?
    );
    Ok(())
}
