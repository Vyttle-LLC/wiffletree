//! A repository's one Wiffletree folder (OpenSpec change `one-space-repository`, decision 3).
//! Every path of a Space is relative to the folder, and `Root` turns those into paths of the
//! repository, which is how sync's rule sets and record name them. An empty root gives a
//! repository not yet moved today's paths exactly. A joining Mac finds the folder by reading the
//! tree, and the move into it is one commit that leaves a forwarding stub at each old
//! `space.yaml` and tracker, which an older host pauses as written by a newer Wiffletree.
use crate::*;
use std::collections::{BTreeMap, BTreeSet};
use storage::TRACKER;
use tracker::ISSUES;

/// What a repository's Space rule sets cover, relative to the folder. A move carries these and
/// leaves the settings rule set's files at the root.
const COVERED: [&str; 5] = [
    "spaces",
    ISSUES,
    TRACKER,
    storage::COMPILERS,
    "second-brain",
];

/// The key only a forwarding stub has.
const MOVED_TO: &str = "moved_to";
/// The format of a forwarding stub, which no host reads as a Space or a tracker.
const STUB_FORMAT: u32 = 2;

/// A repository's Wiffletree folder, or its root for an empty one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Root(String);
impl Root {
    pub(crate) fn new(root: &str) -> Self {
        Self(root.to_owned())
    }
    /// The folder a repository with no Wiffletree files gets.
    pub(crate) fn default_folder() -> Self {
        Self::new(DEFAULT_ROOT)
    }
    pub(crate) fn name(&self) -> &str {
        &self.0
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// The path in the repository of `path`, relative to the folder.
    pub(crate) fn join(&self, path: &str) -> String {
        match self.is_empty() {
            true => path.to_owned(),
            false => format!("{}/{path}", self.0),
        }
    }
    /// The path relative to the folder of `path` in the repository, `None` outside the folder.
    pub(crate) fn strip<'a>(&self, path: &'a str) -> Option<&'a str> {
        match self.is_empty() {
            true => Some(path),
            false => path.strip_prefix(self.0.as_str())?.strip_prefix('/'),
        }
    }
    /// Where the folder is in the clone at `clone`.
    pub(crate) fn in_clone(&self, clone: &Path) -> PathBuf {
        match self.is_empty() {
            true => clone.to_owned(),
            false => clone.join(&self.0),
        }
    }
    /// The issue whose file `path` of the repository is.
    pub(crate) fn issue_of(&self, path: &str) -> Option<IssueId> {
        tracker::issue_of(self.strip(path)?)
    }
    /// Whether `path` of the repository is a file a scan reads before a push.
    pub(crate) fn scanned(&self, path: &str) -> bool {
        self.strip(path).is_some_and(scan::scanned)
    }
    /// Where `path` of the repository was before the repository moved into a folder.
    pub(crate) fn moved_from<'a>(&self, path: &'a str) -> Option<&'a str> {
        self.strip(path).filter(|_| !self.is_empty())
    }
    /// Whether `path` of the repository is outside a folder that is not the root.
    fn outside(&self, path: &str) -> bool {
        !self.is_empty() && self.strip(path).is_none()
    }
}

/// How many commits up to the clone's head wrote the file at `path` of the repository, in the
/// folder or before the move. The move's own commit touches both paths and writes nothing new.
pub(crate) fn saves(clone: &Path, root: &Root, path: &str) -> Result<u32> {
    let count = |path: &str| -> Result<u32> {
        let args = ["rev-list", "--count", "HEAD", "--", path];
        Ok(runtime::git_output(clone, &args)?.trim().parse()?)
    };
    let here = count(path)?;
    let before = root.moved_from(path).map(count).transpose()?;
    Ok(match before {
        None | Some(0) => here,
        Some(before) => here + before - 2,
    })
}

/// Whether `path`, at a repository's root, is one of the files a move carries.
pub(crate) fn moves(path: &str) -> bool {
    !path.split('/').any(|part| part.starts_with('.'))
        && (COVERED.iter()).any(|covered| {
            path == *covered
                || path
                    .strip_prefix(covered)
                    .is_some_and(|r| r.starts_with('/'))
        })
}

/// A stub's place: the `space.yaml` of a Space, or the tracker, at a repository's root.
fn stubbed(path: &str) -> bool {
    path == TRACKER
        || (path.strip_prefix("spaces/"))
            .and_then(|rest| rest.strip_suffix("/space.yaml"))
            .is_some_and(|space| !space.contains('/'))
}

