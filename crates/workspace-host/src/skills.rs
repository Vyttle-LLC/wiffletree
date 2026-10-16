//! The human's own rules for each skill, as plain files in `<data folder>/skills`, one folder per
//! skill with a history file for every change. Only the human's commands write here; no agent
//! tool reaches it. A missing file means no rules, so a fresh install or an update needs nothing.
use crate::*;
use serde::Deserialize;
use std::io::{ErrorKind, Write as _};

const RULES: &str = "additions.md";
const HISTORY: &str = "history";

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
/// What a history entry records: the skill's whole state after a change, and the change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub change: Change,
    pub stock_version: String,
    pub rules: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    SavedRules,
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
pub struct SkillStore {
    root: PathBuf,
}
impl SkillStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    fn folder(&self, skill: Skill) -> PathBuf {
        self.root.join(skill.folder())
    }
    /// A file that cannot be read counts as missing, and says why.
    pub fn state(&self, skill: Skill) -> SkillState {
        let (rules, unreadable) = match fs::read_to_string(self.folder(skill).join(RULES)) {
            Ok(rules) => (rules, None),
            Err(error) if error.kind() == ErrorKind::NotFound => (String::new(), None),
            Err(error) => (
                String::new(),
                Some(format!("{RULES} could not be read: {error}")),
            ),
        };
        SkillState {
            skill,
            stock: stock(skill).into(),
            stock_version: stock_version(skill).into(),
            rules,
            unreadable,
        }
    }
    pub fn save_rules(&self, skill: Skill, rules: &str) -> Result<()> {
        ensure!(
            rules.chars().count() <= MAX_SKILL_TEXT,
            "Your rules can be at most 32,000 characters"
        );
        self.write_rules(skill, rules)?;
        self.record(skill, Change::SavedRules, rules)
    }
    pub fn reset(&self, skill: Skill) -> Result<()> {
        self.write_rules(skill, "")?;
        self.record(skill, Change::ResetToStock, "")
    }
    /// Writes the entry's state back as a new entry; the history keeps every entry.
    pub fn restore(&self, skill: Skill, entry: &str) -> Result<()> {
        let (_, restored) = self
            .history(skill)
            .into_iter()
            .find(|(name, _)| name == entry)
            .context("That history entry does not exist")?;
        self.write_rules(skill, &restored.rules)?;
        let change = Change::Restored {
            entry: entry.into(),
        };
        self.record(skill, change, &restored.rules)
    }
    /// At start, an entry for each skill whose latest entry recorded another stock version.
    /// A skill with no history gets none, and one whose rules cannot be read waits for a start
    /// when they can, so no entry records rules it could not see.
    pub fn record_new_stock_versions(&self) -> Result<()> {
        for skill in Skill::ALL {
            let Some((_, latest)) = self.history(skill).pop() else {
                continue;
            };
            let state = self.state(skill);
            if latest.stock_version != stock_version(skill) && state.unreadable.is_none() {
                let change = Change::NewStockVersion {
                    previous: latest.stock_version,
                };
                self.record(skill, change, &state.rules)?;
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
    /// Atomically, like `save_settings`; no rules means no file.
    fn write_rules(&self, skill: Skill, rules: &str) -> Result<()> {
        let folder = self.folder(skill);
        if rules.is_empty() {
            return match fs::remove_file(folder.join(RULES)) {
                Err(error) if error.kind() != ErrorKind::NotFound => Err(error.into()),
                _ => Ok(()),
            };
        }
        fs::create_dir_all(&folder)?;
        let mut staged = tempfile::NamedTempFile::new_in(&folder)?;
        staged.write_all(rules.as_bytes())?;
        staged.persist(folder.join(RULES))?;
        Ok(())
    }
    /// One file per entry, named by its time, never replacing another.
    fn record(&self, skill: Skill, change: Change, rules: &str) -> Result<()> {
        let folder = self.folder(skill).join(HISTORY);
        fs::create_dir_all(&folder)?;
        let entry = Entry {
            change,
            stock_version: stock_version(skill).into(),
            rules: rules.into(),
        };
        let mut staged = tempfile::NamedTempFile::new_in(&folder)?;
        serde_json::to_writer_pretty(&mut staged, &entry)?;
        let mut at = chrono::Utc::now();
        loop {
            let name = format!("{}.json", at.format("%Y-%m-%dT%H-%M-%S%.3fZ"));
            match staged.persist_noclobber(folder.join(name)) {
                Ok(_) => return Ok(()),
                Err(error) if error.error.kind() == ErrorKind::AlreadyExists => {
                    staged = error.file;
                    at += chrono::TimeDelta::milliseconds(1);
                }
                Err(error) => return Err(error.error.into()),
            }
        }
    }
}
/// The store of the host whose data folder is `home`.
pub fn store(home: &Path) -> SkillStore {
    SkillStore::new(home.join("skills"))
}
impl Host {
    pub fn skills(&self) -> SkillStore {
        store(&self.home)
    }
    /// What a role's next turn is sent, read from the store each time.
    pub fn instructions(&self, role: Role) -> String {
        self.skills().instructions(role)
    }
    pub(crate) fn skill_states(&self) -> Vec<SkillState> {
        let store = self.skills();
        Skill::ALL.map(|skill| store.state(skill)).into()
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
        host.home.join("skills/implementing/additions.md")
    }

    #[test]
    fn a_fresh_install_is_at_stock_and_writes_nothing() {
        let (_home, host) = host();
        assert!(!host.home.join("skills").exists());
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
            rules: rules.into(),
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
        let folder = home.path().join("skills/implementing");
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
        assert_eq!(entry.rules, rules);

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
        let history = host.home.join("skills/implementing/history");
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
        let path =
            (host.home.join("skills").join(skill.folder())).join(format!("history/{name}.json"));
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
            (latest.stock_version.as_str(), latest.rules.as_str()),
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
        let history = home.path().join("skills/reviewing/history");
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
        assert_eq!(latest.rules, "Read the tests first");
        // The new stock text is sent with the same rules, and nothing else gains an entry.
        let reviewing = host.skills().state(Skill::Reviewing);
        assert_eq!(
            host.instructions(Role::Tester),
            provider::instructions(Role::Tester, stock(Skill::Reviewing), &reviewing.rules)
        );
        assert!(!host.home.join("skills/implementing").exists());
        drop(host);
        let host = Host::open(home.path()).unwrap();
        assert_eq!(changes(&host, Skill::Reviewing).len(), 2);
    }
}
