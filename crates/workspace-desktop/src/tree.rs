//! The sidebar's shape: each project's coordinator, its ticket workspaces, and their agents.
use crate::ui::age;
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

fn cycle_line(verification: &Verification) -> String {
    let outcome = match verification.outcome {
        VerificationOutcome::Running => "running",
        VerificationOutcome::Passed => "passed",
        VerificationOutcome::Blocked => "blocked",
    };
    format!(
        "Verification cycle {}: {outcome}, {} of {} rounds",
        verification.cycle,
        verification.rounds.len(),
        verification.max_rounds
    )
}

/// One line for the latest cycle and one per round, with its commit and each verifier's result,
/// then one line per earlier cycle, newest first.
pub fn verification_lines(ticket: &Ticket) -> Vec<String> {
    let Some(verification) = &ticket.verification else {
        return vec![];
    };
    let mut lines = vec![cycle_line(verification)];
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
    lines.extend(ticket.previous_cycles.iter().rev().map(cycle_line));
    lines
}

/// A ticket accepted with a waiver: its row's waived ring says this beside the label.
pub fn waiver_note(ticket: &Ticket) -> Option<String> {
    ticket
        .waiver
        .as_ref()
        .filter(|_| ticket.state == "accepted")?;
    let open = ticket
        .ledger
        .iter()
        .filter(|e| e.status == EntryStatus::Open)
        .count();
    let plural = if open == 1 { "" } else { "s" };
    Some(format!(
        "Accepted with waiver · {open} open finding{plural}"
    ))
}

/// How a part of a ticket's PR line is colored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Muted,
    Good,
    Bad,
    Merged,
}
#[derive(Debug, PartialEq)]
pub struct PrPart {
    pub text: String,
    pub tone: Tone,
}

/// A ticket row's PR line, such as "#142 · checks ✓ · 1 thread · behind", in parts. Only the
/// parts that apply appear; a finished PR shows `merged` or `closed` instead of its checks.
pub fn pr_line(pr: &PullRequest) -> Vec<PrPart> {
    let part = |text: &str, tone| PrPart {
        text: text.into(),
        tone,
    };
    let mut parts = vec![part(&format!("#{}", pr.number), Tone::Muted)];
    match pr.state {
        PrState::Merged => parts.push(part("merged", Tone::Merged)),
        PrState::Closed => parts.push(part("closed", Tone::Muted)),
        PrState::Open => {
            match pr.checks {
                CheckState::Success => parts.push(part("checks ✓", Tone::Good)),
                CheckState::Failure => parts.push(part("checks ✗", Tone::Bad)),
                CheckState::Pending => parts.push(part("checks …", Tone::Muted)),
                CheckState::None => {}
            }
            if pr.unresolved_threads > 0 {
                let plural = if pr.unresolved_threads == 1 { "" } else { "s" };
                parts.push(part(
                    &format!("{} thread{plural}", pr.unresolved_threads),
                    Tone::Muted,
                ));
            }
            let merge_word = match pr.merge_state {
                _ if pr.draft => Some(("draft", Tone::Muted)),
                MergeState::Behind => Some(("behind", Tone::Muted)),
                MergeState::Dirty => Some(("conflict", Tone::Bad)),
                MergeState::Blocked => Some(("blocked", Tone::Muted)),
                _ => None,
            };
            if let Some((word, tone)) = merge_word {
                parts.push(part(word, tone));
            }
        }
    }
    parts
}

/// The PR in full words, for the row's tooltip and the ticket's card.
pub fn pr_sentence(pr: &PullRequest) -> String {
    let mut parts = vec![match pr.state {
        PrState::Merged => match &pr.merge_commit {
            Some(commit) => format!(
                "PR #{} merged into {} as {}",
                pr.number,
                pr.base,
                short_sha(commit)
            ),
            None => format!("PR #{} merged into {}", pr.number, pr.base),
        },
        PrState::Closed => format!("PR #{} closed without merging", pr.number),
        PrState::Open if pr.draft => format!("PR #{} draft into {}", pr.number, pr.base),
        PrState::Open => format!("PR #{} open into {}", pr.number, pr.base),
    }];
    if pr.state == PrState::Open {
        match pr.checks {
            CheckState::Success => parts.push("checks passed".into()),
            CheckState::Failure => parts.push("checks failed".into()),
            CheckState::Pending => parts.push("checks running".into()),
            CheckState::None => {}
        }
        if pr.unresolved_threads > 0 {
            let plural = if pr.unresolved_threads == 1 { "" } else { "s" };
            parts.push(format!(
                "{} unresolved thread{plural}",
                pr.unresolved_threads
            ));
        }
        match pr.merge_state {
            MergeState::Behind => parts.push(format!("behind {}", pr.base)),
            MergeState::Dirty => parts.push(format!("conflicts with {}", pr.base)),
            MergeState::Blocked => parts.push("blocked".into()),
            _ => {}
        }
    }
    parts.join(" · ")
}

