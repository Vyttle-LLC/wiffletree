//! A coordinator asking for a repository its project does not use, and the human's answer.
//! Approving changes that one project: no Space's list changes.
mod common;
use common::git;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{fs, path::Path, path::PathBuf};
use workspace_core::*;
use workspace_host::Host;

/// A committed repository at `path`, whose `origin` is `identity` on GitHub.
fn clone_of(path: &Path, identity: &str) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "-b", "main"]);
    git(path, &["commit", "--allow-empty", "-m", "Initial"]);
    let url = format!("https://github.com/{identity}.git");
    git(path, &["remote", "add", "origin", &url]);
}

struct Fixture {
    home: tempfile::TempDir,
    folder: PathBuf,
    _folder: tempfile::TempDir,
    host: Host,
    /// Lists `Vyttle-LLC/plugins`; the project is not in it.
    vyttle: SpaceId,
    personal: SpaceId,
    project: Project,
    coordinator: Session,
    plugins: Repository,
}

/// Mudling's case: a project in Personal, and a repository only the Vyttle Space lists.
fn fixture() -> Fixture {
    let held = tempfile::tempdir().unwrap();
    let folder = fs::canonicalize(held.path()).unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path().join("home")).unwrap();
    host.set_workspaces_dir(home.path().join("workspaces").to_str().unwrap())
        .unwrap();
    let path = folder.join("plugins");
    clone_of(&path, "Vyttle-LLC/plugins");
    let plugins = add_on_github(&mut host, home.path(), &path, "Vyttle-LLC", "plugins");
    let vyttle = host.create_space("Vyttle", "", "", None).unwrap().space;
    let id = vyttle.id.clone();
    host.update_space(Space {
        repositories: vec!["Vyttle-LLC/plugins".into()],
        ..vyttle
    })
    .unwrap();
    let project = host.create_project("Mudling").unwrap();
    let personal = project.space.clone();
    assert_ne!(personal, id);
    let coordinator = (host.sessions().unwrap().into_iter())
        .find(|s| s.project_id == project.id && s.role == Role::ProjectOrchestrator)
        .unwrap();
    Fixture {
        home,
        folder,
        _folder: held,
        host,
        vyttle: id,
        personal,
        project,
        coordinator,
        plugins,
    }
}

/// Adds the repository at `path` as `refresh_github` would leave it.
fn add_on_github(host: &mut Host, home: &Path, path: &Path, owner: &str, name: &str) -> Repository {
    let added = host.add_repository(path.to_str().unwrap(), "HEAD").unwrap();
    Connection::open(home.join("home/workspace.sqlite3"))
        .unwrap()
        .execute(
            "UPDATE repositories SET data=json_set(data,'$.github',json(?2)) WHERE id=?1",
            params![added.id, json!({"owner": owner, "name": name}).to_string()],
        )
        .unwrap();
    host.repository(&added.id).unwrap()
}

impl Fixture {
    fn ask(&mut self, repository: &str) -> anyhow::Result<Value> {
        let args = json!({"repository": repository, "reason": "The registry lives there"});
        (self.host).agent_tool(&self.coordinator.id, "request_repository", args)
    }
    fn items(&self) -> Vec<Attention> {
        self.host.open_attention(&self.project.id).unwrap()
    }
    fn answer(&mut self, answer: &str) -> Attention {
        let id = self.items().remove(0).id;
        let answered = self.host.execute(Command::ResolveAttention {
            id,
            answer: answer.into(),
        });
        serde_json::from_value(answered.unwrap()).unwrap()
    }
    fn used(&self) -> Vec<String> {
        (self.host.project_repositories(&self.project.id).unwrap())
            .into_iter()
            .map(|r| r.name)
            .collect()
    }
    fn lists(&self) -> Vec<Vec<String>> {
        [&self.vyttle, &self.personal]
            .map(|space| self.host.space(space).unwrap().space.repositories)
            .to_vec()
    }
    /// The one result message of `item`.
    fn result(&self, item: &Attention) -> Message {
        self.host.message(&item.operation_id).unwrap()
    }
    fn new_clone(&self, name: &str) -> String {
        let path = self.folder.join(name);
        clone_of(&path, &format!("Vyttle-LLC/{name}"));
        path.to_str().unwrap().to_owned()
    }
}

