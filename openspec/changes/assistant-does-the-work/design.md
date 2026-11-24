## Context

See proposal.md for why, and `ui/mockups.html` for the look, which Michael approved on November 23, 2026, together with the four decisions below and every recommendation in the draft proposal.

At origin/main 89f130d:
- Assistant threads are sessions owned by a Space (`SessionOwner::Space`), with no parent and no project, each its own provider session in a scratch folder. `provider/restricted.rs` launches them outside YOLO: Claude in `dontAsk` with an allow-list, Codex in the workspace-write sandbox rooted at the scratch folder. Their tools are `THREAD_TOOLS` (`save_output`, `suggest_space`, `project_brief`, `propose_job`, `propose_issue`, `log_work`, `query_brain`); issue threads have `ISSUE_THREAD_TOOLS`.
- Every project role launches in YOLO mode (`provider.rs`): Claude with `--dangerously-skip-permissions` and `--strict-mcp-config`, Codex with `--dangerously-bypass-approvals-and-sandbox`.
- A project's coordinator is created by `insert_project` with `parent_id: None`. `ask_user` is allowed only for `Role::ProjectOrchestrator` and creates an attention (Inbox) item. `report` goes to the session's parent; the coordinator has none, so the human sees its replies. Check-ins of a parentless coordinator become Inbox items (workspace-host, "Check-ins keep parents informed").
- `create_project_from` lets a coordinator create a project in its Space, started on a brief sent as a Wiffletree message, recorded in `created_by`, and refused for briefed coordinators until the human speaks to them.
- Publishing, merge and permission approvals are created only by the host, as Inbox items, and settled only by the human's client.
- `router.rs` and `provider/router.rs` hold the router; `settings/assistant.json` holds the name and the threshold; Who runs what has a Router row.
- `set_live(project, false)` stops a project's deliveries; turns are stopped with `stop_agents`.

## Goals / Non-Goals

**Goals:** one conversation where the human asks, starts and steers work, and sees loops; no project upkeep for Assistant work; the Assistant acts on this Mac without confirmation cards; it reads the human's mail, calendar and other apps freely and acts in them with one click; an instruction inside someone else's text never acts by itself; code changes to managed repositories still land through reviewed tickets and pull requests; the Assistant can look into its work's tickets, reviews, worktrees, builds and logs directly; content written by others still cannot create a schedule.

**Non-Goals:** a second Assistant or one chat per Space; changing how coordinators, tickets, reviews, publishing or merging work inside a project; changing issue threads' restricted mode, Boards, the brain, Spaces or built-in host jobs; sharing the conversation; letting the Assistant approve anything only the human approves today; connectors hosted or written by Wiffletree; running an app's OAuth or storing its tokens; letting a loop act.

## The human's decisions and where they are specified

| # | Decision (November 23, 2026) | Specified in |
| --- | --- | --- |
| 1 | The Assistant acts freely on this Mac, like a YOLO agent; code changes to repositories Wiffletree manages land only through work. Revised the same day: no host enforcement, full read and run access to every repository, worktree and project folder | assistant "The Assistant acts on this Mac", "The Assistant reads its work directly"; Decision 1 below |
| 2 | It starts work without asking; the work card has Stop | assistant "The Assistant starts work", "The Assistant steers its work" |
| 3 | Assistant work is hidden from Projects and lives under the Assistant, with Open details | native-workspace "The sidebar lists projects only", "Projects are listed in a section for each Space"; assistant "The Assistant's rail" |
| 4 | One chat across all Spaces; the Assistant infers the Space and asks inline only when unsure | assistant "One conversation across every Space", "The Assistant infers each request's Space"; assistant-router (removed) |
| 5 | The Assistant is a true personal assistant: it reaches email, calendar and similar apps through Space sources, and Wiffletree still builds no connectors and handles no app sign-in | assistant "The Assistant uses apps through Space sources"; spaces "Sources a Space allows", "Connecting a source is one form"; Decision 9 below |
| 6 | Read vs act: reading is free; sending or changing anything in another app needs one click on an action card; per source the human may mark kinds of action "Do without asking"; loops stay read-only | assistant "Reading is free; acting needs a card", "Kinds of action done without asking", "Text written by others never triggers an action"; scheduled-jobs "Jobs only read from other apps"; Decision 9 below |

Decisions 5 and 6 were added by Michael on November 23, 2026, after the first draft of this change; he confirmed decision 6's default the same day.

