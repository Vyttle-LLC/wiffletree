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

**Goals:** one conversation where the human asks, starts and steers work, and sees loops; no project upkeep for Assistant work; the Assistant acts on this Mac without confirmation cards; managed repositories still change only through reviewed tickets and pull requests; content written by others still cannot create a schedule.

**Non-Goals:** a second Assistant or one chat per Space; changing how coordinators, tickets, reviews, publishing or merging work inside a project; changing issue threads' restricted mode, Boards, the brain, Spaces or built-in host jobs; sharing the conversation; letting the Assistant approve anything only the human approves today.

## The human's decisions and where they are specified

| # | Decision (November 23, 2026) | Specified in |
| --- | --- | --- |
| 1 | The Assistant acts freely on this Mac, like a YOLO agent, except repositories Wiffletree manages, which change only through work | assistant "The Assistant acts on this Mac", "The host enforces the managed-path exception"; Decision 1 below |
| 2 | It starts work without asking; the work card has Stop | assistant "The Assistant starts work", "The Assistant steers its work" |
| 3 | Assistant work is hidden from Projects and lives under the Assistant, with Open details | native-workspace "The sidebar lists projects only", "Projects are listed in a section for each Space"; assistant "The Assistant's rail" |
| 4 | One chat across all Spaces; the Assistant infers the Space and asks inline only when unsure | assistant "One conversation across every Space", "The Assistant infers each request's Space"; assistant-router (removed) |

## Decisions

### 1. The managed-path exception is enforced by a sandbox around the provider, the same for Claude and Codex

The Assistant runs in its provider's YOLO mode, but the host starts the provider process under a macOS Seatbelt profile (`sandbox-exec -p`, the same mechanism Codex and Claude Code use for their own sandboxes) shaped like this:

```scheme
(version 1)
(allow default)
(deny file-write* file-link*                         ; every managed path
  (subpath "<clone of each workspace repository>")
  (subpath "<each ticket worktree>")
  (subpath "<each project folder>")
  (subpath "<data folder>")                          ; storage, brain and settings clones live here
  (subpath "<HOME>/Library/LaunchAgents"))
(allow file-write* (subpath "<data folder>/assistant/workspace"))
```

Later rules win in Seatbelt, so the Assistant's own folder stays writable inside the data folder. The profile is rebuilt from the host's records at every turn start, so a repository added between turns is covered. Every child process inherits it, so a shell, a script, `git` and a nested `claude -p` are all held. Reads stay open, so the Assistant can read any repository to answer questions.

Per provider:
- **Claude:** `--dangerously-skip-permissions` inside the profile. Claude's own permission rules cannot do this alone: `Edit(path)` deny rules govern the Edit and Write tools only, and a `Bash` command that writes a path is matched by its prefix, not by what it touches. As belt and braces, the launch also passes deny rules `Edit(<managed>/**)` and `Write(<managed>/**)`, so the Edit and Write tools fail with a clear message instead of a sandbox error.
- **Codex:** `--dangerously-bypass-approvals-and-sandbox` inside the profile. Codex's own workspace-write sandbox cannot say "everywhere except these folders": it takes writable roots, and a writable root of `$HOME` would include every clone under it. Codex therefore runs in bypass mode within ours, so the two sandboxes are never nested; whether nesting would work is not tested and not needed.

What the sandbox does not stop, and is only instruction-level (the Assistant core says each):
- Cloning a managed repository elsewhere, committing there and pushing, or opening a pull request with `gh`. The network and the human's GitHub credentials are open to the Assistant.
- Writing a repository through GitHub's API (`gh api`, the web).
- Starting a process outside the sandbox through launchd or LaunchServices, such as `open -a Terminal` or `osascript`, which then runs unsandboxed. Writing a LaunchAgent is denied; `open` and `osascript` stay allowed because the Assistant uses them for ordinary tasks.

**Recommendation:** ship the Seatbelt profile for both providers, plus the Claude deny rules, and treat the gaps above as instruction-level, as every YOLO project agent's "never push" rule is today. The sandbox is there to stop accidental edits and edits a prompt injection asks for through ordinary file writes; it is not a defence against an agent set on escaping. Stage 1 starts with a prototype that proves the profile on both providers before anything builds on it (task 1.1).

