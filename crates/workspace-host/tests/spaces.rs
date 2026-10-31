//! Spaces (store schema 9): the upgrade from projects' brain folders, Space commands, session owners, New project's defaults and the Spaces a
//! repository is in.
use rusqlite::{Connection, params};
use serde_json::json;
use std::path::Path;
use workspace_core::*;
use workspace_host::Host;

fn coordinator(host: &Host, project: &str) -> Session {
    (host.sessions().unwrap().into_iter())
        .find(|s| s.project_id == project && s.parent_id.is_none())
        .unwrap()
}

fn log(session: &str, milestone: &str) -> LogInput {
    LogInput {
        session_id: session.into(),
        milestone: Some(milestone.into()),
        kind: LogKind::Decision,
        title: format!("Decision {milestone}"),
        repository: None,
        changed: "Spaces".into(),
        why: "One home".into(),
        state: "Reported; unverified".into(),
    }
}

/// Rewinds a store to schema 8, as the release before Spaces left it: each project names its
/// brain folder, or `null`, and there is no Spaces store.
fn before_spaces(home: &Path, brains: &[(&str, Option<&str>)]) {
    let db = Connection::open(home.join("workspace.sqlite3")).unwrap();
    db.execute_batch(
        "DROP TABLE spaces; DROP TABLE trackers; DROP TABLE repository_suggestions;
         ALTER TABLE work_logs DROP COLUMN space_id;
         PRAGMA user_version = 8;",
    )
    .unwrap();
    for (project, brain) in brains {
        db.execute(
            "UPDATE projects SET data=json_set(json_remove(data,'$.space'),'$.brain',?2) WHERE id=?1",
            params![project, brain],
        )
        .unwrap();
    }
}

/// Each Space's name with the names of its projects, in the order the Spaces were created.
fn spaces_with_projects(host: &Host) -> Vec<(String, Vec<String>)> {
    let projects = host.projects().unwrap();
    (host.spaces().unwrap().into_iter())
        .map(|record| {
            let mut names: Vec<String> = (projects.iter())
                .filter(|p| p.space == record.space.id)
                .map(|p| p.name.clone())
                .collect();
            names.sort();
            (record.space.name, names)
        })
        .collect()
}

fn canonical(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

#[test]
fn a_fresh_store_has_only_personal() {
    let home = tempfile::tempdir().unwrap();
    let host = Host::open_set_up(home.path()).unwrap();
    let spaces = host.spaces().unwrap();
    assert_eq!(spaces.len(), 1);
    let personal = &spaces[0];
    assert_eq!(personal.space.name, PERSONAL);
    assert_eq!(personal.space.brain, None);
    assert!(matches!(
        personal.space.storage.repository,
        StorageRepository::Local { .. }
    ));
    assert_eq!(host.tracker(&personal.space).unwrap().prefix, "PERS");
}

#[test]
fn the_upgrade_makes_a_space_per_brain_folder_and_personal_for_the_rest() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let ids: Vec<String> = ["a", "b", "c", "d", "e", "f"]
        .iter()
        .map(|name| host.create_project(name).unwrap().id)
        .collect();
    // An archived project comes along.
    let archived = coordinator(&host, &ids[5]).id;
    host.set_archived(&archived, true).unwrap();
    drop(host);
    let team = "/Users/m/dev/sage/team-brain";
    let vyttle = "/Users/m/dev/vyttle/vyttle-brain";
    before_spaces(
        home.path(),
        &[
            (&ids[0], Some(team)),
            (&ids[1], Some(team)),
            (&ids[2], Some(team)),
            (&ids[3], Some(vyttle)),
            (&ids[4], None),
            (&ids[5], None),
        ],
    );
    let host = Host::open_set_up(home.path()).unwrap();
    assert_eq!(
        spaces_with_projects(&host),
        [
            (
                "team-brain".into(),
                vec!["a".into(), "b".into(), "c".into()]
            ),
            ("vyttle-brain".into(), vec!["d".into()]),
            (PERSONAL.into(), vec!["e".into(), "f".into()]),
        ]
    );
    let spaces = host.spaces().unwrap();
    // Each brain folder stays a local brain, so exports keep going where they went.
    assert_eq!(spaces[0].local.brain_path.as_deref(), Some(team));
    assert_eq!(
        spaces[0].space.brain,
        Some(SpaceBrain {
            repository: BrainRepository::local(),
            folder: "second-brain".into(),
            extra: Extra::new(),
        })
    );
    assert_eq!(spaces[2].space.brain, None);
    // Each gets its own proposed prefix and lists no repositories yet.
    let prefixes: Vec<String> = (spaces.iter())
        .map(|r| host.tracker(&r.space).unwrap().prefix)
        .collect();
    assert_eq!(prefixes, ["TB", "VB", "PERS"]);
    assert!(spaces.iter().all(|r| r.space.repositories.is_empty()));
}

