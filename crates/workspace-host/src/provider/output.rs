//! A provider's stdout, read on its own thread. The reader alone owns the pipe and decides
//! when to stop reading; the turn only takes the events it read, in order, however slowly.
use super::*;
use std::{
    collections::VecDeque,
    os::fd::AsRawFd,
    process::ChildStdout,
    sync::{Condvar, Mutex, MutexGuard},
};

pub(super) type OutputEvent = std::result::Result<Value, String>;

/// Events the reader may queue ahead of the turn while the provider runs, so a slow turn
/// holds back a fast provider.
const BUFFER: usize = 128;
const MAX_LINE: usize = 2 * 1024 * 1024;
/// How often a waiting reader looks at the stop flag and its deadlines.
const POLL: Duration = Duration::from_millis(20);
/// After the provider exits, its stdout is finite unless a process outside its group holds
/// the pipe. The reader then stops once the pipe has been quiet this long...
const IDLE: Duration = Duration::from_millis(500);
/// ...this long after the exit...
pub const READ_LIMIT: Duration = Duration::from_secs(5);
/// ...or once it has read this much since the exit. A provider blocked on a full pipe cannot
/// exit, so its own unread output is at most one pipe buffer (64 KiB on macOS); this leaves
/// ample margin and bounds what an outside writer can make the host hold.
pub const MAX_BYTES_AFTER_EXIT: usize = 1024 * 1024;

