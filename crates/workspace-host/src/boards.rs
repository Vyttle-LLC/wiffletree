//! Boards (stage 4): every output a thread or job saved, listed from this Mac's private board
//! store and each Space's storage, with its versions from Git history. Listing and reading
//! history run off the host thread. Only Share to Space…, which the user asks for, and a job's
//! outputs write an output into a Space's storage.
use crate::*;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use workspace_core::board::{
    BoardOutput, BoardVersion, FileKind, Keeping, LATEST, plain_file_name,
};
use workspace_core::job::{Cron, JobKey, Recipe, Sharing};

/// The folder of a Space's storage, beside its jobs' folders, that holds outputs the user
/// shared into it. Job folders are job ids, which hold no underscore.
const SHARED_OUTPUTS: &str = "_shared";
const MAX_VERSIONS: usize = 100;
const PREVIEW_CHARS: usize = 300;
/// The part of an output a preview reads.
const PREVIEW_BYTES: u64 = 16 * 1024;

/// One repository folder that holds outputs: `<slug>.json` files beside `<slug>/` folders for
/// outputs saved from threads or shared, and in `jobs`, a folder per job holding the same.
struct Shelf {
    repository: PathBuf,
    space: SpaceId,
    keeping: Keeping,
    loose: String,
    jobs: String,
}

/// What Boards reads, taken from the store on the host thread and read off it.
pub(crate) struct Shelves {
    shelves: Vec<Shelf>,
    recipes: Vec<Recipe>,
    /// The missed runs each catch-up run caught up on, by run.
    catch_ups: HashMap<String, u32>,
    /// The outputs waiting for approval, by run and title.
    waiting: HashSet<(String, String)>,
    /// Where an output's earlier versions are copied for the viewer.
    versions: PathBuf,
    /// This Mac's private board store.
    store: PathBuf,
    now: i64,
}

/// An output found on a shelf, before it is described.
struct Found<'a> {
    shelf: &'a Shelf,
    /// The output's folder, relative to its repository.
    folder: String,
    job: Option<JobKey>,
    about: Value,
}
impl Found<'_> {
    fn id(&self) -> String {
        let named = format!("{}\0{}", self.shelf.repository.display(), self.folder);
        let digest = Sha256::digest(named.as_bytes());
        digest[..8].iter().map(|b| format!("{b:02x}")).collect()
    }
    fn metadata(&self) -> String {
        format!("{}.json", self.folder)
    }
}

