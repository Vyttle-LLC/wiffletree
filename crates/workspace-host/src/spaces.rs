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
/// Space, when it gets "Personal" so a new project has somewhere to go. Pushes nothing.
pub(crate) fn migrate(db: &Connection) -> Result<()> {
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
        insert_space(&tx, &space, &SpaceLocal::default(), &tracker)?;
        tx.commit()?;
    }
    db.execute_batch("PRAGMA user_version = 9;")?;
    Ok(())
}

/// The prefixes of the trackers on this Mac.
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
            repository: BrainRepository::Local,
            folder: SpaceBrain::DEFAULT_FOLDER.into(),
        });
        insert_space(db, &space.0, &local, &space.1)?;
        space_of_brain.insert(brain.clone(), space.0.id);
    }
    let personal = if brains.is_empty() || projects.iter().any(|(_, p)| brain_of(p).is_none()) {
        let (space, tracker) = new_space(PERSONAL, &mut prefixes);
        insert_space(db, &space, &SpaceLocal::default(), &tracker)?;
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
    };
    let tracker = TrackerConfig {
        format: TrackerConfig::FORMAT,
        prefix,
    };
    (space, tracker)
}

fn insert_space(
    db: &Connection,
    space: &Space,
    local: &SpaceLocal,
    tracker: &TrackerConfig,
) -> Result<()> {
    db.execute(
        "INSERT INTO spaces(id,data,local) VALUES (?1,?2,?3)",
        params![space.id.as_str(), encode(space)?, encode(local)?],
    )?;
    db.execute(
        "INSERT INTO trackers VALUES (?1,?2)",
        params![space.storage.repository.key(), encode(tracker)?],
    )?;
    Ok(())
}

fn validate(space: &Space) -> Result<()> {
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
        if let BrainRepository::GitHub { owner, name } = &brain.repository {
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
            Source::Mcp { server, tools } => (server, tools),
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
    /// Who owns a session: its project, unless it records a Space or a brain as `owner`. No
    /// role runs outside projects yet, so no session the host creates records one.
    pub fn session_owner(&self, id: &str) -> Result<SessionOwner> {
        let (project, owner): (String, Option<String>) = self
            .db
            .query_row(
                "SELECT project_id,json_extract(data,'$.owner') FROM sessions WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .context("Session not found")?;
        Ok(match owner {
            Some(owner) => serde_json::from_str(&owner)?,
            None => SessionOwner::Project(project),
        })
    }
    /// The tracker of a Space's storage repository.
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
    fn ensure_unused_name(&self, name: &str, except: Option<&SpaceId>) -> Result<()> {
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
        let tx = self.db.savepoint()?;
        insert_space(&tx, &space, &local, &tracker)?;
        tx.commit()?;
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
        let space = Space {
            name: space.name.trim().into(),
            ..space
        };
        self.db.execute(
            "UPDATE spaces SET data=?2 WHERE id=?1",
            params![space.id.as_str(), encode(&space)?],
        )?;
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
        let is_personal = |space: &Space| space.name == PERSONAL;
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
        let path = fs::canonicalize(path)
            .context("Brain directory must already exist; setup is explicit")?;
        ensure!(path.is_dir(), "Brain attachment must be a directory");
        let path: String = path.to_str().context("Brain path is not UTF-8")?.into();
        let SpaceRecord {
            mut space,
            mut local,
        } = self.space(&project.space)?;
        let folder = match space.brain {
            Some(SpaceBrain {
                repository: BrainRepository::Local,
                folder,
            }) => folder,
            _ => SpaceBrain::DEFAULT_FOLDER.into(),
        };
        space.brain = Some(SpaceBrain {
            repository: BrainRepository::Local,
            folder,
        });
        local.brain_path = Some(path.clone());
        let tx = self.db.transaction()?;
        tx.execute(
            "UPDATE spaces SET data=?2,local=?3 WHERE id=?1",
            params![space.id.as_str(), encode(&space)?, encode(&local)?],
        )?;
        Self::event(&tx, &project.id, None, "brain_attached", &path)?;
        tx.commit()?;
        Ok(())
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
