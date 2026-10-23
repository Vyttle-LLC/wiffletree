//! An edit step's changes as bounded diffs: from Claude's structured patch, from a written
//! file's content, or from Git when the provider reports which files changed but not how.
use crate::runtime::git_answer;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
};
use workspace_core::{DiffHunk, FileDiff, MAX_DIFF_LINES};

/// Longest diff line kept, in characters.
const MAX_LINE: usize = 400;

/// A diff built a line at a time: every line is counted, the first `MAX_DIFF_LINES` are kept.
struct Bounded {
    diff: FileDiff,
    room: usize,
}
impl Bounded {
    fn new(path: String, created: bool) -> Self {
        Self {
            diff: FileDiff {
                path,
                created,
                added: 0,
                removed: 0,
                hunks: vec![],
                omitted_lines: 0,
            },
            room: MAX_DIFF_LINES,
        }
    }
    /// A hunk with no room left for its lines is not kept.
    fn hunk(&mut self, old_start: u32, new_start: u32) {
        if self.room > 0 {
            self.diff.hunks.push(DiffHunk {
                old_start,
                new_start,
                lines: vec![],
            });
        }
    }
    fn line(&mut self, line: &str) {
        match line.as_bytes().first() {
            Some(b'+') => self.diff.added += 1,
            Some(b'-') => self.diff.removed += 1,
            _ => {}
        }
        match self.diff.hunks.last_mut().filter(|_| self.room > 0) {
            Some(hunk) => {
                hunk.lines.push(crate::steps::clip(line, MAX_LINE));
                self.room -= 1;
            }
            None => self.diff.omitted_lines += 1,
        }
    }
}

/// Counts every line of `hunks` and keeps the first `MAX_DIFF_LINES` of them.
pub(crate) fn file_diff(path: String, created: bool, hunks: Vec<DiffHunk>) -> FileDiff {
    let mut diff = Bounded::new(path, created);
    for hunk in hunks {
        diff.hunk(hunk.old_start, hunk.new_start);
        hunk.lines.iter().for_each(|line| diff.line(line));
    }
    diff.diff
}

