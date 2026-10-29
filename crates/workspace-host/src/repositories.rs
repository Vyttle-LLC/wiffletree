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
    fs::canonicalize(settings::expand_home(value)?).context("Folder does not exist")
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
pub(crate) fn discover(
    folders: &[String],
    known: &BTreeSet<String>,
) -> Result<RepositoryDiscovery> {
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
pub(crate) fn scan_roots(
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

/// GitHub's facts about a repository are read again once they are this old.
const GITHUB_FACTS_MS: i64 = 24 * 60 * 60_000;
/// Repositories per GitHub request.
const REPOSITORIES_PER_QUERY: usize = 50;
/// The only host whose remotes name an owner; see `pr_watch::github_repository`.
pub(crate) const GITHUB_HOST: &str = "github.com";

/// Where `origin` says a repository lives, read on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// Its one push URL, on github.com.
    GitHub {
        owner: String,
        name: String,
        push_url: String,
    },
    /// No `origin`, or one on another host.
    NotOnGitHub,
    /// On github.com, but not one place a push can be checked against; the reason is shown.
    Unusable(String),
}

/// Reads `origin` without the network. Git pushes to every push URL of a remote, so a second
/// one would be a destination nobody checked; and the fetch URL must name the same repository.
pub(crate) fn read_origin(repository: &Path) -> Origin {
    let urls = |args: &[&str]| -> Vec<String> {
        runtime::git_output(repository, args)
            .map(|output| output.lines().map(|l| l.trim().to_owned()).collect())
            .unwrap_or_default()
    };
    let push = urls(&["remote", "get-url", "--push", "--all", "origin"]);
    let Some(push_url) = push.first() else {
        return Origin::NotOnGitHub;
    };
    if push.len() > 1 {
        return Origin::Unusable("origin pushes to more than one place".into());
    }
    let Some((owner, name)) = pr_watch::github_repository(push_url) else {
        return Origin::NotOnGitHub;
    };
    let fetched = urls(&["remote", "get-url", "origin"])
        .first()
        .and_then(|url| pr_watch::github_repository(url));
    if fetched.as_ref() != Some(&(owner.clone(), name.clone())) {
        return Origin::Unusable(
            "origin fetches from a different repository than it pushes to".into(),
        );
    }
    Origin::GitHub {
        owner,
        name,
        push_url: push_url.clone(),
    }
}

/// A repository as stored, for a read off the host thread.
pub(crate) struct GitHubInput {
    pub id: String,
    pub path: PathBuf,
    pub github: Option<GitHubRepository>,
}
/// What a read found for one repository: its `origin` on disk and GitHub's facts about it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct GitHubRead {
    pub id: String,
    pub github: Option<GitHubRepository>,
    pub origin_problem: Option<String>,
}
#[derive(Debug, Default)]
pub(crate) struct GitHubReading {
    pub repositories: Vec<GitHubRead>,
    /// Why GitHub gave no answer at all; earlier answers are kept.
    pub error: Option<String>,
}

fn github_query(repositories: usize) -> String {
    let variables: Vec<String> = (0..repositories)
        .map(|i| format!("$o{i}: String!, $n{i}: String!"))
        .collect();
    let aliases: String = (0..repositories)
        .map(|i| {
            format!(
                " r{i}: repository(owner: $o{i}, name: $n{i}) {{ nameWithOwner isFork visibility \
                 parent {{ nameWithOwner }} owner {{ __typename }} }}"
            )
        })
        .collect();
    format!("query({}) {{{aliases} }}", variables.join(", "))
}

