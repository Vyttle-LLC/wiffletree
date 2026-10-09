//! Ticket verification through the host's tools: one call, pinned commits, read-only
//! verifiers, failures back to the implementer and a capped number of rounds.
mod common;
use serde_json::{Value, json};
use std::path::Path;
use workspace_core::*;
use workspace_host::Host;

fn git(path: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(path)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn profile(provider: Provider, model: &str, effort: &str) -> ModelProfile {
    ModelProfile {
        provider,
        model: model.into(),
        effort: effort.into(),
    }
}

/// Both subscriptions, with testers and implementers on either provider.
fn both_providers(host: &mut Host) {
    let access = |models: &[(&str, &str)]| ProviderAccess {
        enabled: true,
        models: models
            .iter()
            .map(|(model, effort)| AllowedModel {
                model: (*model).into(),
                effort: (*effort).into(),
            })
            .collect(),
    };
    let mut selection = host.model_selection().unwrap();
    selection.providers = [
        (
            Provider::Claude,
            access(&[("opus", "high"), ("opus", "medium"), ("sonnet", "high")]),
        ),
        (
            Provider::Codex,
            access(&[("gpt-6.1-sol", "high"), ("gpt-6.1-sol", "medium")]),
        ),
    ]
    .into();
    selection.role_providers = [
        (Role::ProjectOrchestrator, [Provider::Claude].into()),
        (
            Role::Implementer,
            [Provider::Claude, Provider::Codex].into(),
        ),
        (Role::Tester, [Provider::Claude, Provider::Codex].into()),
    ]
    .into();
    host.set_model_selection(&selection).unwrap();
}

fn verifier(role: Role, focus: &str, provider: Option<Provider>) -> VerifierConfig {
    VerifierConfig {
        role,
        focus: focus.into(),
        instruction: Some(format!("Check {focus}")),
        provider,
    }
}

/// The SH-1171 setting: a Claude tester, a Codex tester and a style reviewer.
fn three_verifiers() -> Vec<VerifierConfig> {
    vec![
        verifier(Role::Tester, "Claude", Some(Provider::Claude)),
        verifier(Role::Tester, "Codex", Some(Provider::Codex)),
        verifier(Role::Reviewer, "Style", None),
    ]
}

/// What a turn does first: takes every queued message as its input.
fn take_input(host: &mut Host, session: &str) {
    for message in host.messages(session, None, 100).unwrap() {
        if message.receipt == Receipt::Queued {
            host.advance_receipt(&message.id, Receipt::Delivered)
                .unwrap();
        }
    }
}

/// An evidenced blocking finding summarized as `summary`.
fn blocking(summary: &str) -> Value {
    json!({"severity":"blocking","location":"src/calls.rs:42","summary":summary,
        "trigger":"A call arrives while the line is busy","evidence":"calls::busy_line fails"})
}

struct Fixture {
    _directory: tempfile::TempDir,
    host: Host,
    coordinator: Session,
    implementer: Session,
    ticket: Ticket,
}
impl Fixture {
    /// A ticket whose implementer committed work and reported ready, with `verifiers` and `cap`.
    fn ready(verifiers: Vec<VerifierConfig>, cap: u32) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let repository = directory.path().join("web");
        std::fs::create_dir_all(&repository).unwrap();
        git(&repository, &["init", "-q", "-b", "main"]);
        git(
            &repository,
            &["commit", "-q", "--allow-empty", "-m", "Base"],
        );
        let mut host = Host::open(directory.path().join("home")).unwrap();
        host.set_workspaces_dir(directory.path().join("workspaces").to_str().unwrap())
            .unwrap();
        both_providers(&mut host);
        host.set_verification(VerificationSettings {
            verifiers,
            max_rounds: cap,
            max_cycles: 2,
        })
        .unwrap();
        let project = host.create_project("Calls").unwrap();
        let coordinator = host.sessions().unwrap().remove(0);
        let repository = host
            .attach_repository(&project.id, repository.to_str().unwrap(), "main")
            .unwrap();
        let ticket = host
            .create_ticket(
                &coordinator.id,
                &repository.id,
                "Fix calls",
                "Fix inbound calls",
            )
            .unwrap();
        let implementer = host
            .assign_ticket(&ticket.id, Role::Implementer, Provider::Codex, "Fix", None)
            .unwrap();
        let mut fixture = Self {
            _directory: directory,
            host,
            coordinator,
            implementer,
            ticket,
        };
        let implementer = fixture.implementer.id.clone();
        fixture.handled(&implementer);
        fixture.commit("fix.txt", "one");
        fixture
            .report(&implementer, "ready", "ready_for_testing")
            .unwrap();
        fixture
    }
    fn worktree(&self) -> &Path {
        Path::new(&self.ticket.worktree)
    }
    fn commit(&mut self, file: &str, content: &str) -> String {
        std::fs::write(self.worktree().join(file), content).unwrap();
        git(self.worktree(), &["add", file]);
        git(self.worktree(), &["commit", "-q", "-m", content]);
        git(self.worktree(), &["rev-parse", "HEAD"])
    }
    /// Reports from within `session`'s turn, which first took its queued input.
    fn report(&mut self, session: &str, id: &str, kind: &str) -> anyhow::Result<Value> {
        take_input(&mut self.host, session);
        self.host.agent_tool(
            session,
            "report",
            json!({"message_id":id,"kind":kind,"body":format!("{kind} evidence from {session}")}),
        )
    }
    /// Reports `kind` with `findings` from within `session`'s turn.
    fn report_findings(
        &mut self,
        session: &str,
        id: &str,
        kind: &str,
        findings: Value,
    ) -> anyhow::Result<Value> {
        take_input(&mut self.host, session);
        self.host.agent_tool(
            session,
            "report",
            json!({"message_id":id,"kind":kind,"body":format!("{kind} evidence from {session}"),"findings":findings}),
        )
    }
    /// Fails with one evidenced blocking finding, summarized as `id`.
    fn fail(&mut self, session: &str, id: &str) -> anyhow::Result<Value> {
        self.report_findings(session, id, "failed", json!([blocking(id)]))
    }
    fn verify(&mut self) -> anyhow::Result<Ticket> {
        let id = self.ticket.id.clone();
        let coordinator = self.coordinator.id.clone();
        Ok(serde_json::from_value(self.host.agent_tool(
            &coordinator,
            "verify_ticket",
            common::verify_args(&self.host, &id),
        )?)?)
    }
    fn current(&self) -> Ticket {
        self.host.ticket(&self.ticket.id).unwrap()
    }
    /// Each verifier's session id, by focus.
    fn verifier(&self, focus: &str) -> String {
        self.current()
            .verification
            .unwrap()
            .rounds
            .iter()
            .flat_map(|r| r.verifiers.clone())
            .find(|v| v.focus == focus)
            .unwrap()
            .session_id
    }
    fn archived(&self, session: &str) -> bool {
        self.host.session(session).unwrap().archived
    }
    fn round(&self) -> VerificationRound {
        self.current()
            .verification
            .unwrap()
            .current_round()
            .unwrap()
            .clone()
    }
    /// Messages to `session` that would start its next turn: queued and not quiet.
    fn waking(&self, session: &str) -> Vec<Message> {
        let quiet = self.quiet_ids(session);
        self.host
            .messages(session, None, 100)
            .unwrap()
            .into_iter()
            .filter(|m| m.receipt == Receipt::Queued && !quiet.contains(&m.id))
            .collect()
    }
    fn quiet_ids(&self, session: &str) -> Vec<String> {
        let db = rusqlite::Connection::open(self.host.home.join("workspace.sqlite3")).unwrap();
        db.prepare("SELECT id FROM messages WHERE recipient=?1 AND quiet=1")
            .unwrap()
            .query_map([session], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }
    /// Marks everything queued for the coordinator as handled, as a turn would.
    fn coordinator_reads(&mut self) {
        let id = self.coordinator.id.clone();
        self.handled(&id);
    }
    fn handled(&mut self, session: &str) {
        for message in self.host.messages(session, None, 100).unwrap() {
            if message.receipt == Receipt::Queued {
                for receipt in [
                    Receipt::Delivered,
                    Receipt::Acknowledged,
                    Receipt::Completed,
                ] {
                    self.host.advance_receipt(&message.id, receipt).unwrap();
                }
            }
        }
    }
}

#[test]
fn one_call_starts_every_verifier_on_the_same_commit() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    let head = git(f.worktree(), &["rev-parse", "HEAD"]);

    let ticket = f.verify().unwrap();

    assert_eq!(ticket.state, "verifying");
    let verification = ticket.verification.unwrap();
    assert_eq!(verification.outcome, VerificationOutcome::Running);
    assert_eq!(verification.max_rounds, 2);
    let round = &verification.rounds[0];
    assert_eq!((round.round, round.commit.as_str()), (1, head.as_str()));
    assert_eq!(round.verifiers.len(), 3);
    for run in &round.verifiers {
        let session = f.host.session(&run.session_id).unwrap();
        assert_eq!(session.parent_id.as_ref(), Some(&f.coordinator.id));
        let runtime = f.host.session_runtime(&run.session_id).unwrap();
        assert_eq!(runtime.workdir.as_ref(), Some(&f.ticket.worktree));
        assert_eq!(runtime.focus.as_deref(), Some(run.focus.as_str()));
        let message = f.host.message(&run.message_id).unwrap();
        assert_eq!(message.recipient, run.session_id);
        assert_eq!(message.sender.as_ref(), Some(&f.coordinator.id));
        assert!(message.body.contains(&head), "{}", message.body);
        assert!(message.body.contains(&format!("Check {}", run.focus)));
    }
    let providers: Vec<_> = round
        .verifiers
        .iter()
        .map(|v| f.host.session(&v.session_id).unwrap().provider)
        .collect();
    assert_eq!(
        providers[..2],
        [Provider::Claude, Provider::Codex],
        "each tester runs on its configured provider"
    );
}

