//! A Space following root folders: what a root check adds to it, and what it leaves out.
mod common;
use common::git;
use rusqlite::Connection;
use std::{fs, path::Path, path::PathBuf};
use workspace_core::*;
use workspace_host::Host;

/// A committed repository at `path`, on GitHub as `identity` when it has one.
fn clone_of(path: &Path, identity: Option<&str>) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "-b", "main"]);
    git(
        path,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "Fixture",
        ],
    );
    if let Some(identity) = identity {
        let url = format!("https://github.com/{identity}.git");
        git(path, &["remote", "add", "origin", &url]);
    }
}

struct Fixture {
    _folder: tempfile::TempDir,
    home: tempfile::TempDir,
    host: Host,
    vyttle: PathBuf,
    other: PathBuf,
    space: SpaceId,
}

/// Two root folders with a repository each, and a Space named Vyttle listing nothing.
fn fixture() -> Fixture {
    let folder = tempfile::tempdir().unwrap();
    let dev = fs::canonicalize(folder.path()).unwrap();
    let (vyttle, other) = (dev.join("vyttle"), dev.join("vyttle-llc"));
    clone_of(&vyttle.join("site"), Some("Vyttle-LLC/site"));
    clone_of(&other.join("tool"), Some("Vyttle-LLC/tool"));
    let home = tempfile::tempdir().unwrap();
    let mut host = Host::open_set_up(home.path()).unwrap();
    let paths = [path(&vyttle.join("site")), path(&other.join("tool"))];
    host.import_repositories(None, &paths, &[path(&vyttle), path(&other)], &[])
        .unwrap();
    let space = host.create_space("Vyttle", "", "", None).unwrap().space.id;
    Fixture {
        _folder: folder,
        home,
        host,
        vyttle,
        other,
        space,
    }
}

fn path(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}

impl Fixture {
    fn follow(&mut self, space: &SpaceId, roots: &[&Path]) {
        let roots = roots.iter().map(|root| path(root)).collect();
        self.host.set_followed_roots(space, roots).unwrap();
    }
    fn listed(&self, space: &SpaceId) -> Vec<String> {
        self.host.space(space).unwrap().space.repositories
    }
    fn followed(&self, space: &SpaceId) -> Vec<String> {
        self.host.space(space).unwrap().local.followed_roots
    }
    /// The Space's storage repository on disk.
    fn storage(&self, space: &SpaceId) -> PathBuf {
        let key = self
            .host
            .space(space)
            .unwrap()
            .space
            .storage
            .repository
            .key();
        self.home.path().join("repos").join(key)
    }
    fn space_file(&self, space: &SpaceId) -> PathBuf {
        let folder = self.host.space(space).unwrap().space.storage.folder;
        self.storage(space)
            .join("spaces")
            .join(folder)
            .join("space.yaml")
    }
}

#[test]
fn a_repository_first_found_in_either_followed_root_joins_with_a_commit() {
    let mut f = fixture();
    let space = f.space.clone();
    f.follow(&space, &[&f.vyttle.clone(), &f.other.clone()]);
    let commits = |f: &Fixture| git(&f.storage(&space), &["rev-list", "--count", "HEAD"]);
    let before: u32 = commits(&f).trim().parse().unwrap();

    clone_of(&f.vyttle.join("pica-web"), Some("Vyttle-LLC/pica-web"));
    clone_of(&f.other.join("pica-api"), Some("Vyttle-LLC/pica-api"));
    let refresh = f.host.refresh_repositories().unwrap();

    assert_eq!(refresh.added.len(), 2, "{:?}", refresh.warnings);
    assert_eq!(
        f.listed(&space),
        ["Vyttle-LLC/pica-web", "Vyttle-LLC/pica-api"]
    );
    let named = |line: &str| refresh.joined.iter().any(|joined| joined.starts_with(line));
    assert!(
        named("pica-web joined Vyttle from "),
        "{:?}",
        refresh.joined
    );
    assert!(
        named("pica-api joined Vyttle from "),
        "{:?}",
        refresh.joined
    );
    let after: u32 = commits(&f).trim().parse().unwrap();
    assert_eq!(after, before + 2);

    let yaml = fs::read_to_string(f.space_file(&space)).unwrap();
    assert!(yaml.contains("Vyttle-LLC/pica-web"));
    assert!(!yaml.contains("followed"), "{yaml}");
    assert!(!yaml.contains(&path(&f.vyttle)), "{yaml}");
}

#[test]
fn two_spaces_on_one_root_both_gain_a_repository_and_others_do_not() {
    let mut f = fixture();
    let (first, root) = (f.space.clone(), f.vyttle.clone());
    let second = f.host.create_space("Sage", "", "", None).unwrap().space.id;
    let bystander = f
        .host
        .create_space("Elsewhere", "", "", None)
        .unwrap()
        .space
        .id;
    f.follow(&first, &[&root]);
    f.follow(&second, &[&root]);
    let other = f.other.clone();
    f.follow(&bystander, &[&other]);

    clone_of(&root.join("web"), Some("Vyttle-LLC/web"));
    let refresh = f.host.refresh_repositories().unwrap();

    assert_eq!(f.listed(&first), ["Vyttle-LLC/web"]);
    assert_eq!(f.listed(&second), ["Vyttle-LLC/web"]);
    assert!(f.listed(&bystander).is_empty());
    assert_eq!(refresh.joined.len(), 2, "{:?}", refresh.joined);
}