impl Shelves {
    fn found(&self) -> Vec<Found<'_>> {
        let mut found = vec![];
        for shelf in &self.shelves {
            found.extend(outputs_in(shelf, &shelf.loose, None));
            let Ok(jobs) = fs::read_dir(shelf.repository.join(&shelf.jobs)) else {
                continue;
            };
            for job in jobs.flatten() {
                let name = job.file_name().to_string_lossy().into_owned();
                // A link to a folder could name any folder on this Mac.
                let folder = job.file_type().is_ok_and(|kind| kind.is_dir());
                if name.starts_with(['_', '.']) || !folder {
                    continue;
                }
                let key = JobKey::recipe(&shelf.space, &name);
                found.extend(outputs_in(
                    shelf,
                    &format!("{}/{name}", shelf.jobs),
                    Some(key),
                ));
            }
        }
        found
    }
    fn find(&self, id: &str) -> Result<Found<'_>> {
        (self.found().into_iter())
            .find(|found| found.id() == id)
            .context("That output is no longer on this Mac")
    }
    /// Every output, newest first.
    pub(crate) fn list(&self) -> Vec<BoardOutput> {
        let mut outputs: Vec<BoardOutput> = (self.found().iter())
            .filter_map(|found| self.describe(found))
            .collect();
        outputs.sort_by_key(|output| std::cmp::Reverse(output.updated_at));
        outputs
    }
    fn describe(&self, found: &Found) -> Option<BoardOutput> {
        let shelf = found.shelf;
        let folder = shelf.repository.join(&found.folder);
        let file = found.about["file"].as_str()?.to_owned();
        let path = folder.join(&file);
        // A link, to the folder or its file, could point anywhere on this Mac.
        let real = |path: &Path, file: bool| {
            fs::symlink_metadata(path).is_ok_and(|m| if file { m.is_file() } else { m.is_dir() })
        };
        if !plain_file_name(&file) || !real(&folder, false) || !real(&path, true) {
            return None;
        }
        let recipe = (found.job.as_ref())
            .and_then(|key| self.recipes.iter().find(|recipe| &recipe.key() == key));
        let updated_at = found.about["saved_at"].as_i64().unwrap_or_default();
        let runner = recipe
            .filter(|r| shelf.keeping == Keeping::SharedStorage && r.sharing == Sharing::OneRunner)
            .and_then(|r| r.runner.clone());
        let title = (found.about["title"].as_str()).unwrap_or(&file).to_owned();
        let run_id = found.about["run_id"].as_str().map(str::to_owned);
        let waiting = shelf.keeping == Keeping::Private
            && (run_id.clone()).is_some_and(|run| self.waiting.contains(&(run, title.clone())));
        Some(BoardOutput {
            id: found.id(),
            title,
            run_id,
            waiting,
            space_id: shelf.space.clone(),
            job: found.job.clone(),
            job_name: recipe.map(|r| r.name.clone()),
            preview: preview(&path),
            folder: folder.to_string_lossy().into_owned(),
            file,
            updated_at,
            keeping: shelf.keeping,
            runner,
            stale: recipe.is_some_and(|r| stale(r, updated_at, self.now)),
        })
    }
    /// An output's versions: `LATEST`, the output as it is now, then its history newest first:
    /// each commit that rewrote its metadata, and for a shared job's output, the runner's saves
    /// that a later save replaced before a push, which only this Mac keeps. The commit that
    /// holds the latest is not listed twice. An output whose own metadata names no plain file
    /// has no latest.
    pub(crate) fn versions(&self, id: &str) -> Result<Vec<BoardVersion>> {
        let found = self.find(id)?;
        let latest = self.version_of(LATEST, &found.about, 0, false);
        let mut history = self.logged(&found.shelf.repository, &found.metadata(), false)?;
        if let Some(folder) = self.unpushed_of(&found) {
            history.extend(self.logged(&self.store, &format!("{folder}.json"), true)?);
            history.sort_by_key(|version| std::cmp::Reverse(version.saved_at));
        }
        // The latest's metadata is as untrusted as an earlier commit's.
        if !plain_file_name(&latest.file) {
            return Ok(history);
        }
        let held = |version: &BoardVersion| {
            let commit = LATEST.to_owned();
            !version.local_only
                && BoardVersion {
                    commit,
                    ..version.clone()
                } == latest
        };
        if history.first().is_some_and(held) {
            history.remove(0);
        }
        history.insert(0, latest);
        Ok(history)
    }
    /// The version whose metadata is `about`, saved at `committed` (Unix seconds) if it says no
    /// time.
    fn version_of(
        &self,
        commit: &str,
        about: &Value,
        committed: i64,
        local_only: bool,
    ) -> BoardVersion {
        let run_id = about["run_id"].as_str().map(str::to_owned);
        BoardVersion {
            commit: commit.to_owned(),
            saved_at: about["saved_at"].as_i64().unwrap_or(committed * 1000),
            catch_up: (run_id.as_ref())
                .and_then(|run| self.catch_ups.get(run).copied())
                .unwrap_or_default(),
            run_id,
            shared: about["shared"].as_bool().unwrap_or_default(),
            local_only,
            file: about["file"].as_str().unwrap_or_default().to_owned(),
        }
    }
    /// The private board store's folder that keeps the replaced, never pushed saves of a
    /// shared job's output, by its title.
    fn unpushed_of(&self, found: &Found) -> Option<String> {
        let job = found
            .job
            .as_ref()
            .filter(|_| found.shelf.keeping == Keeping::SharedStorage)?;
        let (_, job) = job.as_str().rsplit_once('/')?;
        let base = format!(
            "{}/{}/{job}/{}",
            found.shelf.space,
            assistant::JOB_OUTPUTS,
            jobs::UNPUSHED
        );
        let title = found.about["title"].as_str()?;
        let folder = assistant::output_folder(&self.store.join(&base), title).ok()?;
        Some(format!("{base}/{folder}"))
    }
    /// The versions Git holds of the output whose metadata is `metadata` in `repository`.
    fn logged(
        &self,
        repository: &Path,
        metadata: &str,
        local_only: bool,
    ) -> Result<Vec<BoardVersion>> {
        if !repository.join(".git").exists() {
            return Ok(vec![]);
        }
        let limit = format!("-n{MAX_VERSIONS}");
        let log = runtime::git_output(
            repository,
            &["log", &limit, "--format=%H %ct", "--", metadata],
        )?;
        let mut versions = vec![];
        for line in log.lines() {
            let Some((commit, time)) = line.split_once(' ') else {
                continue;
            };
            let shown = format!("{commit}:{metadata}");
            let about: Value = runtime::git_output(repository, &["show", &shown])
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            let committed = time.parse().unwrap_or_default();
            let version = self.version_of(commit, &about, committed, local_only);
            // An earlier commit's metadata is as untrusted as the latest's.
            if plain_file_name(&version.file) {
                versions.push(version);
            }
        }
        Ok(versions)
    }
    /// The folder holding the version `commit` of an output: its own folder for `LATEST`, else
    /// a copy of the commit's files, made once.
    pub(crate) fn version_folder(&self, id: &str, commit: &str) -> Result<PathBuf> {
        let found = self.find(id)?;
        if commit == LATEST {
            let file = found.about["file"].as_str().unwrap_or_default();
            ensure!(
                plain_file_name(file),
                "That version is not one of the output's"
            );
            return Ok(found.shelf.repository.join(&found.folder));
        }
        let versions = self.versions(id)?;
        let at = (versions.iter())
            .position(|version| version.commit == commit)
            .context("That version is not one of the output's")?;
        let (repository, folder) = match versions[at].local_only {
            true => (
                self.store.clone(),
                self.unpushed_of(&found).context("That version is gone")?,
            ),
            false => (found.shelf.repository.clone(), found.folder.clone()),
        };
        let copy = self.versions.join(id).join(commit);
        if copy.is_dir() {
            return Ok(copy);
        }
        let staged = self.versions.join(id).join(format!(".{commit}"));
        if staged.exists() {
            fs::remove_dir_all(&staged)?;
        }
        let prefix = format!("{folder}/");
        let listed = runtime::git_output(
            &repository,
            &["ls-tree", "-r", "-z", "--name-only", commit, "--", &prefix],
        )?;
        for path in listed.split('\0').filter(|path| !path.is_empty()) {
            let relative = path.strip_prefix(&prefix).context("Unexpected path")?;
            let bytes =
                runtime::git_bytes(&repository, &["show", &format!("{commit}:{path}")], &[])?;
            shared::write_atomically(&staged.join(relative), &bytes)?;
        }
        fs::create_dir_all(&staged)?;
        fs::rename(&staged, &copy)?;
        Ok(copy)
    }
}

