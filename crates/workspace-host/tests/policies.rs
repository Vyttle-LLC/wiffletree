mod common;
use common::signed_in;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::path::Path;
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
/// Each row as "job: first → then", with "wait" for no then choice.
fn rows(selection: &ModelSelection) -> Vec<String> {
    (selection.jobs.iter())
        .map(|row| {
            let then = (row.main.then.model()).map_or("wait".into(), ToString::to_string);
            format!("{}: {} → {then}", row.job.name(), row.main.first)
        })
        .collect()
}
const MIGRATED: &str = "Set up from your previous settings. Check it.";

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
        Role::Maintenance | Role::Keeper => codex_fixed(role),
    }
}

/// Opens a store as it was before model selection: `setup` adds sessions on a fresh host, then
/// each role gets its legacy row, pins and migrations are forgotten, and the shared folder,
/// which came later, is removed.
fn seed(home: &Path, rows: impl Fn(Role) -> Value, setup: impl FnOnce(&mut Host)) -> Host {
    seed_detecting(home, rows, setup, &signed_in(&PROVIDERS))
}
/// `seed`, where the upgrade to the table finds the CLIs as `detect` says.
fn seed_detecting(
    home: &Path,
    rows: impl Fn(Role) -> Value,
    setup: impl FnOnce(&mut Host),
    detect: &dyn Fn(Provider) -> ProviderReport,
) -> Host {
    let mut host = Host::open_set_up(home).unwrap();
    setup(&mut host);
    drop(host);
    let db = Connection::open(home.join("workspace.sqlite3")).unwrap();
    // Legacy stores predate the PR keeper, which never had a policy row.
    for role in Role::ALL.into_iter().filter(|&r| r != Role::Keeper) {
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
    std::fs::remove_dir_all(home.join("shared")).unwrap();
    Host::open_detecting(home, detect).unwrap()
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

#[test]
fn current_big_small_policies_become_the_approved_table() {
    let home = tempfile::tempdir().unwrap();
    let host = seed(home.path(), current, |_| {});
    let selection = host.model_selection().unwrap();
    selection.validate().unwrap();
    assert_eq!(
        selection.revision, 2,
        "the selection's migration, then the table's"
    );
    // No PR keeper model was chosen, so there is no PR keeper row; the default reviewers were
    // Claude's and Codex's second opinions, so they wait.
    assert_eq!(
        rows(&selection),
        [
            "Coordinator: claude · opus · high → codex · gpt-6.1-sol · high",
            "Implementer: claude · opus · medium → codex · gpt-6.1-sol · high",
            "Tester: claude · sonnet · high → codex · gpt-6.1-sol · medium",
            "Tests: claude · sonnet · high → codex · gpt-6.1-sol · medium",
            "Correctness: claude · opus · high → wait",
            "Regressions: codex · gpt-6.1-sol · high → wait",
        ]
    );
    let implementer = selection.row(&Job::Implementer).unwrap();
    assert_eq!(
        implementer.small.as_ref().unwrap().first,
        claude("sonnet", "high")
    );
    // The tiers' prose is gone; the rest of the guide stays.
    let guide = &selection.guide;
    assert!(guide.starts_with(STARTER_GUIDE), "{guide}");
    assert!(!guide.contains("Previous settings"), "{guide}");
    // What the table does not hold is in the one-time note.
    assert_eq!(selection.notes[0], MIGRATED);
    for line in [
        "Allowed before: Claude opus at high, medium.",
        "Allowed before: Claude sonnet at high, medium.",
        "Allowed before: Codex gpt-6.1-sol at high, medium, low.",
        "Allowed before: Codex gpt-6-astra at high.",
        "Coordinator used Claude before.",
        "Tester used Claude before.",
        // The guide's previous-settings section moved into the note.
        "From the guide: - Claude: large work → opus · medium; small work → sonnet · high.",
    ] {
        assert!(
            selection.notes.iter().any(|n| n == line),
            "{line}: {:?}",
            selection.notes
        );
    }
    // Re-running changes nothing, and the table survives a restart.
    drop(host);
    let again = Host::open(home.path()).unwrap().model_selection().unwrap();
    assert_eq!(again, selection);
}

#[test]
fn legacy_codex_store_takes_the_opus_migration_then_the_catalogs_models() {
    let home = tempfile::tempdir().unwrap();
    let host = seed(home.path(), codex_fixed, |_| {});
    let selection = host.model_selection().unwrap();
    selection.validate().unwrap();
    let first = |job: Job| selection.row(&job).unwrap().main.first.clone();
    assert_eq!(first(Job::Coordinator), claude("opus", "high"));
    assert_eq!(first(Job::Implementer), claude("opus", "medium"));
    // Sonnet was never allowed here, but the signed-in CLI lists it.
    assert_eq!(first(Job::Tester), claude("sonnet", "high"));
    let tester = selection.row(&Job::Tester).unwrap();
    assert_eq!(
        tester.main.then,
        Then::Model(codex("gpt-6.1-sol", "medium"))
    );
}

#[test]
fn a_store_with_one_provider_runs_every_row_on_it() {
    let home = tempfile::tempdir().unwrap();
    let host = seed_detecting(
        home.path(),
        current,
        |_| {},
        &signed_in(&[Provider::Claude]),
    );
    let selection = host.model_selection().unwrap();
    selection.validate().unwrap();
    for row in &selection.jobs {
        assert_eq!(row.main.first.provider, Provider::Claude, "{:?}", row.job);
        assert_eq!(row.main.then, Then::Wait, "{:?}", row.job);
    }
    // With no CLI signed in at the upgrade, the providers that were enabled are used.
    let home = tempfile::tempdir().unwrap();
    let host = seed_detecting(home.path(), automatic_tiers, |_| {}, &signed_in(&[]));
    let selection = host.model_selection().unwrap();
    selection.validate().unwrap();
    for row in &selection.jobs {
        assert_eq!(row.main.first.provider, Provider::Claude, "{:?}", row.job);
        assert_eq!(row.main.then, Then::Wait, "{:?}", row.job);
    }
    assert_eq!(
        selection.row(&Job::Coordinator).unwrap().main.first,
        claude("opus", "high")
    );
}

/// The largest settings the old validator accepted: 32 profiles per role, IDs at their length
/// limits. The migrations must never refuse them, or the host could not open.
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
    let home = tempfile::tempdir().unwrap();
    let selection = seed(home.path(), fixed_with_32, |_| {})
        .model_selection()
        .unwrap();
    selection.validate().unwrap();
    assert!(selection.notes.len() > 1);
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
    assert!(selection.guide.len() <= MAX_GUIDE_BYTES);
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
fn a_verifier_saved_with_a_provider_becomes_that_providers_waiting_lens() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("settings.json"),
        json!({"workspaces_dir":"/w","verification":{"max_rounds":2,"verifiers":[
            {"role":"tester","focus":"Tests"},
            {"role":"reviewer","focus":"Codex","provider":"codex","size":"small"},
            {"role":"reviewer","focus":"Style"}]}})
        .to_string(),
    )
    .unwrap();
    let host = seed(home.path(), current, |_| {});
    let selection = host.model_selection().unwrap();
    let row = |focus: &str| {
        let job = Job::Verifier {
            focus: focus.into(),
        };
        selection.row(&job).unwrap().main.clone()
    };
    assert_eq!(row("Codex").first, codex("gpt-6.1-sol", "high"));
    assert_eq!(row("Codex").then, Then::Wait, "it waits for Codex");
    assert_eq!(row("Style").first, codex("gpt-6.1-sol", "medium"));
    assert_eq!(row("Style").then, Then::Model(claude("sonnet", "medium")));
    assert_eq!(row("Tests").first, claude("sonnet", "high"));
    // The saved list itself is kept; only the size and provider are no longer read.
    assert_eq!(host.settings().verification.verifiers.len(), 3);
}

