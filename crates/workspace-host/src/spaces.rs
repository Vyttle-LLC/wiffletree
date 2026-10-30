//! The Spaces store (schema 9): every project belongs to a Space, and the upgrade turns each
//! project's attached brain folder into one. Only the human changes a Space.
use crate::*;
use std::collections::BTreeMap;

const SCHEMA: &str = "
CREATE TABLE spaces (id TEXT PRIMARY KEY, data TEXT NOT NULL, local TEXT NOT NULL, used_at INTEGER NOT NULL DEFAULT 0);
CREATE TABLE trackers (storage TEXT PRIMARY KEY, data TEXT NOT NULL);
ALTER TABLE work_logs ADD COLUMN space_id TEXT;
";

/// Creates the Spaces store and, in the same transaction, a Space for each distinct brain folder
/// attached to a project and "Personal" for projects without one, or for a store with no
/// project. Runs once: a store that has the `spaces` table is left alone, unless it holds no
/// Space, when it gets "Personal" so a new project has somewhere to go. Pushes nothing. The
/// workspace repositories it found wait for the human to confirm their suggested Spaces.
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS repository_suggestions (repository_id TEXT PRIMARY KEY);",
    )?;
    let present: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='spaces')",
        [],
        |r| r.get(0),
    )?;
    if !present {
        let tx = db.unchecked_transaction()?;
        tx.execute_batch(SCHEMA)?;
        upgrade(&tx)?;
        tx.commit()?;
    }
    let empty: bool = db.query_row("SELECT NOT EXISTS(SELECT 1 FROM spaces)", [], |r| r.get(0))?;
    if empty {
        let tx = db.unchecked_transaction()?;
        let (space, tracker) = new_space(PERSONAL, &mut prefixes(&tx)?);
        insert_space(&tx, &space, &SpaceLocal::default(), Some(&tracker))?;
        tx.commit()?;
    }
    db.execute_batch("PRAGMA user_version = 9;")?;
    Ok(())
}