/// The bytes of the forwarding stub for `content`, the file at `path`.
fn stub_for(path: &str, content: &[u8], root: &Root) -> Result<Vec<u8>> {
    let file: serde_yaml::Value =
        serde_yaml::from_slice(content).with_context(|| format!("{path} can't be read"))?;
    let keys: &[&str] = match path == TRACKER {
        true => &["prefix"],
        false => &["id", "name"],
    };
    let mut stub = serde_yaml::Mapping::new();
    stub.insert("format".into(), STUB_FORMAT.into());
    for key in keys {
        let value = file
            .get(key)
            .with_context(|| format!("{path} has no {key}"))?;
        stub.insert((*key).into(), value.clone());
    }
    stub.insert(MOVED_TO.into(), root.name().into());
    Ok(serde_yaml::to_string(&stub)?.into_bytes())
}

/// Where a forwarding stub says its files went, `None` for any other file.
fn stub_target(content: &[u8]) -> Option<String> {
    let file: serde_yaml::Value = serde_yaml::from_slice(content).ok()?;
    Some(file.get(MOVED_TO)?.as_str()?.to_owned())
}

/// The folder a path such as `<folder>/spaces/<space>/space.yaml` or
/// `<folder>/wiffletree-issues.yaml` is in; empty at the repository's root.
fn holder(path: &str) -> Option<&str> {
    if path == TRACKER {
        return Some("");
    }
    if let Some(folder) = path.strip_suffix(&format!("/{TRACKER}")) {
        return Some(folder);
    }
    let (spaces, _) = path.strip_suffix("/space.yaml")?.rsplit_once('/')?;
    match spaces == "spaces" {
        true => Some(""),
        false => spaces.strip_suffix("/spaces"),
    }
}

/// Every folder at `commit` holding a Space or a tracker, stubs skipped, sorted, with the root
/// as the empty folder. A board output named like one of these files is not a folder of its own.
fn folders_at(clone: &Path, commit: &str) -> Result<Vec<String>> {
    let hidden = |path: &str| path.split('/').any(|part| part.starts_with('.'));
    let files = repos::files_at(clone, commit, &|path| {
        !hidden(path) && holder(path).is_some()
    })?;
    let found: BTreeSet<&str> = (files.iter())
        .filter(|(_, content)| stub_target(content).is_none())
        .filter_map(|(path, _)| holder(path))
        .collect();
    let inside = |folder: &str, other: &str| {
        let other = Root::new(other);
        (other.strip(folder)).is_some_and(|rest| {
            folder != other.name() && (rest.starts_with("spaces/") || rest.starts_with("issues/"))
        })
    };
    Ok((found.iter())
        .filter(|folder| !found.iter().any(|other| inside(folder, other)))
        .map(|folder| (*folder).to_owned())
        .collect())
}

/// The folder a repository holds at `commit`: `None` when it holds no Wiffletree files, and the
/// empty root for a layout at its root. Two are refused, naming both.
pub(crate) fn find(clone: &Path, commit: &str) -> Result<Option<Root>> {
    let folders = folders_at(clone, commit)?;
    if let [_, _, ..] = folders.as_slice() {
        let named: Vec<String> = (folders.iter())
            .map(|folder| match folder.is_empty() {
                true => "the repository's root".to_owned(),
                false => format!("{folder}/"),
            })
            .collect();
        bail!(
            "This repository holds Wiffletree files in {}, and a repository holds one folder",
            named.join(" and ")
        );
    }
    Ok(folders.first().map(|folder| Root::new(folder)))
}

/// The folder a repository's files moved into by `commit`, for a Mac that has them at the root;
/// `None` while its one layout is still at the root.
pub(crate) fn moved_to(clone: &Path, commit: &str) -> Result<Option<Root>> {
    let folders = folders_at(clone, commit)?;
    Ok(match folders.as_slice() {
        [one] if !one.is_empty() => Some(Root::new(one)),
        _ => None,
    })
}

