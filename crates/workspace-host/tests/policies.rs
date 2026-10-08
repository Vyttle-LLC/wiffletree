use rusqlite::Connection;
use workspace_core::*;
use workspace_host::Host;

fn legacy(role: Role) -> RolePolicy {
    let default = ModelProfile {
        provider: Provider::Codex,
        model: "gpt-6.1-sol".into(),
        effort: "medium".into(),
    };
    let small = ModelProfile {
        effort: "low".into(),
        ..default.clone()
    };
    let complex = ModelProfile {
        provider: Provider::Codex,
        model: "gpt-6-astra".into(),
        effort: "high".into(),
    };
    RolePolicy {
        role,
        mode: RoutingMode::Fixed,
        default: default.clone(),
        allowed: vec![small.clone(), default.clone(), complex.clone()],
        small,
        standard: default,
        complex,
        provider_profiles: Vec::new(),
        turn_budget_minutes: None,
    }
}

#[test]
fn stored_policies_without_a_turn_budget_load_with_their_role_default() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open(home.path()).unwrap();
    let mut custom = legacy(Role::TaskOrchestrator);
    custom.turn_budget_minutes = Some(25);
    host.set_policy(&custom).unwrap();
    drop(host);
    // Strip the field the way a store written before turn budgets has it.
    let db = Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    db.execute(
        "UPDATE policies SET data=json_remove(data,'$.turn_budget_minutes') WHERE role IN ('project_orchestrator','implementer')",
        [],
    )
    .unwrap();
    drop(db);
    let host = Host::open(home.path()).unwrap();
    let budget = |role| host.policy(role).unwrap().turn_budget();
    assert_eq!(budget(Role::ProjectOrchestrator), Some(10));
    assert_eq!(budget(Role::Implementer), None);
    assert_eq!(budget(Role::TaskOrchestrator), Some(25));
    let mut over = legacy(Role::ProjectOrchestrator);
    for minutes in [0, MAX_TURN_BUDGET_MINUTES + 1] {
        over.turn_budget_minutes = Some(minutes);
        assert!(over.validate().is_err(), "{minutes}");
    }
    // A worker's cut-off turn would complete uncertain write work instead of holding it.
    let mut worker = legacy(Role::Implementer);
    worker.turn_budget_minutes = Some(5);
    assert!(worker.validate().is_err());
    assert_eq!(worker.turn_budget(), None, "even if one was stored");
}

#[test]
fn opus_migration_preserves_custom_defaults_and_existing_agents_and_runs_once() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open(home.path()).unwrap();
    let original = legacy(Role::ProjectOrchestrator);
    host.set_policy(&original).unwrap();
    host.set_policy(&legacy(Role::Implementer)).unwrap();
    let mut custom = legacy(Role::TaskOrchestrator);
    custom.mode = RoutingMode::Automatic;
    host.set_policy(&custom).unwrap();
    host.create_project("Existing").unwrap();
    let root = host.sessions().unwrap().remove(0);
    assert_eq!(root.provider, Provider::Codex);
    assert!(host.session_runtime(&root.id).unwrap().profile.is_none());
    host.create_project("Configured").unwrap();
    let configured = host.sessions().unwrap().remove(1);
    host.configure_session(&configured.id, original.complex.clone())
        .unwrap();
    drop(host);
    let db = Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    db.execute("DELETE FROM policy_migrations", []).unwrap();
    drop(db);
    let mut host = Host::open(home.path()).unwrap();
    assert_eq!(
        host.policy(Role::ProjectOrchestrator)
            .unwrap()
            .default
            .model,
        "opus"
    );
    assert_eq!(
        host.policy(Role::Implementer).unwrap().default.model,
        "opus"
    );
    assert_eq!(host.policy(Role::TaskOrchestrator).unwrap(), custom);
    assert_eq!(host.session(&root.id).unwrap().provider, Provider::Codex);
    assert_eq!(
        host.session_runtime(&root.id).unwrap().profile,
        Some(original.default.clone())
    );
    assert_eq!(
        host.session_runtime(&configured.id).unwrap().profile,
        Some(original.complex.clone())
    );
    let new_project = host.create_project("New").unwrap();
    assert_eq!(
        host.sessions()
            .unwrap()
            .iter()
            .find(|s| s.project_id == new_project.id)
            .unwrap()
            .provider,
        Provider::Claude
    );
    host.set_policy(&original).unwrap();
    drop(host);
    let host = Host::open(home.path()).unwrap();
    assert_eq!(host.policy(Role::ProjectOrchestrator).unwrap(), original);
}