## Decisions

### 1. The Assistant has full repository access; code changes land through work by instruction

*Revised by Michael on November 23, 2026.* The first draft held the Assistant out of managed repositories with a macOS Seatbelt profile around its provider. Michael dropped it: the Assistant needs to look into its work, inspect diffs, run builds and tests, and read logs in the projects it started, and a write-deny profile gets in the way of all of that while adding a deprecated mechanism (`sandbox-exec`) to the launch.

So the Assistant launches exactly as a coordinator does: Claude with `--dangerously-skip-permissions`, Codex with `--dangerously-bypass-approvals-and-sandbox`, working in `<data folder>/assistant/workspace/`, with full read and run access to every repository, worktree and project folder.

Code changes to a repository added to Wiffletree still land through work, as tickets that are reviewed and published as pull requests. This is an instruction in the Assistant core, the same way the Coordinating core tells coordinators not to implement, with its reason: edits made from two places collide with the implementer's, and reviewed pull requests are the product's guarantee. The host does not enforce it.

*Alternatives considered:* the Seatbelt profile (dropped by Michael, above); Claude deny rules on managed paths (they cannot hold `Bash`, and would only make the rule look enforced).

### 2. The Assistant is one session owned by the install

A new `SessionOwner::Install` (one session, created at the first message), with no parent, no project and no Space, working in `<data folder>/assistant/workspace/`. It is a session like any other, so durable messages, queued-behind-turn delivery, compaction and process tracking all apply unchanged. It runs one turn at a time; the human's messages are delivered first in a turn's input. It is offered the Assistant's tools and none of the coordination tools.

### 3. Work is a project whose coordinator's parent is the Assistant

`start_work` is a sibling of `create_project_from`: same atomic creation, same brief-as-Wiffletree-message, but the project records `assistant_work` (small flag, state) and its coordinator gets `parent_id = <Assistant>`. `work_status` gives the Assistant a coordinator's `workspace_context`-style read of one of its projects (plan, tickets with cycles and findings, agents, reports), so it can look before asking. Most routing then follows from parenthood: `report` and check-ins already go to a parent. Two changes are needed: `ask_user` from a coordinator with the Assistant as parent is delivered as a message to the Assistant instead of creating an attention item; and the "Human" check-in rule applies only to a coordinator without a parent. Host-created approvals do not change: they remain Inbox items, and the Assistant is told of each. The desktop shows the same item as a card in the chat; settling it anywhere settles it once, because it is one attention record.

The coordinator of Assistant work is refused `create_project`, so work does not chain and the Assistant stays the one place work starts. Its `send_message` reaches its workers and the Assistant.

`pause_work` and `resume_work` are `set_live` plus stopping running turns; `stop_work` is the same plus marking the work Stopped. Work is done when its coordinator reports `completed`.

### 4. The fast path is a flag on the brief, not a new kind of project

