//! Spaces (store schema 9): the upgrade from projects' brain folders, exports through a
//! project's Space, Space commands and session owners.
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
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
        milestone: milestone.into(),
        kind: LogKind::Decision,
        title: format!("Decision {milestone}"),
        reference: "—".into(),
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
        "DROP TABLE spaces; DROP TABLE trackers; ALTER TABLE work_logs DROP COLUMN space_id;
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
            repository: BrainRepository::Local,
            folder: "second-brain".into()
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
fn an_upgraded_project_exports_to_the_same_folder_and_nothing_twice() {
    let home = tempfile::tempdir().unwrap();
    let brain = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let project = host.create_project("Memory").unwrap();
    let session = coordinator(&host, &project.id);
    host.attach_brain(&project.id, brain.path().to_str().unwrap())
        .unwrap();
    let first = host.append_log(log(&session.id, "one")).unwrap();
    host.append_log(log(&session.id, "two")).unwrap();
    assert_eq!(host.export_logs(&project.id).unwrap().exported, 2);
    drop(host);
    before_spaces(
        home.path(),
        &[(&project.id, Some(&canonical(brain.path())))],
    );
    let path: PathBuf = brain.path().join("second-brain/logs/project.md");
    let exported = std::fs::read_to_string(&path).unwrap();

    let mut host = Host::open_set_up(home.path()).unwrap();
    let space = host.project(&project.id).unwrap().space;
    assert_eq!(
        host.space(&space).unwrap().space.name,
        brain.path().file_name().unwrap().to_string_lossy()
    );
    assert_eq!(host.export_logs(&project.id).unwrap().exported, 0);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), exported);
    let third = host.append_log(log(&session.id, "three")).unwrap();
    assert_eq!(host.export_logs(&project.id).unwrap().exported, 1);
    let content = std::fs::read_to_string(&path).unwrap();
    for id in [&first.id, &third.id] {
        assert_eq!(content.matches(&format!("workspace-entry:{id}")).count(), 1);
    }
}

#[test]
fn a_moved_project_exports_later_entries_to_its_new_space() {
    let home = tempfile::tempdir().unwrap();
    let (old_brain, new_brain) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut host = Host::open_set_up(home.path()).unwrap();
    let project = host.create_project("Calls").unwrap();
    let session = coordinator(&host, &project.id);
    host.attach_brain(&project.id, old_brain.path().to_str().unwrap())
        .unwrap();
    let exported = host.append_log(log(&session.id, "one")).unwrap();
    assert_eq!(host.export_logs(&project.id).unwrap().exported, 1);
    // The reading chosen: an entry belongs to the Space its project was in when it was logged,
    // so one logged before the move and exported after it still goes to the old Space's brain.
    let pending = host.append_log(log(&session.id, "two")).unwrap();

    let vyttle = host.create_space("Vyttle", "", "", None).unwrap().space;
    let moved = host.move_project_to_space(&project.id, &vyttle.id).unwrap();
    assert_eq!(moved.space, vyttle.id);
    assert_eq!(moved.repositories, project.repositories);
    host.attach_brain(&project.id, new_brain.path().to_str().unwrap())
        .unwrap();
    let later = host.append_log(log(&session.id, "three")).unwrap();
    assert_eq!(host.export_logs(&project.id).unwrap().exported, 2);

    let read = |brain: &Path| {
        std::fs::read_to_string(brain.join("second-brain/logs/project.md")).unwrap_or_default()
    };
    let (old, new) = (read(old_brain.path()), read(new_brain.path()));
    for (id, in_old, in_new) in [(&exported.id, 1, 0), (&pending.id, 1, 0), (&later.id, 0, 1)] {
        let marker = format!("workspace-entry:{id}");
        assert_eq!(
            (old.matches(&marker).count(), new.matches(&marker).count()),
            (in_old, in_new)
        );
    }
}

/// A project in Personal with `old` as its brain, moved to a new Space "Vyttle" with no brain.
fn moved_from(host: &mut Host, old: &Path) -> (Project, Session) {
    let project = host.create_project("Calls").unwrap();
    let session = coordinator(host, &project.id);
    host.attach_brain(&project.id, old.to_str().unwrap())
        .unwrap();
    let vyttle = host.create_space("Vyttle", "", "", None).unwrap().space;
    host.append_log(log(&session.id, "before")).unwrap();
    host.move_project_to_space(&project.id, &vyttle.id).unwrap();
    (project, session)
}

fn entries(brain: &Path) -> String {
    std::fs::read_to_string(brain.join("second-brain/logs/project.md")).unwrap_or_default()
}

#[test]
fn a_space_with_nothing_pending_is_never_touched() {
    let (home, folders) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (old, new) = (folders.path().join("old"), folders.path().join("new"));
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&new).unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let (project, session) = moved_from(&mut host, &old);
    // The entry logged before the move reaches the old brain, which is then renamed away.
    assert_eq!(host.export_logs(&project.id).unwrap().exported, 1);
    std::fs::rename(&old, folders.path().join("gone")).unwrap();
    host.attach_brain(&project.id, new.to_str().unwrap())
        .unwrap();
    let later = host.append_log(log(&session.id, "after")).unwrap();
    let run = host.export_logs(&project.id).unwrap();
    assert_eq!(run.exported, 1);
    assert!(entries(&new).contains(&format!("workspace-entry:{}", later.id)));
}

#[test]
fn one_spaces_missing_brain_leaves_the_others_entries_delivered() {
    let (home, folders) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (old, new) = (folders.path().join("old"), folders.path().join("new"));
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&new).unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    // The entry logged before the move waits for a brain that is no longer there.
    let (project, session) = moved_from(&mut host, &old);
    std::fs::remove_dir(&old).unwrap();
    host.attach_brain(&project.id, new.to_str().unwrap())
        .unwrap();
    let later = host.append_log(log(&session.id, "after")).unwrap();
    let error = format!("{:#}", host.export_logs(&project.id).unwrap_err());
    assert!(error.starts_with("Personal: "), "{error}");
    assert!(!error.contains("Vyttle"), "{error}");
    assert!(entries(&new).contains(&format!("workspace-entry:{}", later.id)));
    let run = &host.runs(&project.id).unwrap()[0];
    assert_eq!(run.exported, 1);
    assert!(run.outcome.starts_with("failed: Personal: "));
}

#[test]
fn entries_for_the_previous_spaces_brain_export_when_the_current_space_has_none() {
    let (home, brain) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut host = Host::open_set_up(home.path()).unwrap();
    let (project, _) = moved_from(&mut host, brain.path());
    assert_eq!(host.export_logs(&project.id).unwrap().exported, 1);
    assert!(entries(brain.path()).contains("workspace-entry:"));
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
fn a_space_without_a_brain_exports_nothing() {
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let project = host.create_project("Quiet").unwrap();
    let session = coordinator(&host, &project.id);
    host.append_log(log(&session.id, "one")).unwrap();
    let error = host.export_logs(&project.id).unwrap_err();
    assert!(format!("{error:#}").contains("Space has no brain"));
}

#[test]
fn attaching_a_brain_sets_it_for_every_project_in_the_space() {
    let home = tempfile::tempdir().unwrap();
    let brain = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let one = host.create_project("One").unwrap();
    let two = host.create_project("Two").unwrap();
    assert_eq!(one.space, two.space);
    host.attach_brain(&one.id, brain.path().to_str().unwrap())
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
    assert_eq!(created.space, vyttle.id);
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
