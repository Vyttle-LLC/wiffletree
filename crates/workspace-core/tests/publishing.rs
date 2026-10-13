use serde_json::json;
use workspace_core::*;

#[test]
fn a_repository_stored_before_owners_decodes_and_the_new_fields_round_trip() {
    let stored: Repository = serde_json::from_value(
        json!({"id":"r","name":"app","path":"/code/app","base":"origin/main"}),
    )
    .unwrap();
    assert_eq!(
        (stored.github.clone(), stored.origin_problem.clone()),
        (None, None)
    );
    // A repository without the facts is written as it was before.
    assert_eq!(
        serde_json::to_value(&stored).unwrap(),
        json!({"id":"r","name":"app","path":"/code/app","base":"origin/main"})
    );
    let repository = Repository {
        github: Some(GitHubRepository {
            owner: "old-org".into(),
            name: "app".into(),
            facts: Some(GitHubFacts {
                name_with_owner: "new-org/app".into(),
                organization: true,
                fork: true,
                parent: Some("upstream/app".into()),
                public: false,
                checked_at: 7,
            }),
            problem: None,
        }),
        ..stored
    };
    let value = serde_json::to_value(&repository).unwrap();
    assert_eq!(value["github"]["facts"]["name_with_owner"], "new-org/app");
    assert_eq!(
        serde_json::from_value::<Repository>(value).unwrap(),
        repository
    );
}

#[test]
fn settings_and_a_snapshot_saved_before_owners_allow_nobody() {
    let settings: HostSettings = serde_json::from_value(json!({"workspaces_dir":"/w"})).unwrap();
    assert!(settings.allowed_owners.is_empty());
    assert!(HostSettings::new("/w".into()).allowed_owners.is_empty());
    let snapshot: Snapshot = serde_json::from_value(json!({
        "projects":[],"repositories":[],"sessions":[],"attention":[]
    }))
    .unwrap();
    assert!(snapshot.allowed_owners.is_empty());
    assert_eq!(snapshot.github_error, None);
}

#[test]
fn a_ticket_stored_before_publishing_decodes_and_a_request_round_trips() {
    let mut ticket: Ticket = serde_json::from_value(json!({
        "id":"t","coordinator_id":"c","repository_id":"r","title":"T","brief":"B",
        "worktree":"/w","branch":"wiffletree/t","state":"planned"
    }))
    .unwrap();
    assert!(ticket.publishing.is_none() && ticket.pushed.is_none());
    let request = PublishRequest {
        id: "r1".into(),
        caller: "implementer".into(),
        repository: "celadon-theme/website".into(),
        push_url: "git@github.com:celadon-theme/website.git".into(),
        branch: "wiffletree/t".into(),
        base: "main".into(),
        commit: "9e03c1b".into(),
        cycle: 1,
        outcome: VerificationOutcome::Passed,
        size: "2 commits · 3 files changed".into(),
        title: Some("Fix".into()),
        body: Some("Why".into()),
        step: PublishStep::OpenPullRequest,
        waiting: Some("attention".into()),
        blocked_review_approved: false,
        approvals: 1,
    };
    ticket.publishing = Some(request.clone());
    ticket.pushed = Some("9e03c1b".into());
    let value = serde_json::to_value(&ticket).unwrap();
    assert_eq!(value["publishing"]["step"], "open_pull_request");
    let decoded: Ticket = serde_json::from_value(value).unwrap();
    assert_eq!(decoded.publishing, Some(request));
    assert_eq!(decoded.pushed.as_deref(), Some("9e03c1b"));
}

#[test]
fn a_publish_item_is_an_approval_and_its_result_comes_from_wiffletree() {
    let attention = |operation: &str| Attention {
        id: "a".into(),
        project_id: "p".into(),
        session_id: "s".into(),
        host: "local".into(),
        operation_id: operation.into(),
        prompt: "Push?".into(),
        options: vec![],
        answer: None,
    };
    let publish = attention("publish:r1:push");
    assert!(publish.is_permission() && publish.is_publish());
    let permission = attention("permission:1");
    assert!(permission.is_permission() && !permission.is_publish());
    assert!(!attention("deploy-question").is_permission());
    assert_eq!(host_notice("publish:r1"), Some("Wiffletree"));
    assert_eq!(host_notice("answer:a"), None);
}

#[test]
fn allowing_an_owner_is_a_client_command() {
    let command = Command::SetOwnerAllowed {
        entry: "Sage-Social".into(),
        allowed: true,
    };
    assert_eq!(
        serde_json::to_value(&command).unwrap(),
        json!({"type":"set_owner_allowed","entry":"Sage-Social","allowed":true})
    );
}

