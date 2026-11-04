## Context

Wiffletree already runs sessions the provider holds to what they are given: assistant threads, job runs and brain compiles (`provider/restricted.rs`). Each is a session with a role, a pinned model and a restricted launch, and the scheduler runs its turns, records its steps and waits at a usage limit. A brain compile shows how to keep such a session out of every project's agent tree: its session has no project and no parent, and a run record ties it to what owns it.

## Goals / Non-Goals

**Goals:** a coordinator starts a blind evaluator on an exact model and is woken when it ends; the human decides per project whether evaluators may use the screen; nothing the evaluator reads, writes or says reaches the coordinator except through the run folder.

**Non-Goals:** messaging a running evaluator; an editable deny-list of apps; evaluators on a paired remote Mac; any change to Who runs what.

## Decisions

### A run is a session outside the agent tree, with its own record

An evaluator run is an `evaluations` row: id, project, the coordinator that started it, provider, model, effort, mode, folder, state, reason and times. When it starts it gets a session of role `evaluator` with no project, no parent and no repository, as a compile does. That keeps it out of `workspace_context`'s team, the Agents panel's ticket tree, check-ins and `send_message`, which would otherwise hand its replies to the coordinator. The session gives it the scheduler's turn, steps, transcript and usage-limit wait for free. Each run gets a new session, so there is no history to resume across runs.

The session is archived when the run ends, so nothing can start another turn in it. A run whose turn a crash, kill or long quit cut short is ended as failed when the host next starts, "Wiffletree stopped during the run.", and its coordinator is told, as job runs are ended. A run still waiting to deliver its prompt, or waiting for its limit's reset, goes on.

### Isolation is the provider's, as for threads

Claude runs as `-p` with stream-json, `--setting-sources ""`, `--strict-mcp-config` with no server but an app run's driver, `dontAsk`, `--tools Read,Glob,Grep,Write,Edit`, allow rules scoped to `//<folder>/**`, writes to `~/.claude` denied and auto memory off. No system prompt is appended. Its start is checked as a thread's is: any other tool, any MCP server but an app run's driver, another permission mode or auto memory stops the run. On this Mac (Claude Code 2.1.317) such a run could not read `/etc/hosts` or write `/tmp`, and received no CLAUDE.md.

Codex runs `exec --json --ignore-user-config --ignore-rules` in a workspace-write sandbox rooted at the folder, with no network, no temporary folders, web search disabled, the threads' features off and also `shell_tool`, `unified_exec`, `view_image`, which reads any path, and `image_generation`. Its code mode then has only `apply_patch`, which the sandbox holds to the folder. Any MCP call stops the run. No developer instructions are given.

`--ignore-user-config` does not keep `~/.codex/AGENTS.md` out: a probe on Codex 0.160.0 received it. So an evaluator's Codex gets its own `CODEX_HOME` in Wiffletree's data folder, holding only a link to the user's `auth.json`, so sign-in and token refreshes still use the user's one file. The probe with that home received no AGENTS.md.

A separate `CODEX_HOME` still lets Codex find the human's skills under `~/.agents/skills`. A request captured by a local endpoint carried them in its skills catalog, and neither `skip_host_skill_discovery` nor turning off `skill_search` removed them. So the evaluator's Codex also runs with an empty `HOME` of its own, beside its Codex home. Only Codex's bundled system skills (imagegen, openai-docs, skill-creator, skill-installer) remain, as Claude keeps its built-in ones.

The link holds only because Codex rewrites `auth.json` in place. A probe on 0.160.0 confirmed it with a throwaway home: a fake, stale `auth.json` reached through a link, and `CODEX_REFRESH_TOKEN_URL_OVERRIDE` pointing at a local server. Codex refreshed, the refreshed token landed in the link's target with the same inode, and the link stayed a link. The home holds no lock for the ChatGPT token refresh, so two homes share nothing that one home would not.

The environment is cleared and only `HOME`, `USER`, `LOGNAME`, `PATH`, `SHELL`, `TMPDIR`, `LANG` and the `LC_*` locale variables are passed on, so no `WORKSPACE_TOKEN`, `WORKSPACE_SOCKET` or other variable of Wiffletree's or the human's sessions reaches the run or anything it starts.

### The model is checked, pinned and never replaced

