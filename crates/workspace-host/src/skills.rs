//! The human's own rules or rewrite for each skill, as plain files in the shared folder's
//! `skills`, one folder per skill with a history file for every change. Only the human's commands
//! write here; no agent tool reaches it. A missing file means none, so a fresh install or an
//! update needs nothing.
use crate::*;
use serde::Deserialize;
use std::io::{ErrorKind, Write as _};

const RULES: &str = "additions.md";
const REWRITE: &str = "rewrite.md";
const BASE: &str = "base.md";
/// A skill's current state, which sync compares and pauses as one.
pub(crate) const CURRENT: [&str; 3] = [RULES, REWRITE, BASE];
const HISTORY: &str = "history";
/// How a file written at a time is named, as a history entry's "2026-10-16T09-30-00.000Z".
const TIME: &str = "%Y-%m-%dT%H-%M-%S%.3fZ";

pub fn stock(skill: Skill) -> &'static str {
    match skill {
        Skill::Coordinating => include_str!("../skills/stock/coordinating.md"),
        Skill::Implementing => include_str!("../skills/stock/implementing.md"),
        Skill::Reviewing => include_str!("../skills/stock/reviewing.md"),
        Skill::KeepingAPr => include_str!("../skills/stock/keeping-a-pr.md"),
    }
}
pub fn stock_version(skill: Skill) -> &'static str {
    version(stock(skill))
}
/// The version on a bundled text's first line, as "v1" in "# Implementing — v1".
pub fn version(text: &str) -> &str {
    let first = text.lines().next().unwrap_or_default();
    first.rsplit_once(" — ").map_or("", |(_, version)| version)
}
/// A stock version's number, as 2 for "v2".
fn number(version: &str) -> Option<u32> {
    version.strip_prefix('v')?.parse().ok()
}
/// Refuses rules or a rewrite over the limit; `what` names it, as "Your rules".
pub(crate) fn check_text(what: &str, text: &str) -> Result<()> {
    ensure!(
        text.chars().count() <= MAX_SKILL_TEXT,
        "{what} can be at most 32,000 characters"
    );
    Ok(())
}
/// The text the human edits in a skill's folder of the shared folder: rules or a rewrite.
pub(crate) fn is_edited_text(path: &str) -> bool {
    let mut parts = path.split('/');
    let parts = [(); 4].map(|()| parts.next());
    matches!(
        parts,
        [Some("skills"), Some(_), Some(RULES | REWRITE), None]
    )
}
/// What the human has written for a skill: rules in Additions mode, or a rewrite.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edits {
    pub rules: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rewrite: Option<Rewritten>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rewritten {
    pub text: String,
    /// The stock text the rewrite started from, or was last marked up to date against.
    pub base: String,
}
/// What a history entry records: the skill's whole state after a change, and the change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub change: Change,
    pub stock_version: String,
    #[serde(flatten)]
    pub edits: Edits,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    SavedRules,
    StartedRewrite,
    SavedRewrite,
    MarkedUpToDate,
    ResetToStock,
    /// `entry` names the entry whose state was restored.
    Restored {
        entry: String,
    },
    /// A release brought a stock version other than the one `previous` entry recorded.
    NewStockVersion {
        previous: String,
    },
}
/// A history entry by its name, the time it was written, or why it could not be read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recorded {
    pub name: String,
    pub entry: Result<Entry, String>,
}
/// A line of what changed from a rewrite's base to the stock text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "text", rename_all = "snake_case")]
pub enum Line {
    Unchanged(String),
    Removed(String),
    Added(String),
}
pub struct SkillStore<'a> {
    root: PathBuf,
    recorder: Option<shared::Recorder<'a>>,
}
impl SkillStore<'_> {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            recorder: None,
        }
    }
    fn folder(&self, skill: Skill) -> PathBuf {
        self.root.join(skill.folder())
    }
    /// A file that cannot be read counts as missing, and says why. In Rewrite mode the rules
    /// are not read, since none are sent.
    fn read(&self, skill: Skill) -> (Edits, Option<String>) {
        let folder = self.folder(skill);
        let mut unreadable = vec![];
        let mut read = |file: &str| match fs::read_to_string(folder.join(file)) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => {
                unreadable.push(format!("{file} could not be read: {error}"));
                None
            }
        };
        let rewrite = read(REWRITE).map(|text| Rewritten {
            text,
            base: read(BASE).unwrap_or_default(),
        });
        let rules = match rewrite {
            Some(_) => String::new(),
            None => read(RULES).unwrap_or_default(),
        };
        let unreadable = (!unreadable.is_empty()).then(|| unreadable.join("; "));
        (Edits { rules, rewrite }, unreadable)
    }
    /// What the human has written, refused while a file cannot be read, so no change drops it.
    fn readable(&self, skill: Skill) -> Result<Edits> {
        match self.read(skill) {
            (edits, None) => Ok(edits),
            (_, Some(unreadable)) => bail!("{unreadable}"),
        }
    }
    pub fn state(&self, skill: Skill) -> SkillState {
        let (Edits { rules, rewrite }, unreadable) = self.read(skill);
        SkillState {
            skill,
            stock: stock(skill).into(),
            stock_version: stock_version(skill).into(),
            rules,
            rewrite: rewrite.map(|Rewritten { text, base }| Rewrite {
                text,
                base_version: version(&base).into(),
                out_of_date: base != stock(skill),
                base,
            }),
            unreadable,
            newer_stock_version: None,
        }
    }
    pub fn save_rules(&self, skill: Skill, rules: &str) -> Result<()> {
        check_text("Your rules", rules)?;
        self.readable(skill)?;
        let edits = Edits {
            rules: rules.into(),
            rewrite: None,
        };
        self.change(skill, Change::SavedRules, edits)
    }
    /// The rewrite starts as the stock text and the human's rules under their heading, based on
    /// the stock text.
    pub fn start_rewrite(&self, skill: Skill) -> Result<()> {
        let Edits { rules, rewrite } = self.readable(skill)?;
        ensure!(rewrite.is_none(), "This skill is already rewritten");
        let mut text = stock(skill).to_owned();
        if !rules.is_empty() {
            text.push_str(&format!("\n{}\n{rules}", provider::rules_heading(skill)));
        }
        let rewrite = Rewritten {
            text,
            base: stock(skill).into(),
        };
        let edits = Edits {
            rules: String::new(),
            rewrite: Some(rewrite),
        };
        self.change(skill, Change::StartedRewrite, edits)
    }
    pub fn save_rewrite(&self, skill: Skill, text: &str) -> Result<()> {
        check_text("Your rewrite", text)?;
        self.change_rewrite(skill, Change::SavedRewrite, |rewrite| {
            rewrite.text = text.into()
        })
    }
    /// Records this release's stock text as the base, after the human has brought in by hand
    /// what they wanted; the rewrite is not changed.
    pub fn mark_up_to_date(&self, skill: Skill) -> Result<()> {
        self.change_rewrite(skill, Change::MarkedUpToDate, |rewrite| {
            rewrite.base = stock(skill).into()
        })
    }
    pub fn reset(&self, skill: Skill) -> Result<()> {
        self.change(skill, Change::ResetToStock, Edits::default())
    }
    /// Writes the entry's state back, a rewrite with its base, as a new entry; the history keeps
    /// every entry.
    pub fn restore(&self, skill: Skill, entry: &str) -> Result<()> {
        let (_, restored) = self
            .history(skill)
            .into_iter()
            .find(|(name, _)| name == entry)
            .context("That history entry does not exist")?;
        let change = Change::Restored {
            entry: entry.into(),
        };
        self.change(skill, change, restored.edits)
    }
    /// What changed from a rewrite's base to this release's stock text; nothing without one.
    pub fn stock_changes(&self, skill: Skill) -> Vec<Line> {
        let Some(Rewritten { base, .. }) = self.read(skill).0.rewrite else {
            return vec![];
        };
        let diff = similar::TextDiff::from_lines(base.as_str(), stock(skill));
        let lines = diff.iter_all_changes().map(|change| {
            let text = change.value().trim_end_matches('\n').to_owned();
            match change.tag() {
                similar::ChangeTag::Equal => Line::Unchanged(text),
                similar::ChangeTag::Delete => Line::Removed(text),
                similar::ChangeTag::Insert => Line::Added(text),
            }
        });
        lines.collect()
    }
    fn change_rewrite(
        &self,
        skill: Skill,
        change: Change,
        edit: impl FnOnce(&mut Rewritten),
    ) -> Result<()> {
        let mut edits = self.readable(skill)?;
        let rewrite = (edits.rewrite.as_mut()).context("Start a rewrite of this skill first")?;
        edit(rewrite);
        self.change(skill, change, edits)
    }
    /// At start, an entry for each skill whose history records only older stock versions, so
    /// machines on different releases sharing a history do not add one back and forth. A skill
    /// with no history gets none, and one whose files cannot be read waits for a start when
    /// they can, so no entry records a state it could not see.
    pub fn record_new_stock_versions(&self) -> Result<()> {
        for skill in Skill::ALL {
            let history = self.history(skill);
            let Some((_, latest)) = history.last().cloned() else {
                continue;
            };
            let this_release = number(stock_version(skill));
            let all_older = (history.iter()).all(|(_, e)| number(&e.stock_version) < this_release);
            let _writing = self.writing(skill);
            let (edits, unreadable) = self.read(skill);
            if all_older && unreadable.is_none() {
                let change = Change::NewStockVersion {
                    previous: latest.stock_version,
                };
                let (path, entry) = self.record(skill, change, edits)?;
                self.recorded(&path, Some(&entry))?;
            }
        }
        Ok(())
    }
    /// What a role's next turn is sent, with its skill as the store holds it.
    pub fn instructions(&self, role: Role) -> String {
        provider::role_instructions(role, &self.state(Skill::for_role(role)))
    }
    /// Oldest first, the entries that can be read, each with its name, the time it was written.
    pub fn history(&self, skill: Skill) -> Vec<(String, Entry)> {
        (self.entries(skill).into_iter())
            .filter_map(|Recorded { name, entry }| Some((name, entry.ok()?)))
            .collect()
    }
    /// Newest first, as the skill's page lists them.
    pub fn listing(&self, skill: Skill) -> Vec<Recorded> {
        let mut entries = self.entries(skill);
        entries.reverse();
        entries
    }
    /// Oldest first, an entry that cannot be read with why. A history that cannot be read
    /// counts as none.
    fn entries(&self, skill: Skill) -> Vec<Recorded> {
        let Ok(files) = fs::read_dir(self.folder(skill).join(HISTORY)) else {
            return vec![];
        };
        let mut entries = files
            .filter_map(|file| {
                let path = file.ok()?.path();
                let name = path
                    .file_name()?
                    .to_str()?
                    .strip_suffix(".json")?
                    .to_owned();
                let entry = fs::read(&path)
                    .map_err(|error| error.to_string())
                    .and_then(|bytes| {
                        serde_json::from_slice(&bytes).map_err(|error| error.to_string())
                    });
                Some(Recorded { name, entry })
            })
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }
    /// Makes the files hold `edits` and records the change, all or nothing: the new files are
    /// staged first, then each file the change touches is moved aside and its new one renamed
    /// into place, and a failure at any step, the record's included, puts back what was moved.
    /// `rewrite.md` decides the mode, so it changes last. The other mode's file is ignored in
    /// this one, so it is removed afterwards when it can be.
    fn change(&self, skill: Skill, change: Change, edits: Edits) -> Result<()> {
        let _writing = self.writing(skill);
        let (files, leftover) = match &edits.rewrite {
            Some(Rewritten { text, base }) => (
                [(BASE, Some(base.as_str())), (REWRITE, Some(text.as_str()))],
                RULES,
            ),
            None => {
                let rules = Some(edits.rules.as_str()).filter(|rules| !rules.is_empty());
                ([(RULES, rules), (REWRITE, None)], BASE)
            }
        };
        let folder = self.folder(skill);
        fs::create_dir_all(&folder)?;
        let staged = (files.into_iter())
            .map(|(file, text)| {
                let staged = text.map(|text| stage(&folder, text)).transpose()?;
                Ok((folder.join(file), staged))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut moved = vec![];
        let replace = || -> Result<(PathBuf, Vec<u8>)> {
            for (path, staged) in staged {
                moved.push(Moved::aside(&folder, path.clone())?);
                if let Some(staged) = staged {
                    staged.persist(&path)?;
                }
            }
            self.record(skill, change, edits.clone())
        };
        let entry = match replace() {
            Ok(entry) => entry,
            Err(error) => {
                let kept: Vec<_> = (moved.into_iter().rev())
                    .filter_map(Moved::put_back)
                    .map(|kept| kept.display().to_string())
                    .collect();
                if kept.is_empty() {
                    return Err(error);
                }
                bail!(
                    "{error:#}. What was there before could not be put back, so it is kept at {}",
                    kept.join(" and ")
                );
            }
        };
        // Sync's record notes the change only once it holds.
        for (file, text) in files {
            self.recorded(&folder.join(file), text.map(str::as_bytes))?;
        }
        self.recorded(&entry.0, Some(&entry.1))?;
        match fs::remove_file(folder.join(leftover)) {
            Err(error) if error.kind() != ErrorKind::NotFound => Ok(()),
            _ => self.recorded(&folder.join(leftover), None),
        }
    }
    fn recorded(&self, path: &Path, content: Option<&[u8]>) -> Result<()> {
        match &self.recorder {
            Some(recorder) => recorder.wrote(path, content),
            None => Ok(()),
        }
    }
    /// Marks the skill's folder as being written, so no sync pass reads it mid-change.
    fn writing(&self, skill: Skill) -> Option<shared::Writing> {
        let key = format!("skills/{}/", skill.folder());
        self.recorder
            .as_ref()
            .map(|recorder| recorder.writing(&key))
    }
    /// The newest stock version the history records, when it is newer than this release's.
    fn newer_stock_version(&self, skill: Skill) -> Option<String> {
        let history = self
            .history(skill)
            .into_iter()
            .map(|(_, e)| e.stock_version);
        let this_release = number(stock_version(skill));
        history
            .filter(|version| number(version) > this_release)
            .max_by_key(|version| number(version))
    }
    /// One file per entry, named by its time, never replacing another; answers with the entry's
    /// path and content.
    fn record(&self, skill: Skill, change: Change, edits: Edits) -> Result<(PathBuf, Vec<u8>)> {
        let folder = self.folder(skill).join(HISTORY);
        fs::create_dir_all(&folder)?;
        let entry = Entry {
            change,
            stock_version: stock_version(skill).into(),
            edits,
        };
        let bytes = serde_json::to_vec_pretty(&entry)?;
        let mut staged = tempfile::NamedTempFile::new_in(&folder)?;
        staged.write_all(&bytes)?;
        let mut at = chrono::Utc::now();
        loop {
            let name = format!("{}.json", at.format(TIME));
            match staged.persist_noclobber(folder.join(&name)) {
                Ok(_) => return Ok((folder.join(name), bytes)),
                Err(error) if error.error.kind() == ErrorKind::AlreadyExists => {
                    staged = error.file;
                    at += chrono::TimeDelta::milliseconds(1);
                }
                Err(error) => return Err(error.error.into()),
            }
        }
    }
}
/// A file's new text, written beside it to be renamed into place.
fn stage(folder: &Path, text: &str) -> Result<tempfile::NamedTempFile> {
    let mut staged = tempfile::NamedTempFile::new_in(folder)?;
    staged.write_all(text.as_bytes())?;
    Ok(staged)
}
/// A file a change moved aside, none when there was no file. It is deleted once the change is
/// recorded, or put back when the change fails, and kept beside it when it cannot be.
struct Moved {
    path: PathBuf,
    previous: Option<tempfile::TempPath>,
}
impl Moved {
    fn aside(folder: &Path, path: PathBuf) -> Result<Self> {
        let previous = tempfile::NamedTempFile::new_in(folder)?.into_temp_path();
        match fs::rename(&path, &previous) {
            Ok(()) => Ok(Self {
                path,
                previous: Some(previous),
            }),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self {
                path,
                previous: None,
            }),
            Err(error) => Err(error.into()),
        }
    }
    /// Where the file is kept, as `rewrite.md.kept-<time>`, when it cannot be put back.
    fn put_back(self) -> Option<PathBuf> {
        let Some(previous) = self.previous else {
            let _ = fs::remove_file(&self.path);
            return None;
        };
        let Err(error) = previous.persist(&self.path) else {
            return None;
        };
        let mut kept = self.path.into_os_string();
        kept.push(format!(".kept-{}", chrono::Utc::now().format(TIME)));
        match error.path.persist(&kept) {
            Ok(()) => Some(kept.into()),
            Err(error) => error.path.keep().ok(),
        }
    }
}
/// The store of the host whose data folder is `home`.
pub fn store(home: &Path) -> SkillStore<'static> {
    SkillStore::new(shared::SharedStore::of(home).skills_folder())
}
impl Host {
    pub fn skills(&self) -> SkillStore<'_> {
        SkillStore {
            recorder: Some(self.recorder()),
            ..store(&self.home)
        }
    }
    /// What a role's next turn is sent, read from the store each time.
    pub fn instructions(&self, role: Role) -> String {
        self.skills().instructions(role)
    }
    pub(crate) fn skill_states(&self) -> Vec<SkillState> {
        let store = self.skills();
        let state = |skill| SkillState {
            newer_stock_version: store.newer_stock_version(skill),
            ..store.state(skill)
        };
        Skill::ALL.map(state).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES_HEADING: &str = "## Your own rules for Implementing\n";

    fn host() -> (tempfile::TempDir, Host) {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        (home, host)
    }

    fn changes(host: &Host, skill: Skill) -> Vec<Change> {
        let history = host.skills().history(skill);
        history.into_iter().map(|(_, entry)| entry.change).collect()
    }

    fn rules_file(host: &Host) -> PathBuf {
        host.home.join("shared/skills/implementing/additions.md")
    }

    #[test]
    fn a_fresh_install_is_at_stock_and_writes_nothing() {
        let (_home, host) = host();
        assert!(!host.shared().skills_folder().exists());
        for state in host.skill_states() {
            assert_eq!((state.rules.as_str(), &state.unreadable), ("", &None));
            assert_eq!(state.stock_version, "v1");
            assert_eq!(state.stock, stock(state.skill));
        }
        assert_eq!(
            host.instructions(Role::Implementer),
            provider::skill(Role::Implementer)
        );
    }

    #[test]
    fn saving_rules_writes_the_file_and_one_entry_and_sends_them_next() {
        let (_home, mut host) = host();
        let rules = "Write PR descriptions with a Summary and a Testing section";
        let saved = host
            .execute(Command::SaveSkillRules {
                skill: Skill::Implementing,
                rules: rules.into(),
            })
            .unwrap();
        assert_eq!(saved["rules"], rules);
        assert_eq!(fs::read_to_string(rules_file(&host)).unwrap(), rules);
        let history = host.skills().history(Skill::Implementing);
        let entry = Entry {
            change: Change::SavedRules,
            stock_version: "v1".into(),
            edits: Edits {
                rules: rules.into(),
                rewrite: None,
            },
        };
        assert_eq!(
            history.into_iter().map(|(_, e)| e).collect::<Vec<_>>(),
            [entry]
        );
        let instructions = host.instructions(Role::Implementer);
        assert!(instructions.starts_with(&provider::skill(Role::Implementer)));
        assert!(instructions.ends_with(&format!(
            "{RULES_HEADING}{}\n{rules}",
            provider::RULES_SENTENCE
        )));
        let snapshot = host.snapshot().unwrap();
        let skills: Vec<_> = snapshot.skills.iter().map(|s| s.skill).collect();
        assert_eq!(skills, Skill::ALL);
        assert_eq!(snapshot.skills[1].rules, rules);
        // The same rules reach every role of the skill, and no other skill's.
        assert!(host.instructions(Role::Maintenance).ends_with(rules));
        assert!(!host.instructions(Role::Tester).contains(rules));
    }

    #[test]
    fn an_empty_text_removes_the_rules() {
        let (_home, host) = host();
        host.skills()
            .save_rules(Skill::Implementing, "Rule")
            .unwrap();
        host.skills().save_rules(Skill::Implementing, "").unwrap();
        assert!(!rules_file(&host).exists());
        assert!(!host.instructions(Role::Implementer).contains(RULES_HEADING));
        assert_eq!(changes(&host, Skill::Implementing).len(), 2);
    }

    #[test]
    fn a_file_that_cannot_be_read_counts_as_none_and_says_so() {
        let (_home, host) = host();
        fs::create_dir_all(rules_file(&host).parent().unwrap()).unwrap();
        fs::write(rules_file(&host), b"\xff\xfe not text").unwrap();
        let state = host.skills().state(Skill::Implementing);
        assert_eq!(state.rules, "");
        assert!(
            state
                .unreadable
                .unwrap()
                .starts_with("additions.md could not be read")
        );
        assert_eq!(
            host.instructions(Role::Implementer),
            provider::skill(Role::Implementer)
        );
    }

    #[test]
    fn a_folder_that_cannot_be_read_counts_as_none_and_the_host_still_starts() {
        use std::os::unix::fs::PermissionsExt as _;
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        host.skills()
            .save_rules(Skill::Implementing, "Rule")
            .unwrap();
        drop(host);
        let folder = home.path().join("shared/skills/implementing");
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o000)).unwrap();
        let host = Host::open(home.path()).unwrap();
        let state = host.skills().state(Skill::Implementing);
        let instructions = host.instructions(Role::Implementer);
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(state.unreadable.is_some());
        assert_eq!(instructions, provider::skill(Role::Implementer));
    }

    #[test]
    fn a_text_too_long_is_refused_and_the_rules_stay() {
        let (_home, mut host) = host();
        host.skills()
            .save_rules(Skill::Implementing, "Kept")
            .unwrap();
        let refused = host
            .execute(Command::SaveSkillRules {
                skill: Skill::Implementing,
                rules: "x".repeat(40_000),
            })
            .unwrap_err();
        assert!(refused.to_string().contains("32,000"), "{refused}");
        assert_eq!(host.skills().state(Skill::Implementing).rules, "Kept");
        assert_eq!(changes(&host, Skill::Implementing), [Change::SavedRules]);
        // The limit counts characters, not bytes.
        let longest = "é".repeat(MAX_SKILL_TEXT);
        host.skills()
            .save_rules(Skill::Implementing, &longest)
            .unwrap();
    }

    #[test]
    fn resetting_keeps_the_rules_in_the_history_and_restoring_brings_them_back() {
        let (_home, mut host) = host();
        let rules = "One\nTwo\nThree";
        host.skills()
            .save_rules(Skill::Implementing, rules)
            .unwrap();
        let reset = Command::ResetSkill {
            skill: Skill::Implementing,
        };
        assert_eq!(host.execute(reset).unwrap()["rules"], "");
        assert!(!rules_file(&host).exists());
        assert!(!host.instructions(Role::Implementer).contains(RULES_HEADING));
        let history = host.skills().history(Skill::Implementing);
        let (saved, entry) = &history[0];
        assert_eq!(entry.edits.rules, rules);

        let restore = Command::RestoreSkill {
            skill: Skill::Implementing,
            entry: saved.clone(),
        };
        assert_eq!(host.execute(restore).unwrap()["rules"], rules);
        assert!(host.instructions(Role::Implementer).ends_with(rules));
        let restored = Change::Restored {
            entry: saved.clone(),
        };
        assert_eq!(
            changes(&host, Skill::Implementing),
            [Change::SavedRules, Change::ResetToStock, restored]
        );
        let unknown = Command::RestoreSkill {
            skill: Skill::Implementing,
            entry: "../../settings".into(),
        };
        assert!(host.execute(unknown).is_err());
    }

    #[test]
    fn the_history_lists_newest_first_and_keeps_an_entry_that_cannot_be_read() {
        let (_home, mut host) = host();
        host.skills()
            .save_rules(Skill::Implementing, "Rule")
            .unwrap();
        host.skills().reset(Skill::Implementing).unwrap();
        let history = host.home.join("shared/skills/implementing/history");
        fs::write(history.join("2000-01-01T00-00-00.000Z.json"), "not json").unwrap();
        let listed: Vec<Recorded> = serde_json::from_value(
            host.execute(Command::SkillHistory {
                skill: Skill::Implementing,
            })
            .unwrap(),
        )
        .unwrap();
        let changes: Vec<_> = (listed.iter())
            .map(|recorded| recorded.entry.as_ref().map(|entry| entry.change.clone()))
            .collect();
        assert!(
            matches!(
                &changes[..],
                [Ok(Change::ResetToStock), Ok(Change::SavedRules), Err(_)]
            ),
            "{changes:?}"
        );
        assert_eq!(listed[2].name, "2000-01-01T00-00-00.000Z");
        // Restoring and the update check still read only the entries that can be read.
        assert_eq!(host.skills().history(Skill::Implementing).len(), 2);
    }

    /// Rewrites the skill's one history entry as a release whose stock was v0 wrote it.
    fn written_by_v0(host: &Host, skill: Skill) {
        let (name, _) = host.skills().history(skill).remove(0);
        let path = (host.shared().skills_folder().join(skill.folder()))
            .join(format!("history/{name}.json"));
        let entry = fs::read_to_string(&path).unwrap();
        fs::write(&path, entry.replace("\"v1\"", "\"v0\"")).unwrap();
    }

    #[test]
    fn an_update_waits_while_the_rules_cannot_be_read() {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        host.skills()
            .save_rules(Skill::Implementing, "Rule")
            .unwrap();
        written_by_v0(&host, Skill::Implementing);
        fs::write(rules_file(&host), b"\xff not text").unwrap();
        drop(host);

        let host = Host::open(home.path()).unwrap();
        assert_eq!(changes(&host, Skill::Implementing), [Change::SavedRules]);
        fs::write(rules_file(&host), "Rule").unwrap();
        drop(host);
        let host = Host::open(home.path()).unwrap();
        let (_, latest) = host.skills().history(Skill::Implementing).pop().unwrap();
        assert_eq!(
            (latest.stock_version.as_str(), latest.edits.rules.as_str()),
            ("v1", "Rule")
        );
    }

    #[test]
    fn a_new_stock_version_entry_that_cannot_be_written_does_not_stop_the_host() {
        use std::os::unix::fs::PermissionsExt as _;
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        host.skills().save_rules(Skill::Reviewing, "Rule").unwrap();
        written_by_v0(&host, Skill::Reviewing);
        drop(host);
        let history = home.path().join("shared/skills/reviewing/history");
        fs::set_permissions(&history, fs::Permissions::from_mode(0o555)).unwrap();
        let opened = Host::open(home.path());
        fs::set_permissions(&history, fs::Permissions::from_mode(0o755)).unwrap();
        let host = opened.unwrap();
        assert_eq!(changes(&host, Skill::Reviewing), [Change::SavedRules]);
    }

    #[test]
    fn the_context_probe_reads_the_stored_rules_beside_a_running_host() {
        let (_home, host) = host();
        host.skills()
            .save_rules(Skill::Implementing, "Rule")
            .unwrap();
        let probed = store(&host.home).instructions(Role::Implementer);
        assert_eq!(probed, host.instructions(Role::Implementer));
        assert!(probed.ends_with("Rule"));
    }

    #[test]
    fn an_update_adds_one_new_stock_version_entry_and_keeps_the_rules() {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        host.skills()
            .save_rules(Skill::Reviewing, "Read the tests first")
            .unwrap();
        written_by_v0(&host, Skill::Reviewing);
        drop(host);

        let host = Host::open(home.path()).unwrap();
        let previous = Change::NewStockVersion {
            previous: "v0".into(),
        };
        assert_eq!(
            changes(&host, Skill::Reviewing),
            [Change::SavedRules, previous.clone()]
        );
        let (_, latest) = host.skills().history(Skill::Reviewing).pop().unwrap();
        assert_eq!(latest.stock_version, "v1");
        assert_eq!(latest.edits.rules, "Read the tests first");
        // The new stock text is sent with the same rules, and nothing else gains an entry.
        let reviewing = host.skills().state(Skill::Reviewing);
        assert_eq!(
            host.instructions(Role::Tester),
            provider::instructions(Role::Tester, stock(Skill::Reviewing), &reviewing.rules)
        );
        assert!(!host.home.join("shared/skills/implementing").exists());
        drop(host);
        let host = Host::open(home.path()).unwrap();
        assert_eq!(changes(&host, Skill::Reviewing).len(), 2);
    }

    #[test]
    fn two_machines_on_different_releases_write_no_entry_back_and_forth() {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        host.skills()
            .save_rules(Skill::Coordinating, "Rule")
            .unwrap();
        // The other machine, on v2, recorded its new stock version in the shared history.
        let (name, _) = host.skills().history(Skill::Coordinating).remove(0);
        let history = host.shared().skills_folder().join("coordinating/history");
        let entry = fs::read_to_string(history.join(format!("{name}.json"))).unwrap();
        let entry = entry.replace("\"v1\"", "\"v2\"");
        fs::write(history.join("2099-01-01T00-00-00.000Z.json"), entry).unwrap();
        drop(host);
        let host = Host::open(home.path()).unwrap();
        assert_eq!(host.skills().history(Skill::Coordinating).len(), 2);
        let states = host.skill_states();
        assert_eq!(states[0].newer_stock_version.as_deref(), Some("v2"));
        assert_eq!(states[1].newer_stock_version, None);
    }

    fn coordinating(host: &Host, file: &str) -> PathBuf {
        host.home.join("shared/skills/coordinating").join(file)
    }

    /// Bases Coordinating's rewrite on a stand-in for an earlier stock text, as if it had been
    /// written before a release that changed it.
    fn based_on_an_earlier_release(host: &Host, base: &str) {
        fs::write(coordinating(host, "base.md"), base).unwrap();
    }

    #[test]
    fn starting_a_rewrite_with_rules_folds_them_in_and_saving_sends_it_in_place_of_stock() {
        let (_home, mut host) = host();
        let rules = "Create tickets only once I say go.\nName every ticket after its outcome.";
        host.skills()
            .save_rules(Skill::Coordinating, rules)
            .unwrap();
        let started: SkillState = serde_json::from_value(
            host.execute(Command::StartSkillRewrite {
                skill: Skill::Coordinating,
            })
            .unwrap(),
        )
        .unwrap();
        let stock = stock(Skill::Coordinating);
        let rewrite = started.rewrite.unwrap();
        assert_eq!(
            rewrite.text,
            format!("{stock}\n## Your own rules for Coordinating\n{rules}")
        );
        assert_eq!(
            (rewrite.base_version.as_str(), rewrite.out_of_date),
            ("v1", false)
        );
        assert_eq!(started.rules, "");
        assert!(!coordinating(&host, "additions.md").exists());
        assert_eq!(
            fs::read_to_string(coordinating(&host, "base.md")).unwrap(),
            stock
        );

        let text = "Plan first.\nCreate tickets only once I say go.";
        host.execute(Command::SaveSkillRewrite {
            skill: Skill::Coordinating,
            text: text.into(),
        })
        .unwrap();
        let instructions = host.instructions(Role::ProjectOrchestrator);
        assert_eq!(
            instructions,
            provider::instructions(Role::ProjectOrchestrator, text, "")
        );
        assert!(!instructions.contains(provider::RULES_SENTENCE));
        assert_eq!(
            changes(&host, Skill::Coordinating),
            [
                Change::SavedRules,
                Change::StartedRewrite,
                Change::SavedRewrite
            ]
        );
    }

    #[test]
    fn an_empty_rewrite_is_accepted_and_a_text_too_long_is_refused() {
        let (_home, mut host) = host();
        let save = |text: String| Command::SaveSkillRewrite {
            skill: Skill::Reviewing,
            text,
        };
        // Only a rewrite that has been started can be saved.
        assert!(host.execute(save(String::new())).is_err());
        host.skills().start_rewrite(Skill::Reviewing).unwrap();
        host.execute(save(String::new())).unwrap();
        assert_eq!(
            host.instructions(Role::Tester),
            provider::instructions(Role::Tester, "", "")
        );
        let refused = host.execute(save("x".repeat(40_000))).unwrap_err();
        assert!(refused.to_string().contains("32,000"), "{refused}");
        assert_eq!(
            host.skills().state(Skill::Reviewing).rewrite.unwrap().text,
            ""
        );
    }

    #[test]
    fn a_release_that_changes_the_stock_text_marks_a_rewrite_out_of_date_until_marked() {
        let (_home, mut host) = host();
        host.skills().start_rewrite(Skill::Coordinating).unwrap();
        host.skills()
            .save_rewrite(Skill::Coordinating, "My way")
            .unwrap();
        let stock = stock(Skill::Coordinating);
        let (heading, body) = stock.split_once('\n').unwrap();
        based_on_an_earlier_release(
            &host,
            &format!("# Coordinating — v0\nA line v1 removed.\n{body}"),
        );
        let rewrite = host.skills().state(Skill::Coordinating).rewrite.unwrap();
        assert_eq!(
            (rewrite.base_version.as_str(), rewrite.out_of_date),
            ("v0", true)
        );
        assert!(
            host.instructions(Role::ProjectOrchestrator)
                .ends_with("My way")
        );
        let lines: Vec<Line> = serde_json::from_value(
            host.execute(Command::SkillStockChanges {
                skill: Skill::Coordinating,
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            lines[..3],
            [
                Line::Removed("# Coordinating — v0".into()),
                Line::Removed("A line v1 removed.".into()),
                Line::Added(heading.into()),
            ]
        );
        assert!(
            lines[3..]
                .iter()
                .all(|line| matches!(line, Line::Unchanged(_)))
        );

        host.execute(Command::MarkSkillUpToDate {
            skill: Skill::Coordinating,
        })
        .unwrap();
        let state = host.skills().state(Skill::Coordinating);
        let rewrite = state.rewrite.unwrap();
        assert_eq!(
            (rewrite.text.as_str(), rewrite.out_of_date),
            ("My way", false)
        );
        assert_eq!(
            fs::read_to_string(coordinating(&host, "base.md")).unwrap(),
            stock
        );
        assert!(
            host.skills()
                .stock_changes(Skill::Coordinating)
                .iter()
                .all(|line| matches!(line, Line::Unchanged(_)))
        );
        assert_eq!(
            changes(&host, Skill::Coordinating).last(),
            Some(&Change::MarkedUpToDate)
        );
    }

    #[test]
    fn a_stock_change_without_a_new_version_still_marks_a_rewrite_out_of_date() {
        let (_home, host) = host();
        host.skills().start_rewrite(Skill::Coordinating).unwrap();
        let stock = stock(Skill::Coordinating);
        based_on_an_earlier_release(&host, &format!("{stock}A line the release removed.\n"));
        let rewrite = host.skills().state(Skill::Coordinating).rewrite.unwrap();
        assert_eq!(
            (rewrite.base_version.as_str(), rewrite.out_of_date),
            ("v1", true)
        );
    }

    #[test]
    fn resetting_a_rewritten_skill_removes_its_files_and_keeps_it_in_the_history() {
        let (_home, host) = host();
        host.skills().start_rewrite(Skill::Coordinating).unwrap();
        host.skills()
            .save_rewrite(Skill::Coordinating, "My way")
            .unwrap();
        based_on_an_earlier_release(&host, "# Coordinating — v0\n");
        host.skills().reset(Skill::Coordinating).unwrap();
        for file in ["additions.md", "rewrite.md", "base.md"] {
            assert!(!coordinating(&host, file).exists(), "{file}");
        }
        let state = host.skills().state(Skill::Coordinating);
        assert_eq!((state.rules.as_str(), state.rewrite), ("", None));
        assert_eq!(
            host.instructions(Role::ProjectOrchestrator),
            provider::skill(Role::ProjectOrchestrator)
        );
        let history = host.skills().history(Skill::Coordinating);
        let [.., (_, saved), (_, reset)] = &history[..] else {
            panic!("{history:?}")
        };
        assert_eq!(reset.change, Change::ResetToStock);
        let rewrite = saved.edits.rewrite.as_ref().unwrap();
        assert_eq!(rewrite.text, "My way");
        assert_eq!(rewrite.base, stock(Skill::Coordinating));
    }

    #[test]
    fn restoring_an_old_rewrite_restores_its_base_and_it_is_out_of_date_at_once() {
        let (_home, host) = host();
        host.skills().start_rewrite(Skill::Coordinating).unwrap();
        based_on_an_earlier_release(&host, "# Coordinating — v0\nOld\n");
        host.skills()
            .save_rewrite(Skill::Coordinating, "Against v0")
            .unwrap();
        host.skills().reset(Skill::Coordinating).unwrap();
        host.skills()
            .save_rules(Skill::Coordinating, "Rule")
            .unwrap();
        let (saved, _) = host.skills().history(Skill::Coordinating).remove(1);
        host.skills().restore(Skill::Coordinating, &saved).unwrap();
        let state = host.skills().state(Skill::Coordinating);
        let rewrite = state.rewrite.unwrap();
        assert_eq!(
            (
                rewrite.text.as_str(),
                rewrite.base_version.as_str(),
                rewrite.out_of_date
            ),
            ("Against v0", "v0", true)
        );
        assert!(!coordinating(&host, "additions.md").exists());
        assert!(
            host.instructions(Role::ProjectOrchestrator)
                .ends_with("Against v0")
        );
    }

    #[test]
    fn a_rewrite_does_not_start_while_the_rules_cannot_be_read() {
        let (_home, host) = host();
        host.skills()
            .save_rules(Skill::Coordinating, "Rule")
            .unwrap();
        fs::write(coordinating(&host, "additions.md"), b"\xff not text").unwrap();
        let refused = host
            .skills()
            .start_rewrite(Skill::Coordinating)
            .unwrap_err();
        assert!(
            refused
                .to_string()
                .starts_with("additions.md could not be read"),
            "{refused}"
        );
        assert_eq!(
            fs::read(coordinating(&host, "additions.md")).unwrap(),
            b"\xff not text"
        );
        assert!(!coordinating(&host, "rewrite.md").exists());
        assert_eq!(changes(&host, Skill::Coordinating), [Change::SavedRules]);
    }

    #[test]
    fn a_rewrite_whose_base_cannot_be_read_is_neither_saved_nor_marked_up_to_date() {
        let (_home, host) = host();
        host.skills().start_rewrite(Skill::Coordinating).unwrap();
        fs::write(coordinating(&host, "base.md"), b"\xff not text").unwrap();
        let skills = host.skills();
        for refused in [
            skills.save_rewrite(Skill::Coordinating, "My way"),
            skills.mark_up_to_date(Skill::Coordinating),
        ] {
            let refused = refused.unwrap_err().to_string();
            assert!(
                refused.starts_with("base.md could not be read"),
                "{refused}"
            );
        }
        assert_eq!(
            fs::read(coordinating(&host, "base.md")).unwrap(),
            b"\xff not text"
        );
        let rewrite = fs::read_to_string(coordinating(&host, "rewrite.md")).unwrap();
        assert_eq!(rewrite, stock(Skill::Coordinating));
        assert_eq!(
            changes(&host, Skill::Coordinating),
            [Change::StartedRewrite]
        );
    }

    #[test]
    fn a_rewrite_does_not_start_over_an_existing_one() {
        let (_home, host) = host();
        host.skills().start_rewrite(Skill::Coordinating).unwrap();
        host.skills()
            .save_rewrite(Skill::Coordinating, "My way")
            .unwrap();
        assert!(host.skills().start_rewrite(Skill::Coordinating).is_err());
        let rewrite = host.skills().state(Skill::Coordinating).rewrite.unwrap();
        assert_eq!(rewrite.text, "My way");
        assert_eq!(
            changes(&host, Skill::Coordinating),
            [Change::StartedRewrite, Change::SavedRewrite]
        );
    }

    #[test]
    fn a_base_that_cannot_be_removed_does_not_stop_a_reset() {
        let (_home, host) = host();
        host.skills().start_rewrite(Skill::Coordinating).unwrap();
        fs::remove_file(coordinating(&host, "base.md")).unwrap();
        fs::create_dir(coordinating(&host, "base.md")).unwrap();
        host.skills().reset(Skill::Coordinating).unwrap();
        let state = host.skills().state(Skill::Coordinating);
        assert_eq!((state.rewrite, state.unreadable), (None, None));
        assert_eq!(
            changes(&host, Skill::Coordinating),
            [Change::StartedRewrite, Change::ResetToStock]
        );
        assert_eq!(coordinating_files(&host), ["base.md", "history"]);
        // A base that cannot be written leaves the rules in effect.
        host.skills()
            .save_rules(Skill::Coordinating, "Rule")
            .unwrap();
        assert!(host.skills().start_rewrite(Skill::Coordinating).is_err());
        let state = host.skills().state(Skill::Coordinating);
        assert_eq!((state.rules.as_str(), state.rewrite), ("Rule", None));
    }

    /// The names in Coordinating's folder, so a failed change is seen to leave nothing behind.
    fn coordinating_files(host: &Host) -> Vec<String> {
        let files = fs::read_dir(host.home.join("shared/skills/coordinating")).unwrap();
        let mut names: Vec<_> = (files.map(|file| file.unwrap().file_name()))
            .map(|name| name.into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    /// Runs `act` while `file` is locked, as `chflags uchg` locks it on macOS.
    fn while_locked<T>(file: &Path, act: impl FnOnce() -> T) -> T {
        let chflags = |flag: &str| {
            let status = std::process::Command::new("chflags")
                .arg(flag)
                .arg(file)
                .status()
                .unwrap();
            assert!(status.success());
        };
        chflags("uchg");
        let result = act();
        chflags("nouchg");
        result
    }

    /// Runs `act` while no history entry can be written.
    fn while_the_history_is_read_only<T>(host: &Host, act: impl FnOnce() -> T) -> T {
        use std::os::unix::fs::PermissionsExt as _;
        let history = coordinating(host, "history");
        fs::set_permissions(&history, fs::Permissions::from_mode(0o555)).unwrap();
        let result = act();
        fs::set_permissions(&history, fs::Permissions::from_mode(0o755)).unwrap();
        result
    }

    #[test]
    fn marking_up_to_date_with_a_locked_rewrite_changes_nothing() {
        let (_home, host) = host();
        host.skills().start_rewrite(Skill::Coordinating).unwrap();
        based_on_an_earlier_release(&host, "# Coordinating — v0\n");
        let marked = while_locked(&coordinating(&host, "rewrite.md"), || {
            host.skills().mark_up_to_date(Skill::Coordinating)
        });
        assert!(marked.is_err());
        let base = fs::read_to_string(coordinating(&host, "base.md")).unwrap();
        assert_eq!(base, "# Coordinating — v0\n");
        let rewrite = host.skills().state(Skill::Coordinating).rewrite.unwrap();
        assert!(rewrite.out_of_date);
        assert_eq!(
            changes(&host, Skill::Coordinating),
            [Change::StartedRewrite]
        );
        assert_eq!(
            coordinating_files(&host),
            ["base.md", "history", "rewrite.md"]
        );
    }

    #[test]
    fn a_rewrite_save_that_cannot_be_recorded_puts_the_rewrite_back() {
        let (_home, host) = host();
        host.skills().start_rewrite(Skill::Coordinating).unwrap();
        let record = || -> Vec<(String, Option<Vec<u8>>)> {
            let mut rows = host
                .db
                .prepare("SELECT path,written FROM sync_record ORDER BY path")
                .unwrap();
            let rows = rows.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
            rows.collect::<rusqlite::Result<_>>().unwrap()
        };
        let recorded = record();
        host.shared_written.take();
        let saved = while_the_history_is_read_only(&host, || {
            host.skills().save_rewrite(Skill::Coordinating, "My way")
        });
        assert!(saved.is_err());
        // Sync's record notes nothing of a change that was put back.
        assert_eq!(record(), recorded);
        assert!(!host.shared_written.get());
        let rewrite = host.skills().state(Skill::Coordinating).rewrite.unwrap();
        assert_eq!(rewrite.text, stock(Skill::Coordinating));
        assert_eq!(
            changes(&host, Skill::Coordinating),
            [Change::StartedRewrite]
        );
        assert_eq!(
            coordinating_files(&host),
            ["base.md", "history", "rewrite.md"]
        );
    }

    #[test]
    fn a_reset_that_cannot_be_recorded_keeps_the_rewrite() {
        let (_home, host) = host();
        host.skills().start_rewrite(Skill::Coordinating).unwrap();
        host.skills()
            .save_rewrite(Skill::Coordinating, "My way")
            .unwrap();
        let reset =
            while_the_history_is_read_only(&host, || host.skills().reset(Skill::Coordinating));
        assert!(reset.is_err());
        let state = host.skills().state(Skill::Coordinating);
        assert_eq!(state.rewrite.unwrap().text, "My way");
        assert_eq!(
            changes(&host, Skill::Coordinating),
            [Change::StartedRewrite, Change::SavedRewrite]
        );
        assert_eq!(
            coordinating_files(&host),
            ["base.md", "history", "rewrite.md"]
        );
    }

    #[test]
    fn rules_are_not_saved_while_the_rewrite_cannot_be_read() {
        let (_home, host) = host();
        host.skills().start_rewrite(Skill::Coordinating).unwrap();
        fs::write(coordinating(&host, "rewrite.md"), b"\xff not text").unwrap();
        assert_eq!(host.skills().state(Skill::Coordinating).rewrite, None);
        let refused = host
            .skills()
            .save_rules(Skill::Coordinating, "Rule")
            .unwrap_err()
            .to_string();
        assert!(
            refused.starts_with("rewrite.md could not be read"),
            "{refused}"
        );
        assert_eq!(
            coordinating_files(&host),
            ["base.md", "history", "rewrite.md"]
        );
        assert_eq!(
            changes(&host, Skill::Coordinating),
            [Change::StartedRewrite]
        );
    }

    #[test]
    fn a_file_that_cannot_be_put_back_is_kept_beside_it() {
        let folder = tempfile::tempdir().unwrap();
        let rewrite = folder.path().join("rewrite.md");
        fs::write(&rewrite, "My way").unwrap();
        let moved = Moved::aside(folder.path(), rewrite.clone()).unwrap();
        fs::create_dir(&rewrite).unwrap();
        let kept = moved.put_back().unwrap();
        assert_eq!(fs::read_to_string(&kept).unwrap(), "My way");
        let name = kept.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("rewrite.md.kept-"), "{name}");
    }

    #[test]
    fn an_entry_written_before_rewrites_reads_as_rules() {
        let entry: Entry = serde_json::from_str(
            r#"{"change":{"kind":"saved_rules"},"stock_version":"v1","rules":"Rule"}"#,
        )
        .unwrap();
        assert_eq!(
            entry.edits,
            Edits {
                rules: "Rule".into(),
                rewrite: None
            }
        );
    }
}