#[test]
fn two_brain_folders_of_one_name_get_their_parents_names() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let sage = host.create_project("Sage work").unwrap().id;
    let vyttle = host.create_project("Vyttle work").unwrap().id;
    drop(host);
    before_spaces(
        home.path(),
        &[
            (&sage, Some("/Users/m/dev/sage/brain")),
            (&vyttle, Some("/Users/m/dev/vyttle/brain")),
        ],
    );
    let host = Host::open_set_up(home.path()).unwrap();
    // No project lacks a brain, so there is no Personal.
    assert_eq!(
        spaces_with_projects(&host),
        [
            ("sage/brain".into(), vec!["Sage work".into()]),
            ("vyttle/brain".into(), vec!["Vyttle work".into()]),
        ]
    );
}

#[test]
fn a_second_start_changes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let one = host.create_project("One").unwrap().id;
    let two = host.create_project("Two").unwrap().id;
    drop(host);
    before_spaces(
        home.path(),
        &[(&one, Some("/Users/m/dev/brain")), (&two, None)],
    );
    let host = Host::open_set_up(home.path()).unwrap();
    let (spaces, projects) = (host.spaces().unwrap(), host.projects().unwrap());
    let trackers: Vec<_> = (spaces.iter())
        .map(|r| host.tracker(&r.space).unwrap())
        .collect();
    drop(host);
    let host = Host::open_set_up(home.path()).unwrap();
    assert_eq!(host.spaces().unwrap(), spaces);
    assert_eq!(host.projects().unwrap(), projects);
    let again: Vec<_> = (spaces.iter())
        .map(|r| host.tracker(&r.space).unwrap())
        .collect();
    assert_eq!(again, trackers);
}

#[test]
fn a_failed_upgrade_changes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let good = host.create_project("Good").unwrap().id;
    let bad = host.create_project("Bad").unwrap().id;
    drop(host);
    before_spaces(home.path(), &[(&good, Some("/Users/m/dev/brain"))]);
    let db = Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    db.execute(
        "UPDATE projects SET data='\"unreadable\"' WHERE id=?1",
        [&bad],
    )
    .unwrap();
    drop(db);
    assert!(Host::open_set_up(home.path()).is_err());
    let db = Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    let spaces: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='spaces')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!spaces);
    let brain: String = db
        .query_row(
            "SELECT json_extract(data,'$.brain') FROM projects WHERE id=?1",
            [&good],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(brain, "/Users/m/dev/brain");
}

#[test]
fn a_start_with_no_space_creates_personal() {
    let home = tempfile::tempdir().unwrap();
    drop(Host::open_set_up(home.path()).unwrap());
    let db = Connection::open(home.path().join("workspace.sqlite3")).unwrap();
    db.execute_batch("DELETE FROM spaces; DELETE FROM trackers;")
        .unwrap();
    drop(db);
    let host = Host::open_set_up(home.path()).unwrap();
    let names: Vec<String> = (host.spaces().unwrap().into_iter())
        .map(|r| r.space.name)
        .collect();
    assert_eq!(names, [PERSONAL]);
}

#[test]
fn a_space_without_a_brain_logs_nothing() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let project = host.create_project("Quiet").unwrap();
    let session = coordinator(&host, &project.id);
    let error = host.append_log(log(&session.id, "one")).unwrap_err();
    assert!(format!("{error:#}").contains("Space has no brain"));
    assert!(host.logs(&project.id, None, 100).unwrap().is_empty());
}

#[test]
fn attaching_a_brain_sets_it_for_every_project_in_the_space() {
    let home = tempfile::tempdir().unwrap();
    let brain = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let one = host.create_project("One").unwrap();
    let two = host.create_project("Two").unwrap();
    assert_eq!(one.space, two.space);
    host.set_space_brain(&one.space, brain.path().to_str().unwrap())
        .unwrap();
    let record = host.space(&two.space).unwrap();
    assert_eq!(
        record.local.brain_path.as_deref(),
        Some(canonical(brain.path()).as_str())
    );
    assert!(record.space.brain.is_some());
    // The path stays on this Mac: the shared definition names only a local brain.
    let definition = serde_json::to_string(&record.space).unwrap();
    assert!(!definition.contains(&canonical(brain.path())));
}