/// GitHub's answers for `(owner, name)` pairs from one read-only request: its facts, or why it
/// had none for that repository. Fails only when the answer has no data at all.
fn ask_github(
    path: &Path,
    asked: &[(String, String)],
    at: i64,
    gh: &impl Fn(&Path, &[&str]) -> Result<runtime::GhAnswer>,
) -> Result<Vec<std::result::Result<GitHubFacts, String>>> {
    // Raw-string fields only: `-F` would let gh coerce values or read files.
    let mut fields = vec![format!("query={}", github_query(asked.len()))];
    for (i, (owner, name)) in asked.iter().enumerate() {
        fields.extend([format!("o{i}={owner}"), format!("n{i}={name}")]);
    }
    // The host is named, so `GH_HOST` cannot point the question at another GitHub.
    let mut args = vec!["api", "graphql", "--hostname", GITHUB_HOST];
    for field in &fields {
        args.extend(["-f", field]);
    }
    let answer = gh(path, &args)?;
    let json: Value = serde_json::from_str(&answer.output).unwrap_or_default();
    let data = json["data"].as_object().with_context(|| {
        match (json["errors"][0]["message"].as_str(), answer.error.as_str()) {
            (Some(message), _) => message.to_owned(),
            (None, "") => "gh gave no answer".to_owned(),
            (None, error) => error.to_owned(),
        }
    })?;
    Ok((0..asked.len())
        .map(|i| {
            let alias = format!("r{i}");
            let node = data.get(&alias).filter(|node| node.is_object());
            let name_with_owner = node.and_then(|n| n["nameWithOwner"].as_str());
            match (node, name_with_owner) {
                (Some(node), Some(name_with_owner)) => Ok(GitHubFacts {
                    name_with_owner: name_with_owner.to_owned(),
                    organization: node["owner"]["__typename"] == "Organization",
                    fork: node["isFork"] == true,
                    parent: node["parent"]["nameWithOwner"].as_str().map(str::to_owned),
                    public: node["visibility"] == "PUBLIC",
                    checked_at: at,
                }),
                _ => Err(json["errors"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|error| error["path"][0] == alias.as_str())
                    .and_then(|error| error["message"].as_str())
                    .unwrap_or("GitHub returned no repository")
                    .to_owned()),
            }
        })
        .collect())
}

