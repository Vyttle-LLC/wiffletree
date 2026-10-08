# Local development build

Wiffletree is the product name, selected October 5, 2026. This is a working local trial, not the completed PRD release.

## Build and open

On macOS with Rust, Xcode and its Metal toolchain:

```sh
bash scripts/package_macos.sh
open "target/Wiffletree.app"
```

The bundle includes both the GPUI client and its `workspace-host` communication helper, plus the Interlock macOS app icon. Packaging builds the standard and Retina icon representations from the canonical brand-kit PNGs using the macOS `sips` and `iconutil` tools. It is ad-hoc signed for local development and never updates itself; see [releases and updates](#releases-and-updates) for distributed builds. The default store is `~/Library/Application Support/Wiffletree`, outside your repositories. Existing installations automatically reuse `~/Library/Application Support/Agent Workspace` when no Wiffletree data directory exists. This preserves stored sessions and absolute ticket-worktree paths without moving live data. An explicit `--data-dir` still takes precedence. A new store starts empty. Only one host can own a store.

To try unreleased work without interrupting the installed app, build the beta:

```sh
bash scripts/package_macos.sh --beta
open "target/Wiffletree Beta.app"
```

**Wiffletree Beta** has its own bundle ID (`com.vyttle.wiffletree.beta`), a BETA badge beside the wordmark and its own store at `~/Library/Application Support/Wiffletree Beta`. It runs beside the release app and never opens, migrates or locks the store that live sessions use, so a newer schema in the beta cannot strand the release app. The two stores do not share projects; do not point the beta at the release store with `--data-dir`.

For an isolated trial:

```sh
cargo build --locked --release -p workspace-desktop -p workspace-host
./target/release/workspace-desktop --data-dir /tmp/wiffletree-trial \
  --demo-repository "$PWD"
```

The optional demo argument creates one project and attaches the supplied repository at `HEAD`. It does not start inference. Use a persistent data directory for actual work: ticket worktrees live inside that directory, so deleting it would delete their working files.

## Provider setup

Install the standalone Claude Code and Codex CLIs, then sign in using their normal CLI flows. The app searches `PATH`, `~/.local/bin`, `/opt/homebrew/bin`, and `/usr/local/bin`. `WORKSPACE_CLAUDE_BIN` and `WORKSPACE_CODEX_BIN` can select an explicit binary for development.

```sh
claude auth login
codex login
```

The app uses [Claude print mode](https://code.claude.com/docs/en/headless) with JSON events and [Codex noninteractive execution](https://learn.chatgpt.com/docs/non-interactive-mode) with JSON events. Codex's equivalent of print mode is `exec`, not `-p`. Each turn resumes the exact stored provider session. No provider fallback or global configuration edits occur. The app injects its own scoped [MCP tools](https://learn.chatgpt.com/docs/extend/mcp) and bundled role instructions automatically.

On October 5, renewed Claude CLI authentication passed live inference, workspace-tool connection and exact-session resume. Both directions passed: Claude coordinator → Codex implementer → Claude tester, and Codex coordinator → Claude implementer → Codex tester. Each ticket was accepted and its completed result reached the main coordinator. Claude's real command-permission callback also resumed successfully after approval of the exact fixture test command. Codex-only execution was verified on October 4. These live checks use disposable repositories and do not modify the user's product checkout.

## First project

1. Click the sidebar **+**, press **⌘N**, or use **New project** on the empty start screen. A saved project and main coordinator appear immediately; type the name directly in the sidebar. Enter or clicking away saves it. A blank name or Escape keeps the existing name (initially **New project**, numbered when needed). Double-click a project row or click its pencil to rename it later. No repository is required to create the project.
2. Open **Repositories** in the sidebar's Workspace section and click **Add repositories…**. Enter a repository or a parent folder and click **Add**, or use **Choose…** to pick several. A repository is added as is; any other folder is scanned one level deep, so add each level that holds repositories (for example `~/dev/*` and `~/dev/company/*`). Each Add (or Enter) rescans; review paths and inferred bases, untick what you don't want, then click **Add selected**. Parent folders are remembered as **Root folders** on the Repositories page and checked again on launch, when the window comes back to the front (at most once a minute) and when the page opens; new repositories are added with a toast. Checks run off the host thread, list each root one level deep and run read-only Git (`GIT_OPTIONAL_LOCKS=0`, no prompts, bounded time) only for folders not already known, so a check with nothing new costs one directory listing per root. They skip hidden folders, linked worktrees and repositories without a commit yet (picked up after the first commit). Repositories you untick or remove stay out of later checks until you add them by hand; a deleted folder is marked **Missing** rather than dropped. Removing a root stops the checks and keeps its repositories. Repositories belong to the workspace, and every project uses all of them by default, including ones added later. To narrow a project, open its **Overview → Repositories → Add → Choose repositories…**, untick **Use every workspace repository** and pick the set; adding repositories from inside such a project (its ⋯ menu or Overview) also includes them there. **Attach one with a custom base…** sets a repository's base ref, which starts at `HEAD` and accepts any ref such as a local branch; the base is shared by every project. A repository leaves the workspace from the Repositories page only while no team, archived or not, works in it; a project can drop a repository only after archiving its team there. **Add repository team** on a project without repositories opens Add repositories first. Already-added roots are marked; partial failures retain successful additions and remain retryable. Opening an older store merges per-project attachments of the same folder into one workspace repository; each existing project keeps the repositories it had, and a project that had none uses them all.
3. Open **Models** in the sidebar's Workspace section to approve Big/Small profiles for Coordinator, Implementer, Tester and Reviewer. Each role has its own Claude/Codex default toggle and separate tabs for configuring both providers. Choose Big/Small models from dropdowns populated by the installed CLIs; Claude offers latest aliases and pinned versions. Choose reasoning effort from its dropdown, using the selected model’s reported effort levels when available. Coordinator covers both main and repository coordinators and starts with Opus/high; implementers start with Opus/medium. The Save bar stays pinned at the bottom of the panel and enables once something changes; **Discard** restores the saved profiles. Review both providers before **Save profiles**. New agents use their default provider’s Big profile without a model-picking step. Parents choose only saved profiles. Use the model and effort selectors beside an agent’s chat composer to override just that agent. Pause an active turn first. Existing profiles survive default changes; started conversations cannot switch providers.
4. Send the main coordinator the goal and acceptance criteria. Messaging a project starts it; there is no separate step to go live. While any of its agents is mid-turn the header offers **Stop**, which interrupts active turns and holds the queue. A stopped project with waiting messages shows a notice with **Resume**; sending another message, retrying held input or answering an Inbox item also resumes it. It can plan, ask questions, create repository teams, and delegate ticket execution through the workspace tools. Each repository coordinator handles all that project's tickets in its repository.
5. Follow the main conversation. Open **Teams**, select a repository coordinator, and open **Tickets** for each ticket's state, branch and agents. **Assign agent** on a ticket opens the dialog with that ticket selected; the instruction is optional. Each ticket has fresh implementer/tester sessions, visible under it in the sidebar. You can inspect or message any session.
6. Answer main-coordinator questions above the composer in the main conversation, one at a time, or all together in **Inbox**, whose rail icon shows the open count. A question can offer choices you pick with one click; each also has its own answer field and a **Dismiss** action that clears it without waking the coordinator; approvals show the exact operation with **Deny** and **Approve once**. The coordinator closes its own questions once your messages or other evidence settle them, and stepping in after a turn-budget pause clears that notice. All roles run in YOLO mode: Claude uses `--dangerously-skip-permissions` with the full built-in toolset; Codex uses `--dangerously-bypass-approvals-and-sandbox`. Explicit agent requests for human input or approvals still appear in Inbox.

Suggested first prompt:

> Inspect the attached repository and propose a small, independently testable change for this project. Ask me about any missing requirements. When I approve the plan, create its ticket, delegate implementation and independent testing, and report the tested branch and evidence. Keep integration manual.

## Model defaults

The Models editor has four role cards. Each contains a clearly labeled default-provider toggle, separate underlined Claude/Codex profile tabs, and Big/Small model and effort dropdowns. Model discovery runs off the UI thread without inference. Refreshing preserves unsaved selections, and saved models stay selectable if the catalog no longer lists them. If a CLI is missing or discovery fails, built-in and saved options remain available with a visible notice. Saving approves exactly those pairs and updates future agents atomically; existing agents retain their profiles. Switching tabs preserves unsaved inputs for both providers. Built-in coordinator/implementer policies migrate once while preserving custom policies and existing agents.

Parents consult the saved policy and request a provider plus `size: big` or `size: small`. The host resolves the exact profile; Small cannot substitute Luna for a configured Sol model. Unapproved exact proposals are rejected before assignment, and unavailable models report an error without automatic fallback. Legacy fixed policies remain fixed until the user saves the new profiles. Older JSON clients retain the `complexity` protocol: Small selects Small; Standard/Complex select Big under the new settings.

The conversation composer provides model and effort menus for the selected agent. These explicit user overrides do not need to be in the role allowlist. Agent assignment proposals remain bounded by saved profiles. Model IDs are passed unchanged to the CLI; use a version-specific ID to pin a version. Aliases such as `opus` resolve through the provider CLI. Installation/catalog discovery alone does not prove account entitlement: a live turn verifies access. Dual-provider review on one ticket remains a follow-up because a ticket currently permits one reviewer assignment.

## Communication and recovery

Messages and explicit reports are persisted in SQLite before acknowledgment. An event schedules an idle recipient immediately; coordinators do not poll. A busy recipient receives new input at its next turn boundary. Each turn takes every queued message for its recipient in order, up to 20; the rest wait for the next turn. Progress reports are delivered too, but batched: when only progress is queued, an idle parent waits 20 seconds after the first report so a burst arrives in one turn, and any other message or report delivers them at once. Workers talk to their parent; repository coordinators talk to the main coordinator and their workers. The main coordinator resolves decisions or asks the human.

Each ticket owns an isolated branch/worktree. Its implementer and tester run sequentially in that workspace. Separate tickets can run concurrently. Acceptance requires a tester/reviewer `passed` report; finishing a provider turn does not mark a ticket accepted. Coordinator context persists across tickets via the provider session ID; ticket agents are never reused for other tickets.

**Stop** interrupts the project's active provider processes and holds its queue until you message or resume it. **Pause** interrupts the selected session. A session whose turn was cut short shows **Interrupted** (the host's `disconnected` status); the held-input notice is amber after a pause and red after a failure. Closing the app stops its host and provider turns. After a restart, a project that was running keeps running and one that was stopped stays stopped. Only the sessions whose turn the stop cut off wait: they show **Interrupted** with their input held. Everything else in a running project carries on without **Go live**: queued messages are delivered and each timer that came due fires once, late. Interrupted or failed input becomes **held**; the session's conversation shows the error above the composer. Inspect the worktree, then choose **Retry** or **Skip** there. Retrying can repeat external effects, so the app never retries uncertain work automatically. Restarting and enabling a project preserves its coordinator identities and transcripts.

Coordinators can set timers with `schedule`, `unschedule` and `list_schedules`; workers get an error. `at` and `until` take RFC 3339 or an offset such as `+90m`, up to 90 days ahead; `every` takes `30m`, `2h` or `1d` and is between 5 minutes and 90 days. Labels are bounded to 80 bytes, prompts to 4 KiB and each session to 20 active timers. Timers are stored in the `schedules` table and fire into the owner's own queue as `timer:{id}:{slot}` messages, whether or not the project is running, so a stopped project collects them until it runs again. A fire waiting to be delivered absorbs later slots and is marked late, as is a fire enqueued or delivered more than a minute after its slot; `schedule_fires` records each fire's slot, missed count and lateness, which the turn input shows as a `Timer status` line; the stored message never changes. The host wakes for the earliest progress batch or timer slot, sleeping at most a minute at a time so a Mac's sleep cannot hold a timer back for long. A session's first turn after a host start begins with a short note naming the turn the stop interrupted, its active timers and its late fires. The project **Overview** lists active timers. `unschedule` and archiving a session stop its timers and withdraw fires still queued, and an archived session cannot set new ones; a fire held after an interrupted turn stays held for **Retry** or **Skip**. A fire that cannot be enqueued, for example because its owner's queue is full, rolls back and is retried a minute later instead of spinning.

Coordinators may run short, read-only checks themselves from a timer, without creating workers: the main coordinator's final message is the result in the human chat, and a repository coordinator reports to its parent. Coordinator turns have a 10-minute budget, which a coordinator role policy's optional `turn_budget_minutes` (1–30) overrides; policies stored without the field use the default, and saving role defaults keeps it. Worker turns have no budget, and a worker policy that sets one is rejected, because a cut-off turn's input is completed rather than held for a retry. When a turn runs past its budget the host sets the turn's cancel flag, waking for the deadline like any other timed wake. The run's outcome is `over_budget`; the chat shows "Stopped at its N-minute turn budget", the activity log records `turn_over_budget`, a repository coordinator's parent is told, and the turn's input is completed rather than held. The session returns to Ready, and its next turn's prompt names the cut-off turn.

**Archive** hides a session and everything it owns: a project's "…" menu archives the project, and any session's **Overview** archives that team or agent. Archiving interrupts active turns in that tree, stops the project when its main coordinator is archived, and refuses new messages, tickets and agents until restored. Conversations, tickets, branches and provider sessions are kept; the worktrees of tickets the tree owns are removed, including ignored build output. Removal is all or nothing and refuses, naming each ticket, while a worktree is locked or has uncommitted or untracked files. A worktree whose agent is still in its turn is removed when that turn ends, or at a later turn's end or startup once a provider process left behind by a stopped host has exited; if the agent left unsaved work, the worktree is kept and its coordinator, the nearest unarchived owner, or the human inbox is told. Closing or accepting a ticket removes its worktree the same way. **Archived** in the sidebar header lists archived sessions dimmed; open one and choose **Restore**, which also restores the owners above it and re-creates each open ticket's worktree from its branch. Closed and accepted tickets are not re-created. Restoring a project leaves it stopped. Headless: `set_archived` with `session_id` and `archived`.

The local administrative JSON protocol trusts its caller. Provider tools instead use a private Unix socket, per-turn credentials and host-assigned sender identity. No TCP port is opened. Existing stores migrate additively; historical unassigned workers remain visible but need a ticket assignment before live execution.

## Conversation rendering

The native transcript renders Markdown with selectable text, headings, emphasis, lists/checklists, blockquotes, links, tables, inline code and fenced code. Tree-sitter provides syntax highlighting for bundled languages; code blocks include Copy. Unknown fence languages remain readable code. Markdown is parsed in the component's background task and transcript rows remain virtualized. Only each turn's final text becomes a chat reply; narration and tool calls stream as work steps (see Live work). Mathematical typesetting and rendered Mermaid diagrams are not implemented.

The sidebar uses the outlined two-tone wordmark and red Interlock symbol, with separate light/dark artwork. Main-coordinator welcome screens use the same symbol. The four SVGs are embedded in the executable and loaded through GPUI’s cached full-color image renderer; no font installation, runtime file lookup or network request is needed. Interface typography remains the macOS system family, and action/status colors retain the existing Tidal tokens.

The sidebar and inspector have draggable dividers. The sidebar minimum is 240 px so the wordmark and New project button fit without clipping. Projects collapse from their chevron; the selected project row offers Rename and Add repository team. Ticket rows show a state dot, and ticket agents are listed by role. Project/team tools stay in the right inspector rail. Projects are created and named directly in the sidebar; other creation forms are dialogs. Widths, drafts, collapsed projects and the appearance choice (System, Light or Dark from the sidebar menu) currently persist during the client session, not across restart. The activity card's minimized state is the first client preference saved across restarts, in `desktop-preferences.json` in the data directory; a missing or unreadable file means the defaults.

The window uses a themed title bar. The conversation header carries the breadcrumb, status, Pause/Resume , the context fill chip and **Stop** while the project is working. Your messages appear as right-aligned bubbles with their delivery receipt; the session's own replies and other agents' reports are attributed, timestamped and, for reports, tagged with their kind. While the agent works, the activity card described in Live work follows the last message. Results and failures of background requests appear as toasts; failures that belong to an open dialog or the inline project name stay there.

Enter sends a conversation draft; Shift Enter adds a newline. Command Enter also sends. These shortcuts are scoped to the composer and preserve IME composition. The Models editor uses full-width cards, labeled dropdowns and scrollable model menus so model choices and effort values remain readable when resizing the inspector.

## Attachments

Drop files from Finder onto the selected conversation to attach them to the draft. Each appears above the composer as a chip: a thumbnail for PNG, JPEG, GIF and WebP images (recognized by content, then extension), otherwise the name and size. A chip's × removes it. Folders, files over 50 MB and any file whose name is already attached, the same file included, are refused with a notice naming each one and why; the rest of the drop is still attached. Pending attachments belong to that session's draft, and a draft with attachments can be sent without text. Nothing is written until you send.

Sending copies each file to `<data dir>/projects/<project-id>/attachments/<message-id>/<file name>` as read-only (0444), and the message stores those copies' paths, sizes and image formats (schema 5, an additive column on existing messages). If copying or storing the message fails, that message's folder is removed. Copies live as long as their message; archiving and restoring keep them. In the transcript a message's files show under its bubble, and clicking one opens it with its default application.

The delivered prompt lists every attachment's absolute path after the message text. Images also reach the model as images. Codex gets `--image=<path>` before `resume <session> -`; the joined form matters because `--image` takes several values and would otherwise consume the stdin `-`. Codex also splits those values at commas (no other character tested does), so an image whose name has a comma is stored with a hard link beside it, `.codex-<n>.<ext>`, and Codex is given the link. The prompt and the transcript show the original name. A data directory whose own path contains a comma is not supported for Codex images. Claude gets `--input-format stream-json` and one stream-json user message on stdin whose content is the prompt plus a base64 image block per image; turns without images still send plain text. Both were verified on October 7 with Codex CLI 0.160.0 and Claude Code 2.1.293, for new and resumed sessions and for images up to 22 MB. Only the human's messages carry attachments; agents' `send_message` does not.

## Live work

The adapters decode Claude stream-json and Codex `exec --json` into work steps (`crates/workspace-host/src/stream.rs`). Recorded and synthetic streams for both providers are in `crates/workspace-host/tests/fixtures/streams/`; when a CLI changes its output, record a new turn, sanitize paths and account details, and update the fixture tests. Claude currently streams redacted thinking, so thinking rarely appears. Codex command titles drop the `/bin/zsh -lc` wrapper.

The host keeps a running turn's steps in memory and stores each one when it starts and when its state changes. `steps` returns one run's steps changed after a revision cursor (`run_id: null` means the latest run), and `run_summaries` returns duration and counts for finished runs. The desktop listens to a step signal separate from state changes and fetches at most about 30 times a second, so streaming never reloads the snapshot. Bounds: 200-character titles, 40-character notes, 4 KiB details (output keeps its tail, prose its head), and 2,000 steps per run, with omitted amounts counted. Steps of runs finished more than 30 days ago are pruned when the store opens, and steps left running by a stopped host are marked interrupted.

Headless: `{"type":"steps","session_id":"…","run_id":null,"after":0}` and `{"type":"run_summaries","session_id":"…","run_ids":["…"]}`.

## Usage and provider quota

Claude and Codex account capacity stays in the sidebar as one row per provider of compact 5-hour and weekly pills, each filled to the share used and colored at 75% and 90%. Hover for the reset and reading age; click to open Usage and limits. Workspace-wide information lives in the sidebar's **Workspace** section, apart from the per-session tools on the right rail. **Usage and limits** opens a full page: account limits for Claude and Codex side by side whatever is selected, then recorded tokens for all projects, with Today, 7-day and 30-day periods, project/provider filters, daily input/output bars, exact values on selecting a day, and ranked coordinator totals separating own requests from workers. Graphs use America/New_York calendar days. Missing historical collection appears as missing coverage; today is partial.

Quota percentages come from provider telemetry, never from converting local tokens into subscription capacity. Codex uses the supported `account/rateLimits/read` app-server request every five minutes and on Refresh; discovery makes no model call. Available buckets and window durations are displayed independently, with reset time, reading age and 75%/90% warning colors. Claude captures `rate_limit_event` during ordinary live turns. Its CLI may report status without a percentage; those windows show **Unavailable**, and older readings retain their age. Refresh cannot force Claude to emit quota data without a response. No account-page scraping, credential extraction or dummy inference is used. Account consumption includes other applications and machines. Metadata-only Codex `account/read` and Claude `auth status --json` identify the signed-in account when available; histories remain separate by account.

The host stores request counters in an indexed SQLite ledger. Claude partial stream events provide request identities and input/cache/output counts; resumed-session cumulative result totals are retained in run metadata but never added as fresh usage. Codex 0.160 completion usage is cumulative across the conversation. Persistent per-thread checkpoints subtract the previous reading to record each turn once; missing resumed baselines and decreasing counters are withheld rather than invented. An interval spanning an unreported turn is marked partial. Replayed events, stream updates and restarts do not duplicate requests. Interrupted requests retain reported input and output with a partial marker. Cache tokens are included in input and reasoning tokens in output when reported. Native subagents are included only when their request events are visible, attributed to the managing agent; independently identifying every native subagent is not yet supported.

Existing Codex run totals are normalized as cumulative checkpoints once, preserving observed differences. Legacy Claude totals cannot reliably distinguish new usage from resumed-session totals and remain unreported. No historical token counts or missing account percentages are fabricated. Daily quota graphs sum observed increases within the same reset window and calendar day; unknown intervals remain unmeasured. The recent consumption rate needs two readings within fifteen minutes and is hidden when stale. Account quota remains global even with a project filter.

The additive database migration advances the store to version 3. Model/role/custom-date filters, weekday patterns, worker drilldown, account daily usage import, configurable thresholds and exhaustion-based scheduling remain PRD follow-up work. Graph-query performance at the million-request target is not yet measured.

Headless commands: `usage` accepts `days` (1–30), optional `project_id`/`provider`, and an IANA `timezone`; `quotas` returns the latest persisted readings. `workspace-host probe-quota` performs a metadata-only Codex quota read.

## Context window

Each session's **Overview** shows how full its context window is and with what, and the conversation header shows the fill level. The app measures after each turn finishes and on **Refresh**; a measurement makes no model call and does not change the provider's session.

- **Claude** reports its own categories. The app runs `claude -p "/context" --resume <session> --no-session-persistence` from the session's directory with the turn's model, role instructions and strict MCP configuration. Deferred tools are listed by Claude but not loaded, so they are left out; the autocompact buffer gives the compaction point. The workspace's own tool server needs a live turn's credentials and is not counted.
- **Codex** reports only the last request's total and the model's window, which the app reads from the session record under `~/.codex/sessions` (or `CODEX_HOME`). Categories are estimated: content sizes in that record at four characters per token, scaled down to the reported total after compaction, with the unaccounted remainder shown as tools and overhead. The panel labels them as estimates. Codex does not state a compaction point.

A session measured before this change shows the bar after its next turn, because the directory it runs in is recorded from then on. Headless: `workspace-host probe-context <claude|codex> <role> <model> <provider-session> <directory>`.

## Driving the app for testing

`workspace-desktop --automation-socket <path>` opens a development-only Unix socket (mode 600) that turns JSON lines into real input. Mouse events are posted to the application's own AppKit queue, so hit-testing, hover, menus and dialogs behave as they do for a person; keys and typed text go through GPUI's keystroke dispatch. The pointer, the keyboard focus of other applications and macOS Accessibility permission are not involved. Nothing listens unless the flag is passed. In this mode the app does not activate itself at launch and keeps its window out of Stage Manager's strip, so the window can be captured at full size while you work in another application.

`scripts/drive_desktop.py` is the client:

```sh
./target/release/workspace-desktop --data-dir /tmp/wiffletree-trial \
  --demo-repository "$PWD" --automation-socket /tmp/wiffletree-ui.sock &
python3 scripts/drive_desktop.py --socket /tmp/wiffletree-ui.sock click 1247 62
python3 scripts/drive_desktop.py --socket /tmp/wiffletree-ui.sock type "Ship the toolbar"
python3 scripts/drive_desktop.py --socket /tmp/wiffletree-ui.sock key enter
python3 scripts/drive_desktop.py --socket /tmp/wiffletree-ui.sock scroll 1100 500 -300
python3 scripts/drive_desktop.py --socket /tmp/wiffletree-ui.sock drop 740 400 shot.png notes.txt
python3 scripts/drive_desktop.py --socket /tmp/wiffletree-ui.sock shot /tmp/window.png
```

Coordinates are window points from the top-left corner. `shot` saves the window at one pixel per point, so a position read from a screenshot can be passed straight back to `click`. `key` takes GPUI binding syntax such as `cmd-n`. `drop` replays the file-drop events macOS sends for a Finder drag at that point, so the app's own drop handler receives the files. Drops hit-test against the last drawn frame, so after the window has sat covered, send a `move` first. Keep the socket path short: macOS limits Unix socket paths to about 100 bytes. Captures need Screen Recording permission for the calling terminal and fail while the screen is locked; input still works. GPUI stops drawing a window that other windows fully cover, so each step draws a frame itself and captures stay current while you work in front of the app. Use an isolated `--data-dir` and, for runs that must not call a provider, point `WORKSPACE_CLAUDE_BIN` and `WORKSPACE_CODEX_BIN` at `crates/workspace-host/tests/fixtures/provider.py`.

## Releases and updates

Every pull request and every push to `main` runs `.github/workflows/ci.yml`: formatting, warnings-as-errors Clippy, all Rust tests and the deterministic communication tests. Merging does not release anything. Both workflows run on GitHub-hosted macOS runners (`macos-15`).

To release, tag a commit on `main` with its version and push the tag:

```sh
git switch main && git pull
git tag v0.2.0
git push origin v0.2.0
```

The tag runs `.github/workflows/release.yml`. It rejects tags that aren't `v<major>.<minor>.<patch>` or that point outside `main`. Then it reruns the checks and builds the bundle with that version. It signs the bundle with the Developer ID and the hardened runtime, notarizes and staples it, and publishes `Wiffletree-<version>-macos-arm64.zip` as the release for the tag. Creating the release in GitHub's UI with a new tag works too; the workflow attaches the build to that release.

The app shows each update in a card at the bottom of the sidebar. The card has the release notes' opening paragraph, a **Changelog** link to the release page and **Restart to update**. So start the notes with one plain sentence about what changed, then let GitHub generate the PR list below it. In the UI, write the sentence and click **Generate release notes**. From the command line:

```sh
gh release create v0.2.0 --target main --generate-notes \
  --notes "Scheduled tasks, config sync and a sidebar status section."
```

Everything above the first heading or list item is the summary. Without one, the card just says the version is ready. Each tag must be higher than the last, because installed apps only move to a newer version.

`scripts/install.sh [tag]` is the public installer the README's `curl … | bash` line runs. It replaces `/Applications/Wiffletree.app` (or `$WIFFLETREE_INSTALL_DIR/Wiffletree.app`) with a release, the latest by default, after checking its signature and Gatekeeper assessment. It quits a running copy first, which interrupts active turns. Use it to replace a local development build with a release, which then updates itself. A tag whose release has no build yet fails with a retry hint, so expect that for the few minutes the release workflow is still publishing.

Signing and notarization use these repository secrets. The release fails without them rather than publishing an unsigned build.

| Secret | Contents |
| --- | --- |
| `MACOS_CERTIFICATE` | Base64 of the exported *Developer ID Application* certificate and key (`.p12`) |
| `MACOS_CERTIFICATE_PASSWORD` | The `.p12` export password |
| `APPLE_API_KEY` | Base64 of an App Store Connect API key (`.p8`) with Developer access |
| `APPLE_API_KEY_ID`, `APPLE_API_ISSUER` | That key's ID and issuer ID |

Only builds with `WIFFLETREE_VERSION` embedded, meaning CI releases, update themselves; the beta never does. `scripts/package_macos.sh` sets that variable from the environment. A release build checks `releases/latest` at launch, hourly, and on demand, without authentication. To check on demand, click the version at the bottom right of the sidebar (local builds show `v<version> dev`) or choose **Check for Updates…** from the app menu. **About Wiffletree** in that menu opens the standard panel with the version and copyright. Because the checks are unauthenticated, updates need the repository to be public. When a newer version appears, the app does four things before staging it in `~/Library/Caches/com.vyttle.wiffletree/update`:

- Downloads the archive for its architecture.
- Requires the archive to match the SHA-256 digest GitHub publishes for it.
- Verifies the code signature.
- Requires the same signing team as the installed app. An ad-hoc install accepts any valid signature, which allows the move to Developer ID.

**Restart to update** then replaces the bundle in place after the app exits and reopens it. An ordinary quit installs the update without reopening. Restarting interrupts active turns like any quit; the sessions it cut off come back Interrupted, and the rest of each running project carries on. If the app's folder isn't writable, for example when it's run from a translocated download, the updater stays off.

## Current limits

- Acceptance preserves the tested branch. **Integration/merge is manual**; no automatic GitHub PR, push, merge, dependency rebase or combined integration testing is implemented. Tickets start from the repository attachment's configured base. Use independent tickets for the first trial; dependent tickets need an updated base and explicit integration planning.
- No remote host or detached background operation yet. Closing the app interrupts execution; coordinator timers wait in the store and fire late, once, when the app is next open.
- Attachments only come from dropping files on a conversation: no paste, attach button, folders, task or project file library, or cleanup apart from the message's own lifetime.
- Eight active turns globally and four per project by default, with coordinator capacity reserved. A project stops scheduling after 100 turns without a human message, retry or answer; each turn has a 30-minute limit, and coordinator turns a 10-minute role budget by default. These are execution bounds, not a dollar-spend cap.
- Host dispatch measurements exclude model startup/reasoning, a busy recipient's current turn, Git checkout and permission waits. Worktree creation currently runs on the host's database thread; large repository checkout can delay host commands, although it does not run on the UI thread. Moving that Git preparation off the actor remains performance work.
- Full native responsiveness, long-document Markdown behavior, accessibility/IME, memory bounds under prolonged load and an eight-hour soak remain release gates. Compilation does not prove “no lag.”

## Validation

```sh
cargo fmt --all --check
cargo test --locked -p workspace-core -p workspace-host
cargo clippy --locked --workspace --all-targets -- -D warnings
python3 scripts/test_communications.py
bash scripts/package_macos.sh
```

`test_communications.py` uses explicit fake provider CLIs with the real host, MCP subprocess, socket, SQLite and Git worktrees. It covers cross-provider handoff, permission/answer routing, coordinator timers, exact-session resume, cancellation, restart and held-input reconciliation without inference.

These optional checks make bounded real model calls in disposable workspaces:

```sh
python3 scripts/smoke_live.py --provider codex
python3 scripts/smoke_live.py --provider claude
python3 scripts/smoke_handoff.py
python3 scripts/smoke_handoff.py --coordinator claude --implementer codex --tester claude
python3 scripts/smoke_handoff.py --coordinator codex --implementer claude --tester codex
```

The October 5 branding integration passed desktop tests, warnings-as-errors desktop Clippy, formatting, shell syntax and release packaging. The macOS plist, ICNS representations and bundle signature were verified. Live native inspection in an isolated demo store confirmed the red symbol and two-tone wordmark in light and dark appearances and a non-clipping header at the 240 px sidebar minimum.

The Wiffletree release bundle built successfully and all 18 Rust tests, warnings-as-errors Clippy and deterministic communication tests pass. The rename adds coverage for preserving legacy data and selecting the new default directory. The October 4 native Markdown visual check was blocked because the Mac was locked; no new visual verification is claimed.

The October 5 Claude tool/resume probe completed in 4.73 seconds then 7.41 seconds with the same provider session. The Claude-led mixed-provider ticket, including completed delivery to the main coordinator, passed in 100.94 seconds. The reverse arrangement passed in 113.88 seconds, with all recorded queue waits at 1–2 ms. In the Claude-led run, most queue waits were 1–2 ms and one was 4.277 seconds. Queue timing includes waits for an occupied recipient or ticket workspace, not just dispatch overhead. The fixture also exercised blocker escalation; an overly specific test instruction was subsequently made provider-neutral. The smoke harness approves only a short exact allowlist of fixture inspection/test commands; this approval allowlist is confined to the test harness.

The live Codex tool/resume probe completed in 11.11 seconds then 9.43 seconds on October 4. A complete Codex fixture fixed a Python bug, ran independent tests and accepted the ticket in 123.13 seconds. Recorded queue-to-scheduling times in that fixture were 1–3 ms; the original checkout was unchanged. These are single-run observations, not latency guarantees or a cross-provider benchmark.

The October 5 usability update passes all 31 workspace tests on the current main base, deterministic cross-provider communication checks, formatting, warnings-as-errors Clippy and signed release packaging. Native checks confirmed readable agent selection, full-width Models cards at the inspector’s minimum width, fresh Opus/high coordinators, a persisted chat effort override, Enter submission, Shift Enter newlines, and selection/import of eleven discovered repositories alongside an existing attachment. Native Add folder entry and folder-list rendering now pass after removing a collapsing form wrapper; scanning and importing all twelve repositories on an empty test project also passed. No real provider inference was started for this update. Native inspection confirmed readable Big/Small fields, separate Claude/Codex tabs and an atomic save of both providers for all four roles; dual-provider reviewer assignments are not implemented.

The October 6 interface pass passes formatting, warnings-as-errors Clippy, all workspace tests, the deterministic communication checks and release packaging. Native screenshots of a store seeded through the fake provider confirmed the conversation, every inspector panel and the attach, import, ticket and assign-agent dialogs in light and dark appearances. A second pass drove the app through `scripts/drive_desktop.py` against the same store with the fake provider: rail and menu clicks, sending while stopped, Go live through a completed reply, answering an Inbox question, saving role defaults, ⌘N with inline naming, creating a ticket, assigning an agent from its ticket, capturing a memory and scrolling. It found and fixed three defects: ⌘N did nothing without focus, a new project's default name was not selected for replacement, and text typed into inspector fields was scrolled out of view. Real provider turns, the folder pickers, Pause, held-input Retry/Skip and the working row were not exercised.

The October 7 dependency upgrade moves the desktop app to gpui-component 0.7.1 on `gpui-pre` 0.3.8, a snapshot of Zed's GPUI, and drops the vendored Markdown spacing patch. It passes formatting, warnings-as-errors Clippy, all workspace tests and the deterministic communication checks; debug and release builds launch. Both builds were driven through `scripts/drive_desktop.py` with a fake provider replying in rich Markdown, side by side with the previous release, in light and dark appearances: Markdown spacing, lists, tables, code blocks with Copy, selection, the question card and its choices, Inbox, every inspector panel, the team, ticket, agent and repository dialogs, and the Usage, Models and Repositories pages. Remaining differences are upstream styling: round list bullets, a light-blue dark syntax palette, tighter button padding and left-aligned dropdowns.

The October 7 status pass rolls agent activity up the project tree and reserves warning yellow for Needs you. It passes formatting, warnings-as-errors Clippy and all workspace tests, including new roll-up tests. Native screenshots of a store seeded through the fake provider, with one implementer held mid-turn, confirmed the spinning Working glyph on the project, repository coordinator and implementer rows, Team working badges in the conversation header and Teams panel, the solid Needs you badge and muted Done and Blocked glyphs in light and dark appearances. Tooltips and reduced motion were not exercised. Ticket assignment now accepts a focus, so several reviewers share one ticket's worktree instead of each getting a review ticket; a host test covers distinct focuses, idempotent retries and the shared worktree, and the deterministic communication checks pass. Reviewers on one ticket now run at the same time while an implementer or tester still has the worktree to itself; a fake-provider run held three reviewers on one ticket and the native tree showed all three working together.

See [host baseline](performance/host-baseline.md) for earlier measurements. Pinned GPUI dependencies emit an upstream future-compatibility warning for `block`, which Zed's macOS platform still pulls in through `cocoa`; the workspace itself passes warnings-as-errors Clippy.

Model dropdown checks also confirmed both CLI catalogs, readable latest/pinned Claude versions, native selection of pinned Opus, independent Tester Claude/Codex switching and a readable Codex list. The final native save check was interrupted when macOS locked; atomic profile persistence remains covered by host tests. Discovery sends only initialization/catalog requests and does not start inference.

Configure approved profiles now uses underlined provider tabs; default-provider selection remains a separate pair of buttons. Both Big/Small effort values use dropdowns, shared with chat effort selection and populated from the selected model’s reported levels when available. Saved effort values survive catalog changes. The four desktop tests, formatting, Clippy and signed release packaging pass. Native screenshot inspection for this refinement returned Stage Manager thumbnails rather than the full window; the rebuilt app was relaunched with no agents working.
