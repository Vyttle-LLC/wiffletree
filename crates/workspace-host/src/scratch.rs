//! Each ticket's scratch folder, its agents' `TMPDIR`: what their builds and tools write outside
//! the worktree lands there, where the host can find and reclaim it, instead of in folders under
//! `/tmp` that nobody removes. It lies outside every worktree, so it never makes one unclean.
use std::path::{Path, PathBuf};

/// The scratch folder of `ticket`'s agents under the host of `data`. It sits beside the build
/// gate's folder, whose path has no spaces, which some build tools' variables cannot hold.
pub fn ticket_scratch(data: &Path, ticket: &str) -> PathBuf {
    crate::build_gate::temp_folder(data)
        .join("scratch")
        .join(ticket)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_ticket_gets_its_own_scratch_folder_outside_the_data_folder() {
        let data = Path::new("/Users/me/Library/Application Support/Wiffletree");
        let scratch = ticket_scratch(data, "ticket-1");
        assert_eq!(scratch, ticket_scratch(data, "ticket-1"));
        assert_ne!(scratch, ticket_scratch(data, "ticket-2"));
        assert!(scratch.starts_with(std::env::temp_dir()));
        assert!(!scratch.to_string_lossy().contains(' '));
    }
}
