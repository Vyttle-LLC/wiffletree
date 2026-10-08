use rusqlite::Connection;
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};
use workspace_core::*;
use workspace_host::Host;

fn profile(provider: Provider, model: &str, effort: &str) -> ModelProfile {
    ModelProfile {
        provider,
        model: model.into(),
        effort: effort.into(),
    }
}
fn claude(model: &str, effort: &str) -> ModelProfile {
    profile(Provider::Claude, model, effort)
}
fn codex(model: &str, effort: &str) -> ModelProfile {
    profile(Provider::Codex, model, effort)
}
fn allowed(models: &[(&str, &str)]) -> Vec<AllowedModel> {
    models
        .iter()
        .map(|(model, effort)| AllowedModel {
            model: (*model).into(),
            effort: (*effort).into(),
        })
        .collect()
}

// Saved policy rows in every shape a store can hold from before model selection.

/// The first built-in: fixed on Codex Sol. Still the shape of the maintenance row.
fn codex_fixed(role: Role) -> Value {
    json!({"role":role,"mode":"fixed","default":codex("gpt-6.1-sol","medium"),
        "allowed":[codex("gpt-6.1-sol","low"),codex("gpt-6.1-sol","medium"),codex("gpt-6-astra","high")],
        "small":codex("gpt-6.1-sol","low"),"standard":codex("gpt-6.1-sol","medium"),
        "complex":codex("gpt-6-astra","high"),"turn_budget_minutes":null})
}
/// The later built-in: coordinators and implementers fixed on Opus.
fn opus_fixed(role: Role, effort: &str) -> Value {
    let mut policy = codex_fixed(role);
    policy["default"] = json!(claude("opus", effort));
    policy["small"] = json!(claude("opus", "low"));
    policy["standard"] = json!(claude("opus", "medium"));
    policy["complex"] = json!(claude("opus", "high"));
    for effort in ["low", "medium", "high"] {
        policy["allowed"]
            .as_array_mut()
            .unwrap()
            .push(json!(claude("opus", effort)));
    }
    policy
}
/// What the Models page saved: automatic, Big/Small for both providers.
fn big_small(
    role: Role,
    default: Provider,
    claude_pair: [(&str, &str); 2],
    codex_pair: [(&str, &str); 2],
) -> Value {
    let pair = |provider, [big, small]: [(&str, &str); 2]| json!({"provider":provider,"big":profile(provider,big.0,big.1),"small":profile(provider,small.0,small.1)});
    let profiles = [
        pair(Provider::Claude, claude_pair),
        pair(Provider::Codex, codex_pair),
    ];
    let primary = &profiles[usize::from(default == Provider::Codex)];
    let allowed: Vec<_> = profiles
        .iter()
        .flat_map(|p| [p["big"].clone(), p["small"].clone()])
        .collect();
    json!({"role":role,"mode":"automatic","default":primary["big"],"allowed":allowed,
        "small":primary["small"],"standard":primary["big"],"complex":primary["big"],
        "provider_profiles":profiles,"turn_budget_minutes":null})
}
/// Automatic with tiers but no Big/Small profiles.
fn automatic_tiers(role: Role) -> Value {
    json!({"role":role,"mode":"automatic","default":claude("opus","medium"),
        "allowed":[claude("sonnet","high"),claude("opus","medium"),claude("opus","high")],
        "small":claude("sonnet","high"),"standard":claude("opus","medium"),
        "complex":claude("opus","high"),"turn_budget_minutes":null})
}
/// The human's current data: Big/Small everywhere, reviewer on Codex, maintenance untouched.
fn current(role: Role) -> Value {
    let sol = [("gpt-6.1-sol", "high"), ("gpt-6.1-sol", "medium")];
    match role {
        Role::ProjectOrchestrator | Role::TaskOrchestrator => big_small(
            role,
            Provider::Claude,
            [("opus", "high"), ("opus", "medium")],
            sol,
        ),
        Role::Implementer => big_small(
            role,
            Provider::Claude,
            [("opus", "medium"), ("sonnet", "high")],
            sol,
        ),
        Role::Tester => big_small(
            role,
            Provider::Claude,
            [("opus", "high"), ("sonnet", "high")],
            sol,
        ),
        Role::Reviewer => big_small(
            role,
            Provider::Codex,
            [("opus", "high"), ("sonnet", "medium")],
            sol,
        ),
        Role::Maintenance => codex_fixed(role),
    }
}