#[test]
fn a_saved_keeper_model_becomes_the_keeper_rows_first_choice() {
    let home = tempfile::tempdir().unwrap();
    let host = seed(home.path(), current, |_| {});
    drop(host);
    // Give the converted selection a keeper model, as a store from the last release had.
    let db = Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    let saved: String = db
        .query_row("SELECT data FROM model_selection", [], |r| r.get(0))
        .unwrap();
    let mut legacy: Value = serde_json::from_str(&saved).unwrap();
    assert!(legacy.get("jobs").is_some());
    legacy = json!({"revision":1,"guide":"Mine.\n","keeper":claude("sonnet","low"),
        "providers":{"claude":{"enabled":true,"models":[{"model":"sonnet","effort":"low"}]},
            "codex":{"enabled":true,"models":[{"model":"gpt-6.1-sol","effort":"medium"}]}},
        "role_providers":{"implementer":["claude"]}});
    db.execute_batch("DELETE FROM policy_migrations WHERE id='who-runs-what'")
        .unwrap();
    db.execute("UPDATE model_selection SET data=?1", [legacy.to_string()])
        .unwrap();
    drop(db);
    let host = Host::open_detecting(home.path(), &signed_in(&PROVIDERS)).unwrap();
    let keeper = host
        .model_selection()
        .unwrap()
        .keeper()
        .unwrap()
        .main
        .clone();
    assert_eq!(keeper.first, claude("sonnet", "low"));
    assert_eq!(keeper.then, Then::Model(codex("gpt-6.1-sol", "medium")));
    let notes = host.model_selection().unwrap().notes;
    assert!(
        notes
            .iter()
            .any(|n| n == "The PR keeper used Claude sonnet at low before.")
    );
}