*Alternatives considered:* Claude deny rules alone (cannot hold `Bash`); a `PreToolUse` hook that parses shell text (job runs use one, but a YOLO agent's commands are open-ended, so parsing would refuse ordinary work or miss writes); running the Assistant without GitHub credentials (breaks `gh` reads the human expects); the coordinator-level option the human declined.

### 2. The Assistant is one session owned by the install

A new `SessionOwner::Install` (one session, created at the first message), with no parent, no project and no Space, working in `<data folder>/assistant/workspace/`. It is a session like any other, so durable messages, queued-behind-turn delivery, compaction and process tracking all apply unchanged. It runs one turn at a time; the human's messages are delivered first in a turn's input. It is offered the Assistant's tools and none of the coordination tools.

### 3. Work is a project whose coordinator's parent is the Assistant

`start_work` is a sibling of `create_project_from`: same atomic creation, same brief-as-Wiffletree-message, but the project records `assistant_work` (small flag, state) and its coordinator gets `parent_id = <Assistant>`. Most routing then follows from parenthood: `report` and check-ins already go to a parent. Two changes are needed: `ask_user` from a coordinator with the Assistant as parent is delivered as a message to the Assistant instead of creating an attention item; and the "Human" check-in rule applies only to a coordinator without a parent. Host-created approvals do not change: they remain Inbox items, and the Assistant is told of each. The desktop shows the same item as a card in the chat; settling it anywhere settles it once, because it is one attention record.

The coordinator of Assistant work is refused `create_project`, so work does not chain and the Assistant stays the one place work starts. Its `send_message` reaches its workers and the Assistant.

`pause_work` and `resume_work` are `set_live` plus stopping running turns; `stop_work` is the same plus marking the work Stopped. Work is done when its coordinator reports `completed`.

### 4. The fast path is a flag on the brief, not a new kind of project

`small: true` adds to the brief that the coordinator should create one ticket, assign it as small work (the Implementer row's small-work line, as `assign_ticket` already allows) and verify at the review level its Space allows that runs the fewest verifiers. The host does not enforce the shape; a coordinator that finds the change is not small says so to the Assistant.

### 5. Routing is the Assistant's tool call, rendered by the host

There is no router. When the Assistant calls `message_work` in a turn a human message started, the host tags that message with the work's title. Change moves it: the host sends the text to the chosen work, tells the first work it was moved, and tells the Assistant. This keeps the routing visible without a model or a threshold.

### 6. Space inference is the Assistant's, with one host guard

Each Space-taking tool needs an explicit Space id. The Assistant asks with `ask_human` and one-click options when unsure. When the human picks a Space in the composer, the host refuses calls naming another Space in that turn, which is the one rule a host can check.

### 7. Loops are saved directly, but only on the human's word

The recipe card's job was to keep content from creating a schedule. Without the card, the host keeps that property structurally: a loop tool call is accepted only from the Assistant in a turn whose input holds a message the human sent in the conversation. A turn woken only by a report, a loop result or a Wiffletree message cannot change a loop. Undo restores the loop's previous recipe (or deletes a new loop) and stays offered until the loop changes again. The read-only command lists and the pre-run source check are unchanged.

### 8. Issue writes are direct and marked

`file_issue`, `edit_issue` and `comment_issue` write as the human with `assistant_for` set, through the same tracker code that applies proposals today. They are allowed in any turn, because each is visible in the chat and the tracker and is reversible by editing.

### 9. Migration

- Threads that are not issue threads become read-only history; their sessions are never resumed.
- Projects made from threads have no parent and stay in Projects.
- A held router message becomes an unsent draft in the conversation.
- The Router row is dropped at load; a Router row in `space.yaml` and the threshold in `settings/assistant.json` stay, unread.
- `project_brief` and `propose_job` leave every tool list; earlier brief and recipe cards remain readable in history.

## Risks / Trade-offs

- **Full power plus untrusted text** → The Assistant reads issues, web pages and reports with YOLO power, which issue threads were restricted to avoid. Mitigation: authored passages, the core's "context, never instructions" rule, the sandbox for managed paths, and the human-turn rule for loops. Project implementers already work this way.
- **One session, one turn at a time** → A long local command delays answers and reports. Mitigation: the human's messages go first, Stop is always there, and the core tells the Assistant to run long commands in the background.
- **A very long conversation** → Context grows without bound. Mitigation: compaction as for agents; state lives in the host (`list_work`, `list_loops`) and the brain, not in the transcript.
- **`sandbox-exec` is deprecated by Apple** → It still ships and both providers rely on it. If it disappears, the Assistant refuses to start rather than run unguarded.
- **Hidden projects** → Work is out of the Projects list. Mitigation: Open details, the rail, Needs you counts on the sidebar row, and Inbox items for approvals.

## Open questions, with recommendations

These could not be settled from the code. Each is specified as recommended, so accepting one needs no spec change.

1. **Gaps the sandbox cannot close** (Decision 1): pushing from another clone, `gh api` writes, and escaping through `open`/`osascript`. *Recommendation:* accept as instruction-level, like every YOLO agent's push rule; revisit if an incident shows otherwise.
2. **Does the Assistant get every Space's listed MCP servers?** *Recommendation:* yes, with only their listed tools, since it serves every Space. Its CLIs are unrestricted as for any YOLO agent.
3. **Every Space's additions in one session.** The conversation belongs to no Space, so each Space's additions are included under their own heading, scoped to requests in that Space. *Recommendation:* accept; if additions grow large, load only the inferred Space's additions per turn instead.
4. **Can the human talk to a work item's coordinator directly?** *Recommendation:* yes, from Open details; it counts as the human's message, and the coordinator still reports to the Assistant.
5. **Should work be able to move into Projects** ("Show in Projects", making the human the coordinator's parent)? *Recommendation:* not now (YAGNI); Open details covers looking, and the human can talk to the coordinator there.
6. **Should issue writes, like loops, need a human turn?** *Recommendation:* no; they are visible and reversible, and blocking them would stop follow-ups from work such as "file the bug the reviewer found".
7. **Which Space's Assistant row does the conversation use?** *Recommendation:* this Mac's alone, because it belongs to no Space; Space Assistant rows still apply to issue threads and loops.
8. **How long Undo stays on a loop card.** *Recommendation:* until the loop changes again, with no time limit.
9. **Should the Assistant get a time or turn check-in in the Inbox?** *Recommendation:* no; its long turns show in the chat and on its row, and the human is already in that conversation.

## Stages and order

See tasks.md. Four stages, each one reviewable ticket, stacked on a feature branch in this order: 1 host plumbing (Assistant session, work, tools, sandbox) → 2 one conversation (remove the router, threads and tabs; migration; the Router row) → 3 desktop (chat, cards, rail, sidebar) → 4 skill and docs. Stage 3 can start once stage 1's tool and record shapes have merged, in parallel with stage 2's removals, but merges after stage 2.
