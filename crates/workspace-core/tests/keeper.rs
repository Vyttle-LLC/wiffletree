use serde_json::json;
use workspace_core::*;

#[test]
fn settings_and_tickets_stored_before_the_keeper_decode_with_everything_off() {
    let settings: HostSettings = serde_json::from_value(json!({"workspaces_dir": "/w"})).unwrap();
    assert_eq!(settings.keeper, KeeperSettings::default());
    assert!(!settings.keeper.rebase && !settings.keeper.is_on());
    assert!(!settings.keeper.conflicts && !settings.keeper.checks);
    assert!(settings.keeper.review_level.is_none());
    assert_eq!(settings.keeper.quiet_minutes, 2);
    let ticket: Ticket = serde_json::from_value(json!({
        "id":"t","coordinator_id":"c","repository_id":"r","title":"T","brief":"B",
        "worktree":"/w","branch":"wiffletree/t","state":"passed"
    }))
    .unwrap();
    assert_eq!(ticket.keeper, None);
    let repository: Repository =
        serde_json::from_value(json!({"id":"r","name":"app","path":"/r","base":"origin/main"}))
            .unwrap();
    assert_eq!(repository.checks, None);
    assert!(
        serde_json::to_value(&ticket)
            .unwrap()
            .get("keeper")
            .is_none()
    );
}

#[test]
fn the_quiet_period_accepts_one_to_sixty_minutes() {
    let quiet = |quiet_minutes| KeeperSettings {
        rebase: true,
        quiet_minutes,
        comments: false,
        ..KeeperSettings::default()
    };
    assert!(quiet(1).validate().is_ok() && quiet(60).validate().is_ok());
    assert!(quiet(0).validate().is_err() && quiet(61).validate().is_err());
}

#[test]
fn keeper_messages_come_from_the_wiffletree_pr_keeper() {
    assert_eq!(
        host_notice("keeper:t:push:e21f0aa:4a1c2e9"),
        Some("Wiffletree PR keeper")
    );
    assert_eq!(host_notice("xkeeper:t"), None);
}

#[test]
fn saving_the_keeper_is_a_client_command() {
    let command = Command::SetKeeper {
        keeper: KeeperSettings::default(),
    };
    assert_eq!(
        serde_json::to_value(&command).unwrap(),
        json!({"type":"set_keeper","keeper":{"rebase":false,"quiet_minutes":2,"comments":false,"conflicts":false,"checks":false}})
    );
}

#[test]
fn rows_say_kept_or_not_kept_only_while_a_trigger_is_on() {
    let repository = |owner: &str| -> Repository {
        serde_json::from_value(json!({
            "id": owner, "name": owner, "path": "/r", "base": "origin/main",
            "github": {"owner": owner, "name": "app", "facts": {
                "name_with_owner": format!("{owner}/app"), "organization": true,
                "fork": false, "public": true, "checked_at": 1}}
        }))
        .unwrap()
    };
    let allowed = ["Vyttle-LLC".to_owned()];
    let on = KeeperSettings {
        rebase: true,
        quiet_minutes: 2,
        comments: false,
        ..KeeperSettings::default()
    };
    let (wiffletree, celadon) = (repository("Vyttle-LLC"), repository("celadon-theme"));
    assert_eq!(kept_label(&wiffletree, &allowed, &on), Some("kept"));
    assert_eq!(kept_label(&celadon, &allowed, &on), Some("not kept"));
    let sorting = KeeperSettings {
        comments: true,
        ..KeeperSettings::default()
    };
    assert_eq!(kept_label(&wiffletree, &allowed, &sorting), Some("kept"));
    let off = KeeperSettings::default();
    assert_eq!(kept_label(&wiffletree, &allowed, &off), None);
    assert_eq!(kept_label(&celadon, &allowed, &off), None);
}

#[test]
fn sorting_needs_a_claude_model() {
    let model = |provider| JobRow {
        job: Job::Keeper,
        main: Line {
            first: ModelProfile {
                provider,
                model: "m".into(),
                effort: "low".into(),
            },
            then: None,
        },
        small: None,
    };
    let none = check_sorting_model(None).unwrap_err().to_string();
    assert!(
        none.contains("choose a Claude model in Models first"),
        "{none}"
    );
    let codex = check_sorting_model(Some(&model(Provider::Codex)));
    assert!(
        codex
            .unwrap_err()
            .to_string()
            .starts_with("Sorting comments needs a Claude model")
    );
    assert!(check_sorting_model(Some(&model(Provider::Claude))).is_ok());
}
