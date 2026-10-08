//! Small stdio MCP bridge. Credentials identify the caller; agents cannot supply a sender.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
};

pub fn read_json(reader: &mut impl BufRead) -> Result<Option<Value>> {
    let mut line = Vec::new();
    if reader.take(256 * 1024 + 1).read_until(b'\n', &mut line)? == 0 {
        return Ok(None);
    }
    ensure!(line.len() <= 256 * 1024, "Protocol message exceeds bound");
    Ok(Some(serde_json::from_slice(&line)?))
}
pub fn tools() -> Value {
    let string = json!({"type":"string"});
    let profile = json!({"type":"object","properties":{"provider":{"type":"string","enum":["claude","codex"]},"model":string,"effort":string},"required":["provider","model","effort"],"additionalProperties":false});
    let tool = |name: &str, description: &str, properties: Value, required: Vec<&str>| json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}});
    json!([
        tool(
            "workspace_context",
            "Read your identity, parent, team, repositories, tickets and runtime. The coordinator also gets model_selection: the providers and models allowed on this machine, each role's providers, the configured verifiers and the guide.",
            json!({}),
            vec![]
        ),
        tool(
            "create_ticket",
            "Coordinator: create a ticket in one of the project's repositories, with its own branch and worktree. Repeating the same repository and title returns the existing ticket.",
            json!({"repository_id":string,"title":string,"brief":string}),
            vec!["repository_id", "title", "brief"]
        ),
        tool(
            "assign_ticket",
            "Coordinator: assign a dedicated agent. Choose an exact profile from model_selection in workspace_context, within the role's providers (any enabled provider for a reviewer) and the models allowed on this machine, guided by its guide, and give a one-line reason naming the deciding factor. Other choices are refused and nothing is substituted. Existing assignments with the same role and focus are returned unchanged; use send_message for follow-up fixes. To add another agent in a role, such as a second reviewer, give each a distinct short focus.",
            json!({"ticket_id":string,"focus":string,"role":{"type":"string","enum":["implementer","tester","reviewer"]},"instruction":string,"profile":profile,"reason":string}),
            vec!["ticket_id", "role", "instruction", "profile", "reason"]
        ),
        tool(
            "verify_ticket",
            "Coordinator: once the implementer reports ready_for_testing, start every configured verifier on the ticket's current commit at once. For each verifier in model_selection.verifiers, give its focus, an exact profile within its provider (or, without one, its role's providers) and the models allowed on this machine, guided by the guide, and a one-line reason. Any refused choice starts no verifier, and nothing is substituted. A verifier session that already exists under that focus is reused only if your profile matches its pinned one; otherwise it is archived and a fresh verifier starts on your choice. Verifiers are read-only; failures go straight back to the implementer and only failed verifiers re-run, up to the round cap. You wake once, when the cycle passes or is blocked.",
            json!({"ticket_id":string,"verifiers":{"type":"array","items":{"type":"object","properties":{"focus":string,"profile":profile,"reason":string},"required":["focus","profile","reason"],"additionalProperties":false}}}),
            vec!["ticket_id", "verifiers"]
        ),
        tool(
            "send_message",
            "Immediately queue a message to your parent or direct child. Reuse message_id only when retrying identical content. Do not wait or poll for replies.",
            json!({"recipient":string,"message_id":string,"body":string}),
            vec!["recipient", "message_id", "body"]
        ),
        tool(
            "report",
            "Report progress, a blocker or a result to your parent. Progress is batched into its next turn; other kinds wake it immediately. Ask for a decision with kind blocked and the exact question. Give concrete evidence. Never wait for discovery.",
            json!({"message_id":string,"kind":{"type":"string","enum":["progress","blocked","ready_for_testing","passed","failed","completed"]},"body":string}),
            vec!["message_id", "kind", "body"]
        ),
        tool(
            "stop_turn",
            "Stop your direct child's running turn, for example after a check-in shows it stuck or on the wrong path. Give the reason; its next turn is told why. Its input is not retried, so send it what to do next. To let a turn continue, do nothing.",
            json!({"session_id":string,"reason":string}),
            vec!["session_id", "reason"]
        ),
        tool(
            "schedule",
            "Coordinator: set a durable timer that sends you prompt at its time, as your own message, even between turns and across host restarts. Give at (RFC 3339 or an offset like +90m), every (e.g. 30m, 2h; at least 5m) or both, and optionally until. Fires that pile up while you are busy or the host is stopped arrive once, marked late. Provider-native schedulers do not fire between managed turns; use this instead. Returns the timer id and next fire time.",
            json!({"label":string,"prompt":string,"at":string,"every":string,"until":string}),
            vec!["label", "prompt"]
        ),
        tool(
            "unschedule",
            "Coordinator: stop one of your timers. A fire still waiting in your queue is withdrawn; one held after an interrupted turn stays for Retry or Skip.",
            json!({"id":string}),
            vec!["id"]
        ),
        tool(
            "list_schedules",
            "Coordinator: list your active timers with their cadence and next fire time.",
            json!({}),
            vec![]
        ),
        tool(
            "accept_ticket",
            "Coordinator: accept a ticket whose verification passed at the commit still checked out. Does not merge or publish. Archives its agents and removes its worktree, keeping conversations and branch, once any agent still in its turn finishes; refuses while the worktree has uncommitted or untracked files.",
            json!({"ticket_id":string}),
            vec!["ticket_id"]
        ),
        tool(
            "close_ticket",
            "Coordinator: close a ticket that needs no more work, such as a finished review, once its findings are recorded. Archives its agents and removes its worktree; conversations and branch are kept. Refuses while the worktree has uncommitted or untracked files or an agent has not reported; an agent that reported and is still finishing its turn delays only the removal.",
            json!({"ticket_id":string}),
            vec!["ticket_id"]
        ),
        tool(
            "ask_user",
            "Coordinator: place one decision in front of the human, then finish your turn. The answer resumes you. When there are distinct choices, list them as options: short, self-explanatory labels the human can pick with one click. They can always answer in their own words instead.",
            json!({"request_id":string,"question":string,"options":{"type":"array","items":{"type":"string"},"maxItems":6}}),
            vec!["request_id", "question"]
        ),
        tool(
            "close_question",
            "Coordinator: remove your own open inbox question once it no longer needs the human, e.g. their message or new evidence already settled it.",
            json!({"request_id":string,"resolution":string}),
            vec!["request_id", "resolution"]
        ),
        tool(
            "request_permission",
            "Provider permission callback. Parks this operation until the human allows it once or denies it. Never call to bypass a denial.",
            json!({"tool_name":string,"input":{"type":"object","additionalProperties":true},"tool_use_id":string}),
            vec!["tool_name", "input"]
        )
    ])
}
pub fn serve_stdio() -> Result<()> {
    let socket = std::env::var("WORKSPACE_SOCKET").context("Missing workspace endpoint")?;
    let token = std::env::var("WORKSPACE_TOKEN").context("Missing session credential")?;
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    while let Some(request) = read_json(&mut input)? {
        let Some(id) = request.get("id") else {
            continue;
        };
        let result: Result<Value> = (|| {
            Ok(match request["method"].as_str().unwrap_or_default() {
                "initialize" => {
                    json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"wiffletree","version":"0.2.0"}})
                }
                "ping" => json!({}),
                "tools/list" => json!({"tools":tools()}),
                "tools/call" => {
                    let mut stream =
                        UnixStream::connect(&socket).context("Workspace host unavailable")?;
                    stream.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;
                    writeln!(
                        stream,
                        "{}",
                        json!({"token":token,"name":request["params"]["name"],"arguments":request["params"]["arguments"]})
                    )?;
                    let reply = read_json(&mut BufReader::new(stream))?
                        .context("Workspace disconnected")?;
                    let error = reply.get("error").and_then(Value::as_str);
                    json!({"isError":error.is_some(),"content":[{"type":"text","text":error.map(str::to_owned).unwrap_or_else(||reply["result"].to_string())}]})
                }
                _ => anyhow::bail!("Method not found"),
            })
        })();
        let response = match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
            Err(e) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":format!("{e:#}")}})
            }
        };
        writeln!(output, "{response}")?;
        output.flush()?;
    }
    Ok(())
}
