//! The shared boundary for Claude runs with no workspace tools or conversation.
use super::*;

/// Claude with every tool off but the one that returns its answer to `schema`, no MCP server,
/// hooks, workspace access, earlier conversation or stored session, told only `prompt`.
pub(super) fn configure(
    cmd: &mut ProcessCommand,
    profile: &ModelProfile,
    prompt: &str,
    schema: &str,
) {
    cmd.env_remove("WORKSPACE_SOCKET")
        .env_remove("WORKSPACE_TOKEN")
        .args([
            "-p",
            "--tools",
            "",
            "--strict-mcp-config",
            "--no-session-persistence",
        ])
        // Hooks would receive the input, such as untrusted comments, on stdin.
        .args(["--settings", r#"{"disableAllHooks":true}"#])
        .args(["--output-format", "stream-json", "--verbose", "--no-chrome"])
        .args(["--model", &profile.model, "--effort", &profile.effort])
        .args(["--system-prompt", prompt])
        .args(["--json-schema", schema]);
}

pub(super) struct Answer {
    pub answer: Option<Value>,
    pub usage: Value,
    pub tokens: u64,
}
pub(super) struct Run {
    label: &'static str,
    attested: bool,
}
impl Run {
    pub fn new(label: &'static str) -> Self {
        Self {
            label,
            attested: false,
        }
    }
    pub fn event(&mut self, value: &Value) -> Result<Option<Answer>> {
        if value["type"] == "system" && value["subtype"] == "init" {
            ensure!(
                value["tools"] == json!(["StructuredOutput"])
                    && value["mcp_servers"].as_array().is_some_and(Vec::is_empty),
                "The {} run started with tools {} and MCP servers {}{}",
                self.label,
                value["tools"],
                value["mcp_servers"],
                if self.label == "merge" { "." } else { "" }
            );
            self.attested = true;
        }
        if value["type"] != "result" || !self.attested {
            return Ok(None);
        }
        let usage = value["usage"].clone();
        let tokens = [
            "input_tokens",
            "cache_creation_input_tokens",
            "cache_read_input_tokens",
            "output_tokens",
        ]
        .iter()
        .filter_map(|key| usage[key].as_u64())
        .sum();
        Ok(Some(Answer {
            answer: (value["is_error"] != true && value["structured_output"].is_object())
                .then(|| value["structured_output"].clone()),
            usage,
            tokens,
        }))
    }
}
/// Keep a bounded stderr tail while stdout is consumed. A pipe inherited by a process
/// outside the provider's group cannot prevent the caller from completing.
pub(super) struct Errors {
    tail: Arc<std::sync::Mutex<Vec<u8>>>,
    closed: std::sync::mpsc::Receiver<()>,
}
impl Errors {
    pub fn spawn(mut pipe: impl Read + Send + 'static) -> Self {
        let tail = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (done, closed) = std::sync::mpsc::sync_channel(1);
        let held = tail.clone();
        std::thread::spawn(move || {
            let mut chunk = [0; 4096];
            while let Ok(read @ 1..) = pipe.read(&mut chunk) {
                let mut tail = held.lock().unwrap_or_else(|e| e.into_inner());
                tail.extend_from_slice(&chunk[..read]);
                if tail.len() > 16384 {
                    let excess = tail.len() - 16384;
                    tail.drain(..excess);
                }
            }
            let _ = done.send(());
        });
        Self { tail, closed }
    }
    pub fn finish(self) -> String {
        let _ = self.closed.recv_timeout(STDERR_DRAIN);
        without_terminal_codes(&String::from_utf8_lossy(
            &self.tail.lock().unwrap_or_else(|e| e.into_inner()),
        ))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_an_attested_run_can_return_an_answer_and_usage_counts_all_token_classes() {
        let mut run = Run::new("sorting");
        let result = json!({"type":"result","structured_output":{"ok":true},"usage":{
            "input_tokens":2,"cache_creation_input_tokens":3,"cache_read_input_tokens":5,"output_tokens":7}});
        assert!(run.event(&result).unwrap().is_none());
        assert!(
            run.event(&json!({"type":"system","subtype":"init","tools":["Bash"],"mcp_servers":[]}))
                .is_err()
        );
        run.event(&json!({"type":"system","subtype":"init","tools":["StructuredOutput"],"mcp_servers":[]})).unwrap();
        let answer = run.event(&result).unwrap().unwrap();
        assert_eq!(answer.tokens, 17);
        assert_eq!(answer.answer, Some(json!({"ok":true})));
        let mut failed = result;
        failed["is_error"] = json!(true);
        assert!(run.event(&failed).unwrap().unwrap().answer.is_none());
    }
    #[test]
    fn stderr_keeps_only_the_bounded_tail() {
        let mut data = vec![b'x'; 100_000];
        data.extend(b"end");
        let text = Errors::spawn(std::io::Cursor::new(data)).finish();
        assert_eq!(text.len(), 16384);
        assert!(text.ends_with("end"));
    }
}