#[test]
fn verify_ticket_refuses_unready_dirty_running_and_oversized_cycles() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    let sessions = f.host.sessions().unwrap().len();

    std::fs::write(f.worktree().join("scratch.txt"), "draft").unwrap();
    let dirty = f.verify().unwrap_err().to_string();
    assert!(dirty.contains("uncommitted or untracked"), "{dirty}");
    std::fs::remove_file(f.worktree().join("scratch.txt")).unwrap();

    let mut project = f.host.project(&f.coordinator.project_id).unwrap();
    let db = rusqlite::Connection::open(f.host.home.join("workspace.sqlite3")).unwrap();
    let set_turn_limit = |project: &mut Project, limit: usize| {
        project.turn_limit = limit;
        db.execute(
            "UPDATE projects SET data=?2 WHERE id=?1",
            rusqlite::params![project.id, serde_json::to_string(project).unwrap()],
        )
        .unwrap();
    };
    set_turn_limit(&mut project, 3);
    let small = f.verify().unwrap_err().to_string();
    assert!(
        small.contains("3 verifiers") && small.contains("turn limit of 3"),
        "{small}"
    );
    set_turn_limit(&mut project, 20);
    let mut many = three_verifiers();
    for focus in ["A", "B", "C", "D"] {
        many.push(verifier(Role::Reviewer, focus, None));
    }
    f.host
        .set_verification(VerificationSettings {
            verifiers: many,
            max_rounds: 2,
            max_cycles: 2,
        })
        .unwrap();
    let host_wide = f.verify().unwrap_err().to_string();
    assert!(
        host_wide.contains("7 verifiers") && host_wide.contains("host-wide limit is 6"),
        "{host_wide}"
    );
    assert_eq!(
        f.host.sessions().unwrap().len(),
        sessions,
        "no verifier started"
    );
    assert_eq!(f.current().state, "ready_for_testing");

    set_turn_limit(&mut project, 4);
    f.host
        .set_verification(VerificationSettings {
            verifiers: three_verifiers(),
            max_rounds: 2,
            max_cycles: 2,
        })
        .unwrap();
    f.verify().unwrap();
    let running = f.verify().unwrap_err().to_string();
    assert!(running.contains("already running"), "{running}");
}

#[test]
fn an_unready_ticket_starts_no_verifier() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    let db = rusqlite::Connection::open(f.host.home.join("workspace.sqlite3")).unwrap();
    db.execute(
        "UPDATE tickets SET data=json_set(data,'$.state','assigned') WHERE id=?1",
        [&f.ticket.id],
    )
    .unwrap();
    let sessions = f.host.sessions().unwrap().len();

    let refused = f.verify().unwrap_err().to_string();

    assert!(refused.contains("ready_for_testing"), "{refused}");
    assert_eq!(f.host.sessions().unwrap().len(), sessions);
    assert!(f.current().verification.is_none());
}

#[test]
fn a_first_pass_neither_passes_the_ticket_nor_allows_acceptance() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    f.verify().unwrap();
    let claude = f.verifier("Claude");

    f.report(&claude, "pass", "passed").unwrap();

    assert_eq!(f.current().state, "verifying");
    let agents: Vec<Session> = f.host.sessions().unwrap();
    let refused = f
        .host
        .agent_tool(
            &f.coordinator.id,
            "accept_ticket",
            json!({"ticket_id":f.ticket.id}),
        )
        .unwrap_err()
        .to_string();
    assert!(refused.contains("cycle 1 is still running"), "{refused}");
    assert_eq!(f.current().state, "verifying");
    assert_eq!(f.host.sessions().unwrap(), agents);
    assert!(f.worktree().exists());
}

#[test]
fn everyone_passing_wakes_the_coordinator_once_with_each_verifier_and_the_commit() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    f.coordinator_reads();
    let ticket = f.verify().unwrap();
    let commit = ticket.verification.unwrap().rounds[0].commit.clone();
    for focus in ["Claude", "Codex", "Style"] {
        assert!(f.waking(&f.coordinator.id).is_empty());
        let session = f.verifier(focus);
        f.report(&session, "pass", "passed").unwrap();
    }

    let ticket = f.current();
    assert_eq!(ticket.state, "passed");
    assert_eq!(
        ticket.verification.as_ref().unwrap().outcome,
        VerificationOutcome::Passed
    );
    let waking = f.waking(&f.coordinator.id);
    assert_eq!(waking.len(), 1, "{waking:?}");
    for name in [
        "Tester · Claude",
        "Tester · Codex",
        "Reviewer · Style",
        &commit,
    ] {
        assert!(waking[0].body.contains(name), "{name}\n{}", waking[0].body);
    }
    assert_eq!(
        f.quiet_ids(&f.coordinator.id).len(),
        3,
        "the three verdicts ride along"
    );

    f.host
        .agent_tool(
            &f.coordinator.id,
            "accept_ticket",
            json!({"ticket_id":f.ticket.id}),
        )
        .unwrap();
    assert_eq!(f.current().state, "accepted");
}

#[test]
fn a_failure_goes_to_the_implementer_and_only_failed_verifiers_re_run() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    f.coordinator_reads();
    f.verify().unwrap();
    let (claude, codex, style) = (
        f.verifier("Claude"),
        f.verifier("Codex"),
        f.verifier("Style"),
    );
    let before = |f: &Fixture, session: &str| f.host.messages(session, None, 100).unwrap().len();
    let (claude_before, style_before) = (before(&f, &claude), before(&f, &style));
    f.report(&claude, "pass", "passed").unwrap();
    f.fail(&codex, "fail").unwrap();
    f.report(&style, "pass", "passed").unwrap();

    assert_eq!(f.current().state, "failed");
    assert!(
        f.waking(&f.coordinator.id).is_empty(),
        "the coordinator has no turn started by the reports"
    );
    let sent = f.waking(&f.implementer.id);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].id, format!("verification:{}:1:1", f.ticket.id));
    assert_eq!(sent[0].sender, None, "sent as Wiffletree");
    assert!(
        sent[0].body.contains("F1 · src/calls.rs:42 · fail"),
        "{}",
        sent[0].body
    );
    assert!(!sent[0].body.contains("evidence from"), "{}", sent[0].body);

    // Ready with unsaved work is refused and starts nothing.
    std::fs::write(f.worktree().join("fix.txt"), "two").unwrap();
    let refused = f
        .report(&f.implementer.id.clone(), "dirty", "ready_for_testing")
        .unwrap_err()
        .to_string();
    assert!(refused.contains("commit or discard"), "{refused}");
    assert_eq!(f.round().round, 1);

    let fixed = f.commit("fix.txt", "two");
    f.report(&f.implementer.id.clone(), "fixed", "ready_for_testing")
        .unwrap();

    let ticket = f.current();
    assert_eq!(ticket.state, "verifying");
    let round = f.round();
    assert_eq!((round.round, round.commit.as_str()), (2, fixed.as_str()));
    assert_eq!(
        round
            .verifiers
            .iter()
            .map(|v| v.session_id.as_str())
            .collect::<Vec<_>>(),
        [codex.as_str()],
        "the Codex tester re-runs in the same session"
    );
    assert_eq!(before(&f, &claude), claude_before);
    assert_eq!(before(&f, &style), style_before);
    assert!(f.waking(&f.coordinator.id).is_empty());

    f.report(&codex, "pass-2", "passed").unwrap();
    let ticket = f.current();
    assert_eq!(ticket.state, "passed");
    let summary = &f.waking(&f.coordinator.id)[0].body;
    assert!(summary.contains(&fixed), "{summary}");
}