/// Opens a store as it was before model selection: `setup` adds sessions on a fresh host, then
/// each role gets its legacy row and pins and migrations are forgotten.
fn seed(home: &Path, rows: impl Fn(Role) -> Value, setup: impl FnOnce(&mut Host)) -> Host {
    let mut host = Host::open(home).unwrap();
    setup(&mut host);
    drop(host);
    let db = Connection::open(home.join("workspace.sqlite3")).unwrap();
    for role in Role::ALL {
        db.execute(
            "INSERT INTO policies VALUES (?1,?2) ON CONFLICT(role) DO UPDATE SET data=excluded.data",
            rusqlite::params![
                serde_json::to_value(role).unwrap().as_str().unwrap(),
                rows(role).to_string()
            ],
        )
        .unwrap();
    }
    db.execute_batch(
        "DELETE FROM policy_migrations; DELETE FROM model_selection;
         UPDATE runtimes SET data=json_remove(data,'$.profile','$.selection');",
    )
    .unwrap();
    drop(db);
    Host::open(home).unwrap()
}
fn git_repo() -> std::path::PathBuf {
    let dir = tempfile::tempdir().unwrap().keep();
    for args in [
        vec!["init", "-q"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "Initial",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .current_dir(&dir)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    dir
}
fn stored(home: &Path) -> (String, Vec<String>, Vec<String>) {
    let db = Connection::open(home.join("workspace.sqlite3")).unwrap();
    let rows = |sql: &str| -> Vec<String> {
        db.prepare(sql)
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    (
        db.query_row("SELECT data FROM model_selection", [], |r| r.get(0))
            .unwrap(),
        rows("SELECT data FROM runtimes ORDER BY session_id"),
        rows("SELECT data FROM policies ORDER BY role"),
    )
}
fn providers(selection: &ModelSelection, role: Role) -> BTreeSet<Provider> {
    selection.role_providers[&role].clone()
}

#[test]
fn current_big_small_policies_migrate_without_loss() {
    let home = tempfile::tempdir().unwrap();
    let host = seed(home.path(), current, |_| {});
    let selection = host.model_selection().unwrap();
    selection.validate().unwrap();
    assert_eq!(selection.revision, 1);
    for role in PROVIDER_ROLES {
        assert_eq!(
            providers(&selection, role),
            BTreeSet::from([Provider::Claude]),
            "{role:?} keeps its default provider"
        );
    }
    // Every previously allowed profile, in any role, is still allowed under its provider.
    for role in Role::ALL {
        for allowed in current(role)["allowed"].as_array().unwrap() {
            let profile: ModelProfile = serde_json::from_value(allowed.clone()).unwrap();
            selection.permits(Role::Reviewer, &profile).unwrap();
        }
    }
    assert_eq!(
        selection.allowed(Provider::Claude),
        allowed(&[
            ("opus", "high"),
            ("opus", "medium"),
            ("sonnet", "high"),
            ("sonnet", "medium")
        ])
    );
    assert_eq!(
        selection.allowed(Provider::Codex),
        allowed(&[
            ("gpt-6.1-sol", "high"),
            ("gpt-6.1-sol", "medium"),
            ("gpt-6.1-sol", "low"),
            ("gpt-6-astra", "high")
        ])
    );
    let guide = &selection.guide;
    assert!(guide.starts_with(
        "Pick the model by the size and risk of the work. Claude: small, contained change → Sonnet; large or cross-cutting → Opus. Codex: small → GPT 6.1 Sol at medium; large → GPT 6.1 Sol at high. Raise effort for migrations, data or security changes.\n\n## Previous settings (migrated "
    ));
    for line in [
        "### Coordinators (main and repository)",
        "- Claude: large work → opus · high; small work → opus · medium.",
        "### Implementer",
        "- Claude: large work → opus · medium; small work → sonnet · high.",
        "### Tester",
        "- Claude: large work → opus · high; small work → sonnet · high.",
        "### Reviewer",
        "- Default: codex · gpt-6.1-sol · high (coordinators could choose within its list).",
        "- Claude: large work → opus · high; small work → sonnet · medium.",
        "- Codex: large work → gpt-6.1-sol · high; small work → gpt-6.1-sol · medium.",
        "### Maintenance",
        "- Was fixed to codex · gpt-6.1-sol · medium; coordinators could not choose another model.",
        "- Tiers: small work → codex · gpt-6.1-sol · low; standard → codex · gpt-6.1-sol · medium; complex → codex · gpt-6-astra · high.",
    ] {
        assert!(guide.contains(line), "guide is missing {line:?}:\n{guide}");
    }
}

#[test]
fn legacy_codex_store_takes_the_opus_migration_then_model_selection() {
    let home = tempfile::tempdir().unwrap();
    let host = seed(home.path(), codex_fixed, |_| {});
    let selection = host.model_selection().unwrap();
    for role in [Role::ProjectOrchestrator, Role::Implementer] {
        assert_eq!(
            providers(&selection, role),
            BTreeSet::from([Provider::Claude])
        );
    }
    assert_eq!(
        providers(&selection, Role::Tester),
        BTreeSet::from([Provider::Codex])
    );
    assert!(
        !selection
            .role_providers
            .contains_key(&Role::TaskOrchestrator)
    );
    assert_eq!(
        selection.allowed(Provider::Claude),
        allowed(&[("opus", "high"), ("opus", "medium"), ("opus", "low")])
    );
    assert_eq!(
        selection.prefill(Role::ProjectOrchestrator),
        Some(claude("opus", "high"))
    );
    assert!(selection.guide.contains(
        "### Tester\n- Was fixed to codex · gpt-6.1-sol · medium; coordinators could not choose another model.\n- Tiers: small work → codex · gpt-6.1-sol · low; standard → codex · gpt-6.1-sol · medium; complex → codex · gpt-6-astra · high."
    ));
}

#[test]
fn fixed_opus_custom_fixed_and_automatic_tiers_become_allowlist_and_prose() {
    let home = tempfile::tempdir().unwrap();
    let host = seed(
        home.path(),
        |role| match role {
            Role::ProjectOrchestrator | Role::TaskOrchestrator => opus_fixed(role, "high"),
            Role::Implementer => automatic_tiers(role),
            Role::Tester => {
                let mut custom = opus_fixed(role, "high");
                custom["default"] = json!(claude("sonnet", "high"));
                custom["allowed"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(claude("sonnet", "high")));
                custom
            }
            _ => codex_fixed(role),
        },
        |_| {},
    );
    let selection = host.model_selection().unwrap();
    assert_eq!(
        providers(&selection, Role::Tester),
        BTreeSet::from([Provider::Claude])
    );
    selection
        .permits(Role::Implementer, &claude("sonnet", "high"))
        .unwrap();
    let guide = &selection.guide;
    assert!(
        guide.contains(
            "### Coordinators (main and repository)\n- Was fixed to claude · opus · high"
        )
    );
    assert!(guide.contains(
        "### Implementer\n- Default: claude · opus · medium (coordinators could choose within its list).\n- Tiers: small work → claude · sonnet · high; standard → claude · opus · medium; complex → claude · opus · high."
    ));
    assert!(guide.contains("### Tester\n- Was fixed to claude · sonnet · high"));
    assert!(!guide.contains("Also listed"));
}

/// The largest settings the old validator accepted: 32 profiles per role, IDs at their length
/// limits. The migration must never refuse them, or the host could not open.
#[test]
fn the_largest_legacy_store_still_migrates() {
    let long = |role: Role, i: usize, width: usize| {
        let mut id = format!("{role:?}-{i}-");
        id.extend(std::iter::repeat_n('x', width - id.len()));
        id
    };
    let fixed_with_32 = |role: Role| {
        let allowed: Vec<_> = (0..32)
            .map(|i| claude(&long(role, i, 128), &long(role, i, 32)))
            .collect();
        json!({"role":role,"mode":"fixed","default":allowed[0],"allowed":allowed,
            "small":allowed[1],"standard":allowed[2],"complex":allowed[3],"turn_budget_minutes":null})
    };
    // Every role at 32 profiles on one provider: the per-provider maximum.
    let home = tempfile::tempdir().unwrap();
    let selection = seed(home.path(), fixed_with_32, |_| {})
        .model_selection()
        .unwrap();
    selection.validate().unwrap();
    assert_eq!(
        selection.allowed(Provider::Claude).len(),
        MAX_MODELS_PER_PROVIDER
    );
    // Long Big/Small profiles on every role: the longest guide.
    let home = tempfile::tempdir().unwrap();
    let selection = seed(
        home.path(),
        |role| {
            let pair = |provider: Provider, i| {
                [
                    profile(provider, &long(role, i, 128), &long(role, i, 32)),
                    profile(provider, &long(role, i + 1, 128), &long(role, i + 1, 32)),
                ]
            };
            let [claude_big, claude_small] = pair(Provider::Claude, 0);
            let [codex_big, codex_small] = pair(Provider::Codex, 2);
            json!({"role":role,"mode":"automatic","default":claude_big,
                "allowed":[claude_big,claude_small,codex_big,codex_small],
                "small":claude_small,"standard":claude_big,"complex":claude_big,
                "provider_profiles":[{"provider":"claude","big":claude_big,"small":claude_small},
                    {"provider":"codex","big":codex_big,"small":codex_small}],
                "turn_budget_minutes":null})
        },
        |_| {},
    )
    .model_selection()
    .unwrap();
    selection.validate().unwrap();
    assert!(
        selection.guide.len() <= MAX_GUIDE_BYTES,
        "{}",
        selection.guide.len()
    );
}

#[test]
fn rows_with_or_without_a_former_turn_budget_migrate() {
    for budget in [None, Some(json!(25))] {
        let home = tempfile::tempdir().unwrap();
        let host = seed(
            home.path(),
            |role| {
                let mut row = current(role);
                match &budget {
                    Some(minutes) => row["turn_budget_minutes"] = minutes.clone(),
                    None => {
                        row.as_object_mut().unwrap().remove("turn_budget_minutes");
                    }
                }
                row
            },
            |_| {},
        );
        let selection = host.model_selection().unwrap();
        selection.validate().unwrap();
        assert!(!selection.guide.contains("turn_budget"));
    }
}

#[test]
fn saved_verifier_sizes_become_guide_prose() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("settings.json"),
        json!({"workspaces_dir":"/w","verification":{"max_rounds":2,"verifiers":[
            {"role":"tester","focus":"Tests"},
            {"role":"reviewer","focus":"Codex","provider":"codex","size":"small"},
            {"role":"tester","focus":"Claude","size":"big"}]}})
        .to_string(),
    )
    .unwrap();
    let host = seed(home.path(), current, |_| {});
    let guide = host.model_selection().unwrap().guide;
    assert!(
        guide.contains(
            "### Verifiers\n- Reviewer · Codex: ran the small Codex profile, gpt-6.1-sol · medium.\n- Tester · Claude: ran the big Claude profile, opus · high.\n"
        ),
        "{guide}"
    );
    assert!(!guide.contains("Tester · Tests"));
    // The saved list itself is kept; only the size is no longer read.
    let verifiers = host.settings().verification.verifiers;
    assert_eq!(verifiers.len(), 3);
    assert_eq!(verifiers[1].provider, Some(Provider::Codex));
}

#[test]
fn unpinned_sessions_keep_the_model_they_would_have_used() {
    let home = tempfile::tempdir().unwrap();
    let mut ids = Vec::new();
    let mut host = seed(
        home.path(),
        |role| match role {
            Role::ProjectOrchestrator => opus_fixed(role, "high"),
            Role::Tester => automatic_tiers(role),
            _ => current(role),
        },
        |host| {
            for name in ["Fixed", "Mismatch"] {
                host.create_project(name).unwrap();
            }
            let roots = host.sessions().unwrap();
            let project = &roots[0].project_id;
            ids.push(roots[0].id.clone());
            // A Codex coordinator under a Claude-only fixed policy: today it cannot run.
            let mismatch = roots[1].clone();
            host.configure_session(&mismatch.id, codex("gpt-6.1-sol", "medium"))
                .unwrap();
            ids.push(mismatch.id);
            let dir = git_repo();
            host.set_workspaces_dir(tempfile::tempdir().unwrap().keep().to_str().unwrap())
                .unwrap();
            let repo = host
                .attach_repository(project, dir.to_str().unwrap(), "HEAD")
                .unwrap();
            let ticket = host
                .create_ticket(&roots[0].id, &repo.id, "Work", "Do it")
                .unwrap();
            let implementer = host
                .assign_ticket(&ticket.id, Role::Implementer, Provider::Codex, "Do", None)
                .unwrap();
            ids.push(implementer.id);
            let tester = host
                .assign_ticket(&ticket.id, Role::Tester, Provider::Claude, "Test", None)
                .unwrap();
            ids.push(tester.id);
        },
    );
    let profile = |host: &Host, id: &str| host.session_runtime(id).unwrap().profile;
    assert_eq!(
        profile(&host, &ids[0]),
        Some(claude("opus", "high")),
        "fixed → default"
    );
    assert_eq!(
        profile(&host, &ids[1]),
        None,
        "provider mismatch stays unpinned"
    );
    assert_eq!(
        profile(&host, &ids[2]),
        Some(codex("gpt-6.1-sol", "high")),
        "Big/Small → that provider's Big"
    );
    assert_eq!(
        profile(&host, &ids[3]),
        Some(claude("opus", "medium")),
        "automatic tiers → standard"
    );
    let selection = host.session_runtime(&ids[0]).unwrap().selection.unwrap();
    assert_eq!(selection.chosen_by, Chooser::Default);
    assert_eq!(selection.reason, "Kept the model it used before");
    // The mismatched session now runs on its provider's first allowed model.
    let mismatch = host.session(&ids[1]).unwrap();
    assert_eq!(
        host.turn_profile(&mismatch).unwrap(),
        codex("gpt-6.1-sol", "high")
    );
    // Pinned after the migration: untouched by reopening.
    host.configure_session(&ids[3], claude("sonnet", "medium"))
        .unwrap();
    drop(host);
    let host = Host::open(home.path()).unwrap();
    assert_eq!(profile(&host, &ids[3]), Some(claude("sonnet", "medium")));
}

#[test]
fn a_fresh_store_gets_todays_built_ins_converted() {
    let home = tempfile::tempdir().unwrap();
    let host = Host::open(home.path()).unwrap();
    let selection = host.model_selection().unwrap();
    assert_eq!(
        selection,
        ModelSelection {
            revision: 1,
            providers: [
                (
                    Provider::Claude,
                    ProviderAccess {
                        enabled: true,
                        models: allowed(&[("opus", "high"), ("opus", "medium"), ("opus", "low"), ("sonnet", "high"), ("sonnet", "medium")]),
                    },
                ),
                (
                    Provider::Codex,
                    ProviderAccess {
                        enabled: true,
                        models: allowed(&[("gpt-6.1-sol", "medium"), ("gpt-6.1-sol", "low"), ("gpt-6-astra", "high"), ("gpt-6.1-sol", "high")]),
                    },
                ),
            ]
            .into(),
            role_providers: [
                (Role::ProjectOrchestrator, BTreeSet::from([Provider::Claude])),
                (Role::Implementer, BTreeSet::from([Provider::Claude])),
                (Role::Tester, BTreeSet::from([Provider::Codex])),
            ]
            .into(),
            guide: "Pick the model by the size and risk of the work. Claude: small, contained change → Sonnet; large or cross-cutting → Opus. Codex: small → GPT 6.1 Sol at medium; large → GPT 6.1 Sol at high. Raise effort for migrations, data or security changes.\n".into(),
        }
    );
    // The roles the migration gives today's built-in policies; the allowlist adds only the
    // models the starter guide recommends.
    drop(host);
    let migrated = seed(
        tempfile::tempdir().unwrap().path(),
        |role| match role {
            Role::ProjectOrchestrator | Role::TaskOrchestrator => opus_fixed(role, "high"),
            Role::Implementer => opus_fixed(role, "medium"),
            _ => codex_fixed(role),
        },
        |_| {},
    )
    .model_selection()
    .unwrap();
    assert_eq!(migrated.role_providers, selection.role_providers);
    assert!(
        !migrated
            .allowed(Provider::Claude)
            .iter()
            .any(|m| m.model == "sonnet")
    );
    for provider in PROVIDERS {
        let set = |s: &ModelSelection| s.allowed(provider).iter().cloned().collect::<BTreeSet<_>>();
        assert!(set(&migrated).is_subset(&set(&selection)));
    }
}

#[test]
fn reopening_a_migrated_store_changes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let mut host = seed(home.path(), current, |host| {
        host.create_project("Existing").unwrap();
    });
    let mut edited = host.model_selection().unwrap();
    edited.guide.push_str("\nRaise effort for migrations.\n");
    edited.role_providers.insert(
        Role::Implementer,
        BTreeSet::from([Provider::Claude, Provider::Codex]),
    );
    host.set_model_selection(&edited).unwrap();
    drop(host);
    let before = stored(home.path());
    drop(Host::open(home.path()).unwrap());
    assert_eq!(stored(home.path()), before);
}

