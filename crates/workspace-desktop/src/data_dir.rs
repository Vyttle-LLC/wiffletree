use std::path::{Path, PathBuf};

/// Set at build time by `package_macos.sh --beta`. A beta runs beside the release app with its
/// own store, so it never migrates or locks the store live sessions are using.
pub fn is_beta() -> bool {
    option_env!("WIFFLETREE_CHANNEL") == Some("beta")
}

pub fn default_directory(home: &Path) -> PathBuf {
    directory(home, is_beta())
}

fn directory(home: &Path, beta: bool) -> PathBuf {
    let support = home.join("Library/Application Support");
    if beta {
        return support.join("Wiffletree Beta");
    }
    let current = support.join("Wiffletree");
    let legacy = support.join("Agent Workspace");
    // Moving the old store would invalidate absolute worktree and provider-session paths.
    if !current.exists() && legacy.exists() {
        legacy
    } else {
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_reuses_existing_data_without_moving_it() {
        let home = tempfile::tempdir().unwrap();
        let legacy = home
            .path()
            .join("Library/Application Support/Agent Workspace");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("workspace.sqlite3"), b"existing store").unwrap();
        assert_eq!(directory(home.path(), false), legacy);
        assert_eq!(
            std::fs::read(legacy.join("workspace.sqlite3")).unwrap(),
            b"existing store"
        );
        assert!(
            !home
                .path()
                .join("Library/Application Support/Wiffletree")
                .exists()
        );
    }

    #[test]
    fn fresh_or_explicitly_created_wiffletree_store_has_priority() {
        let home = tempfile::tempdir().unwrap();
        let current = home.path().join("Library/Application Support/Wiffletree");
        assert_eq!(directory(home.path(), false), current);
        std::fs::create_dir_all(&current).unwrap();
        std::fs::create_dir_all(
            home.path()
                .join("Library/Application Support/Agent Workspace"),
        )
        .unwrap();
        assert_eq!(directory(home.path(), false), current);
    }

    #[test]
    fn beta_never_shares_the_release_store() {
        let home = tempfile::tempdir().unwrap();
        let support = home.path().join("Library/Application Support");
        std::fs::create_dir_all(support.join("Wiffletree")).unwrap();
        std::fs::create_dir_all(support.join("Agent Workspace")).unwrap();
        assert_eq!(
            directory(home.path(), true),
            support.join("Wiffletree Beta")
        );
    }
}