`small: true` adds to the brief that the coordinator should create one ticket, assign it as small work (the Implementer row's small-work line, as `assign_ticket` already allows) and verify at the review level its Space allows that runs the fewest verifiers. The host does not enforce the shape; a coordinator that finds the change is not small says so to the Assistant.

### 5. Routing is the Assistant's tool call, rendered by the host

There is no router. When the Assistant calls `message_work` in a turn a human message started, the host tags that message with the work's title. Change moves it: the host sends the text to the chosen work, tells the first work it was moved, and tells the Assistant. This keeps the routing visible without a model or a threshold.

### 6. Space inference is the Assistant's, with one host guard

Each Space-taking tool needs an explicit Space id. The Assistant asks with `ask_human` and one-click options when unsure. When the human picks a Space in the composer, the host refuses calls naming another Space in that turn, which is the one rule a host can check.

### 7. Loops are saved directly, but only on the human's word

The recipe card's job was to keep content from creating a schedule. Without it on every loop, the host keeps that property with the same held-turn rule as app actions (Decision 9): a loop tool call applies at once only in a turn the human's message started, whose input holds nothing but the human's messages, and which has not yet read text written by others through a tool the host sees (a source read, an issue read, `query_brain`, `work_status`, a web search or fetch, or a report). In any other turn the call becomes a loop card showing the exact change and what was read, applied only by the human. This is a host rule over what the host can observe, not a structural guarantee: text the Assistant reads through its shell, such as `cat` on a downloaded email, does not hold the turn (open question 9). Undo restores the loop's previous recipe (or deletes a new loop) and stays offered until the loop changes again. The read-only command lists and the pre-run source check are unchanged.

### 8. Issue writes are direct and marked

`file_issue`, `edit_issue` and `comment_issue` write as the human with `assistant_for` set, through the same tracker code that applies proposals today. They are allowed in any turn, because each is visible in the chat and the tracker and is reversible by editing.

### 9. Apps: the host stands between the Assistant and every act

Sources keep today's shape (`Source::Mcp { server, tools }`, `Source::Cli { command, prefixes, check }`). Their existing lists become the **read** lists, so every saved source keeps working, and each gains optional **act** lists and a "Do without asking" mark per act tool or prefix. Loops still see only read lists.

Enforcement, per kind of source:
- **MCP:** the provider is offered only each source's read tools. Claude offers every tool a server lists, so a command-line server starts through the host helper's `mcp-filter`, which lists and passes on only the read tools; a server reached by URL (HTTP or SSE) is given to Claude as configured, because Claude holds its sign-in, with its act tools and every other tool Claude has reported for it denied by name, and Claude's resource tools are denied. When Claude's start reports a URL server's tool not seen before, the host records it, stops the turn before the model acts and starts the same input again with it denied, unless the human stopped the Assistant meanwhile. Codex gets `enabled_tools` and no user configuration. For both, a call to any other source tool stops the turn as it is made. Act tools are never offered to the provider. `act` names the server, tool and arguments; after Run, or at once for a Do-without-asking kind in an untainted turn, the host starts a command-line server as the provider would (the loop pre-run check already does this) and calls the tool itself.
- **MCP reached by URL:** the host holds no app's sign-in; Claude does. So after Run (or at once, as above) the host starts a short `claude -p` run, never at the Assistant's model's request, in an empty folder with no session: `--tools ""` turns every built-in off, `--strict-mcp-config` gives it only that server as configured for Claude, `--allowedTools` allows only `mcp__<server>__<tool>`, the server's other act tools and other reported tools and the resource tools are denied by name, and `--permission-mode dontAsk` overrides any bypass mode in the human's settings. Its prompt tells it to call that tool once with the card's JSON arguments. Two checks hold it to that call:
  - **Before the call runs:** a `PreToolUse` hook with matcher `*`, the host binary's `act-check`, reads the expected tool and arguments from the run's folder and exits 2 for any other tool, any input not equal to the card's arguments, and any second call. It allows the call only by making the card's marker, in the data folder under `assistant/act-calls/<card id>`, so the act goes through at most once per card, across every attempt and retry; a run stopped after its call was allowed is never started again, and the card says the call went through but its result never arrived, asking the human to check the app. Tool names use Claude's form of the server's name (each character but a letter, digit, `_` or `-` as `_`; for a `claude.ai` connector, runs of `_` collapsed). This is the mechanism chosen over `--permission-prompt-tool`: a permission prompt is consulted only for a call no rule decides, so an allow rule in the human's own settings would skip it, while a hook runs before every call whatever the rules say, and exit 2 refuses even an allowed tool. Hooks run as the job runs' `job-bash-check` already does. The run loads none of the human's own settings (`--setting-sources local` in an empty folder): a `PreToolUse` hook of theirs runs beside `act-check` and could replace the call's input after it was checked, which the stream would not show. Settings an organization manages for the Mac still apply.
  - **While it runs:** the host reads the run's stream. Its start must be in `dontAsk` mode, show the server connected (otherwise it is signed out, and the card says so and how to sign in) and offer exactly the act tool; a start that offers another tool of the server not yet denied is stopped before the model acts and started once more with it denied, as the Assistant's own launch does; the first other call, second call or call with other arguments stops the run and fails the card with the reason. The tool's result is the card's result, and the run stops as soon as it arrives; nothing the run writes as text is kept or acted on.
  Such an act runs only through Claude: on Codex the act is refused before a card, and Run fails if Claude is not installed. There is no fallback to the Assistant's model calling an act. The run uses the Claude model of Who runs what's Assistant row; a run that alters the arguments fails the card rather than sending something else. A restart for tools not seen before shares the first attempt's time limit, and what the start offered is recorded as the Assistant's own launch records it, so the conversation and later runs deny it from their first start.
- **claude.ai connectors:** a source may name a connector the human connected on claude.ai, such as `claude.ai Gmail` or `claude.ai Google Calendar`, as `claude mcp list` shows it. No configuration file defines one, and Claude Code 2.1.322 refuses one declared in `--mcp-config` (type `claudeai-proxy` is reserved for connectors). Claude fetches the human's connectors itself, and only when the run is not `--strict-mcp-config`; `ENABLE_CLAUDEAI_MCP_SERVERS` only turns them off. They load whatever the setting sources: with `--setting-sources local` or `""` a start reports only the connectors, while `user` adds the human's own and plugins' servers. It connects them after its start unless `MCP_CONNECTION_NONBLOCKING=false`, in which case its start reports them with their tools; without it a start often reports none, whatever the setting sources. An `allowedMcpServers` list in `--settings` matches connectors only by URL and leaves them pending at the start, so it cannot limit them. Wiffletree therefore limits them by name:
  - **Reads:** when a Space lists a connector, the Assistant's Claude starts without `--strict-mcp-config` and with `MCP_CONNECTION_NONBLOCKING=false`, and so also loads the human's other connectors, user servers and plugins' servers. Each server no Space lists is denied whole (`mcp__<server>`, a rule that removes all its tools from the start); the connector's tools but its reads are denied by name. Both are learned from Claude's start: a start that offers a server or tool not yet denied stops before the model acts, the host records it, and the same input starts again with it denied, as for a URL server's tools. Without a listed connector the conversation stays strict.
  - **Acts:** the one-tool run above, still with `--setting-sources local`, so none of the human's hooks, plugins or permission rules load beside `act-check`, but without `--mcp-config` or `--strict-mcp-config` and with `MCP_CONNECTION_NONBLOCKING=false`. Every other connector the conversation recorded is denied whole; a start that offers one not yet denied is stopped and started once more with it denied, and recorded. The start must still offer exactly the act tool.
  - **Loops:** a recipe may list a connector's read tools. Its run is moved to Claude as a CLI source's is, and launches as a loop always does, with `--setting-sources ""`, but not strict and with `MCP_CONNECTION_NONBLOCKING=false`. The connector's other tools and every other connector are denied, learned from the start with the same stop-and-restart (the run goes on; its prompt is queued again). Without a connector a loop stays strict. Issue threads stay strict.
  - **Codex** cannot reach a connector: its Spaces list says the source is unavailable there, it is not offered, `act` on it is refused, and a loop that lists one runs on Claude or, with no Claude choice, does not start, saying why.
- **CLI:** the documented path is `read_source`, which runs a listed read prefix through the host with the loop runs' shell-text rule (no redirection, chaining or substitution), and `act`, which runs an act prefix the same way after Run. Without a sandbox the host cannot stop the Assistant running `gws gmail send` in its own shell, so for CLI sources the card is instruction-level: the Assistant core says never to run a source's CLI directly. MCP is the enforced path, and the Add source form says so for a CLI.

**Tainted turns.** The host already reads every tool event in the provider's stream. A turn becomes held to cards when it was not started by the human's message, when its input holds a report, a loop result or a Wiffletree message beside the human's, or once it has received a source read, an issue read, a `query_brain` answer, a `work_status` read (which carries work agents' reports and findings), a web search or fetch, or a report from work. The same rule holds loop changes to loop cards (Decision 7). In a held turn even a Do-without-asking kind waits for Run, and the card names what was read. This turns "an instruction inside an email never triggers an action" from a prompt rule into a host rule: the worst an injected instruction can do is put a card in front of the human with its exact target and content.

