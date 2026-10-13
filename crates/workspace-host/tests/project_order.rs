//! The manual order of projects: new projects land at the top, moves keep a block together,
//! and the order is stored.
use workspace_core::Command;
use workspace_host::Host;

/// A store whose projects were created in the order given, so the last one is topmost.
fn store(names: &[&str]) -> (tempfile::TempDir, Host) {
    let directory = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(directory.path()).unwrap();
    for name in names {
        host.create_project(name).unwrap();
    }
    (directory, host)
}

fn names(host: &Host) -> String {
    let projects = host.projects().unwrap();
    projects.iter().map(|p| p.name.as_str()).collect()
}

fn id(host: &Host, name: &str) -> String {
    let projects = host.projects().unwrap();
    projects.into_iter().find(|p| p.name == name).unwrap().id
}

fn moved(host: &mut Host, block: &[&str], before: Option<&str>) -> String {
    let ids: Vec<String> = block.iter().map(|name| id(host, name)).collect();
    let before = before.map(|name| id(host, name));
    host.move_projects(&ids, before.as_deref()).unwrap();
    names(host)
}

#[test]
fn a_new_project_lands_at_the_top_and_the_order_survives_a_reopen() {
    let (directory, mut host) = store(&["C", "B", "A"]);
    assert_eq!(names(&host), "ABC");
    assert_eq!(moved(&mut host, &["C"], Some("A")), "CAB");
    host.create_project("D").unwrap();
    assert_eq!(names(&host), "DCAB");
    drop(host);

    let host = Host::open_set_up(directory.path()).unwrap();

    assert_eq!(names(&host), "DCAB");
    let snapshot = host.snapshot().unwrap();
    let listed: String = snapshot.projects.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(listed, "DCAB", "every client reads the same order");
}

#[test]
fn one_project_moves_before_another_or_to_the_end() {
    let (_directory, mut host) = store(&["D", "C", "B", "A"]);
    assert_eq!(moved(&mut host, &["D"], Some("B")), "ADBC");
    assert_eq!(moved(&mut host, &["A"], Some("C")), "DBAC");
    assert_eq!(moved(&mut host, &["D"], None), "BACD");
    assert_eq!(moved(&mut host, &["B"], Some("A")), "BACD", "already there");
}

#[test]
fn a_block_moves_together_and_keeps_its_own_order() {
    let (_directory, mut host) = store(&["E", "D", "C", "B", "A"]);
    // Listed out of order and apart: they land together, in the order they had.
    assert_eq!(moved(&mut host, &["D", "B"], Some("A")), "BDACE");
    assert_eq!(moved(&mut host, &["B", "A"], None), "DCEBA");
    assert_eq!(moved(&mut host, &["E", "A"], Some("C")), "DEACB");
}

#[test]
fn the_move_command_returns_the_new_order() {
    let (_directory, mut host) = store(&["B", "A"]);
    let command: Command = serde_json::from_value(serde_json::json!({
        "type": "move_projects", "project_ids": [id(&host, "B")], "before": id(&host, "A")
    }))
    .unwrap();

    let order = host.execute(command).unwrap();

    let returned: Vec<&str> = order
        .as_array()
        .unwrap()
        .iter()
        .map(|project| project["name"].as_str().unwrap())
        .collect();
    assert_eq!(returned, ["B", "A"]);
    // Without `before`, the block goes to the end.
    let command: Command = serde_json::from_value(serde_json::json!({
        "type": "move_projects", "project_ids": [id(&host, "B")]
    }))
    .unwrap();
    host.execute(command).unwrap();
    assert_eq!(names(&host), "AB");
}

#[test]
fn an_invalid_move_is_refused_and_leaves_the_order_unchanged() {
    let (_directory, mut host) = store(&["C", "B", "A"]);
    let (a, b) = (id(&host, "A"), id(&host, "B"));
    let missing = "missing-project".to_owned();
    let refused: [(Vec<&String>, Option<&String>); 6] = [
        (vec![], Some(&a)),
        (vec![&missing], Some(&a)),
        (vec![&b, &missing], Some(&a)),
        (vec![&b], Some(&missing)),
        (vec![&b, &b], Some(&a)),
        (vec![&a, &b], Some(&b)),
    ];
    for (ids, before) in refused {
        let ids: Vec<String> = ids.into_iter().cloned().collect();
        let before = before.map(String::as_str);
        assert!(
            host.move_projects(&ids, before).is_err(),
            "{ids:?} {before:?}"
        );
        assert_eq!(names(&host), "ABC");
    }
}