#[derive(Default)]
struct State {
    events: VecDeque<OutputEvent>,
    exited_at: Option<Instant>,
    /// The turn has ended and takes nothing more.
    stopped: bool,
    /// The reader has closed the pipe; what is queued is all there is.
    finished: bool,
}
pub(super) struct Output {
    state: Mutex<State>,
    changed: Condvar,
}
pub(super) enum Next {
    Event(OutputEvent),
    Waiting,
    Finished,
}
impl Output {
    pub(super) fn spawn(stdout: ChildStdout) -> Result<Arc<Self>> {
        let fd = stdout.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let output = Arc::new(Self {
            state: Mutex::default(),
            changed: Condvar::new(),
        });
        let reader = output.clone();
        std::thread::spawn(move || reader.read(stdout));
        Ok(output)
    }
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn update(&self, change: impl FnOnce(&mut State)) {
        change(&mut self.state());
        self.changed.notify_all();
    }
    /// From now on the reader never waits for room, and its deadlines run. Call this before
    /// stopping the process group, which can take its grace period.
    pub(super) fn provider_exited(&self) {
        self.update(|state| {
            state.exited_at.get_or_insert_with(Instant::now);
        });
    }
    /// The next event, waiting up to `wait` for one.
    pub(super) fn next(&self, wait: Duration) -> Next {
        let deadline = Instant::now() + wait;
        let mut state = self.state();
        loop {
            if let Some(event) = state.events.pop_front() {
                self.changed.notify_all();
                return Next::Event(event);
            }
            if state.finished {
                return Next::Finished;
            }
            let now = Instant::now();
            if now >= deadline {
                return Next::Waiting;
            }
            state = self
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
    fn read(&self, mut stdout: ChildStdout) {
        let mut chunk = vec![0; 64 * 1024];
        let mut line = Vec::new();
        // Since the provider exited: bytes read, and when the pipe last went quiet.
        let mut read_after_exit = 0;
        let mut quiet_since: Option<Instant> = None;
        loop {
            let (stopped, exited_at) = {
                let state = self.state();
                (state.stopped, state.exited_at)
            };
            if stopped {
                break;
            }
            let room = match exited_at {
                Some(_) => chunk.len().min(MAX_BYTES_AFTER_EXIT - read_after_exit),
                None => chunk.len(),
            };
            match stdout.read(&mut chunk[..room]) {
                Ok(0) => break,
                Ok(read) => {
                    quiet_since = None;
                    if !self.lines(&chunk[..read], &mut line) {
                        break;
                    }
                    if let Some(exited_at) = exited_at {
                        read_after_exit += read;
                        if read_after_exit >= MAX_BYTES_AFTER_EXIT
                            || exited_at.elapsed() >= READ_LIMIT
                        {
                            break;
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if let Some(exited_at) = exited_at {
                        let quiet = *quiet_since.get_or_insert_with(Instant::now);
                        if quiet.elapsed() >= IDLE || exited_at.elapsed() >= READ_LIMIT {
                            break;
                        }
                    }
                    let mut pipe = libc::pollfd {
                        fd: stdout.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    unsafe { libc::poll(&mut pipe, 1, POLL.as_millis() as i32) };
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        // An unterminated last line still counts, however reading ended; once the turn has
        // stopped the push is refused, and an oversized line was already reported.
        if !line.is_empty() && line.len() <= MAX_LINE {
            self.push(parse(&line));
        }
        // Closing the pipe tells a process still holding it that nobody is listening.
        drop(stdout);
        self.update(|state| state.finished = true);
    }
    /// Queues each complete line of `bytes`, keeping a partial one in `line`; false once the
    /// reader should stop.
    fn lines(&self, mut bytes: &[u8], line: &mut Vec<u8>) -> bool {
        while let Some(end) = bytes.iter().position(|&b| b == b'\n') {
            line.extend_from_slice(&bytes[..end]);
            bytes = &bytes[end + 1..];
            if line.len() > MAX_LINE {
                self.push(Err("Provider event exceeds bound".into()));
                return false;
            }
            if !line.is_empty() && !self.push(parse(line)) {
                return false;
            }
            line.clear();
        }
        line.extend_from_slice(bytes);
        if line.len() > MAX_LINE {
            self.push(Err("Provider event exceeds bound".into()));
            return false;
        }
        true
    }
    /// Queues an event, waiting for room only while the provider runs; false once the turn
    /// has stopped.
    fn push(&self, event: OutputEvent) -> bool {
        let mut state = self.state();
        while state.events.len() >= BUFFER && state.exited_at.is_none() && !state.stopped {
            state = self
                .changed
                .wait_timeout(state, POLL)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        if state.stopped {
            return false;
        }
        state.events.push_back(event);
        self.changed.notify_all();
        true
    }
}
fn parse(line: &[u8]) -> OutputEvent {
    serde_json::from_slice(line).map_err(|e| e.to_string())
}
/// Ends reading when the turn ends, however it ends; the reader closes the pipe within one
/// poll.
pub(super) struct StopReading(pub(super) Arc<Output>);
impl Drop for StopReading {
    fn drop(&mut self) {
        self.0.update(|state| state.stopped = true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn after_the_exit_the_reader_accepts_no_more_than_its_byte_cap() {
        let line = r#"{"type":"noise"}"#;
        let mut writer = ProcessCommand::new("yes")
            .arg(line)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let output = Output::spawn(writer.stdout.take().unwrap()).unwrap();
        // Everything it reads from here on counts against the cap.
        output.provider_exited();
        let (mut accepted, mut cut_short) = (0, 0);
        loop {
            match output.next(Duration::from_secs(1)) {
                Next::Event(Ok(_)) => accepted += line.len() + 1,
                // The line the cap cut, flushed like any unterminated last line.
                Next::Event(Err(_)) => cut_short += 1,
                Next::Waiting => {}
                Next::Finished => break,
            }
        }
        let _ = writer.kill();
        let _ = writer.wait();
        assert!(cut_short <= 1);
        // Each line is counted with its newline, which the flushed last line may lack.
        assert!(accepted <= MAX_BYTES_AFTER_EXIT + 1, "{accepted} bytes");
        assert!(
            accepted > MAX_BYTES_AFTER_EXIT - 2 * (line.len() + 1),
            "{accepted} bytes"
        );
    }
}
