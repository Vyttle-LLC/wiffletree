//! Files a client reads through the host: whether linked paths exist, a file's bytes under the
//! roots the host serves, and folder listings. Every path is resolved, following symlinks and
//! `..`, before it is checked.
use crate::Host;
use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use std::{
    fs,
    io::Read,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
use workspace_core::{
    FileContents, FolderEntry, FolderListing, MAX_ATTACHMENT_BYTES, ResolvedPath,
};

/// Credential locations no root ever serves, under the host user's home folder unless noted.
const DENIED_IN_HOME: [&str; 11] = [
    ".ssh",
    ".config/gh",
    ".gnupg",
    ".aws",
    ".netrc",
    ".git-credentials",
    ".config/git/credentials",
    ".claude/.credentials.json",
    ".claude.json",
    ".codex/auth.json",
    "Library/Keychains",
];

/// The credential locations, each as named and as resolved.
#[derive(Clone, Debug)]
pub struct DenyList(Vec<PathBuf>);
impl DenyList {
    pub fn new(home: &Path, xdg_config_home: Option<&Path>, codex_home: Option<&Path>) -> Self {
        let mut named: Vec<PathBuf> = DENIED_IN_HOME.iter().map(|p| home.join(p)).collect();
        named.extend(xdg_config_home.map(|config| config.join("git/credentials")));
        named.extend(codex_home.map(|codex| codex.join("auth.json")));
        let resolved: Vec<PathBuf> = named.iter().filter_map(|p| resolve(p).ok()).collect();
        Self(named.into_iter().chain(resolved).collect())
    }
    /// From `HOME`, `XDG_CONFIG_HOME` and `CODEX_HOME`.
    pub fn from_env() -> Self {
        let var = |name| std::env::var_os(name).map(PathBuf::from);
        Self::new(
            &var("HOME").unwrap_or_default(),
            var("XDG_CONFIG_HOME").as_deref(),
            var("CODEX_HOME").as_deref(),
        )
    }
    fn denies(&self, resolved: &Path) -> bool {
        self.0.iter().any(|denied| resolved.starts_with(denied))
    }
}

/// What the host serves a client, all resolved.
#[derive(Clone, Debug)]
pub struct Roots {
    /// Project homes, ticket worktrees, attached repositories, the workspace folder and every
    /// project's attachment store.
    roots: Vec<PathBuf>,
    /// The data folder, of which only `inside_data` is served.
    data: PathBuf,
    inside_data: Vec<PathBuf>,
    home: PathBuf,
    deny: DenyList,
}
impl Roots {
    /// Roots that do not exist are left out. Inside the data folder only `served_in_data`, the
    /// attachment stores and project homes there, is served, whatever `roots` holds, and only
    /// those reached through real folders: one that is a symlink, or under one inside the data
    /// folder, serves nothing.
    pub fn new(
        roots: impl IntoIterator<Item = PathBuf>,
        served_in_data: impl IntoIterator<Item = PathBuf>,
        data: &Path,
        home: &Path,
        deny: DenyList,
    ) -> Result<Self> {
        let data = resolve(data)?;
        let existing = |paths: Vec<PathBuf>| -> Vec<PathBuf> {
            paths.iter().filter_map(|p| resolve(p).ok()).collect()
        };
        let (mut inside_data, mut outside_data) = (vec![], vec![]);
        for root in served_in_data {
            let Ok(resolved) = resolve(&root) else {
                continue;
            };
            if !resolved.starts_with(&data) {
                outside_data.push(resolved);
            } else if resolved != data && plainly_inside(&root, &data) {
                inside_data.push(resolved);
            }
        }
        let mut roots = existing(roots.into_iter().collect());
        roots.extend(outside_data);
        Ok(Self {
            roots,
            data,
            inside_data,
            home: resolve(home).unwrap_or_else(|_| home.to_owned()),
            deny,
        })
    }
    /// Resolves `path` and checks the result. `home` also allows anything under the host's home
    /// folder, which only listings for the host's own clients use.
    fn check(&self, path: &Path, home: bool) -> Result<PathBuf> {
        ensure!(path.is_absolute(), "Ask for an absolute path");
        let resolved = resolve(path).with_context(|| format!("{} is not there", path.display()))?;
        ensure!(
            !self.deny.denies(&resolved),
            "{} is a credential location, which the host never serves",
            path.display()
        );
        let under = |roots: &[PathBuf]| roots.iter().any(|root| resolved.starts_with(root));
        if let Ok(inside) = resolved.strip_prefix(&self.data) {
            ensure!(
                !never_served(inside) && under(&self.inside_data),
                "The host serves only attachments and project folders from its data folder"
            );
        } else {
            ensure!(
                under(&self.roots) || (home && resolved.starts_with(&self.home)),
                "{} is outside what the host serves",
                path.display()
            );
        }
        Ok(resolved)
    }
}

/// The data folder's own files, never served whatever root holds them: the store and its
/// copies, its backups, the host's binaries and logs, and its settings.
fn never_served(inside_data: &Path) -> bool {
    const NAMES: [&str; 6] = [
        "backups",
        "host",
        "logs",
        "shared",
        "settings.json",
        "host.lock",
    ];
    inside_data.components().next().is_some_and(|first| {
        let name = first.as_os_str().to_string_lossy();
        name.starts_with(crate::STORE_FILE) || NAMES.contains(&name.as_ref())
    })
}

/// Whether no folder between `root` and the data folder, `root` included, is a symlink inside
/// the data folder, such as an attachment store linked elsewhere. Symlinks outside it do not
/// count: those above it, such as `/var`, and a workspace folder chosen as an alias of a folder in
/// it, whose project homes `never_served` still guards.
fn plainly_inside(root: &Path, data: &Path) -> bool {
    let mut symlink = false;
    for folder in root.ancestors() {
        if resolve(folder).is_ok_and(|resolved| resolved == data) {
            return !symlink;
        }
        symlink |= fs::symlink_metadata(folder).map_or(true, |m| m.file_type().is_symlink());
    }
    true
}

fn resolve(path: &Path) -> Result<PathBuf> {
    Ok(fs::canonicalize(path)?)
}

impl Host {
    /// What `ReadFile` and `ListFolder` serve now: every project's home and attachment store,
    /// every ticket's worktree, every repository and the workspace folder.
    pub fn file_roots(&self) -> Result<Roots> {
        let mut roots = vec![PathBuf::from(self.settings().workspaces_dir)];
        let mut projects = vec![];
        for project in self.projects()? {
            projects.push(self.project_directory(&project.id)?);
            projects.push(
                self.home
                    .join("projects")
                    .join(&project.id)
                    .join("attachments"),
            );
        }
        roots.extend(
            self.tickets()?
                .into_iter()
                .map(|t| PathBuf::from(t.worktree)),
        );
        roots.extend(
            self.repositories()?
                .into_iter()
                .map(|r| PathBuf::from(r.path)),
        );
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        Roots::new(roots, projects, &self.home, &home, DenyList::from_env())
    }
}

/// Each reference as a file, as it is when absolute or under `~/`, or else under the first base
/// that holds it. Reads only metadata.
pub fn resolve_paths(
    references: &[String],
    bases: &[PathBuf],
    home: Option<&Path>,
) -> Vec<Option<ResolvedPath>> {
    references
        .iter()
        .map(|reference| {
            let path = match reference.strip_prefix('~') {
                Some(rest) if rest.is_empty() || rest.starts_with('/') => {
                    home?.join(rest.trim_start_matches('/'))
                }
                _ => PathBuf::from(reference),
            };
            let path = if path.is_absolute() {
                path.is_file().then_some(path)
            } else {
                bases
                    .iter()
                    .map(|base| base.join(&path))
                    .find(|c| c.is_file())
            }?;
            Some(ResolvedPath {
                runs_when_opened: runs_when_opened(&path),
                path,
            })
        })
        .collect()
}

/// Whether opening `path` would run it instead of showing it: an executable, or a script
/// Terminal runs.
fn runs_when_opened(path: &Path) -> bool {
    const LAUNCHERS: [&str; 3] = ["command", "tool", "terminal"];
    let executable = path
        .metadata()
        .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0);
    let launcher = path.extension().is_some_and(|extension| {
        LAUNCHERS
            .iter()
            .any(|launcher| extension.eq_ignore_ascii_case(launcher))
    });
    executable || launcher
}