#[test]
fn signed_in_clis_and_their_catalogs_seed_the_table_and_every_old_setting_is_noted() {
    let home = tempfile::tempdir().unwrap();
    drop(seed(home.path(), current, |_| {}));
    // A selection from the last release: a retired Claude model, Codex disabled though its CLI
    // is signed in now, and the section an earlier upgrade wrote into the guide.
    let guide = "Mine.\n\n## Previous settings (migrated 2026-10-09)\n\n### Tester\n- Claude: small work → sonnet · low.\n";
    let legacy = json!({"revision":4,"guide":guide,"keeper":claude("retired-sonnet-model","medium"),
        "providers":{"claude":{"enabled":true,"models":[{"model":"retired-opus-model","effort":"high"}]},
            "codex":{"enabled":false,"models":[{"model":"gpt-6.1-sol","effort":"high"}]}},
        "role_providers":{"tester":["claude"]}});
    let db = Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    db.execute_batch("DELETE FROM policy_migrations WHERE id='who-runs-what'")
        .unwrap();
    db.execute("UPDATE model_selection SET data=?1", [legacy.to_string()])
        .unwrap();
    drop(db);
    std::fs::write(home.path().join("shared/settings/model-guide.md"), guide).unwrap();
    let host = Host::open_detecting(home.path(), &signed_in(&PROVIDERS)).unwrap();
    let selection = host.model_selection().unwrap();
    selection.validate().unwrap();
    let coordinator = selection.row(&Job::Coordinator).unwrap();
    assert_eq!(coordinator.main.first, claude("opus", "high"));
    assert_eq!(
        coordinator.main.then,
        Then::Model(codex("gpt-6.1-sol", "high"))
    );
    assert_eq!(selection.guide, "Mine.\n");
    // A retired PR keeper model moves to the closest one the CLI lists, and the note keeps it.
    let keeper = selection.keeper().unwrap();
    assert_eq!(keeper.main.first, claude("sonnet", "medium"));
    for line in [
        "Allowed before: Claude retired-opus-model at high.",
        "Allowed before: Codex gpt-6.1-sol at high, then disabled.",
        "Tester used Claude before.",
        "From the guide: ### Tester",
        "From the guide: - Claude: small work → sonnet · low.",
        "The PR keeper used Claude retired-sonnet-model at medium before.",
    ] {
        assert!(
            selection.notes.iter().any(|n| n == line),
            "{line}: {:?}",
            selection.notes
        );
    }
    // Re-running changes nothing.
    drop(host);
    assert_eq!(
        Host::open(home.path()).unwrap().model_selection().unwrap(),
        selection
    );
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
    // The mismatched session runs on the Coordinator row's choice on its own provider.
    let mismatch = host.session(&ids[1]).unwrap();
    assert_eq!(
        host.turn_profile(&mismatch).unwrap(),
        codex("gpt-6.1-sol", "high")
    );
    // Pinned after the migration: untouched by reopening.
    host.configure_session(&ids[3], claude("sonnet", "medium"))
        .unwrap();
    drop(host);
    let host = Host::open_set_up(home.path()).unwrap();
    assert_eq!(profile(&host, &ids[3]), Some(claude("sonnet", "medium")));
}