#[test]
fn saving_validates_the_whole_selection_and_bumps_the_revision() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open(home.path()).unwrap();
    let saved = host.model_selection().unwrap();
    let mut invalid = saved.clone();
    invalid.role_providers.insert(Role::Tester, BTreeSet::new());
    assert!(host.set_model_selection(&invalid).is_err());
    assert_eq!(host.model_selection().unwrap(), saved);
    let next = host.set_model_selection(&saved).unwrap();
    assert_eq!(next.revision, saved.revision + 1);
    assert_eq!(host.model_selection().unwrap(), next);
}

#[test]
fn chat_override_is_unlimited_and_preserves_provider_identity() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open(home.path()).unwrap();
    host.create_project("Chat overrides").unwrap();
    let root = host.sessions().unwrap().remove(0);
    let runtime = host.session_runtime(&root.id).unwrap();
    assert_eq!(runtime.profile, Some(claude("opus", "high")));
    assert_eq!(runtime.selection.unwrap().reason, "Role default");
    let unlisted = claude("sonnet", "max");
    assert!(
        host.model_selection()
            .unwrap()
            .permits(root.role, &unlisted)
            .is_err()
    );
    host.configure_session(&root.id, unlisted.clone()).unwrap();
    let runtime = host.session_runtime(&root.id).unwrap();
    assert_eq!(runtime.profile, Some(unlisted.clone()));
    assert_eq!(runtime.selection.unwrap().chosen_by, Chooser::Human);
    host.set_status(&root.id, Status::Working).unwrap();
    assert!(
        host.configure_session(&root.id, claude("opus", "high"))
            .is_err()
    );
    host.set_status(&root.id, Status::Ready).unwrap();
    drop(host);
    let db = Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    db.execute(
        "UPDATE runtimes SET data=json_set(data,'$.provider_session_id','provider-conversation') WHERE session_id=?1",
        [&root.id],
    )
    .unwrap();
    drop(db);
    let mut host = Host::open(home.path()).unwrap();
    assert_eq!(
        host.session_runtime(&root.id).unwrap().profile,
        Some(unlisted)
    );
    assert!(
        host.configure_session(&root.id, codex("gpt-6.1-sol", "medium"))
            .is_err()
    );
    assert_eq!(host.session(&root.id).unwrap().provider, Provider::Claude);
}
