//! Host settings in `settings.json`, and the readable folder layout they choose for new work.
use crate::*;

const FILE: &str = "settings.json";
const SLUG_LIMIT: usize = 48;

/// `~/wiffletree`, or `~/wiffletree-beta` for a beta build so the two never share folders.
fn default_workspaces_dir() -> String {
    let name = if option_env!("WIFFLETREE_CHANNEL") == Some("beta") {
        "wiffletree-beta"
    } else {
        "wiffletree"
    };
    std::env::home_dir()
        .unwrap_or_default()
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// Expands a leading `~` to the home directory.
pub(crate) fn expand_home(value: &str) -> Result<PathBuf> {
    Ok(if value == "~" || value.starts_with("~/") {
        std::env::home_dir()
            .context("Home directory unavailable")?
            .join(value.strip_prefix("~/").unwrap_or(""))
    } else {
        PathBuf::from(value)
    })
}

/// A readable, lowercase, filesystem-safe name, or `fallback` when nothing readable remains.
pub(crate) fn slug(text: &str, fallback: &str) -> String {
    let mut slug = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.truncate(SLUG_LIMIT);
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        fallback.into()
    } else {
        slug.into()
    }
}

/// `base`, or the first of `base-2`, `base-3`… that is not taken.
fn unused(base: &str, taken: impl Fn(&str) -> bool) -> String {
    (1..)
        .map(|n| match n {
            1 => base.to_owned(),
            n => format!("{base}-{n}"),
        })
        .find(|candidate| !taken(candidate))
        .expect("an unused suffix exists")
}

/// The slug that names a project's folders: its home folder's name, or the slug a project from
/// before workspace folders was given for its first ticket.
fn project_slug(project: &Project) -> Option<String> {
    project
        .home
        .as_deref()
        .and_then(|home| Path::new(home).file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .or_else(|| project.slug.clone())
}

/// The operating system's reason alone, without the path of a probe file the user never chose.
fn reason(error: &std::io::Error) -> std::io::Error {
    error
        .raw_os_error()
        .map(std::io::Error::from_raw_os_error)
        .unwrap_or_else(|| error.kind().into())
}

/// Every local branch under `wiffletree/`, as `refs/heads/...` names.
fn wiffletree_branches(repository: &Path) -> Result<Vec<String>> {
    let refs = runtime::git_output(
        repository,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads/wiffletree",
        ],
    )
    .context("Could not list the repository's wiffletree branches")?;
    let refs: Vec<String> = refs.lines().map(str::to_owned).collect();
    ensure!(
        !refs.iter().any(|r| r == "refs/heads/wiffletree"),
        "A branch named \"wiffletree\" blocks ticket branches; rename it in {}",
        repository.display()
    );
    Ok(refs)
}