/// Reads every repository's `origin` on disk, then asks GitHub, read-only, about those whose
/// facts are missing, older than a day or for another `origin`: the distinct ones in one
/// request per 50. Runs off the host thread.
pub(crate) fn read_github(
    inputs: &[GitHubInput],
    at: i64,
    gh: impl Fn(&Path, &[&str]) -> Result<runtime::GhAnswer>,
) -> GitHubReading {
    let mut reading = GitHubReading::default();
    // The distinct repositories to ask about, and a folder to run `gh` in.
    let mut stale: Vec<(String, String)> = vec![];
    let mut folder = None;
    for input in inputs {
        let (github, origin_problem) = match read_origin(&input.path) {
            Origin::GitHub { owner, name, .. } => {
                let known = input
                    .github
                    .clone()
                    .filter(|known| known.owner == owner && known.name == name)
                    .unwrap_or(GitHubRepository {
                        owner,
                        name,
                        facts: None,
                        problem: None,
                    });
                let fresh = known
                    .facts
                    .as_ref()
                    .is_some_and(|facts| at - facts.checked_at < GITHUB_FACTS_MS);
                let pair = (known.owner.clone(), known.name.clone());
                if !fresh && !stale.contains(&pair) {
                    stale.push(pair);
                    folder.get_or_insert(input.path.as_path());
                }
                (Some(known), None)
            }
            Origin::NotOnGitHub => (None, None),
            Origin::Unusable(reason) => (None, Some(reason)),
        };
        reading.repositories.push(GitHubRead {
            id: input.id.clone(),
            github,
            origin_problem,
        });
    }
    let Some(folder) = folder else {
        return reading;
    };
    for asked in stale.chunks(REPOSITORIES_PER_QUERY) {
        let answers = match ask_github(folder, asked, at, &gh) {
            Ok(answers) => answers,
            Err(error) => {
                reading.error = Some(format!("{error:#}"));
                continue;
            }
        };
        for ((owner, name), answer) in asked.iter().zip(answers) {
            let repositories = reading
                .repositories
                .iter_mut()
                .filter_map(|read| read.github.as_mut())
                .filter(|github| github.owner == *owner && github.name == *name);
            for github in repositories {
                (github.facts, github.problem) = match &answer {
                    Ok(facts) => (Some(facts.clone()), None),
                    Err(reason) => (None, Some(reason.clone())),
                };
            }
        }
    }
    reading
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

    /// Every repository as stored, for `read_github`, which decides off the host thread which
    /// of them GitHub must be asked about.
    pub(crate) fn github_inputs(&self) -> Result<Vec<GitHubInput>> {
        Ok(self
            .repositories()?
            .into_iter()
            .map(|r| GitHubInput {
                id: r.id,
                path: r.path.into(),
                github: r.github,
            })
            .collect())
    }

    /// Saves what a read found, for the repositories it changed, and keeps why GitHub could
    /// not be read. Returns whether anything shown changed.
    pub(crate) fn record_github(&mut self, reading: GitHubReading) -> Result<bool> {
        let mut changed = self.github_error != reading.error;
        self.github_error = reading.error;
        for read in reading.repositories {
            changed |= self.save_github(read)?;
        }
        Ok(changed)
    }

    /// Saves one repository's `origin` and GitHub facts, and returns whether they changed.
    pub(crate) fn save_github(&mut self, read: GitHubRead) -> Result<bool> {
        // Removed while the read ran.
        let Ok(mut repository) = self.repository(&read.id) else {
            return Ok(false);
        };
        if repository.github == read.github && repository.origin_problem == read.origin_problem {
            return Ok(false);
        }
        repository.github = read.github;
        repository.origin_problem = read.origin_problem;
        self.db.execute(
            "UPDATE repositories SET data=?2 WHERE id=?1",
            params![repository.id, encode(&repository)?],
        )?;
        Ok(true)
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
                github: None,
                origin_problem: None,
                checks: None,
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
        if let Some(ticket) = self.tickets()?.into_iter().find(|t| {
            t.is_open()
                && !project.uses(&t.repository_id)
                && self
                    .session(&t.coordinator_id)
                    .is_ok_and(|s| s.project_id == project.id && !s.archived)
        }) {
            bail!(
                "Ticket \"{}\" works in a repository you removed; close it first",
                ticket.title
            );
        }
        let detail = match &project.repositories {
            None => "All workspace repositories".to_owned(),
            Some(ids) => format!("{} chosen repositories", ids.len()),
        };
        let tx = self.db.savepoint()?;
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
                .any(|s| s.repository_id.as_deref() == Some(id))
                && !self.tickets()?.iter().any(|t| t.repository_id == id),
            "Tickets work in {}, so it stays in the workspace",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::git;
    use std::cell::RefCell;
    use std::os::unix::process::ExitStatusExt;

    /// A scratch repository under `root` whose `origin` is `url`, if any.
    fn scratch(root: &Path, name: &str, url: Option<&str>) -> PathBuf {
        let path = root.join(name);
        fs::create_dir_all(&path).unwrap();
        git(&path, &["init", "-q", "-b", "main"]);
        if let Some(url) = url {
            git(&path, &["remote", "add", "origin", url]);
        }
        path
    }
    fn inputs(root: &Path, urls: &[String]) -> Vec<GitHubInput> {
        urls.iter()
            .enumerate()
            .map(|(i, url)| GitHubInput {
                id: format!("r{i}"),
                path: scratch(root, &format!("r{i}"), Some(url)),
                github: None,
            })
            .collect()
    }
    fn exit(code: i32) -> std::process::ExitStatus {
        std::process::ExitStatus::from_raw(code << 8)
    }
    /// GitHub's node for a repository, or `None` when it cannot be resolved.
    type Answers = fn(&str, &str) -> Option<Value>;
    fn private(owner: &str, name: &str) -> Option<Value> {
        Some(
            json!({"nameWithOwner": format!("{owner}/{name}"), "isFork": false,
            "visibility": "PRIVATE", "parent": null, "owner": {"__typename": "Organization"}}),
        )
    }
    /// A `gh` that answers each asked repository from `answers` as GitHub does, exiting 1 when
    /// one cannot be resolved, and records its calls' arguments.
    fn fake_gh<'a>(
        calls: &'a RefCell<Vec<Vec<String>>>,
        answers: Answers,
    ) -> impl Fn(&Path, &[&str]) -> Result<runtime::GhAnswer> + 'a {
        move |_, args| {
            calls
                .borrow_mut()
                .push(args.iter().map(|a| (*a).to_owned()).collect());
            let field = |key: String| {
                args.iter()
                    .find_map(|a| a.strip_prefix(&format!("{key}=")))
                    .map(str::to_owned)
            };
            let (mut data, mut errors) = (json!({}), vec![]);
            for i in 0.. {
                let (Some(owner), Some(name)) = (field(format!("o{i}")), field(format!("n{i}")))
                else {
                    break;
                };
                let node = answers(&owner, &name);
                if node.is_none() {
                    errors.push(json!({"type": "NOT_FOUND", "path": [format!("r{i}")],
                        "message": format!("Could not resolve to a Repository with the name '{owner}/{name}'.")}));
                }
                data[format!("r{i}")] = json!(node);
            }
            let failed = !errors.is_empty();
            Ok(runtime::GhAnswer {
                status: exit(i32::from(failed)),
                output: json!({"data": data, "errors": errors}).to_string(),
                error: if failed {
                    "gh: Could not resolve".into()
                } else {
                    String::new()
                },
            })
        }
    }
    fn facts(reading: &GitHubReading, id: &str) -> Option<GitHubFacts> {
        let read = reading.repositories.iter().find(|r| r.id == id).unwrap();
        read.github.as_ref().and_then(|g| g.facts.clone())
    }

    #[test]
    fn an_ssh_or_https_origin_names_its_owner_and_repository() {
        let root = tempfile::tempdir().unwrap();
        for (name, url) in [
            ("ssh", "git@github.com:Vyttle-LLC/wiffletree.git"),
            ("https", "https://github.com/Vyttle-LLC/wiffletree"),
        ] {
            assert_eq!(
                read_origin(&scratch(root.path(), name, Some(url))),
                Origin::GitHub {
                    owner: "Vyttle-LLC".into(),
                    name: "wiffletree".into(),
                    push_url: url.into(),
                }
            );
        }
    }

    #[test]
    fn no_origin_or_one_on_another_host_is_not_on_github() {
        let root = tempfile::tempdir().unwrap();
        let none = scratch(root.path(), "none", None);
        assert_eq!(read_origin(&none), Origin::NotOnGitHub);
        let elsewhere = scratch(root.path(), "elsewhere", Some("git@gitlab.com:o/r.git"));
        assert_eq!(read_origin(&elsewhere), Origin::NotOnGitHub);
        // A remote under another name is not `origin`.
        git(
            &none,
            &["remote", "add", "upstream", "git@github.com:o/r.git"],
        );
        assert_eq!(read_origin(&none), Origin::NotOnGitHub);
    }

    #[test]
    fn an_origin_with_two_push_urls_or_another_fetch_url_names_no_owner() {
        let root = tempfile::tempdir().unwrap();
        let allowed = "git@github.com:allowed-org/app.git";
        let two = scratch(root.path(), "two", Some(allowed));
        git(
            &two,
            &["remote", "set-url", "--add", "--push", "origin", allowed],
        );
        git(
            &two,
            &[
                "remote",
                "set-url",
                "--add",
                "--push",
                "origin",
                "git@github.com:other/app.git",
            ],
        );
        assert_eq!(
            read_origin(&two),
            Origin::Unusable("origin pushes to more than one place".into())
        );
        let split = scratch(root.path(), "split", Some(allowed));
        git(
            &split,
            &[
                "remote",
                "set-url",
                "--push",
                "origin",
                "git@github.com:other/app.git",
            ],
        );
        assert!(
            matches!(read_origin(&split), Origin::Unusable(reason) if reason.contains("fetches"))
        );
        // The page shows why, and such a repository has no owner to allow.
        let input = |id: &str, path: PathBuf| GitHubInput {
            id: id.into(),
            path,
            github: None,
        };
        let reading = read_github(&[input("two", two), input("split", split)], 0, |_, _| {
            panic!("a repository without an owner is not asked about")
        });
        assert!(reading.repositories.iter().all(|r| r.github.is_none()));
        assert_eq!(
            reading.repositories[0].origin_problem.as_deref(),
            Some("origin pushes to more than one place")
        );
    }

    #[test]
    fn the_workspace_is_read_with_one_request_per_fifty_distinct_repositories() {
        let root = tempfile::tempdir().unwrap();
        let urls = |count: usize| -> Vec<String> {
            (0..count)
                .map(|i| format!("git@github.com:Sage-Social/r{i}.git"))
                .collect()
        };
        // 34 repositories, two of them clones of the same one: one request, 33 asked about.
        let mut workspace = urls(33);
        workspace.push(workspace[0].clone());
        let calls = RefCell::new(vec![]);
        let reading = read_github(
            &inputs(&root.path().join("a"), &workspace),
            9,
            fake_gh(&calls, private),
        );
        assert_eq!(reading.error, None);
        assert_eq!(calls.borrow().len(), 1);
        let call = calls.borrow()[0].clone();
        // The question names github.com, whatever `GH_HOST` says.
        assert_eq!(&call[..4], ["api", "graphql", "--hostname", "github.com"]);
        // Raw-string fields only, and a read-only query.
        assert!(call[4..].chunks(2).all(|pair| pair[0] == "-f"));
        assert!(call[5].starts_with("query=query(") && !call[5].contains("mutation"));
        assert!(
            call.contains(&"o32=Sage-Social".to_owned())
                && !call.iter().any(|a| a.starts_with("o33="))
        );
        for id in ["r0", "r33"] {
            let facts = facts(&reading, id).unwrap();
            assert_eq!(facts.name_with_owner, "Sage-Social/r0");
            assert!(facts.organization && !facts.fork && !facts.public);
            assert_eq!(facts.checked_at, 9);
        }
        // 51 distinct repositories take two.
        let calls = RefCell::new(vec![]);
        let reading = read_github(
            &inputs(&root.path().join("b"), &urls(51)),
            9,
            fake_gh(&calls, private),
        );
        assert_eq!(calls.borrow().len(), 2);
        assert!(facts(&reading, "r50").is_some());
    }

    #[test]
    fn a_fork_its_parent_and_a_transferred_repository_are_read_as_github_reports_them() {
        let root = tempfile::tempdir().unwrap();
        let urls = [
            "git@github.com:MHohlios/gpui-component.git".to_owned(),
            "https://github.com/old-org/app.git".to_owned(),
        ];
        let answers: Answers = |owner, name| {
            Some(match name {
                "gpui-component" => json!({"nameWithOwner": format!("{owner}/{name}"),
                    "isFork": true, "visibility": "PUBLIC",
                    "parent": {"nameWithOwner": "longbridge/gpui-component"},
                    "owner": {"__typename": "User"}}),
                _ => private("new-org", name).unwrap(),
            })
        };
        let calls = RefCell::new(vec![]);
        let reading = read_github(&inputs(root.path(), &urls), 1, fake_gh(&calls, answers));
        let fork = facts(&reading, "r0").unwrap();
        assert!(fork.fork && fork.public && !fork.organization);
        assert_eq!(fork.parent.as_deref(), Some("longbridge/gpui-component"));
        // The URL's owner is kept beside the owner GitHub reports, which is the one that counts.
        let moved = reading.repositories[1].github.clone().unwrap();
        assert_eq!(
            (moved.owner.as_str(), moved.name.as_str()),
            ("old-org", "app")
        );
        assert_eq!(moved.facts.unwrap().name_with_owner, "new-org/app");
        let repository = Repository {
            id: "r1".into(),
            name: "app".into(),
            path: String::new(),
            base: "origin/main".into(),
            github: reading.repositories[1].github.clone(),
            origin_problem: None,
            checks: None,
        };
        assert_eq!(owner_entry(&repository).as_deref(), Some("new-org"));
    }

    #[test]
    fn a_fork_is_read_from_github_com_even_when_gh_host_names_another_github() {
        let root = tempfile::tempdir().unwrap();
        let inputs = inputs(
            root.path(),
            &["git@github.com:allowed-org/app.git".to_owned()],
        );
        // As under `GH_HOST`: a question that does not name github.com reaches another host,
        // whose repository of the same name is not a fork.
        let gh = |_: &Path, args: &[&str]| {
            let names_github = args
                .windows(2)
                .any(|pair| pair == ["--hostname", "github.com"]);
            Ok(runtime::GhAnswer {
                status: exit(0),
                output: json!({"data": {"r0": {"nameWithOwner": "allowed-org/app",
                    "isFork": names_github, "visibility": "PRIVATE", "parent": null,
                    "owner": {"__typename": "Organization"}}}})
                .to_string(),
                error: String::new(),
            })
        };
        let reading = read_github(&inputs, 1, gh);
        assert!(facts(&reading, "r0").unwrap().fork);
    }

    #[test]
    fn one_repository_github_cannot_resolve_is_marked_alone_and_the_rest_are_recorded() {
        let root = tempfile::tempdir().unwrap();
        let urls: Vec<String> = (0..34)
            .map(|i| format!("git@github.com:Sage-Social/r{i}.git"))
            .collect();
        let answers: Answers = |owner, name| private(owner, name).filter(|_| name != "r33");
        let calls = RefCell::new(vec![]);
        let reading = read_github(&inputs(root.path(), &urls), 1, fake_gh(&calls, answers));
        // gh exited 1 and still printed the others' data.
        assert_eq!(reading.error, None);
        assert!((0..33).all(|i| facts(&reading, &format!("r{i}")).is_some()));
        let missing = reading.repositories[33].github.clone().unwrap();
        assert_eq!(missing.facts, None);
        assert_eq!(
            missing.problem.as_deref(),
            Some("Could not resolve to a Repository with the name 'Sage-Social/r33'.")
        );
    }

    #[test]
    fn only_missing_day_old_or_repointed_facts_are_asked_for() {
        let root = tempfile::tempdir().unwrap();
        let day = GITHUB_FACTS_MS;
        let mut inputs = inputs(
            root.path(),
            &["git@github.com:Sage-Social/app.git".to_owned()],
        );
        let calls = RefCell::new(vec![]);
        let first = read_github(&inputs, 1_000, fake_gh(&calls, private));
        inputs[0].github = first.repositories[0].github.clone();
        // Nothing stale: no `gh` call, and what was stored stands.
        let fresh = read_github(&inputs, day, |_, _| panic!("nothing is stale"));
        assert_eq!(fresh.repositories, first.repositories);
        // A day later it is read again.
        let later = read_github(&inputs, 1_000 + day, fake_gh(&calls, private));
        assert_eq!(facts(&later, "r0").unwrap().checked_at, 1_000 + day);
        // A repointed `origin` drops the facts read for the old one and asks again.
        git(
            &inputs[0].path,
            &[
                "remote",
                "set-url",
                "origin",
                "git@github.com:Sage-Social/web.git",
            ],
        );
        let repointed = read_github(&inputs, day, fake_gh(&calls, private));
        assert_eq!(
            facts(&repointed, "r0").unwrap().name_with_owner,
            "Sage-Social/web"
        );
        assert_eq!(calls.borrow().len(), 3);
    }

    #[test]
    fn a_read_that_fails_altogether_keeps_owners_and_earlier_answers() {
        let root = tempfile::tempdir().unwrap();
        let mut inputs = inputs(
            root.path(),
            &[
                "git@github.com:Sage-Social/app.git".to_owned(),
                "git@github.com:Sage-Social/web.git".to_owned(),
            ],
        );
        let calls = RefCell::new(vec![]);
        let first = read_github(&inputs[..1], 1_000, fake_gh(&calls, private));
        inputs[0].github = first.repositories[0].github.clone();
        let signed_out = |_: &Path, _: &[&str]| {
            Ok(runtime::GhAnswer {
                status: exit(4),
                output: String::new(),
                error: "To get started with GitHub CLI, please run:  gh auth login".into(),
            })
        };
        let reading = read_github(&inputs, 1_000 + GITHUB_FACTS_MS, signed_out);
        assert_eq!(
            reading.error.as_deref(),
            Some("To get started with GitHub CLI, please run:  gh auth login")
        );
        // The earlier answer is kept, and the owner read on disk is listed without one.
        assert_eq!(facts(&reading, "r0").unwrap().checked_at, 1_000);
        let unanswered = reading.repositories[1].github.clone().unwrap();
        assert_eq!(
            (unanswered.owner.as_str(), unanswered.facts),
            ("Sage-Social", None)
        );
        // A `gh` that cannot run at all reads the same way.
        let missing = read_github(&inputs[1..], 0, |_, _| {
            bail!("gh could not run: No such file")
        });
        assert_eq!(
            missing.error.as_deref(),
            Some("gh could not run: No such file")
        );
    }

    fn host_with(urls: &[&str]) -> (tempfile::TempDir, Host) {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path().join("home")).unwrap();
        for (i, url) in urls.iter().enumerate() {
            let path = scratch(home.path(), &format!("r{i}"), Some(url));
            git(
                &path,
                &[
                    "-c",
                    "user.name=F",
                    "-c",
                    "user.email=f@example.invalid",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    "start",
                ],
            );
            host.add_repository(path.to_str().unwrap(), "HEAD").unwrap();
        }
        (home, host)
    }

    #[test]
    fn a_reading_is_saved_and_shown_and_a_failed_one_keeps_what_was_stored() {
        let (_home, mut host) =
            host_with(&["git@github.com:old-org/app.git", "git@gitlab.com:o/r.git"]);
        let calls = RefCell::new(vec![]);
        let moved: Answers = |_, name| private("new-org", name);
        let reading = read_github(&host.github_inputs().unwrap(), 5, fake_gh(&calls, moved));
        assert!(host.record_github(reading).unwrap());
        let snapshot = host.snapshot().unwrap();
        assert_eq!(snapshot.github_error, None);
        // A transferred repository is listed and checked under the owner GitHub reports.
        assert_eq!(
            owner_entries(&snapshot.repositories).owners[0].login,
            "new-org"
        );
        assert!(is_allowed(
            &Repository {
                base: "origin/main".into(),
                ..snapshot.repositories[0].clone()
            },
            &["new-org".to_owned()]
        ));
        assert_eq!(snapshot.repositories[1].github, None);
        // An unchanged reading saves nothing and is no news.
        let same = read_github(&host.github_inputs().unwrap(), 6, |_, _| {
            panic!("nothing is stale")
        });
        assert!(!host.record_github(same).unwrap());
        // A failed read a day later keeps the answer and says why.
        let failed = read_github(
            &host.github_inputs().unwrap(),
            5 + GITHUB_FACTS_MS,
            |_, _| bail!("gh failed: not logged in"),
        );
        assert!(host.record_github(failed).unwrap());
        let snapshot = host.snapshot().unwrap();
        assert_eq!(
            snapshot.github_error.as_deref(),
            Some("gh failed: not logged in")
        );
        assert_eq!(
            snapshot.repositories[0]
                .github
                .as_ref()
                .unwrap()
                .facts
                .as_ref()
                .unwrap()
                .checked_at,
            5
        );
    }

    /// A project using a GitHub repository GitHub has answered for, with its coordinator.
    fn project_on_github() -> (tempfile::TempDir, Host, Session) {
        let (home, mut host) = host_with(&["git@github.com:celadon-theme/website.git"]);
        let calls = RefCell::new(vec![]);
        let reading = read_github(&host.github_inputs().unwrap(), 5, fake_gh(&calls, private));
        host.record_github(reading).unwrap();
        let repository = host.repositories().unwrap().remove(0);
        host.db
            .execute(
                "UPDATE repositories SET data=?2 WHERE id=?1",
                params![
                    repository.id,
                    encode(&Repository {
                        base: "origin/main".into(),
                        ..repository.clone()
                    })
                    .unwrap()
                ],
            )
            .unwrap();
        host.create_project("Site").unwrap();
        let coordinator = host.sessions().unwrap().remove(0);
        (home, host, coordinator)
    }

    #[test]
    fn an_agent_reads_each_repositorys_owner_and_rule_in_workspace_context() {
        let (_home, mut host, coordinator) = project_on_github();
        let repository =
            |host: &Host| host.agent_context(&coordinator.id).unwrap()["repositories"][0].clone();
        let entry = repository(&host);
        assert_eq!(entry["github"]["owner"], "celadon-theme");
        assert_eq!(entry["github"]["name"], "website");
        assert_eq!(entry["github"]["facts"]["fork"], false);
        assert_eq!(entry["github"]["facts"]["public"], false);
        assert_eq!(entry["publishes_without_asking"], false);
        host.set_owner_allowed("celadon-theme", true).unwrap();
        assert_eq!(repository(&host)["publishes_without_asking"], true);
        host.set_owner_allowed("celadon-theme", false).unwrap();
        assert_eq!(repository(&host)["publishes_without_asking"], false);
    }

    #[test]
    fn no_agent_tool_changes_the_allowed_list() {
        let (_home, mut host, coordinator) = project_on_github();
        host.set_owner_allowed("Sage-Social", true).unwrap();
        let tools = mcp::tools();
        for tool in tools.as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            // No tool takes an owner to allow, and none changes the list whatever it is sent.
            let schema = tool["inputSchema"]["properties"].to_string();
            assert!(
                !schema.contains("allowed") && !schema.contains("owner"),
                "{name}"
            );
            let _ = host.agent_tool(
                &coordinator.id,
                name,
                json!({"entry": "celadon-theme", "allowed": true, "owner": "celadon-theme",
                    "allowed_owners": ["celadon-theme"]}),
            );
        }
        assert_eq!(host.settings().allowed_owners, ["Sage-Social"]);
    }

    #[test]
    fn when_github_cannot_be_read_nothing_is_treated_as_answered() {
        let (_home, mut host) = host_with(&["git@github.com:Sage-Social/app.git"]);
        let reading = read_github(&host.github_inputs().unwrap(), 5, |_, _| {
            bail!("gh failed: not logged in")
        });
        host.record_github(reading).unwrap();
        let snapshot = host.snapshot().unwrap();
        assert_eq!(
            snapshot.github_error.as_deref(),
            Some("gh failed: not logged in")
        );
        let repository = &snapshot.repositories[0];
        // The owner is still listed, from the remote on disk; it asks first whatever is allowed.
        assert_eq!(repository.github.as_ref().unwrap().owner, "Sage-Social");
        assert!(!is_allowed(repository, &["Sage-Social".to_owned()]));
        assert_eq!(
            owner_entries(&snapshot.repositories).owners[0].summary(),
            "1 repository · asking first until GitHub answers"
        );
    }
}
