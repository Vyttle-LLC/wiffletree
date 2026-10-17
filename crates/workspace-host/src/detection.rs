//! Whether each provider's CLI is installed and signed in, and on which plan. Reads the CLIs'
//! own status only: no inference, and no token contents leave this module.
use crate::runtime::LowPriority as _;
use crate::*;
use base64::Engine as _;
use std::{process::Command as ProcessCommand, time::Duration};

const TIMEOUT: Duration = Duration::from_secs(10);

/// Whether `provider`'s CLI is installed and signed in, and the models it lists. Blocks like
/// `detect`.
pub fn report(provider: Provider) -> ProviderReport {
    ProviderReport {
        status: detect(provider),
        models: crate::runtime::models(provider).unwrap_or_default(),
    }
}

/// Blocks for up to `TIMEOUT`; call it off the UI thread. A missing, slow or failing CLI
/// reads as not signed in.
pub fn detect(provider: Provider) -> ProviderStatus {
    let Ok(binary) = provider::executable(provider) else {
        return ProviderStatus::default();
    };
    let (signed_in, plan) = match provider {
        Provider::Claude => status_output(&binary, &["auth", "status", "--json"])
            .map(|[stdout, _]| claude_status(&stdout))
            .unwrap_or_default(),
        Provider::Codex => {
            // Codex prints its status on stderr.
            let signed_in = status_output(&binary, &["login", "status"])
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

/// The command's stdout and stderr, or `None` when it cannot run or outlasts `TIMEOUT`, in
/// which case it is killed.
fn status_output(binary: &Path, args: &[&str]) -> Option<[String; 2]> {
    let mut command = ProcessCommand::new(binary);
    command.args(args).at_low_priority();
    let (_, stdout, stderr) = runtime::bounded_output(command, TIMEOUT, None).ok()?;
    Some([stdout, stderr].map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
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
}
