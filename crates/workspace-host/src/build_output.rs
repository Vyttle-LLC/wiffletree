//! A ticket's build output: folders in its worktree that hold only what a build or install made,
//! and Xcode's DerivedData folders for the worktree's path, each with its size on disk.
//!
//! A folder in the worktree is a candidate when its name is a default for a common ecosystem,
//! it holds a `CACHEDIR.TAG`, or the repository's `.wiffletree/build-output` names it. It counts
//! as build output only when it is a real folder, not reached through a symbolic link, Git
//! ignores everything in it and tracks nothing in it, and it holds no Git repository. Any other
//! candidate is left alone and reported as ambiguous. This module only finds and measures: it
//! deletes nothing.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::runtime;

/// Build output folder names: Rust; Node; Apple; Python; JVM (with Apple's `build`); Terraform.
const DEFAULT_NAMES: &[&str] = &[
    "target",
    "node_modules",
    ".next",
    "dist",
    ".turbo",
    ".expo",
    "Pods",
    ".build",
    "build",
    ".venv",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".gradle",
    ".terraform",
];

/// The committed file in which a repository declares more build output folders, in addition to the
/// defaults: one path or glob per line, relative to the repository's root. `*` and `?` match
/// within one path segment, `**` any number of segments, and a line starting with `#` is a
/// comment.
pub const DECLARATION: &str = ".wiffletree/build-output";

/// The marker file that tools such as Cargo put in a folder that only holds a cache.
const CACHE_DIR_TAG: &str = "CACHEDIR.TAG";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOutput {
    pub found: Vec<Found>,
    pub ambiguous: Vec<Ambiguous>,
}

/// A build output folder and the bytes it holds on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub source: Source,
    pub size: u64,
}

/// Why a folder was a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    DefaultName,
    CacheDirTag,
    Declared,
    /// Xcode's DerivedData folder for a project in the worktree, outside the worktree.
    DerivedData,
}

/// A candidate folder that is not build output, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ambiguous {
    pub path: PathBuf,
    pub reason: Reason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Git tracks a file in it.
    HoldsTracked,
    /// It is, or holds, an untracked file that Git does not ignore.
    NotIgnored,
    /// It holds a Git repository, which may have work that exists nowhere else. SwiftPM's own
    /// clones, in the `checkouts` and `repositories` folders of `.build` or `SourcePackages`,
    /// do not count.
    HoldsRepository,
}

/// Xcode's DerivedData folder in the user's home, where it would be.
pub fn xcode_derived_data() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Developer/Xcode/DerivedData"))
}

/// The build output of the worktree whose root is `worktree`, with DerivedData folders from
/// `derived_data` when given, such as `xcode_derived_data()`.
///
/// Candidates come from the paths Git lists as ignored, untracked or tracked, the folders above
/// them, and the folders inside an ignored folder that is not a candidate itself. A folder inside
/// a candidate that counts as build output is not listed again. Symbolic links are never
/// followed: a candidate must be a real folder at its own path in the worktree.
///
/// It runs Git and walks every folder it finds, which can take seconds for a large build, so
/// call it on a worker thread, never on the host's own thread.
pub fn scan(worktree: &Path, derived_data: Option<&Path>) -> Result<BuildOutput> {
    let resolved = worktree.canonicalize()?;
    let status = Status::read(worktree)?;
    let tracked = tracked_paths(worktree)?;
    let declared = declared_patterns(worktree);
    let mut outputs = Vec::new();
    let mut ambiguous = Vec::new();
    let mut taken: Vec<String> = Vec::new();
    for (folder, source) in candidates(worktree, &status, &tracked, &declared) {
        if taken.iter().any(|outer| within(&folder, outer)) {
            continue;
        }
        let path = worktree.join(&folder);
        // A symbolic link above the folder would put it somewhere the Git checks did not cover.
        if path.canonicalize().ok() != Some(resolved.join(&folder)) {
            continue;
        }
        match status.why_not(&folder, &tracked) {
            None => {
                taken.push(folder);
                outputs.push((path, source));
            }
            Some(reason) => ambiguous.push(Ambiguous { path, reason }),
        }
    }
    if let Some(root) = derived_data {
        outputs.extend(
            derived_data_for(worktree, root)
                .into_iter()
                .map(|path| (path, Source::DerivedData)),
        );
    }
    let mut found = Vec::new();
    let mut counted = HashSet::new();
    for (path, source) in outputs {
        let measured = measure(&path, &mut counted);
        if measured.holds_repository {
            ambiguous.push(Ambiguous {
                path,
                reason: Reason::HoldsRepository,
            });
        } else {
            found.push(Found {
                path,
                source,
                size: measured.size,
            });
        }
    }
    Ok(BuildOutput { found, ambiguous })
}