fn entry(id: &str, status: EntryStatus) -> LedgerEntry {
    LedgerEntry {
        id: id.into(),
        finding: Finding {
            severity: Severity::NonBlocking,
            location: "src/tree.rs:88".into(),
            summary: "Matching is case-sensitive.".into(),
            trigger: String::new(),
            evidence: String::new(),
        },
        focus: "Correctness".into(),
        cycle: 2,
        round: 1,
        status,
        reason: None,
    }
}

fn run(focus: &str, result: VerifierResult) -> VerifierRun {
    VerifierRun {
        session_id: focus.into(),
        role: Role::Reviewer,
        focus: focus.into(),
        message_id: "m".into(),
        result,
        report_id: None,
        reason: None,
    }
}

#[test]
fn open_findings_are_those_neither_fixed_nor_settled() {
    let mut ticket: Ticket = serde_json::from_value(json!({
        "id":"t","coordinator_id":"c","repository_id":"r","title":"T","brief":"B",
        "worktree":"/w","branch":"wiffletree/t","state":"planned"
    }))
    .unwrap();
    ticket.ledger = vec![
        entry("F1", EntryStatus::Open),
        entry("F2", EntryStatus::Untriaged),
        entry("F3", EntryStatus::FixNow),
        entry("F4", EntryStatus::Fixed),
        entry("F5", EntryStatus::FollowUp),
        entry("F6", EntryStatus::WontFix),
    ];
    let open: Vec<&str> = ticket
        .open_findings()
        .iter()
        .map(|e| e.id.as_str())
        .collect();
    assert_eq!(open, ["F1", "F2", "F3"]);
    assert_eq!(
        ticket.ledger[0].heading(),
        "F1 · non-blocking · Correctness · src/tree.rs:88"
    );
}

#[test]
fn a_review_says_how_it_ended_and_which_verifiers_did_not_check() {
    let round = |round, commit: &str, verifiers| VerificationRound {
        round,
        commit: commit.into(),
        verifiers,
    };
    let passed = Verification {
        cycle: 1,
        max_rounds: 3,
        outcome: VerificationOutcome::Passed,
        rounds: vec![round(
            1,
            "9e03c1b7aa",
            vec![
                run("Tests", VerifierResult::Passed),
                run("Correctness", VerifierResult::Passed),
            ],
        )],
    };
    assert_eq!(
        passed.review_line("9e03c1b7aa"),
        "Cycle 1 passed at this commit · Tests, Correctness"
    );
    assert_eq!(
        passed.review_line("4c2a9f1000"),
        "Cycle 1 passed at 9e03c1b, which has the same changes as this commit · Tests, Correctness"
    );
    assert!(passed.unchecked().is_empty());
    let blocked = Verification {
        cycle: 2,
        max_rounds: 3,
        outcome: VerificationOutcome::Blocked,
        rounds: vec![
            round(1, "aaaaaaa1", vec![run("Tests", VerifierResult::Failed)]),
            round(
                2,
                "c81f04d9",
                vec![
                    run("Tests", VerifierResult::Passed),
                    run("Correctness", VerifierResult::Failed),
                    run("Regressions", VerifierResult::Blocked),
                    run("Style", VerifierResult::Pending),
                ],
            ),
        ],
    };
    assert_eq!(
        blocked.review_line("c81f04d9"),
        "Cycle 2 ended blocked at c81f04d, after 2 of 3 rounds · Tests passed, Correctness failed, Regressions could not check, Style never reported"
    );
    assert_eq!(blocked.unchecked(), ["Regressions", "Style"]);
}

/// A reviewed ticket with a waiting request, in `celadon-theme/website`.
fn waiting_ticket(step: &str, outcome: &str) -> (Ticket, Repository) {
    let ticket: Ticket = serde_json::from_value(json!({
        "id":"t","coordinator_id":"c","repository_id":"r","title":"Fix palette contrast",
        "brief":"B","worktree":"/w","branch":"wiffletree/fix-palette-contrast","state":"passed",
        "verification": {"cycle": 1, "max_rounds": 3, "outcome": outcome, "rounds": [
            {"round": 1, "commit": "9e03c1b000", "verifiers": [
                {"session_id":"a","role":"tester","focus":"Tests","message_id":"m","result":"passed"},
                {"session_id":"b","role":"reviewer","focus":"Correctness","message_id":"m",
                 "result": if outcome == "passed" { "passed" } else { "failed" }}]}]},
        "publishing": {"id":"r1","caller":"i","repository":"celadon-theme/website",
            "push_url":"git@github.com:celadon-theme/website.git",
            "branch":"wiffletree/fix-palette-contrast","base":"main","commit":"9e03c1b000",
            "cycle": 1, "outcome": outcome,
            "size":"2 commits · 3 files changed, 41 insertions(+), 7 deletions(-)",
            "title":"Fix palette contrast in dark mode","body":"Muted text fell below 4.5:1.\n- Darken the overlay",
            "step": step, "waiting":"item"}
    }))
    .unwrap();
    let repository: Repository = serde_json::from_value(json!({
        "id":"r","name":"celadon-website","path":"/code/celadon-website","base":"origin/main",
        "github": {"owner":"celadon-theme","name":"website","facts": {
            "name_with_owner":"celadon-theme/website","organization":true,"fork":false,
            "public":true,"checked_at":1}}
    }))
    .unwrap();
    (ticket, repository)
}
fn card(ticket: &Ticket, repository: &Repository, allowed: &[&str]) -> PublishCard {
    let allowed: Vec<String> = allowed.iter().map(|a| (*a).to_owned()).collect();
    let request = ticket.publishing.as_ref().unwrap();
    let ask = publish_ask(ticket, request, repository, &allowed).unwrap();
    publish_card(ticket, request, repository, ask)
}