pub fn read_file(roots: &Roots, path: &Path, max_bytes: u64) -> Result<FileContents> {
    let resolved = roots.check(path, false)?;
    let limit = max_bytes.min(MAX_ATTACHMENT_BYTES);
    // Opening a FIFO or a device could block or have effects, so only a file is opened.
    ensure!(
        fs::metadata(&resolved)?.is_file(),
        "{} is not a file",
        path.display()
    );
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&resolved)?;
    let metadata = file.metadata()?;
    ensure!(metadata.is_file(), "{} is not a file", path.display());
    let too_large = || {
        if limit == MAX_ATTACHMENT_BYTES {
            anyhow::anyhow!("{} is over 50 MB", path.display())
        } else {
            anyhow::anyhow!("{} is over {limit} bytes", path.display())
        }
    };
    if metadata.len() > limit {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    file.by_ref().take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(too_large());
    }
    Ok(FileContents {
        path: resolved,
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

/// The folders in `path`, leaving out credential locations. `home` allows folders under the
/// host's home folder, as for the host's own clients.
pub fn list_folder(roots: &Roots, path: &Path, home: bool) -> Result<FolderListing> {
    let resolved = roots.check(path, home)?;
    if !resolved.is_dir() {
        bail!("{} is not a folder", path.display());
    }
    let mut folders = vec![];
    for entry in fs::read_dir(&resolved)? {
        let entry = entry?;
        let path = entry.path();
        let shown =
            resolve(&path).is_ok_and(|target| target.is_dir() && !roots.deny.denies(&target));
        if shown && !roots.deny.denies(&path) {
            folders.push(FolderEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                repository: path.join(".git").exists(),
            });
        }
    }
    folders.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(FolderListing {
        path: resolved,
        folders,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A home folder whose workspace folder is the home itself, with every credential location.
    struct Home {
        home: tempfile::TempDir,
        config: tempfile::TempDir,
        roots: Roots,
    }
    fn home() -> Home {
        let home = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let data = home.path().join("Library/Application Support/Wiffletree");
        fs::create_dir_all(&data).unwrap();
        for denied in DENIED_IN_HOME {
            let path = home.path().join(denied);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if [".ssh", ".gnupg", ".aws", "Library/Keychains", ".config/gh"].contains(&denied) {
                fs::create_dir_all(&path).unwrap();
                fs::write(path.join("secret"), "secret").unwrap();
            } else {
                fs::write(&path, "secret").unwrap();
            }
        }
        fs::create_dir_all(config.path().join("git")).unwrap();
        fs::write(config.path().join("git/credentials"), "secret").unwrap();
        let deny = DenyList::new(home.path(), Some(config.path()), None);
        let roots = Roots::new([home.path().to_owned()], vec![], &data, home.path(), deny).unwrap();
        Home {
            home,
            config,
            roots,
        }
    }

    #[test]
    fn a_workspace_folder_of_home_still_never_serves_a_credential() {
        let Home {
            home,
            config,
            roots,
        } = home();
        let home = home.path();
        let files = [
            home.join(".config/gh/secret"),
            home.join(".codex/auth.json"),
            home.join(".config/git/credentials"),
            config.path().join("git/credentials"),
            home.join(".ssh/secret"),
            home.join(".claude.json"),
            home.join(".claude/.credentials.json"),
            home.join(".netrc"),
            home.join(".git-credentials"),
            home.join("Library/Keychains/secret"),
        ];
        let roots_with_config = Roots::new(
            [home.to_owned(), config.path().to_owned()],
            vec![],
            &home.join("Library/Application Support/Wiffletree"),
            home,
            roots.deny.clone(),
        )
        .unwrap();
        for file in &files {
            let error = read_file(&roots_with_config, file, 1024).unwrap_err();
            assert!(
                error.to_string().contains("credential"),
                "{file:?}: {error}"
            );
        }
        for folder in [".ssh", ".config/gh", ".gnupg", ".aws", "Library/Keychains"] {
            let error = list_folder(&roots, &home.join(folder), true).unwrap_err();
            assert!(
                error.to_string().contains("credential"),
                "{folder}: {error}"
            );
        }
        // Nor is one named in a listing of the folder holding it.
        let names = |folder: &str| -> Vec<String> {
            let listing = list_folder(&roots, &home.join(folder), true).unwrap();
            listing.folders.into_iter().map(|f| f.name).collect()
        };
        assert!(!names(".config").contains(&"gh".to_string()));
        assert!(!names("Library").contains(&"Keychains".to_string()));
        assert!(
            !names("")
                .iter()
                .any(|n| [".ssh", ".aws", ".gnupg"].contains(&n.as_str()))
        );
    }

    #[test]
    fn a_fifo_under_a_root_is_refused_without_opening_it() {
        let root = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let fifo = root.path().join("pipe");
        let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `name` is a valid C string.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let deny = DenyList::new(Path::new("/nonexistent"), None, None);
        let roots = Roots::new(
            [root.path().to_owned()],
            vec![],
            data.path(),
            root.path(),
            deny,
        );
        // Opening a FIFO nobody writes to would block; the refusal comes first.
        let error = read_file(&roots.unwrap(), &fifo, 1024).unwrap_err();
        assert!(error.to_string().contains("is not a file"), "{error}");
    }

    #[test]
    fn codex_home_moves_the_codex_credential() {
        let codex = tempfile::tempdir().unwrap();
        fs::write(codex.path().join("auth.json"), "secret").unwrap();
        let data = tempfile::tempdir().unwrap();
        let deny = DenyList::new(Path::new("/nonexistent"), None, Some(codex.path()));
        let roots = Roots::new(
            [codex.path().to_owned()],
            vec![],
            data.path(),
            codex.path(),
            deny,
        )
        .unwrap();
        let error = read_file(&roots, &codex.path().join("auth.json"), 1024).unwrap_err();
        assert!(error.to_string().contains("credential"), "{error}");
    }

    #[test]
    fn listings_outside_the_roots_are_allowed_only_under_home_for_the_host_itself() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("root");
        let elsewhere = home.path().join("elsewhere");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(elsewhere.join("repo/.git")).unwrap();
        fs::create_dir_all(elsewhere.join("plain")).unwrap();
        fs::write(elsewhere.join("file.txt"), "not a folder").unwrap();
        let data = tempfile::tempdir().unwrap();
        let deny = DenyList::new(home.path(), None, None);
        let roots = Roots::new([root], vec![], data.path(), home.path(), deny).unwrap();
        assert!(list_folder(&roots, Path::new("/etc"), true).is_err());
        assert!(list_folder(&roots, &elsewhere, false).is_err());
        let listing = list_folder(&roots, &elsewhere, true).unwrap();
        let entries = [
            FolderEntry {
                name: "plain".into(),
                repository: false,
            },
            FolderEntry {
                name: "repo".into(),
                repository: true,
            },
        ];
        assert_eq!(listing.folders, entries, "folders and repositories only");
    }

    #[test]
    fn references_resolve_under_the_first_base_holding_them() {
        let worktree = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(worktree.path().join("src")).unwrap();
        fs::write(worktree.path().join("src/lib.rs"), "").unwrap();
        fs::write(project.path().join("src.rs"), "").unwrap();
        let tool = worktree.path().join("build.sh");
        fs::write(&tool, "#!/bin/sh").unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
        let bases = [worktree.path().to_owned(), project.path().to_owned()];
        let references = [
            "src/lib.rs",
            "src.rs",
            "build.sh",
            "missing.rs",
            "~/nothing-here",
        ]
        .map(String::from);
        let resolved = resolve_paths(&references, &bases, Some(project.path()));
        let found = |path: PathBuf, runs: bool| {
            Some(ResolvedPath {
                path,
                runs_when_opened: runs,
            })
        };
        assert_eq!(
            resolved,
            [
                found(worktree.path().join("src/lib.rs"), false),
                found(project.path().join("src.rs"), false),
                found(tool, true),
                None,
                None,
            ]
        );
    }
}