fn detected(signed_in: &[Provider]) -> Detection {
    signed_in
        .iter()
        .map(|&provider| {
            let status = ProviderStatus {
                installed: true,
                signed_in: true,
                plan: None,
            };
            (provider, status)
        })
        .collect()
}
fn save(host: &mut Host, setup: &Setup) -> anyhow::Result<Value> {
    host.execute(Command::SetModelSelection {
        selection: Box::new(setup.selection.clone()),
        verification: Some(setup.verification.clone()),
    })
}

#[test]
fn a_fresh_store_saves_nothing_and_starts_no_agent_until_a_setup_is_saved() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open(home.path()).unwrap();
    assert_eq!(host.model_selection().unwrap(), ModelSelection::default());
    let refused = host.create_project("Too early").unwrap_err().to_string();
    assert!(refused.contains("Open Models"), "{refused}");
    assert!(host.snapshot().unwrap().sessions.is_empty());
    // Reopening saves nothing either.
    drop(host);
    let mut host = Host::open(home.path()).unwrap();
    let db = Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    let rows: i64 = db
        .query_row("SELECT COUNT(*) FROM model_selection", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);
    assert!(!home.path().join("settings.json").exists());

    // With neither CLI signed in the suggestion cannot be saved, so nothing can start.
    assert!(save(&mut host, &suggest(&detected(&[]), &fallback_catalog())).is_err());
    assert!(host.create_project("Still too early").is_err());
    assert!(!home.path().join("settings.json").exists());

    for signed_in in [&[Provider::Claude][..], &[Provider::Codex], &PROVIDERS] {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        let setup = suggest(&detected(signed_in), &fallback_catalog());
        let provider = signed_in[0];
        assert_eq!(setup.verification.verifiers.len(), signed_in.len() + 1);
        save(&mut host, &setup).unwrap();
        // Exactly what was shown is saved, verifiers included, and survives a restart.
        drop(host);
        let mut host = Host::open(home.path()).unwrap();
        let saved = host.model_selection().unwrap();
        assert_eq!(
            saved,
            ModelSelection {
                revision: 1,
                ..setup.selection.clone()
            }
        );
        assert_eq!(host.settings().verification, setup.verification);
        // The app's host learns which CLIs are signed in as it starts.
        for (provider, status) in detected(signed_in) {
            host.record_detection(provider, &status);
        }
        host.create_project("Ready").unwrap();
        let coordinator = host.sessions().unwrap().remove(0);
        assert_eq!(coordinator.provider, provider);
        assert_eq!(host.turn_profile(&coordinator).unwrap().provider, provider);
    }
}

