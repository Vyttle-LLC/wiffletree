//! The launchd host's versioned copies, `<data folder>/host/<version>/workspace-host`. A launchd
//! host runs from one, and starts its agents' helpers from it, so swapping the app bundle never
//! touches a running host. A child host runs from the bundle and has no copy.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// The folder of the copies, which also holds the launchd plist.
pub const FOLDER: &str = "host";

/// Where `version`'s copy of the host lives.
pub fn path(data: &Path, version: &str) -> PathBuf {
    data.join(FOLDER).join(version).join("workspace-host")
}

/// Copies `source`, the app bundle's host, to `version`'s copy and returns it. It replaces an
/// existing copy whole, by a rename, so a development build of the same version runs its own
/// binary. The app calls this only while no host runs.
pub fn install(source: &Path, data: &Path, version: &str) -> io::Result<PathBuf> {
    let copy = path(data, version);
    let folder = copy.parent().expect("a copy is in its version's folder");
    fs::create_dir_all(folder)?;
    let partial = folder.join("workspace-host.partial");
    fs::copy(source, &partial)?;
    fs::rename(&partial, &copy)?;
    Ok(copy)
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
    fn install_copies_the_bundles_host_over_an_earlier_copy_of_its_version() {
        use std::os::unix::fs::PermissionsExt as _;
        let data = tempfile::tempdir().unwrap();
        let source = data.path().join("bundle-host");
        fs::write(&source, "new build").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();
        install(data.path(), "1.0.0");

        let copy = super::install(&source, data.path(), "1.0.0").unwrap();

        assert_eq!(copy, path(data.path(), "1.0.0"));
        assert_eq!(fs::read_to_string(&copy).unwrap(), "new build");
        let mode = fs::metadata(&copy).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "it stays executable");
        let entries = fs::read_dir(copy.parent().unwrap()).unwrap().count();
        assert_eq!(entries, 1, "no partial copy is left");
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