`start_evaluation` refuses a provider this Mac has not found signed in, a model the provider's CLI did not list, full IDs such as `claude-opus-5-5` and aliases such as `opus` alike, and an effort the model does not offer. Until the CLI's list has been read, a run is refused with that reason and the service reads the list, so the call succeeds when repeated; Wiffletree never checks against a guess. The profile is pinned on the run's session as given; Who runs what is never consulted.

An alias can move to a newer model without notice, so the run records the model the provider names: Claude's in its start record, Codex's in the `turn_context` of the session record in the evaluator's Codex home, since its JSON stream names none. A run started on a full ID whose provider names another model fails at once, with the reason for the human. A run on an alias records which model it got. The coordinator may see the reported model in `list_evaluations` and the end message. A run whose provider is past its limit when it starts is marked waiting and its turn is held until the reset, as any pinned agent's is. A turn that fails at a limit is retried in the same session at the reset, and the run shows waiting meanwhile.

### Authorization is the human's, on an Inbox card

Each project has an evaluator permission: none yet, off, app or desktop. The first `start_evaluation` in a project with none raises one Inbox card, `evaluators:<project>`, offering App only, Full desktop and Not in this project, and holds the run as waiting. Answering sets the permission and admits or stops the held runs; any other answer is refused, and leaving the card is "Ask me later". `Command::SetEvaluatorPermission` lets the human change it later; it is human-only, and no tool sets it. Archiving a project's coordinator, which is how a project closes, sets it to off and stops the project's runs.

A run's mode must be allowed: desktop needs desktop, app needs app or desktop. Until ticket 4, desktop is refused as not available yet.

### One screen, one queue

App-only runs start as soon as they are admitted. A desktop run needs the screen lock, which one run holds at a time across every project; the others stay queued, oldest first. The lock is released when the holding run ends. Ticket 4 adds the Start-now click between taking the lock and starting.

### An app run drives its own test copy

An app run names its target: the executable, its arguments, which must give a test data folder of its own with `--data-dir`, and the variables it needs. Wiffletree resolves the data folder and refuses one that overlaps its own or the default data folder, one another unended run uses, `--automation-socket` among the arguments, and any `WORKSPACE_*` variable but the provider stubs or any `*_TOKEN` variable.

The copy lives for a turn. Before the provider starts, the turn's thread starts the copy with `--automation-socket` on `$TMPDIR/wiffletree-<run>.sock`, in its own process group, from the evaluator's cleared environment plus the target's variables, with a given `PATH` placed before the host's and `WORKSPACE_SKIP_LOGIN_ENV=1`, which keeps the copy's host from adopting the login shell's `PATH`. It waits up to 90 seconds for the socket to answer `window`; a copy that exits or never answers fails the turn, and so the run, with that reason. Whichever way the turn ends, the copy's group and the group of every process it started get SIGTERM, then SIGKILL after five seconds, and the socket is removed. A run waiting at its usage limit has no copy running; the retry starts a new one on the same data folder. The copy's output goes to `evaluators/logs/<run>.log` in Wiffletree's data folder, for the human.

The driver is `wiffletree-host app-driver-mcp <socket> <run folder>`, the run's only MCP server, named `app`. Claude gets it through `--mcp-config` and allow rules for its eleven tools; its start must show exactly that server connected and no other tool. Codex gets it through `-c mcp_servers.app` with `enabled_tools`, and any other MCP call stops the run. Each tool is one or a few socket steps, in window points from the top-left corner. Screenshots are saved as PNG under a relative name inside the run folder and returned as an image with its pixel size and the window's scale.

The app captures its own window with `CGWindowListCreateImage` on its own window number, at nominal (one pixel per point) or best (native) resolution, drawn into an sRGB bitmap. macOS lets an app capture its own windows without Screen Recording permission. Measured on macOS 27.0.1: launched through `responsibility_spawnattrs_setdisclaim`, so it was its own responsible process and `CGPreflightScreenCaptureAccess` returned false, the app returned its window at 1280×820 and 2560×1640, while `screencapture -l` on the same window failed with "could not create image from window".

### The coordinator hears only the outcome

When a run ends, Wiffletree messages its coordinator: `Run <id> <state> · model <model> · folder <folder> · <duration>`, naming the model the provider reported, or the one asked for when none was. `list_evaluations` gives state, mode, the model asked for and the one reported, folder and times, and a waiting run's reset time. A failure's reason is shown to the human only, since a provider's error can quote the model's text.
