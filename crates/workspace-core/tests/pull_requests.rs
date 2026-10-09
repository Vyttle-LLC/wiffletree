use serde_json::json;
use workspace_core::*;

fn pull_request() -> PullRequest {
    PullRequest {
        number: 142,
        url: "https://github.com/o/r/pull/142".into(),
        state: PrState::Open,
        draft: false,
        base: "main".into(),
        head: "9e03c1b7".into(),
        merge_state: MergeState::HasHooks,
        checks: CheckState::Failure,
        unresolved_threads: 1,
        comments: CommentCursors {
            issue: Some(7),
            review: None,
            inline: Some(3),
        },
        merge_commit: None,
    }
}

#[test]
fn a_pull_request_round_trips_in_snake_case() {
    let pr = pull_request();
    let value = serde_json::to_value(&pr).unwrap();
    assert_eq!(value["state"], "open");
    assert_eq!(value["merge_state"], "has_hooks");
    assert_eq!(value["checks"], "failure");
    assert_eq!(serde_json::from_value::<PullRequest>(value).unwrap(), pr);
}

#[test]
fn an_unknown_merge_state_decodes_as_unknown() {
    let mut value = serde_json::to_value(pull_request()).unwrap();
    value["merge_state"] = json!("queued_for_something_new");
    let pr: PullRequest = serde_json::from_value(value).unwrap();
    assert_eq!(pr.merge_state, MergeState::Unknown);
}

#[test]
fn changes_name_what_moved() {
    let old = pull_request();
    let mut new = old.clone();
    new.head = "3daf610aa".into();
    new.checks = CheckState::Success;
    assert_eq!(
        PullRequest::changes(Some(&old), &new),
        "new head 3daf610, checks success"
    );
    assert_eq!(PullRequest::changes(None, &old), "found, open");
}

#[test]
fn a_snapshot_stored_before_pull_request_checks_decodes() {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "projects":[],"repositories":[],"sessions":[],"attention":[]
    }))
    .unwrap();
    assert!(snapshot.pull_request_checks.is_empty());
}

#[test]
fn only_pr_ids_are_host_notices() {
    assert_eq!(
        host_notice("pr:t:merged:141"),
        Some("Wiffletree PR watcher")
    );
    for id in ["timer:s:1", "verification:t:1:outcome", "output:r", "xpr:t"] {
        assert_eq!(host_notice(id), None, "{id}");
    }
}
