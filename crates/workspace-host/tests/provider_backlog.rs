//! How a provider's stdout is read once the provider exits, through `provider::run`: every
//! event it wrote is delivered however slowly the turn handles them, and processes outside its
//! group can neither keep the turn open nor keep the pipe. Kept in its own test binary because
//! it sets the provider environment for the process.
use std::{
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};
use workspace_core::*;
use workspace_host::{
    Host,
    provider::{self, ProviderEvent, Turn},
};

/// Writes STEPS=n steps and the final reply, then exits. Its prompt can add, before that:
/// STUBBORN, a child in its group that ignores SIGTERM, so stopping the group takes its grace;
/// WRITER, a detached process outside the group writing a line every 1 ms, which records when
/// it finds the pipe closed; FLOOD, one writing
/// 1 KiB lines as fast as it can once the provider has exited, which records how much it wrote
/// once the pipe closes; QUIET,
/// one holding stdout silently, which records whether a write 2 s later still reaches a reader;
/// UNTERMINATED, no newline after the final event.
/// Files go to $FIXTURE_DIR, starting with the provider's own pid.
const PROVIDER: &str = r#"#!/bin/sh
echo $$ > "$FIXTURE_DIR/leader.pid"
prompt=$(cat)
steps=$(echo "$prompt" | sed -n 's/.*STEPS=\([0-9]*\).*/\1/p')
case "$prompt" in *STUBBORN*) (trap '' TERM; exec sleep 30) </dev/null >/dev/null 2>&1 & ;; esac
# Each detached helper marks when it has left the group, so stopping the group cannot catch it.
detached() { while [ ! -e "$FIXTURE_DIR/$1.detached" ]; do sleep 0.01; done; rm "$FIXTURE_DIR/$1.detached"; }
case "$prompt" in *WRITER*)
  perl -e 'use POSIX; setsid(); open M, ">", "$ENV{FIXTURE_DIR}/writer.detached"; close M; $SIG{PIPE}="IGNORE"; $|=1; while (print qq({"type":"noise"}\n)) { select(undef,undef,undef,0.001) } open F, ">", "$ENV{FIXTURE_DIR}/writer.closed"; print F "closed"' &
  echo $! > "$FIXTURE_DIR/writer.pid"; detached writer;;
esac
case "$prompt" in *FLOOD*)
  perl -e 'use POSIX; setsid(); open M, ">", "$ENV{FIXTURE_DIR}/flood.detached"; close M; sleep 0.01 while getppid() != 1; $SIG{PIPE}="IGNORE"; $|=1; $l=q({"type":"noise","pad":").("x" x 1000).qq("}\n); $n=0; while (print $l) { $n+=length $l } open F, ">", "$ENV{FIXTURE_DIR}/flood.bytes"; print F $n' & detached flood;;
esac
case "$prompt" in *QUIET*)
  perl -e 'use POSIX; setsid(); open M, ">", "$ENV{FIXTURE_DIR}/quiet.detached"; close M; $SIG{PIPE}="IGNORE"; $|=1; sleep 2; $ok=print "late\n"; open F, ">", "$ENV{FIXTURE_DIR}/quiet.result"; print F ($ok ? "written" : "closed")' & detached quiet;;
esac
echo '{"type":"thread.started","thread_id":"backlog"}'
i=0
while [ $i -lt $steps ]; do
  echo "{\"type\":\"item.completed\",\"item\":{\"id\":\"step-$i\",\"type\":\"command_execution\",\"command\":\"true\",\"aggregated_output\":\"\",\"exit_code\":0,\"status\":\"completed\"}}"
  i=$((i+1))
done
echo '{"type":"item.completed","item":{"id":"reply","type":"agent_message","text":"DONE"}}'
case "$prompt" in
  *UNTERMINATED*) printf '%s' '{"type":"turn.completed","usage":{}}';;
  *) echo '{"type":"turn.completed","usage":{}}';;
esac
"#;

struct Outcome {
    steps: usize,
    reply: Option<String>,
    error: Option<Option<String>>,
    took: Duration,
    /// How long the outside writer was still read once the provider had been reaped.
    read_after_exit: Option<Duration>,
    steps_after_reading: usize,
}