fn prefixes(db: &Connection) -> Result<Vec<String>> {
    Ok(db
        .prepare("SELECT json_extract(data,'$.prefix') FROM trackers")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

fn upgrade(db: &Connection) -> Result<()> {
    let projects: Vec<(String, Value)> = db
        .prepare("SELECT id,data FROM projects ORDER BY rowid")?
        .query_map([], |r| Ok((r.get(0)?, r.get::<_, String>(1)?)))?
        .map(|row| {
            let (id, data) = row?;
            Ok((id, serde_json::from_str(&data)?))
        })
        .collect::<Result<_>>()?;
    let brain_of = |project: &Value| {
        project["brain"]
            .as_str()
            .filter(|b| !b.is_empty())
            .map(str::to_owned)
    };
    let mut brains: Vec<String> = vec![];
    for (_, project) in &projects {
        if let Some(brain) = brain_of(project)
            && !brains.contains(&brain)
        {
            brains.push(brain);
        }
    }
    let mut prefixes = vec![];
    let mut space_of_brain = BTreeMap::new();
    for (brain, name) in brains.iter().zip(upgraded_names(&brains)) {
        let local = SpaceLocal {
            brain_path: Some(brain.clone()),
            ..SpaceLocal::default()
        };
        let mut space = new_space(&name, &mut prefixes);
        space.0.brain = Some(SpaceBrain {
            repository: BrainRepository::local(),
            folder: SpaceBrain::DEFAULT_FOLDER.into(),
            extra: Extra::new(),
        });
        insert_space(db, &space.0, &local, Some(&space.1))?;
        space_of_brain.insert(brain.clone(), space.0.id);
    }
    let personal = if brains.is_empty() || projects.iter().any(|(_, p)| brain_of(p).is_none()) {
        let (space, tracker) = new_space(PERSONAL, &mut prefixes);
        insert_space(db, &space, &SpaceLocal::default(), Some(&tracker))?;
        Some(space.id)
    } else {
        None
    };
    for (id, mut project) in projects {
        let space = match brain_of(&project) {
            Some(brain) => &space_of_brain[&brain],
            None => personal.as_ref().context("Personal was created")?,
        };
        let fields = project
            .as_object_mut()
            .context("Project is not an object")?;
        fields.remove("brain");
        fields.insert("space".into(), json!(space));
        db.execute(
            "UPDATE projects SET data=?2 WHERE id=?1",
            params![id, project.to_string()],
        )?;
    }
    db.execute(
        "UPDATE work_logs SET space_id=(SELECT json_extract(p.data,'$.space') FROM projects p WHERE p.id=work_logs.project_id)",
        [],
    )?;
    db.execute(
        "INSERT OR IGNORE INTO repository_suggestions SELECT id FROM repositories",
        [],
    )?;
    Ok(())
}

/// Each brain folder's Space name: the folder's name, or "‹parent›/‹folder›" when another folder
/// has the same name or it is named "Personal", followed by a number while still not unique.
fn upgraded_names(brains: &[String]) -> Vec<String> {
    let part = |path: &Path| {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned())
    };
    let folders: Vec<String> = brains.iter().map(|b| part(Path::new(b))).collect();
    let mut taken: Vec<String> = vec![PERSONAL.to_lowercase()];
    let mut names = vec![];
    for (brain, folder) in brains.iter().zip(&folders) {
        let shared = folders
            .iter()
            .filter(|f| f.eq_ignore_ascii_case(folder))
            .count()
            > 1;
        let base = if shared || folder.eq_ignore_ascii_case(PERSONAL) {
            let parent = Path::new(brain).parent().map(part).unwrap_or_default();
            format!("{parent}/{folder}")
        } else {
            folder.clone()
        };
        let name = (1..)
            .map(|n| match n {
                1 => base.clone(),
                n => format!("{base} {n}"),
            })
            .find(|name| !taken.contains(&name.to_lowercase()))
            .unwrap_or(base);
        taken.push(name.to_lowercase());
        names.push(name);
    }
    names
}

/// A Space named `name` with new local storage, whose tracker gets a prefix proposed from the
/// name and not in `prefixes`, which gains it.
fn new_space(name: &str, prefixes: &mut Vec<String>) -> (Space, TrackerConfig) {
    let prefix = propose_prefix(name, prefixes);
    prefixes.push(prefix.clone());
    let space = Space {
        id: SpaceId(new_id()),
        name: name.into(),
        description: String::new(),
        instructions: String::new(),
        brain: None,
        storage: SpaceStorage {
            repository: StorageRepository::Local { id: new_id() },
            folder: settings::slug(name, "space"),
        },
        repositories: vec![],
        models: SpaceModels::default(),
        sources: vec![],
        extra: Extra::new(),
    };
    let tracker = TrackerConfig::new(prefix);
    (space, tracker)
}

pub(crate) fn insert_space(
    db: &Connection,
    space: &Space,
    local: &SpaceLocal,
    tracker: Option<&TrackerConfig>,
) -> Result<()> {
    db.execute(
        "INSERT INTO spaces(id,data,local) VALUES (?1,?2,?3)",
        params![space.id.as_str(), encode(space)?, encode(local)?],
    )?;
    if let Some(tracker) = tracker {
        db.execute(
            "INSERT INTO trackers VALUES (?1,?2)",
            params![space.storage.repository.key(), encode(tracker)?],
        )?;
    }
    Ok(())
}

/// Personal is found by its name in any casing, so a renamed `personal` is never doubled.
fn is_personal(space: &Space) -> bool {
    space.name.eq_ignore_ascii_case(PERSONAL)
}

/// The login `gh` is signed in to GitHub as, or `None` when it cannot say.
pub(crate) fn gh_login(tools: &publish::Tools, home: &Path) -> Option<String> {
    let user = github_access::gh_json(tools, home, "user").ok()?;
    user["login"].as_str().map(str::to_owned)
}

/// The Space counted most, the first of those on a tie, or `None` when none is counted.
fn most(counts: impl Iterator<Item = (SpaceId, usize)>) -> Option<SpaceId> {
    let counts: Vec<(SpaceId, usize)> = counts.filter(|(_, count)| *count > 0).collect();
    let highest = counts.iter().map(|(_, count)| *count).max()?;
    counts
        .into_iter()
        .find(|(_, count)| *count == highest)
        .map(|(space, _)| space)
}

pub(crate) fn validate(space: &Space) -> Result<()> {
    text(&space.name, 128)?;
    ensure!(
        space.description.len() <= 4096,
        "A Space's description is at most 4,096 bytes"
    );
    ensure!(
        space.instructions.len() <= MAX_TEXT_BYTES,
        "A Space's instructions are at most {MAX_TEXT_BYTES} bytes"
    );
    for (at, identity) in space.repositories.iter().enumerate() {
        validate_repository_identity(identity)?;
        ensure!(
            !space.repositories[..at]
                .iter()
                .any(|other| other.eq_ignore_ascii_case(identity)),
            "{identity} is listed twice"
        );
    }
    if let Some(brain) = &space.brain {
        if let BrainRepository::GitHub { owner, name, .. } = &brain.repository {
            validate_repository_identity(&format!("{owner}/{name}"))?;
        }
        let folder = Path::new(&brain.folder);
        ensure!(
            !brain.folder.is_empty()
                && folder
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_))),
            "A brain's folder is a folder inside it, such as second-brain"
        );
    }
    for source in &space.sources {
        let (name, allowed) = match source {
            Source::Mcp { server, tools, .. } => (server, tools),
            Source::Cli {
                command, prefixes, ..
            } => (command, prefixes),
        };
        ensure!(
            !name.trim().is_empty() && allowed.iter().all(|a| !a.trim().is_empty()),
            "Name each source and what it may call"
        );
    }
    Ok(())
}

