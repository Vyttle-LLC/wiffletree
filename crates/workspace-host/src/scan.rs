//! The scans before a push (OpenSpec change `spaces`, task 4.4): the brain gate's
//! `sensitive-info` and `secret-scan` checks, at the version this release bundles, with their
//! default rules and inline `gate:allow` markers, run on this Mac over the files a storage
//! repository's pass would push under `spaces/`, `issues/` and `wiffletree-issues.yaml`. A file
//! that fails, or that a check cannot scan, is held: never pushed, and kept here. Local storage
//! is never pushed, so never scanned. Nothing here keeps or shows what a check matched.
use crate::*;
use repos::Content;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::process::{Command, Stdio};

/// The gate files the two checks need, as this release bundles them.
const GATE: [(&str, &str); 5] = [
    (
        "checks/common.py",
        include_str!("../../../gate/checks/common.py"),
    ),
    (
        "checks/gate_config.py",
        include_str!("../../../gate/checks/gate_config.py"),
    ),
    (
        "checks/sensitive_scan.py",
        include_str!("../../../gate/checks/sensitive_scan.py"),
    ),
    (
        "checks/secret_scan.py",
        include_str!("../../../gate/checks/secret_scan.py"),
    ),
    (
        "config/gitleaks.toml",
        include_str!("../../../gate/config/gitleaks.toml"),
    ),
];
/// Each check by its gate name, and its script.
const CHECKS: [(&str, &str); 2] = [
    ("sensitive-info", "checks/sensitive_scan.py"),
    ("secret-scan", "checks/secret_scan.py"),
];

/// Whether a file at `path` is scanned before it is pushed.
pub(crate) fn scanned(path: &str) -> bool {
    path == storage::TRACKER || path.starts_with("spaces/") || path.starts_with("issues/")
}

/// A file held back from a push: the check, and the rule it failed or why it could not run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Held {
    pub(crate) path: String,
    pub(crate) check: String,
    pub(crate) rule: Option<String>,
    pub(crate) reason: Option<String>,
}

