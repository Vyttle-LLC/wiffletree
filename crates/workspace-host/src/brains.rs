//! A Space's brain is `second-brain/` in its repository's folder (OpenSpec change
//! `one-space-repository`), so there is one for each repository holding a Space. The repository's
//! sync engine carries its files (`brain_sync`). What stays here is the Brains page's read model
//! and the index refresh, which runs off the host thread. The tables an earlier release kept for
//! brains and their batches stay, unwritten, for the upgrade to read.
use crate::*;
use folder::Root;
use publish::Tools;
use std::collections::BTreeMap;

const SCHEMA: &str = include_str!("brains.sql");

/// Creates the tables an earlier release wrote, so an upgrade from before them reads no rows.
pub(crate) fn migrate(host: &mut Host) -> Result<()> {
    let present: bool = host.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='brains')",
        [],
        |r| r.get(0),
    )?;
    if present {
        return Ok(());
    }
    host.db.execute_batch(SCHEMA)?;
    Ok(())
}

/// A lowercase ULID: 48 bits of `at` in milliseconds, then 80 random bits, in Crockford's base
/// 32. Batch ids and log entry ids both use it.
pub(crate) fn new_ulid(at: i64) -> String {
    ulid(at, u128::from_be_bytes(*uuid::Uuid::new_v4().as_bytes()))
}

/// A lowercase ULID of `at` and the low 80 bits of `random`.
pub(crate) fn ulid(at: i64, random: u128) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let time = (at as u128) & ((1 << 48) - 1);
    let random = random & ((1 << 80) - 1);
    let mut value = (time << 80) | random;
    let mut id = [0u8; 26];
    for slot in id.iter_mut().rev() {
        *slot = ALPHABET[(value & 31) as usize];
        value >>= 5;
    }
    String::from_utf8_lossy(&id).into_owned()
}

/// One brain's index refresh, gathered on the host thread and run off it.
pub(crate) struct Pass {
    repository: String,
    index: PathBuf,
    clone: PathBuf,
    /// The commit to index.
    head: String,
    brain_dir: String,
    /// The clones it verifies refs against.
    clones: verify_refs::Clones,
}

/// What a refresh did.
pub(crate) struct Passed {
    pub(crate) reached: bool,
}

/// Brings the brain's index to the commit the repository last synced.
pub(crate) fn pass(pass: Pass, tools: &Tools) -> Passed {
    let refreshed = brain_index::refresh(brain_index::Refresh {
        index: &pass.index,
        checkout: &pass.clone,
        commit: &pass.head,
        brain_dir: &pass.brain_dir,
        clones: &pass.clones,
        gh: Some(&tools.gh),
    });
    if let Err(error) = &refreshed {
        eprintln!("Brain index of {}: {error:#}", pass.repository);
    }
    Passed {
        reached: refreshed.is_ok(),
    }
}

impl Host {
    /// Each repository holding a Space on this Mac, by its name, with its folder and the Spaces
    /// in it.
    fn wikis(&self) -> Result<Vec<(StorageRepository, Root, Vec<SpaceId>)>> {
        let mut wikis: Vec<(StorageRepository, Root, Vec<SpaceId>)> = vec![];
        for record in self.spaces()? {
            let storage = record.space.storage;
            match (wikis.iter_mut()).find(|(repository, ..)| *repository == storage.repository) {
                Some((.., spaces)) => spaces.push(record.space.id),
                None => wikis.push((
                    storage.repository,
                    Root::new(&storage.root),
                    vec![record.space.id],
                )),
            }
        }
        Ok(wikis)
    }

    /// The brain's name by sync's record key, for what is stored by that key.
    pub(crate) fn brain_keys(&self) -> Result<BTreeMap<String, String>> {
        Ok((self.wikis()?.into_iter())
            .map(|(repository, ..)| (repos::key(&repository.key()), repository.key()))
            .collect())
    }

    /// Every wiki on this Mac.
    pub fn brains(&self) -> Result<Vec<Brain>> {
        let mut brains = vec![];
        for (repository, _, spaces) in self.wikis()? {
            let (synced_at, problem) = match Self::storage_key(&repository) {
                Some(key) => self.storage_sync_state(&key)?,
                None => (None, None),
            };
            brains.push(Brain {
                repository: repository.key(),
                spaces,
                synced_at,
                problem,
            });
        }
        Ok(brains)
    }

    /// The wiki in the repository `repository`.
    pub fn brain(&self, repository: &str) -> Result<Brain> {
        (self.brains()?.into_iter())
            .find(|b| b.repository.eq_ignore_ascii_case(repository))
            .with_context(|| format!("No Space here is in {repository}"))
    }

