//! The launchd host's versioned copies, `<data folder>/host/<version>/workspace-host`. A launchd
//! host runs from one, and starts its agents' helpers from it, so swapping the app bundle never
//! touches a running host. A child host runs from the bundle and has no copy.
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The folder of the copies, which also holds the launchd plist.
pub const FOLDER: &str = "host";

/// Where `version`'s copy of the host lives.
pub fn path(data: &Path, version: &str) -> PathBuf {
    data.join(FOLDER).join(version).join("workspace-host")
}

/// When `binary` is one of `data`'s copies, removes every other copy: one host owns a store, so
/// no other copy is in use. The app copies a missing version again before it starts it, so a
/// copy removed here as an update arrives is only copied twice. A failure is logged.
pub fn remove_unused(data: &Path, binary: &Path) {
    let (Ok(binary), Ok(folder)) = (binary.canonicalize(), data.join(FOLDER).canonicalize()) else {
        return;
    };
    let Some(own) = binary.parent().filter(|own| own.parent() == Some(&*folder)) else {
        return;
    };
    let Ok(entries) = fs::read_dir(&folder) else {
        return;
    };
    for entry in entries.flatten() {
        let copy = entry.path();
        if copy != own
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
            && let Err(error) = fs::remove_dir_all(&copy)
        {
            eprintln!("Remove the unused host copy {}: {error}", copy.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(data: &Path, version: &str) -> PathBuf {
        let copy = path(data, version);
        fs::create_dir_all(copy.parent().unwrap()).unwrap();
        fs::write(&copy, version).unwrap();
        copy
    }

    #[test]
    fn a_copy_removes_the_other_copies_and_keeps_the_plist() {
        let data = tempfile::tempdir().unwrap();
        let old = install(data.path(), "0.1.0");
        let own = install(data.path(), "0.2.0");
        let plist = data
            .path()
            .join(FOLDER)
            .join("com.vyttle.wiffletree.host.plist");
        fs::write(&plist, "").unwrap();

        remove_unused(data.path(), &own);

        assert!(own.exists() && plist.exists());
        assert!(!old.parent().unwrap().exists());
    }

    #[test]
    fn a_host_outside_the_copies_removes_nothing() {
        let data = tempfile::tempdir().unwrap();
        let copy = install(data.path(), "0.1.0");
        let elsewhere = data.path().join("workspace-host");
        fs::write(&elsewhere, "").unwrap();

        remove_unused(data.path(), &elsewhere);

        assert!(copy.exists());
    }
}