impl Host {
    /// In the order they were created.
    pub fn spaces(&self) -> Result<Vec<SpaceRecord>> {
        Ok(self
            .db
            .prepare("SELECT data,local FROM spaces ORDER BY rowid")?
            .query_map([], |r| {
                Ok(SpaceRecord {
                    space: decode(r, 0)?,
                    local: decode(r, 1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// Every Space's id, in the order they were created.
    pub fn space_ids(&self) -> Result<Vec<SpaceId>> {
        Ok(self.spaces()?.into_iter().map(|r| r.space.id).collect())
    }
    pub fn space(&self, id: &SpaceId) -> Result<SpaceRecord> {
        self.find_space(id)?.context("Space not found")
    }
    pub(crate) fn find_space(&self, id: &SpaceId) -> Result<Option<SpaceRecord>> {
        Ok(self
            .db
            .query_row(
                "SELECT data,local FROM spaces WHERE id=?1",
                [id.as_str()],
                |r| {
                    Ok(SpaceRecord {
                        space: decode(r, 0)?,
                        local: decode(r, 1)?,
                    })
                },
            )
            .optional()?)
    }
    /// Who owns a session: its project, unless it records a Space or a brain.
    pub fn session_owner(&self, id: &str) -> Result<SessionOwner> {
        let session = self.session(id)?;
        Ok(session
            .owner
            .unwrap_or(SessionOwner::Project(session.project_id)))
    }
    pub fn tracker(&self, space: &Space) -> Result<TrackerConfig> {
        self.db
            .query_row(
                "SELECT data FROM trackers WHERE storage=?1",
                [space.storage.repository.key()],
                |r| decode(r, 0),
            )
            .context("The Space's storage has no tracker")
    }
    /// The Space last used for a new project, or the first one.
    pub fn default_space(&self) -> Result<SpaceId> {
        self.db
            .query_row(
                "SELECT id FROM spaces ORDER BY used_at DESC, rowid LIMIT 1",
                [],
                |r| r.get(0).map(SpaceId),
            )
            .optional()?
            .context("Create a Space first")
    }
    /// Records that a new project went into `space`, so New project defaults to it.
    pub(crate) fn mark_space_used(db: &Connection, space: &SpaceId) -> Result<()> {
        let changed = db.execute(
            "UPDATE spaces SET used_at=(SELECT MAX(used_at)+1 FROM spaces) WHERE id=?1",
            [space.as_str()],
        )?;
        ensure!(changed == 1, "Space not found");
        Ok(())
    }
    pub(crate) fn ensure_unused_name(&self, name: &str, except: Option<&SpaceId>) -> Result<()> {
        let clash = self.spaces()?.into_iter().any(|record| {
            Some(&record.space.id) != except && record.space.name.eq_ignore_ascii_case(name.trim())
        });
        ensure!(!clash, "Another Space is named {}", name.trim());
        Ok(())
    }
    /// A new Space with local storage and a local tracker. Without `prefix` its tracker gets the
    /// one proposed from its name.
    pub fn create_space(
        &mut self,
        name: &str,
        description: &str,
        instructions: &str,
        prefix: Option<&str>,
    ) -> Result<SpaceRecord> {
        self.ensure_unused_name(name, None)?;
        let mut prefixes = prefixes(&self.db)?;
        if let Some(prefix) = prefix {
            validate_prefix(prefix)?;
            ensure!(
                !prefixes.iter().any(|p| p == prefix),
                "Another storage repository on this Mac uses the prefix {prefix}; choose one no other uses"
            );
        }
        let (mut space, mut tracker) = new_space(name.trim(), &mut prefixes);
        space.description = description.into();
        space.instructions = instructions.into();
        if let Some(prefix) = prefix {
            tracker.prefix = prefix.into();
        }
        validate(&space)?;
        let local = SpaceLocal::default();
        // A savepoint, so removing a Space can recreate Personal within its own change.
        self.atomically(|host| {
            insert_space(&host.db, &space, &local, Some(&tracker))?;
            host.write_space_file(&space).inspect_err(|_| {
                let _ = fs::remove_dir_all(host.storage_folder(&space.storage.repository));
            })
        })?;
        Ok(SpaceRecord { space, local })
    }
    /// Replaces a Space's definition. Its storage stays where it is.
    pub fn update_space(&mut self, space: Space) -> Result<SpaceRecord> {
        let current = self.space(&space.id)?;
        ensure!(
            space.storage == current.space.storage,
            "A Space's storage cannot be changed here"
        );
        validate(&space)?;
        self.ensure_unused_name(&space.name, Some(&space.id))?;
        if let Some(SpaceBrain {
            repository: BrainRepository::GitHub { owner, name, .. },
            ..
        }) = &space.brain
        {
            self.refuse_second_use(&format!("{owner}/{name}"), storage::Use::Brain)?;
        }
        let space = Space {
            name: space.name.trim().into(),
            ..space
        };
        self.atomically(|host| {
            host.db.execute(
                "UPDATE spaces SET data=?2 WHERE id=?1",
                params![space.id.as_str(), encode(&space)?],
            )?;
            host.write_space_file(&space)
        })?;
        Ok(SpaceRecord {
            space,
            local: current.local,
        })
    }
    /// Removes a Space that no open project belongs to and that is not the last one, moving its
    /// archived projects to Personal, which is created if it was removed. Its storage and brain
    /// repositories are left as they are.
    pub fn remove_space(&mut self, id: &SpaceId) -> Result<()> {
        let removed = self.space(id)?.space;
        let spaces = self.spaces()?;
        ensure!(
            spaces.len() > 1,
            "At least one Space must exist; {} is the last",
            removed.name
        );
        let projects: Vec<Project> = (self.projects()?.into_iter())
            .filter(|p| &p.space == id)
            .collect();
        let mut open = vec![];
        for project in &projects {
            if !self.project_archived(&project.id)? {
                open.push(project.name.as_str());
            }
        }
        ensure!(
            open.is_empty(),
            "{} still holds open projects: {}. Move or archive them first",
            removed.name,
            open.join(", ")
        );
        ensure!(
            projects.is_empty() || !is_personal(&removed),
            "Personal holds archived projects, which have nowhere else to go"
        );
        self.atomically(|host| {
            if !projects.is_empty() {
                let personal = match spaces.iter().find(|r| is_personal(&r.space)) {
                    Some(record) => record.space.id.clone(),
                    None => host.create_space(PERSONAL, "", "", None)?.space.id,
                };
                for mut project in projects {
                    project.space = personal.clone();
                    host.db.execute(
                        "UPDATE projects SET data=?2 WHERE id=?1",
                        params![project.id, encode(&project)?],
                    )?;
                }
            }
            host.db
                .execute("DELETE FROM spaces WHERE id=?1", [id.as_str()])?;
            let storage = removed.storage.repository.key();
            let shared =
                (host.spaces()?.iter()).any(|r| r.space.storage.repository.key() == storage);
            if !shared {
                host.db
                    .execute("DELETE FROM trackers WHERE storage=?1", [storage])?;
            }
            Ok(())
        })
    }
    /// Moves a project to another Space: its later entries go to that Space's brain, its earlier
    /// ones stay where they are, and its repositories do not change.
    pub fn move_project_to_space(&mut self, project: &str, space: &SpaceId) -> Result<Project> {
        let mut project = self.project(project)?;
        let target = self.space(space)?.space;
        if project.space == target.id {
            return Ok(project);
        }
        project.space = target.id;
        let tx = self.db.transaction()?;
        tx.execute(
            "UPDATE projects SET data=?2 WHERE id=?1",
            params![project.id, encode(&project)?],
        )?;
        Self::event(&tx, &project.id, None, "space_changed", &target.name)?;
        tx.commit()?;
        Ok(project)
    }
    /// Sets the brain of the project's Space to the local folder `path`, keeping the folder in
    /// it that a local brain already names.
    pub fn attach_brain(&mut self, project: &str, path: &str) -> Result<()> {
        let project = self.project(project)?;
        self.set_space_brain(&project.space, path, Some(&project.id))
    }
    /// Sets the Space's brain to the local folder `path`, keeping the folder in it that a local
    /// brain already names; `project`, when it was set from one, records it there.
    pub fn set_space_brain(
        &mut self,
        space: &SpaceId,
        path: &str,
        project: Option<&str>,
    ) -> Result<()> {
        let path = fs::canonicalize(path)
            .context("Brain directory must already exist; setup is explicit")?;
        ensure!(path.is_dir(), "Brain attachment must be a directory");
        let path: String = path.to_str().context("Brain path is not UTF-8")?.into();
        let SpaceRecord {
            mut space,
            mut local,
        } = self.space(space)?;
        // A local brain keeps its folder and the keys this release does not know.
        space.brain = Some(match space.brain {
            Some(
                brain @ SpaceBrain {
                    repository: BrainRepository::Local { .. },
                    ..
                },
            ) => brain,
            _ => SpaceBrain {
                repository: BrainRepository::local(),
                folder: SpaceBrain::DEFAULT_FOLDER.into(),
                extra: Extra::new(),
            },
        });
        local.brain_path = Some(path.clone());
        self.atomically(|host| {
            host.db.execute(
                "UPDATE spaces SET data=?2,local=?3 WHERE id=?1",
                params![space.id.as_str(), encode(&space)?, encode(&local)?],
            )?;
            if let Some(project) = project {
                Self::event(&host.db, project, None, "brain_attached", &path)?;
            }
            host.write_space_file(&space)
        })
    }
    /// The workspace repositories whose GitHub name `space` lists, and the ones it lists that
    /// are not cloned on this Mac.
    pub fn space_repositories(&self, space: &SpaceId) -> Result<SpaceRepositories> {
        let space = self.space(space)?.space;
        let here = self.repositories()?;
        let listed =
            |repository: &Repository| space.repositories.iter().any(|i| repository.is_github(i));
        Ok(SpaceRepositories {
            repository_ids: (here.iter().filter(|r| listed(r)))
                .map(|r| r.id.clone())
                .collect(),
            not_cloned: (space.repositories.iter())
                .filter(|identity| !here.iter().any(|r| r.is_github(identity)))
                .cloned()
                .collect(),
            space_id: space.id,
        })
    }
    /// What New project starts with: `space`, or the Space last used for a new project, and
    /// its repositories on this Mac.
    pub fn new_project_defaults(&self, space: Option<SpaceId>) -> Result<SpaceRepositories> {
        let space = match space {
            Some(space) => space,
            None => self.default_space()?,
        };
        self.space_repositories(&space)
    }
    /// Puts `repository`, named `owner/name`, in exactly `spaces`, writing only the Spaces whose
    /// list changes, each to its `space.yaml` too, so membership travels with the Space. The
    /// workspace repository itself is left as it is.
    pub fn set_repository_spaces(&mut self, repository: &str, spaces: &[SpaceId]) -> Result<()> {
        validate_repository_identity(repository)?;
        for space in spaces {
            self.space(space)?;
        }
        self.atomically(|host| {
            for SpaceRecord { mut space, .. } in host.spaces()? {
                let listed = (space.repositories.iter())
                    .position(|identity| identity.eq_ignore_ascii_case(repository));
                match (spaces.contains(&space.id), listed) {
                    (true, None) => space.repositories.push(repository.into()),
                    (false, Some(at)) => {
                        space.repositories.remove(at);
                    }
                    _ => continue,
                }
                host.db.execute(
                    "UPDATE spaces SET data=?2 WHERE id=?1",
                    params![space.id.as_str(), encode(&space)?],
                )?;
                host.write_space_file(&space)?;
            }
            Ok(())
        })
    }
    /// The Space to suggest for `repository` when it is added: the one listing the most
    /// repositories of its GitHub owner, or `None` for No Space. The owner only suggests.
    pub fn suggest_repository_space(&self, repository: &str) -> Result<Option<SpaceId>> {
        validate_repository_identity(repository)?;
        let owner = repository_owner(repository);
        Ok(most(self.spaces()?.into_iter().map(|record| {
            let count = (record.space.repositories.iter())
                .filter(|identity| repository_owner(identity).eq_ignore_ascii_case(owner))
                .count();
            (record.space.id, count)
        })))
    }
    /// The upgrade's suggestions for the repositories it found that are still here and on
    /// GitHub, by owner: the Space whose projects use that owner's repositories most, else
    /// Personal for the human's own `gh` login, else No Space. They change nothing and run no
    /// `gh`: until the read at start has found the login, there is no Personal suggestion.
    pub(crate) fn repository_suggestions(&self) -> Result<Vec<RepositorySuggestion>> {
        let found = self.found_repositories()?;
        let workspace = self.repositories()?;
        let suggested: Vec<(&Repository, String)> = (workspace.iter())
            .filter(|r| found.contains(&r.id))
            .filter_map(|r| Some((r, r.github_identity()?)))
            .collect();
        if suggested.is_empty() {
            return Ok(vec![]);
        }
        let owner_of = |id: &str| {
            (workspace.iter())
                .find(|r| r.id == id)
                .and_then(Repository::github_identity)
                .map(|identity| repository_owner(&identity).to_ascii_lowercase())
        };
        let uses: Vec<(SpaceId, String)> = (self.repository_uses()?.into_iter())
            .filter_map(|(space, repository)| Some((space, owner_of(&repository)?)))
            .collect();
        let spaces = self.spaces()?;
        let personal = match spaces.iter().find(|r| is_personal(&r.space)) {
            Some(record) => SuggestedSpace::Space(record.space.id.clone()),
            None => SuggestedSpace::NewPersonal,
        };
        let login = self.gh_login.as_ref();
        Ok(suggested
            .into_iter()
            .map(|(repository, identity)| {
                let owner = repository_owner(&identity).to_ascii_lowercase();
                let by_projects = most(spaces.iter().map(|record| {
                    let count = (uses.iter())
                        .filter(|(space, used)| space == &record.space.id && used == &owner)
                        .count();
                    (record.space.id.clone(), count)
                }));
                let own = login.is_some_and(|l| l.eq_ignore_ascii_case(&owner));
                RepositorySuggestion {
                    repository_id: repository.id.clone(),
                    space: (by_projects.map(SuggestedSpace::Space))
                        .or_else(|| own.then(|| personal.clone())),
                    repository: identity,
                }
            })
            .collect())
    }
    /// Whether the upgrade's suggestions wait on the human's `gh` login, which no read has
    /// found yet.
    pub(crate) fn wants_gh_login(&self) -> Result<bool> {
        Ok(self.gh_login.is_none() && !self.found_repositories()?.is_empty())
    }
    /// Keeps the human's `gh` login for the host's lifetime, once one is found.
    pub(crate) fn record_gh_login(&mut self, login: Option<String>) {
        if login.is_some() {
            self.gh_login = login;
        }
    }
    /// Applies the human's choices for the upgrade's suggestions, and stops offering them.
    /// Choosing Personal when it is missing creates it.
    pub fn confirm_repository_suggestions(&mut self, choices: &[RepositorySpaces]) -> Result<()> {
        self.atomically(|host| {
            for choice in choices {
                let mut spaces = choice.space_ids.clone();
                if choice.new_personal {
                    let personal = host.spaces()?.into_iter().find(|r| is_personal(&r.space));
                    spaces.push(match personal {
                        Some(record) => record.space.id,
                        None => host.create_space(PERSONAL, "", "", None)?.space.id,
                    });
                }
                host.set_repository_spaces(&choice.repository, &spaces)?;
            }
            host.db.execute("DELETE FROM repository_suggestions", [])?;
            Ok(())
        })
    }
    /// The workspace repositories the upgrade found, while their suggestions wait.
    fn found_repositories(&self) -> Result<Vec<String>> {
        Ok(self
            .db
            .prepare("SELECT repository_id FROM repository_suggestions")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// Each project's Space with each repository it uses: the ones its selection names, or,
    /// for a project that may use every repository, the ones its tickets were in.
    fn repository_uses(&self) -> Result<Vec<(SpaceId, String)>> {
        let ticketed: Vec<(String, String)> = (self.db.prepare(
            "SELECT DISTINCT s.project_id,json_extract(t.data,'$.repository_id') FROM tickets t JOIN sessions s ON s.id=t.coordinator_id",
        )?)
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
        let mut uses = vec![];
        for project in self.projects()? {
            let used: Vec<String> = match project.repositories {
                Some(ids) => ids,
                None => (ticketed.iter())
                    .filter(|(id, _)| id == &project.id)
                    .map(|(_, repository)| repository.clone())
                    .collect(),
            };
            uses.extend(used.into_iter().map(|r| (project.space.clone(), r)));
        }
        Ok(uses)
    }
    /// A project is archived when its coordinator is.
    fn project_archived(&self, project: &str) -> Result<bool> {
        Ok(self.db.query_row(
            &format!(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE project_id=?1 AND parent_id IS NULL AND role=?2 AND {ARCHIVED_FLAG}=1)"
            ),
            params![project, tag(&Role::ProjectOrchestrator)?],
            |r| r.get(0),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(brains: &[&str]) -> Vec<String> {
        upgraded_names(&brains.iter().map(|b| b.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn a_project_is_refused_when_no_space_exists() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.db.execute("DELETE FROM spaces", []).unwrap();
        let error = host.create_project("Calls").unwrap_err();
        assert!(format!("{error:#}").contains("Create a Space first"));
        let error = (host.execute(Command::CreateProject {
            name: "Calls".into(),
            space_id: None,
        }))
        .unwrap_err();
        assert!(format!("{error:#}").contains("Create a Space first"));
        assert!(host.projects().unwrap().is_empty());
    }

    /// A workspace repository whose `origin` is the GitHub repository `identity`.
    fn on_github(host: &Host, identity: &str) -> Repository {
        let (owner, name) = identity.split_once('/').unwrap();
        let repository = Repository {
            id: new_id(),
            name: name.into(),
            path: format!("/dev/{identity}"),
            base: "origin/main".into(),
            github: Some(GitHubRepository {
                owner: owner.into(),
                name: name.into(),
                facts: None,
                problem: None,
            }),
            origin_problem: None,
            checks: None,
        };
        host.db
            .execute(
                "INSERT INTO repositories VALUES (?1,?2,?3)",
                params![repository.id, repository.path, encode(&repository).unwrap()],
            )
            .unwrap();
        repository
    }

    /// A `gh` signed in as `login`, counting its calls in `calls`.
    fn counted_signed_in(
        login: &'static str,
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> publish::Tools {
        publish::Tools {
            gh: std::sync::Arc::new(move |_, args| {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                assert_eq!(args.last(), Some(&"user"), "{args:?}");
                Ok(runtime::GhAnswer {
                    status: std::os::unix::process::ExitStatusExt::from_raw(0),
                    output: json!({"login": login}).to_string(),
                    error: String::new(),
                })
            }),
            ..publish::Tools::default()
        }
    }

    #[test]
    fn the_upgrade_suggests_spaces_by_projects_then_the_gh_login_and_writes_nothing() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        let personal = host.default_space().unwrap();
        let sage = host.create_space("Sage", "", "", None).unwrap().space.id;
        let [mobile, api, dotfiles, scratch] = [
            "Sage-Social/mobile-app",
            "Sage-Social/api",
            "MHohlios/dotfiles",
            "octo/scratch",
        ]
        .map(|identity| on_github(&host, identity));
        for name in ["Calls", "Billing", "Watch"] {
            let project = host.create_project_in(name, &sage).unwrap();
            host.set_project_repositories(
                &project.id,
                Some(vec![mobile.id.clone(), api.id.clone()]),
            )
            .unwrap();
        }
        // One Personal project uses a Sage-Social repository, fewer than Sage's three.
        let notes = host.create_project_in("Notes", &personal).unwrap();
        host.set_project_repositories(&notes.id, Some(vec![api.id.clone()]))
            .unwrap();
        // The upgrade found these repositories.
        host.db
            .execute(
                "INSERT INTO repository_suggestions SELECT id FROM repositories",
                [],
            )
            .unwrap();
        let before = host.spaces().unwrap();
        host.record_gh_login(Some("mhohlios".into()));

        let suggestions = host.repository_suggestions().unwrap();
        let suggested = |repository: &Repository| {
            (suggestions.iter())
                .find(|s| s.repository_id == repository.id)
                .unwrap()
                .space
                .clone()
        };
        let sage = Some(SuggestedSpace::Space(sage));
        assert_eq!(suggestions.len(), 4);
        assert_eq!(suggested(&mobile), sage);
        assert_eq!(suggested(&api), sage);
        assert_eq!(
            suggested(&dotfiles),
            Some(SuggestedSpace::Space(personal)),
            "the human's own login"
        );
        assert_eq!(suggested(&scratch), None, "No Space");
        assert_eq!(host.spaces().unwrap(), before, "suggesting writes nothing");

        // Confirming nothing writes nothing, and the suggestions are not offered again.
        host.confirm_repository_suggestions(&[RepositorySpaces {
            repository: "octo/scratch".into(),
            space_ids: vec![],
            new_personal: false,
        }])
        .unwrap();
        assert_eq!(host.spaces().unwrap(), before);
        assert!(host.repository_suggestions().unwrap().is_empty());
    }

    #[test]
    fn confirmed_suggestions_list_the_repositories_in_their_spaces() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        let personal = host.default_space().unwrap();
        let dotfiles = on_github(&host, "MHohlios/dotfiles");
        host.db
            .execute(
                "INSERT INTO repository_suggestions VALUES (?1)",
                [&dotfiles.id],
            )
            .unwrap();
        // A signed-out gh leaves no login, so there is no Personal suggestion.
        let signed_out = publish::Tools {
            gh: std::sync::Arc::new(|_, _| anyhow::bail!("gh is not installed")),
            ..publish::Tools::default()
        };
        host.record_gh_login(gh_login(&signed_out, home.path()));
        let suggestions = host.repository_suggestions().unwrap();
        assert_eq!(suggestions[0].space, None);
        assert_eq!(suggestions[0].repository, "MHohlios/dotfiles");

        host.confirm_repository_suggestions(&[RepositorySpaces {
            repository: "MHohlios/dotfiles".into(),
            space_ids: vec![personal.clone()],
            new_personal: false,
        }])
        .unwrap();
        assert_eq!(
            host.space(&personal).unwrap().space.repositories,
            ["MHohlios/dotfiles"]
        );
        assert!(host.repository_suggestions().unwrap().is_empty());
    }

    #[test]
    fn suggestions_run_no_gh_and_use_the_login_the_read_at_start_found() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        let dotfiles = on_github(&host, "MHohlios/dotfiles");
        host.db
            .execute(
                "INSERT INTO repository_suggestions VALUES (?1)",
                [&dotfiles.id],
            )
            .unwrap();
        let calls = std::sync::Arc::default();
        let gh = counted_signed_in("MHohlios", std::sync::Arc::clone(&calls));
        let calls = || calls.load(std::sync::atomic::Ordering::SeqCst);
        let personal = Some(SuggestedSpace::Space(host.default_space().unwrap()));
        assert!(host.wants_gh_login().unwrap());
        // Before the login is known: no gh, and no Personal suggestion.
        for _ in 0..2 {
            assert_eq!(host.repository_suggestions().unwrap()[0].space, None);
        }
        assert_eq!(calls(), 0);

        // The service's read at start, off the host thread.
        host.record_gh_login(gh_login(&gh, home.path()));
        assert_eq!(calls(), 1);
        for _ in 0..2 {
            assert_eq!(host.repository_suggestions().unwrap()[0].space, personal);
        }
        assert_eq!(calls(), 1);
        assert!(!host.wants_gh_login().unwrap());
    }

    #[test]
    fn personal_is_found_in_any_casing() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        let mut personal = host.spaces().unwrap().remove(0).space;
        personal.name = "personal".into();
        host.update_space(personal.clone()).unwrap();
        let dotfiles = on_github(&host, "MHohlios/dotfiles");
        host.db
            .execute(
                "INSERT INTO repository_suggestions VALUES (?1)",
                [&dotfiles.id],
            )
            .unwrap();
        host.record_gh_login(Some("MHohlios".into()));

        let suggestions = host.repository_suggestions().unwrap();
        assert_eq!(
            suggestions[0].space,
            Some(SuggestedSpace::Space(personal.id.clone()))
        );
        host.confirm_repository_suggestions(&[RepositorySpaces {
            repository: "MHohlios/dotfiles".into(),
            space_ids: vec![],
            new_personal: true,
        }])
        .unwrap();
        let spaces = host.spaces().unwrap();
        assert_eq!(spaces.len(), 1, "no second Personal");
        assert_eq!(spaces[0].space.repositories, ["MHohlios/dotfiles"]);
    }

    #[test]
    fn personal_is_suggested_for_the_login_and_created_only_when_confirmed() {
        let home = tempfile::tempdir().unwrap();
        let project = {
            let mut host = Host::open_set_up(home.path()).unwrap();
            on_github(&host, "MHohlios/dotfiles");
            host.create_project("Notes").unwrap().id
        };
        // Rewind to schema 8, where the only project has a brain, so the upgrade makes no
        // Personal.
        let db = Connection::open(home.path().join(STORE_FILE)).unwrap();
        db.execute_batch(
            "DROP TABLE spaces; DROP TABLE trackers; DROP TABLE repository_suggestions;
             ALTER TABLE work_logs DROP COLUMN space_id; PRAGMA user_version = 8;",
        )
        .unwrap();
        db.execute(
            "UPDATE projects SET data=json_set(json_remove(data,'$.space'),'$.brain','/u/dev/brain') WHERE id=?1",
            [&project],
        )
        .unwrap();
        drop(db);
        let mut host = Host::open_set_up(home.path()).unwrap();
        let names = |host: &Host| -> Vec<String> {
            (host.spaces().unwrap().into_iter())
                .map(|r| r.space.name)
                .collect()
        };
        assert_eq!(names(&host), ["brain"]);
        host.record_gh_login(Some("MHohlios".into()));

        let suggestions = host.repository_suggestions().unwrap();
        assert_eq!(suggestions[0].space, Some(SuggestedSpace::NewPersonal));
        assert_eq!(
            names(&host),
            ["brain"],
            "nothing is created before confirming"
        );

        host.confirm_repository_suggestions(&[RepositorySpaces {
            repository: "MHohlios/dotfiles".into(),
            space_ids: vec![],
            new_personal: true,
        }])
        .unwrap();
        let personal = &host.spaces().unwrap()[1];
        assert_eq!(personal.space.name, PERSONAL);
        assert_eq!(personal.space.repositories, ["MHohlios/dotfiles"]);
        assert_eq!(personal.space.brain, None);
        assert!(matches!(
            personal.space.storage.repository,
            StorageRepository::Local { .. }
        ));
    }

    #[test]
    fn upgraded_names_are_unique() {
        assert_eq!(
            names(&["/u/dev/sage/team-brain", "/u/dev/vyttle/vyttle-brain"]),
            ["team-brain", "vyttle-brain"]
        );
        assert_eq!(
            names(&["/u/dev/sage/brain", "/u/dev/vyttle/brain"]),
            ["sage/brain", "vyttle/brain"]
        );
        assert_eq!(names(&["/u/notes/Personal"]), ["notes/Personal"]);
        assert_eq!(
            names(&["/a/sage/brain", "/b/sage/brain", "/c/vyttle/Brain"]),
            ["sage/brain", "sage/brain 2", "vyttle/Brain"]
        );
    }
}