/// Runs the bundled checks with Python from `PATH`.
pub(crate) struct Scanner {
    gate: PathBuf,
    /// `PATH` for the checks and the tools they run; the host's own when `None`.
    path: Option<OsString>,
}
impl Scanner {
    /// The bundled gate, written once per version to `<data folder>/gate/<hash>/`, run with
    /// `path` as `PATH`, or the host's own.
    pub(crate) fn bundled(home: &Path, path: Option<OsString>) -> Result<Self> {
        let mut hash = Sha256::new();
        for (path, text) in GATE {
            hash.update(path);
            hash.update(text);
        }
        let version: String = (hash.finalize().iter().take(6))
            .map(|b| format!("{b:02x}"))
            .collect();
        let gate = home.join("gate").join(version);
        if !gate.join(GATE[0].0).exists() {
            let staging = tempfile::Builder::new().prefix("gate-").tempdir_in(home)?;
            for (path, text) in GATE {
                shared::write_atomically(&staging.path().join(path), text.as_bytes())?;
            }
            fs::create_dir_all(gate.parent().context("The gate is in a folder")?)?;
            // Another host's write of the same version may have won; either is the same.
            let _ = fs::rename(staging.keep(), &gate);
        }
        Ok(Self { gate, path })
    }
    /// The files of `files` that may not be pushed: each scanned file with content is checked
    /// as a change from its version at the clone's `remote` commit, so only lines it adds are
    /// scanned, as the gate scans a pull request.
    pub(crate) fn scan(
        &self,
        clone: &Path,
        remote: &str,
        files: &[(String, Content)],
    ) -> Vec<Held> {
        let candidates: Vec<(&String, &Vec<u8>)> = (files.iter())
            .filter(|(path, _)| scanned(path))
            .filter_map(|(path, content)| Some((path, content.as_ref()?)))
            .collect();
        if candidates.is_empty() {
            return vec![];
        }
        let hold_all = |check: &str, reason: String| {
            (candidates.iter())
                .map(|(path, _)| Held {
                    path: (*path).clone(),
                    check: check.into(),
                    rule: None,
                    reason: Some(reason.clone()),
                })
                .collect()
        };
        let scratch = match self.scratch(clone, remote, &candidates) {
            Ok(scratch) => scratch,
            Err(error) => return hold_all(CHECKS[0].0, format!("{error:#}")),
        };
        let mut held: Vec<Held> = vec![];
        let paths: Vec<&str> = candidates.iter().map(|(path, _)| path.as_str()).collect();
        for (check, script) in CHECKS {
            let found = match self.run(&scratch, script, &paths) {
                Ok(found) => found,
                Err(reason) => {
                    let unheld: Vec<Held> = (hold_all(check, reason).into_iter())
                        .filter(|h| !held.iter().any(|o| o.path == h.path))
                        .collect();
                    held.extend(unheld);
                    continue;
                }
            };
            for (path, rule) in found {
                if !held.iter().any(|h| h.path == path) {
                    held.push(Held {
                        path,
                        check: check.into(),
                        rule: Some(rule),
                        reason: None,
                    });
                }
            }
        }
        held
    }
    /// A scratch repository of two commits: the files as GitHub's head holds them, then as they
    /// would be pushed. Its first commit is `GATE_BASE`.
    fn scratch(
        &self,
        clone: &Path,
        remote: &str,
        candidates: &[(&String, &Vec<u8>)],
    ) -> Result<(tempfile::TempDir, String)> {
        let scratch = tempfile::Builder::new()
            .prefix("wiffletree-scan-")
            .tempdir()?;
        let at = scratch.path();
        let git = |args: &[&str]| -> Result<String> {
            let identity = [
                "-c",
                "user.name=Wiffletree",
                "-c",
                "user.email=scan@localhost",
            ];
            let signing = ["-c", "commit.gpgsign=false", "-c", "maintenance.auto=false"];
            runtime::git_output(at, &[&identity[..], &signing[..], args].concat())
        };
        git(&["init", "--quiet", "--initial-branch=main"])?;
        let wanted = |path: &str| candidates.iter().any(|(c, _)| *c == path);
        for (path, content) in repos::files_at(clone, remote, &wanted)? {
            shared::write_atomically(&at.join(path), &content)?;
        }
        git(&["add", "--all"])?;
        git(&[
            "commit",
            "--quiet",
            "--allow-empty",
            "--no-verify",
            "-m",
            "GitHub",
        ])?;
        let base = git(&["rev-parse", "HEAD"])?.trim().to_owned();
        for (path, content) in candidates {
            shared::write_atomically(&at.join(path), content)?;
        }
        git(&["add", "--all"])?;
        git(&[
            "commit",
            "--quiet",
            "--allow-empty",
            "--no-verify",
            "-m",
            "This Mac",
        ])?;
        Ok((scratch, base))
    }
    /// Runs one check in the scratch repository over the files at `paths`: each file and rule it
    /// reports, or why it could not run, from its own error line, which never quotes what it
    /// scanned. A failure that names none of `paths`, or a finding it cannot place, such as a
    /// traceback's exit 1, counts as not running, so every file is held.
    fn run(
        &self,
        (scratch, base): &(tempfile::TempDir, String),
        script: &str,
        paths: &[&str],
    ) -> std::result::Result<Vec<(String, String)>, String> {
        let mut command = Command::new("python3");
        command
            .arg(self.gate.join(script))
            .current_dir(scratch.path())
            .env("GATE_BASE", base)
            .env_remove("GITHUB_ACTIONS")
            .stdin(Stdio::null());
        if let Some(path) = &self.path {
            command.env("PATH", path);
        }
        let output = command
            .output()
            .map_err(|error| format!("python3 could not start: {error}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        match output.status.code() {
            Some(0) => Ok(vec![]),
            Some(1) => {
                let reported: Vec<&str> = (stdout.lines())
                    .filter(|line| line.starts_with("ERROR: "))
                    .collect();
                let found: Vec<(String, String)> = (reported.iter())
                    .filter_map(|line| finding(line, paths))
                    .collect();
                match !found.is_empty() && found.len() == reported.len() {
                    true => Ok(found),
                    false => Err(
                        "it stopped with an error; it needs python3 3.11 or newer and UTF-8 files"
                            .into(),
                    ),
                }
            }
            _ => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let reason = (stderr.lines())
                    .find_map(|line| line.strip_prefix("ERROR: "))
                    .unwrap_or("the check stopped");
                Err(reason.to_owned())
            }
        }
    }
}