#[test]
fn projects_go_to_the_space_last_used_or_their_creators() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let vyttle = host.create_space("Vyttle", "", "", None).unwrap().space;
    let site = host.create_project_in("Site", &vyttle.id).unwrap();
    assert_eq!(site.space, vyttle.id);
    assert_eq!(host.default_space().unwrap(), vyttle.id);
    assert_eq!(host.create_project("Next").unwrap().space, vyttle.id);
    let personal = host.spaces().unwrap()[0].space.id.clone();
    host.create_project_in("Notes", &personal).unwrap();
    let coordinator = coordinator(&host, &site.id);
    let created = host
        .create_project_from(&coordinator.id, "Billing", "Move invoices.", None)
        .unwrap();
    assert_eq!(created.project.space, vyttle.id);
    assert!(
        host.create_project_in("Nowhere", &SpaceId("missing".into()))
            .is_err()
    );
}

#[test]
fn space_names_and_prefixes_are_unique() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let sage = host.create_space("Sage", "Work", "Be brief", None).unwrap();
    assert_eq!(host.tracker(&sage.space).unwrap().prefix, "SAGE");
    assert!(host.create_space("sage", "", "", None).is_err());
    let clash = host.create_space("Sage Watch", "", "", Some("SAGE"));
    assert!(format!("{:#}", clash.unwrap_err()).contains("prefix SAGE"));
    assert!(host.create_space("Watch", "", "", Some("Wa")).is_err());
    let watch = host.create_space("Sage Watch", "", "", None).unwrap();
    assert_eq!(host.tracker(&watch.space).unwrap().prefix, "SW");

    let mut renamed = watch.space.clone();
    renamed.name = "SAGE".into();
    assert!(host.update_space(renamed).is_err());
    let mut moved = watch.space.clone();
    moved.storage = sage.space.storage.clone();
    assert!(host.update_space(moved).is_err());
    let mut edited = watch.space.clone();
    edited.instructions = "Use the watch simulator".into();
    edited.repositories = vec!["Sage-Social/watch-app".into()];
    edited.sources = vec![Source::Cli {
        command: "gh".into(),
        prefixes: vec!["gh pr view".into()],
        check: Some("gh auth status".into()),
        extra: Extra::new(),
    }];
    assert_eq!(host.update_space(edited.clone()).unwrap().space, edited);
    let mut bad = edited;
    bad.repositories = vec!["watch-app".into()];
    assert!(host.update_space(bad).is_err());
}

#[test]
fn removing_a_space_keeps_one_and_moves_archived_projects_to_personal() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let personal = host.spaces().unwrap()[0].space.id.clone();
    let error = host.remove_space(&personal).unwrap_err();
    assert!(format!("{error:#}").contains("At least one Space must exist"));

    let wood = host
        .create_space("Woodworking", "", "", None)
        .unwrap()
        .space;
    let bench = host.create_project_in("Bench", &wood.id).unwrap();
    let error = host.remove_space(&wood.id).unwrap_err();
    assert!(format!("{error:#}").contains("Bench"));
    host.set_archived(&coordinator(&host, &bench.id).id, true)
        .unwrap();
    // Personal was removed first, so removing Woodworking creates it again.
    host.create_space("Sage", "", "", None).unwrap();
    host.remove_space(&personal).unwrap();
    host.remove_space(&wood.id).unwrap();
    let names: Vec<String> = (host.spaces().unwrap().into_iter())
        .map(|r| r.space.name)
        .collect();
    assert_eq!(names, ["Sage", PERSONAL]);
    let space = host.project(&bench.id).unwrap().space;
    assert_eq!(host.space(&space).unwrap().space.name, PERSONAL);
}

#[test]
fn personal_is_not_removed_while_it_holds_archived_projects() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let personal = host.spaces().unwrap()[0].space.id.clone();
    let old = host.create_project_in("Old", &personal).unwrap();
    host.set_archived(&coordinator(&host, &old.id).id, true)
        .unwrap();
    host.create_space("Sage", "", "", None).unwrap();
    let error = host.remove_space(&personal).unwrap_err();
    assert!(format!("{error:#}").contains("nowhere else to go"));
    assert_eq!(host.spaces().unwrap().len(), 2);
    assert_eq!(host.project(&old.id).unwrap().space, personal);
}