#[test]
fn chat_override_is_independent_of_role_defaults_and_preserves_provider_identity() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open(home.path()).unwrap();
    host.create_project("Chat overrides").unwrap();
    let root = host.sessions().unwrap().remove(0);
    let original = host.policy(root.role).unwrap();
    assert_eq!(original.default.effort, "high");
    let profile = ModelProfile {
        provider: Provider::Claude,
        model: "sonnet".into(),
        effort: "high".into(),
    };
    assert!(!original.allowed.contains(&profile));
    host.configure_session(&root.id, profile.clone()).unwrap();
    assert_eq!(host.policy(root.role).unwrap(), original);
    host.set_status(&root.id, Status::Working).unwrap();
    assert!(
        host.configure_session(&root.id, original.default.clone())
            .is_err()
    );
    host.set_status(&root.id, Status::Ready).unwrap();
    drop(host);
    let db = Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    let mut runtime: SessionRuntime = serde_json::from_str(
        &db.query_row(
            "SELECT data FROM runtimes WHERE session_id=?1",
            [&root.id],
            |r| r.get::<_, String>(0),
        )
        .unwrap(),
    )
    .unwrap();
    runtime.provider_session_id = Some("provider-conversation".into());
    db.execute(
        "UPDATE runtimes SET data=?2 WHERE session_id=?1",
        rusqlite::params![root.id, serde_json::to_string(&runtime).unwrap()],
    )
    .unwrap();
    drop(db);
    let mut host = Host::open(home.path()).unwrap();
    assert_eq!(
        host.session_runtime(&root.id).unwrap().profile,
        Some(profile)
    );
    assert!(
        host.configure_session(&root.id, legacy(root.role).default)
            .is_err()
    );
    assert_eq!(host.session(&root.id).unwrap().provider, Provider::Claude);
}

#[test]
fn four_role_defaults_save_atomically_and_keep_existing_agents() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open(home.path()).unwrap();
    host.create_project("Existing defaults").unwrap();
    let root = host.sessions().unwrap().remove(0);
    let roles = [
        Role::ProjectOrchestrator,
        Role::Implementer,
        Role::Tester,
        Role::Reviewer,
    ];
    let defaults: Vec<_> = roles
        .into_iter()
        .map(|role| RoleDefault {
            role,
            default_provider: Provider::Claude,
            profiles: [Provider::Claude, Provider::Codex]
                .into_iter()
                .map(|provider| {
                    let big = ModelProfile {
                        provider,
                        model: if provider == Provider::Claude {
                            "opus"
                        } else {
                            "gpt-6.1-sol"
                        }
                        .into(),
                        effort: "high".into(),
                    };
                    ProviderProfiles {
                        provider,
                        small: ModelProfile {
                            effort: "medium".into(),
                            ..big.clone()
                        },
                        big,
                    }
                })
                .collect(),
        })
        .collect();
    let mut custom = host.policy(Role::TaskOrchestrator).unwrap();
    custom.turn_budget_minutes = Some(3);
    host.set_policy(&custom).unwrap();
    let previous = host.policies().unwrap();
    let mut invalid = defaults.clone();
    invalid[3].profiles[1].small.effort.clear();
    assert!(host.set_role_defaults(&invalid).is_err());
    assert_eq!(host.policies().unwrap(), previous);
    invalid = defaults.clone();
    invalid[3].role = Role::Tester;
    assert!(host.set_role_defaults(&invalid).is_err());
    assert_eq!(host.policies().unwrap(), previous);
    host.set_role_defaults(&defaults).unwrap();
    assert_eq!(
        host.policy(Role::TaskOrchestrator).unwrap().default,
        defaults[0].profiles[0].big
    );
    assert_eq!(
        host.session_runtime(&root.id).unwrap().profile,
        Some(
            previous
                .iter()
                .find(|p| p.role == root.role)
                .unwrap()
                .default
                .clone()
        )
    );
    let project = host.create_project("New default").unwrap();
    let new_root = host
        .sessions()
        .unwrap()
        .into_iter()
        .find(|s| s.project_id == project.id)
        .unwrap();
    assert_eq!(
        host.policy(new_root.role).unwrap().default,
        defaults[0].profiles[0].big
    );
    let budget = |role| host.policy(role).unwrap().turn_budget();
    assert_eq!(
        budget(Role::TaskOrchestrator),
        Some(3),
        "defaults keep budgets"
    );
    assert_eq!(budget(Role::ProjectOrchestrator), Some(10));
    assert_eq!(budget(Role::Tester), None);
    drop(host);
    let host = Host::open(home.path()).unwrap();
    for entry in defaults {
        let policy = host.policy(entry.role).unwrap();
        assert_eq!(policy.default, entry.profiles[0].big);
        for provider in entry.profiles {
            assert_eq!(
                policy
                    .select(Complexity::Small, None, Some(provider.provider), None)
                    .unwrap()
                    .profile,
                provider.small
            );
            assert_eq!(
                policy
                    .select(Complexity::Complex, None, Some(provider.provider), None)
                    .unwrap()
                    .profile,
                provider.big
            );
        }
        let luna = ModelProfile {
            provider: Provider::Codex,
            model: "gpt-6-luna".into(),
            effort: "medium".into(),
        };
        assert!(
            policy
                .select(Complexity::Small, Some(&luna), Some(Provider::Codex), None)
                .is_err()
        );
        let mut widened = policy;
        widened.allowed.push(luna);
        assert!(widened.validate().is_err());
    }
    assert_eq!(
        host.session_runtime(&root.id)
            .unwrap()
            .profile
            .unwrap()
            .model,
        "opus"
    );
}