#[test]
fn the_round_cap_blocks_the_ticket_and_wakes_the_coordinator_once() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    f.coordinator_reads();
    f.verify().unwrap();
    let (claude, codex, style) = (
        f.verifier("Claude"),
        f.verifier("Codex"),
        f.verifier("Style"),
    );
    f.report(&claude, "pass", "passed").unwrap();
    f.report(&style, "pass", "passed").unwrap();
    f.fail(&codex, "fail-1").unwrap();
    f.commit("fix.txt", "two");
    f.report(&f.implementer.id.clone(), "fixed", "ready_for_testing")
        .unwrap();
    let implementer_messages = f.host.messages(&f.implementer.id, None, 100).unwrap().len();

    f.fail(&codex, "fail-2").unwrap();

    let ticket = f.current();
    assert_eq!(ticket.state, "blocked");
    assert_eq!(
        ticket.verification.unwrap().outcome,
        VerificationOutcome::Blocked
    );
    assert_eq!(
        f.host.messages(&f.implementer.id, None, 100).unwrap().len(),
        implementer_messages,
        "no third request"
    );
    let waking = f.waking(&f.coordinator.id);
    assert_eq!(waking.len(), 1, "{waking:?}");
    for expected in [
        "cycle 1 of 2, 2 of 2 rounds",
        "round cap of 2",
        "Open findings:\n- F2 · src/calls.rs:42 · fail-2",
    ] {
        assert!(
            waking[0].body.contains(expected),
            "{expected}\n{}",
            waking[0].body
        );
    }
    assert!(
        f.host.snapshot().unwrap().attention.is_empty(),
        "the human's inbox gains nothing"
    );

    // A later call starts a fresh cycle with every verifier.
    f.report(&f.implementer.id.clone(), "again", "ready_for_testing")
        .unwrap();
    let fresh = f.verify().unwrap().verification.unwrap();
    assert_eq!(fresh.cycle, 2);
    assert_eq!(fresh.rounds[0].verifiers.len(), 3);
}

#[test]
fn a_cap_of_three_allows_a_third_round() {
    let mut f = Fixture::ready(vec![verifier(Role::Tester, "Codex", None)], 3);
    f.verify().unwrap();
    let codex = f.verifier("Codex");
    for round in 1..=2 {
        f.fail(&codex, &format!("fail-{round}")).unwrap();
        f.commit("fix.txt", &format!("fix {round}"));
        f.report(
            &f.implementer.id.clone(),
            &format!("fixed-{round}"),
            "ready_for_testing",
        )
        .unwrap();
    }
    assert_eq!(f.round().round, 3);
    assert_eq!(f.current().state, "verifying");
}

#[test]
fn a_blocked_verifier_blocks_the_ticket_after_its_round_without_another_round() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    f.coordinator_reads();
    f.verify().unwrap();
    f.report(&f.verifier("Claude"), "pass", "passed").unwrap();
    f.fail(&f.verifier("Codex"), "fail").unwrap();
    f.report(&f.verifier("Style"), "blocked", "blocked")
        .unwrap();

    assert_eq!(f.current().state, "blocked");
    assert_eq!(f.current().verification.unwrap().rounds.len(), 1);
    let waking = f.waking(&f.coordinator.id);
    assert_eq!(waking.len(), 1);
    assert!(
        waking[0].body.contains("blocked evidence from"),
        "{}",
        waking[0].body
    );
    assert!(
        f.waking(&f.implementer.id).is_empty(),
        "no failure is sent for another round"
    );
}

#[test]
fn a_verifier_that_changes_the_worktree_fails_whatever_it_reported() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    f.verify().unwrap();
    let claude = f.verifier("Claude");
    f.report(&claude, "pass", "passed").unwrap();
    f.commit("verifier.txt", "sneaky");

    f.report(&f.verifier("Codex"), "pass", "passed").unwrap();

    let round = f.round();
    let result = |focus: &str| round.verifiers.iter().find(|v| v.focus == focus).unwrap();
    assert_eq!(result("Claude").result, VerifierResult::Passed);
    assert_eq!(result("Claude").reason, None);
    assert_eq!(result("Codex").result, VerifierResult::Failed);
    assert_eq!(
        result("Codex").reason.as_deref(),
        Some("worktree changed during verification")
    );
}

#[test]
fn an_implementer_blocked_between_rounds_ends_the_cycle_and_wakes_the_coordinator() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    f.coordinator_reads();
    f.verify().unwrap();
    f.report(&f.verifier("Claude"), "pass", "passed").unwrap();
    f.fail(&f.verifier("Codex"), "fail").unwrap();
    f.report(&f.verifier("Style"), "pass", "passed").unwrap();

    f.report(&f.implementer.id.clone(), "stuck", "blocked")
        .unwrap();

    let ticket = f.current();
    assert_eq!(ticket.state, "blocked");
    let verification = ticket.verification.unwrap();
    assert_eq!(verification.outcome, VerificationOutcome::Blocked);
    assert_eq!(verification.rounds.len(), 1, "no further round");
    let waking = f.waking(&f.coordinator.id);
    assert_eq!(waking.len(), 1);
    assert_eq!(
        waking[0].id,
        format!("report:{}:stuck", f.implementer.id),
        "the implementer's own report wakes the coordinator"
    );
    assert!(f.host.snapshot().unwrap().attention.is_empty());
}

#[test]
fn a_verdict_after_its_cycle_ended_is_recorded_without_moving_the_ticket() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    f.coordinator_reads();
    f.verify().unwrap();
    f.report(&f.verifier("Claude"), "pass", "passed").unwrap();
    f.report(&f.implementer.id.clone(), "stuck", "blocked")
        .unwrap();
    f.coordinator_reads();

    let codex = f.verifier("Codex");
    f.fail(&codex, "late").unwrap();

    let ticket = f.current();
    assert_eq!(ticket.state, "blocked");
    let verification = ticket.verification.unwrap();
    assert_eq!(verification.outcome, VerificationOutcome::Blocked);
    let run = verification.rounds[0]
        .verifiers
        .iter()
        .find(|v| v.session_id == codex)
        .unwrap();
    assert_eq!(run.result, VerifierResult::Failed);
    assert_eq!(
        run.report_id.as_deref(),
        Some(format!("report:{codex}:late").as_str())
    );
    assert!(f.waking(&f.coordinator.id).is_empty(), "it does not wake");
    assert!(
        f.quiet_ids(&f.coordinator.id)
            .contains(&format!("report:{codex}:late")),
        "it reaches the coordinator in its next batch"
    );
}

#[test]
fn acceptance_refuses_commits_after_the_verified_one() {
    let mut f = Fixture::ready(vec![verifier(Role::Tester, "Codex", None)], 2);
    let verified = f.verify().unwrap().verification.unwrap().rounds[0]
        .commit
        .clone();
    f.report(&f.verifier("Codex"), "pass", "passed").unwrap();
    let moved = f.commit("late.txt", "after");

    let refused = f
        .host
        .agent_tool(
            &f.coordinator.id,
            "accept_ticket",
            json!({"ticket_id":f.ticket.id}),
        )
        .unwrap_err()
        .to_string();

    assert!(
        refused.contains(&verified) && refused.contains(&moved),
        "{refused}"
    );
    assert_eq!(f.current().state, "passed");
    assert!(f.worktree().exists());
}

#[test]
fn each_verifier_runs_the_coordinators_exact_choice_and_nothing_else() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    let project = f.coordinator.project_id.clone();
    let choice = |focus: &str, profile: ModelProfile| json!({"focus":focus,"profile":profile,"reason":format!("{focus} pass")});
    let valid = || {
        vec![
            choice("Claude", profile(Provider::Claude, "sonnet", "high")),
            choice("Codex", profile(Provider::Codex, "gpt-6.1-sol", "medium")),
            choice("style", profile(Provider::Codex, "gpt-6.1-sol", "high")),
        ]
    };
    let with = |index: usize, replacement: Option<Value>| {
        let mut choices = valid();
        match replacement {
            Some(value) => choices[index] = value,
            None => {
                choices.remove(index);
            }
        }
        json!({"ticket_id":f.ticket.id,"verifiers":choices})
    };
    let cases = [
        (
            with(
                1,
                Some(choice("Codex", profile(Provider::Claude, "opus", "high"))),
            ),
            "Claude is not configured for Tester · Codex. Tester · Codex uses: Codex.",
        ),
        (
            with(
                0,
                Some(choice("Claude", profile(Provider::Claude, "opus", "max"))),
            ),
            "claude · opus · max is not allowed on this machine.",
        ),
        (
            with(2, None),
            "Choose a profile and reason for Reviewer · Style",
        ),
        (
            with(
                2,
                Some(choice(
                    "Security",
                    profile(Provider::Codex, "gpt-6.1-sol", "high"),
                )),
            ),
            "No verifier has the focus \"Security\"",
        ),
        (
            with(
                0,
                Some(
                    json!({"focus":"Claude","profile":profile(Provider::Claude, "opus", "high"),"reason":"one\ntwo"}),
                ),
            ),
            "one line",
        ),
    ];
    let sessions = f.host.sessions().unwrap().len();
    for (args, message) in cases {
        let error = f
            .host
            .agent_tool(&f.coordinator.id, "verify_ticket", args)
            .unwrap_err();
        assert!(format!("{error:#}").contains(message), "{error:#}");
        assert_eq!(f.host.sessions().unwrap().len(), sessions, "no verifier");
        assert_eq!(f.current().state, "ready_for_testing");
        assert!(f.current().verification.is_none());
    }
    let rejected: Vec<_> = f
        .host
        .activity(&project, None, 100)
        .unwrap()
        .into_iter()
        .filter(|a| a.kind == "model_rejected")
        .map(|a| a.detail)
        .collect();
    assert_eq!(rejected.len(), 2, "{rejected:?}");
    assert!(
        rejected
            .iter()
            .all(|d| d.ends_with("No verifier was started."))
    );

    let started: Ticket = serde_json::from_value(
        f.host
            .agent_tool(
                &f.coordinator.id,
                "verify_ticket",
                json!({"ticket_id":f.ticket.id,"verifiers":valid()}),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(started.state, "verifying");
    for (focus, expected) in [
        ("Claude", profile(Provider::Claude, "sonnet", "high")),
        ("Codex", profile(Provider::Codex, "gpt-6.1-sol", "medium")),
        ("Style", profile(Provider::Codex, "gpt-6.1-sol", "high")),
    ] {
        let session = f.host.session(&f.verifier(focus)).unwrap();
        assert_eq!(session.provider, expected.provider);
        let runtime = f.host.session_runtime(&session.id).unwrap();
        assert_eq!(runtime.profile, Some(expected));
        let selection = runtime.selection.unwrap();
        assert_eq!(
            selection.chosen_by,
            Chooser::Coordinator {
                session_id: f.coordinator.id.clone()
            }
        );
        assert_eq!(
            selection.reason.to_lowercase(),
            format!("{} pass", focus.to_lowercase())
        );
    }
}

#[test]
fn an_invalid_verification_setting_keeps_the_saved_one() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    let saved = f.host.settings();
    let mut duplicate = three_verifiers();
    duplicate.push(verifier(Role::Reviewer, "Codex", None));
    for invalid in [
        VerificationSettings {
            verifiers: duplicate,
            max_rounds: 2,
            max_cycles: 2,
        },
        VerificationSettings {
            verifiers: three_verifiers(),
            max_rounds: 0,
            max_cycles: 2,
        },
    ] {
        assert!(f.host.set_verification(invalid).is_err());
        assert_eq!(f.host.settings(), saved);
    }
}

