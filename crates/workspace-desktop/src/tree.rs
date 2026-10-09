//! The sidebar's shape: each project's coordinator, its ticket workspaces, and their agents.
use std::collections::BTreeSet;
use workspace_core::*;

pub enum Row<'a> {
    Project(&'a Session),
    Ticket {
        ticket: &'a Ticket,
        label: String,
    },
    Agent {
        session: &'a Session,
        depth: usize,
        label: String,
    },
}

/// "repository · ticket", or the title alone when the repository is unknown.
pub fn ticket_label(snapshot: &Snapshot, ticket: &Ticket) -> String {
    match snapshot
        .repositories
        .iter()
        .find(|r| r.id == ticket.repository_id)
    {
        Some(repository) => format!("{} · {}", repository.name, ticket.title),
        None => ticket.title.clone(),
    }
}

/// One line for the cycle and one per round: its commit and each verifier's result.
pub fn verification_lines(ticket: &Ticket) -> Vec<String> {
    let Some(verification) = &ticket.verification else {
        return vec![];
    };
    let outcome = match verification.outcome {
        VerificationOutcome::Running => "running",
        VerificationOutcome::Passed => "passed",
        VerificationOutcome::Blocked => "blocked",
    };
    let mut lines = vec![format!(
        "Verification cycle {}: {outcome}, {} of {} rounds",
        verification.cycle,
        verification.rounds.len(),
        verification.max_rounds
    )];
    for round in &verification.rounds {
        let results = round
            .verifiers
            .iter()
            .map(|v| {
                format!(
                    "{} {}",
                    v.role.agent_label(Some(&v.focus)),
                    v.result.label()
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let commit: String = round.commit.chars().take(7).collect();
        lines.push(format!("Round {} at {commit}: {results}", round.round));
    }
    lines
}

/// Newest project first. Archived sessions appear only when `show_archived`; a ticket whose
/// agents are all archived, such as a closed one, goes with them.
pub fn rows<'a>(
    snapshot: &'a Snapshot,
    show_archived: bool,
    collapsed: &BTreeSet<String>,
) -> Vec<Row<'a>> {
    let shown = |s: &&Session| show_archived || !s.archived;
    let ticket_of = |id: &str| {
        snapshot
            .runtimes
            .iter()
            .find(|r| r.session_id == id)
            .and_then(|r| r.ticket_id.as_deref())
    };
    let focus_of = |id: &str| {
        snapshot
            .runtimes
            .iter()
            .find(|r| r.session_id == id)
            .and_then(|r| r.focus.as_deref())
    };
    let mut rows = vec![];
    for project in snapshot.projects.iter().rev() {
        let Some(root) = snapshot
            .sessions
            .iter()
            .filter(shown)
            .find(|s| s.project_id == project.id && s.parent_id.is_none())
        else {
            continue;
        };
        rows.push(Row::Project(root));
        if collapsed.contains(&project.id) {
            continue;
        }
        for ticket in snapshot
            .tickets
            .iter()
            .filter(|t| t.coordinator_id == root.id)
        {
            let on_ticket = |s: &&Session| ticket_of(&s.id) == Some(ticket.id.as_str());
            let agents: Vec<&Session> = snapshot
                .sessions
                .iter()
                .filter(shown)
                .filter(on_ticket)
                .collect();
            if agents.is_empty() && snapshot.sessions.iter().any(|s| on_ticket(&s)) {
                continue;
            }
            rows.push(Row::Ticket {
                ticket,
                label: ticket_label(snapshot, ticket),
            });
            for session in agents {
                rows.push(Row::Agent {
                    session,
                    depth: 2,
                    // The ticket row above already names the work.
                    label: session.role.agent_label(focus_of(&session.id)),
                });
            }
        }
        // Older stores can contain workers created before ticket ownership existed, and
        // retired repository coordinators stay readable from the archive.
        for session in snapshot
            .sessions
            .iter()
            .filter(shown)
            .filter(|s| s.parent_id.as_ref() == Some(&root.id) && ticket_of(&s.id).is_none())
        {
            let label = if session.role == Role::TaskOrchestrator {
                format!("{} (retired)", session.name)
            } else {
                session.name.clone()
            };
            rows.push(Row::Agent {
                session,
                depth: 1,
                label,
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use workspace_host::Host;

    fn git(path: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(path)
            .args(["-c", "user.name=T", "-c", "user.email=t@example.invalid"])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    /// A temporary store with a project using `sagechat-sms` and one ticket in it.
    fn store() -> (tempfile::TempDir, Host, Session, Ticket) {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("sagechat-sms");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "Base"]);
        let mut host = Host::open(directory.path().join("home")).unwrap();
        host.set_workspaces_dir(directory.path().join("workspaces").to_str().unwrap())
            .unwrap();
        let project = host.create_project("Calls").unwrap();
        let root = host.sessions().unwrap().remove(0);
        let repository = host
            .attach_repository(&project.id, repo.to_str().unwrap(), "HEAD")
            .unwrap();
        let ticket = host
            .create_ticket(&root.id, &repository.id, "Fix inbound calls", "Fix them")
            .unwrap();
        (directory, host, root, ticket)
    }

    fn shape(rows: &[Row]) -> Vec<(usize, String)> {
        rows.iter()
            .map(|row| match row {
                Row::Project(s) => (0, s.name.clone()),
                Row::Ticket { label, .. } => (1, label.clone()),
                Row::Agent { depth, label, .. } => (*depth, label.clone()),
            })
            .collect()
    }

    #[test]
    fn a_project_shows_its_coordinator_then_ticket_workspaces_then_their_agents() {
        let (_directory, mut host, _root, ticket) = store();
        host.assign_ticket(&ticket.id, Role::Implementer, Provider::Codex, "Fix", None)
            .unwrap();
        for focus in ["Correctness", "Style"] {
            host.assign_ticket(
                &ticket.id,
                Role::Reviewer,
                Provider::Codex,
                "Review",
                Some(focus),
            )
            .unwrap();
        }
        let snapshot = host.snapshot().unwrap();

        let rows = rows(&snapshot, false, &BTreeSet::new());

        assert_eq!(
            shape(&rows),
            [
                (0, "Calls".to_owned()),
                (1, "sagechat-sms · Fix inbound calls".into()),
                (2, "Implementer".into()),
                (2, "Reviewer · Correctness".into()),
                (2, "Reviewer · Style".into()),
            ]
        );
        let Row::Ticket { ticket: shown, .. } = &rows[1] else {
            panic!("ticket row")
        };
        assert_eq!(
            shown.state, "assigned",
            "the row carries the ticket's state"
        );
    }

    #[test]
    fn a_closed_ticket_hides_with_its_agents_and_a_collapsed_project_hides_everything() {
        let (_directory, mut host, root, ticket) = store();
        host.assign_ticket(&ticket.id, Role::Reviewer, Provider::Codex, "Review", None)
            .unwrap();
        host.close_ticket(&ticket.id).unwrap();
        let snapshot = host.snapshot().unwrap();

        assert_eq!(
            shape(&rows(&snapshot, false, &BTreeSet::new())),
            [(0, "Calls".to_owned())]
        );
        assert_eq!(shape(&rows(&snapshot, true, &BTreeSet::new())).len(), 3);
        let collapsed = BTreeSet::from([root.project_id.clone()]);
        assert_eq!(rows(&snapshot, true, &collapsed).len(), 1);
    }

    #[test]
    fn the_tickets_view_lists_each_round_with_its_commit_and_results() {
        let (_directory, mut host, _root, ticket) = store();
        host.set_verification(VerificationSettings {
            verifiers: ["Claude", "Codex"]
                .map(|focus| VerifierConfig {
                    role: Role::Tester,
                    focus: focus.into(),
                    instruction: None,
                    provider: None,
                })
                .to_vec(),
            max_rounds: 2,
            max_cycles: 2,
        })
        .unwrap();
        let implementer = host
            .assign_ticket(&ticket.id, Role::Implementer, Provider::Codex, "Fix", None)
            .unwrap();
        host.agent_tool(
            &implementer.id,
            "report",
            serde_json::json!({"message_id":"ready","kind":"ready_for_testing","body":"Done"}),
        )
        .unwrap();
        assert!(verification_lines(&host.ticket(&ticket.id).unwrap()).is_empty());
        let profile = host
            .model_selection()
            .unwrap()
            .prefill(Role::Tester)
            .unwrap();
        let choices = ["Claude", "Codex"].map(|focus| VerifierChoice {
            focus: focus.into(),
            profile: profile.clone(),
            reason: "Routine check".into(),
        });
        let verifying = host.verify_ticket(&ticket.id, &choices).unwrap();
        let round = &verifying.verification.as_ref().unwrap().rounds[0];
        // The verifier's turn takes its round input before it reports.
        host.advance_receipt(&round.verifiers[0].message_id, Receipt::Delivered)
            .unwrap();
        host.agent_tool(
            &round.verifiers[0].session_id,
            "report",
            serde_json::json!({"message_id":"ok","kind":"passed","body":"Green"}),
        )
        .unwrap();

        let lines = verification_lines(&host.ticket(&ticket.id).unwrap());

        let commit: String = round.commit.chars().take(7).collect();
        assert_eq!(
            lines,
            [
                "Verification cycle 1: running, 1 of 2 rounds".to_owned(),
                format!("Round 1 at {commit}: Tester · Claude passed, Tester · Codex pending"),
            ]
        );
    }

    #[test]
    fn retired_coordinators_and_older_workers_sit_under_the_coordinator() {
        let (directory, host, root, ticket) = store();
        drop(host);
        // A repository coordinator as the previous release stored it, then migrated on open.
        let retired = Session {
            id: "retired".into(),
            project_id: root.project_id.clone(),
            parent_id: Some(root.id.clone()),
            repository_id: Some(ticket.repository_id.clone()),
            name: "sagechat-sms".into(),
            role: Role::TaskOrchestrator,
            provider: Provider::Codex,
            status: Status::Ready,
            archived: false,
        };
        let db =
            rusqlite::Connection::open(directory.path().join("home/workspace.sqlite3")).unwrap();
        db.execute_batch(&format!(
            "INSERT INTO sessions VALUES ('retired','{}','{}','task_orchestrator','{}'); \
             UPDATE tickets SET coordinator_id='retired',data=json_set(data,'$.coordinator_id','retired'); \
             ALTER TABLE messages DROP COLUMN quiet; PRAGMA user_version = 5;",
            root.project_id,
            root.id,
            serde_json::to_string(&retired).unwrap()
        ))
        .unwrap();
        drop(db);
        let mut host = Host::open(directory.path().join("home")).unwrap();
        let older = host
            .create_session(
                &root.project_id,
                &root.id,
                None,
                "Older worker",
                Role::Implementer,
                Provider::Codex,
            )
            .unwrap();
        let snapshot = host.snapshot().unwrap();

        assert_eq!(
            shape(&rows(&snapshot, false, &BTreeSet::new())),
            [
                (0, "Calls".to_owned()),
                (1, "sagechat-sms · Fix inbound calls".into()),
                (1, older.name.clone()),
            ]
        );
        let archive = shape(&rows(&snapshot, true, &BTreeSet::new()));
        assert!(
            archive.contains(&(1, "sagechat-sms (retired)".to_owned())),
            "{archive:?}"
        );
    }
}