/// The outputs whose metadata is directly in `base`.
fn outputs_in<'a>(shelf: &'a Shelf, base: &str, job: Option<JobKey>) -> Vec<Found<'a>> {
    let Ok(entries) = fs::read_dir(shelf.repository.join(base)) else {
        return vec![];
    };
    let Ok(repository) = shelf.repository.canonicalize() else {
        return vec![];
    };
    let mut found = vec![];
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(slug) = name.strip_suffix(".json") else {
            continue;
        };
        // The output's folder must be where its path says, with no link and no `.` or `..` on
        // the way, as it becomes the root its board is served from.
        let folder = format!("{base}/{slug}");
        let in_place = (shelf.repository.join(&folder).canonicalize())
            .is_ok_and(|resolved| resolved == repository.join(&folder));
        let metadata = entry.file_type().is_ok_and(|kind| kind.is_file());
        if !plain_file_name(slug) || slug.starts_with('.') || !in_place || !metadata {
            continue;
        }
        let Some(about) = (fs::read(entry.path()).ok())
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        else {
            continue;
        };
        found.push(Found {
            shelf,
            folder,
            job: job.clone(),
            about,
        });
    }
    found
}

/// The first lines of a Markdown or text file, or an HTML file's title.
fn preview(path: &Path) -> Option<String> {
    use std::io::Read;
    let kind = FileKind::of(&path.to_string_lossy());
    let text = path.extension().is_some_and(|e| e == "txt");
    if !matches!(kind, FileKind::Markdown | FileKind::Html) && !text {
        return None;
    }
    let mut start = vec![];
    fs::File::open(path)
        .ok()?
        .take(PREVIEW_BYTES)
        .read_to_end(&mut start)
        .ok()?;
    let start = String::from_utf8_lossy(&start);
    let shown = match kind {
        FileKind::Html => {
            // ASCII lowercasing keeps every byte where it was, so its offsets slice `start`.
            let lower = start.to_ascii_lowercase();
            let open = lower.find("<title>")? + "<title>".len();
            let close = open + lower[open..].find("</title>")?;
            start[open..close].trim().to_owned()
        }
        _ => (start.lines())
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(6)
            .collect::<Vec<_>>()
            .join("\n"),
    };
    (!shown.is_empty()).then(|| shown.chars().take(PREVIEW_CHARS).collect())
}