fn is_real_folder(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

/// The paths `git status` lists as ignored or untracked, relative to the root. An untracked or
/// ignored folder as a whole is listed once, as `name/`; the slash is dropped here.
struct Status {
    ignored: Vec<String>,
    untracked: Vec<String>,
}
impl Status {
    fn read(worktree: &Path) -> Result<Self> {
        // `matching` lists an ignored folder without walking it, which matters for node_modules.
        let args = [
            "status",
            "--porcelain=v1",
            "-z",
            "--ignored=matching",
            "--untracked-files=normal",
        ];
        let output = runtime::git_bytes_large(worktree, &args, &[])?;
        let mut status = Self {
            ignored: Vec::new(),
            untracked: Vec::new(),
        };
        let mut records = output.split(|byte| *byte == 0);
        while let Some(record) = records.next() {
            let Some((code, path)) = record.split_at_checked(3) else {
                continue;
            };
            let path = String::from_utf8_lossy(path)
                .trim_end_matches('/')
                .to_owned();
            match code {
                b"!! " => status.ignored.push(path),
                b"?? " => status.untracked.push(path),
                // A rename or copy is followed by its source path.
                [b'R' | b'C', ..] | [_, b'R' | b'C', ..] => {
                    records.next();
                }
                _ => {}
            }
        }
        Ok(status)
    }

    /// Why `folder` is not build output, or `None` when every file in it is ignored and none is
    /// tracked. Untracked files are listed by themselves or by a folder holding them, so one in
    /// or above `folder` shows a file in it that Git does not ignore.
    fn why_not(&self, folder: &str, tracked: &HashSet<String>) -> Option<Reason> {
        if tracked.contains(folder) {
            Some(Reason::HoldsTracked)
        } else if self
            .untracked
            .iter()
            .any(|path| within(path, folder) || within(folder, path))
        {
            Some(Reason::NotIgnored)
        } else {
            None
        }
    }
}

/// Every tracked path and every folder above one, relative to the root.
fn tracked_paths(worktree: &Path) -> Result<HashSet<String>> {
    let output = runtime::git_bytes_large(worktree, &["ls-files", "-z"], &[])?;
    let mut paths = HashSet::new();
    for path in output
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let mut path = String::from_utf8_lossy(path).into_owned();
        while paths.insert(path.clone()) {
            match path.rsplit_once('/') {
                Some((parent, _)) => path = parent.to_owned(),
                None => break,
            }
        }
    }
    Ok(paths)
}

/// The candidate folders in path order, so a folder comes before those inside it: the ignored,
/// untracked and tracked paths' folders and those above them, and the folders inside an ignored
/// folder that is not a candidate itself, which Git lists without what it holds.
fn candidates(
    worktree: &Path,
    status: &Status,
    tracked: &HashSet<String>,
    declared: &[Vec<String>],
) -> BTreeMap<String, Source> {
    let mut search = Search {
        worktree,
        declared,
        considered: HashSet::new(),
        candidates: BTreeMap::new(),
    };
    for path in status.ignored.iter().chain(&status.untracked) {
        if is_real_folder(&worktree.join(path)) {
            search.upward(Some(path));
        } else {
            search.upward(parent(path));
        }
    }
    for path in tracked {
        search.upward(parent(path));
    }
    for path in &status.ignored {
        if is_real_folder(&worktree.join(path)) && !search.candidates.contains_key(path) {
            search.downward(path);
        }
    }
    search.candidates
}

