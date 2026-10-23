//! An edit step's changes as bounded diffs: from Claude's structured patch, from a written
//! file's content, or from Git when the provider reports which files changed but not how.
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read},
    os::unix::process::CommandExt,
    path::Path,
    process::{ChildStdout, Command, Stdio},
    sync::mpsc,
    time::Instant,
};
use workspace_core::{DiffHunk, FileDiff, MAX_DIFF_LINES};

/// Longest diff line kept, in characters.
const MAX_LINE: usize = 400;
/// Git time for all of one edit's files together; files not reached by then get no diff.
const GIT_BUDGET: std::time::Duration = crate::runtime::GIT_TIMEOUT;

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
        // "\ No newline at end of file" is a note about the line before, not a line.
        if line.starts_with('\\') {
            return;
        }
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
fn file_diff(path: String, created: bool, hunks: Vec<DiffHunk>) -> FileDiff {
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
    from_git_by(cwd, files, Instant::now() + GIT_BUDGET)
}

fn from_git_by(cwd: &Path, files: &[(&str, String)], deadline: Instant) -> Vec<FileDiff> {
    files
        .iter()
        .take_while(|_| Instant::now() < deadline)
        .filter_map(|(path, shown)| git_diff(cwd, path, shown, deadline))
        .collect()
}

fn git_diff(cwd: &Path, path: &str, shown: &str, deadline: Instant) -> Option<FileDiff> {
    let diff = |args: &[&str]| {
        let base = ["diff", "--no-color", "--no-ext-diff", "--no-textconv"];
        let shown = shown.to_owned();
        git_read(cwd, &[&base[..], args].concat(), deadline, move |output| {
            read_unified(shown, output)
        })
    };
    if let Some((Some(diff), _)) = diff(&["HEAD", "--", path]) {
        return Some(diff);
    }
    let listed = git_read(cwd, &["ls-files", "--", path], deadline, |mut output| {
        let mut listed = String::new();
        output.read_to_string(&mut listed).map(|_| listed)
    });
    let Some((Ok(listed), true)) = listed else {
        return None;
    };
    if !listed.is_empty() || !cwd.join(path).is_file() {
        return None;
    }
    diff(&["--no-index", "--", "/dev/null", path])?.0
}

/// Runs Git in a process group of its own and reads its output on another thread as it
/// streams, so a diff of any size keeps its first lines and counts the rest. Answers what
/// `read` made of the output and whether Git succeeded; past `deadline` the group is killed
/// and there is no answer.
fn git_read<T: Send + 'static>(
    cwd: &Path,
    args: &[&str],
    deadline: Instant,
    read: impl FnOnce(BufReader<ChildStdout>) -> T + Send + 'static,
) -> Option<(T, bool)> {
    let mut git = Command::new("git")
        .arg("--no-pager")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    let output = BufReader::new(git.stdout.take()?);
    let (send, answer) = mpsc::channel();
    std::thread::spawn(move || drop(send.send(read(output))));
    let answer = answer
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok();
    if answer.is_none() {
        // SAFETY: killpg only sends a signal, to the group Git leads.
        unsafe { libc::killpg(git.id() as i32, libc::SIGKILL) };
    }
    let succeeded = git.wait().is_ok_and(|status| status.success());
    answer.map(|answer| (answer, succeeded))
}

/// One file's unified diff, as `git diff` prints it.
fn read_unified(shown: String, mut output: impl BufRead) -> Option<FileDiff> {
    let mut created = false;
    let mut diff: Option<Bounded> = None;
    let mut bytes = vec![];
    while output.read_until(b'\n', &mut bytes).ok()? > 0 {
        let text = String::from_utf8_lossy(&bytes);
        let line = text.trim_end_matches(['\n', '\r']);
        if let Some(header) = line.strip_prefix("@@ ") {
            let (old_start, new_start) = hunk_starts(header)?;
            diff.get_or_insert_with(|| Bounded::new(shown.clone(), created))
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
    fn a_missing_newline_note_is_not_a_line() {
        let patch = json!([{"oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": 2, "lines": [
            "-a", "\\ No newline at end of file", "+a", "+b", "\\ No newline at end of file"
        ]}]);
        let diff = from_structured_patch("a".into(), &patch).unwrap();
        assert_eq!(lines(&diff), ["-a", "+a", "+b"]);
        assert_eq!((diff.added, diff.removed, diff.omitted_lines), (2, 1, 0));
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

    #[test]
    fn a_hanging_git_is_stopped_and_the_files_left_get_no_diff() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        git(repo, &["init", "-q"]);
        // A clean filter runs whenever Git reads the working file, so the diff waits on it.
        git(repo, &["config", "filter.slow.clean", "sleep 30; cat"]);
        std::fs::write(repo.join(".gitattributes"), "*.txt filter=slow\n").unwrap();
        for name in ["a.txt", "b.txt"] {
            std::fs::write(repo.join(name), "one\n").unwrap();
        }
        git(repo, &["add", ".gitattributes"]);
        git(repo, &["commit", "-qm", "base"]);
        git(
            repo,
            &["-c", "filter.slow.clean=cat", "add", "a.txt", "b.txt"],
        );
        git(
            repo,
            &["-c", "filter.slow.clean=cat", "commit", "-qm", "files"],
        );
        for name in ["a.txt", "b.txt"] {
            std::fs::write(repo.join(name), "two\n").unwrap();
        }
        let files = [("a.txt", "a.txt".into()), ("b.txt", "b.txt".into())];
        let started = Instant::now();
        let deadline = started + std::time::Duration::from_millis(500);
        assert!(from_git_by(repo, &files, deadline).is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }
}
