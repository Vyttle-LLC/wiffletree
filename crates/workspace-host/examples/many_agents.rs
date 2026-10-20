//! The many-agents fixture: a store whose 40 agents keep working, for measuring what the desktop
//! does with a busy host. Never opens a user's project store.
//!
//! `many_agents seed <dir>` builds the store in a new folder: five live projects, each with a
//! coordinator, a small Git repository and seven tickets, each with a worker, some history and a
//! message waiting for it. Open it with the desktop and `busy_provider.py` as its Claude,
//! and its agents start and never stop (see `docs/performance/refresh-cost.md`).
//!
//! `many_agents probe <dir> [seconds]` joins that store's running host as a script client, and
//! reports the events it sends per second and how long a snapshot takes over `host.sock`.
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use workspace_core::*;
use workspace_host::Host;

const PROJECTS: usize = 5;
const WORKERS: usize = 7;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["seed", dir] => seed(Path::new(dir)),
        ["probe", dir] => probe(Path::new(dir), 30),
        ["probe", dir, seconds] => probe(Path::new(dir), seconds.parse()?),
        _ => anyhow::bail!("Usage: many_agents seed <new folder> | probe <folder> [seconds]"),
    }
}

fn git(path: &Path, args: &[&str]) -> Result<()> {
    let status = std::process::Command::new("git")
        .current_dir(path)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(args)
        .output()?
        .status;
    ensure!(
        status.success(),
        "git {args:?} failed in {}",
        path.display()
    );
    Ok(())
}

fn seed(dir: &Path) -> Result<()> {
    ensure!(
        !dir.exists() || dir.read_dir()?.next().is_none(),
        "{} is not empty; the fixture needs a new folder",
        dir.display()
    );
    let mut host = Host::open_set_up(dir.join("home"))?;
    host.set_workspaces_dir(dir.join("workspaces").to_str().context("folder name")?)?;
    for p in 0..PROJECTS {
        // No origin, so nothing in the fixture reaches GitHub.
        let repository = dir.join(format!("repos/fixture-{p}"));
        std::fs::create_dir_all(&repository)?;
        git(&repository, &["init", "-q"])?;
        git(
            &repository,
            &["commit", "-q", "--allow-empty", "-m", "Base"],
        )?;
        let project = host.create_project(&format!("Busy project {p}"))?;
        let root = (host.sessions()?.into_iter())
            .find(|s| s.project_id == project.id && s.parent_id.is_none())
            .context("coordinator")?;
        let repository =
            host.attach_repository(&project.id, repository.to_str().unwrap(), "HEAD")?;
        for w in 0..WORKERS {
            let title = format!("Fixture ticket {p}.{w}: a title as long as a real one");
            let ticket = host.create_ticket(&root.id, &repository.id, &title, &"b".repeat(2000))?;
            let instruction = "Work through the ticket";
            let worker = host.assign_ticket(
                &ticket.id,
                Role::Implementer,
                Provider::Claude,
                instruction,
                None,
            )?;
            let profile = ModelProfile {
                provider: Provider::Claude,
                model: "opus".into(),
                effort: "high".into(),
            };
            host.configure_session(&worker.id, profile)?;
            for m in 0..20 {
                let body = format!("Earlier instruction {m}: {}", "x".repeat(200));
                host.send(&new_id(), Some(&root.id), &worker.id, &body)?;
            }
        }
        host.set_live(&project.id, true)?;
    }
    let snapshot = host.snapshot()?;
    println!(
        "Seeded {} sessions in {} projects; snapshot {} bytes",
        snapshot.sessions.len(),
        snapshot.projects.len(),
        serde_json::to_vec(&snapshot)?.len()
    );
    Ok(())
}

/// The socket of a host already running for a data folder, joined without the app's credential.
struct Running(PathBuf);
impl Launch for Running {
    fn endpoint(&mut self, _: &AtomicBool) -> std::io::Result<Endpoint> {
        Ok(Endpoint {
            socket: self.0.join(SOCKET_FILE),
            credential: None,
        })
    }
}

fn percentile(sorted: &[f64], p: usize) -> f64 {
    sorted[(sorted.len() - 1) * p / 100]
}

fn probe(dir: &Path, seconds: u64) -> Result<()> {
    let counts = Arc::new(Mutex::new([0usize; 5]));
    let steps = Arc::new(AtomicUsize::new(0));
    let (counted, stepped) = (counts.clone(), steps.clone());
    let client = Client {
        name: "many-agents probe".into(),
        kind: "script".into(),
    };
    let on_event = Box::new(move |kind: &EventKind| {
        let slot = match kind {
            EventKind::State => 0,
            EventKind::Steps { sessions } => {
                stepped.fetch_add(sessions.len(), Ordering::Relaxed);
                1
            }
            EventKind::Progress => 2,
            EventKind::Usage => 3,
            EventKind::Resync => 4,
        };
        counted.lock().unwrap()[slot] += 1;
    });
    let home = dir.join("home");
    let client = HostClient::connect(Box::new(Running(home)), client, on_event)?;
    let started = Instant::now();
    let (mut times, mut bytes, mut working) = (vec![], 0, vec![]);
    while started.elapsed() < Duration::from_secs(seconds) {
        let asked = Instant::now();
        let reply = client
            .request(Command::Snapshot)
            .map_err(anyhow::Error::msg)?;
        let value = reply.recv_blocking()?.map_err(anyhow::Error::msg)?;
        times.push(asked.elapsed().as_secs_f64() * 1000.);
        bytes = serde_json::to_vec(&value)?.len();
        let snapshot: Snapshot = serde_json::from_value(value)?;
        working.push(
            (snapshot.sessions.iter())
                .filter(|s| s.status == Status::Working)
                .count(),
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    let elapsed = started.elapsed().as_secs_f64();
    client.stop();
    times.sort_by(f64::total_cmp);
    let counts = *counts.lock().unwrap();
    let rate = |n: usize| (n as f64 / elapsed * 10.).round() / 10.;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "seconds": elapsed,
            "events_per_second": {
                "state": rate(counts[0]),
                "steps": rate(counts[1]),
                "sessions_in_steps_events": rate(steps.load(Ordering::Relaxed)),
                "progress": rate(counts[2]),
                "usage": rate(counts[3]),
                "resync": counts[4],
            },
            "agents_working_mean": working.iter().sum::<usize>() as f64 / working.len() as f64,
            "snapshot_bytes": bytes,
            "snapshot_over_socket_ms": {
                "samples": times.len(),
                "p50": percentile(&times, 50),
                "p95": percentile(&times, 95),
                "max": times.last(),
            },
        }))?
    );
    Ok(())
}
