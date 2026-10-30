//! Small helpers shared by host unit tests.
use std::path::{Path, PathBuf};

pub(crate) fn git(path: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(path)
        .args(["-c", "user.name=F", "-c", "user.email=f@example.invalid"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    if let Some(repository) = created(path, args) {
        without_background_maintenance(&repository);
    }
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// The repository `git init` or `git clone` made in `path`: the folder its last argument names,
/// or `path` itself when that argument is an option or its value.
fn created(path: &Path, args: &[&str]) -> Option<PathBuf> {
    if !matches!(args.first(), Some(&("init" | "clone"))) {
        return None;
    }
    let named = path.join(args.last().unwrap());
    Some(if named.is_dir() { named } else { path.into() })
}

/// Keeps Git from repacking a test repository in the background, as it may after any commit or
/// push. The repack deletes loose objects that a local clone of the repository may be copying.
fn without_background_maintenance(repository: &Path) {
    git(repository, &["config", "maintenance.auto", "false"]);
    git(repository, &["config", "gc.auto", "0"]);
}