#[test]
fn the_push_card_names_the_repository_branch_commit_and_review() {
    let (ticket, repository) = waiting_ticket("push", "passed");
    let card = card(&ticket, &repository, &[]);
    assert_eq!(card.title, "Push this branch to celadon-theme/website?");
    assert_eq!(
        card.rows,
        [
            (
                "Repository",
                "celadon-theme/website · public · celadon-theme is not an allowed owner".to_owned()
            ),
            (
                "Branch",
                "wiffletree/fix-palette-contrast · not on GitHub yet".to_owned()
            ),
            (
                "Commit",
                "9e03c1b · 2 commits · 3 files changed, 41 insertions(+), 7 deletions(-)"
                    .to_owned()
            ),
            (
                "Review",
                "Cycle 1 passed at this commit · Tests, Correctness".to_owned()
            ),
        ]
    );
    assert_eq!(
        card.note,
        "Wiffletree runs the push itself, for exactly this commit. If the branch has moved by the time you answer, nothing is pushed. Opening the pull request is a separate approval."
    );
    assert_eq!(card.approve, "Push once");
    assert!(card.description.is_none() && card.findings.is_empty() && card.would_send.is_none());
    // A branch the host pushed before, as when its pull request was then denied, is on GitHub.
    let (mut ticket, repository) = waiting_ticket("push", "passed");
    ticket.pushed = Some("4c2a9f1000".into());
    assert_eq!(
        self::card(&ticket, &repository, &[]).rows[1],
        ("Branch", "wiffletree/fix-palette-contrast".to_owned())
    );
}

fn pull_request(number: u64, state: &str) -> PullRequest {
    serde_json::from_value(
        json!({"number": number, "url": "u", "state": state, "draft": false,
            "base": "main", "head": "h", "merge_state": "clean", "checks": "success",
            "unresolved_threads": 0, "comments": {}}),
    )
    .unwrap()
}

#[test]
fn a_waiting_push_card_says_whether_the_pull_request_will_ask() {
    let (mut ticket, repository) = waiting_ticket("push", "passed");
    let note = |ticket: &Ticket, allowed: &[String]| {
        waiting_card(
            ticket,
            ticket.publishing.as_ref().unwrap(),
            &repository,
            allowed,
        )
        .note
    };
    let allowed = ["celadon-theme".to_owned()];
    assert!(note(&ticket, &[]).ends_with("Opening the pull request is a separate approval."));
    // The owner was ticked while the item waited: the pull request no longer asks.
    let ticked = waiting_card(
        &ticket,
        ticket.publishing.as_ref().unwrap(),
        &repository,
        &allowed,
    );
    assert_eq!(
        ticked.note,
        "Wiffletree runs the push itself, for exactly this commit. If the branch has moved by the time you answer, nothing is pushed. Wiffletree then opens the pull request without asking again."
    );
    assert_eq!(
        ticked.rows[0].1,
        "celadon-theme/website · public · celadon-theme is an allowed owner"
    );
    assert_eq!(ticked.approve, "Push once");
    // Another pull request after a closed one asks in every repository.
    ticket.pull_request = Some(pull_request(45, "closed"));
    assert!(note(&ticket, &allowed).ends_with("Opening the pull request is a separate approval."));
}

#[test]
fn the_pull_request_card_shows_the_title_and_description_in_full() {
    let (ticket, repository) = waiting_ticket("open_pull_request", "passed");
    let card = card(&ticket, &repository, &[]);
    assert_eq!(
        card.title,
        "Open this pull request in celadon-theme/website?"
    );
    assert_eq!(
        card.rows[1],
        (
            "Branches",
            "wiffletree/fix-palette-contrast → main".to_owned()
        )
    );
    assert_eq!(
        card.rows[3],
        ("Title", "Fix palette contrast in dark mode".to_owned())
    );
    assert_eq!(
        card.description.as_deref(),
        Some("Muted text fell below 4.5:1.\n- Darken the overlay")
    );
    assert_eq!(card.approve, "Open pull request");
    // The Inbox item's own text leaves the description to the card.
    assert!(!card.text().contains("Darken the overlay"));
}