#[test]
fn without_a_saved_setting_a_tester_and_two_lens_reviewers_verify_with_caps_of_three_and_two() {
    let directory = tempfile::tempdir().unwrap();
    let host = Host::open(directory.path()).unwrap();
    let settings = host.settings().verification;
    assert_eq!((settings.max_rounds, settings.max_cycles), (3, 2));
    assert_eq!(settings, VerificationSettings::default());
    let focuses: Vec<_> = settings
        .verifiers
        .iter()
        .map(|v| v.focus.as_str())
        .collect();
    assert_eq!(focuses, ["Tests", "Correctness", "Regressions"]);
}

#[test]
fn a_verdict_answering_an_earlier_cycle_cannot_pass_the_next_one() {
    let mut f = Fixture::ready(vec![verifier(Role::Tester, "Codex", None)], 2);
    f.verify().unwrap();
    let codex = f.verifier("Codex");
    // Its turn takes cycle 1's input, then the implementer ends that cycle.
    take_input(&mut f.host, &codex);
    f.report(&f.implementer.id.clone(), "stuck", "blocked")
        .unwrap();
    f.report(&f.implementer.id.clone(), "again", "ready_for_testing")
        .unwrap();
    f.coordinator_reads();
    let cycle_two = f.verify().unwrap().verification.unwrap();
    assert_eq!(cycle_two.cycle, 2);
    assert!(f.archived(&codex), "the ended cycle retired its verifier");
    assert_ne!(f.verifier("Codex"), codex, "cycle 2 starts a fresh session");
    assert!(
        f.host
            .messages(&codex, None, 100)
            .unwrap()
            .iter()
            .all(|m| !m.id.contains(":2:")),
        "no cycle 2 input reaches the archived session"
    );

    // The cycle-1 turn now passes, while cycle 2's input still waits in its queue.
    f.host
        .agent_tool(
            &codex,
            "report",
            json!({"message_id":"late-pass","kind":"passed","body":"Green on the old commit"}),
        )
        .unwrap();

    let ticket = f.current();
    assert_eq!(ticket.state, "verifying");
    let verification = ticket.verification.unwrap();
    assert_eq!(verification.outcome, VerificationOutcome::Running);
    assert_eq!(
        verification.rounds[0].verifiers[0].result,
        VerifierResult::Pending
    );
    assert!(f.waking(&f.coordinator.id).is_empty());

    assert!(
        f.quiet_ids(&f.coordinator.id)
            .contains(&format!("report:{codex}:late-pass")),
        "the archived verifier's verdict still reaches the coordinator's next batch"
    );

    // Cycle 2 runs in a fresh session; its verdict is the one that counts.
    f.report(&f.verifier("Codex"), "pass-2", "passed").unwrap();
    assert_eq!(f.current().state, "passed");
}

#[test]
fn a_verdict_after_the_cycle_passed_changes_nothing_and_wakes_no_one() {
    let mut f = Fixture::ready(vec![verifier(Role::Tester, "Codex", None)], 2);
    f.verify().unwrap();
    let codex = f.verifier("Codex");
    f.report(&codex, "pass", "passed").unwrap();
    f.coordinator_reads();
    let passed = f.current().verification;

    f.report(&codex, "second-thoughts", "failed").unwrap();

    let ticket = f.current();
    assert_eq!(ticket.state, "passed");
    assert_eq!(ticket.verification, passed);
    assert!(f.waking(&f.coordinator.id).is_empty());
    assert!(
        f.quiet_ids(&f.coordinator.id)
            .contains(&format!("report:{codex}:second-thoughts")),
        "it is stored for the coordinator's next turn"
    );
}

#[test]
fn a_verifier_dropped_from_the_settings_stays_quiet_when_it_reports_late() {
    let mut f = Fixture::ready(vec![verifier(Role::Reviewer, "Old", None)], 2);
    f.verify().unwrap();
    let old = f.verifier("Old");
    take_input(&mut f.host, &old);
    f.report(&f.implementer.id.clone(), "stuck", "blocked")
        .unwrap();
    f.host
        .set_verification(VerificationSettings {
            verifiers: vec![verifier(Role::Tester, "New", None)],
            max_rounds: 2,
            max_cycles: 2,
        })
        .unwrap();
    f.report(&f.implementer.id.clone(), "again", "ready_for_testing")
        .unwrap();
    f.verify().unwrap();
    let new = f.verifier("New");
    f.report(&new, "pass", "passed").unwrap();
    f.coordinator_reads();
    let passed = f.current().verification;
    assert_eq!(
        passed.as_ref().unwrap().outcome,
        VerificationOutcome::Passed
    );

    // The dropped reviewer's cycle-1 turn reports at last.
    f.host
        .agent_tool(
            &old,
            "report",
            json!({"message_id":"late-fail","kind":"failed","body":"Style problems on the old commit"}),
        )
        .unwrap();

    let ticket = f.current();
    assert_eq!(ticket.state, "passed");
    assert_eq!(ticket.verification, passed);
    assert!(f.waking(&f.coordinator.id).is_empty());
    assert!(
        f.quiet_ids(&f.coordinator.id)
            .contains(&format!("report:{old}:late-fail"))
    );
}

/// `verify_ticket` with one choice for the single "Review" verifier.
fn verify_review(f: &mut Fixture, chosen: ModelProfile) -> anyhow::Result<Value> {
    let coordinator = f.coordinator.id.clone();
    f.host.agent_tool(
        &coordinator,
        "verify_ticket",
        json!({"ticket_id":f.ticket.id,"verifiers":[{"focus":"Review","profile":chosen,"reason":"Review pass"}]}),
    )
}

/// Ends cycle 1 with the implementer blocked, then reports it ready again.
fn ready_for_cycle_two(f: &mut Fixture) {
    f.report(&f.implementer.id.clone(), "stuck", "blocked")
        .unwrap();
    f.report(&f.implementer.id.clone(), "again", "ready_for_testing")
        .unwrap();
    f.coordinator_reads();
}

/// The "Review" verifier of the current cycle's first round.
fn current_review(f: &Fixture) -> String {
    f.current().verification.unwrap().rounds[0].verifiers[0]
        .session_id
        .clone()
}

#[test]
fn a_verifier_whose_provider_changed_starts_a_fresh_session_and_retires_the_old_one() {
    let mut f = Fixture::ready(
        vec![verifier(Role::Reviewer, "Review", Some(Provider::Claude))],
        2,
    );
    let opus = profile(Provider::Claude, "opus", "high");
    verify_review(&mut f, opus.clone()).unwrap();
    let old = current_review(&f);
    ready_for_cycle_two(&mut f);
    f.host
        .set_verification(VerificationSettings {
            verifiers: vec![verifier(Role::Reviewer, "Review", Some(Provider::Codex))],
            max_rounds: 2,
            max_cycles: 2,
        })
        .unwrap();
    let sol = profile(Provider::Codex, "gpt-6.1-sol", "high");
    verify_review(&mut f, sol.clone()).unwrap();
    let verification = f.current().verification.unwrap();
    assert_eq!(verification.cycle, 2);
    let new = current_review(&f);
    assert_ne!(new, old, "the Claude session is not reused");
    let session = f.host.session(&new).unwrap();
    assert_eq!(session.provider, Provider::Codex);
    let runtime = f.host.session_runtime(&new).unwrap();
    assert_eq!(runtime.profile, Some(sol));
    assert_eq!(runtime.selection.unwrap().reason, "Review pass");
    // The old session is retired with its model untouched.
    assert!(f.host.session(&old).unwrap().archived);
    assert_eq!(f.host.session_runtime(&old).unwrap().profile, Some(opus));
}