/// Runs one turn whose every step takes 2 ms to handle. With `hold_until_closed`, the first
/// step after the provider has been reaped waits until the outside writer finds the pipe
/// closed, so the rest of the backlog is handled after reading has ended, however fast or slow
/// the machine is.
fn run(home: &Path, prompt: &str, hold_until_closed: bool) -> Outcome {
    let mut session = Host::open(home).unwrap().sessions().unwrap().remove(0);
    session.provider = Provider::Codex;
    let turn = Turn {
        run_id: new_id(),
        session,
        profile: ModelProfile {
            provider: Provider::Codex,
            model: "gpt-6.1-sol".into(),
            effort: "medium".into(),
        },
        provider_session: None,
        cwd: home.into(),
        prompt: prompt.into(),
        attachments: vec![],
        socket: home.join("unused"),
        token: String::new(),
        helper: home.join("provider"),
    };
    let mut outcome = Outcome {
        steps: 0,
        reply: None,
        error: None,
        took: Duration::ZERO,
        read_after_exit: None,
        steps_after_reading: 0,
    };
    let started = Instant::now();
    provider::run(
        turn,
        Arc::new(AtomicBool::new(false)),
        |event| match event {
            // Each event reaches the actor through a blocking send; here it is slower still.
            ProviderEvent::Step(_) => {
                outcome.steps += 1;
                if outcome.read_after_exit.is_some() {
                    outcome.steps_after_reading += 1;
                } else if hold_until_closed && reaped(home) {
                    let held = Instant::now();
                    recorded(&home.join("writer.closed"));
                    outcome.read_after_exit = Some(held.elapsed());
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            ProviderEvent::Reply(text) => outcome.reply = Some(text),
            ProviderEvent::Finished { error, .. } => outcome.error = Some(error),
            _ => {}
        },
    )
    .unwrap();
    outcome.took = started.elapsed();
    outcome
}

/// The turn reaps the provider only after it has told the reader of the exit.
fn reaped(home: &Path) -> bool {
    let leader: i32 = recorded(&home.join("leader.pid")).trim().parse().unwrap();
    let gone = unsafe { libc::kill(leader, 0) } == -1;
    gone && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// Waits for a fixture process to write its file.
fn recorded(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(path)
            && !text.is_empty()
        {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "{} was never written",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_exited_providers_output_is_read_fully_and_its_pipe_let_go() {
    let home = tempfile::tempdir().unwrap();
    let provider = home.path().join("provider");
    std::fs::write(&provider, PROVIDER).unwrap();
    std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: this test binary holds one test, so nothing else reads the environment meanwhile.
    unsafe {
        std::env::set_var("WORKSPACE_CODEX_BIN", &provider);
        std::env::set_var("FIXTURE_DIR", home.path());
    }
    Host::open(home.path())
        .unwrap()
        .create_project("Backlog")
        .unwrap();

    // A backlog still being handled after reading has ended, while stopping the group takes
    // its grace and an outside writer keeps writing: reading stops at its limit after the
    // exit, and every step and the reply arrive. More steps than the pipe and the reader's
    // queue hold, so the provider also waits for the turn before it can exit.
    let backlog = run(home.path(), "STEPS=1000 STUBBORN WRITER", true);
    assert!(backlog.steps > 1000, "{} steps", backlog.steps);
    assert_eq!(backlog.reply.as_deref(), Some("DONE"));
    assert_eq!(backlog.error, Some(None), "the turn succeeds");
    let read = backlog
        .read_after_exit
        .expect("steps remained when the provider was reaped");
    // The margin covers the reader's poll and the writer noticing the closed pipe.
    assert!(
        read < provider::READ_LIMIT + Duration::from_secs(1),
        "the writer was read for {read:?} after the exit"
    );
    assert!(
        backlog.steps_after_reading > 0,
        "steps are handled after reading ends"
    );
    let writer: i32 = recorded(&home.path().join("writer.pid"))
        .trim()
        .parse()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { libc::kill(writer, 0) } == 0 {
        if Instant::now() > deadline {
            unsafe { libc::kill(writer, libc::SIGKILL) };
            panic!("the outside writer outlived the turn");
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    // A writer as fast as it can go after the exit is read only up to the cap. Besides that it
    // can have written what the reader held before it learnt of the exit (its 128-event queue
    // of 1 KiB lines and its 64 KiB read chunk) and the unread pipe buffer.
    let flood = run(home.path(), "STEPS=20 FLOOD", false);
    assert_eq!(flood.reply.as_deref(), Some("DONE"));
    assert_eq!(flood.error, Some(None));
    let written: usize = recorded(&home.path().join("flood.bytes"))
        .trim()
        .parse()
        .unwrap();
    const HELD_AROUND_THE_EXIT: usize = 128 * 1024 + 64 * 1024 + 64 * 1024;
    assert!(
        written <= provider::MAX_BYTES_AFTER_EXIT + HELD_AROUND_THE_EXIT,
        "{written} bytes were taken"
    );

    // A silent holder does not keep the turn open, and the pipe is closed behind it.
    let quiet = run(home.path(), "STEPS=20 QUIET", false);
    assert_eq!(quiet.reply.as_deref(), Some("DONE"));
    assert!(quiet.took < Duration::from_secs(2), "{:?}", quiet.took);
    assert_eq!(recorded(&home.path().join("quiet.result")), "closed");

    // An unterminated final event still ends the turn, though the holder keeps the pipe open
    // until reading stops at its idle deadline.
    std::fs::remove_file(home.path().join("quiet.result")).unwrap();
    let unterminated = run(home.path(), "STEPS=20 QUIET UNTERMINATED", false);
    assert_eq!(unterminated.reply.as_deref(), Some("DONE"));
    assert_eq!(unterminated.error, Some(None), "the turn succeeds");
    assert_eq!(recorded(&home.path().join("quiet.result")), "closed");
}