#[test]
fn a_refused_first_save_keeps_the_verifiers_unsaved_too() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open(home.path()).unwrap();
    let mut setup = suggest(&detected(&PROVIDERS), &fallback_catalog());
    setup.verification.verifiers.clear();
    assert!(save(&mut host, &setup).is_err());
    assert!(!host.model_selection().unwrap().is_saved());
    let mut setup = suggest(&detected(&PROVIDERS), &fallback_catalog());
    setup.selection.jobs.clear();
    assert!(save(&mut host, &setup).is_err());
    assert!(!home.path().join("shared/settings/review.json").exists());
}

fn refuse_selection_writes(home: &Path) {
    Connection::open(home.join("workspace.sqlite3"))
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER refuse BEFORE INSERT ON model_selection
             BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
        )
        .unwrap();
}

#[test]
fn a_first_save_whose_selection_write_fails_writes_no_shared_file() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open(home.path()).unwrap();
    refuse_selection_writes(home.path());
    assert!(
        save(
            &mut host,
            &suggest(&detected(&PROVIDERS), &fallback_catalog())
        )
        .is_err()
    );
    assert!(!host.model_selection().unwrap().is_saved());
    for file in ["review.json", "model-guide.md", "models.json"] {
        assert!(!home.path().join("shared/settings").join(file).exists());
    }
}

#[test]
fn a_combined_save_whose_selection_write_fails_puts_the_review_settings_and_guide_back() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    host.set_verification(VerificationSettings::default())
        .unwrap();
    let files = || {
        ["review.json", "model-guide.md", "models.json"]
            .map(|file| std::fs::read(home.path().join("shared/settings").join(file)).unwrap())
    };
    let before = (files(), host.model_selection().unwrap());
    refuse_selection_writes(home.path());
    let mut setup = suggest(&detected(&[Provider::Claude]), &fallback_catalog());
    setup.selection.guide = "A new guide.".into();
    setup.selection.switch_at = 90;
    setup.verification.max_rounds += 1;
    assert!(save(&mut host, &setup).is_err());
    assert_eq!((files(), host.model_selection().unwrap()), before);
}

#[test]
fn a_combined_save_refused_for_the_keeper_changes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let mut selection = host.model_selection().unwrap();
    let first = claude("sonnet", "medium");
    selection.set_keeper(Some(Line {
        first,
        then: Then::Wait,
    }));
    host.set_model_selection(&selection).unwrap();
    host.set_keeper(KeeperSettings {
        comments: true,
        ..KeeperSettings::default()
    })
    .unwrap();
    let before = (host.settings(), host.model_selection().unwrap());
    let mut setup = suggest(&detected(&PROVIDERS), &fallback_catalog());
    setup.selection.set_keeper(None);
    setup.selection.guide = "A new guide.".into();
    setup.verification.max_rounds += 1;
    let refused = save(&mut host, &setup).unwrap_err().to_string();
    assert!(refused.contains("PR keeper model"), "{refused}");
    assert_eq!((host.settings(), host.model_selection().unwrap()), before);
}

#[test]
fn reopening_a_migrated_store_changes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let mut host = seed(home.path(), current, |host| {
        host.create_project("Existing").unwrap();
    });
    let mut edited = host.model_selection().unwrap();
    edited.guide.push_str("\nRaise effort for migrations.\n");
    edited.row_mut(&Job::Implementer).unwrap().small = None;
    host.set_model_selection(&edited).unwrap();
    drop(host);
    let settings = || std::fs::read(home.path().join("settings.json")).ok();
    let before = (stored(home.path()), settings());
    drop(Host::open(home.path()).unwrap());
    assert_eq!((stored(home.path()), settings()), before);
}