#[test]
fn a_worker_owned_by_a_space_or_a_brain_is_refused_and_nothing_is_stored() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let project = host.create_project("Calls").unwrap();
    let parent = coordinator(&host, &project.id);
    let space = SessionOwner::Space(project.space.clone());
    let brain = SessionOwner::Brain("Sage-Social/sagehaven-docs".into());
    let (sessions, projects) = (host.sessions().unwrap(), host.projects().unwrap());
    for owner in [&space, &brain] {
        for role in [Role::Implementer, Role::ProjectOrchestrator] {
            let error = host
                .create_owned_session(owner, &parent.id, None, "Worker", role, Provider::Claude)
                .unwrap_err();
            assert!(format!("{error:#}").contains("cannot run outside a project"));
        }
    }
    assert_eq!(host.sessions().unwrap(), sessions);
    assert_eq!(host.projects().unwrap(), projects);
    let owned = host
        .create_owned_session(
            &SessionOwner::Project(project.id.clone()),
            &parent.id,
            None,
            "Worker",
            Role::Implementer,
            Provider::Claude,
        )
        .unwrap();
    assert_eq!(
        host.session_owner(&owned.id).unwrap(),
        SessionOwner::Project(project.id)
    );
}

/// A workspace repository whose `origin` is the GitHub repository `identity`, written beside
/// the host as its GitHub read would leave it.
fn on_github(home: &Path, identity: &str) -> String {
    let (owner, name) = identity.split_once('/').unwrap();
    let id = format!("repo-{name}");
    let repository = json!({"id": id, "name": name, "path": format!("/dev/{identity}"),
        "base": "origin/main", "github": {"owner": owner, "name": name}});
    let db = Connection::open(home.join("workspace.sqlite3")).unwrap();
    db.execute(
        "INSERT INTO repositories VALUES (?1,?2,?3)",
        params![id, repository["path"].as_str(), repository.to_string()],
    )
    .unwrap();
    id
}

/// `space` listing `repositories`, saved as the human's Space page saves it.
fn listing(host: &mut Host, space: &SpaceId, repositories: &[&str]) {
    let mut definition = host.space(space).unwrap().space;
    definition.repositories = repositories.iter().map(|r| r.to_string()).collect();
    host.update_space(definition).unwrap();
}

#[test]
fn new_project_starts_in_the_space_last_used_with_its_repositories_here() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let personal = host.default_space().unwrap();
    let sage = host.create_space("Sage", "", "", None).unwrap().space.id;
    let mobile = on_github(home.path(), "Sage-Social/mobile-app");
    let watch = on_github(home.path(), "Sage-Social/watch-app");
    let dotfiles = on_github(home.path(), "MHohlios/dotfiles");
    listing(
        &mut host,
        &sage,
        &[
            "Sage-Social/api",
            "sage-social/WATCH-APP",
            "Sage-Social/mobile-app",
        ],
    );
    listing(&mut host, &personal, &["MHohlios/dotfiles"]);
    host.create_project_in("Calls", &sage).unwrap();

    let defaults = host.new_project_defaults(None).unwrap();
    assert_eq!(
        defaults,
        SpaceRepositories {
            space_id: sage.clone(),
            // In workspace order, matched ignoring case; no personal repository.
            repository_ids: vec![mobile, watch],
            not_cloned: vec!["Sage-Social/api".into()],
        }
    );
    // Switching the Space replaces them with the new Space's.
    let switched = host.new_project_defaults(Some(personal.clone())).unwrap();
    assert_eq!(switched.space_id, personal);
    assert_eq!(switched.repository_ids, [dotfiles]);
    assert!(switched.not_cloned.is_empty());
    // Asking is a read: the Space last used stays Sage.
    assert_eq!(host.default_space().unwrap(), sage);
    let answered = host
        .execute(Command::NewProjectDefaults { space_id: None })
        .unwrap();
    assert_eq!(answered["space_id"], sage.as_str());
}