#[test]
fn approving_a_repository_another_space_lists_changes_this_project_only() {
    let mut f = fixture();
    assert!(f.used().is_empty());
    let refused = (f.host)
        .create_ticket(&f.coordinator.id, &f.plugins.id, "Too soon", "Do it")
        .unwrap_err();
    assert!(
        refused.to_string().contains("request_repository"),
        "{refused}"
    );
    let lists = f.lists();

    let waiting = f.ask("vyttle-llc/PLUGINS").unwrap();
    assert_eq!(waiting["status"], "waiting_for_human");
    let item = f.items().remove(0);
    assert!(item.is_permission() && item.is_repository_request());
    assert_eq!(item.session_id, f.coordinator.id);
    let request = item.repository_request.clone().unwrap();
    assert_eq!(request.title(), "Add plugins to this project?");
    assert_eq!(request.reason, "The registry lives there");
    assert_eq!(request.identity.as_deref(), Some("Vyttle-LLC/plugins"));
    assert!(f.used().is_empty(), "nothing before the answer");
    assert!(
        f.host.dismiss_attention(&item.id).is_err(),
        "only an answer settles it"
    );

    let answered = f.answer("approved");

    assert_eq!(f.used(), ["plugins"]);
    let result = f.result(&answered);
    assert_eq!(result.recipient, f.coordinator.id);
    assert_eq!(result.receipt, Receipt::Queued);
    assert!(result.body.contains(&f.plugins.id), "{}", result.body);
    assert_eq!(host_notice(&result.id), Some("Wiffletree"));
    (f.host)
        .create_ticket(&f.coordinator.id, &f.plugins.id, "Registry", "Do it")
        .unwrap();
    assert_eq!(f.lists(), lists);
    assert_eq!(f.host.project(&f.project.id).unwrap().space, f.personal);
    assert!(f.items().is_empty());
}

#[test]
fn a_folder_joins_the_workspace_and_the_project_and_no_space() {
    let mut f = fixture();
    let path = f.new_clone("fresh");
    let lists = f.lists();

    assert_eq!(f.ask(&path).unwrap()["status"], "waiting_for_human");
    assert!(!(f.host.repositories().unwrap().iter()).any(|r| r.path == path));
    let request = f.items().remove(0).repository_request.unwrap();
    assert_eq!(
        request.title(),
        "Add fresh to the workspace and this project?"
    );
    assert_eq!(request.repository_id, None);

    let answered = f.answer("approved");

    let repositories = f.host.repositories().unwrap();
    let added = repositories.iter().find(|r| r.path == path).unwrap();
    assert_eq!(added.base, "main");
    assert_eq!(f.used(), ["fresh"]);
    assert_eq!(f.lists(), lists);
    assert!(f.result(&answered).body.contains(&added.id));
}

#[test]
fn a_folder_under_a_followed_root_joins_no_space() {
    let mut f = fixture();
    let root = f.folder.join("root");
    let path = root.join("fresh");
    clone_of(&path, "Vyttle-LLC/fresh");
    let root = root.to_str().unwrap();
    f.host
        .import_repositories(None, &[f.plugins.path.clone()], &[root.to_owned()], &[])
        .unwrap();
    (f.host)
        .set_followed_roots(&f.vyttle, vec![root.to_owned()])
        .unwrap();
    let lists = f.lists();

    f.ask(path.to_str().unwrap()).unwrap();
    f.answer("approved");
    f.host.refresh_repositories().unwrap();

    assert_eq!(f.used(), ["fresh"]);
    assert_eq!(f.lists(), lists);
}

#[test]
fn a_folder_a_root_check_added_while_the_card_waited_keeps_its_base() {
    let mut f = fixture();
    let path = f.new_clone("fresh");
    f.ask(&path).unwrap();
    let known = f.host.add_repository(&path, "HEAD").unwrap();

    f.answer("approved");

    assert_eq!(f.host.repository(&known.id).unwrap().base, "HEAD");
    assert_eq!(f.used(), ["fresh"]);
}

#[test]
fn denying_changes_nothing_and_tells_the_coordinator_and_it_may_ask_again() {
    let mut f = fixture();
    f.ask("Vyttle-LLC/plugins").unwrap();

    let denied = f.answer("denied");

    assert!(f.used().is_empty());
    let result = f.result(&denied);
    assert_eq!(result.recipient, f.coordinator.id);
    assert_eq!(result.receipt, Receipt::Queued);
    assert!(result.body.contains("denied"), "{}", result.body);

    assert_eq!(
        f.ask("Vyttle-LLC/plugins").unwrap()["status"],
        "waiting_for_human"
    );
    let again = f.items();
    assert_eq!(again.len(), 1);
    assert_ne!(again[0].id, denied.id);
}

#[test]
fn an_answer_counts_once() {
    let mut f = fixture();
    f.ask("Vyttle-LLC/plugins").unwrap();
    let denied = f.answer("denied");
    let second = f.host.execute(Command::ResolveAttention {
        id: denied.id.clone(),
        answer: "approved".into(),
    });
    assert!(second.is_err());
    assert!(f.used().is_empty());
}

