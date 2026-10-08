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
    let tool = |name: &str, description: &str, properties: Value, required: Vec<&str>| json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}});
    json!([
        tool(
            "workspace_context",
            "Read your identity, parent, team, repositories, tickets, runtime and role policies.",
            json!({}),
            vec![]
        ),
        tool(
            "create_repo_coordinator",
            "Main coordinator: create one persistent repository coordinator using its saved provider and Big/Small profile. Existing coordinators keep their profile. Then send its brief.",
            json!({"repository_id":string,"provider":{"type":"string","enum":["claude","codex"]},"size":{"type":"string","enum":["big","small"]}}),
            vec!["repository_id"]
        ),
        tool(
            "create_ticket",
            "Repository coordinator: create a ticket with an isolated branch and worktree. Repeated identical titles are idempotent.",
            json!({"title":string,"brief":string}),
            vec!["title", "brief"]
        ),
        tool(
            "assign_ticket",
            "Repository coordinator: assign a dedicated agent using its saved provider and Big/Small profile. Exact proposals must be approved in the role policy; no model substitution is allowed. Existing assignments with the same role and focus are returned; use send_message for follow-up fixes. To add another agent in a role, such as a second reviewer, give each a distinct short focus.",
            json!({"ticket_id":string,"focus":string,"role":{"type":"string","enum":["implementer","tester","reviewer"]},"provider":{"type":"string","enum":["claude","codex"]},"instruction":string,"size":{"type":"string","enum":["big","small"]},"complexity":{"type":"string","enum":["small","standard","complex"]},"profile":{"type":"object","properties":{"provider":{"type":"string","enum":["claude","codex"]},"model":string,"effort":string},"required":["provider","model","effort"],"additionalProperties":false}}),
            vec!["ticket_id", "role", "instruction"]
        ),
        tool(
            "send_message",
            "Immediately queue a message to your parent or direct child. Reuse message_id only when retrying identical content. Do not wait or poll for replies.",
            json!({"recipient":string,"message_id":string,"body":string}),
            vec!["recipient", "message_id", "body"]
        ),
        tool(
            "report",
            "Report progress, a blocker or a result to your coordinator immediately. Give concrete evidence. Never wait for discovery.",
            json!({"message_id":string,"kind":{"type":"string","enum":["progress","blocked","ready_for_testing","passed","failed","completed"]},"body":string}),
            vec!["message_id", "kind", "body"]
        ),
        tool(
            "accept_ticket",
            "Repository coordinator: accept a ticket only after independent verification passed. Does not merge or publish. Removes the ticket's worktree and keeps its branch; refuses while the worktree has uncommitted or untracked files or an agent on it is working.",
            json!({"ticket_id":string}),
            vec!["ticket_id"]
        ),
        tool(
            "close_ticket",
            "Repository coordinator: close a ticket that needs no more work, such as a finished review, once its findings are recorded. Archives its agents and removes its worktree; conversations and branch are kept. Refuses while the worktree has uncommitted or untracked files or an agent on it is working.",
            json!({"ticket_id":string}),
            vec!["ticket_id"]
        ),
        tool(
            "archive_team",
            "Main coordinator: archive one of your repository coordinators and its agents once every ticket is accepted, or closed if abandoned. Refuses, listing every blocker, while a team member is working or a ticket is not accepted or closed. Closes accepted tickets and removes the team's ticket worktrees; conversations and branches are kept and the human can restore the team from the sidebar. Repeating it for an archived team is harmless.",
            json!({"session_id":string}),
            vec!["session_id"]
        ),
        tool(
            "ask_user",
            "Main coordinator: place one decision in front of the human, then finish your turn. The answer resumes you. When there are distinct choices, list them as options: short, self-explanatory labels the human can pick with one click. They can always answer in their own words instead.",
            json!({"request_id":string,"question":string,"options":{"type":"array","items":{"type":"string"},"maxItems":6}}),
            vec!["request_id", "question"]
        ),
        tool(
            "close_question",
            "Main coordinator: remove your own open inbox question once it no longer needs the human, e.g. their message or new evidence already settled it.",
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
