//! Wiffletree Evaluator (OpenSpec change `ui-evaluators`): the helper app that alone holds the
//! Screen Recording and Accessibility grants full-desktop UI evaluators use. The host starts
//! one per run through LaunchServices, so macOS charges the helper, not Wiffletree, for what it
//! does. It sees and touches only the apps the run names, and quits when the run ends.
//!
//! - `serve <socket> <run> [--model <label>] [--queued <n>] <bundle id>…` serves the run on its
//!   socket (see `protocol`) and shows the floating control panel (see `panel`).
//! - `check` prints the helper's grants as JSON.
//! - `request` asks macOS for both grants, which shows its prompts, waits up to two minutes
//!   for both, then prints them.
#[cfg(target_os = "macos")]
mod macos;
mod panel;
mod policy;
mod protocol;

use anyhow::{Result, bail};

const USAGE: &str = "Usage: wiffletree-evaluator serve <socket> <run> [--model <label>] [--queued <n>] <bundle id>… | check | request";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.split_first() {
        Some((mode, rest)) => run(mode, rest),
        None => bail!(USAGE),
    }
}

#[cfg(not(target_os = "macos"))]
fn run(_mode: &str, _args: &[String]) -> Result<()> {
    bail!("Wiffletree Evaluator runs only on macOS")
}

#[cfg(target_os = "macos")]
fn run(mode: &str, args: &[String]) -> Result<()> {
    let print = |grants: protocol::Grants| {
        println!(
            "{}",
            serde_json::json!({"screen_recording": grants.screen_recording, "accessibility": grants.accessibility})
        );
    };
    match (mode, args) {
        ("serve", [socket, run, rest @ ..]) => {
            let (model, queued, apps) = panel::options(rest)?;
            serve(socket.as_ref(), run, apps, model, queued)
        }
        ("check", []) => {
            print(macos::grants(false));
            Ok(())
        }
        ("request", []) => {
            print(macos::request_grants());
            Ok(())
        }
        _ => bail!(USAGE),
    }
}

/// How long the host has to open the control connection before the helper gives up.
#[cfg(target_os = "macos")]
const CONTROL_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

#[cfg(target_os = "macos")]
fn serve(
    socket: &std::path::Path,
    run: &str,
    apps: &[String],
    model: String,
    queued: usize,
) -> Result<()> {
    use std::sync::Arc;
    let allowed = policy::Allowed::new(apps)?;
    let _ = std::fs::remove_file(socket);
    // Only this user may connect: the socket is created with mode 600.
    // SAFETY: umask only changes this process's file creation mask.
    let mask = unsafe { libc::umask(0o177) };
    let listener = std::os::unix::net::UnixListener::bind(socket);
    // SAFETY: as above.
    unsafe { libc::umask(mask) };
    let listener = listener?;
    let control = Arc::new(protocol::Control::default());
    let socket = socket.to_owned();
    let quit = move || {
        macos::note("The run's control connection closed: quitting");
        let _ = std::fs::remove_file(&socket);
        std::process::exit(0);
    };
    {
        let (control, quit) = (control.clone(), quit.clone());
        std::thread::spawn(move || {
            std::thread::sleep(CONTROL_WAIT);
            if !control.held() {
                quit();
            }
        });
    }
    let serving = control.clone();
    let named = apps.to_vec();
    macos::run_app(run, control, model, &named, queued, move |escape| {
        let grants = macos::grants(escape);
        let hello = serde_json::json!({
            "pid": std::process::id(),
            "screen_recording": grants.screen_recording,
            "accessibility": grants.accessibility,
            "escape": grants.escape,
        });
        let desktop: Arc<dyn protocol::Desktop> = Arc::new(macos::Mac);
        std::thread::spawn(move || {
            protocol::serve(listener, desktop, allowed, hello, serving, quit);
        });
    })
}
