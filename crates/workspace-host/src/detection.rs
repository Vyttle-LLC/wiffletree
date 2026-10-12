//! Whether each provider's CLI is installed and signed in, and on which plan. Reads the CLIs'
//! own status only: no inference, and no token contents leave this module.
use crate::*;
use base64::Engine as _;
use std::{
    io::Read,
    process::{Command as ProcessCommand, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(10);
const OUTPUT_LIMIT: u64 = 65_536;

/// Blocks for up to `TIMEOUT`; call it off the UI thread. A missing, slow or failing CLI
/// reads as not signed in.
pub fn detect(provider: Provider) -> ProviderStatus {
    let Ok(binary) = provider::executable(provider) else {
        return ProviderStatus::default();
    };
    let (signed_in, plan) = match provider {
        Provider::Claude => status_output(&binary, &["auth", "status", "--json"], TIMEOUT)
            .map(|[stdout, _]| claude_status(&stdout))
            .unwrap_or_default(),
        Provider::Codex => {
            // Codex prints its status on stderr.
            let signed_in = status_output(&binary, &["login", "status"], TIMEOUT)
                .is_some_and(|streams| streams.iter().any(|text| codex_signed_in(text)));
            let plan = context::codex_home()
                .ok()
                .filter(|_| signed_in)
                .and_then(|home| codex_plan(&home.join("auth.json")));
            (signed_in, plan)
        }
    };
    ProviderStatus {
        installed: true,
        signed_in,
        plan,
    }
}

/// The command's stdout and stderr, or `None` when it cannot run or outlasts `timeout`, in
/// which case it is killed.
fn status_output(binary: &Path, args: &[&str], timeout: Duration) -> Option<[String; 2]> {
    struct Killed(std::process::Child);
    impl Drop for Killed {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Killed(
        ProcessCommand::new(binary)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .ok()?,
    );
    let read = |stream: Box<dyn Read + Send>| {
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stream.take(OUTPUT_LIMIT).read_to_end(&mut bytes);
            let _ = sender.send(String::from_utf8_lossy(&bytes).into_owned());
        });
        receiver
    };
    let stdout = read(Box::new(child.0.stdout.take()?));
    let stderr = read(Box::new(child.0.stderr.take()?));
    let deadline = Instant::now() + timeout;
    let left = || deadline.saturating_duration_since(Instant::now());
    Some([
        stdout.recv_timeout(left()).ok()?,
        stderr.recv_timeout(left()).ok()?,
    ])
}

/// Signed in and the plan, from `claude auth status --json`.
fn claude_status(output: &str) -> (bool, Option<String>) {
    let Ok(status) = serde_json::from_str::<Value>(output) else {
        return (false, None);
    };
    let signed_in = status["loggedIn"] == true;
    let plan = status["subscriptionType"]
        .as_str()
        .filter(|plan| signed_in && !plan.is_empty())
        .map(str::to_owned);
    (signed_in, plan)
}

/// `codex login status` says "Logged in using …", or "Not logged in".
fn codex_signed_in(output: &str) -> bool {
    output
        .lines()
        .any(|line| line.trim_start().starts_with("Logged in"))
}

/// The `chatgpt_plan_type` claim of the ID token Codex saved. Best effort: any failure to
/// read it means the plan is unknown.
fn codex_plan(auth_file: &Path) -> Option<String> {
    let auth: Value = serde_json::from_slice(&fs::read(auth_file).ok()?).ok()?;
    let payload = auth["tokens"]["id_token"].as_str()?.split('.').nth(1)?;
    let claims: Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .ok()?,
    )
    .ok()?;
    claims["https://api.openai.com/auth"]["chatgpt_plan_type"]
        .as_str()
        .filter(|plan| !plan.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_status_reads_sign_in_and_plan() {
        assert_eq!(
            claude_status(r#"{"loggedIn":true,"authMethod":"claude.ai","subscriptionType":"max"}"#),
            (true, Some("max".into()))
        );
        // An API-key sign-in reports no plan.
        assert_eq!(claude_status(r#"{"loggedIn":true}"#), (true, None));
        assert_eq!(
            claude_status(r#"{"loggedIn":false,"subscriptionType":"max"}"#),
            (false, None)
        );
        for malformed in [
            "",
            "Not logged in",
            "{\"loggedIn\":",
            "[]",
            "{\"loggedIn\":\"yes\"}",
        ] {
            assert_eq!(claude_status(malformed), (false, None), "{malformed}");
        }
    }

    #[test]
    fn codex_status_needs_the_logged_in_line() {
        assert!(codex_signed_in("Logged in using ChatGPT\n"));
        assert!(codex_signed_in(
            "warning: update available\n  Logged in using an API key\n"
        ));
        for signed_out in ["", "Not logged in\n", "error: not logged in", "garbage"] {
            assert!(!codex_signed_in(signed_out), "{signed_out}");
        }
    }

    fn auth_file(directory: &Path, contents: &str) -> PathBuf {
        let path = directory.join("auth.json");
        fs::write(&path, contents).unwrap();
        path
    }
    fn id_token(claims: Value) -> String {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string());
        format!("header.{payload}.signature")
    }

    #[test]
    fn codex_plan_comes_from_the_id_token_claim() {
        let directory = tempfile::tempdir().unwrap();
        let token = id_token(json!({"https://api.openai.com/auth":{"chatgpt_plan_type":"plus"}}));
        let path = auth_file(
            directory.path(),
            &json!({"tokens":{"id_token":token}}).to_string(),
        );
        assert_eq!(codex_plan(&path), Some("plus".into()));
    }

    #[test]
    fn an_unreadable_or_malformed_codex_auth_file_means_plan_unknown() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(codex_plan(&directory.path().join("missing.json")), None);
        // A directory in the file's place cannot be read.
        assert_eq!(codex_plan(directory.path()), None);
        let no_claim = id_token(json!({"email":"fixture@example.invalid"}));
        for contents in [
            "",
            "not json",
            "{}",
            r#"{"OPENAI_API_KEY":"fixture","tokens":null}"#,
            r#"{"tokens":{"id_token":"one-part"}}"#,
            r#"{"tokens":{"id_token":"header.!!not-base64!!.signature"}}"#,
            r#"{"tokens":{"id_token":"header.bm90IGpzb24.signature"}}"#,
            &json!({"tokens":{"id_token":no_claim}}).to_string(),
        ] {
            let path = auth_file(directory.path(), contents);
            assert_eq!(codex_plan(&path), None, "{contents}");
        }
    }

    #[test]
    fn a_cli_that_cannot_run_gives_no_output() {
        assert!(status_output(Path::new("/nonexistent/cli"), &["status"], TIMEOUT).is_none());
    }

    #[test]
    fn a_finished_cli_gives_both_streams() {
        let [stdout, stderr] = status_output(
            Path::new("/bin/sh"),
            &["-c", "echo out; echo err >&2"],
            TIMEOUT,
        )
        .unwrap();
        assert_eq!((stdout.as_str(), stderr.as_str()), ("out\n", "err\n"));
    }

    #[test]
    fn a_hanging_cli_is_given_up_on_at_the_timeout_and_killed() {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("pid");
        let script = format!("echo $$ > '{}'; exec sleep 30", pid_file.display());
        let started = Instant::now();
        let output = status_output(
            Path::new("/bin/sh"),
            &["-c", &script],
            Duration::from_millis(300),
        );
        assert!(output.is_none());
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid = fs::read_to_string(&pid_file).unwrap();
        let alive = ProcessCommand::new("kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(!alive, "the hanging CLI was left running");
    }
}
