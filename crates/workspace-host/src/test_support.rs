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

/// A `PATH` for the scans before a push with this Mac's Python and Git, and, with `gitleaks`, a
/// stand-in gitleaks that finds nothing, so tests run alike where gitleaks is not installed.
pub(crate) fn scan_tools(gitleaks: bool) -> tempfile::TempDir {
    let bin = tempfile::tempdir().unwrap();
    let path = std::env::var_os("PATH").unwrap_or_default();
    for tool in ["python3", "git"] {
        let found = std::env::split_paths(&path)
            .map(|folder| folder.join(tool))
            .find(|candidate| candidate.is_file())
            .unwrap_or_else(|| panic!("{tool} is on PATH"));
        std::os::unix::fs::symlink(found, bin.path().join(tool)).unwrap();
    }
    if gitleaks {
        let script = bin.path().join("gitleaks");
        let finds_nothing = "#!/bin/sh\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = --report-path ]; then echo '[]' > \"$2\"; fi\n  shift\ndone\n";
        std::fs::write(&script, finds_nothing).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
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
pub(crate) fn without_background_maintenance(repository: &Path) {
    git(repository, &["config", "maintenance.auto", "false"]);
    git(repository, &["config", "gc.auto", "0"]);
}