/// When the ticket's repository last had its PRs checked, as "Checked 3m ago".
pub fn pr_checked(snapshot: &Snapshot, ticket: &Ticket) -> Option<String> {
    let at = snapshot
        .pull_request_checks
        .iter()
        .find(|c| c.repository_id == ticket.repository_id)?
        .checked_at?;
    Some(format!("Checked {}", age(at).to_lowercase()))
}

/// One decision-ledger entry as the Tickets panel lists it.
#[derive(Debug, PartialEq)]
pub struct LedgerRow {
    pub id: String,
    pub summary: String,
    /// Its location and source: "src/calls.rs:42 · Regressions · cycle 2, round 1".
    pub place: String,
    pub status: EntryStatus,
    /// Its severity, then the coordinator's reason once decided.
    pub note: String,
}

pub fn ledger_rows(ticket: &Ticket) -> Vec<LedgerRow> {
    ticket
        .ledger
        .iter()
        .map(|entry| {
            let severity = match entry.finding.severity {
                Severity::Blocking => "Blocking",
                Severity::NonBlocking => "Non-blocking",
                Severity::PreExisting => "Pre-existing",
            };
            LedgerRow {
                id: entry.id.clone(),
                summary: entry.finding.summary.clone(),
                place: format!(
                    "{} · {} · cycle {}, round {}",
                    entry.finding.location, entry.focus, entry.cycle, entry.round
                ),
                status: entry.status,
                note: match &entry.reason {
                    Some(reason) => format!("{severity} · {reason}"),
                    None => severity.into(),
                },
            }
        })
        .collect()
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

    fn entry(
        id: &str,
        severity: Severity,
        status: EntryStatus,
        reason: Option<&str>,
    ) -> LedgerEntry {
        LedgerEntry {
            id: id.into(),
            finding: Finding {
                severity,
                location: "live.rs:398".into(),
                summary: format!("Summary {id}"),
                trigger: "No base ref".into(),
                evidence: "Test fails".into(),
            },
            focus: "Regressions".into(),
            cycle: 2,
            round: 1,
            status,
            reason: reason.map(Into::into),
        }
    }

    fn verification(cycle: u32, outcome: VerificationOutcome, rounds: u32) -> Verification {
        Verification {
            cycle,
            max_rounds: 3,
            outcome,
            rounds: (1..=rounds)
                .map(|round| VerificationRound {
                    round,
                    commit: format!("{round}abcdef0123"),
                    verifiers: vec![],
                })
                .collect(),
        }
    }

    #[test]
    fn the_tickets_view_shows_every_cycle_and_the_ledger_with_its_decisions() {
        let (_directory, _host, _root, mut ticket) = store();
        ticket.verification = Some(verification(2, VerificationOutcome::Blocked, 3));
        ticket.previous_cycles = vec![verification(1, VerificationOutcome::Passed, 2)];
        ticket.ledger = vec![
            entry("F1", Severity::Blocking, EntryStatus::Fixed, None),
            entry(
                "F2",
                Severity::NonBlocking,
                EntryStatus::WontFix,
                Some("Matches the naming"),
            ),
            entry("F3", Severity::Blocking, EntryStatus::Open, None),
            entry(
                "F4",
                Severity::PreExisting,
                EntryStatus::FollowUp,
                Some("Ticket later"),
            ),
            entry("F5", Severity::NonBlocking, EntryStatus::Untriaged, None),
        ];

        let lines = verification_lines(&ticket);
        let rows = ledger_rows(&ticket);

        assert_eq!(lines[0], "Verification cycle 2: blocked, 3 of 3 rounds");
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[4], "Verification cycle 1: passed, 2 of 3 rounds");
        assert_eq!(rows.len(), 5);
        assert_eq!(
            rows[1],
            LedgerRow {
                id: "F2".into(),
                summary: "Summary F2".into(),
                place: "live.rs:398 · Regressions · cycle 2, round 1".into(),
                status: EntryStatus::WontFix,
                note: "Non-blocking · Matches the naming".into(),
            }
        );
        assert_eq!(rows[2].note, "Blocking");
        assert_eq!(rows[3].note, "Pre-existing · Ticket later");
    }

    #[test]
    fn only_an_accepted_ticket_with_a_waiver_gets_the_waived_ring() {
        let (_directory, _host, _root, mut ticket) = store();
        ticket.ledger = vec![entry("F3", Severity::Blocking, EntryStatus::Open, None)];
        for state in ["passed", "blocked", "accepted"] {
            ticket.state = state.into();
            assert_eq!(waiver_note(&ticket), None, "{state}");
        }
        ticket.waiver = Some("F3 needs a fetched base".into());
        assert_eq!(
            waiver_note(&ticket).as_deref(),
            Some("Accepted with waiver · 1 open finding")
        );
        ticket.state = "blocked".into();
        assert_eq!(waiver_note(&ticket), None);
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

    fn pr(state: PrState, checks: CheckState, merge_state: MergeState) -> PullRequest {
        PullRequest {
            number: 142,
            url: "https://github.com/o/r/pull/142".into(),
            state,
            draft: false,
            base: "main".into(),
            head: "3daf610aa".into(),
            merge_state,
            checks,
            unresolved_threads: 0,
            comments: CommentCursors::default(),
            merge_commit: (state == PrState::Merged).then(|| "be5f484cc".into()),
        }
    }
    fn line(pr: &PullRequest) -> String {
        pr_line(pr)
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join(" · ")
    }
    fn tones(pr: &PullRequest) -> Vec<Tone> {
        pr_line(pr).iter().map(|p| p.tone).collect()
    }

    #[test]
    fn the_pr_line_shows_only_the_parts_that_apply() {
        use {CheckState as C, MergeState as M, PrState as S};
        let mut behind = pr(S::Open, C::Success, M::Behind);
        behind.unresolved_threads = 1;
        assert_eq!(line(&behind), "#142 · checks ✓ · 1 thread · behind");
        assert_eq!(
            tones(&behind),
            [Tone::Muted, Tone::Good, Tone::Muted, Tone::Muted]
        );
        assert_eq!(
            pr_sentence(&behind),
            "PR #142 open into main · checks passed · 1 unresolved thread · behind main"
        );
        let mut running = pr(S::Open, C::Pending, M::Clean);
        running.unresolved_threads = 2;
        assert_eq!(line(&running), "#142 · checks … · 2 threads");
        let failed = pr(S::Open, C::Failure, M::Unstable);
        assert_eq!(line(&failed), "#142 · checks ✗");
        assert_eq!(tones(&failed), [Tone::Muted, Tone::Bad]);
        assert_eq!(line(&pr(S::Open, C::Success, M::Clean)), "#142 · checks ✓");
        let conflict = pr(S::Open, C::Success, M::Dirty);
        assert_eq!(line(&conflict), "#142 · checks ✓ · conflict");
        assert_eq!(tones(&conflict)[2], Tone::Bad);
        assert_eq!(
            pr_sentence(&conflict),
            "PR #142 open into main · checks passed · conflicts with main"
        );
        let mut draft = pr(S::Open, C::None, M::Blocked);
        draft.draft = true;
        assert_eq!(line(&draft), "#142 · draft");
        assert_eq!(pr_sentence(&draft), "PR #142 draft into main · blocked");
        assert_eq!(line(&pr(S::Open, C::None, M::Blocked)), "#142 · blocked");
        let mut merged = pr(S::Merged, C::Success, M::Unknown);
        merged.number = 141;
        merged.unresolved_threads = 1;
        assert_eq!(line(&merged), "#141 · merged");
        assert_eq!(tones(&merged), [Tone::Muted, Tone::Merged]);
        assert_eq!(pr_sentence(&merged), "PR #141 merged into main as be5f484");
        let closed = pr(S::Closed, C::Failure, M::Dirty);
        assert_eq!(line(&closed), "#142 · closed");
        assert_eq!(pr_sentence(&closed), "PR #142 closed without merging");
    }
}