/// Two of the job's scheduled times passed since its output's last version; a paused job
/// misses its times on purpose, so its output is never stale.
fn stale(recipe: &Recipe, updated_at: i64, now: i64) -> bool {
    if !recipe.enabled {
        return false;
    }
    let (Ok(cron), Ok(zone)) = (
        Cron::parse(&recipe.schedule.cron),
        jobs::timezone(&recipe.schedule.timezone),
    ) else {
        return false;
    };
    jobs::times_between(&cron, zone, updated_at + 1, now) >= 2
}

impl Host {
    /// What Boards reads: the private board store and the storage of each Space that is on.
    pub(crate) fn shelves(&self) -> Result<Shelves> {
        let store = assistant::board_store(&self.home);
        let mut shelves = vec![];
        for record in self.spaces()? {
            if !record.local.enabled {
                continue;
            }
            let space = record.space;
            shelves.push(Shelf {
                repository: store.clone(),
                space: space.id.clone(),
                keeping: Keeping::Private,
                loose: space.id.to_string(),
                jobs: format!("{}/{}", space.id, assistant::JOB_OUTPUTS),
            });
            let boards = format!("spaces/{}/boards", space.storage.folder);
            let boards = folder::Root::new(&space.storage.root).join(&boards);
            shelves.push(Shelf {
                repository: self.storage_folder(&space.storage.repository),
                space: space.id.clone(),
                keeping: match space.storage.repository {
                    StorageRepository::Local { .. } => Keeping::LocalStorage,
                    StorageRepository::GitHub { .. } => Keeping::SharedStorage,
                },
                loose: format!("{boards}/{SHARED_OUTPUTS}"),
                jobs: boards,
            });
        }
        let catch_ups = self
            .db
            .prepare("SELECT id, json_extract(data,'$.catch_up') FROM job_runs WHERE json_extract(data,'$.catch_up')>0")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let waiting = self
            .db
            .prepare("SELECT r.id, json_extract(o.value,'$.title') FROM job_runs r, json_each(r.data,'$.outputs') o WHERE json_extract(o.value,'$.waiting')=1")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Shelves {
            shelves,
            recipes: self.recipes()?,
            catch_ups,
            waiting,
            versions: self.home.join("assistant").join("versions"),
            store,
            now: now(),
        })
    }
    /// Share to Space…: copies an output that is only on this Mac into `space`'s storage, as
    /// its own output there with the same title. The user confirmed who can read it first.
    pub fn share_board(&mut self, id: &str, space: &SpaceId) -> Result<BoardOutput> {
        let shelves = self.shelves()?;
        let found = shelves.find(id)?;
        ensure!(
            found.shelf.keeping == Keeping::Private,
            "Only an output kept only on this Mac can be shared"
        );
        let output = shelves.describe(&found).context("The output has no file")?;
        ensure!(
            !output.waiting,
            "This output waits for your approval: choose Share or Keep private"
        );
        let record = self.space(space)?;
        ensure!(
            record.local.enabled,
            "{} is off on this Mac",
            record.space.name
        );
        let target = record.space;
        let base = format!("spaces/{}/boards/{SHARED_OUTPUTS}", target.storage.folder);
        let bytes = fs::read(output.path())?;
        let about = json!({"title": output.title, "saved_at": now(), "shared": true});
        let message = format!("{}: shared the output {}", target.name, output.title);
        let folder = self.write_output(&target, &base, &output.file, bytes, about, &message)?;
        let shared = self.shelves()?.list();
        (shared.into_iter())
            .find(|o| {
                o.space_id == target.id
                    && o.keeping != Keeping::Private
                    && o.folder.ends_with(&folder)
            })
            .context("The shared copy was not found")
    }
    /// Writes an output into `space`'s storage under `base`, at the folder of the title `about`
    /// names, as `file` with `about` beside it; a title written before is replaced, its old file
    /// included. Committed at once in local storage, and pushed by the next pass from GitHub.
    /// Returns the output's folder, relative to the repository's folder.
    pub(crate) fn write_output(
        &self,
        space: &Space,
        base: &str,
        file: &str,
        bytes: Vec<u8>,
        mut about: Value,
        message: &str,
    ) -> Result<String> {
        let storage = self.root_folder(&space.storage.repository)?;
        let title = about["title"].as_str().context("An output has a title")?;
        let folder = format!(
            "{base}/{}",
            assistant::output_folder(&storage.join(base), title)?
        );
        about["file"] = json!(file);
        let about = serde_json::to_vec_pretty(&about)?;
        let mut files: Vec<(String, Option<&[u8]>)> = vec![];
        for old in (fs::read_dir(storage.join(&folder)).into_iter().flatten()).flatten() {
            let name = old.file_name().to_string_lossy().into_owned();
            if name != file {
                files.push((format!("{folder}/{name}"), None));
            }
        }
        files.push((format!("{folder}/{file}"), Some(&bytes)));
        files.push((format!("{folder}.json"), Some(&about)));
        self.write_storage_files(space, &files, message)?;
        Ok(folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::git;

    fn host() -> (tempfile::TempDir, Host, Space) {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open_set_up(home.path()).unwrap();
        let personal = host.space(&host.default_space().unwrap()).unwrap().space;
        (home, host, personal)
    }

    fn save(store: &Path, base: &str, title: &str, file: &str, body: &str, about: Value) {
        let scratch = tempfile::tempdir().unwrap();
        fs::write(scratch.path().join(file), body).unwrap();
        assistant::store_output(store, base, title, &scratch.path().join(file), about).unwrap();
    }

    #[test]
    fn boards_lists_thread_and_job_outputs_with_their_versions() {
        let (home, mut host, personal) = host();
        let store = assistant::board_store(home.path());
        let thread = json!({"title":"Competitor pricing","thread_id":"t1","saved_at":1000});
        save(
            &store,
            personal.id.as_str(),
            "Competitor pricing",
            "pricing.md",
            "# Pricing\n\nThree tiers.\n",
            thread,
        );
        let recipe = Recipe {
            id: "release-dashboard".into(),
            name: "Release dashboard".into(),
            space_id: personal.id.clone(),
            prompt: "Draw it.".into(),
            schedule: workspace_core::job::Schedule {
                cron: "0 8 * * 1".into(),
                timezone: "America/New_York".into(),
            },
            model: None,
            sources: vec![],
            output: None,
            sharing: Sharing::OneRunner,
            runner: None,
            enabled: true,
            extra: Default::default(),
        };
        host.save_job(recipe).unwrap();
        let storage = host.storage_folder(&personal.storage.repository);
        let base = folder::Root::new(&personal.storage.root).join(&format!(
            "spaces/{}/boards/release-dashboard",
            personal.storage.folder
        ));
        let now = now();
        for (body, run, saved_at) in [
            ("<title>Week one</title>", "r1", now - 1000),
            ("<title>Week two</title>", "r2", now),
        ] {
            let about = json!({"title":"Release dashboard","run_id":run,"saved_at":saved_at});
            save(
                &storage,
                &base,
                "Release dashboard",
                "dashboard.html",
                body,
                about,
            );
        }
        host.db
            .execute(
                "INSERT INTO job_runs VALUES ('r1', ?1, 0, ?2)",
                params![
                    JobKey::recipe(&personal.id, "release-dashboard").as_str(),
                    json!({"id":"r1","job":"x","started_at":0,"outcome":"finished","catch_up":3})
                        .to_string()
                ],
            )
            .unwrap();

        let shelves = host.shelves().unwrap();
        let outputs = shelves.list();
        assert_eq!(outputs.len(), 2, "{outputs:?}");
        let dashboard = &outputs[0];
        assert_eq!(
            (
                dashboard.title.as_str(),
                dashboard.job_name.as_deref(),
                dashboard.keeping
            ),
            (
                "Release dashboard",
                Some("Release dashboard"),
                Keeping::LocalStorage
            )
        );
        assert_eq!(dashboard.preview.as_deref(), Some("Week two"));
        assert!(!dashboard.stale);
        let pricing = &outputs[1];
        assert_eq!(
            (pricing.job.as_ref(), pricing.keeping),
            (None, Keeping::Private)
        );
        assert_eq!(pricing.preview.as_deref(), Some("# Pricing\nThree tiers."));

        let versions = shelves.versions(&dashboard.id).unwrap();
        let runs: Vec<_> = versions
            .iter()
            .map(|v| (v.run_id.as_deref(), v.catch_up))
            .collect();
        assert_eq!(runs, [(Some("r2"), 0), (Some("r1"), 3)]);
        // The latest is the output's own folder; an earlier one is a copy that leaves it be.
        let latest = shelves
            .version_folder(&dashboard.id, &versions[0].commit)
            .unwrap();
        assert_eq!(latest, Path::new(&dashboard.folder));
        let earlier = shelves
            .version_folder(&dashboard.id, &versions[1].commit)
            .unwrap();
        assert_eq!(
            fs::read_to_string(earlier.join("dashboard.html")).unwrap(),
            "<title>Week one</title>"
        );
        assert_eq!(
            fs::read_to_string(dashboard.path()).unwrap(),
            "<title>Week two</title>"
        );
        assert!(shelves.version_folder(&dashboard.id, "HEAD").is_err());
    }

    #[test]
    fn an_earlier_version_names_its_own_file_and_a_title_with_unicode_previews() {
        let (home, host, personal) = host();
        let store = assistant::board_store(home.path());
        for (file, body) in [
            ("first.md", "# First"),
            ("second.html", "<!doctype html><!--İ--><title>é</title>"),
        ] {
            let about = json!({"title":"Brief","thread_id":"t1","saved_at":1});
            save(&store, personal.id.as_str(), "Brief", file, body, about);
        }
        let shelves = host.shelves().unwrap();
        let brief = shelves.list().remove(0);
        assert_eq!(
            (brief.file.as_str(), brief.preview.as_deref()),
            ("second.html", Some("é"))
        );
        let versions = shelves.versions(&brief.id).unwrap();
        let files: Vec<&str> = versions.iter().map(|v| v.file.as_str()).collect();
        assert_eq!(files, ["second.html", "first.md"]);
        let first = shelves
            .version_folder(&brief.id, &versions[1].commit)
            .unwrap();
        assert_eq!(
            fs::read_to_string(first.join("first.md")).unwrap(),
            "# First"
        );
    }

    #[test]
    fn a_version_or_output_naming_a_path_or_a_link_is_left_out() {
        let (home, host, personal) = host();
        let store = assistant::board_store(home.path());
        let space = personal.id.as_str();
        let about = json!({"title":"Brief","thread_id":"t1","saved_at":1});
        save(&store, space, "Brief", "brief.md", "# Brief", about);
        // Two earlier commits whose metadata names a file outside the version's folder.
        let metadata = store.join(space).join("brief.json");
        for named in ["/etc/passwd", "../../secret.md"] {
            let about = json!({"title":"Brief","file":named,"saved_at":1});
            fs::write(&metadata, about.to_string()).unwrap();
            git(
                &store,
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "commit",
                    "-qam",
                    named,
                ],
            );
        }
        let about = json!({"title":"Brief","file":"brief.md","saved_at":2});
        fs::write(&metadata, about.to_string()).unwrap();
        git(
            &store,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qam",
                "back",
            ],
        );
        // And an output whose folder is a link to a folder elsewhere.
        let elsewhere = tempfile::tempdir().unwrap();
        fs::write(elsewhere.path().join("secret.md"), "secret").unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), store.join(space).join("linked")).unwrap();
        let linked = json!({"title":"Linked","file":"secret.md","saved_at":3});
        fs::write(store.join(space).join("linked.json"), linked.to_string()).unwrap();

        let shelves = host.shelves().unwrap();
        let outputs = shelves.list();
        let titles: Vec<&str> = outputs.iter().map(|o| o.title.as_str()).collect();
        assert_eq!(titles, ["Brief"]);
        let versions = shelves.versions(&outputs[0].id).unwrap();
        let files: Vec<&str> = versions.iter().map(|v| v.file.as_str()).collect();
        assert_eq!(files, ["brief.md", "brief.md"]);
        // The latest is left out when its own metadata names a path; the history stays.
        let bad = json!({"title":"Brief","file":"../../secret.md","saved_at":4});
        fs::write(&metadata, bad.to_string()).unwrap();
        let versions = shelves.versions(&outputs[0].id).unwrap();
        assert!(versions.iter().all(|v| v.commit != LATEST), "{versions:?}");
        assert_eq!(versions.len(), 2);
        assert!(shelves.version_folder(&outputs[0].id, LATEST).is_err());
    }

    #[test]
    fn a_dot_slug_or_a_linked_job_folder_never_widens_a_boards_root() {
        let (_home, host, personal) = host();
        let storage = host.root_folder(&personal.storage.repository).unwrap();
        let boards = storage.join(format!("spaces/{}/boards", personal.storage.folder));
        fs::create_dir_all(&boards).unwrap();
        std::os::unix::fs::symlink("../../..", boards.join("escape")).unwrap();
        let about = json!({"title":"Board","file":"index.html","saved_at":1}).to_string();
        fs::write(storage.join("..json"), &about).unwrap();
        fs::write(storage.join("index.html"), "<p>root</p>").unwrap();
        // A dot slug beside real outputs is refused too, linked or not.
        let shared = boards.join("_shared");
        fs::create_dir_all(&shared).unwrap();
        fs::write(shared.join("..json"), &about).unwrap();
        fs::write(shared.join(".json"), &about).unwrap();
        fs::write(boards.join("index.html"), "<p>boards</p>").unwrap();
        assert!(host.shelves().unwrap().list().is_empty());
    }

    #[test]
    fn an_output_is_stale_after_two_scheduled_runs_without_a_version() {
        let mut recipe: Recipe = serde_json::from_value(json!({
            "id":"brief","name":"Brief","space_id":"s","prompt":"p",
            "schedule":{"cron":"0 7 * * *","timezone":"UTC"},"enabled":true
        }))
        .unwrap();
        let day = 86_400_000;
        let saved = 7 * 3_600_000;
        assert!(!stale(&recipe, saved, saved + day));
        assert!(stale(&recipe, saved, saved + 2 * day));
        recipe.schedule.cron = "0 7 * * 1".into();
        assert!(!stale(&recipe, saved, saved + 2 * day));
        recipe.schedule.cron = "0 7 * * *".into();
        recipe.enabled = false;
        assert!(!stale(&recipe, saved, saved + 2 * day));
    }

    #[test]
    fn share_to_space_copies_a_private_output_into_the_storage_as_a_commit() {
        let (home, mut host, personal) = host();
        let store = assistant::board_store(home.path());
        let about = json!({"title":"Chart","thread_id":"t1","saved_at":1});
        save(
            &store,
            personal.id.as_str(),
            "Chart",
            "chart.html",
            "<p>one</p>",
            about,
        );
        let output = host.shelves().unwrap().list().remove(0);
        let shared = host.share_board(&output.id, &personal.id).unwrap();
        assert_eq!(
            (shared.title.as_str(), shared.keeping),
            ("Chart", Keeping::LocalStorage)
        );
        assert_eq!(fs::read_to_string(shared.path()).unwrap(), "<p>one</p>");
        let storage = host.storage_folder(&personal.storage.repository);
        let subject = git(&storage, &["log", "-1", "--format=%s"]);
        assert!(subject.contains("shared the output Chart"), "{subject}");
        // A shared copy is not shared again, and the private output stays.
        assert!(host.share_board(&shared.id, &personal.id).is_err());
        assert_eq!(host.shelves().unwrap().list().len(), 2);
    }
}
