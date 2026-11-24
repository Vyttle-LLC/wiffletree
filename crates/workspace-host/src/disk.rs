//! Free space on a disk, as both the host and the build gate read it.
use std::{ffi::CString, os::unix::ffi::OsStrExt, path::Path};

/// Free and total bytes on the disk holding `path`, as an unprivileged user may use them.
// The field types differ by platform, so a conversion may be one to the same type.
#[allow(clippy::useless_conversion)]
pub fn space(path: &Path) -> Option<(u64, u64)> {
    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: an all-zero statvfs is a valid value for the call to fill in.
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is a valid C string and `stats` a valid place to write.
    if unsafe { libc::statvfs(path.as_ptr(), &mut stats) } != 0 {
        return None;
    }
    let block = u64::from(stats.f_frsize).max(1);
    let (free, total) = (u64::from(stats.f_bavail), u64::from(stats.f_blocks));
    Some((free.saturating_mul(block), total.saturating_mul(block)))
}

/// Free bytes on the disk holding `path`.
pub fn free(path: &Path) -> Option<u64> {
    space(path).map(|(free, _)| free)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_real_folder_has_some_free_space_within_its_disk() {
        let folder = tempfile::tempdir().unwrap();
        let (free, total) = super::space(folder.path()).unwrap();
        assert!(total > 0 && free <= total);
        assert_eq!(super::space("/no/such/folder".as_ref()), None);
    }
}
