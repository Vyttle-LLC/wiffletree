//! An edit step's changes as bounded diffs: from Claude's structured patch, from a written
//! file's content, or from Git when the provider reports which files changed but not how.
use crate::runtime::git_answer;
use serde_json::Value;
use std::path::Path;
use workspace_core::{DiffHunk, FileDiff, MAX_DIFF_LINES};

/// Longest diff line kept, in characters.
const MAX_LINE: usize = 400;
/// Files read from Git for one edit step.
const MAX_GIT_FILES: usize = 20;

/// Counts every line of `hunks` and keeps the first `MAX_DIFF_LINES` of them.
pub(crate) fn file_diff(path: String, created: bool, hunks: Vec<DiffHunk>) -> FileDiff {
    let mut diff = FileDiff {
        path,
        created,
        added: 0,
        removed: 0,
        hunks: vec![],
        omitted_lines: 0,
    };
    let mut room = MAX_DIFF_LINES;
    for mut hunk in hunks {
        for line in &hunk.lines {
            match line.as_bytes().first() {
                Some(b'+') => diff.added += 1,
                Some(b'-') => diff.removed += 1,
                _ => {}
            }
        }
        let kept = hunk.lines.len().min(room);
        diff.omitted_lines += (hunk.lines.len() - kept) as u32;
        if kept == 0 {
            continue;
        }
        room -= kept;
        hunk.lines.truncate(kept);
        for line in &mut hunk.lines {
            if line.chars().count() > MAX_LINE {
                *line = crate::steps::clip(line, MAX_LINE);
            }
        }
        diff.hunks.push(hunk);
    }
    diff
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
    let lines: Vec<String> = content.lines().map(|line| format!("+{line}")).collect();
    let hunks = if lines.is_empty() {
        vec![]
    } else {
        vec![DiffHunk {
            old_start: 0,
            new_start: 1,
            lines,
        }]
    };
    file_diff(path, true, hunks)
}

/// Each changed file's diff as Git sees it in `cwd`: against `HEAD`, or as a new file when
/// untracked. This is every uncommitted change to the file, not only the step's own.
/// `files` pairs the path Git is given with the path shown.
pub(crate) fn from_git(cwd: &Path, files: &[(&str, String)]) -> Vec<FileDiff> {
    files
        .iter()
        .take(MAX_GIT_FILES)
        .filter_map(|(path, shown)| parse_unified(shown.clone(), &git_diff(cwd, path)?))
        .collect()
}

fn git_diff(cwd: &Path, path: &str) -> Option<String> {
    let diff = |args: &[&str]| {
        let base = ["diff", "--no-color", "--no-ext-diff"];
        git_answer(cwd, &[&base[..], args].concat()).ok()
    };
    let tracked = diff(&["HEAD", "--", path])?;
    if tracked.status.success() && !tracked.output.is_empty() {
        return Some(tracked.output);
    }
    let listed = git_answer(cwd, &["ls-files", "--", path]).ok()?;
    if !listed.status.success() || !listed.output.is_empty() || !cwd.join(path).is_file() {
        return None;
    }
    // `--no-index` exits 1 when the files differ, which they always do here.
    Some(diff(&["--no-index", "--", "/dev/null", path])?.output)
}

/// One file's unified diff, as `git diff` prints it.
fn parse_unified(path: String, text: &str) -> Option<FileDiff> {
    let mut created = false;
    let mut hunks: Vec<DiffHunk> = vec![];
    for line in text.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            hunks.push(hunk_header(header)?);
        } else if let Some(hunk) = hunks.last_mut() {
            if matches!(line.as_bytes().first(), Some(b' ' | b'+' | b'-')) {
                hunk.lines.push(line.to_owned());
            }
        } else if line.starts_with("new file mode") {
            created = true;
        }
    }
    (!hunks.is_empty()).then(|| file_diff(path, created, hunks))
}

/// The start lines from "-12,3 +12,4 @@ fn name".
fn hunk_header(header: &str) -> Option<DiffHunk> {
    let mut ranges = header.split(' ');
    let mut start = |sign: char| -> Option<u32> {
        let range = ranges.next()?.strip_prefix(sign)?;
        range.split(',').next()?.parse().ok()
    };
    Some(DiffHunk {
        old_start: start('-')?,
        new_start: start('+')?,
        lines: vec![],
    })
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
}
