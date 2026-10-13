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
        size: "2 commits · 3 files changed".into(),
        title: Some("Fix".into()),
        body: Some("Why".into()),
        step: PublishStep::OpenPullRequest,
        waiting: Some("attention".into()),
        blocked_review_approved: false,
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
