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
        })
        .unwrap();
        let project = host.create_project("Calls").unwrap();
        let coordinator = host.sessions().unwrap().remove(0);
        let repository = host
            .attach_repository(&project.id, repository.to_str().unwrap(), "HEAD")
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
    assert!(refused.contains("verifying"), "{refused}");
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
    f.report(&codex, "fail", "failed").unwrap();
    f.report(&style, "pass", "passed").unwrap();

    assert_eq!(f.current().state, "failed");
    assert!(
        f.waking(&f.coordinator.id).is_empty(),
        "the coordinator has no turn started by the reports"
    );
    let sent = f.waking(&f.implementer.id);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].id, format!("verification:{}:1:1", f.ticket.id));
    assert_eq!(sent[0].sender.as_ref(), Some(&f.coordinator.id));
    assert!(
        sent[0]
            .body
            .contains(&format!("failed evidence from {codex}"))
    );
    assert!(
        !sent[0]
            .body
            .contains(&format!("passed evidence from {claude}"))
    );

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
    f.report(&codex, "fail-1", "failed").unwrap();
    f.commit("fix.txt", "two");
    f.report(&f.implementer.id.clone(), "fixed", "ready_for_testing")
        .unwrap();
    let implementer_messages = f.host.messages(&f.implementer.id, None, 100).unwrap().len();

    f.report(&codex, "fail-2", "failed").unwrap();

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
    for expected in ["2 of 2 rounds", "round cap of 2", "Tester · Codex"] {
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
        f.report(&codex, &format!("fail-{round}"), "failed")
            .unwrap();
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
    f.report(&f.verifier("Codex"), "fail", "failed").unwrap();
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
    f.report(&f.verifier("Codex"), "fail", "failed").unwrap();
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
    f.report(&codex, "late", "failed").unwrap();

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
        },
        VerificationSettings {
            verifiers: three_verifiers(),
            max_rounds: 0,
        },
    ] {
        assert!(f.host.set_verification(invalid).is_err());
        assert_eq!(f.host.settings(), saved);
    }
}

#[test]
fn without_a_saved_setting_a_tester_and_two_reviewers_verify_with_a_cap_of_two() {
    let directory = tempfile::tempdir().unwrap();
    let host = Host::open(directory.path()).unwrap();
    let settings = host.settings().verification;
    assert_eq!(settings.max_rounds, 2);
    assert_eq!(settings, VerificationSettings::default());
    assert_eq!(settings.verifiers.len(), 3);
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

    // Its turn on cycle 2's input is the one that counts.
    f.report(&codex, "pass-2", "passed").unwrap();
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
fn a_verifier_at_the_same_profile_reuses_its_session() {
    let mut f = Fixture::ready(vec![verifier(Role::Reviewer, "Review", None)], 2);
    let sonnet = profile(Provider::Claude, "sonnet", "high");
    verify_review(&mut f, sonnet.clone()).unwrap();
    let first = current_review(&f);
    ready_for_cycle_two(&mut f);
    verify_review(&mut f, sonnet.clone()).unwrap();
    assert_eq!(f.current().verification.unwrap().cycle, 2);
    assert_eq!(current_review(&f), first);
    assert!(!f.host.session(&first).unwrap().archived);
    assert_eq!(
        f.host.session_runtime(&first).unwrap().profile,
        Some(sonnet)
    );
}