#[test]
fn what_needs_no_decision_raises_no_card() {
    let mut f = fixture();
    let home = f.home.path().to_owned();
    // Already used, by name and by folder.
    let listed = f.new_clone("listed");
    let listed = add_on_github(&mut f.host, &home, Path::new(&listed), "octo", "listed");
    let mut personal = f.host.space(&f.personal).unwrap().space;
    personal.repositories.push("octo/listed".into());
    f.host.update_space(personal).unwrap();
    assert_eq!(f.ask("octo/listed").unwrap()["status"], "already_used");
    assert_eq!(f.ask(&listed.path).unwrap()["status"], "already_used");

    // Not on this Mac.
    let refused = f.ask("octo/elsewhere").unwrap_err().to_string();
    assert!(
        refused.contains("cloned first") && refused.contains("folder path"),
        "{refused}"
    );

    // Not a repository root, or no folder at all.
    let inner = f.folder.join("plugins/inner");
    fs::create_dir(&inner).unwrap();
    assert!(f.ask(inner.to_str().unwrap()).is_err());
    assert!(f.ask("/no/such/folder").is_err());
    let plain = f.folder.join("plain");
    fs::create_dir(&plain).unwrap();
    assert!(f.ask(plain.to_str().unwrap()).is_err());

    // Neither a name nor an absolute path.
    assert!(f.ask("plugins").is_err());
    assert!(f.ask("~/plugins").is_err());
    assert!(f.ask("a/b/c").is_err());

    assert!(f.items().is_empty());
}

#[test]
fn a_linked_worktree_is_refused_but_stays_addable_by_hand() {
    let mut f = fixture();
    let main = f.new_clone("main-clone");
    let linked = f.folder.join("linked");
    git(
        Path::new(&main),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "side",
            linked.to_str().unwrap(),
        ],
    );
    let linked = linked.to_str().unwrap();

    let refused = f.ask(linked).unwrap_err().to_string();
    assert!(
        refused.contains("linked Git worktree") && refused.contains("main clone"),
        "{refused}"
    );
    assert!(f.items().is_empty());
    assert!(!(f.host.repositories().unwrap().iter()).any(|r| r.path == linked));

    assert_eq!(f.ask(&main).unwrap()["status"], "waiting_for_human");
    f.host.add_repository(linked, "HEAD").unwrap();
}

#[test]
fn a_repeat_of_a_waiting_request_raises_no_second_card() {
    let mut f = fixture();
    f.ask("Vyttle-LLC/plugins").unwrap();
    let path = f.plugins.path.clone();
    for repeat in ["Vyttle-LLC/plugins", path.as_str()] {
        assert_eq!(f.ask(repeat).unwrap()["status"], "waiting_for_human");
    }
    assert_eq!(f.items().len(), 1);
}

#[test]
fn several_clones_of_a_name_are_asked_for_by_folder() {
    let mut f = fixture();
    let home = f.home.path().to_owned();
    let second = f.folder.join("second");
    clone_of(&second, "Vyttle-LLC/plugins");
    add_on_github(&mut f.host, &home, &second, "Vyttle-LLC", "plugins");

    let refused = f.ask("Vyttle-LLC/plugins").unwrap_err().to_string();
    assert!(refused.contains(&f.plugins.path), "{refused}");
    assert!(f.items().is_empty());

    f.ask(second.to_str().unwrap()).unwrap();
    f.answer("approved");
    assert_eq!(f.used(), ["second"]);
    assert_eq!(
        f.ask("Vyttle-LLC/plugins").unwrap()["status"],
        "already_used",
        "the clone in use answers for the name"
    );
}

#[test]
fn only_the_coordinator_may_ask() {
    let mut f = fixture();
    let home = f.home.path().to_owned();
    let own = f.new_clone("own");
    let own = add_on_github(&mut f.host, &home, Path::new(&own), "octo", "own");
    f.host
        .set_added_repositories(&f.project.id, vec![own.id.clone()])
        .unwrap();
    let ticket = (f.host)
        .create_ticket(&f.coordinator.id, &own.id, "Work", "Do it")
        .unwrap();
    let worker = (f.host)
        .assign_ticket(&ticket.id, Role::Implementer, Provider::Claude, "Do", None)
        .unwrap();
    let args = json!({"repository": "Vyttle-LLC/plugins", "reason": "I want it"});
    let refused = (f.host)
        .agent_tool(&worker.id, "request_repository", args)
        .unwrap_err();
    assert!(refused.to_string().contains("coordinator"), "{refused}");
    assert!(f.items().is_empty());
}

