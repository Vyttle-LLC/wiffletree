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
    let small = json!({"type":"boolean","description":"Ask for small work: the row's small-work line, when it has one. Defaults to false."});
    let tool = |name: &str, description: &str, properties: Value, required: Vec<&str>| json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}});
    json!([
        tool(
            "workspace_context",
            "Read your identity, parent, team, repositories, tickets and runtime. The coordinator also gets model_selection: the Who runs what table, with each job's first and then choices (then null means it waits, and \"skip\", offered only on verifier rows, means a verify_ticket review goes ahead without it; a reviewer you start with assign_ticket on such a row waits instead), its small-work line and what each would start on right now and why, or that it skips; the switch threshold; the configured verifiers and review levels with their round and cycle caps; and the human's optional notes. A coordinator that created projects with create_project also gets created_projects: each one's id, name and created time with its tickets as short entries, to follow, not to act on. Accepted and closed tickets appear as one short entry (id, title, state, repository, branch, stacked_on, base, pull request) and archived agents as short entries; pass ticket_id to read one ticket of your project in full, with its brief, cycles, ledger and pull request, instead of the rest of the context.",
            json!({"ticket_id":string}),
            vec![]
        ),
        tool(
            "create_ticket",
            "Coordinator: create a ticket in one of the project's repositories, with its own branch and worktree. Repeating the same repository and title returns the existing ticket. For work that depends on another ticket's unmerged work in the same repository, give that ticket's id as stack_on: the new branch starts from its branch tip, reviews see only the new ticket's changes, and its pull request opens into that branch. The parent must not be closed. For work that belongs on a branch other than the repository's base, give that branch's name on origin as base, such as Parental-controls: the new branch starts from origin/<base>, reviews measure against it, and its pull request opens into it. The branch must be on origin. Give base or stack_on, not both; a stacked ticket takes its parent's base, and repeating a title on another base is refused.",
            json!({"repository_id":string,"title":string,"brief":string,"stack_on":string,"base":string}),
            vec!["repository_id", "title", "brief"]
        ),
        tool(
            "create_project",
            "Coordinator: create a separate Wiffletree project for an outcome that is not a ticket of this one. It starts at once, with its own coordinator from Who runs what, and appears in the sidebar like any other. brief becomes that coordinator's first message, labelled as your words and not the human's, so it plans and asks the human before creating tickets. Give repository_ids to choose the repositories it uses, or leave them out for every workspace repository; an unknown id is refused. Refused before the human saves the model setup. A refusal creates nothing. Each call creates a new project, so repeating a name creates a second one. The new project records you as its creator, and the two coordinators cannot message each other.",
            json!({"name":string,"brief":string,"repository_ids":{"type":"array","items":string}}),
            vec!["name", "brief"]
        ),
        tool(
            "assign_ticket",
            "Coordinator: assign a dedicated agent. You never choose its model: Wiffletree starts it on its job's row in Who runs what, moving to the next choice when a subscription is past the switch threshold, or pinning the earliest reset and holding it when every choice is. A focus that names a configured verifier uses that verifier's row; for any other reviewer, give like: a configured verifier's focus to use its row, or leave it out for the first reviewer's. Pass small: true for small, contained work. A row whose then is skip skips only in verify_ticket reviews; an agent assigned here on such a row waits for the reset. The result names the model and the reason. Existing assignments with the same role and focus are returned unchanged; use send_message for follow-up fixes. To add another agent in a role, such as a second reviewer, give each a distinct short focus.",
            json!({"ticket_id":string,"focus":string,"role":{"type":"string","enum":["implementer","tester","reviewer"]},"instruction":string,"small":small,"like":string}),
            vec!["ticket_id", "role", "instruction"]
        ),
        tool(
            "verify_ticket",
            "Coordinator: once the implementer reports ready_for_testing, start a review at a level from model_selection.review_levels, chosen by each level's use_when line. Every verifier of that level starts on the ticket's current commit at once, and all of them must pass. Give the level's name; each verifier starts on its row in Who runs what, as assign_ticket does, and small: true asks the whole level for small work. A verifier whose every choice is spent waits for the reset and you are told; the rest run. A verifier whose row says skip, and whose first choice is spent or not signed in, starts no session: the cycle records it as skipped with the reason, passes when every verifier that ran passed, and the message when it ends names it. A verifier that still has an open finding never skips; it waits. When every verifier of the level would skip, the review does not start. A re-check level starts only on a ticket whose last review passed, and no level may leave out a verifier that still has an open finding. Any refusal starts no verifier. A verifier session that already exists under that focus is reused only if its row resolves to its pinned model; otherwise it is archived and a fresh verifier starts. Verifiers are read-only; failures go straight back to the implementer and only failed verifiers re-run, up to the round cap. At most two cycles run across this machine at once: a further call queues, says so in its result, and starts by itself when a slot frees. You wake once, when the cycle passes or is blocked. A ticket may start only as many full reviews as the cycle cap allows; re-checks are not counted.",
            json!({"ticket_id":string,"level":string,"small":small}),
            vec!["ticket_id", "level"]
        ),
        tool(
            "send_message",
            "Immediately queue a message to your parent or direct child. Reuse message_id only when retrying identical content. Do not wait or poll for replies.",
            json!({"recipient":string,"message_id":string,"body":string}),
            vec!["recipient", "message_id", "body"]
        ),
        tool(
            "report",
            "Report progress, a blocker or a result to your parent. Progress is batched into its next turn; other kinds wake it immediately. Ask for a decision with kind blocked and the exact question. Give concrete evidence. Never wait for discovery. Verifiers attach every finding to passed or failed: the host fails your check only for a blocking finding with a path:line location, a trigger and evidence. Give id only to report your own open, follow-up or won't-fix entry again.",
            json!({"message_id":string,"kind":{"type":"string","enum":workspace_core::ReportKind::ALL.map(workspace_core::ReportKind::name)},"body":string,
                "findings":{"type":"array","items":{"type":"object","properties":{"id":string,"severity":{"type":"string","enum":["blocking","non_blocking","pre_existing"]},"location":string,"summary":string,"trigger":string,"evidence":string},"required":["severity","location","summary","trigger","evidence"],"additionalProperties":false}}}),
            vec!["message_id", "kind", "body"]
        ),
        tool(
            "set_plan",
            "Record your checklist for this turn: the steps you expect, in order, each with its text and whether it is done. Send the whole list every time; it replaces the last one. Call it when you start work and again as items finish or the plan changes. It only shows your progress to the human: it messages no one and never replaces report. At most 50 items of up to 200 characters each.",
            json!({"items":{"type":"array","maxItems":50,"items":{"type":"object","properties":{"text":{"type":"string","minLength":1,"maxLength":200},"done":{"type":"boolean"}},"required":["text","done"],"additionalProperties":false}}}),
            vec!["items"]
        ),
        tool(
            "stop_agents",
            "Stop your direct children named in session_ids, or all of them when omitted, for example after a check-in shows one stuck or on the wrong path. Running turns stop and their input is not retried; idle children are paused. Each stays paused, so messages and timer fires wait until you call resume_agent. Give the reason; the next turn is told why. Returns stopped, paused_idle or already_paused per agent. Other arguments are refused, so a mistyped session_ids never stops every child.",
            json!({"session_ids":{"type":"array","items":string},"reason":string}),
            vec!["reason"]
        ),
        tool(
            "resume_agent",
            "Resume a direct child that is paused or interrupted: stopped by you, by the human or by a host restart or failure. Resume a child the human stopped only when the human says so. If it has held input from an interrupted turn, inspect its worktree first, then pass held: retry to redeliver it with a note that the turn was interrupted, or skip to drop it. message is delivered as new input. A turn starts when it has input; the result says who stopped the child. Refused while the child is working, not stopped or archived, and then nothing changes.",
            json!({"session_id":string,"held":{"type":"string","enum":["retry","skip"]},"message":string}),
            vec!["session_id"]
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
            "triage_findings",
            "Coordinator: decide each untriaged finding in a ticket's ledger once: fix_now, follow_up or wont_fix, each with a one-line reason. A noted finding, non-blocking or pre-existing, needs no decision; decide one only to pull it into a fix with fix_now or to record follow_up or wont_fix. An open finding is already routed to the implementer; overrule it only with follow_up or wont_fix, for example after the implementer's objection. Decisions never change, and the call applies all of them or none. It messages no one: send fix_now work to the implementer with send_message, then verify again.",
            json!({"ticket_id":string,"decisions":{"type":"array","items":{"type":"object","properties":{"id":string,"decision":{"type":"string","enum":["fix_now","follow_up","wont_fix"]},"reason":string},"required":["id","decision","reason"],"additionalProperties":false}}}),
            vec!["ticket_id", "decisions"]
        ),
        tool(
            "accept_ticket",
            "Coordinator: accept a ticket whose latest verification cycle passed at the commit still checked out or a patch-equivalent one, such as a squash. Triage every untriaged finding first; noted findings need no decision. After a blocked cycle in which every verifier checked, give waived: a one-line reason for accepting the open findings it names; the cycle stays blocked and the waiver is recorded. Once the ticket has used every full review the cycle cap allows, waived may also cover commits past the verified one: it must name each by its short SHA of at least 7 characters, and they are recorded with it. Does not merge or publish. Archives its agents and removes its worktree, keeping conversations and branch, once any agent still in its turn finishes; refuses while the worktree has uncommitted or untracked files.",
            json!({"ticket_id":string,"waived":string}),
            vec!["ticket_id"]
        ),
        tool(
            "publish_ticket",
            "Coordinator or the ticket's implementer: push the ticket's branch to its repository's origin on GitHub and, when the ticket has no open pull request, open one into the ticket's base if it has one, otherwise the repository's base, with title and body, which are then required. A stacked ticket's pull request opens into its parent's branch instead, so publish the parent first, or into the parent's base once the parent's pull request has merged. When its pull request is open, this pushes the new commits, and title, body or both, when given, then replace the open pull request's title and description; leave both out to only push. The workspace runs the push and opens the pull request itself, and edits it; you never push, open or edit one from your shell. It refuses unless the ticket's review has ended and the worktree is on the ticket's branch at the reviewed commit or one with the same changes. It runs without asking when the human allowed the repository's owner, as publishes_without_asking in workspace_context shows, and the review passed or the coordinator waived a blocked review; otherwise each step waits for the human's approval in their inbox, as does any new pull request after an earlier one was closed or merged. Coordinator only: after a blocked review in which every verifier checked and no finding is untriaged, give waived, a one-line reason naming the open findings you accept; once the ticket has used every full review the cycle cap allows, it may also cover commits no review checked by naming each by its short SHA of at least 7 characters. The waiver is recorded for later publishes of the review until a new cycle starts. Returns at once with the steps being run or the step that waits and why; the result arrives later as a message from Wiffletree, so finish your turn.",
            json!({"ticket_id":string,"title":string,"body":string,"waived":string}),
            vec!["ticket_id"]
        ),
        tool(
            "close_ticket",
            "Coordinator: close a ticket that needs no more work, such as a finished review, once its findings are recorded. Archives its agents and removes its worktree; conversations and branch are kept. Refuses while the worktree has uncommitted or untracked files or an agent has not reported; an agent that reported and is still finishing its turn delays only the removal.",
            json!({"ticket_id":string}),
            vec!["ticket_id"]
        ),
        tool(
            "archive_agent",
            "Coordinator: archive one agent of a ticket you own once you no longer need it, such as a reviewer whose findings the implementer has. Its conversation is kept and it can be restored. Refuses an agent mid-turn, an open ticket's implementer (accept or close the ticket instead) and a verifier the running cycle still needs; verifiers are archived automatically when their cycle no longer needs them.",
            json!({"session_id":string}),
            vec!["session_id"]
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