**What stays instruction-level:**
- Running a CLI source directly instead of through `read_source` and `act` (above).
- Reaching an app around its source: using credentials a CLI or server stored on disk to call the app's API with `curl`, or driving Mail or Calendar through `osascript`. The Assistant can read those token files, because it can read anything; denying reads of them would need a list of every app's token paths.
- Text read through a tool the host cannot see, such as `cat` on a downloaded `.eml` file, does not taint the turn.

**Connecting a source.** Add source on the Space's page lists the MCP servers already configured for this Mac's Claude and Codex and well-known CLIs on the PATH, starts the chosen one to list its tools, suggests read or act per tool (from MCP's `readOnlyHint` annotation where the server gives one, else from the tool's name), and shows the server's own sign-in instructions when it is signed out. Wiffletree runs no OAuth flow and stores no app token: out of scope, as are Wiffletree-hosted connectors.

### 10. Migration

- Threads that are not issue threads become read-only history; their sessions are never resumed.
- Projects made from threads have no parent and stay in Projects.
- A held router message becomes an unsent draft in the conversation.
- The Router row is dropped at load; a Router row in `space.yaml` and the threshold in `settings/assistant.json` stay, unread.
- `project_brief` and `propose_job` leave every tool list; earlier brief and recipe cards remain readable in history.