/// Claude's `structuredPatch`: hunks with their start lines and prefixed lines.
pub(crate) fn from_structured_patch(path: String, patch: &Value) -> Option<FileDiff> {
    let hunks = patch
        .as_array()?
        .iter()
        .map(|hunk| {
            Some(DiffHunk {
                old_start: hunk["oldStart"].as_u64()? as u32,
                new_start: hunk["newStart"].as_u64()? as u32,
                lines: hunk["lines"]
                    .as_array()?
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    (!hunks.is_empty()).then(|| file_diff(path, false, hunks))
}

/// A newly written file: every line is an addition.
pub(crate) fn new_file(path: String, content: &str) -> FileDiff {
    let mut diff = Bounded::new(path, true);
    if !content.is_empty() {
        diff.hunk(0, 1);
        content
            .lines()
            .for_each(|line| diff.line(&format!("+{line}")));
    }
    diff.diff
}

/// Each changed file's diff as Git sees it in `cwd`: against `HEAD`, or as a new file when
/// untracked. This is every uncommitted change to the file, not only the step's own.
/// `files` pairs the path Git is given with the path shown.
pub(crate) fn from_git(cwd: &Path, files: &[(&str, String)]) -> Vec<FileDiff> {
    files
        .iter()
        .filter_map(|(path, shown)| git_diff(cwd, path, shown))
        .collect()
}

fn git_diff(cwd: &Path, path: &str, shown: &str) -> Option<FileDiff> {
    if let Some(diff) = stream_diff(cwd, &["HEAD", "--", path], shown) {
        return Some(diff);
    }
    let listed = git_answer(cwd, &["ls-files", "--", path]).ok()?;
    if !listed.status.success() || !listed.output.is_empty() || !cwd.join(path).is_file() {
        return None;
    }
    stream_diff(cwd, &["--no-index", "--", "/dev/null", path], shown)
}

/// Runs `git diff` and reads its output as it streams, so a diff of any size keeps its first
/// lines and counts the rest.
fn stream_diff(cwd: &Path, args: &[&str], shown: &str) -> Option<FileDiff> {
    let mut git = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["diff", "--no-color", "--no-ext-diff"])
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // The reader is dropped before the wait, so Git stops if it is left unread.
    let diff = read_unified(shown, BufReader::new(git.stdout.take()?));
    let _ = git.wait();
    diff
}

/// One file's unified diff, as `git diff` prints it.
fn read_unified(shown: &str, mut output: impl BufRead) -> Option<FileDiff> {
    let mut created = false;
    let mut diff: Option<Bounded> = None;
    let mut bytes = vec![];
    while output.read_until(b'\n', &mut bytes).ok()? > 0 {
        let text = String::from_utf8_lossy(&bytes);
        let line = text.trim_end_matches(['\n', '\r']);
        if let Some(header) = line.strip_prefix("@@ ") {
            let (old_start, new_start) = hunk_starts(header)?;
            diff.get_or_insert_with(|| Bounded::new(shown.to_owned(), created))
                .hunk(old_start, new_start);
        } else if let Some(diff) = &mut diff {
            if matches!(line.as_bytes().first(), Some(b' ' | b'+' | b'-')) {
                diff.line(line);
            }
        } else if line.starts_with("new file mode") {
            created = true;
        }
        bytes.clear();
    }
    diff.map(|diff| diff.diff)
}

/// The start lines from "-12,3 +12,4 @@ fn name".
fn hunk_starts(header: &str) -> Option<(u32, u32)> {
    let mut ranges = header.split(' ');
    let mut start = |sign: char| -> Option<u32> {
        let range = ranges.next()?.strip_prefix(sign)?;
        range.split(',').next()?.parse().ok()
    };
    Some((start('-')?, start('+')?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::git;
    use serde_json::json;

    fn lines(diff: &FileDiff) -> Vec<&str> {
        diff.hunks
            .iter()
            .flat_map(|h| h.lines.iter().map(String::as_str))
            .collect()
    }

    #[test]
    fn a_structured_patch_keeps_every_hunk_with_its_start_lines() {
        let patch = json!([
            {"oldStart": 3, "oldLines": 2, "newStart": 3, "newLines": 2, "lines": [" a", "-b", "+c"]},
            {"oldStart": 40, "oldLines": 1, "newStart": 40, "newLines": 3, "lines": [" x", "+y", "+z"]}
        ]);
        let diff = from_structured_patch("src/a.rs".into(), &patch).unwrap();
        assert_eq!((diff.added, diff.removed, diff.omitted_lines), (3, 1, 0));
        assert!(!diff.created);
        assert_eq!(
            diff.hunks
                .iter()
                .map(|h| (h.old_start, h.new_start))
                .collect::<Vec<_>>(),
            [(3, 3), (40, 40)]
        );
        assert_eq!(lines(&diff), [" a", "-b", "+c", " x", "+y", "+z"]);
        assert!(from_structured_patch("a".into(), &json!([])).is_none());
    }

    #[test]
    fn a_long_diff_keeps_its_first_lines_and_counts_the_rest() {
        let hunk = |start: u32, n: usize| DiffHunk {
            old_start: start,
            new_start: start,
            lines: (0..n).map(|i| format!("+line {i}")).collect(),
        };
        let diff = file_diff(
            "a".into(),
            false,
            vec![hunk(1, 30), hunk(100, 25), hunk(200, 5)],
        );
        assert_eq!(diff.added, 60);
        assert_eq!(lines(&diff).len(), MAX_DIFF_LINES);
        assert_eq!(diff.omitted_lines, 20);
        // A hunk with no room left is dropped rather than kept with no lines.
        assert_eq!(diff.hunks.len(), 2);
        let long = new_file("b".into(), &"x".repeat(MAX_LINE * 2));
        assert_eq!(lines(&long)[0].chars().count(), MAX_LINE);
    }

    #[test]
    fn a_written_file_is_all_additions() {
        let diff = new_file("src/new.rs".into(), "fn a() {}\n\nfn b() {}\n");
        assert!(diff.created);
        assert_eq!((diff.added, diff.removed), (3, 0));
        assert_eq!(lines(&diff), ["+fn a() {}", "+", "+fn b() {}"]);
        assert_eq!(diff.hunks[0].new_start, 1);
    }

    #[test]
    fn git_reads_a_changed_file_against_head_and_an_untracked_one_as_new() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        git(repo, &["init", "-q"]);
        let numbered: String = (1..=50).map(|n| format!("line {n}\n")).collect();
        std::fs::write(repo.join("kept.txt"), &numbered).unwrap();
        git(repo, &["add", "."]);
        git(repo, &["commit", "-qm", "base"]);
        std::fs::write(
            repo.join("kept.txt"),
            numbered.replace("line 30\n", "line thirty\n"),
        )
        .unwrap();
        std::fs::write(repo.join("new.txt"), "one\ntwo\n").unwrap();
        let absolute = repo.join("kept.txt").to_string_lossy().into_owned();
        let diffs = from_git(
            repo,
            &[
                (absolute.as_str(), "kept.txt".into()),
                ("new.txt", "new.txt".into()),
                ("unchanged-missing.txt", "unchanged-missing.txt".into()),
            ],
        );
        assert_eq!(diffs.len(), 2);
        let (kept, new) = (&diffs[0], &diffs[1]);
        assert_eq!(kept.path, "kept.txt");
        assert_eq!((kept.added, kept.removed, kept.created), (1, 1, false));
        assert_eq!(kept.hunks[0].old_start, 27);
        assert!(lines(kept).contains(&"-line 30"));
        assert!(lines(kept).contains(&"+line thirty"));
        assert_eq!((new.added, new.removed, new.created), (2, 0, true));
        assert_eq!(lines(new), ["+one", "+two"]);
    }

    #[test]
    fn git_reads_every_changed_file_and_keeps_the_head_of_a_diff_of_any_size() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        git(repo, &["init", "-q"]);
        let mut names: Vec<String> = (0..25).map(|n| format!("file-{n}.txt")).collect();
        for name in &names {
            std::fs::write(repo.join(name), "one\n").unwrap();
        }
        names.push("big.txt".into());
        // Over the 4 MiB that a buffered Git answer allows.
        let wide = "x".repeat(500);
        let big: String = (0..10_000).map(|n| format!("{n} {wide}\n")).collect();
        std::fs::write(repo.join("big.txt"), big).unwrap();
        let files: Vec<_> = names
            .iter()
            .map(|name| (name.as_str(), name.clone()))
            .collect();
        let diffs = from_git(repo, &files);
        assert_eq!(diffs.len(), 26);
        assert!(diffs[..25].iter().all(|d| d.added == 1));
        let big = &diffs[25];
        assert_eq!((big.added, big.omitted_lines), (10_000, 10_000 - 40));
        assert_eq!(lines(big).len(), MAX_DIFF_LINES);
        assert!(lines(big)[0].starts_with("+0 x"));
    }
}