/// Each path at `commit`, with its mode and blob.
fn tree(clone: &Path, commit: &str) -> Result<BTreeMap<String, String>> {
    let listing = runtime::git_output(clone, &["ls-tree", "-r", "-z", commit])?;
    let mut paths = BTreeMap::new();
    for entry in listing.split('\0').filter(|entry| !entry.is_empty()) {
        let (kind, path) = entry
            .split_once('\t')
            .context("Git listed an entry with no path")?;
        paths.insert(path.to_owned(), kind.to_owned());
    }
    Ok(paths)
}

/// Stages the move on top of `head`, whose tree the index holds: each file a move carries goes
/// to the folder with no change of content, and a stub stays at each old `space.yaml` and
/// tracker. Refused when the folder already holds files, as it must be Wiffletree's.
pub(crate) fn stage_move(clone: &Path, head: &str, root: &Root) -> Result<()> {
    let before = tree(clone, head)?;
    ensure!(
        !before.keys().any(|path| root.strip(path).is_some()),
        "{}/ already holds files, and the folder must be Wiffletree's",
        root.name()
    );
    let contents = repos::files_at(clone, head, &|path| moves(path) && stubbed(path))?;
    let mut index = String::new();
    for (path, kind) in before.iter().filter(|(path, _)| moves(path)) {
        index.push_str(&format!("{kind}\t{}\0", root.join(path)));
        match contents.get(path) {
            Some(content) => {
                let stub = stub_for(path, content, root)?;
                let id = runtime::git_bytes(clone, &["hash-object", "-w", "--stdin"], &stub)?;
                let id = String::from_utf8(id)?;
                index.push_str(&format!("100644 {}\t{path}\0", id.trim()));
            }
            None => index.push_str(&format!("0 {}\t{path}\0", "0".repeat(40))),
        }
    }
    runtime::git_bytes(
        clone,
        &["update-index", "-z", "--index-info"],
        index.as_bytes(),
    )?;
    Ok(())
}

/// The move's own check, of `commit` on `parent`: each path in the folder holds the blob its old
/// path held in the parent, and every other path is a stub or as it was. This is by content, as
/// a stub at the old path makes Git report those files as modified and added, not renamed.
pub(crate) fn check_move(clone: &Path, parent: &str, commit: &str, root: &Root) -> Result<()> {
    let (before, after) = (tree(clone, parent)?, tree(clone, commit)?);
    for (path, kind) in &before {
        let kept = if moves(path) {
            after.get(&root.join(path))
        } else {
            after.get(path)
        };
        ensure!(kept == Some(kind), "The move changes {path}");
    }
    let contents = repos::files_at(clone, commit, &|path| root.outside(path) && stubbed(path))?;
    for (path, kind) in &after {
        let same = match root.strip(path) {
            Some(old) => before.get(old) == Some(kind) && moves(old),
            None if before.get(path) == Some(kind) => !moves(path),
            None => {
                (contents.get(path))
                    .and_then(|stub| stub_target(stub))
                    .as_deref()
                    == Some(root.name())
            }
        };
        ensure!(same, "The move adds {path}, which is no moved file or stub");
    }
    Ok(())
}

/// Moves this clone's files a move carries under the folder, unpushed ones with them. Run again
/// after a start cut short, it leaves a file already there and a file left at the root, which is
/// a stub or what the first run moved.
pub(crate) fn move_files(clone: &Path, root: &Root) -> Result<()> {
    for covered in COVERED {
        let at = clone.join(covered);
        let files = match at.is_dir() {
            true => storage::files_under(&at)?,
            false if at.is_file() => vec![at.clone()],
            false => vec![],
        };
        for file in files {
            let moved = root.in_clone(clone).join(file.strip_prefix(clone)?);
            if !moved.exists() {
                fs::create_dir_all(moved.parent().context("A file is in a folder")?)?;
                fs::rename(&file, &moved)?;
            }
        }
        remove_empty(&at);
    }
    Ok(())
}

/// Removes the folders under `folder` that hold no file, and `folder` itself if then empty.
fn remove_empty(folder: &Path) {
    for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            remove_empty(&entry.path());
        }
    }
    drop(fs::remove_dir(folder));
}