impl Host {
    /// The saved settings, or the defaults when the file is missing, unreadable or invalid.
    pub fn settings(&self) -> HostSettings {
        fs::read(self.home.join(FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| HostSettings {
                workspaces_dir: default_workspaces_dir(),
            })
    }
    pub fn set_workspaces_dir(&mut self, path: &str) -> Result<HostSettings> {
        text(path, 4096)?;
        let folder = expand_home(path.trim())?;
        ensure!(
            folder.is_absolute(),
            "Choose an absolute folder for workspaces"
        );
        fs::create_dir_all(&folder)
            .map_err(|e| anyhow::anyhow!("Cannot create {}: {}", folder.display(), reason(&e)))?;
        // A new, uniquely named file: never one that exists, which could be a symlink elsewhere.
        tempfile::NamedTempFile::new_in(&folder)
            .and_then(|probe| probe.close())
            .map_err(|e| anyhow::anyhow!("Cannot write to {}: {}", folder.display(), reason(&e)))?;
        let settings = HostSettings {
            workspaces_dir: folder.to_string_lossy().into_owned(),
        };
        self.save_settings(&settings)?;
        Ok(settings)
    }
    /// Writes atomically and keeps fields this version does not know.
    fn save_settings(&self, settings: &HostSettings) -> Result<()> {
        let path = self.home.join(FILE);
        let mut saved = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        if let Value::Object(fields) = serde_json::to_value(settings)? {
            saved.as_object_mut().unwrap().extend(fields);
        }
        let mut staged = tempfile::NamedTempFile::new_in(&self.home)?;
        serde_json::to_writer_pretty(&mut staged, &saved)?;
        staged.persist(&path)?;
        Ok(())
    }
    /// Where the main coordinator runs. Projects from before workspace folders keep their
    /// original directory, because providers resume conversations by directory.
    pub fn project_directory(&self, project_id: &str) -> Result<PathBuf> {
        Ok(match self.project(project_id)?.home {
            Some(home) => PathBuf::from(home),
            None => self.home.join("projects").join(project_id),
        })
    }
    /// `<workspaces>/projects/<slug>` with a slug no other project or folder uses.
    pub(crate) fn new_project_home(&self, name: &str) -> Result<String> {
        let workspaces = PathBuf::from(self.settings().workspaces_dir);
        let slug = self.unused_project_slug(name, &workspaces)?;
        Ok(workspaces
            .join("projects")
            .join(slug)
            .to_string_lossy()
            .into_owned())
    }
    fn unused_project_slug(&self, name: &str, workspaces: &Path) -> Result<String> {
        let projects = self.projects()?;
        Ok(unused(&slug(name, "project"), |candidate| {
            projects
                .iter()
                .any(|p| project_slug(p).as_deref() == Some(candidate))
                || workspaces.join("projects").join(candidate).exists()
                || workspaces.join("tasks").join(candidate).exists()
        }))
    }
    /// The project's slug. A project from before workspace folders gets one on first use and
    /// keeps it, so renaming it never moves where its tickets go.
    fn fixed_project_slug(&self, project: &Project, workspaces: &Path) -> Result<String> {
        if let Some(slug) = project_slug(project) {
            return Ok(slug);
        }
        let slug = self.unused_project_slug(&project.name, workspaces)?;
        let mut project = project.clone();
        project.slug = Some(slug.clone());
        self.db.execute(
            "UPDATE projects SET data=?2 WHERE id=?1",
            params![project.id, encode(&project)?],
        )?;
        Ok(slug)
    }
    /// `<workspaces>/tasks/<project>/<repository>/<ticket>` and its `wiffletree/<ticket>`
    /// branch, suffixing the ticket slug until neither is in use.
    pub(crate) fn new_ticket_place(
        &self,
        project: &Project,
        repository: &Repository,
        title: &str,
    ) -> Result<(PathBuf, String)> {
        let repository_path = Path::new(&repository.path);
        let workspaces = PathBuf::from(self.settings().workspaces_dir);
        let parent = workspaces
            .join("tasks")
            .join(self.fixed_project_slug(project, &workspaces)?)
            .join(slug(
                &repository_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
                "repository",
            ));
        let tickets = self.tickets()?;
        let branches = wiffletree_branches(repository_path)?;
        let slug = unused(&slug(title, "ticket"), |candidate| {
            let path = parent.join(candidate);
            let branch = format!("refs/heads/wiffletree/{candidate}");
            path.exists()
                || tickets.iter().any(|t| Path::new(&t.worktree) == path)
                || branches.iter().any(|r| {
                    r.strip_prefix(&branch)
                        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
                })
        });
        Ok((parent.join(&slug), format!("wiffletree/{slug}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_readable_safe_and_bounded() {
        assert_eq!(
            slug("  Fix: Inbound calls (v2)!  ", "ticket"),
            "fix-inbound-calls-v2"
        );
        assert_eq!(slug("Café ☕", "ticket"), "caf");
        assert_eq!(slug("../..", "ticket"), "ticket");
        assert_eq!(slug("", "project"), "project");
        let long = slug(&"word ".repeat(40), "ticket");
        assert!(long.len() <= SLUG_LIMIT && !long.ends_with('-'), "{long}");
    }

    #[test]
    fn unused_suffixes_after_the_base() {
        let taken = ["notes", "notes-2"];
        assert_eq!(unused("notes", |c| taken.contains(&c)), "notes-3");
        assert_eq!(unused("other", |c| taken.contains(&c)), "other");
    }
}