#[test]
fn only_a_first_find_acts_so_what_the_human_removed_stays_out() {
    let mut f = fixture();
    let (space, root) = (f.space.clone(), f.vyttle.clone());
    f.follow(&space, &[&root]);
    clone_of(&root.join("web"), Some("Vyttle-LLC/web"));
    f.host.refresh_repositories().unwrap();

    f.host.set_repository_spaces("Vyttle-LLC/web", &[]).unwrap();
    assert!(f.host.refresh_repositories().unwrap().joined.is_empty());
    assert!(f.listed(&space).is_empty());

    // As the read after each root check does, so the workspace knows the clone it holds.
    Connection::open(f.home.path().join("workspace.sqlite3"))
        .unwrap()
        .execute(
            "UPDATE repositories SET data=json_set(data,'$.github',json(?1)) WHERE json_extract(data,'$.name')='web'",
            [r#"{"owner":"Vyttle-LLC","name":"web"}"#],
        )
        .unwrap();
    clone_of(&root.join("web-copy"), Some("Vyttle-LLC/web"));
    let refresh = f.host.refresh_repositories().unwrap();
    assert_eq!(
        refresh.added.len(),
        1,
        "another clone reaches the workspace"
    );
    assert!(refresh.joined.is_empty(), "{:?}", refresh.joined);
    assert!(f.listed(&space).is_empty());
}

#[test]
fn repositories_already_here_added_by_hand_or_not_on_github_join_nothing() {
    let mut f = fixture();
    let (space, root) = (f.space.clone(), f.vyttle.clone());
    f.follow(&space, &[&root]);

    // What was here before following is not a find.
    assert!(f.host.refresh_repositories().unwrap().joined.is_empty());

    clone_of(&root.join("by-hand"), Some("Vyttle-LLC/by-hand"));
    f.host
        .import_repositories(None, &[path(&root.join("by-hand"))], &[], &[])
        .unwrap();
    assert!(f.host.refresh_repositories().unwrap().joined.is_empty());

    clone_of(&root.join("sketches"), None);
    let refresh = f.host.refresh_repositories().unwrap();
    assert_eq!(refresh.added.len(), 1, "it still joins the workspace");
    assert!(refresh.joined.is_empty());
    assert!(f.listed(&space).is_empty());
}

#[test]
fn a_repository_the_space_already_lists_is_not_written_again() {
    let mut f = fixture();
    let (space, root) = (f.space.clone(), f.vyttle.clone());
    f.follow(&space, &[&root]);
    f.host
        .set_repository_spaces("vyttle-llc/WEB", std::slice::from_ref(&space))
        .unwrap();

    clone_of(&root.join("web"), Some("Vyttle-LLC/web"));
    assert!(f.host.refresh_repositories().unwrap().joined.is_empty());
    assert_eq!(f.listed(&space), ["vyttle-llc/WEB"]);
}

#[test]
fn a_failed_space_write_keeps_the_repository_and_warns_without_retrying() {
    let mut f = fixture();
    let (space, root) = (f.space.clone(), f.vyttle.clone());
    f.follow(&space, &[&root]);
    let folder = f.space_file(&space).parent().unwrap().to_owned();
    fs::remove_dir_all(&folder).unwrap();
    fs::write(&folder, "in the way").unwrap();

    clone_of(&root.join("web"), Some("Vyttle-LLC/web"));
    let refresh = f.host.refresh_repositories().unwrap();

    assert_eq!(refresh.added.len(), 1);
    assert!(refresh.joined.is_empty());
    assert_eq!(refresh.not_joined.len(), 1, "{:?}", refresh.not_joined);
    assert!(refresh.not_joined[0].starts_with("web stayed out of Vyttle"));
    assert!(f.listed(&space).is_empty());

    fs::remove_file(&folder).unwrap();
    fs::create_dir_all(&folder).unwrap();
    let again = f.host.refresh_repositories().unwrap();
    assert!(again.joined.is_empty() && again.not_joined.is_empty());
    assert!(f.listed(&space).is_empty());
}

#[test]
fn removing_a_root_ends_following_in_every_space_and_keeps_the_repositories() {
    let mut f = fixture();
    let (first, root, other) = (f.space.clone(), f.vyttle.clone(), f.other.clone());
    let second = f.host.create_space("Sage", "", "", None).unwrap().space.id;
    f.follow(&first, &[&root, &other]);
    f.follow(&second, &[&root]);
    f.host
        .set_repository_spaces("Vyttle-LLC/site", std::slice::from_ref(&first))
        .unwrap();

    f.host.remove_repository_root(&path(&root)).unwrap();

    assert_eq!(f.followed(&first), [path(&other)]);
    assert!(f.followed(&second).is_empty());
    assert_eq!(f.listed(&first), ["Vyttle-LLC/site"]);
}

#[test]
fn only_a_root_folder_can_be_followed() {
    let mut f = fixture();
    let (space, root) = (f.space.clone(), f.vyttle.clone());
    let refused = f
        .host
        .set_followed_roots(&space, vec![path(&root.join("site"))])
        .unwrap_err();
    assert!(refused.to_string().contains("no longer a root folder"));

    let twice = vec![path(&root), path(&root)];
    let record = f.host.set_followed_roots(&space, twice).unwrap();
    assert_eq!(record.local.followed_roots, [path(&root)]);

    let stopped = f.host.set_followed_roots(&space, vec![]).unwrap();
    assert!(stopped.local.followed_roots.is_empty());
}

#[test]
fn a_store_from_before_following_loads_with_no_followed_roots() {
    let older: SpaceLocal = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
    assert_eq!(older, SpaceLocal::default());
    let written = serde_json::to_string(&SpaceLocal::default()).unwrap();
    assert!(!written.contains("followed_roots"), "{written}");
}