/// Writes the stubs `commit` holds at the root back into the clone.
pub(crate) fn restore_stubs(clone: &Path, commit: &str, root: &Root) -> Result<()> {
    let stubs: Vec<String> = (tree(clone, commit)?.into_keys())
        .filter(|path| root.outside(path) && stubbed(path))
        .collect();
    if !stubs.is_empty() {
        let stubs: Vec<&str> = stubs.iter().map(String::as_str).collect();
        runtime::git_output(clone, &[&["checkout", commit, "--"], &stubs[..]].concat())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::git;

    /// A repository holding each of `files`, committed; its head.
    fn repository(folder: &Path, files: &[(&str, &str)]) -> String {
        git(folder, &["init", "-q", "--initial-branch=main"]);
        for (path, content) in files {
            let file = folder.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, content).unwrap();
        }
        git(folder, &["add", "--all"]);
        git(
            folder,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "Start",
            ],
        );
        git(folder, &["rev-parse", "HEAD"]).trim().to_owned()
    }

    const SAGE: &str = "format: 1\nid: sage-id\nname: Sage\n";
    const TRACKER_FILE: &str = "format: 1\nprefix: SAGE\n";

    fn found(files: &[(&str, &str)]) -> Result<Option<Root>> {
        let folder = tempfile::tempdir().unwrap();
        let head = repository(folder.path(), files);
        find(folder.path(), &head)
    }

    #[test]
    fn a_root_joins_and_strips_and_an_empty_one_gives_todays_paths() {
        let empty = Root::default();
        assert_eq!(empty.join("issues/A-1/issue.md"), "issues/A-1/issue.md");
        assert_eq!(empty.strip("README.md"), Some("README.md"));
        let root = Root::new("team/wiffletree");
        assert_eq!(root.join(TRACKER), format!("team/wiffletree/{TRACKER}"));
        assert_eq!(root.strip(&root.join("spaces/a")), Some("spaces/a"));
        assert_eq!(root.strip("team/wiffletree-2/spaces/a"), None);
        assert_eq!(root.strip("README.md"), None);
        assert_eq!(
            root.issue_of("team/wiffletree/issues/SAGE-1/issue.md")
                .unwrap()
                .to_string(),
            "SAGE-1"
        );
        assert!(root.scanned("team/wiffletree/spaces/a/space.yaml"));
        assert!(!root.scanned("spaces/a/space.yaml"));
    }

    #[test]
    fn a_valid_folder_is_a_relative_path_of_plain_names() {
        for good in ["wiffletree", "team/wiffletree", "a b/c"] {
            validate_root(good).unwrap();
        }
        for bad in [
            "", "/etc", "a/", "a//b", ".", "..", "a/../b", ".hidden", "a/.b",
        ] {
            assert!(validate_root(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn one_folder_is_found_with_its_space_or_its_tracker() {
        let space = found(&[("wiffletree/spaces/vyttle/space.yaml", SAGE)]);
        assert_eq!(space.unwrap(), Some(Root::new("wiffletree")));
        let tracker = found(&[("wiffletree/wiffletree-issues.yaml", TRACKER_FILE)]);
        assert_eq!(tracker.unwrap(), Some(Root::new("wiffletree")));
    }

    #[test]
    fn a_custom_nested_folder_is_found() {
        let files = [
            ("team/wiffletree/spaces/sage/space.yaml", SAGE),
            ("team/wiffletree/wiffletree-issues.yaml", TRACKER_FILE),
            ("README.md", "Other documents.\n"),
        ];
        assert_eq!(found(&files).unwrap(), Some(Root::new("team/wiffletree")));
    }

    #[test]
    fn two_folders_are_refused_naming_both() {
        let error = found(&[
            ("wiffletree/spaces/sage/space.yaml", SAGE),
            ("other/spaces/watch/space.yaml", SAGE),
        ])
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("other/") && error.contains("wiffletree/"),
            "{error}"
        );
    }

    #[test]
    fn a_layout_at_the_root_is_joined_as_it_stands() {
        let files = [("spaces/sage/space.yaml", SAGE), (TRACKER, TRACKER_FILE)];
        assert_eq!(found(&files).unwrap(), Some(Root::default()));
    }

    #[test]
    fn a_repository_of_other_documents_holds_no_folder() {
        assert_eq!(found(&[("docs/README.md", "Hello.\n")]).unwrap(), None);
    }

    #[test]
    fn stubs_are_skipped_so_a_moved_repository_has_one_folder() {
        let stub = "format: 2\nid: sage-id\nname: Sage\nmoved_to: wiffletree\n";
        let files = [
            ("spaces/sage/space.yaml", stub),
            (TRACKER, "format: 2\nprefix: SAGE\nmoved_to: wiffletree\n"),
            ("wiffletree/spaces/sage/space.yaml", SAGE),
        ];
        assert_eq!(found(&files).unwrap(), Some(Root::new("wiffletree")));
    }

    #[test]
    fn a_board_output_named_like_a_tracker_is_no_second_folder() {
        let files = [
            ("wiffletree/spaces/sage/space.yaml", SAGE),
            (
                "wiffletree/spaces/sage/boards/job/out/wiffletree-issues.yaml",
                "x\n",
            ),
        ];
        assert_eq!(found(&files).unwrap(), Some(Root::new("wiffletree")));
    }

    /// A repository at the root, moved by one commit; the folder and both commits.
    fn moved(files: &[(&str, &str)]) -> (tempfile::TempDir, String, String) {
        let folder = tempfile::tempdir().unwrap();
        let head = repository(folder.path(), files);
        let root = Root::default_folder();
        stage_move(folder.path(), &head, &root).unwrap();
        git(
            folder.path(),
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "Move",
            ],
        );
        let moved = git(folder.path(), &["rev-parse", "HEAD"]).trim().to_owned();
        (folder, head, moved)
    }

    const ROOT_LAYOUT: [(&str, &str); 5] = [
        ("spaces/sage/space.yaml", SAGE),
        ("spaces/sage/boards/job/out.json", "{}\n"),
        (TRACKER, TRACKER_FILE),
        ("issues/SAGE-1/issue.md", "---\nid: SAGE-1\n---\n"),
        ("settings/review.json", "{}\n"),
    ];

    #[test]
    fn the_move_carries_each_file_unchanged_with_a_stub_at_each_old_space_and_tracker() {
        let (folder, parent, commit) = moved(&ROOT_LAYOUT);
        let root = Root::default_folder();
        check_move(folder.path(), &parent, &commit, &root).unwrap();
        let listed = git(folder.path(), &["ls-tree", "-r", "--name-only", &commit]);
        let listed: BTreeSet<&str> = listed.lines().collect();
        let expected: BTreeSet<&str> = [
            "settings/review.json",
            "spaces/sage/space.yaml",
            TRACKER,
            "wiffletree/spaces/sage/space.yaml",
            "wiffletree/spaces/sage/boards/job/out.json",
            "wiffletree/wiffletree-issues.yaml",
            "wiffletree/issues/SAGE-1/issue.md",
        ]
        .into();
        assert_eq!(listed, expected);
        let stub = git(
            folder.path(),
            &["show", &format!("{commit}:spaces/sage/space.yaml")],
        );
        assert_eq!(
            stub,
            "format: 2\nid: sage-id\nname: Sage\nmoved_to: wiffletree"
        );
        assert_eq!(find(folder.path(), &commit).unwrap(), Some(root));
    }

    #[test]
    fn the_move_refuses_a_folder_that_already_holds_files() {
        let folder = tempfile::tempdir().unwrap();
        let head = repository(
            folder.path(),
            &[
                ("spaces/sage/space.yaml", SAGE),
                ("wiffletree/notes.md", "Mine.\n"),
            ],
        );
        let error = stage_move(folder.path(), &head, &Root::default_folder()).unwrap_err();
        assert!(
            error.to_string().contains("must be Wiffletree's"),
            "{error}"
        );
    }

    #[test]
    fn the_check_refuses_a_changed_file_and_anything_else_the_commit_holds() {
        let check = |path: &str, content: &str| {
            let (folder, parent, _) = moved(&ROOT_LAYOUT);
            let file = folder.path().join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, content).unwrap();
            git(folder.path(), &["add", "--all"]);
            git(
                folder.path(),
                &[
                    "-c",
                    "user.name=T",
                    "-c",
                    "user.email=t@t",
                    "commit",
                    "-qm",
                    "Slip",
                ],
            );
            let head = git(folder.path(), &["rev-parse", "HEAD"]).trim().to_owned();
            check_move(folder.path(), &parent, &head, &Root::default_folder())
        };
        for (path, content) in [
            ("wiffletree/issues/SAGE-1/issue.md", "Changed.\n"),
            ("wiffletree/issues/SAGE-2/issue.md", "Added.\n"),
            ("README.md", "Beside the move.\n"),
            ("settings/review.json", "[]\n"),
            (TRACKER, "format: 1\nprefix: SAGE\n"),
        ] {
            assert!(check(path, content).is_err(), "{path}");
        }
    }
}