#[test]
fn a_restored_verifier_at_the_same_profile_reuses_its_session() {
    let mut f = Fixture::ready(vec![verifier(Role::Reviewer, "Review", None)], 2);
    let sonnet = profile(Provider::Claude, "sonnet", "high");
    verify_review(&mut f, sonnet.clone()).unwrap();
    let first = current_review(&f);
    ready_for_cycle_two(&mut f);
    // The ended cycle retired it; only a session the human restores can be reused.
    assert!(f.archived(&first));
    f.host.set_archived(&first, false).unwrap();
    verify_review(&mut f, sonnet.clone()).unwrap();
    assert_eq!(f.current().verification.unwrap().cycle, 2);
    assert_eq!(current_review(&f), first);
    assert!(!f.host.session(&first).unwrap().archived);
    assert_eq!(
        f.host.session_runtime(&first).unwrap().profile,
        Some(sonnet)
    );
}

#[test]
fn passed_verifiers_retire_after_their_round_and_the_rest_when_the_cycle_ends() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    f.verify().unwrap();
    let (claude, codex, style) = (
        f.verifier("Claude"),
        f.verifier("Codex"),
        f.verifier("Style"),
    );
    f.report(&claude, "pass", "passed").unwrap();
    assert!(!f.archived(&claude), "a pass mid-round retires nothing");
    f.fail(&codex, "fail").unwrap();
    f.report(&style, "pass", "passed").unwrap();

    assert!(f.archived(&claude) && f.archived(&style));
    assert!(!f.archived(&codex), "the failed verifier re-checks the fix");
    f.commit("fix.txt", "two");
    f.report(&f.implementer.id.clone(), "fixed", "ready_for_testing")
        .unwrap();
    f.report(&codex, "pass-2", "passed").unwrap();

    assert_eq!(f.current().state, "passed");
    assert!(f.archived(&codex));
    assert!(!f.archived(&f.implementer.id), "the implementer stays");
}

#[test]
fn a_cycle_blocked_by_its_implementer_retires_even_pending_verifiers() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    f.verify().unwrap();
    f.report(&f.verifier("Claude"), "pass", "passed").unwrap();
    let codex = f.verifier("Codex");

    f.report(&f.implementer.id.clone(), "stuck", "blocked")
        .unwrap();

    for focus in ["Claude", "Codex", "Style"] {
        assert!(f.archived(&f.verifier(focus)), "{focus}");
    }
    assert!(!f.archived(&f.implementer.id));

    // The archived verifier's turn still reports; it is recorded and wakes no one.
    f.coordinator_reads();
    f.fail(&codex, "late").unwrap();
    let ticket = f.current();
    assert_eq!(ticket.state, "blocked");
    let verification = ticket.verification.unwrap();
    assert_eq!(verification.outcome, VerificationOutcome::Blocked);
    assert_eq!(verification.rounds.len(), 1, "no new round");
    let run = verification.rounds[0]
        .verifiers
        .iter()
        .find(|v| v.session_id == codex)
        .unwrap();
    assert_eq!(
        run.result,
        VerifierResult::Failed,
        "recorded for the record"
    );
    assert!(f.waking(&f.coordinator.id).is_empty());
    assert!(f.waking(&f.implementer.id).is_empty());
}

/// Calls `archive_agent` as `caller` on `session`.
fn archive_agent(f: &mut Fixture, caller: &str, session: &str) -> anyhow::Result<Value> {
    f.host
        .agent_tool(caller, "archive_agent", json!({"session_id":session}))
}

#[test]
fn archive_agent_retires_an_idle_reviewer_and_keeps_it_restorable() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    let reviewer = f
        .host
        .assign_ticket(
            &f.ticket.id,
            Role::Reviewer,
            Provider::Codex,
            "Review",
            Some("Security"),
        )
        .unwrap();
    f.report(&reviewer.id, "findings", "failed").unwrap();
    let coordinator = f.coordinator.id.clone();

    archive_agent(&mut f, &coordinator, &reviewer.id).unwrap();

    assert!(f.archived(&reviewer.id));
    assert!(!f.host.messages(&reviewer.id, None, 100).unwrap().is_empty());
    f.host.set_archived(&reviewer.id, false).unwrap();
    assert!(!f.archived(&reviewer.id));
}

#[test]
fn archive_agent_refuses_each_agent_still_needed() {
    let mut f = Fixture::ready(three_verifiers(), 2);
    let coordinator = f.coordinator.id.clone();
    let implementer = f.implementer.id.clone();
    let refused = |f: &mut Fixture, caller: &str, session: &str| {
        let error = archive_agent(f, caller, session).unwrap_err().to_string();
        assert!(!f.archived(session), "{error}");
        error
    };

    let error = refused(&mut f, &coordinator, &implementer);
    assert!(error.contains("implementer of open ticket"), "{error}");

    f.verify().unwrap();
    let codex = f.verifier("Codex");
    let error = refused(&mut f, &coordinator, &codex);
    assert!(error.contains("owes verification cycle 1"), "{error}");
    f.report(&f.verifier("Claude"), "pass", "passed").unwrap();
    f.report(&f.verifier("Style"), "pass", "passed").unwrap();
    f.fail(&codex, "fail").unwrap();
    let error = refused(&mut f, &coordinator, &codex);
    assert!(
        error.contains("owes verification cycle 1"),
        "a failed verifier still re-checks the fix: {error}"
    );

    f.host.set_status(&codex, Status::Working).unwrap();
    let error = refused(&mut f, &coordinator, &codex);
    assert!(error.contains("mid-turn"), "{error}");

    let error = refused(&mut f, &implementer, &codex);
    assert!(
        error.contains("not an agent of a ticket you own"),
        "{error}"
    );
    let other = f.host.create_project("Billing").unwrap();
    let stranger = f
        .host
        .sessions()
        .unwrap()
        .into_iter()
        .find(|s| s.project_id == other.id)
        .unwrap();
    let error = refused(&mut f, &stranger.id, &codex);
    assert!(
        error.contains("not an agent of a ticket you own"),
        "{error}"
    );
    let error = refused(&mut f, &coordinator, &coordinator);
    assert!(
        error.contains("not an agent of a ticket you own"),
        "{error}"
    );
}

fn finding(severity: &str, summary: &str) -> Value {
    json!({"severity":severity,"location":"src/calls.rs:7","summary":summary,"trigger":"","evidence":""})
}

impl Fixture {
    fn ledger(&self) -> Vec<LedgerEntry> {
        self.current().ledger
    }
    fn entry(&self, id: &str) -> LedgerEntry {
        self.ledger().into_iter().find(|e| e.id == id).unwrap()
    }
    fn run(&self, focus: &str) -> VerifierRun {
        self.round()
            .verifiers
            .into_iter()
            .find(|v| v.focus == focus)
            .unwrap()
    }
    /// Commits a fix and reports ready, starting the next round.
    fn fix(&mut self, content: &str) {
        self.commit("fix.txt", content);
        let implementer = self.implementer.id.clone();
        self.report(&implementer, content, "ready_for_testing")
            .unwrap();
    }
}

fn one_tester(cap: u32) -> Fixture {
    let mut f = Fixture::ready(vec![verifier(Role::Tester, "Codex", None)], cap);
    f.verify().unwrap();
    f
}

#[test]
fn a_malformed_finding_or_a_foreign_id_refuses_the_whole_report() {
    let mut f = one_tester(2);
    let codex = f.verifier("Codex");
    let refused = [
        json!([{"severity":"critical","location":"a.rs:1","summary":"Bad","trigger":"t","evidence":"e"}]),
        json!([{"severity":"blocking","location":"a.rs:1","summary":"Two\nlines","trigger":"t","evidence":"e"}]),
        json!([{"severity":"blocking","location":"a.rs:1","summary":"Long","trigger":"t","evidence":"e".repeat(4096)}]),
        json!([{"severity":"blocking","location":" ","summary":"Nowhere","trigger":"t","evidence":"e"}]),
        json!([blocking("Real"), {"id":"F9","severity":"blocking","location":"a.rs:1","summary":"Unknown","trigger":"t","evidence":"e"}]),
    ];
    for (n, findings) in refused.into_iter().enumerate() {
        let id = format!("bad-{n}");
        assert!(
            f.report_findings(&codex, &id, "failed", findings).is_err(),
            "{id}"
        );
        assert!(f.host.message(&format!("report:{codex}:{id}")).is_err());
    }
    assert_eq!(f.run("Codex").result, VerifierResult::Pending);
    assert!(f.ledger().is_empty());
}