    /// The repository and folder of the Spaces `brain` belongs to.
    pub(crate) fn brain_home(&self, brain: &Brain) -> Result<(StorageRepository, Root)> {
        (self.wikis()?.into_iter())
            .find(|(repository, ..)| repository.key() == brain.repository)
            .map(|(repository, root, _)| (repository, root))
            .with_context(|| format!("No Space here is in {}", brain.repository))
    }

    /// The clone that holds `brain`.
    pub(crate) fn brain_clone(&self, brain: &Brain) -> Result<PathBuf> {
        let (repository, _) = self.brain_home(brain)?;
        Ok(self.storage_folder(&repository))
    }

    /// The commit a read of the repository starts from: the one its pass last synced, or for a
    /// local repository its head. `None` before it has one.
    pub(crate) fn brain_head(&self, repository: &StorageRepository) -> Result<Option<String>> {
        let Some(key) = Self::storage_key(repository) else {
            let clone = self.storage_folder(repository);
            let head = ["rev-parse", "--verify", "--quiet", "HEAD"];
            let head = runtime::git_answer(&clone, &head)?;
            return Ok(head.status.success().then(|| head.output.trim().to_owned()));
        };
        let synced: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT synced FROM sync_record WHERE repository=?1 AND path=''",
                [key],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(synced.map(String::from_utf8).transpose()?)
    }

    /// The Brains page: every wiki, with its compile, and what has no project to go to.
    pub fn brains_page(&self) -> Result<BrainsPage> {
        let conflicts = (self.conflict_cards()?.into_iter())
            .filter(|card| card.projects.is_empty())
            .collect();
        let held = (self.held_entries()?.into_iter())
            .filter(|entry| entry.project_id.is_none())
            .collect();
        Ok(BrainsPage {
            brains: self.brains()?,
            compiles: self.brain_compile_states()?,
            conflicts,
            held,
            left_pull_requests: self.left_pull_requests()?,
        })
    }

    /// The batch pull requests an earlier release left open on a brain a Space still names,
    /// each once. Nothing watches them; Bring in a brain ends the Space's naming it.
    fn left_pull_requests(&self) -> Result<Vec<LeftPullRequest>> {
        let named: Vec<String> = (self.spaces()?.into_iter())
            .filter_map(|record| match record.space.brain?.repository {
                BrainRepository::GitHub { owner, name, .. } => Some(format!("{owner}/{name}")),
                BrainRepository::Local { .. } => None,
            })
            .collect();
        let rows: Vec<(String, u64)> = (self.db.prepare(
            "SELECT DISTINCT repository,json_extract(data,'$.pull_request.number') FROM brain_batches
             WHERE json_extract(data,'$.pull_request') IS NOT NULL
               AND json_extract(data,'$.stage') NOT IN ('merged','discarded') ORDER BY 1,2",
        )?)
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
        Ok((rows.into_iter())
            .filter(|(repository, _)| named.iter().any(|n| n.eq_ignore_ascii_case(repository)))
            .map(|(repository, number)| LeftPullRequest { repository, number })
            .collect())
    }

    /// The repository of the brain of a project, a thread or a job run: its Space's, and in no
    /// other way.
    pub(crate) fn owner_brain(&self, owner: &SessionOwner) -> Result<String> {
        Ok(match owner {
            SessionOwner::Project(project) => {
                let space = self.project(project)?.space;
                self.space(&space)?.space.storage.repository.key()
            }
            SessionOwner::Space(space) => self.space(space)?.space.storage.repository.key(),
            SessionOwner::Brain(repository) => repository.clone(),
        })
    }

    /// Each brain to refresh and how often: GitHub is asked again about the PRs entries name, at
    /// the pace an idle pull request is watched. A pass that synced makes it due at once.
    pub(crate) fn brain_cadences(&self) -> Result<Vec<(String, i64)>> {
        Ok((self.wikis()?.into_iter())
            .map(|(repository, ..)| (repository.key(), pr_watch::IDLE_MS))
            .collect())
    }

    /// What a refresh of the brain needs, or `None` before its repository has a commit.
    pub(crate) fn brain_pass(&mut self, repository: &str) -> Result<Option<Pass>> {
        let brain = self.brain(repository)?;
        let (storage, root) = self.brain_home(&brain)?;
        let Some(head) = self.brain_head(&storage)? else {
            return Ok(None);
        };
        Ok(Some(Pass {
            index: brain_index::index_file(&self.home, &brain.repository),
            clones: verify_refs::Clones::of(&self.repositories()?),
            clone: self.storage_folder(&storage),
            brain_dir: brain_sync::brain_dir(&root),
            repository: brain.repository,
            head,
        }))
    }

    pub(crate) fn coordinator_of(&self, project: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT id FROM sessions WHERE project_id=?1 AND parent_id IS NULL AND role=?2",
                params![project, tag(&Role::ProjectOrchestrator)?],
                |r| r.get(0),
            )
            .optional()?)
    }
}
