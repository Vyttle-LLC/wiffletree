//! The one-time legacy log import (OpenSpec change `brain-log-capture`): each `## YYYY-MM-DD —
//! Title` entry of a brain's legacy `logs/<repo>.md` files on `main` becomes one entry file in
//! the gate's contract, with `source: skill`, in log-only batches of at most 200 files. An
//! entry's id comes from its repository, date, title and text, so importing again adds nothing.
//! Legacy files are only read, and nothing here logs an entry's text.
use super::*;
use memory::{
    MAX_BODY_CHARS, MAX_TITLE_CHARS, NO_REF, body_chars, entry_body, entry_text, one_line,
    valid_folder, valid_login, valid_ref,
};
use publish::Tools;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

/// The most files one import batch adds.
const MAX_IMPORT_FILES: usize = 200;
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

impl Host {
    /// The brain's folder, as its config on `main` or the first Space naming it says.
    fn import_folder(&self, brain: &Brain) -> Result<String> {
        if let Some(config) = brain.config.config() {
            return Ok(config.brain_dir.clone());
        }
        let space_brain = self.owner_brain(&SessionOwner::Brain(brain.repository.clone()))?;
        Ok(space_brain.map_or_else(|| "second-brain".into(), |b| b.folder))
    }

    /// The legacy `logs/<repo>.md` files on the brain's `main` as the host last fetched it.
    fn legacy_logs(&self, brain: &Brain, head: &str) -> Result<Vec<(String, String)>> {
        let logs = format!("{}/logs/", self.import_folder(brain)?);
        let listed = runtime::git_output(
            Path::new(&brain.checkout),
            &["ls-tree", "--name-only", head, "--", &logs],
        )?;
        Ok(listed
            .lines()
            .filter_map(|path| {
                let repository = path.strip_prefix(&logs)?.strip_suffix(".md")?;
                valid_folder(repository).then(|| (path.to_owned(), repository.to_owned()))
            })
            .collect())
    }