#[test]
fn an_id_must_name_an_open_or_settled_entry_of_the_verifiers_own_focus() {
    let mut f = Fixture::ready(
        vec![
            verifier(Role::Tester, "Codex", None),
            verifier(Role::Reviewer, "Style", None),
        ],
        3,
    );
    f.verify().unwrap();
    let (codex, style) = (f.verifier("Codex"), f.verifier("Style"));
    let codex_findings = json!([blocking("Busy line"), finding("non_blocking", "Naming")]);
    f.report_findings(&codex, "fail", "failed", codex_findings)
        .unwrap();
    f.fail(&style, "Style break").unwrap();
    f.fix("two");
    let again = |id: &str| {
        let mut finding = blocking("Again");
        finding["id"] = json!(id);
        json!([finding])
    };

    let foreign = f.report_findings(&style, "foreign", "failed", again("F1"));
    let untriaged = f.report_findings(&codex, "untriaged", "failed", again("F2"));

    assert!(foreign.unwrap_err().to_string().contains("not an open"));
    assert!(untriaged.is_err());
    assert!(
        f.round()
            .verifiers
            .iter()
            .all(|v| v.result == VerifierResult::Pending)
    );
    f.report_findings(&codex, "own", "failed", again("F1"))
        .unwrap();
    assert_eq!(f.entry("F1").finding.summary, "Again");
}

#[test]
fn findings_in_a_late_verdict_are_ignored() {
    let mut f = one_tester(2);
    let codex = f.verifier("Codex");
    f.report(&codex, "pass", "passed").unwrap();

    f.report_findings(
        &codex,
        "second-thoughts",
        "failed",
        json!([blocking("Late")]),
    )
    .unwrap();

    assert!(f.ledger().is_empty());
    assert_eq!(f.current().state, "passed");
}

#[test]
fn a_failure_without_evidence_passes_and_its_finding_awaits_triage() {
    let mut f = one_tester(2);
    let codex = f.verifier("Codex");

    f.report_findings(
        &codex,
        "fail",
        "failed",
        json!([finding("blocking", "No proof")]),
    )
    .unwrap();

    let run = f.run("Codex");
    assert_eq!(run.result, VerifierResult::Passed);
    assert_eq!(
        run.reason.as_deref(),
        Some("reported failed without an evidenced blocking finding")
    );
    let entry = f.entry("F1");
    assert_eq!(entry.status, EntryStatus::Untriaged);
    assert_eq!(
        (entry.focus.as_str(), entry.cycle, entry.round),
        ("Codex", 1, 1)
    );
}

#[test]
fn a_pass_with_an_evidenced_blocking_finding_fails() {
    let mut f = one_tester(2);
    let codex = f.verifier("Codex");

    f.report_findings(&codex, "pass", "passed", json!([blocking("Busy line")]))
        .unwrap();

    let run = f.run("Codex");
    assert_eq!(run.result, VerifierResult::Failed);
    assert_eq!(
        run.reason.as_deref(),
        Some("reported passed with an evidenced blocking finding")
    );
    assert_eq!(f.entry("F1").status, EntryStatus::Open);
}

#[test]
fn only_non_blocking_findings_pass_and_await_triage() {
    let mut f = one_tester(2);
    let codex = f.verifier("Codex");
    let findings = json!([
        finding("non_blocking", "Naming"),
        finding("non_blocking", "Comment"),
        finding("pre_existing", "Old bug"),
    ]);

    f.report_findings(&codex, "fail", "failed", findings)
        .unwrap();

    assert_eq!(f.run("Codex").result, VerifierResult::Passed);
    let statuses: Vec<_> = f
        .ledger()
        .iter()
        .map(|e| (e.id.clone(), e.status))
        .collect();
    assert_eq!(
        statuses,
        [
            ("F1".into(), EntryStatus::Untriaged),
            ("F2".into(), EntryStatus::Untriaged),
            ("F3".into(), EntryStatus::Untriaged),
        ]
    );
}

#[test]
fn the_implementer_gets_only_the_rounds_open_entries_from_wiffletree() {
    let mut f = Fixture::ready(three_verifiers(), 3);
    f.coordinator_reads();
    f.verify().unwrap();
    f.report(&f.verifier("Claude"), "pass", "passed").unwrap();
    f.report(&f.verifier("Style"), "pass", "passed").unwrap();
    let findings = json!([blocking("Busy line"), finding("non_blocking", "Naming nit")]);

    f.report_findings(&f.verifier("Codex"), "fail", "failed", findings)
        .unwrap();

    let sent = f.waking(&f.implementer.id);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].sender, None);
    let body = &sent[0].body;
    for expected in [
        "F1 · src/calls.rs:42 · Busy line",
        "Trigger: A call arrives while the line is busy",
        "Evidence: calls::busy_line fails",
        "tell the coordinator in one sentence with evidence",
    ] {
        assert!(body.contains(expected), "{expected}\n{body}");
    }
    assert!(
        !body.contains("Naming nit") && !body.contains("F2"),
        "{body}"
    );
    assert!(f.waking(&f.coordinator.id).is_empty());
}

#[test]
fn a_verifier_that_changed_the_worktree_is_named_to_the_implementer() {
    let mut f = Fixture::ready(three_verifiers(), 3);
    f.verify().unwrap();
    f.report(&f.verifier("Codex"), "pass", "passed").unwrap();
    f.report(&f.verifier("Style"), "pass", "passed").unwrap();
    f.commit("verifier.txt", "sneaky");

    f.report(&f.verifier("Claude"), "pass", "passed").unwrap();

    let sent = f.waking(&f.implementer.id);
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0]
            .body
            .contains("Tester · Claude: worktree changed during verification"),
        "{}",
        sent[0].body
    );
}

#[test]
fn a_blocking_finding_not_reported_again_is_fixed() {
    let mut f = one_tester(3);
    let codex = f.verifier("Codex");
    f.fail(&codex, "Busy line").unwrap();
    f.fix("two");

    f.report(&codex, "pass-2", "passed").unwrap();

    assert_eq!(f.entry("F1").status, EntryStatus::Fixed);
    assert_eq!(f.current().state, "passed");
}

#[test]
fn a_blocking_finding_reported_again_stays_open_with_the_new_round_and_evidence() {
    let mut f = one_tester(3);
    let codex = f.verifier("Codex");
    f.fail(&codex, "Busy line").unwrap();
    f.fix("two");
    let mut again = blocking("Busy line");
    again["id"] = json!("F1");
    again["evidence"] = json!("calls::busy_line still fails");

    f.report_findings(&codex, "fail-2", "failed", json!([again]))
        .unwrap();

    let entry = f.entry("F1");
    assert_eq!(entry.status, EntryStatus::Open);
    assert_eq!(entry.round, 2);
    assert_eq!(entry.finding.evidence, "calls::busy_line still fails");
    assert_eq!(f.ledger().len(), 1);
    assert_eq!(f.run("Codex").result, VerifierResult::Failed);
}

#[test]
fn a_re_check_that_cannot_run_leaves_the_entry_open() {
    let mut f = one_tester(3);
    let codex = f.verifier("Codex");
    f.fail(&codex, "Busy line").unwrap();
    f.fix("two");

    f.report(&codex, "cannot", "blocked").unwrap();

    assert_eq!(f.entry("F1").status, EntryStatus::Open);
    assert_eq!(f.current().state, "blocked");
}

#[test]
fn a_re_check_from_a_changed_worktree_fails_and_leaves_the_entry_open() {
    let mut f = one_tester(3);
    let codex = f.verifier("Codex");
    f.fail(&codex, "Busy line").unwrap();
    f.fix("two");
    f.commit("verifier.txt", "sneaky");

    f.report(&codex, "pass-2", "passed").unwrap();

    let run = f.run("Codex");
    assert_eq!(run.result, VerifierResult::Failed);
    assert_eq!(
        run.reason.as_deref(),
        Some("worktree changed during verification")
    );
    assert_eq!(f.entry("F1").status, EntryStatus::Open);
}

#[test]
fn an_open_entry_whose_verifier_left_the_settings_awaits_triage_when_the_next_cycle_ends() {
    let mut f = Fixture::ready(vec![verifier(Role::Reviewer, "Claude", None)], 2);
    f.verify().unwrap();
    f.fail(&f.verifier("Claude"), "Busy line").unwrap();
    f.report(&f.implementer.id.clone(), "stuck", "blocked")
        .unwrap();
    assert_eq!(
        f.entry("F1").status,
        EntryStatus::Open,
        "its verifier was in the cycle that ended"
    );
    f.host
        .set_verification(VerificationSettings {
            verifiers: vec![verifier(Role::Tester, "New", None)],
            max_rounds: 2,
            max_cycles: 2,
        })
        .unwrap();
    f.report(&f.implementer.id.clone(), "again", "ready_for_testing")
        .unwrap();
    f.verify().unwrap();
    assert_eq!(f.entry("F1").status, EntryStatus::Open);

    f.report(&f.verifier("New"), "pass", "passed").unwrap();

    assert_eq!(f.entry("F1").status, EntryStatus::Untriaged);
}

impl Fixture {
    fn triage(&mut self, decisions: Value) -> anyhow::Result<Value> {
        let coordinator = self.coordinator.id.clone();
        let ticket = self.ticket.id.clone();
        self.host.agent_tool(
            &coordinator,
            "triage_findings",
            json!({"ticket_id":ticket,"decisions":decisions}),
        )
    }
    /// The latest `verify:` message `session` received.
    fn round_input(&self, session: &str) -> String {
        self.host
            .messages(session, None, 100)
            .unwrap()
            .into_iter()
            .rfind(|m| m.id.starts_with("verify:"))
            .unwrap()
            .body
    }
}

