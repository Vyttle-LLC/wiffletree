//! The settings shared with every machine, one file each in `<data folder>/shared/`, with the
//! skills beside them. A missing file means that setting's default.
use crate::*;
use serde::Deserialize;
use serde_json::Map;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::Write as _;
use std::sync::{Arc, Mutex};

pub(crate) const FOLDER: &str = "shared";
pub(crate) const FORMAT: &str = "wiffletree.json";
pub(crate) const REVIEW: &str = "settings/review.json";
pub(crate) const GUIDE: &str = "settings/model-guide.md";
pub(crate) const CHECK_INS: &str = "settings/check-ins.json";
pub(crate) const KEEPER: &str = "settings/pr-keeper.json";

/// The check-in thresholds as `settings/check-ins.json` holds them.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct CheckIns {
    checkin_child_turns: u32,
    checkin_human_turns: u32,
}
impl Default for CheckIns {
    fn default() -> Self {
        let defaults = HostSettings::new(String::new());
        Self {
            checkin_child_turns: defaults.checkin_child_turns,
            checkin_human_turns: defaults.checkin_human_turns,
        }
    }
}

/// The host's writes to the shared folder, each under a key naming a file or, ending in `/`,
/// a folder, so a sync pass running off the host thread leaves alone what was written while it
/// ran. The key `""` names the whole folder.
#[derive(Default)]
pub(crate) struct Writes {
    ended: u64,
    running: BTreeMap<String, usize>,
    ended_at: BTreeMap<String, u64>,
}
pub(crate) type SharedWrites = Arc<Mutex<Writes>>;
impl Writes {
    /// How many writes have ended, which a pass compares against.
    pub(crate) fn ended(&self) -> u64 {
        self.ended
    }
    /// Whether a write under a key covering `path` runs now or ended after `since`.
    pub(crate) fn touched(&self, path: &str, since: u64) -> bool {
        self.running
            .keys()
            .any(|key| path.starts_with(key.as_str()))
            || (self.ended_at.iter()).any(|(key, &at)| at > since && path.starts_with(key.as_str()))
    }
}
/// A write to the shared folder, running until dropped.
pub(crate) struct Writing {
    writes: SharedWrites,
    key: String,
}
impl Writing {
    pub(crate) fn start(writes: &SharedWrites, key: &str) -> Self {
        *lock(writes).running.entry(key.into()).or_default() += 1;
        Self {
            writes: writes.clone(),
            key: key.into(),
        }
    }
}
impl Drop for Writing {
    fn drop(&mut self) {
        let mut writes = lock(&self.writes);
        writes.ended += 1;
        let ended = writes.ended;
        writes.ended_at.insert(self.key.clone(), ended);
        if let Some(running) = writes.running.get_mut(&self.key) {
            *running -= 1;
            if *running == 0 {
                writes.running.remove(&self.key);
            }
        }
    }
}
pub(crate) fn lock(writes: &SharedWrites) -> std::sync::MutexGuard<'_, Writes> {
    writes
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
/// Refuses a shared file this release would refuse to save, so sync neither applies nor
/// pushes it.
pub(crate) fn check(path: &str, content: &[u8]) -> Result<()> {
    let text = std::str::from_utf8(content).map_err(|_| anyhow::anyhow!("it is not UTF-8 text"))?;
    match path {
        REVIEW => serde_json::from_str::<VerificationSettings>(text)?.validate(),
        CHECK_INS => {
            let check_ins: CheckIns = serde_json::from_str(text)?;
            let (child, human) = (check_ins.checkin_child_turns, check_ins.checkin_human_turns);
            HostSettings::validate_check_ins(child, human)
        }
        KEEPER => serde_json::from_str::<KeeperSettings>(text)?.validate(),
        FORMAT => {
            let format: Value = serde_json::from_str(text)?;
            ensure!(
                format["format"] == 1,
                "it names a format this release does not know"
            );
            Ok(())
        }
        GUIDE => {
            ensure!(text.len() <= MAX_GUIDE_BYTES, "the guide is over 32 KB");
            Ok(())
        }
        _ if skills::is_edited_text(path) => skills::check_text("The text", text),
        _ => Ok(()),
    }
}
/// Notes each write the host makes to the shared folder in sync's record, and that one was made.
#[derive(Clone)]
pub(crate) struct Recorder<'a> {
    db: &'a Connection,
    root: PathBuf,
    wrote: &'a Cell<bool>,
    writes: &'a SharedWrites,
}
impl Recorder<'_> {
    pub(crate) fn writing(&self, key: &str) -> Writing {
        Writing::start(self.writes, key)
    }
    /// `None` for a file removed.
    pub(crate) fn wrote(&self, path: &Path, content: Option<&[u8]>) -> Result<()> {
        let file = path.strip_prefix(&self.root)?.to_str();
        self.db.execute(
            "INSERT INTO sync_record(path,written) VALUES (?1,?2) ON CONFLICT(path) DO UPDATE SET written=excluded.written",
            params![file.context("A shared file's path is not UTF-8")?, content],
        )?;
        self.wrote.set(true);
        Ok(())
    }
}

pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let folder = path.parent().context("A shared file is in a folder")?;
    fs::create_dir_all(folder)?;
    let mut staged = tempfile::NamedTempFile::new_in(folder)?;
    staged.write_all(bytes)?;
    staged.persist(path)?;
    Ok(())
}

pub(crate) struct SharedStore<'a> {
    root: PathBuf,
    recorder: Option<Recorder<'a>>,
}
impl SharedStore<'_> {
    pub(crate) fn at(root: PathBuf) -> Self {
        Self {
            root,
            recorder: None,
        }
    }
    /// The shared folder of the data folder `home`.
    pub(crate) fn of(home: &Path) -> Self {
        Self::at(home.join(FOLDER))
    }
    /// Whether the folder names its format, as a set-up settings repository does.
    pub(crate) fn holds_settings(&self) -> bool {
        self.exists(FORMAT)
    }
    pub(crate) fn skills_folder(&self) -> PathBuf {
        self.root.join("skills")
    }
    pub(crate) fn review(&self) -> VerificationSettings {
        self.read_json(REVIEW).unwrap_or_default()
    }
    pub(crate) fn set_review(&self, review: &VerificationSettings) -> Result<()> {
        self.write_json(REVIEW, review)
    }
    /// The agent and coordinator thresholds.
    pub(crate) fn check_ins(&self) -> (u32, u32) {
        let check_ins: CheckIns = self.read_json(CHECK_INS).unwrap_or_default();
        (check_ins.checkin_child_turns, check_ins.checkin_human_turns)
    }
    pub(crate) fn set_check_ins(&self, child_turns: u32, human_turns: u32) -> Result<()> {
        let check_ins = CheckIns {
            checkin_child_turns: child_turns,
            checkin_human_turns: human_turns,
        };
        self.write_json(CHECK_INS, &check_ins)
    }
    pub(crate) fn keeper(&self) -> KeeperSettings {
        self.read_json(KEEPER).unwrap_or_default()
    }
    pub(crate) fn set_keeper(&self, keeper: &KeeperSettings) -> Result<()> {
        self.write_json(KEEPER, keeper)
    }
    /// `None` when no guide was saved; an empty guide is a saved one.
    pub(crate) fn guide(&self) -> Option<String> {
        fs::read_to_string(self.root.join(GUIDE)).ok()
    }
    pub(crate) fn set_guide(&self, guide: &str) -> Result<()> {
        self.write(GUIDE, guide.as_bytes())
    }
    fn exists(&self, file: &str) -> bool {
        self.root.join(file).exists()
    }
    fn read_json<T: DeserializeOwned>(&self, file: &str) -> Option<T> {
        serde_json::from_slice(&fs::read(self.root.join(file)).ok()?).ok()
    }
    /// Pretty, with sorted keys, keeping the fields this release does not know. A known field
    /// left out when unset, such as the keeper's review level, does not keep its old value.
    fn write_json<T: Serialize + DeserializeOwned>(&self, file: &str, value: &T) -> Result<()> {
        let mut saved: Map<String, Value> = self.read_json(file).unwrap_or_default();
        let known = serde_json::from_value::<T>(Value::Object(saved.clone()))
            .and_then(serde_json::to_value);
        if let Ok(Value::Object(known)) = known {
            saved.retain(|field, _| !known.contains_key(field));
        }
        if let Value::Object(fields) = serde_json::to_value(value)? {
            saved.extend(fields);
        }
        // Another crate in the workspace may turn on serde_json's insertion order.
        let mut saved = Value::Object(saved);
        saved.sort_all_objects();
        let mut bytes = serde_json::to_vec_pretty(&saved)?;
        bytes.push(b'\n');
        self.write(file, &bytes)
    }
    fn write(&self, file: &str, bytes: &[u8]) -> Result<()> {
        let path = self.root.join(file);
        let _writing = self.recorder.as_ref().map(|r| r.writing(file));
        write_atomically(&path, bytes)?;
        self.recorded(&path, Some(bytes))
    }
    fn remove(&self, file: &str) -> Result<()> {
        let path = self.root.join(file);
        let _writing = self.recorder.as_ref().map(|r| r.writing(file));
        fs::remove_file(&path)?;
        self.recorded(&path, None)
    }
    fn recorded(&self, path: &Path, content: Option<&[u8]>) -> Result<()> {
        match &self.recorder {
            Some(recorder) => recorder.wrote(path, content),
            None => Ok(()),
        }
    }
}
impl Host {
    pub(crate) fn shared(&self) -> SharedStore<'_> {
        SharedStore {
            recorder: Some(self.recorder()),
            ..SharedStore::of(&self.home)
        }
    }
    pub(crate) fn recorder(&self) -> Recorder<'_> {
        Recorder {
            db: &self.db,
            root: self.home.join(FOLDER),
            wrote: &self.shared_written,
            writes: &self.shared_writes,
        }
    }
    /// Runs `save`; when it fails, the shared `file` is put back as it was before.
    pub(crate) fn put_back_on_error<T>(
        &mut self,
        file: &str,
        save: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let previous = fs::read(self.shared().root.join(file)).ok();
        save(self).inspect_err(|_| {
            let shared = self.shared();
            let _ = match &previous {
                Some(bytes) => shared.write(file, bytes),
                None => shared.remove(file),
            };
        })
    }
    /// Copies each shared setting that has no file yet from where it was saved before the
    /// shared folder, exactly as saved, and moves the skills folder in. `wiffletree.json` is
    /// written last, so running it again finishes a move that was cut short, and once it exists
    /// nothing is copied.
    pub(crate) fn move_into_shared_folder(&self) -> Result<()> {
        let shared = self.shared();
        if shared.exists(FORMAT) {
            return Ok(());
        }
        let saved: Map<String, Value> = fs::read(self.home.join("settings.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let check_ins: Map<String, Value> = ["checkin_child_turns", "checkin_human_turns"]
            .into_iter()
            .filter_map(|field| Some((field.to_owned(), saved.get(field)?.clone())))
            .collect();
        let copies = [
            (REVIEW, saved.get("verification").cloned()),
            (
                CHECK_INS,
                (!check_ins.is_empty()).then_some(Value::Object(check_ins)),
            ),
            (KEEPER, saved.get("keeper").cloned()),
        ];
        for (file, value) in copies {
            if let Some(value) = value.filter(|_| !shared.exists(file)) {
                shared.write_json(file, &value)?;
            }
        }
        if !shared.exists(GUIDE)
            && let Some(selection) = self.saved_selection()?
        {
            shared.set_guide(&selection.guide)?;
        }
        let skills = self.home.join("skills");
        if skills.exists() && !shared.skills_folder().exists() {
            fs::create_dir_all(&shared.root)?;
            fs::rename(skills, shared.skills_folder())?;
        }
        shared.write_json(FORMAT, &json!({"format": 1}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(home: &Path, file: &str) -> Option<String> {
        fs::read_to_string(home.join(FOLDER).join(file)).ok()
    }

    #[test]
    fn a_fresh_install_has_no_shared_setting_and_every_default() {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        for file in [REVIEW, GUIDE, CHECK_INS, KEEPER] {
            assert_eq!(read(home.path(), file), None, "{file}");
        }
        assert_eq!(
            read(home.path(), FORMAT).unwrap(),
            "{\n  \"format\": 1\n}\n"
        );
        let settings = host.settings();
        let defaults = HostSettings::new(settings.workspaces_dir.clone());
        assert_eq!(settings, defaults);
        assert_eq!(host.model_selection().unwrap(), ModelSelection::default());
        for kept_free in ["logs", "tickets"] {
            assert!(!home.path().join(FOLDER).join(kept_free).exists());
        }
    }

    #[test]
    fn an_update_copies_each_setting_exactly_as_saved_and_leaves_the_old_values() {
        let home = tempfile::tempdir().unwrap();
        let settings_file = home.path().join("settings.json");
        let saved = json!({
            "workspaces_dir": "/tmp/w",
            "verification": {"verifiers": [{"role": "tester", "focus": "Tests"}], "max_rounds": 2},
            "checkin_child_turns": 10,
            "checkin_human_turns": 300,
            "keeper": {"rebase": true},
            "allowed_owners": ["Sage-Social"]
        })
        .to_string();
        fs::write(&settings_file, &saved).unwrap();
        let host = Host::open(home.path()).unwrap();
        let file = |name| serde_json::from_str::<Value>(&read(home.path(), name).unwrap()).unwrap();
        assert_eq!(
            file(REVIEW),
            json!({"verifiers": [{"role": "tester", "focus": "Tests"}], "max_rounds": 2})
        );
        assert_eq!(
            file(CHECK_INS),
            json!({"checkin_child_turns": 10, "checkin_human_turns": 300})
        );
        assert_eq!(file(KEEPER), json!({"rebase": true}));
        let settings = host.settings();
        assert_eq!(
            (settings.checkin_child_turns, settings.checkin_human_turns),
            (10, 300)
        );
        assert!(settings.keeper.rebase);
        let levels: Vec<&str> = settings
            .verification
            .levels
            .iter()
            .map(|l| &*l.name)
            .collect();
        assert_eq!(levels, ["Full"]);
        assert_eq!(settings.allowed_owners, ["Sage-Social"]);
        // Saving a shared setting writes its file, and settings.json keeps the old values.
        let mut host = host;
        host.set_check_ins(50, 500).unwrap();
        host.set_owner_allowed("MHohlios", true).unwrap();
        let raw: Value = serde_json::from_slice(&fs::read(&settings_file).unwrap()).unwrap();
        assert_eq!(
            (&raw["checkin_child_turns"], &raw["checkin_human_turns"]),
            (&json!(10), &json!(300))
        );
        assert_eq!(raw["verification"]["levels"], Value::Null);
        assert_eq!(host.settings().checkin_child_turns, 50);
    }

    #[test]
    fn the_guide_moves_exactly_as_saved_and_the_database_keeps_it() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        let mut selection = host.model_selection().unwrap();
        selection.guide = "  Use *opus* for migrations.\n\n".into();
        host.set_model_selection(&selection).unwrap();
        // A store from before the shared folder: the guide is only in the database.
        let row = |host: &Host| host.saved_selection().unwrap().unwrap().guide;
        host.db
            .execute(
                "UPDATE model_selection SET data=json_set(data,'$.guide',?1)",
                [&selection.guide],
            )
            .unwrap();
        drop(host);
        fs::remove_dir_all(home.path().join(FOLDER)).unwrap();
        let host = Host::open(home.path()).unwrap();
        assert_eq!(read(home.path(), GUIDE).unwrap(), selection.guide);
        assert_eq!(host.model_selection().unwrap().guide, selection.guide);
        assert_eq!(row(&host), selection.guide);
    }

    #[test]
    fn a_guide_from_another_machine_shows_before_any_setup_and_the_first_save_keeps_it() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        let guide = "Opus for anything touching auth.\n";
        host.shared().set_guide(guide).unwrap();
        let shown = host.model_selection().unwrap();
        assert!(!shown.is_saved());
        assert_eq!(shown.guide, guide);
        let signed_in = ProviderStatus {
            installed: true,
            signed_in: true,
            plan: None,
        };
        let mut setup = suggest(&PROVIDERS.map(|p| (p, signed_in.clone())).into());
        setup.selection.guide = shown.guide;
        host.set_setup(&setup.selection, Some(setup.verification))
            .unwrap();
        assert_eq!(read(home.path(), GUIDE).unwrap(), guide);
    }

    #[test]
    fn an_interrupted_move_writes_the_missing_files_and_leaves_the_written_one() {
        let home = tempfile::tempdir().unwrap();
        fs::write(
            home.path().join("settings.json"),
            r#"{"workspaces_dir":"/tmp/w","checkin_child_turns":10,"keeper":{"rebase":true},
                "verification":{"verifiers":[{"role":"tester","focus":"Tests"}],"max_rounds":2}}"#,
        )
        .unwrap();
        let review = home.path().join(FOLDER).join(REVIEW);
        fs::create_dir_all(review.parent().unwrap()).unwrap();
        let written = r#"{"verifiers":[{"role":"tester","focus":"Tests"}],"max_rounds":3}"#;
        fs::write(&review, written).unwrap();
        let host = Host::open(home.path()).unwrap();
        assert_eq!(fs::read_to_string(&review).unwrap(), written);
        assert_eq!(host.settings().verification.max_rounds, 3);
        assert_eq!(
            read(home.path(), CHECK_INS).unwrap(),
            "{\n  \"checkin_child_turns\": 10\n}\n"
        );
        assert_eq!(host.settings().checkin_human_turns, 100);
        assert!(host.settings().keeper.rebase);
    }

    #[test]
    fn once_the_move_is_done_a_missing_file_is_never_refilled_from_old_values() {
        let home = tempfile::tempdir().unwrap();
        fs::write(
            home.path().join("settings.json"),
            r#"{"workspaces_dir":"/tmp/w","checkin_child_turns":10}"#,
        )
        .unwrap();
        drop(Host::open(home.path()).unwrap());
        fs::remove_file(home.path().join(FOLDER).join(CHECK_INS)).unwrap();
        let host = Host::open(home.path()).unwrap();
        assert_eq!(read(home.path(), CHECK_INS), None);
        assert_eq!(host.settings().checkin_child_turns, 25);
    }

    #[test]
    fn an_empty_guide_is_a_saved_guide() {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        assert!(!host.snapshot().unwrap().guide_saved);
        host.shared().set_guide("").unwrap();
        let snapshot = host.snapshot().unwrap();
        assert!(snapshot.guide_saved);
        assert_eq!(snapshot.model_selection.guide, "");
    }

    #[test]
    fn skills_saved_before_the_shared_folder_move_in_unchanged() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        host.execute(Command::SaveSkillRules {
            skill: Skill::Implementing,
            rules: "Run the linter.".into(),
        })
        .unwrap();
        let history = host.skills().history(Skill::Implementing);
        drop(host);
        // Put the skills where the release before the shared folder kept them, with no shared folder.
        let old = home.path().join("skills");
        fs::rename(home.path().join(FOLDER).join("skills"), &old).unwrap();
        fs::remove_dir_all(home.path().join(FOLDER)).unwrap();
        let host = Host::open(home.path()).unwrap();
        assert!(!old.exists());
        assert_eq!(
            read(home.path(), "skills/implementing/additions.md").unwrap(),
            "Run the linter."
        );
        assert_eq!(host.skills().history(Skill::Implementing), history);
        assert!(
            host.instructions(Role::Implementer)
                .contains("Run the linter.")
        );
    }

    #[test]
    fn a_save_keeps_fields_this_release_does_not_know_and_sorts_keys() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        let keeper = home.path().join(FOLDER).join(KEEPER);
        fs::create_dir_all(keeper.parent().unwrap()).unwrap();
        fs::write(&keeper, r#"{"future_trigger": true, "rebase": false}"#).unwrap();
        let on = KeeperSettings {
            rebase: true,
            ..KeeperSettings::default()
        };
        host.set_keeper(on.clone()).unwrap();
        let text = fs::read_to_string(&keeper).unwrap();
        let raw: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(raw["future_trigger"], true);
        assert_eq!(host.settings().keeper, on);
        // A field left out when unset is removed with its old value; the unknown one stays.
        let leveled = KeeperSettings {
            review_level: Some("Light".into()),
            ..on.clone()
        };
        host.shared().set_keeper(&leveled).unwrap();
        host.shared().set_keeper(&on).unwrap();
        assert_eq!(host.settings().keeper, on);
        let raw: Value = serde_json::from_slice(&fs::read(&keeper).unwrap()).unwrap();
        assert_eq!(raw["future_trigger"], true);
        let keys: Vec<&str> = text
            .lines()
            .filter_map(|line| line.trim().strip_prefix('"')?.split('"').next())
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn a_hand_edited_threshold_is_used() {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        let file = home.path().join(FOLDER).join(CHECK_INS);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, r#"{"checkin_human_turns": 300}"#).unwrap();
        let settings = host.settings();
        assert_eq!(
            (settings.checkin_child_turns, settings.checkin_human_turns),
            (25, 300)
        );
    }
}