struct Search<'a> {
    worktree: &'a Path,
    declared: &'a [Vec<String>],
    /// Every folder looked at, each with all the folders above it.
    considered: HashSet<String>,
    candidates: BTreeMap<String, Source>,
}
impl Search<'_> {
    fn upward(&mut self, mut folder: Option<&str>) {
        while let Some(current) = folder {
            if !self.consider(current) {
                break;
            }
            folder = parent(current);
        }
    }

    /// Looks at the folders inside `folder`, but not inside a candidate or a symbolic link.
    fn downward(&mut self, folder: &str) {
        let path = self.worktree.join(folder);
        if !is_real_folder(&path) {
            return;
        }
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let inner = format!("{folder}/{}", entry.file_name().to_string_lossy());
            if self.consider(&inner) && !self.candidates.contains_key(&inner) {
                self.downward(&inner);
            }
        }
    }

    /// Records `folder` as a candidate if it is one, or returns false if it was looked at before.
    fn consider(&mut self, folder: &str) -> bool {
        if !self.considered.insert(folder.to_owned()) {
            return false;
        }
        if !is_real_folder(&self.worktree.join(folder)) {
            return true;
        }
        if let Some(source) = source_of(self.worktree, folder, self.declared) {
            self.candidates.insert(folder.to_owned(), source);
        }
        true
    }
}

fn parent(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(parent, _)| parent)
}

fn source_of(worktree: &Path, folder: &str, declared: &[Vec<String>]) -> Option<Source> {
    let segments: Vec<&str> = folder.split('/').collect();
    let name = segments.last()?;
    if declared
        .iter()
        .any(|pattern| glob_matches(pattern, &segments))
    {
        Some(Source::Declared)
    } else if DEFAULT_NAMES.contains(name) {
        Some(Source::DefaultName)
    } else if worktree.join(folder).join(CACHE_DIR_TAG).is_file() {
        Some(Source::CacheDirTag)
    } else {
        None
    }
}

/// The repository's declared patterns as committed at `HEAD`, each split into path segments. An
/// uncommitted declaration, or change to one, declares nothing.
fn declared_patterns(worktree: &Path) -> Vec<Vec<String>> {
    let blob = format!("HEAD:{DECLARATION}");
    let text = match runtime::git_answer(worktree, &["cat-file", "blob", &blob]) {
        Ok(answer) if answer.status.success() => answer.output,
        _ => return Vec::new(),
    };
    text.lines()
        .map(|line| line.trim().trim_matches('/'))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split('/').map(str::to_owned).collect())
        .collect()
}

fn glob_matches(pattern: &[String], path: &[&str]) -> bool {
    match (pattern.split_first(), path.split_first()) {
        (None, None) => true,
        (Some((any, rest)), _) if any == "**" => {
            glob_matches(rest, path) || (!path.is_empty() && glob_matches(pattern, &path[1..]))
        }
        (Some((segment, rest)), Some((name, tail))) => {
            segment_matches(segment.as_bytes(), name.as_bytes()) && glob_matches(rest, tail)
        }
        _ => false,
    }
}

fn segment_matches(pattern: &[u8], name: &[u8]) -> bool {
    match (pattern.split_first(), name.split_first()) {
        (None, None) => true,
        (Some((b'*', rest)), _) => {
            segment_matches(rest, name)
                || (!name.is_empty() && segment_matches(pattern, &name[1..]))
        }
        (Some((b'?', rest)), Some((_, tail))) => segment_matches(rest, tail),
        (Some((wanted, rest)), Some((byte, tail))) => wanted == byte && segment_matches(rest, tail),
        _ => false,
    }
}

/// Whether the relative path `path` is `folder` or inside it.
fn within(path: &str, folder: &str) -> bool {
    path.strip_prefix(folder)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// The folders under `root` whose `info.plist` names a workspace or project in the worktree.
/// Xcode keys each by the project's path, so every worktree of a repository gets its own.
fn derived_data_for(worktree: &Path, root: &Path) -> Vec<PathBuf> {
    let resolved = worktree.canonicalize().unwrap_or_else(|_| worktree.into());
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|folder| {
            is_real_folder(folder)
                && fs::read_to_string(folder.join("info.plist"))
                    .ok()
                    .and_then(|plist| workspace_path(&plist))
                    .is_some_and(|path| {
                        let path = Path::new(&path);
                        path.starts_with(worktree) || path.starts_with(&resolved)
                    })
        })
        .collect();
    folders.sort();
    folders
}