fn decision(id: &str, decision: &str, reason: &str) -> Value {
    json!({"id":id,"decision":decision,"reason":reason})
}

/// One tester that reported a blocking finding (F1) and two non-blocking ones (F2, F3) in round 1.
fn three_findings() -> Fixture {
    let mut f = one_tester(3);
    let findings = json!([
        blocking("Busy line"),
        finding("non_blocking", "Naming"),
        finding("non_blocking", "Comment")
    ]);
    f.report_findings(&f.verifier("Codex"), "fail", "failed", findings)
        .unwrap();
    f
}

#[test]
fn a_finding_is_triaged_once_and_wakes_no_one() {
    let mut f = three_findings();
    f.coordinator_reads();
    let implementer_inputs = f.waking(&f.implementer.id).len();

    f.triage(json!([decision(
        "F2",
        "wont_fix",
        "Matches the existing naming"
    )]))
    .unwrap();
    let again = f.triage(json!([decision("F2", "follow_up", "Later")]));

    assert!(again.unwrap_err().to_string().contains("already won't fix"));
    let entry = f.entry("F2");
    assert_eq!(entry.status, EntryStatus::WontFix);
    assert_eq!(entry.reason.as_deref(), Some("Matches the existing naming"));
    assert!(f.waking(&f.coordinator.id).is_empty());
    assert_eq!(f.waking(&f.implementer.id).len(), implementer_inputs);
    let events = f
        .host
        .activity(&f.coordinator.project_id, None, 50)
        .unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.kind == "findings_triaged" && e.detail.ends_with("F2 won't fix")),
        "{events:?}"
    );
}

#[test]
fn a_triage_call_applies_every_decision_or_none() {
    let mut f = three_findings();
    for refused in [
        json!([
            decision("F2", "follow_up", "Later"),
            decision("F99", "wont_fix", "Gone")
        ]),
        json!([
            decision("F2", "follow_up", "Later"),
            decision("F2", "wont_fix", "Twice")
        ]),
        json!([decision("F2", "follow_up", "Two\nlines")]),
        json!([decision("F2", "fixed", "Not a decision")]),
    ] {
        assert!(f.triage(refused.clone()).is_err(), "{refused}");
        assert_eq!(f.entry("F2").status, EntryStatus::Untriaged, "{refused}");
    }
    let implementer = f.implementer.id.clone();
    let worker = f.host.agent_tool(
        &implementer,
        "triage_findings",
        json!({"ticket_id":f.ticket.id,"decisions":[decision("F2","wont_fix","Mine")]}),
    );
    assert!(
        worker
            .unwrap_err()
            .to_string()
            .contains("project coordinator")
    );
    assert_eq!(f.entry("F2").status, EntryStatus::Untriaged);
}

#[test]
fn overruling_a_routed_finding_mid_cycle_lets_its_re_report_pass() {
    let mut f = three_findings();
    f.triage(json!([decision(
        "F1",
        "wont_fix",
        "The implementer showed the line is never busy"
    )]))
    .unwrap();
    f.fix("two");
    let mut again = blocking("Busy line");
    again["id"] = json!("F1");

    f.report_findings(&f.verifier("Codex"), "again", "failed", json!([again]))
        .unwrap();

    assert_eq!(f.run("Codex").result, VerifierResult::Passed);
    assert_eq!(f.entry("F1").status, EntryStatus::WontFix);
    assert_eq!(f.current().state, "passed");
}

#[test]
fn fix_now_does_not_overrule_an_open_finding() {
    let mut f = three_findings();

    let refused = f.triage(json!([decision("F1", "fix_now", "Fix it")]));

    assert!(refused.unwrap_err().to_string().contains("already routed"));
    assert_eq!(f.entry("F1").status, EntryStatus::Open);
    f.fix("two");
    let mut again = blocking("Busy line");
    again["id"] = json!("F1");
    f.report_findings(&f.verifier("Codex"), "again", "failed", json!([again]))
        .unwrap();
    assert_eq!(f.run("Codex").result, VerifierResult::Failed);
    assert_eq!(f.entry("F1").status, EntryStatus::Open);
}

/// Ends the current cycle with the implementer blocked, then reports it ready again.
fn end_cycle(f: &mut Fixture) {
    let implementer = f.implementer.id.clone();
    let cycle = f.current().verification.unwrap().cycle;
    f.report(&implementer, &format!("stuck-{cycle}"), "blocked")
        .unwrap();
    f.report(&implementer, &format!("again-{cycle}"), "ready_for_testing")
        .unwrap();
    f.coordinator_reads();
}

#[test]
fn the_cycle_cap_refuses_a_third_cycle_until_the_human_raises_it() {
    let mut f = one_tester(2);
    end_cycle(&mut f);
    f.verify().unwrap();
    end_cycle(&mut f);
    let sessions = f.host.sessions().unwrap().len();

    let refused = f.verify().unwrap_err().to_string();

    for expected in ["2 verification cycles", "waiver", "close", "ask the human"] {
        assert!(refused.contains(expected), "{expected}: {refused}");
    }
    assert_eq!(
        f.host.sessions().unwrap().len(),
        sessions,
        "no verifier started"
    );
    assert_eq!(f.current().verification.unwrap().cycle, 2);

    let mut raised = f.host.settings().verification;
    raised.max_cycles = 3;
    f.host.set_verification(raised).unwrap();
    assert_eq!(f.verify().unwrap().verification.unwrap().cycle, 3);
}

#[test]
fn a_new_cycle_keeps_the_earlier_one() {
    let mut f = one_tester(2);
    f.report(&f.verifier("Codex"), "pass", "passed").unwrap();
    let first = f.current().verification.unwrap();
    let implementer = f.implementer.id.clone();
    f.report(&implementer, "again", "ready_for_testing")
        .unwrap();

    f.verify().unwrap();

    let ticket = f.current();
    assert_eq!(ticket.previous_cycles, [first]);
    assert_eq!(
        ticket.previous_cycles[0].outcome,
        VerificationOutcome::Passed
    );
    assert_eq!(ticket.verification.unwrap().cycle, 2);
}

#[test]
fn every_round_names_the_diff_and_the_decided_entries() {
    let mut f = three_findings();
    f.triage(json!([
        decision("F2", "wont_fix", "Matches the naming"),
        decision("F3", "follow_up", "Ticket for later")
    ]))
    .unwrap();
    end_cycle(&mut f);
    let base = git(f.worktree(), &["rev-parse", "main"]);
    let head = git(f.worktree(), &["rev-parse", "HEAD"]);

    f.verify().unwrap();

    let input = f.round_input(&f.verifier("Codex"));
    for expected in [
        format!("Base: {base} (main)"),
        format!("Diff: {base}..{head}, 1 file changed, 1 insertion(+)"),
        "Already decided; do not re-raise unless the cited code changed:".into(),
        "- F2 · src/calls.rs:7 · Naming (won't fix)".into(),
        "- F3 · src/calls.rs:7 · Comment (follow-up)".into(),
        "- F1 · src/calls.rs:42 · Busy line".into(),
        "Instruction: Check Codex".into(),
    ] {
        assert!(input.contains(&expected), "{expected}\n{input}");
    }
    assert!(!input.contains("New since"), "{input}");
}

#[test]
fn round_two_re_checks_only_the_open_findings_and_the_new_diff() {
    let mut f = one_tester(3);
    let first = f.round().commit;
    f.fail(&f.verifier("Codex"), "Busy line").unwrap();

    f.fix("two");

    let fixed = f.round().commit;
    let input = f.round_input(&f.verifier("Codex"));
    for expected in [
        "Check only whether your open blocking findings are fixed".to_owned(),
        format!("New since round 1: {first}..{fixed}, 1 file changed"),
        "report one again by its id only if it is still present:\n- F1 · src/calls.rs:42 · Busy line".into(),
        "Instruction: Check Codex".into(),
    ] {
        assert!(input.contains(&expected), "{expected}\n{input}");
    }
}

#[test]
fn a_re_check_that_could_not_run_is_listed_in_the_next_cycle() {
    let mut f = one_tester(3);
    f.fail(&f.verifier("Codex"), "Busy line").unwrap();
    f.fix("two");
    f.report(&f.verifier("Codex"), "cannot", "blocked").unwrap();
    let implementer = f.implementer.id.clone();
    f.report(&implementer, "again", "ready_for_testing")
        .unwrap();

    f.verify().unwrap();

    let input = f.round_input(&f.verifier("Codex"));
    assert!(
        input.contains("- F1 · src/calls.rs:42 · Busy line"),
        "{input}"
    );
}

#[test]
fn a_round_starts_when_the_base_is_unavailable() {
    let mut f = Fixture::ready(vec![verifier(Role::Tester, "Codex", None)], 2);
    let db = rusqlite::Connection::open(f.host.home.join("workspace.sqlite3")).unwrap();
    db.execute(
        "UPDATE repositories SET data=json_set(data,'$.base','origin/gone')",
        [],
    )
    .unwrap();

    f.verify().unwrap();

    let input = f.round_input(&f.verifier("Codex"));
    assert!(
        input.contains("Base unavailable: No merge-base with origin/gone"),
        "{input}"
    );
}