## Risks / Trade-offs

- **Full power plus untrusted text** → The Assistant reads issues, web pages and reports with YOLO power, which issue threads were restricted to avoid. Mitigation: authored passages, the core's "context, never instructions" rule, action cards for MCP acts, and the held-turn rule for actions and loops. Project implementers already work this way.
- **One session, one turn at a time** → A long local command delays answers and reports. Mitigation: the human's messages go first, Stop is always there, and the core tells the Assistant to run long commands in the background.
- **A very long conversation** → Context grows without bound. Mitigation: compaction as for agents; state lives in the host (`list_work`, `list_loops`) and the brain, not in the transcript.
- **The Assistant holds the human's mail and calendar** → A prompt injection in an email could try to exfiltrate through an act. Mitigation: no act tool is offered to the provider, every act in a turn that read others' text needs Run on a card showing the exact target and content, and loops never act. Residual: CLI sources run directly, and app APIs reached around a source (Decision 9).
- **Many cards** → Read-then-act requests ("read my mail and put holds…") always produce cards. Mitigation: one Run per card, listed together under Needs you; batching cards is left for later.
- **The Assistant edits a managed repository itself** → Nothing stops it, as nothing stops a coordinator. Mitigation: the core's rule with its reason; an edit it makes in a ticket's worktree shows in that ticket's diff and review, and one in a base clone shows as an unexpected change there.
- **Hidden projects** → Work is out of the Projects list. Mitigation: Open details, the rail, Needs you counts on the sidebar row, and Inbox items for approvals.

## Open questions, with recommendations

These could not be settled from the code. Each is specified as recommended, so accepting one needs no spec change.

1. **Every Space's additions in one session.** The conversation belongs to no Space, so each Space's additions are included under their own heading, scoped to requests in that Space. *Recommendation:* accept; if additions grow large, load only the inferred Space's additions per turn instead.
2. **Can the human talk to a work item's coordinator directly?** *Recommendation:* yes, from Open details; it counts as the human's message, and the coordinator still reports to the Assistant.
3. **Should work be able to move into Projects** ("Show in Projects", making the human the coordinator's parent)? *Recommendation:* not now (YAGNI); Open details covers looking, and the human can talk to the coordinator there.
4. **Should issue writes be limited to turns the human started?** *Recommendation:* no; they are visible and reversible, and blocking them would stop follow-ups from work such as "file the bug the reviewer found".
5. **Which Space's Assistant row does the conversation use?** *Recommendation:* this Mac's alone, because it belongs to no Space; Space Assistant rows still apply to issue threads and loops.
6. **How long Undo stays on a loop card.** *Recommendation:* until the loop changes again, with no time limit.
7. **Should the Assistant get a time or turn check-in in the Inbox?** *Recommendation:* no; its long turns show in the chat and on its row, and the human is already in that conversation.
8. **Acting around the card** (Decision 9): running a CLI source directly, calling an app's API with a stored token, or driving Mail and Calendar with `osascript`. *Recommendation:* instruction-level, and prefer MCP sources where an app offers one; Add source says a CLI's acts rely on the Assistant following its instructions.
9. **Does reading a file or a command's output taint a turn?** *Recommendation:* no; only source reads, issue reads, `query_brain`, `work_status`, web search and fetch, and reports, which the host can see and which are where others' text arrives.
10. **Should issue writes in a tainted turn also need a card?** The tracker is Wiffletree's own, so decision 6 does not cover it. *Recommendation:* no, as question 4 says; revisit if issue text proves a vector.

## Stages and order

See tasks.md. Four stages, each one reviewable ticket, stacked on a feature branch in this order: 1 host plumbing (Assistant session, work, tools, sources and action cards) → 2 one conversation (remove the router, threads and tabs; migration; the Router row) → 3 desktop (chat, cards, rail, sidebar) → 4 skill and docs. Stage 3 can start once stage 1's tool and record shapes have merged, in parallel with stage 2's removals, but merges after stage 2.