/// The `WorkspacePath` string in an XML property list, the format Xcode writes it in.
fn workspace_path(plist: &str) -> Option<String> {
    let (_, after) = plist.split_once("<key>WorkspacePath</key>")?;
    let (value, _) = after
        .trim_start()
        .strip_prefix("<string>")?
        .split_once("</string>")?;
    Some(
        value
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&"),
    )
}

/// What a walk of a folder and everything under it saw, without following symbolic links.
#[derive(Default)]
struct Measured {
    /// The bytes allocated on disk. A file with several hard links counts once across a scan.
    /// APFS clones share blocks with the file they were cloned from, but each is counted in
    /// full, so a folder seeded with `cp -c` may be over-counted. Entries that cannot be read
    /// count as nothing.
    size: u64,
    /// Whether a `.git` is inside, other than in SwiftPM's clones.
    holds_repository: bool,
}

fn measure(path: &Path, counted: &mut HashSet<(u64, u64)>) -> Measured {
    let mut measured = Measured::default();
    measured.add(path, counted, true);
    measured
}

impl Measured {
    fn add(&mut self, path: &Path, counted: &mut HashSet<(u64, u64)>, repositories_count: bool) {
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return;
        };
        if repositories_count && path.file_name() == Some(".git".as_ref()) {
            self.holds_repository = true;
        }
        if metadata.nlink() > 1
            && !metadata.is_dir()
            && !counted.insert((metadata.dev(), metadata.ino()))
        {
            return;
        }
        self.size += metadata.blocks() * 512;
        if !metadata.is_dir() {
            return;
        }
        let swiftpm = matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some(".build" | "SourcePackages")
        );
        for entry in fs::read_dir(path).into_iter().flatten().flatten() {
            let clones = swiftpm
                && matches!(
                    entry.file_name().to_str(),
                    Some("checkouts" | "repositories")
                );
            self.add(&entry.path(), counted, repositories_count && !clones);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::git;

    fn write(root: &Path, path: &str, content: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// A repository committed with `files`, and a ticket worktree checked out from it.
    fn worktree(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        let main = folder.path().join("main");
        fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        for (path, content) in files {
            write(&main, path, content);
        }
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-q", "-m", "start"]);
        let worktree = folder.path().join("ticket");
        git(
            &main,
            &["worktree", "add", "-q", "-b", "ticket", "../ticket"],
        );
        (folder, worktree)
    }

    fn relative(root: &Path, path: &Path) -> String {
        path.strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    fn found(root: &Path, output: &BuildOutput) -> Vec<(String, Source)> {
        output
            .found
            .iter()
            .map(|found| (relative(root, &found.path), found.source))
            .collect()
    }

    fn ambiguous(root: &Path, output: &BuildOutput) -> Vec<(String, Reason)> {
        output
            .ambiguous
            .iter()
            .map(|ambiguous| (relative(root, &ambiguous.path), ambiguous.reason))
            .collect()
    }

    #[test]
    fn only_ignored_untracked_folders_are_build_output_and_user_work_is_left_alone() {
        let (_folder, root) = worktree(&[
            (".gitignore", "/target/\nnode_modules/\n*.pyc\ndist/\n"),
            ("README.md", "readme\n"),
            ("src/main.rs", "fn main() {}\n"),
            ("ios/build/notes.txt", "tracked, named like build output\n"),
        ]);
        write(&root, "README.md", "an uncommitted change\n");
        write(&root, "ios/build/notes.txt", "an uncommitted change\n");
        write(&root, "notes.txt", "untracked\n");
        write(
            &root,
            "build/notes.txt",
            "untracked, named like build output\n",
        );
        write(&root, "target/debug/app", "binary");
        write(&root, "node_modules/left-pad/index.js", "module");
        write(&root, "app/__pycache__/main.pyc", "bytecode");
        write(&root, "dist/keep.txt", "tracked though ignored");
        git(&root, &["add", "-f", "dist/keep.txt"]);
        git(&root, &["commit", "-q", "-m", "keep"]);
        write(&root, "dist/bundle.js", "bundle");

        let output = scan(&root, None).unwrap();

        assert_eq!(
            found(&root, &output),
            [
                ("app/__pycache__".into(), Source::DefaultName),
                ("node_modules".into(), Source::DefaultName),
                ("target".into(), Source::DefaultName),
            ]
        );
        assert_eq!(
            ambiguous(&root, &output),
            [
                ("build".into(), Reason::NotIgnored),
                ("dist".into(), Reason::HoldsTracked),
                ("ios/build".into(), Reason::HoldsTracked),
            ]
        );
        for work in [
            "README.md",
            "src/main.rs",
            "notes.txt",
            "build/notes.txt",
            "dist/keep.txt",
        ] {
            assert!(
                !output
                    .found
                    .iter()
                    .any(|found| root.join(work).starts_with(&found.path)),
                "{work} is not build output"
            );
        }
        assert!(output.found.iter().all(|found| found.size > 0));
    }

    #[test]
    fn an_ignored_folder_with_a_cachedir_tag_is_build_output_and_an_unignored_one_is_reported() {
        let (_folder, root) = worktree(&[(".gitignore", "/cache/\n/other/\n")]);
        write(
            &root,
            "cache/CACHEDIR.TAG",
            "Signature: 8a477f597d28d172789f06886806bc55",
        );
        write(&root, "cache/blob", "cached");
        write(&root, "other/blob", "ignored, but neither named nor tagged");
        write(
            &root,
            "tagged/CACHEDIR.TAG",
            "Signature: 8a477f597d28d172789f06886806bc55",
        );

        let output = scan(&root, None).unwrap();

        assert_eq!(
            found(&root, &output),
            [("cache".into(), Source::CacheDirTag)]
        );
        assert_eq!(
            ambiguous(&root, &output),
            [("tagged".into(), Reason::NotIgnored)]
        );
    }

    #[test]
    fn the_repository_file_declares_more_folders_by_path_and_glob() {
        let (_folder, root) = worktree(&[
            (
                DECLARATION,
                "# Generated by codegen\n/generated/\nout/**/cache\n\nreports\n",
            ),
            (".gitignore", "generated/\ncache/\n"),
        ]);
        write(&root, "generated/api.rs", "generated");
        write(&root, "out/a/b/cache/blob", "cached");
        write(&root, "reports/today.html", "untracked");

        let output = scan(&root, None).unwrap();

        assert_eq!(
            found(&root, &output),
            [
                ("generated".into(), Source::Declared),
                ("out/a/b/cache".into(), Source::Declared),
            ]
        );
        assert_eq!(
            ambiguous(&root, &output),
            [("reports".into(), Reason::NotIgnored)]
        );
    }

    #[test]
    fn folders_inside_a_folder_git_ignores_as_a_whole_are_found() {
        let (_folder, root) = worktree(&[
            (DECLARATION, "artifacts/custom\n"),
            (".gitignore", "/artifacts/\n"),
        ]);
        write(&root, "artifacts/target/debug/app", "binary");
        write(
            &root,
            "artifacts/cache/CACHEDIR.TAG",
            "Signature: 8a477f597d28d172789f06886806bc55",
        );
        write(&root, "artifacts/custom/blob", "generated");
        write(
            &root,
            "artifacts/logs/today.log",
            "neither named nor tagged",
        );

        let output = scan(&root, None).unwrap();

        assert_eq!(
            found(&root, &output),
            [
                ("artifacts/cache".into(), Source::CacheDirTag),
                ("artifacts/custom".into(), Source::Declared),
                ("artifacts/target".into(), Source::DefaultName),
            ]
        );
        assert!(output.ambiguous.is_empty());
    }

    #[test]
    fn an_uncommitted_declaration_declares_nothing() {
        let (_folder, root) = worktree(&[(".gitignore", "/private-notes/\n")]);
        write(
            &root,
            "private-notes/notes.txt",
            "ignored, but not build output",
        );
        write(&root, DECLARATION, "private-notes\n");

        let output = scan(&root, None).unwrap();

        assert!(output.found.is_empty());
        assert!(output.ambiguous.is_empty());
    }

    #[test]
    fn derived_data_is_found_for_projects_under_the_worktree_only() {
        let (folder, root) = worktree(&[("README.md", "readme\n")]);
        let derived_data = folder.path().join("DerivedData");
        let plist = |workspace: &Path| {
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n\t<key>LastAccessedDate</key>\n\t<date>2026-11-23T10:00:00Z</date>\n\t<key>WorkspacePath</key>\n\t<string>{}</string>\n</dict>\n</plist>\n",
                workspace.display()
            )
        };
        let sibling = folder.path().join("ticket-2/App.xcodeproj");
        write(
            &derived_data,
            "App-ours/info.plist",
            &plist(&root.join("ios/App.xcworkspace")),
        );
        write(&derived_data, "App-ours/Build/Products/App", "binary");
        write(
            &derived_data,
            "App-ours/SourcePackages/checkouts/package/.git/HEAD",
            "ref: refs/heads/main\n",
        );
        write(&derived_data, "App-sibling/info.plist", &plist(&sibling));
        write(&derived_data, "ModuleCache.noindex/blob", "shared");

        let output = scan(&root, Some(&derived_data)).unwrap();

        assert_eq!(output.found.len(), 1);
        assert_eq!(output.found[0].path, derived_data.join("App-ours"));
        assert_eq!(output.found[0].source, Source::DerivedData);
        assert!(output.found[0].size > 0);
    }

    #[test]
    fn a_hard_linked_file_counts_once() {
        let folder = tempfile::tempdir().unwrap();
        write(folder.path(), "a/file", &"x".repeat(64 * 1024));
        let once = measure(&folder.path().join("a"), &mut HashSet::new()).size;
        fs::hard_link(folder.path().join("a/file"), folder.path().join("a/link")).unwrap();

        assert_eq!(
            measure(&folder.path().join("a"), &mut HashSet::new()).size,
            once
        );
    }

    #[test]
    fn nothing_is_found_or_measured_through_a_symbolic_link() {
        let (folder, root) = worktree(&[
            (".gitignore", "/alias\n/target/\n"),
            ("src/build/notes.txt", "tracked\n"),
        ]);
        std::os::unix::fs::symlink("src", root.join("alias")).unwrap();
        let large = 1024 * 1024;
        write(folder.path(), "elsewhere/large", &"x".repeat(large));
        write(&root, "target/debug/app", "binary");
        std::os::unix::fs::symlink(folder.path().join("elsewhere"), root.join("target/link"))
            .unwrap();

        let output = scan(&root, None).unwrap();

        assert_eq!(
            found(&root, &output),
            [("target".into(), Source::DefaultName)]
        );
        assert!(output.found[0].size < large as u64);
        assert_eq!(
            ambiguous(&root, &output),
            [("src/build".into(), Reason::HoldsTracked)]
        );
    }

    #[test]
    fn a_default_name_inside_build_output_is_not_listed_again() {
        let (_folder, root) = worktree(&[(".gitignore", "node_modules/\n")]);
        write(&root, "node_modules/x/node_modules/y/index.js", "module");

        let output = scan(&root, None).unwrap();

        assert_eq!(
            found(&root, &output),
            [("node_modules".into(), Source::DefaultName)]
        );
        assert!(output.ambiguous.is_empty());
    }

    #[test]
    fn build_output_holding_a_git_repository_is_reported_except_swiftpm_clones() {
        let (_folder, root) = worktree(&[(".gitignore", "/target/\n/.build/\n")]);
        write(&root, "target/debug/app", "binary");
        write(
            &root,
            "target/scratch/clone/.git/HEAD",
            "ref: refs/heads/main\n",
        );
        write(
            &root,
            ".build/checkouts/package/.git/HEAD",
            "ref: refs/heads/main\n",
        );
        write(
            &root,
            ".build/repositories/package-1/HEAD",
            "ref: refs/heads/main\n",
        );
        write(
            &root,
            ".build/repositories/package-1/.git",
            "gitdir: elsewhere\n",
        );

        let output = scan(&root, None).unwrap();

        assert_eq!(
            found(&root, &output),
            [(".build".into(), Source::DefaultName)]
        );
        assert_eq!(
            ambiguous(&root, &output),
            [("target".into(), Reason::HoldsRepository)]
        );
    }
}