#[test]
fn a_request_needs_a_one_line_reason_and_no_other_argument() {
    let mut f = fixture();
    let id = f.coordinator.id.clone();
    for args in [
        json!({"repository": "Vyttle-LLC/plugins"}),
        json!({"repository": "Vyttle-LLC/plugins", "reason": "  "}),
        json!({"repository": "Vyttle-LLC/plugins", "reason": "One\nTwo"}),
        json!({"repository": "Vyttle-LLC/plugins", "reason": "Why", "space": "Vyttle"}),
    ] {
        assert!(f.host.agent_tool(&id, "request_repository", args).is_err());
    }
    assert!(f.items().is_empty());
}

#[test]
fn the_prefix_is_the_hosts_alone() {
    let mut f = fixture();
    let forged = json!({"request_id": "repository:forged", "question": "Add it?"});
    let id = f.coordinator.id.clone();
    assert!(f.host.agent_tool(&id, "ask_user", forged).is_err());
}

#[test]
fn a_waiting_request_survives_a_restart() {
    let mut f = fixture();
    f.ask("Vyttle-LLC/plugins").unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    drop(std::mem::replace(
        &mut f.host,
        Host::open_set_up(elsewhere.path()).unwrap(),
    ));
    f.host = Host::open(f.home.path().join("home")).unwrap();

    assert_eq!(f.items().len(), 1);
    assert_eq!(
        f.ask("Vyttle-LLC/plugins").unwrap()["status"],
        "waiting_for_human"
    );
    assert_eq!(f.items().len(), 1);
    let answered = f.answer("approved");

    assert_eq!(f.used(), ["plugins"]);
    assert_eq!(f.result(&answered).receipt, Receipt::Queued);
}

#[test]
fn a_folder_gone_is_told_to_the_coordinator() {
    let mut f = fixture();
    let path = f.new_clone("fresh");
    f.ask(&path).unwrap();
    fs::remove_dir_all(&path).unwrap();

    let answered = f.answer("approved");

    let body = f.result(&answered).body;
    assert!(
        body.contains("was not added") && body.contains("gone"),
        "{body}"
    );
    assert!(!(f.host.repositories().unwrap().iter()).any(|r| r.path == path));
    assert!(f.items().is_empty());
}

#[test]
fn a_folder_changed_after_the_card_is_not_added() {
    let mut f = fixture();
    let path = f.new_clone("fresh");
    f.ask(&path).unwrap();
    // The path now leads somewhere the human never saw.
    fs::remove_dir_all(&path).unwrap();
    let swapped = f.new_clone("swapped");
    std::os::unix::fs::symlink(&swapped, &path).unwrap();

    let answered = f.answer("approved");

    let body = f.result(&answered).body;
    assert!(body.contains("did not see"), "{body}");
    assert!(!(f.host.repositories().unwrap().iter()).any(|r| r.path == swapped));

    // The same path, now holding another repository.
    let path = f.new_clone("retargeted");
    f.ask(&path).unwrap();
    git(
        Path::new(&path),
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/octo/other.git",
        ],
    );
    let answered = f.answer("approved");
    let body = f.result(&answered).body;
    assert!(body.contains("origin changed"), "{body}");
    assert!(f.used().is_empty());
}

#[test]
fn a_repository_removed_while_the_card_waits_is_told_to_the_coordinator() {
    let mut f = fixture();
    f.ask("Vyttle-LLC/plugins").unwrap();
    f.host.remove_repository(&f.plugins.id).unwrap();

    let answered = f.answer("approved");

    let body = f.result(&answered).body;
    assert!(body.contains("removed from the workspace"), "{body}");
}

#[test]
fn an_archived_coordinator_hears_why_when_it_is_restored() {
    let mut f = fixture();
    f.ask("Vyttle-LLC/plugins").unwrap();
    f.host.set_archived(&f.coordinator.id, true).unwrap();

    let answered = f.answer("approved");

    assert!(f.used().is_empty());
    f.host.set_archived(&f.coordinator.id, false).unwrap();
    let result = f.result(&answered);
    assert_eq!(result.receipt, Receipt::Queued);
    assert!(result.body.contains("archived"), "{}", result.body);
}

#[test]
fn an_older_item_loads_without_a_request() {
    let item: Attention = serde_json::from_value(json!({"id": "a", "project_id": "p",
        "session_id": "s", "host": "local", "operation_id": "q", "prompt": "Why?",
        "answer": null}))
    .unwrap();
    assert_eq!(item.repository_request, None);
    assert!(
        !serde_json::to_string(&item)
            .unwrap()
            .contains("repository_request")
    );
}
