//! Each ticket's scratch folder, its agents' `TMPDIR`: what their builds and tools write outside
//! the worktree lands there, where the host can find and reclaim it, instead of in folders under
//! `/tmp` that nobody removes. It lies outside every worktree, so it never makes one unclean.
use std::path::{Path, PathBuf};

/// The scratch folder of `ticket`'s agents, in the system's temporary folder, whose path has no
/// spaces. Its name is a hash of the ticket id, not the id itself, so the path stays short:
/// macOS refuses Unix socket paths of 104 bytes or more, and tools bind sockets in `TMPDIR`.
pub fn ticket_scratch(ticket: &str) -> PathBuf {
    let hash = crate::build_gate::fnv(Path::new(ticket));
    std::env::temp_dir().join(format!("wt-{hash:016x}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_ticket_gets_its_own_short_scratch_folder() {
        let ticket = "0cd97fd7-4619-48a9-9eaa-39cf33ead8ce";
        let scratch = ticket_scratch(ticket);
        assert_eq!(scratch, ticket_scratch(ticket));
        assert_ne!(
            scratch,
            ticket_scratch("a2b3c4d5-4619-48a9-9eaa-39cf33ead8ce")
        );
        assert!(scratch.starts_with(std::env::temp_dir()));
        // Room for a socket in a temporary folder of its own, as `tempfile` makes them:
        // `/.tmpXXXXXX/host.sock` must still fit macOS's 103 bytes after macOS's own TMPDIR.
        let added = scratch.as_os_str().len() - std::env::temp_dir().as_os_str().len();
        assert!(added <= 20, "{}", scratch.display());
    }
}
