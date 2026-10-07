use crate::*;
use std::collections::{BTreeMap, BTreeSet};

/// Converts per-project repository attachments (schema 3) into one workspace list (schema 4).
/// Attachments of the same path collapse into the earliest one, and sessions follow it. Each
/// project keeps exactly the repositories it had; a project that had none uses them all.
pub(crate) fn migrate_to_workspace(db: &Connection) -> Result<()> {
    let per_project: i64 = db.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('repositories') WHERE name='project_id'",
        [],
        |r| r.get(0),
    )?;
    if per_project > 0 {
        let tx = db.unchecked_transaction()?;
        let rows = tx
            .prepare("SELECT id, project_id, path, data FROM repositories ORDER BY rowid")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut survivor_by_path = BTreeMap::new();
        let mut survivors = Vec::new();
        let mut renamed = BTreeMap::new();
        let mut chosen: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (id, project, path, data) in rows {
            let survivor = survivor_by_path
                .entry(path.clone())
                .or_insert_with(|| {
                    survivors.push((id.clone(), path, data));
                    id.clone()
                })
                .clone();
            if survivor != id {
                renamed.insert(id, survivor.clone());
            }
            let ids = chosen.entry(project).or_default();
            if !ids.contains(&survivor) {
                ids.push(survivor);
            }
        }
        let projects = tx
            .prepare("SELECT data FROM projects")?
            .query_map([], |r| decode::<Project>(r, 0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for mut project in projects {
            project.repositories = chosen.remove(&project.id);
            tx.execute(
                "UPDATE projects SET data=?2 WHERE id=?1",
                params![project.id, encode(&project)?],
            )?;
        }
        let sessions = tx
            .prepare("SELECT data FROM sessions")?
            .query_map([], |r| decode::<Session>(r, 0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for mut session in sessions {
            if let Some(survivor) = session
                .repository_id
                .as_ref()
                .and_then(|id| renamed.get(id))
            {
                session.repository_id = Some(survivor.clone());
                tx.execute(
                    "UPDATE sessions SET data=?2 WHERE id=?1",
                    params![session.id, encode(&session)?],
                )?;
            }
        }
        tx.execute_batch(
            "CREATE TABLE workspace_repositories (id TEXT PRIMARY KEY, path TEXT NOT NULL UNIQUE, data TEXT NOT NULL);",
        )?;
        for (id, path, data) in survivors {
            let repository: Repository = serde_json::from_str(&data)?;
            tx.execute(
                "INSERT INTO workspace_repositories VALUES (?1,?2,?3)",
                params![id, path, encode(&repository)?],
            )?;
        }
        tx.execute_batch(
            "DROP TABLE repositories; ALTER TABLE workspace_repositories RENAME TO repositories;",
        )?;
        tx.commit()?;
    }
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS repository_roots (path TEXT PRIMARY KEY);
         CREATE TABLE IF NOT EXISTS dismissed_repositories (path TEXT PRIMARY KEY);
         PRAGMA user_version = 4;",
    )?;
    Ok(())
}

fn folder_path(value: &str) -> Result<PathBuf> {
    let value = value.trim().strip_suffix("/*").unwrap_or(value.trim());
    text(value, 4096)?;
    let path = if value == "~" || value.starts_with("~/") {
        PathBuf::from(std::env::var_os("HOME").context("Home directory unavailable")?)
            .join(value.strip_prefix("~/").unwrap_or(""))
    } else {
        PathBuf::from(value)
    };
    fs::canonicalize(path).context("Folder does not exist")
}

fn candidate(path: &Path, known: &BTreeSet<String>) -> Result<RepositoryCandidate> {
    let path = fs::canonicalize(path)?;
    let root = runtime::git_output(&path, &["rev-parse", "--show-toplevel"])?;
    ensure!(
        path == fs::canonicalize(root.trim())?,
        "Expected a Git repository root"
    );
    let remote_head = runtime::git_output(
        &path,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .ok();
    let base = remote_head
        .iter()
        .map(|s| s.trim())
        .chain(["main", "master", "origin/main", "origin/master", "HEAD"])
        .find(|base| {
            runtime::git_output(
                &path,
                &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
            )
            .is_ok()
        })
        .context("Repository has no committed base")?
        .to_owned();
    let name = path
        .file_name()
        .context("Repository name missing")?
        .to_string_lossy()
        .into_owned();
    let path = path
        .to_str()
        .context("Repository path is not UTF-8")?
        .to_owned();
    Ok(RepositoryCandidate {
        name,
        added: known.contains(&path),
        path,
        base,
    })
}

/// Finds Git repositories in each folder, or the folder itself when it is one. Read-only, so it
/// can run off the host thread; `known` marks repositories already in the workspace.
pub fn discover(folders: &[String], known: &BTreeSet<String>) -> Result<RepositoryDiscovery> {
    ensure!(
        !folders.is_empty() && folders.len() <= 32,
        "Choose 1–32 folders"
    );
    let mut discovery = RepositoryDiscovery::default();
    let mut seen = BTreeSet::new();
    for folder in folders {
        let result = (|| -> Result<()> {
            let root = folder_path(folder)?;
            ensure!(root.is_dir(), "Workspace path must be a folder");
            let paths = if root.join(".git").exists() {
                vec![root]
            } else {
                fs::read_dir(&root)?
                    .map(|entry| entry.map(|e| e.path()))
                    .collect::<std::io::Result<Vec<_>>>()?
            };
            for path in paths {
                if !path.is_dir() || !path.join(".git").exists() {
                    continue;
                }
                match candidate(&path, known) {
                    Ok(repo) if seen.insert(repo.path.clone()) => discovery.repositories.push(repo),
                    Ok(_) => {}
                    Err(error) => discovery
                        .warnings
                        .push(format!("{}: {error:#}", path.display())),
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            discovery.warnings.push(format!("{folder}: {error:#}"));
        }
    }
    discovery
        .repositories
        .sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    Ok(discovery)
}

/// Lists each root's immediate children and runs Git only for repositories not in `skip`, so a
/// refresh with nothing new costs one directory listing per root. Linked worktrees (whose `.git`
/// is a file) and empty repositories are left out; they can still be added by hand.
pub fn scan_roots(
    roots: &[String],
    skip: &BTreeSet<String>,
) -> (Vec<RepositoryCandidate>, Vec<String>) {
    let mut found = Vec::new();
    let mut warnings = Vec::new();
    let mut seen = BTreeSet::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            warnings.push(format!("{root}: folder is unavailable"));
            continue;
        };
        for entry in entries.flatten() {
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if hidden || !entry.path().join(".git").is_dir() {
                continue;
            }
            let Some(path) = fs::canonicalize(entry.path())
                .ok()
                .and_then(|p| p.to_str().map(str::to_owned))
            else {
                continue;
            };
            if skip.contains(&path) || !seen.insert(path.clone()) {
                continue;
            }
            match candidate(Path::new(&path), skip) {
                Ok(repo) => found.push(repo),
                Err(error) if format!("{error:#}").contains("no committed base") => {}
                Err(error) => warnings.push(format!("{path}: {error:#}")),
            }
        }
    }
    (found, warnings)
}

impl Host {
    /// The workspace repositories this project uses.
    pub fn project_repositories(&self, project: &str) -> Result<Vec<Repository>> {
        let project = self.project(project)?;
        Ok(self
            .repositories()?
            .into_iter()
            .filter(|r| project.uses(&r.id))
            .collect())
    }

    /// Adds a repository to the workspace, or updates the base of one it already knows.
    pub fn add_repository(&mut self, path: &str, base: &str) -> Result<Repository> {
        text(base, 256)?;
        ensure!(!base.starts_with('-'), "Invalid base ref");
        let path = fs::canonicalize(path).context("Repository path does not exist")?;
        let root = runtime::git_output(&path, &["rev-parse", "--show-toplevel"])?;
        let root = fs::canonicalize(root.trim())?;
        ensure!(
            path == root,
            "Attach the repository root rather than a subdirectory"
        );
        runtime::git_output(
            &path,
            &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
        )
        .context("Configured base must resolve to a commit")?;
        let path = path
            .to_str()
            .context("Repository path is not UTF-8")?
            .to_owned();
        let known = self
            .db
            .query_row(
                "SELECT data FROM repositories WHERE path=?1",
                [&path],
                |r| decode::<Repository>(r, 0),
            )
            .optional()?;
        let repository = match known {
            Some(known) if known.base == base => return Ok(known),
            Some(known) => Repository {
                base: base.into(),
                ..known
            },
            None => Repository {
                id: new_id(),
                name: root
                    .file_name()
                    .context("Repository name missing")?
                    .to_string_lossy()
                    .into(),
                path,
                base: base.into(),
            },
        };
        self.db.execute(
            "INSERT INTO repositories VALUES (?1,?2,?3) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            params![repository.id, repository.path, encode(&repository)?],
        )?;
        // Adding a repository by hand overrides an earlier dismissal.
        self.db.execute(
            "DELETE FROM dismissed_repositories WHERE path=?1",
            [&repository.path],
        )?;
        Ok(repository)
    }

    /// Adds a repository to the workspace and makes sure this project uses it.
    pub fn attach_repository(
        &mut self,
        project: &str,
        path: &str,
        base: &str,
    ) -> Result<Repository> {
        self.project(project)?;
        let repository = self.add_repository(path, base)?;
        self.include_repositories(project, std::slice::from_ref(&repository.id))?;
        Self::event(
            &self.db,
            project,
            None,
            "repository_attached",
            &repository.name,
        )?;
        Ok(repository)
    }

    /// Adds repositories to a project's custom selection; a project using all of them already has them.
    fn include_repositories(&mut self, project: &str, ids: &[String]) -> Result<()> {
        let project = self.project(project)?;
        let Some(chosen) = &project.repositories else {
            return Ok(());
        };
        let missing: Vec<_> = ids.iter().filter(|id| !chosen.contains(id)).collect();
        if missing.is_empty() {
            return Ok(());
        }
        let chosen = chosen.iter().chain(missing).cloned().collect();
        self.set_project_repositories(&project.id, Some(chosen))?;
        Ok(())
    }

    /// `None` lets the project use every workspace repository, including ones added later.
    pub fn set_project_repositories(
        &mut self,
        project: &str,
        ids: Option<Vec<String>>,
    ) -> Result<Project> {
        let mut project = self.project(project)?;
        let known: BTreeSet<String> = self.repositories()?.into_iter().map(|r| r.id).collect();
        if let Some(ids) = &ids {
            ensure!(ids.len() <= 1000, "Choose at most 1000 repositories");
            ensure!(
                ids.iter().all(|id| known.contains(id)),
                "Repository not found"
            );
        }
        let mut unique = BTreeSet::new();
        project.repositories = ids.map(|ids| {
            ids.into_iter()
                .filter(|id| unique.insert(id.clone()))
                .collect()
        });
        if let Some(team) = self.sessions()?.into_iter().find(|s| {
            s.project_id == project.id
                && s.role == Role::TaskOrchestrator
                && !s.archived
                && s.repository_id.as_ref().is_some_and(|id| !project.uses(id))
        }) {
            bail!(
                "{} works in a repository you removed; archive that team first",
                team.name
            );
        }
        let detail = match &project.repositories {
            None => "All workspace repositories".to_owned(),
            Some(ids) => format!("{} chosen repositories", ids.len()),
        };
        let tx = self.db.transaction()?;
        tx.execute(
            "UPDATE projects SET data=?2 WHERE id=?1",
            params![project.id, encode(&project)?],
        )?;
        Self::event(&tx, &project.id, None, "repositories_chosen", &detail)?;
        tx.commit()?;
        Ok(project)
    }

    /// Removes a repository from the workspace once no team, archived or not, works in it.
    pub fn remove_repository(&mut self, id: &str) -> Result<()> {
        let repository = self.repository(id)?;
        ensure!(
            !self
                .sessions()?
                .iter()
                .any(|s| s.repository_id.as_deref() == Some(id)),
            "Teams work in {}, so it stays in the workspace",
            repository.name
        );
        let projects = self.projects()?;
        let tx = self.db.transaction()?;
        for mut project in projects {
            if let Some(ids) = &mut project.repositories
                && ids.iter().any(|r| r == id)
            {
                ids.retain(|r| r != id);
                tx.execute(
                    "UPDATE projects SET data=?2 WHERE id=?1",
                    params![project.id, encode(&project)?],
                )?;
            }
        }
        tx.execute("DELETE FROM repositories WHERE id=?1", [id])?;
        tx.commit()?;
        // Otherwise the next refresh of its root folder would add it straight back.
        self.dismiss(&repository.path)
    }

    fn dismiss(&self, path: &str) -> Result<()> {
        text(path, 4096)?;
        self.db.execute(
            "INSERT OR IGNORE INTO dismissed_repositories VALUES (?1)",
            [path],
        )?;
        Ok(())
    }

    pub fn discover_repositories(&self, folders: &[String]) -> Result<RepositoryDiscovery> {
        discover(folders, &self.known_paths()?)
    }

    pub(crate) fn known_paths(&self) -> Result<BTreeSet<String>> {
        Ok(self.repositories()?.into_iter().map(|r| r.path).collect())
    }

    pub fn repository_roots(&self) -> Result<Vec<String>> {
        self.column("SELECT path FROM repository_roots ORDER BY path")
    }

    fn column(&self, sql: &str) -> Result<Vec<String>> {
        Ok(self
            .db
            .prepare(sql)?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Workspace repositories whose folder has gone, kept because teams may still refer to them.
    pub fn missing_repositories(&self) -> Result<Vec<String>> {
        Ok(self
            .repositories()?
            .into_iter()
            .filter(|r| !Path::new(&r.path).exists())
            .map(|r| r.id)
            .collect())
    }

    pub fn remove_repository_root(&mut self, path: &str) -> Result<()> {
        self.db
            .execute("DELETE FROM repository_roots WHERE path=?1", [path])?;
        Ok(())
    }

    /// What a root refresh needs, read up front so the scan itself can run off the host thread.
    pub fn refresh_inputs(&self) -> Result<(Vec<String>, BTreeSet<String>)> {
        let mut skip = self.known_paths()?;
        skip.extend(self.column("SELECT path FROM dismissed_repositories")?);
        Ok((self.repository_roots()?, skip))
    }

    /// Adds what a root scan found, skipping anything added or dismissed since it started.
    pub fn add_discovered(
        &mut self,
        found: Vec<RepositoryCandidate>,
        warnings: Vec<String>,
    ) -> Result<RepositoryRefresh> {
        let (_, skip) = self.refresh_inputs()?;
        let mut refresh = RepositoryRefresh {
            warnings,
            ..Default::default()
        };
        for candidate in found.into_iter().filter(|c| !skip.contains(&c.path)) {
            match self.add_repository(&candidate.path, &candidate.base) {
                Ok(repository) => refresh.added.push(repository),
                Err(error) => refresh
                    .warnings
                    .push(format!("{}: {error:#}", candidate.path)),
            }
        }
        Ok(refresh)
    }

    pub fn refresh_repositories(&mut self) -> Result<RepositoryRefresh> {
        let (roots, skip) = self.refresh_inputs()?;
        let (found, warnings) = scan_roots(&roots, &skip);
        self.add_discovered(found, warnings)
    }

    /// Adds repositories to the workspace. A project with a custom selection also gains them.
    pub fn import_repositories(
        &mut self,
        project: Option<&str>,
        paths: &[String],
        roots: &[String],
        dismissed: &[String],
    ) -> Result<RepositoryImport> {
        if let Some(project) = project {
            self.project(project)?;
        }
        ensure!(
            !paths.is_empty() && paths.len() <= 1000,
            "Choose 1–1000 repositories"
        );
        let known = self.repositories()?;
        let mut result = RepositoryImport::default();
        let mut seen = BTreeSet::new();
        for path in paths {
            let imported = (|| -> Result<Repository> {
                let path = folder_path(path)?;
                let path_string = path.to_str().context("Repository path is not UTF-8")?;
                if let Some(repo) = known.iter().find(|r| r.path == path_string) {
                    return Ok(repo.clone());
                }
                let repo = candidate(&path, &BTreeSet::new())?;
                self.add_repository(&repo.path, &repo.base)
            })();
            match imported {
                Ok(repo) if seen.insert(repo.id.clone()) => result.imported.push(repo),
                Ok(_) => {}
                Err(error) => result.failures.push(format!("{path}: {error:#}")),
            }
        }
        if let Some(project) = project {
            let ids: Vec<_> = result.imported.iter().map(|r| r.id.clone()).collect();
            self.include_repositories(project, &ids)?;
        }
        ensure!(roots.len() <= 32, "Choose at most 32 root folders");
        // A repository chosen directly is a one-off; only parent folders are kept for refresh.
        for root in roots.iter().filter_map(|r| folder_path(r).ok()) {
            if root.is_dir() && !root.join(".git").exists() {
                self.db.execute(
                    "INSERT OR IGNORE INTO repository_roots VALUES (?1)",
                    [root.to_str().context("Folder path is not UTF-8")?],
                )?;
            }
        }
        for path in dismissed.iter().take(1000) {
            self.dismiss(path)?;
        }
        Ok(result)
    }
}