#[test]
fn a_later_push_card_names_the_pull_request_it_updates() {
    let (mut ticket, repository) = waiting_ticket("push", "passed");
    ticket.pull_request = Some(pull_request(212, "open"));
    assert_eq!(ticket.open_pull_request().map(|pr| pr.number), Some(212));
    let request = ticket.publishing.as_mut().unwrap();
    (request.title, request.body) = (None, None);
    let card = card(&ticket, &repository, &[]);
    assert_eq!(card.title, "Push 2 new commits to celadon-theme/website?");
    assert_eq!(
        card.rows[1],
        (
            "Updates",
            "PR #212 · wiffletree/fix-palette-contrast".to_owned()
        )
    );
    assert!(!card.note.contains("pull request"));
    ticket.publishing.as_mut().unwrap().size = "1 commit · 1 file changed".into();
    assert_eq!(
        self::card(&ticket, &repository, &[]).title,
        "Push 1 new commit to celadon-theme/website?"
    );
}

#[test]
fn the_blocked_review_card_lists_the_open_findings_and_what_one_approval_would_send() {
    let (mut ticket, repository) = waiting_ticket("push", "blocked");
    let mut untriaged = entry("F6", EntryStatus::Untriaged);
    untriaged.finding.location = "crates/workspace-desktop/src/tree.rs:88".into();
    ticket.ledger = vec![entry("F2", EntryStatus::Fixed), untriaged];
    let card = card(&ticket, &repository, &["celadon-theme"]);
    assert_eq!(
        card.title,
        "Publish this ticket although its review ended blocked?"
    );
    assert_eq!(
        card.rows[0].1,
        "celadon-theme/website · public · celadon-theme is an allowed owner"
    );
    assert_eq!(
        card.rows.last().unwrap(),
        &("Review", "Cycle 1 ended blocked at 9e03c1b, after 1 of 3 rounds · Tests passed, Correctness failed".to_owned())
    );
    assert_eq!(
        card.findings,
        [(
            "F6 · non-blocking · Correctness · crates/workspace-desktop/src/tree.rs:88".to_owned(),
            "Matching is case-sensitive. Not triaged yet.".to_owned()
        )]
    );
    assert_eq!(
        card.would_send.as_deref(),
        Some(
            "Push wiffletree/fix-palette-contrast at 9e03c1b and open its pull request, “Fix palette contrast in dark mode”"
        )
    );
    assert_eq!(card.approve, "Publish anyway");
    assert!(card.description.is_some());
    // Where the owner is not allowed, the same notice and findings are on each step's card.
    let step = self::card(&ticket, &repository, &[]);
    assert_eq!(step.title, "Push this branch to celadon-theme/website?");
    assert_eq!(step.findings, card.findings);
    assert!(step.would_send.is_none());
}

#[test]
fn a_step_waits_only_where_the_rules_require() {
    let (mut ticket, repository) = waiting_ticket("push", "passed");
    let allowed = vec!["celadon-theme".to_owned()];
    let ask = |ticket: &Ticket, allowed: &[String]| {
        publish_ask(
            ticket,
            ticket.publishing.as_ref().unwrap(),
            &repository,
            allowed,
        )
    };
    assert_eq!(ask(&ticket, &[]), Some(PublishAsk::NotAllowed));
    assert_eq!(ask(&ticket, &allowed), None);
    // A blocked review asks even where the owner is allowed, until the human approved the commit.
    ticket.verification.as_mut().unwrap().outcome = VerificationOutcome::Blocked;
    assert_eq!(ask(&ticket, &allowed), Some(PublishAsk::BlockedReview));
    ticket.publishing.as_mut().unwrap().blocked_review_approved = true;
    assert_eq!(ask(&ticket, &allowed), None);
    assert_eq!(ask(&ticket, &[]), Some(PublishAsk::NotAllowed));
    // A closed or merged pull request asks before another is opened, and only then.
    let (mut ticket, _) = waiting_ticket("push", "passed");
    ticket.pull_request = Some(pull_request(45, "closed"));
    assert_eq!(ticket.open_pull_request(), None);
    assert_eq!(ask(&ticket, &allowed), None);
    ticket.publishing.as_mut().unwrap().step = PublishStep::OpenPullRequest;
    assert_eq!(ask(&ticket, &allowed), Some(PublishAsk::EndedPullRequest));
    assert_eq!(
        ended_pull_request_notice(&ticket).as_deref(),
        Some("PR #45 was closed without merging")
    );
}
