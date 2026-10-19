//! Starting child processes that inherit nothing they should not.
use std::{os::unix::process::CommandExt, process::Command};

pub trait OwnDescriptors {
    /// Leaves the child only its standard input, output and error. macOS cannot make a pipe
    /// that is close-on-exec from the start, so a child started while another thread is making
    /// one would otherwise hold it for as long as it and everything it started run.
    fn own_descriptors_only(&mut self) -> &mut Self;
}
impl OwnDescriptors for Command {
    fn own_descriptors_only(&mut self) -> &mut Self {
        // SAFETY: `close_others_on_exec` is async-signal-safe.
        unsafe {
            self.pre_exec(|| {
                close_others_on_exec::<1024>();
                Ok(())
            })
        }
    }
}

/// Marks every descriptor above stderr close-on-exec, from a list of up to `LISTED` of them.
///
/// It runs between fork and exec in a copy of a process that had other threads, any of which
/// may have held a lock, the allocator's included. So it only makes system calls: it must not
/// allocate, lock, log or panic.
fn close_others_on_exec<const LISTED: usize>() {
    let close_on_exec = |descriptor| unsafe {
        libc::fcntl(descriptor, libc::F_SETFD, libc::FD_CLOEXEC);
    };
    #[cfg(target_os = "macos")]
    {
        let mut open: [libc::proc_fdinfo; LISTED] = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&open) as libc::c_int;
        let list = libc::PROC_PIDLISTFDS;
        let read =
            unsafe { libc::proc_pidinfo(libc::getpid(), list, 0, open.as_mut_ptr().cast(), size) };
        // A list that filled the buffer may be a cut one. That and a failed call mark every
        // possible descriptor below, so none stays inheritable.
        if (1..size).contains(&read) {
            let listed = read as usize / std::mem::size_of::<libc::proc_fdinfo>();
            let above_stderr = open
                .iter()
                .take(listed)
                .map(|d| d.proc_fd)
                .filter(|d| *d > 2);
            return above_stderr.for_each(close_on_exec);
        }
    }
    (3..unsafe { libc::getdtablesize() }).for_each(close_on_exec);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_whose_descriptor_list_may_be_cut_short_marks_every_descriptor() {
        // More inheritable descriptors than a list of four can hold, so the list fills and the
        // child marks every number up to its limit instead.
        let mut pipes = [[0; 2]; 4];
        for ends in &mut pipes {
            assert_eq!(unsafe { libc::pipe(ends.as_mut_ptr()) }, 0);
        }
        let mut ls = Command::new("/bin/ls");
        ls.arg("/dev/fd");
        // SAFETY: as in `own_descriptors_only`.
        unsafe {
            ls.pre_exec(|| {
                close_others_on_exec::<4>();
                Ok(())
            })
        };
        let output = ls.output();
        for descriptor in pipes.iter().flatten() {
            unsafe { libc::close(*descriptor) };
        }
        // `ls` holds the standard three and the two it opens to read `/dev/fd`.
        let held = String::from_utf8(output.unwrap().stdout).unwrap();
        assert_eq!(held, "0\n1\n2\n3\n4\n");
    }
}