    /// Whether the Brains page offers Import legacy logs: `main` holds a legacy log and the
    /// import is not recorded as done.
    pub(crate) fn offers_legacy_import(&self, brain: &Brain) -> Result<bool> {
        let Some(head) = &brain.head else {
            return Ok(false);
        };
        let done: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM legacy_imports WHERE brain=?1)",
            [&brain.repository],
            |r| r.get(0),
        )?;
        Ok(!done && !self.legacy_logs(brain, head)?.is_empty())
    }

    /// Import legacy logs: queues each legacy entry not yet on `main` or in an unfinished batch
    /// as one entry file, in batches of at most 200 files, and records the entries at or before
    /// a file's `compiled-through` marker as already compiled. With nothing left to add, the
    /// import is recorded as done.
    pub(crate) fn import_legacy_logs(
        &mut self,
        repository: &str,
        tools: &Tools,
    ) -> Result<LegacyImport> {
        let brain = self.brain(repository)?;
        let head = (brain.head.clone()).with_context(|| {
            format!("{repository} has not been fetched yet; Sync now, then try again")
        })?;
        let importer = (self.gh_login.clone()).filter(|l| valid_login(l)).context(
            "Wiffletree has not read your GitHub login yet. Sign gh in, Sync now, then try again.",
        )?;
        let checkout = PathBuf::from(&brain.checkout);
        let folder = self.import_folder(&brain)?;
        let present: HashSet<String> = runtime::git_output(
            &checkout,
            &[
                "ls-tree",
                "-r",
                "--name-only",
                &head,
                "--",
                &format!("{folder}/logs/"),
            ],
        )?
        .lines()
        .map(str::to_owned)
        .collect();
        let pending: HashSet<String> = (self.brain_batches(Some(&brain.repository))?.into_iter())
            .filter(|batch| !batch.is_finished())
            .flat_map(|batch| batch.files)
            .collect();
        let sha = &head[..7];
        let mut logins: HashMap<String, Option<String>> = HashMap::new();
        let mut files: Vec<(String, Vec<u8>)> = vec![];
        let mut compiled = 0;
        // Entries already queued by an earlier import whose pull request has not merged yet.
        let mut waiting = 0;
        for (path, name) in self.legacy_logs(&brain, &head)? {
            let text = runtime::git_output(&checkout, &["show", &format!("{head}:{path}")])?;
            let log = parse(&text);
            let mut lines = None;
            for entry in &log.entries {
                let id = legacy_id(&name, entry);
                if log
                    .compiled_through
                    .as_ref()
                    .is_some_and(|d| entry.date <= *d)
                {
                    compiled += self.db.execute(
                        "INSERT OR IGNORE INTO legacy_compiled VALUES (?1,?2)",
                        params![brain.repository, id],
                    )?;
                }
                let (year, month) = (&entry.date[..4], &entry.date[5..7]);
                let file = format!("{folder}/logs/{name}/{year}/{month}/{}-{id}.md", entry.date);
                if present.contains(&file) {
                    continue;
                }
                if pending.contains(&file) {
                    waiting += 1;
                    continue;
                }
                if lines.is_none() {
                    lines = Some(blame(&checkout, &head, &path)?);
                }
                let commit = lines.as_ref().and_then(|l| l.get(&entry.line));
                let author = match commit {
                    Some(commit) => logins
                        .entry(commit.clone())
                        .or_insert_with(|| {
                            self.commit_login(&brain.repository, commit, &checkout, tools)
                        })
                        .clone(),
                    None => None,
                };
                let (input, reference) = fitted(&name, entry, sha);
                let body = entry_body(&entry.date, &input, &reference);
                let author = author.as_deref().unwrap_or(&importer);
                let text = entry_text(&id, &name, author, "skill", &body);
                files.push((file, text.into_bytes()));
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files.dedup_by(|a, b| a.0 == b.0);
        let added = files.len();
        let chunks: Vec<&[(String, Vec<u8>)]> = files.chunks(MAX_IMPORT_FILES).collect();
        for (index, chunk) in chunks.iter().enumerate() {
            let title = format!(
                "Legacy log import ({} of {}): {} entries",
                index + 1,
                chunks.len(),
                chunk.len()
            );
            self.queue_brain_batch(
                &brain.repository,
                BrainBatchKind::Log,
                BrainSource::BrainsPage,
                &title,
                chunk,
                &tools.git,
            )?;
        }
        // Done once every legacy entry is on `main`.
        let done = added == 0 && waiting == 0;
        if done {
            self.db.execute(
                "INSERT OR IGNORE INTO legacy_imports VALUES (?1,?2)",
                params![brain.repository, now()],
            )?;
        }
        Ok(LegacyImport {
            added,
            batches: chunks.len(),
            compiled,
            done,
        })
    }

    /// The GitHub login of the commit's author, or `None` when GitHub knows none.
    fn commit_login(
        &self,
        brain: &str,
        commit: &str,
        checkout: &Path,
        tools: &Tools,
    ) -> Option<String> {
        let endpoint = format!("repos/{brain}/commits/{commit}");
        let found = github_access::gh_json(tools, checkout, &endpoint).ok()?;
        let login = found["author"]["login"].as_str()?;
        valid_login(login).then(|| login.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brains::tests::{GitHub, REPO, name_brain, passing_checks, run};
    use crate::test_support::git;

    const LEGACY: &str = "second-brain/logs/calls.md";

    fn entry(date: &str, title: &str, fields: &str) -> String {
        format!("## {date} — {title}\n{fields}\n")
    }

    fn only(text: &str) -> LegacyEntry {
        parse(text).entries.remove(0)
    }

    /// Checks `text` at `path` against the gate's entry contract (`gate/README.md`), as its
    /// `log-schema` check reads it; answers the author.
    fn assert_contract(path: &str, text: &str) -> String {
        let (folder, file) = path.rsplit_once('/').unwrap();
        let (date, id) = (&file[..10], &file[11..file.len() - 3]);
        let repo = folder.split('/').nth(2).unwrap();
        assert_eq!(
            folder,
            format!("second-brain/logs/{repo}/{}/{}", &date[..4], &date[5..7])
        );
        let (frontmatter, body) = text
            .strip_prefix("---\n")
            .unwrap()
            .split_once("\n---\n")
            .unwrap();
        let keys: Vec<(&str, &str)> = (frontmatter.lines())
            .map(|line| line.split_once(": ").unwrap())
            .collect();
        let [
            ("id", got),
            ("repo", got_repo),
            ("author", author),
            ("source", "skill"),
        ] = keys[..]
        else {
            panic!("frontmatter {keys:?}");
        };
        assert_eq!((got, got_repo), (id, repo));
        assert!(valid_login(author));
        assert!(body_chars(body) <= MAX_BODY_CHARS, "{path}");
        let mut lines = body.lines();
        assert!(lines.next().unwrap().starts_with(&format!("## {date} — ")));
        let fields: Vec<(&str, &str)> = lines.map(|l| l[2..].split_once(": ").unwrap()).collect();
        let names: Vec<&str> = fields.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, FIELDS);
        assert!(["decision", "observation"].contains(&fields[0].1));
        assert!(valid_ref(fields[1].1), "{}", fields[1].1);
        assert!(fields.iter().all(|(_, value)| !value.trim().is_empty()));
        author.to_owned()
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

    #[test]
    fn the_import_writes_log_only_batches_of_at_most_200_files_once() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.legacy_pointer = home.path().join("no-pointer");
        let sage = host.create_space("Sage", "", "", None).unwrap().space;
        name_brain(&mut host, &sage.id, REPO);
        host.record_gh_login(Some("mhohlios".into()));
        let folder = tempfile::tempdir().unwrap();
        let github = GitHub::new(folder.path());
        let tools = github.tools();

        // A teammate GitHub knows wrote 200 entries, the newest of them above the compiled
        // marker; one entry is too long and one has a ref the contract does not allow.
        let mut legacy = String::from("# calls — working log\n\n");
        for day in 0..200 {
            let date =
                chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap() + chrono::Days::new(day);
            legacy.push_str(&entry(
                &date.to_string(),
                &format!("Entry {day}"),
                "- type: decision\n- ref: —\n- changed: c\n- why: w\n- state: s",
            ));
            if day == 0 {
                legacy.push_str("<!-- compiled-through: 2026-01-10 -->\n");
            }
        }
        let known = github.commit_file_on_main(LEGACY, &legacy);
        github
            .commit_authors
            .lock()
            .unwrap()
            .insert(known, "rb-boyce".into());
        // Someone GitHub knows no login for added two more at the top.
        let added = format!(
            "{}{}{legacy}",
            entry(
                "2026-10-01",
                "Transcript",
                &format!("- changed: {}", "x".repeat(18_000))
            ),
            entry("2026-10-02", "Bad ref", "- ref: see PR 92\n- changed: c"),
        );
        github.commit_file_on_main(LEGACY, &added);
        assert!(run(&mut host, &tools));
        assert!(host.brains_page().unwrap().brains[0].legacy_import);

        let first = host.import_legacy_logs(REPO, &tools).unwrap();
        assert_eq!((first.added, first.batches, first.done), (202, 2, false));
        assert_eq!(
            first.compiled, 10,
            "January 1 to 10 are at or before the marker"
        );
        let batches = host.brain_batches(Some(REPO)).unwrap();
        assert_eq!(
            batches.iter().map(|b| b.files.len()).collect::<Vec<_>>(),
            [200, 2]
        );
        let mut authors = std::collections::BTreeMap::<String, usize>::new();
        for batch in &batches {
            assert_eq!(batch.kind, BrainBatchKind::Log);
            let worktree = host.home.join("brain-batches").join(&batch.id);
            for path in &batch.files {
                assert!(path.starts_with("second-brain/logs/calls/2026/"), "{path}");
                let text = fs::read_to_string(worktree.join(path)).unwrap();
                *authors.entry(assert_contract(path, &text)).or_default() += 1;
            }
            let changed = git(&worktree, &["diff", "--name-status", "HEAD~1", "HEAD"]);
            assert!(
                changed.lines().all(|l| l.starts_with("A\t")),
                "only adds: {changed}"
            );
        }
        assert_eq!(
            authors,
            [("mhohlios".into(), 2), ("rb-boyce".into(), 200)].into()
        );
        let commit_reads = (github.gh_calls("api").iter())
            .filter(|call| {
                let endpoint = call.last().unwrap();
                endpoint.contains("/commits/") && !endpoint.contains("check-runs")
            })
            .count();
        assert_eq!(commit_reads, 2, "one read per commit");

        // While its pull requests are open, a second import adds nothing.
        let again = host.import_legacy_logs(REPO, &tools).unwrap();
        assert_eq!(
            (again.added, again.batches, again.done, again.compiled),
            (0, 0, false, 0)
        );
        assert!(host.brains_page().unwrap().brains[0].legacy_import);

        host.confirm_brain(REPO).unwrap();
        run(&mut host, &tools);
        run(&mut host, &tools);
        passing_checks(&github);
        run(&mut host, &tools);
        github.merge(214);
        github.merge(215);
        run(&mut host, &tools);
        run(&mut host, &tools);
        let legacy_now = git(
            Path::new(&host.brain(REPO).unwrap().checkout),
            &["show", &format!("HEAD:{LEGACY}")],
        );
        assert_eq!(legacy_now, added.trim_end(), "the legacy file is untouched");
        let done = host.import_legacy_logs(REPO, &tools).unwrap();
        assert_eq!((done.added, done.batches, done.done), (0, 0, true));
        assert!(!host.brains_page().unwrap().brains[0].legacy_import);
        assert_eq!(host.brain_batches(Some(REPO)).unwrap().len(), 2);
    }
}