#[test]
fn a_pass_with_an_untriaged_finding_asks_for_triage_then_acceptance() {
    let mut f = one_tester(2);
    f.coordinator_reads();
    let commit = f.round().commit;

    f.report_findings(
        &f.verifier("Codex"),
        "pass",
        "passed",
        json!([finding("non_blocking", "Naming")]),
    )
    .unwrap();

    let waking = f.waking(&f.coordinator.id);
    assert_eq!(waking.len(), 1);
    let body = &waking[0].body;
    for expected in [
        "passed in cycle 1 of 2, 1 of 2 rounds".to_owned(),
        format!("Tester · Codex: passed at {commit}"),
        "Untriaged findings:\n- F1 · src/calls.rs:7 · Naming".into(),
        "Triage the untriaged findings with triage_findings, then accept the ticket.".into(),
    ] {
        assert!(body.contains(&expected), "{expected}\n{body}");
    }
    assert!(!body.contains("fresh cycle"), "{body}");
}

#[test]
fn the_last_cycle_blocked_offers_no_further_verification() {
    let mut f = one_tester(1);
    end_cycle(&mut f);
    f.verify().unwrap();

    f.fail(&f.verifier("Codex"), "Busy line").unwrap();

    let body = &f.waking(&f.coordinator.id)[0].body;
    assert!(body.contains("cycle 2 of 2, 1 of 1 rounds"), "{body}");
    assert!(
        body.contains("waived")
            && body.contains("close the ticket")
            && body.contains("ask the human")
    );
    assert!(!body.contains("verify_ticket"), "{body}");
}

#[test]
fn a_verifier_that_cannot_verify_is_reported_without_offering_a_waiver() {
    let mut f = one_tester(2);
    f.coordinator_reads();

    f.report(&f.verifier("Codex"), "cannot", "blocked").unwrap();

    let body = &f.waking(&f.coordinator.id)[0].body;
    assert!(body.contains("a verifier could not verify"), "{body}");
    assert!(body.contains("blocked evidence from"), "{body}");
    assert!(!body.contains("waived"), "{body}");
    assert!(body.contains("call verify_ticket again"), "{body}");
}

#[test]
fn workspace_context_lists_each_tickets_ledger_and_earlier_cycles() {
    let mut f = three_findings();
    end_cycle(&mut f);
    f.verify().unwrap();
    let coordinator = f.coordinator.id.clone();

    let context = f
        .host
        .agent_tool(&coordinator, "workspace_context", json!({}))
        .unwrap();

    let ticket = &context["tickets"][0];
    let ledger: Vec<(&str, &str, &str)> = ticket["ledger"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["id"].as_str().unwrap(),
                e["status"].as_str().unwrap(),
                e["focus"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        ledger,
        [
            ("F1", "open", "Codex"),
            ("F2", "untriaged", "Codex"),
            ("F3", "untriaged", "Codex")
        ]
    );
    assert_eq!(ticket["previous_cycles"][0]["cycle"], 1);
    assert_eq!(ticket["verification"]["cycle"], 2);
}

impl Fixture {
    fn accept(&mut self, waived: Option<&str>) -> anyhow::Result<Value> {
        let coordinator = self.coordinator.id.clone();
        let mut args = json!({"ticket_id":self.ticket.id});
        if let Some(waived) = waived {
            args["waived"] = json!(waived);
        }
        self.host.agent_tool(&coordinator, "accept_ticket", args)
    }
}

#[test]
fn a_report_after_a_passed_cycle_does_not_block_acceptance() {
    let mut f = one_tester(2);
    f.report(&f.verifier("Codex"), "pass", "passed").unwrap();
    let implementer = f.implementer.id.clone();
    f.report(&implementer, "pr-opened", "completed").unwrap();
    assert_eq!(f.current().state, "completed");

    f.accept(None).unwrap();

    assert_eq!(f.current().state, "accepted");
}

#[test]
fn a_squash_after_verification_is_accepted_without_a_new_cycle() {
    let mut f = one_tester(3);
    f.fail(&f.verifier("Codex"), "Busy line").unwrap();
    f.fix("two");
    f.report(&f.verifier("Codex"), "pass-2", "passed").unwrap();
    let verified = f.round().commit;
    git(f.worktree(), &["reset", "-q", "--soft", "main"]);
    git(f.worktree(), &["commit", "-q", "-m", "Squashed"]);
    assert_ne!(git(f.worktree(), &["rev-parse", "HEAD"]), verified);

    f.accept(None).unwrap();

    let ticket = f.current();
    assert_eq!(ticket.state, "accepted");
    assert_eq!(ticket.verification.unwrap().cycle, 1);
}

#[test]
fn a_moved_head_without_a_comparable_patch_is_refused_with_the_reason() {
    let mut f = one_tester(2);
    f.report(&f.verifier("Codex"), "pass", "passed").unwrap();
    git(f.worktree(), &["commit", "-q", "--amend", "-m", "Reworded"]);
    let db = rusqlite::Connection::open(f.host.home.join("workspace.sqlite3")).unwrap();
    db.execute(
        "UPDATE repositories SET data=json_set(data,'$.base','origin/gone')",
        [],
    )
    .unwrap();

    let refused = f.accept(None).unwrap_err().to_string();

    assert!(
        refused.contains("could not be compared") && refused.contains("origin/gone"),
        "{refused}"
    );
    assert_eq!(f.current().state, "passed");
}

#[test]
fn untriaged_findings_block_acceptance_until_triaged() {
    let mut f = one_tester(2);
    let findings = json!([finding("non_blocking", "Naming")]);
    f.report_findings(&f.verifier("Codex"), "pass", "passed", findings)
        .unwrap();

    let refused = f.accept(None).unwrap_err().to_string();

    assert!(refused.contains("Triage F1"), "{refused}");
    assert!(f.worktree().exists());
    f.triage(json!([decision(
        "F1",
        "follow_up",
        "Rename in the cleanup ticket"
    )]))
    .unwrap();
    f.accept(None).unwrap();
    assert_eq!(f.current().state, "accepted");
}

/// A cycle blocked at a round cap of 1 with F1 open.
fn blocked_at_the_cap() -> Fixture {
    let mut f = one_tester(1);
    f.fail(&f.verifier("Codex"), "Busy line").unwrap();
    assert_eq!(f.current().state, "blocked");
    f
}

#[test]
fn a_waiver_accepts_a_blocked_cycle_and_records_the_open_findings() {
    let mut f = blocked_at_the_cap();
    let waiver = "F1 needs a base ref the host always fetches";

    f.accept(Some(waiver)).unwrap();

    let ticket = f.current();
    assert_eq!(ticket.state, "accepted");
    assert_eq!(ticket.waiver.as_deref(), Some(waiver));
    assert_eq!(
        ticket.verification.unwrap().outcome,
        VerificationOutcome::Blocked
    );
    assert_eq!(f.entry("F1").status, EntryStatus::Open);
    let events = f
        .host
        .activity(&f.coordinator.project_id, None, 50)
        .unwrap();
    let accepted = events.iter().find(|e| e.kind == "ticket_accepted").unwrap();
    assert_eq!(
        accepted.detail,
        format!("{}; waived: {waiver}; open: F1", f.ticket.id)
    );
}

#[test]
fn a_waiver_is_refused_without_a_fully_checked_blocked_cycle() {
    let mut blocked = blocked_at_the_cap();
    let refused = blocked.accept(None).unwrap_err().to_string();
    assert!(refused.contains("waived"), "{refused}");
    let refused = blocked.accept(Some("Two\nlines"));
    assert!(refused.is_err());

    let mut cannot = one_tester(2);
    cannot
        .report(&cannot.verifier("Codex"), "cannot", "blocked")
        .unwrap();
    let refused = cannot
        .accept(Some("Accept anyway"))
        .unwrap_err()
        .to_string();
    assert!(refused.contains("could not verify"), "{refused}");

    let mut passed = one_tester(2);
    passed
        .report(&passed.verifier("Codex"), "pass", "passed")
        .unwrap();
    let refused = passed
        .accept(Some("Accept anyway"))
        .unwrap_err()
        .to_string();
    assert!(refused.contains("nothing to waive"), "{refused}");

    for f in [&blocked, &cannot, &passed] {
        let ticket = f.current();
        assert_ne!(ticket.state, "accepted");
        assert_eq!(ticket.waiver, None);
        assert!(f.worktree().exists());
    }
}

#[test]
fn a_saved_cycle_cap_survives_restart_and_reaches_the_coordinator() {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open(directory.path()).unwrap();
    let mut settings = host.settings().verification;
    settings.max_cycles = 3;
    host.set_verification(settings).unwrap();
    drop(host);

    let mut host = Host::open(directory.path()).unwrap();
    host.create_project("Calls").unwrap();
    let coordinator = host.sessions().unwrap().remove(0);
    let context = host
        .agent_tool(&coordinator.id, "workspace_context", json!({}))
        .unwrap();

    assert_eq!(host.settings().verification.max_cycles, 3);
    assert_eq!(context["model_selection"]["max_cycles"], 3);
    assert_eq!(context["model_selection"]["max_rounds"], 3);
}