#[test]
fn a_repository_may_be_in_several_spaces_or_none() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let personal = host.default_space().unwrap();
    let sage = host.create_space("Sage", "", "", None).unwrap().space.id;
    let watch = host
        .create_space("Sage Watch", "", "", None)
        .unwrap()
        .space
        .id;
    let api = on_github(home.path(), "Sage-Social/api");
    let repository = host.repository(&api).unwrap();
    listing(&mut host, &sage, &["Sage-Social/mobile-app"]);
    let listed = |host: &Host| -> Vec<Vec<String>> {
        (host.spaces().unwrap().into_iter())
            .map(|r| r.space.repositories)
            .collect()
    };

    host.execute(Command::SetRepositorySpaces {
        repository: "Sage-Social/api".into(),
        space_ids: vec![sage.clone(), watch.clone()],
    })
    .unwrap();
    assert_eq!(
        listed(&host),
        [
            vec![],
            vec!["Sage-Social/mobile-app".into(), "Sage-Social/api".into()],
            vec!["Sage-Social/api".to_string()],
        ]
    );
    assert_eq!(
        host.new_project_defaults(Some(watch.clone()))
            .unwrap()
            .repository_ids,
        std::slice::from_ref(&api)
    );

    // Removing the Sage chip leaves Sage Watch's and every other entry as they were.
    host.set_repository_spaces("sage-social/API", std::slice::from_ref(&watch))
        .unwrap();
    assert_eq!(
        listed(&host),
        [
            vec![],
            vec!["Sage-Social/mobile-app".to_string()],
            vec!["Sage-Social/api".to_string()],
        ]
    );

    // No Space.
    host.set_repository_spaces("Sage-Social/api", &[]).unwrap();
    assert_eq!(
        listed(&host),
        [vec![], vec!["Sage-Social/mobile-app".to_string()], vec![]]
    );
    assert_eq!(host.repository(&api).unwrap(), repository);

    // A refusal changes no Space.
    for (repository, spaces) in [
        ("not an identity", vec![personal.clone()]),
        (
            "Sage-Social/api",
            vec![sage.clone(), SpaceId("missing".into())],
        ),
    ] {
        assert!(host.set_repository_spaces(repository, &spaces).is_err());
    }
    assert_eq!(
        listed(&host),
        [vec![], vec!["Sage-Social/mobile-app".to_string()], vec![]]
    );
}

#[test]
fn adding_a_repository_suggests_a_space_by_its_owner() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let sage = host.create_space("Sage", "", "", None).unwrap().space.id;
    let vyttle = host.create_space("Vyttle", "", "", None).unwrap().space.id;
    listing(
        &mut host,
        &sage,
        &["Sage-Social/mobile-app", "Sage-Social/api"],
    );
    listing(
        &mut host,
        &vyttle,
        &["Vyttle-LLC/site", "sage-social/shared"],
    );
    let before = host.spaces().unwrap();
    let suggest = |host: &mut Host, repository: &str| {
        serde_json::from_value::<Option<SpaceId>>(
            host.execute(Command::SuggestRepositorySpace {
                repository: repository.into(),
            })
            .unwrap(),
        )
        .unwrap()
    };
    // Sage lists the most Sage-Social repositories.
    assert_eq!(suggest(&mut host, "Sage-Social/watch-app"), Some(sage));
    assert_eq!(suggest(&mut host, "Vyttle-LLC/app"), Some(vyttle));
    assert_eq!(suggest(&mut host, "octo/scratch"), None, "No Space");
    // The owner only suggests: nothing joins a Space.
    assert_eq!(host.spaces().unwrap(), before);
}

#[test]
fn the_upgrade_holds_suggestions_for_the_repositories_it_found() {
    let home = tempfile::tempdir().unwrap();
    let project = {
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.create_project("Calls").unwrap().id
    };
    let api = on_github(home.path(), "Sage-Social/api");
    before_spaces(home.path(), &[(&project, None)]);
    let found = |home: &Path| -> Vec<String> {
        let db = Connection::open(home.join("workspace.sqlite3")).unwrap();
        (db.prepare("SELECT repository_id FROM repository_suggestions")
            .unwrap())
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    };

    let mut host = Host::open_set_up(home.path()).unwrap();
    assert_eq!(found(home.path()), [api]);
    assert!(
        (host.spaces().unwrap().iter()).all(|r| r.space.repositories.is_empty()),
        "no Space lists a repository before the human confirms"
    );
    host.execute(Command::ConfirmRepositorySuggestions { choices: vec![] })
        .unwrap();
    assert!(found(home.path()).is_empty());
    drop(host);
    // A later start suggests nothing again, and a fresh store has nothing to suggest.
    Host::open_set_up(home.path()).unwrap();
    assert!(found(home.path()).is_empty());
    let fresh = tempfile::tempdir().unwrap();
    Host::open_set_up(fresh.path()).unwrap();
    assert!(found(fresh.path()).is_empty());
}