/// The file of `paths` and the rule a finding line names, `ERROR: <path>:<line>: … [rule: <rule>]`.
/// The path is matched against the files given, as a name may hold `: `, and Git follows a name
/// with a space by a tab.
fn finding(line: &str, paths: &[&str]) -> Option<(String, String)> {
    let rest = line.strip_prefix("ERROR: ")?;
    let path = (paths.iter())
        .filter(|path| {
            (rest.strip_prefix(**path))
                .is_some_and(|after| after.trim_start_matches(['\t', ' ']).starts_with(':'))
        })
        .max_by_key(|path| path.len())?;
    let rule = rest.rsplit_once("[rule: ")?.1.strip_suffix(']')?;
    Some(((*path).to_owned(), rule.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{git, scan_tools};

    /// Scans `files` as changes to an empty repository, with or without gitleaks.
    fn scanned_with(gitleaks: bool, files: &[(&str, &str)]) -> Vec<Held> {
        let (home, clone) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        git(clone.path(), &["init", "-q", "-b", "main"]);
        git(
            clone.path(),
            &["commit", "-q", "--allow-empty", "-m", "GitHub"],
        );
        let bin = scan_tools(gitleaks);
        let scanner = Scanner::bundled(home.path(), Some(bin.path().into())).unwrap();
        let files: Vec<(String, Content)> = (files.iter())
            .map(|(path, text)| (path.to_string(), Some(text.as_bytes().to_vec())))
            .collect();
        scanner.scan(clone.path(), "HEAD", &files)
    }

    // Invented values only: a 555 number and a token of the right shape.
    const PHONE: &str = "Call the parent at (415) 555-0134 tonight.\n";
    const TOKEN: &str = "token: ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789\n";

    #[test]
    fn a_phone_number_in_an_issue_is_held_naming_the_rule_and_never_the_number() {
        let held = scanned_with(
            true,
            &[
                ("issues/SAGE-1/issue.md", PHONE),
                ("spaces/sage/space.yaml", "name: Sage\n"),
            ],
        );
        assert_eq!(
            held,
            [Held {
                path: "issues/SAGE-1/issue.md".into(),
                check: "sensitive-info".into(),
                rule: Some("phone".into()),
                reason: None,
            }]
        );
    }

    #[test]
    fn a_finding_in_a_file_whose_name_has_a_space_or_a_colon_is_held() {
        for path in ["spaces/sage/call notes.md", "issues/A-1/notes: call.md"] {
            let held = scanned_with(true, &[(path, PHONE)]);
            assert_eq!(held.len(), 1, "{path}");
            assert_eq!(
                (&*held[0].path, held[0].rule.as_deref()),
                (path, Some("phone"))
            );
        }
    }

    #[test]
    fn a_check_that_crashes_holds_every_file() {
        let (home, clone) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        git(clone.path(), &["init", "-q", "-b", "main"]);
        git(
            clone.path(),
            &["commit", "-q", "--allow-empty", "-m", "GitHub"],
        );
        let bin = scan_tools(true);
        let scanner = Scanner::bundled(home.path(), Some(bin.path().into())).unwrap();
        // Latin-1, which the check cannot decode, and a traceback exits 1 with no finding.
        let files = [
            (
                "spaces/sage/notes.md".to_owned(),
                Some(b"caf\xe9\n".to_vec()),
            ),
            (
                "spaces/sage/space.yaml".to_owned(),
                Some(b"name: Sage\n".to_vec()),
            ),
        ];
        let held = scanner.scan(clone.path(), "HEAD", &files);
        assert_eq!(held.len(), 2, "{held:?}");
        assert!(held.iter().all(|h| h.rule.is_none() && h.reason.is_some()));
    }

    #[test]
    fn a_token_is_held() {
        let held = scanned_with(true, &[("spaces/sage/space.yaml", TOKEN)]);
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].check, "sensitive-info");
        assert_eq!(held[0].rule.as_deref(), Some("token"));
    }

    #[test]
    fn a_gate_allow_marker_lets_the_line_through() {
        let allowed = "Call the front desk at (415) 555-0134. <!-- gate:allow phone the shop's public number -->\n";
        assert_eq!(
            scanned_with(true, &[("issues/SAGE-1/issue.md", allowed)]),
            []
        );
    }

    #[test]
    fn without_gitleaks_every_file_is_held_saying_it_is_missing() {
        let held = scanned_with(false, &[("spaces/sage/space.yaml", "name: Sage\n")]);
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].check, "secret-scan");
        assert_eq!(held[0].rule, None);
        let reason = held[0].reason.as_deref().unwrap();
        assert!(reason.contains("gitleaks is not installed"), "{reason}");
    }

    #[test]
    fn only_storage_paths_with_content_are_scanned() {
        assert!(scanned("spaces/sage/space.yaml") && scanned("issues/A-1/issue.md"));
        assert!(scanned(storage::TRACKER) && !scanned("settings/review.json"));
        let home = tempfile::tempdir().unwrap();
        let scanner = Scanner::bundled(home.path(), Some("/nowhere".into())).unwrap();
        let removed = [("spaces/sage/space.yaml".to_owned(), None)];
        assert_eq!(scanner.scan(home.path(), "HEAD", &removed), []);
    }
}