#[test]
fn an_unedited_old_starter_guide_gets_the_current_one_and_an_edited_one_stays() {
    let guide_file = |home: &Path| home.join("shared/settings/model-guide.md");
    let mut cases: Vec<(String, bool)> = retired_starter_guides()
        .into_iter()
        .map(|guide| (guide, true))
        .collect();
    cases.push((
        format!("{}\n\n", retired_starter_guides()[0].trim_end()),
        true,
    ));
    cases.push((STARTER_GUIDE.to_owned(), true));
    cases.push((
        format!(
            "{}Prefer Opus for migrations.\n",
            retired_starter_guides()[0]
        ),
        false,
    ));
    for (guide, replaced) in cases {
        let home = tempfile::tempdir().unwrap();
        drop(Host::open_set_up(home.path()).unwrap());
        std::fs::write(guide_file(home.path()), &guide).unwrap();
        let host = Host::open(home.path()).unwrap();
        let expected = if replaced { STARTER_GUIDE } else { &guide };
        assert_eq!(host.model_selection().unwrap().guide, expected);
    }
}

#[test]
fn the_old_starter_guide_with_its_previous_settings_section_ends_as_the_current_one() {
    let home = tempfile::tempdir().unwrap();
    drop(seed(home.path(), current, |_| {}));
    // This shape: the old combined starter paragraph, then the section an earlier upgrade wrote,
    // with trailing blank lines, in the row and in the shared folder.
    let guide = format!(
        "{}\n## Previous settings (migrated 2026-10-08)\n\n### Tester\n- Claude: small work → sonnet · low.\n\n\n",
        retired_starter_guides()[0]
    );
    let legacy = json!({"revision":4,"guide":guide,
        "providers":{"claude":{"enabled":true,"models":[{"model":"opus","effort":"high"}]},
            "codex":{"enabled":true,"models":[{"model":"gpt-6.1-sol","effort":"high"}]}},
        "role_providers":{}});
    let db = Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    db.execute_batch("DELETE FROM policy_migrations WHERE id='who-runs-what'")
        .unwrap();
    db.execute("UPDATE model_selection SET data=?1", [legacy.to_string()])
        .unwrap();
    drop(db);
    std::fs::write(home.path().join("shared/settings/model-guide.md"), guide).unwrap();
    let host = Host::open_detecting(home.path(), &signed_in(&PROVIDERS)).unwrap();
    let selection = host.model_selection().unwrap();
    assert_eq!(selection.guide, STARTER_GUIDE);
    for line in [
        "From the guide: ### Tester",
        "From the guide: - Claude: small work → sonnet · low.",
    ] {
        assert!(
            selection.notes.iter().any(|n| n == line),
            "{:?}",
            selection.notes
        );
    }
    // Re-running changes nothing.
    drop(host);
    assert_eq!(
        Host::open(home.path()).unwrap().model_selection().unwrap(),
        selection
    );
}

#[test]
fn saving_validates_the_whole_selection_and_bumps_the_revision() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let saved = host.model_selection().unwrap();
    let mut invalid = saved.clone();
    invalid.jobs.retain(|row| row.job != Job::Tester);
    invalid.guide = "A guide that is not saved.".into();
    assert!(host.set_model_selection(&invalid).is_err());
    assert_eq!(host.model_selection().unwrap(), saved);
    let next = host.set_model_selection(&saved).unwrap();
    assert_eq!(next.revision, saved.revision + 1);
    assert_eq!(host.model_selection().unwrap(), next);
}

#[test]
fn chat_override_is_unlimited_and_preserves_provider_identity() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    host.create_project("Chat overrides").unwrap();
    let root = host.sessions().unwrap().remove(0);
    let runtime = host.session_runtime(&root.id).unwrap();
    assert_eq!(runtime.profile, Some(claude("opus", "high")));
    let selection = runtime.selection.unwrap();
    assert_eq!(
        (selection.chosen_by, selection.reason.as_str()),
        (Chooser::Wiffletree, "First choice")
    );
    // No row names Sonnet at max.
    let unlisted = claude("sonnet", "max");
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
    let mut host = Host::open_set_up(home.path()).unwrap();
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
