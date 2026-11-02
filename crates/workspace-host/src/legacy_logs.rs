//! Reading a legacy log (OpenSpec change `one-space-repository`, decision 7): each `## YYYY-MM-DD
//! — Title` entry of a brain's legacy `logs/<repo>.md` file parses into one entry in the contract,
//! with `source: skill`, for Bring in a brain, which arrives with the setup screens. An entry's id
//! comes from its repository, date, title and text, so bringing it in again adds nothing. Nothing
//! here logs an entry's text.
use super::*;
use memory::{
    MAX_BODY_CHARS, MAX_TITLE_CHARS, NO_REF, body_chars, entry_body, one_line, valid_login,
    valid_ref,
};
use publish::Tools;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

/// The longest `ref` an imported entry keeps as its `ref`.
const MAX_REF_CHARS: usize = 255;
/// The entry contract's fields.
const FIELDS: [&str; 5] = ["type", "ref", "changed", "why", "state"];
/// The fields a legacy entry may carry that the entry contract has no place for: the old
/// Markdown export's own bookkeeping, which the host store already holds.
const EXPORT_KEYS: [&str; 6] = [
    "entry-id",
    "project",
    "task",
    "session",
    "milestone",
    "evidence",
];

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS legacy_imports (brain TEXT PRIMARY KEY COLLATE NOCASE, done_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS legacy_compiled (brain TEXT NOT NULL COLLATE NOCASE, entry_id TEXT NOT NULL, PRIMARY KEY(brain,entry_id));
";

/// One entry of a legacy log as written, before it is fitted to the contract.
#[derive(Debug, Default, PartialEq)]
struct LegacyEntry {
    /// The heading's line in the file, 1-based.
    line: usize,
    date: String,
    title: String,
    /// The entry's text after its heading, exactly as the file holds it.
    text: String,
    fields: BTreeMap<String, String>,
    /// Lines that are none of the five fields, kept in `changed`.
    extra: Vec<String>,
}

/// A legacy log's entries and its `<!-- compiled-through: YYYY-MM-DD -->` date.
struct LegacyLog {
    entries: Vec<LegacyEntry>,
    compiled_through: Option<String>,
}

/// `YYYY-MM-DD — Title` after `## `, accepting the dashes the skills have written.
fn heading(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("## ")?;
    let date = rest.get(..10)?;
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let title = rest[10..].trim_start();
    let title = ["—", "–", "-"]
        .iter()
        .find_map(|dash| title.strip_prefix(dash))?
        .trim();
    (!title.is_empty()).then(|| (date.to_owned(), title.to_owned()))
}

fn compiled_marker(line: &str) -> Option<String> {
    let date = line
        .trim()
        .strip_prefix("<!-- compiled-through:")?
        .strip_suffix("-->")?
        .trim();
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    Some(date.to_owned())
}

/// `- key: value` with a lowercase key.
fn field(line: &str) -> Option<(String, &str)> {
    let (key, value) = line.strip_prefix("- ")?.split_once(':')?;
    let named = !key.is_empty() && key.chars().all(|c| c.is_ascii_lowercase() || c == '-');
    named.then(|| (key.to_owned(), value.trim()))
}

fn parse(text: &str) -> LegacyLog {
    let mut entries: Vec<LegacyEntry> = vec![];
    let mut compiled_through = None;
    // Which part of the current entry a continuation line belongs to.
    enum Into {
        Field(String),
        Extra,
        Dropped,
    }
    let mut into = Into::Extra;
    for (index, line) in text.lines().enumerate() {
        if let Some(date) = compiled_marker(line) {
            compiled_through = compiled_through.max(Some(date));
            continue;
        }
        if let Some((date, title)) = heading(line) {
            entries.push(LegacyEntry {
                line: index + 1,
                date,
                title,
                ..Default::default()
            });
            into = Into::Extra;
            continue;
        }
        let Some(entry) = entries.last_mut() else {
            continue;
        };
        entry.text.push_str(line);
        entry.text.push('\n');
        let trimmed = line.trim();
        // The old export's entry markers and blank lines carry nothing.
        if trimmed.is_empty() || (trimmed.starts_with("<!--") && trimmed.ends_with("-->")) {
            continue;
        }
        match field(trimmed) {
            Some((key, value)) if FIELDS.contains(&key.as_str()) => {
                // A repeated field keeps both values.
                let joined = entry.fields.entry(key.clone()).or_default();
                if !joined.is_empty() {
                    joined.push_str("; ");
                }
                joined.push_str(value);
                into = Into::Field(key);
            }
            Some((key, _)) if EXPORT_KEYS.contains(&key.as_str()) => into = Into::Dropped,
            Some(_) => {
                entry.extra.push(trimmed.to_owned());
                into = Into::Extra;
            }
            None => match &into {
                Into::Field(key) => {
                    let value = entry.fields.entry(key.clone()).or_default();
                    value.push(' ');
                    value.push_str(trimmed);
                }
                Into::Extra => entry.extra.push(trimmed.to_owned()),
                Into::Dropped => {}
            },
        }
    }
    LegacyLog {
        entries,
        compiled_through,
    }
}

/// The entry's id: a ULID whose time is its date at 12:00 UTC and whose random part comes from a
/// hash of the repository, date, title and text.
fn legacy_id(repository: &str, entry: &LegacyEntry) -> String {
    let noon = chrono::NaiveDate::parse_from_str(&entry.date, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(12, 0, 0))
        .map(|at| at.and_utc().timestamp_millis())
        .unwrap_or_default();
    let mut hash = Sha256::new();
    for part in [repository, &entry.date, &entry.title, entry.text.trim()] {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    let digest = hash.finalize();
    let mut random = [0u8; 16];
    random[6..].copy_from_slice(&digest[..10]);
    brains::ulid(noon, u128::from_be_bytes(random))
}

/// `text` cut so that it is `by` characters shorter once `suffix` replaces what was cut.
fn shortened(text: &str, by: usize, suffix: &str) -> String {
    let keep = text
        .chars()
        .count()
        .saturating_sub(by + suffix.chars().count());
    let kept: String = text.chars().take(keep).collect();
    format!("{}{suffix}", kept.trim_end())
}

/// The entry fitted to the contract: one-line fields, `—` for an empty one, `observation` for a
/// type the contract does not know, and a `ref` the contract does not allow moved into `state`.
/// Over the cap, `changed` is shortened to end with a pointer to the legacy file at `sha`, and
/// then `state`, `why` and the title, should the rest still not fit.
fn fitted(repository: &str, entry: &LegacyEntry, sha: &str) -> (LogInput, String) {
    let value = |key: &str| one_line(entry.fields.get(key).map_or("", String::as_str));
    let or_none = |text: String| {
        if text.is_empty() {
            NO_REF.to_owned()
        } else {
            text
        }
    };
    let mut changed = value("changed");
    for extra in &entry.extra {
        changed = format!("{changed} {}", one_line(extra)).trim().to_owned();
    }
    let mut state = value("state");
    let mut reference = value("ref");
    // A ref is never shortened, so one too long to leave room for the rest moves into `state`.
    if !valid_ref(&reference) || reference.chars().count() > MAX_REF_CHARS {
        if !reference.is_empty() {
            state = format!("{state} (legacy ref: {reference})")
                .trim()
                .to_owned();
        }
        reference = NO_REF.into();
    }
    let mut title = one_line(&entry.title);
    if title.chars().count() > MAX_TITLE_CHARS {
        title = shortened(&title, title.chars().count() - MAX_TITLE_CHARS, "…");
    }
    let mut input = LogInput {
        session_id: String::new(),
        milestone: None,
        kind: match value("type").as_str() {
            "decision" => LogKind::Decision,
            _ => LogKind::Observation,
        },
        title,
        repository: None,
        ticket_id: None,
        changed: or_none(changed),
        why: or_none(value("why")),
        state: or_none(state),
    };
    let pointer = format!("… (full text in legacy logs/{repository}.md at {sha})");
    let over = |input: &LogInput| {
        body_chars(&entry_body(&entry.date, input, &reference)).saturating_sub(MAX_BODY_CHARS)
    };
    let by = over(&input);
    if by > 0 {
        input.changed = shortened(&input.changed, by, &pointer);
    }
    let rest: [fn(&mut LogInput) -> &mut String; 3] =
        [|i| &mut i.state, |i| &mut i.why, |i| &mut i.title];
    for part in rest {
        let by = over(&input);
        if by == 0 {
            break;
        }
        let text = part(&mut input);
        *text = shortened(text, by, "…");
    }
    (input, reference)
}

/// The commit that last changed each line of the file at `head`, by line number.
fn blame(checkout: &Path, head: &str, path: &str) -> Result<HashMap<usize, String>> {
    let porcelain = runtime::git_output(checkout, &["blame", "--porcelain", head, "--", path])?;
    let mut lines = HashMap::new();
    for header in porcelain.lines().filter(|l| !l.starts_with('\t')) {
        let mut words = header.split(' ');
        let (Some(sha), Some(_), Some(line)) = (words.next(), words.next(), words.next()) else {
            continue;
        };
        if sha.len() == 40
            && sha.chars().all(|c| c.is_ascii_hexdigit())
            && let Ok(line) = line.parse()
        {
            lines.insert(line, sha.to_owned());
        }
    }
    Ok(lines)
}

/// The GitHub login of the commit's author, or `None` when GitHub knows none.
fn commit_login(brain: &str, commit: &str, checkout: &Path, tools: &Tools) -> Option<String> {
    let endpoint = format!("repos/{brain}/commits/{commit}");
    let found = github_access::gh_json(tools, checkout, &endpoint).ok()?;
    let login = found["author"]["login"].as_str()?;
    valid_login(login).then(|| login.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(date: &str, title: &str, fields: &str) -> String {
        format!("## {date} — {title}\n{fields}\n")
    }

    fn only(text: &str) -> LegacyEntry {
        parse(text).entries.remove(0)
    }

    #[test]
    fn a_fitted_entry_keeps_to_the_contract_under_its_derived_id() {
        let legacy = only(&entry(
            "2026-09-02",
            "Retry once",
            "- type: decision\n- ref: main @ 3c1e9a0\n- changed: retry\n- why: transient\n- state: shipped",
        ));
        let (input, reference) = fitted("calls", &legacy, "3c1e9a0");
        let id = legacy_id("calls", &legacy);
        let body = entry_body(&legacy.date, &input, &reference);
        let text = memory::entry_text(&id, "calls", "rb", "skill", &body);
        let path = format!("calls/2026/09/2026-09-02-{id}.md");
        assert_eq!(memory::contract_problem(&path, &text), None);
    }

    #[test]
    fn blame_names_the_commit_of_each_line() {
        let repository = tempfile::tempdir().unwrap();
        let at = repository.path();
        crate::test_support::git(at, &["init", "-q", "-b", "main"]);
        fs::write(at.join("calls.md"), "one\n").unwrap();
        crate::test_support::git(at, &["add", "--all"]);
        crate::test_support::git(at, &["commit", "-q", "-m", "First"]);
        let first = crate::test_support::git(at, &["rev-parse", "HEAD"]);
        fs::write(at.join("calls.md"), "one\ntwo\n").unwrap();
        crate::test_support::git(at, &["add", "--all"]);
        crate::test_support::git(at, &["commit", "-q", "-m", "Second"]);
        let second = crate::test_support::git(at, &["rev-parse", "HEAD"]);
        let lines = blame(at, "HEAD", "calls.md").unwrap();
        assert_eq!((&lines[&1], &lines[&2]), (&first, &second));
    }

    #[test]
    fn a_commit_login_is_github_s_and_only_a_valid_one_is_taken() {
        let answering = |output: &'static str| Tools {
            gh: std::sync::Arc::new(move |_, _| {
                Ok(runtime::GhAnswer {
                    status: std::os::unix::process::ExitStatusExt::from_raw(0),
                    output: output.into(),
                    error: String::new(),
                })
            }),
            ..Tools::default()
        };
        let login =
            |output| commit_login("Sage/brain", "3c1e9a0", Path::new("."), &answering(output));
        assert_eq!(
            login(r#"{"author": {"login": "rb"}}"#).as_deref(),
            Some("rb")
        );
        assert_eq!(login(r#"{"author": {"login": "not a login"}}"#), None);
        assert_eq!(login(r#"{"author": null}"#), None);
    }

    #[test]
    fn a_legacy_entry_keeps_its_fields_and_drops_the_old_exports_bookkeeping() {
        let text = format!(
            "# calls — working log\n\n<!-- workspace-entry:01old -->\n{}\n- entry-id: 01old\n- project: p1\n- evidence: agent report\n{}",
            entry(
                "2026-09-02",
                "Retry once",
                "- type: decision\n- ref: main @ 3c1e9a0\n- changed: retry on\n  timeout\n- why: transient\n- state: shipped"
            ),
            entry(
                "2026-09-01",
                "Old entry",
                "Some prose before any field.\n- type: note\n- owner: rb"
            )
        );
        let log = parse(&text);
        assert_eq!(log.entries.len(), 2);
        let (input, reference) = fitted("calls", &log.entries[0], "abc1234");
        assert_eq!(reference, "main @ 3c1e9a0");
        assert_eq!(
            (input.kind, input.changed.as_str()),
            (LogKind::Decision, "retry on timeout")
        );
        assert_eq!(
            (input.why.as_str(), input.state.as_str()),
            ("transient", "shipped")
        );
        assert_eq!(log.entries[0].line, 4);
        // Lines the contract has no field for go in `changed`; an unknown type is an observation.
        let (input, _) = fitted("calls", &log.entries[1], "abc1234");
        assert_eq!(input.kind, LogKind::Observation);
        assert_eq!(input.changed, "Some prose before any field. - owner: rb");
        assert_eq!((input.why.as_str(), input.state.as_str()), (NO_REF, NO_REF));
    }

    #[test]
    fn a_repeated_field_keeps_both_values() {
        let legacy = only(&entry(
            "2026-09-02",
            "Twice",
            "- changed: first\n- why: w\n- changed: second\n  more",
        ));
        let (input, _) = fitted("calls", &legacy, "abc1234");
        assert_eq!(input.changed, "first; second more");
    }

    #[test]
    fn a_malformed_ref_becomes_none_with_its_text_in_state() {
        let legacy = only(&entry(
            "2026-09-02",
            "Deploy",
            "- type: observation\n- ref: PR 92 on main\n- changed: x\n- why: y\n- state: live",
        ));
        let (input, reference) = fitted("calls", &legacy, "abc1234");
        assert_eq!(reference, NO_REF);
        assert_eq!(input.state, "live (legacy ref: PR 92 on main)");
        // A well-formed ref too long to fit moves too, so the entry still fits the cap.
        let long = format!("{} @ 1234567", "a".repeat(2_100));
        let legacy = only(&entry(
            "2026-09-02",
            "Long ref",
            &format!("- ref: {long}\n- changed: c\n- why: w\n- state: s"),
        ));
        let (input, reference) = fitted("calls", &legacy, "abc1234");
        assert_eq!(reference, NO_REF);
        assert!(body_chars(&entry_body("2026-09-02", &input, &reference)) <= MAX_BODY_CHARS);
    }

    #[test]
    fn an_oversized_transcript_fits_the_cap_and_points_to_the_legacy_file() {
        let transcript = "word ".repeat(3_600);
        let legacy = only(&entry(
            "2026-09-02",
            "Long session",
            &format!(
                "- type: observation\n- ref: —\n- changed: {transcript}\n- why: y\n- state: z"
            ),
        ));
        let (input, reference) = fitted("simpatico-sage", &legacy, "abc1234");
        let body = entry_body("2026-09-02", &input, &reference);
        assert!(body_chars(&body) <= MAX_BODY_CHARS);
        assert!(
            body_chars(&body) > MAX_BODY_CHARS - 10,
            "only what must go is cut"
        );
        assert!(input.changed.starts_with("word word"));
        assert!(
            input
                .changed
                .ends_with("… (full text in legacy logs/simpatico-sage.md at abc1234)")
        );
        assert_eq!(
            (input.title.as_str(), input.why.as_str()),
            ("Long session", "y")
        );
        // Should the other fields be long too, they are cut after `changed`.
        let legacy = only(&entry(
            "2026-09-02",
            "Long everything",
            &format!("- changed: short\n- why: {transcript}\n- state: {transcript}"),
        ));
        let (input, reference) = fitted("simpatico-sage", &legacy, "abc1234");
        assert!(body_chars(&entry_body("2026-09-02", &input, &reference)) <= MAX_BODY_CHARS);
        assert!(
            input
                .changed
                .contains("full text in legacy logs/simpatico-sage.md")
        );
    }

    #[test]
    fn ids_are_derived_from_the_entry_and_dated_at_its_noon() {
        let legacy = only(&entry("2026-09-02", "Retry", "- changed: x"));
        let id = legacy_id("calls", &legacy);
        assert_eq!(id, legacy_id("calls", &legacy));
        assert_ne!(id, legacy_id("sms", &legacy));
        let edited = only(&entry("2026-09-02", "Retry", "- changed: y"));
        assert_ne!(id, legacy_id("calls", &edited));
        let noon = chrono::NaiveDate::from_ymd_opt(2026, 9, 2).unwrap();
        let noon = noon.and_hms_opt(12, 0, 0).unwrap().and_utc();
        assert_eq!(id[..10], brains::ulid(noon.timestamp_millis(), 0)[..10]);
    }

    #[test]
    fn the_compiled_through_marker_dates_the_compiled_entries() {
        let text = format!(
            "{}<!-- compiled-through: 2026-09-01 -->\n{}",
            entry("2026-09-03", "New", "- changed: x"),
            entry("2026-09-01", "Old", "- changed: y")
        );
        let log = parse(&text);
        assert_eq!(log.compiled_through.as_deref(), Some("2026-09-01"));
        assert_eq!(log.entries.len(), 2);
        // The marker is part of neither entry, so moving it changes no id.
        assert_eq!(log.entries[0].text, "- changed: x\n");
        assert_eq!(log.entries[1].text, "- changed: y\n");
    }
}
