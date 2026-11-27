//! The Inbox of the many-agents fixture, for the desktop load scenario
//! (`docs/performance/desktop-under-load.md`). Opens the store directly, so run it while its
//! host is stopped.
//!
//! `inbox_questions ask <dir> <per coordinator>` has each coordinator of the store
//! `many_agents seed <dir>` built ask the human that many questions.
//!
//! `inbox_questions check <dir>` prints, for each project, how many questions are still open and
//! what the human sent its coordinator, so the scenario can tell its clicks took effect.
use anyhow::{Context, Result};
use serde_json::json;
use std::path::Path;
use workspace_core::Role;
use workspace_host::Host;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["ask", dir, per_coordinator] => ask(dir, per_coordinator.parse()?),
        ["check", dir] => check(dir),
        _ => anyhow::bail!(
            "Usage: inbox_questions ask <fixture folder> <questions per coordinator> | check <fixture folder>"
        ),
    }
}

fn open(dir: &str) -> Result<Host> {
    Host::open(Path::new(dir).join("home")).context("open the fixture's store")
}

fn ask(dir: &str, per_coordinator: usize) -> Result<()> {
    let mut host = open(dir)?;
    let coordinators: Vec<_> = (host.sessions()?.into_iter())
        .filter(|s| s.role == Role::ProjectOrchestrator)
        .collect();
    for coordinator in &coordinators {
        for q in 0..per_coordinator {
            let question = format!(
                "Question {q} from {}: should the fixture ship `src/main.rs` as it is now?",
                coordinator.name
            );
            let ask = json!({"request_id": format!("load-{q}"), "question": question});
            host.agent_tool(&coordinator.id, "ask_user", ask)?;
        }
    }
    println!("Asked {} questions", coordinators.len() * per_coordinator);
    Ok(())
}

fn check(dir: &str) -> Result<()> {
    let host = open(dir)?;
    let mut projects = serde_json::Map::new();
    for coordinator in
        (host.sessions()?.into_iter()).filter(|s| s.role == Role::ProjectOrchestrator)
    {
        let (mut sent, mut before) = (vec![], None);
        loop {
            let page = host.messages(&coordinator.id, before, workspace_core::MAX_PAGE)?;
            let Some(first) = page.first() else { break };
            before = Some(first.sequence);
            // Pages come newest first, each in order.
            let human = page.into_iter().filter(|m| m.sender.is_none());
            sent.splice(0..0, human.map(|m| m.body));
        }
        let open = host.open_attention(&coordinator.project_id)?.len();
        projects.insert(
            coordinator.name,
            json!({"open_questions": open, "human_sent": sent}),
        );
    }
    println!("{}", serde_json::Value::Object(projects));
    Ok(())
}
