//! Fills the many-agents fixture's Inbox, for the desktop load scenario
//! (`docs/performance/desktop-under-load.md`): `inbox_questions <dir> <per coordinator>` has
//! each coordinator of the store `many_agents seed <dir>` built ask the human that many
//! questions. Run it before the host starts; it opens the store directly.
use anyhow::{Context, Result};
use serde_json::json;
use std::path::Path;
use workspace_core::Role;
use workspace_host::Host;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, per_coordinator] = &args[..] else {
        anyhow::bail!("Usage: inbox_questions <fixture folder> <questions per coordinator>");
    };
    let per_coordinator: usize = per_coordinator
        .parse()
        .context("questions per coordinator")?;
    let mut host = Host::open(Path::new(dir).join("home"))?;
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
