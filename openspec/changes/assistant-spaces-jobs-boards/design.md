## Context

See proposal.md for why and `ui/mockups.html` for the look, which Michael approved on October 29, 2026. The same day, the Spaces material moved to its own change, `spaces`, which ships first; this design keeps pointers where it moved. The human made 14 decisions: 12 in a grill session, the 13th, the two kinds of shared job, afterwards, and the 14th at reconciliation. They are specified as given and listed below with the requirement that holds each. 6 more (b–g) were left to the recommendations here and are listed under "Defaulted decisions — confirm at reconciliation". Decisions this design had to make on its own are listed under "Decided while writing".

At origin/main e93d8af:
- A project's brain is `Project.brain`, a local folder path set by `Host::attach_brain` (`workspace-host/src/memory.rs`); exports go there, recorded in `log_exports`.
- The settings repository (`sync.rs`, `sync.sql`) checks a GitHub repository's visibility read-only, reaches it as `gh` does, clones it into `<data folder>/shared/`, and runs model-free passes that compare whole files against what this machine last synced and pause what both sides changed.
- Every provider turn launches in YOLO mode. The PR keeper's comment sorting is the one run started with tools turned off and a fixed answer shape.
- Coordinators have durable timers (`schedule`, `unschedule`, `list_schedules`) that fold missed slots into one late fire.
- GPUI (`gpui-pre` 0.3.8) and gpui-component 0.7.1 have no web view. GPUI's `Window` implements `raw_window_handle::HasWindowHandle`, which yields the window's AppKit `NSView`, and the desktop already calls AppKit through `objc` 0.2 (`automation.rs`).

## Goals / Non-Goals

**Goals:** a one-line question costs one small thread, not a project; several topics run at once; each topic is filed in the right brain without the user sorting it; recurring read-only work runs on the user's Mac with the user's models; outputs have one place to live and to be opened; a team shares Spaces, jobs and dashboards through Git with no server; code changes still go through projects.

**Non-Goals:** a cloud or headless host; waking a sleeping Mac; connectors written by Wiffletree; a thread or job that edits a repository or acts in another app; Jev or any router provider that needs an API key in version 1; sharing threads; changing coordinator timers.

## The human's decisions and where they are specified

| # | Decision | Specified in |
| --- | --- | --- |
| 1 | Local only; organizations share through Git; missed runs coalesce into one catch-up run | spaces "Shared Spaces live in their storage repository" (in `spaces`); scheduled-jobs "Missed runs come back as one catch-up run" |
| 2 | One assistant per user, named, on a Who runs what model, pinned above Projects | assistant "One assistant per user"; model-routing "Who runs what"; native-workspace "The sidebar lists projects only" |
| 3 | One window, many threads in tabs, each its own provider session, sharing memory through the brain | assistant "Threads in tabs", "Each thread is its own provider session" |
| 4 | Topic-based filing into the right brain, asking when unsure | assistant "Topics are filed in their Space's brain"; assistant-router |
| 5 | A typed router before each message, on existing providers, Jev later | assistant-router (all requirements) |
| 6 | Read-only plus a scratch folder, enforced by the provider; Make this a project | assistant "Thread authority", "The provider enforces thread authority", "Make this a project" |
| 7 | A job is a saved thread recipe, created by asking and confirmed by the user; content cannot create a schedule | scheduled-jobs "A job is a saved thread recipe", "Only the user saves a job" |
| 8 | No connectors; sources listed and checked before each run; jobs never act in other apps | scheduled-jobs "Sources are checked before each run", "Jobs only read from other apps" |
| 9 | Output format is up to the user and the job | scheduled-jobs "A run's output is a file"; boards |
| 10 | Boards library, open by type, WKWebView without network, Quick Look, history | boards (all requirements); decision 6 below |
| 11 | Spaces and their fields; projects belong to a Space; the user creates Spaces | Moved to `spaces`: spaces "A Space", "Only the user creates or edits a Space", "Every project belongs to a Space"; workspace-host "Enforced hierarchy"; project-memory "Safe Markdown export" |
| 12 | Shared Spaces through the storage repository; local-only facts; model fallback; sync rules; privacy | Moved to `spaces`: spaces "Shared Spaces live in their storage repository", "Joining a shared Space", "What stays on this machine", "Edits to a shared Space follow the settings repository's rules", "Personal stays personal". The runner's push rule stays here: scheduled-jobs "A runner's output pushes without comparing" |
| 13 | Two kinds of shared job. "One runner, shared result" is the default, for dashboards: the recipe names a runner (its creator by default), only that Mac runs it and pushes, the job's page shows "Runs on ‹name›'s Mac · last run …", Boards shows how stale the output is, and anyone can choose "Take over running", which is a commit. "Everyone runs their own" makes the recipe a shared template, run on each member's Mac with their own sources, with a private result | scheduled-jobs "Shared jobs have one runner or run for everyone" |
| 14 | October 29, 2026, at reconciliation: one assistant, so the issue assistant becomes an issue thread with web search and fetch off; brain compiles run as built-in host jobs on this scheduler | assistant "Issue threads", "A thread may propose an issue in its Space", "Every issue change is a proposal the human applies"; scheduled-jobs "Built-in host jobs" |

## Defaulted decisions — confirm at reconciliation

The human told the coordinator to proceed on these recommendations. Each is specified, so confirming one needs no further spec work, and changing one touches only the requirement named. The brief's item (a), the two kinds of shared job, was confirmed by the human and is decision 13 above.

- **b. The first push of a new shared job's output waits for the runner's approval.** This fails closed for organizations with sensitive data; an organization's rules may forbid personal data in logs and shared files. *Specified in scheduled-jobs "A shared job's first output waits for approval".*
- **c. Concurrent brain writes from several people.** *Superseded by Michael on October 29, 2026:* brain writes reach `main` only through gated pull requests, one file per entry (`brain-registry-sync`, `brain-log-capture`). This change no longer adds "Brain pushes from several people".
- **d. New Who runs what rows: Assistant, Router and Job**, where Job defaults to Assistant's choice. *Specified in model-routing "Who runs what".*
- **e. The router's confidence threshold defaults to 0.8** and is a setting. *Specified in assistant-router "Host rules act on the answers"; settings-repository.*
- **f. Threads live only in the local database and never sync.** Idle threads leave the tab strip but stay searchable. *Specified in assistant "Threads stay on this Mac", "Idle threads leave the tab strip".*
- **g. Coordinator timers stay as they are.** Jobs are a separate, user-owned mechanism. The project-level brain attachment becomes "the project's Space's brain". *Specified in scheduled-jobs "Jobs are not coordinator timers"; project-memory "Safe Markdown export" (modified in `spaces`).*

## Decided while writing

These were not in the brief. Each is the smallest choice that keeps the decisions above consistent; the coordinator should list them for the human.

1. **Every Space has a storage repository, local by default.** Moved to `spaces`. What stays here: every job in a Space stored on GitHub has a runner, including a Space only one user carries between their own Macs, so one Mac runs it.
2. **Private outputs live in a local board store.** Thread outputs, and the results of "Everyone runs their own" jobs, go to `<data folder>/boards/`, a local Git repository with a folder per Space, and are listed on Boards as "Only on this Mac". "Share to Space…" copies one into the Space's storage after a confirmation naming who can read it.
3. **Spaces have a Sources list.** The brief gives recipes a list of sources; an interactive thread needs the same to answer "what's open in Linear?". A Space lists the sources its threads may use, in the recipe's format; a recipe's list replaces the Space's for its runs. Both are checked the same way.
4. **A Space's default models cover threads and jobs only.** Project roles keep Who runs what. A Space can name an assistant model and a job model; a model this Mac lacks falls back to its rows, with a note.
5. **The upgrade creates Spaces from today's brains.** Moved to `spaces`.
6. **A fifth skill, Assisting**, gives threads and job runs a way of working the human can edit, after a fixed Assistant core that holds the authority rules and tools. The router gets no skill and runs like comment sorting, with tools off.
7. **Recipe changes need approval again.** A change to a shared job's prompt or sources makes its next output wait for the runner's approval, as the first did.
8. **Jobs run only while the host runs.** The host does not start at login today. The Jobs list says so and offers "Start Wiffletree at login", off until the user turns it on.
9. **A job whose sources include a CLI runs only on a provider that can hold that CLI to the source's read commands.** Claude can, through its tool permission rules. Codex is limited to the provider-wide sandbox until its command rules are validated (see Risks); until then, such a run falls to the next choice that can, or does not start and says why.
10. **Router failure asks the user.** A router that fails, times out or answers out of shape is treated as "unsure", never as "continue the current thread", so nothing is filed in a brain by default.

## Decisions

### 1. A Space's folder holds its jobs and boards
A Space is the unit of configuration and of sharing (`spaces`). Its folder in its storage repository:

```text
<storage repository>/
  spaces/
    sage/
      space.yaml          name, description, instructions, brain, repositories,
                          default models, sources, format
      jobs/
        morning-brief.yaml
        release-dashboard.yaml
      boards/
        release-dashboard/
          dashboard.html
        morning-brief/
          brief.md
```

The Space itself, `space.yaml`, the storage repository, its tracker and its clone under `<data folder>/repos/` are specified in `spaces`. This change adds `jobs/` and `boards/` to a Space's folder.

### 2. Space sync is in `spaces`; runner outputs are their own rule set
Space files sync on the repository's one engine with the settings rules (`spaces`, `shared-repositories`). This change adds one rule set: an output under `boards/` written by its job's runner is not compared against other machines, because only the runner writes it; a push refused because the remote moved fetches and pushes again. Another machine's output that lands while this Mac had a local edit pauses like any file.

### 3. The assistant is a session that belongs to a Space, not a project
Each thread is a provider session with role `Assistant`, owned by a Space (`SessionOwner::Space`, from `spaces`), with a scratch folder `<data folder>/assistant/threads/<thread-id>/` and no project or ticket. Issue threads are assistant threads with an issue or a tracker's list attached. Job runs are the same with role `JobRun` and their job. The enforced hierarchy admits these roles outside projects; they have no parent coordinator, use no coordination tools, and they have no reports to send. The human is their only parent: a thread turn needs no check-in because the human is watching it, and a job run that has run 30 minutes puts a note on the assistant's sidebar row, every 30 minutes after, with Stop.

Threads get the instructions: communication contract (thread version), Assistant core, Assisting (stock or the human's), the human's rules for Assisting, then the Space's instructions under a heading saying they come from the Space and, for a shared Space, who last changed them. A job run adds its recipe's prompt as the turn's message.

### 4. The provider enforces thread authority
Threads do not run in YOLO mode; this is the one exception to the PRD's launch policy besides comment sorting.

- **Claude** starts with permission mode that denies anything not allowed, never asking. Allowed: `Read`, `Glob`, `Grep`, `WebSearch`, `WebFetch`; `Write`, `Edit` and `MultiEdit` only on paths under the scratch folder; the thread's workspace tools; and, per source, the source's MCP tools and `Bash` limited to the source's command prefixes. No other `Bash`. The scratch folder is the working directory and `--add-dir` covers nothing else.
- **Codex** starts with the `workspace-write` sandbox rooted at the scratch folder, with `sandbox_workspace_write.exclude_slash_tmp` and `sandbox_workspace_write.exclude_tmpdir_env_var` set to true and no extra writable roots (the brief's "read-only sandbox" with one writable folder). Codex's sandbox then lets the shell write only the scratch folder, apart from what its platform profile always allows, such as `/dev/null` and the terminal devices, and it keeps the scratch folder's own `.git`, if one appears, read-only. Approval policy is `never`, shell network off, and its `web_search` tool on. MCP servers are limited to the source's tools with `enabled_tools`. A job with a CLI source does not run on Codex until decision 9 above is lifted.

The host reads the tools and MCP servers each run reports at its start, as comment sorting does, and stops a run that reports more than it was given.

### 5. The router is a typed decision step
The router is a function, not a session: `RouterProvider::decide(RouterInput) -> RouterAnswer`. Version 1 has two implementations on the user's own CLIs: Claude with structured output (`--json-schema`) and Codex with `--output-schema`, each started like comment sorting, with every tool off, no MCP server, no stored session and an empty working directory. Jev, through OpenRouter or Vercel, can be a third implementation later behind the same trait, with its key in the keychain.

```text
RouterInput  { message, current_thread: Option<{id, title, space, last_lines[≤6, ≤1,500 chars]}>,
               open_threads: [{id, title, space}], spaces: [{id, name, description}] }
RouterAnswer { thread: Choice<Continue | Switch(thread_id) | New>,
               space:  Choice<space_id>,
               title:  String  (for a new thread, ≤ 60 chars) }
Choice<T>    { pick: T, confidence: 0.0–1.0, alternatives: [{pick: T, confidence}] ≤ 2 }
```

`current_thread` is empty for the first message and for a message sent from an empty tab; the router is then told that Continue is not a choice, so its thread answer is New or Switch.

Host rules, in order: an answer out of shape, a failed or spent router, or a timeout of 10 seconds is "unsure"; a pick naming an unknown thread or Space, or Continue with no current thread, is "unsure"; with one Space the Space question is answered by the host; below the threshold on either question, or a thread whose Space differs from the Space answer, is "unsure". Unsure asks the user inline with one-click choices built from the picks and alternatives, plus "Somewhere else…". The message waits, unsent, until the user answers. Otherwise the host acts. The router never sees brains, outputs or other threads' text, and has no tools, so it cannot write to a brain.

### 6. Embedding WKWebView in a GPUI window
GPUI paints the whole window into one Metal layer, so a web view has to be a native `NSView` placed over the area a GPUI element reserves.

- **Crate.** Use `wry` (the Tauri project's web view crate) with `WebViewBuilder::build_as_child`, which takes any `HasWindowHandle`, which GPUI's `Window` implements. `wry`'s macOS extension gives the underlying `WKWebView` for the WebKit settings `wry` does not expose; these are set with `objc2-web-kit`. A hand-written `objc` 0.2 binding is the fallback if `wry`'s `raw-window-handle` version cannot match GPUI's.
- **Placement.** The board pane renders a placeholder element whose `canvas` paint callback reports its bounds in window coordinates each frame. The desktop sets the web view's frame from those bounds (flipping to AppKit's bottom-left origin and applying the scale factor) only when they change, and hides it when the placeholder was not painted in the last frame, for example when another page is open.
- **Overlays.** A native view always draws above the Metal layer, so GPUI menus, popovers, sheets and toasts would be hidden behind it. While any GPUI overlay is open over the board pane, the web view is hidden and the pane shows the last snapshot it took (`takeSnapshot`), so the content does not jump. A board is never shown in a window that has a modal open.
- **Focus.** Keyboard focus moves into the web view on click and back to GPUI on Escape or a click outside; ⌘W, ⌘, and other window shortcuts are handled by GPUI first through the web view's key equivalent handler.
- **Loading.** Files are served through a custom scheme, `wiffletree-board://<board-id>/<path>`, that reads only inside that output's folder; nothing is loaded with `file://`. Every response carries `Content-Security-Policy: default-src 'none'; script-src 'unsafe-inline' 'self'; style-src 'unsafe-inline' 'self'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'`.
- **No network, no navigation.** A `WKContentRuleList` blocks every URL whose scheme is not `wiffletree-board`, so subresources, fetches and sockets fail even if the CSP were bypassed. The navigation delegate allows only the first load and reloads of the same document; `window.open`, links and redirects are refused, and a link with `href` to `http(s)` offers "Open in browser" in a GPUI toast rather than navigating. Downloads, file pickers, camera, microphone, geolocation and notifications are refused. The data store is non-persistent, so no cookies or storage survive.
- **Theme.** The page is told the app's appearance through `prefers-color-scheme`, set from the window's appearance.
- **Cost.** One web view per open board, created on open and released on close; a closed board holds no WebKit process.

The risks are the overlay rule, which needs every GPUI overlay to report itself, and keeping the frame in step during window resizes. Both are covered by the stage 4 prototype task before the Boards UI is built on it.

#### Prototype findings (task 4.1, October 29, 2026)
The approach works. The prototype is `crates/workspace-desktop/examples/board_webview/`, a development example that the app does not include. `cargo run -p workspace-desktop --example board_webview -- --probe` posts real AppKit clicks and keys to the application's queue, checks each requirement and prints the measurements. All 20 checks passed in five runs on a debug build.

- **Crates.** `wry` `=0.57.1`, with default features off because they are Linux-only. It uses `raw-window-handle` 0.6 and `objc2` 0.6, the same versions GPUI 0.3.8 already uses, so `build_as_child(window)` takes GPUI's `Window` directly and no `objc` fallback is needed. `objc2` `=0.6.5`, `objc2-foundation`, `objc2-app-kit` and `objc2-web-kit` `=0.3.2` and `block2` `=0.6.2` are the versions already in the lock. They make the calls `wry` does not wrap: the snapshot, the first responder and the event monitor. In the prototype these are dev-dependencies only.
- **What worked.**
  - *Placement.* A zero-size `canvas` painted last in the window places the web view, or hides it when the placeholder did not paint in that frame. The web view followed a window resize and a pane move, hid on another page and came back.
  - *Overlays.* The snapshot (`takeSnapshotWithConfiguration`, read as TIFF into a GPUI `Image`) took 52–248 ms on a loaded machine. The menu waits for it and then draws over the still image, and the live board returns when the menu closes.
  - *Focus.* One `NSEvent` local monitor moves focus in on a click inside the web view, and gives it back on a click elsewhere in the window or on Escape, which it consumes. Typed keys reached the page. ⌘W and ⌘, reached GPUI while the page had focus.
  - *Loading.* The minimal scheme handler served the fixture with the policy above. Its inline script and a served script ran. A fetch over the network, an image over the network and a `file://` image all failed. The handler resolves symbolic links, so `..`, a link out of the folder and another board's id get 404.
- **What did not work as designed.**
  - *Shortcuts.* GPUI handles window shortcuts first because the web view is a subview of GPUI's own view, and GPUI's view answers `performKeyEquivalent:` before its subviews. The web view's key equivalent handler plays no part. `wry` makes a child web view decline every key equivalent (tauri-apps/tauri#9426), so ⌘A and ⌘C never reached the page until the prototype added an Edit menu. That menu's Copy and Select All are `OsAction` items, which AppKit sends to the first responder.
  - *Blur.* Calling `window.blur()` while the page has focus broke shortcuts. GPUI then dispatches from the root dispatch node, which holds none of the view's action handlers, so ⌘W did nothing.
  - *Process release.* A closed board does not release every WebKit process at once. Its WebContent and Networking processes ended 340–440 ms after ⌘W. WebKit's GPU process, shared by every web view in the app, fell to about 18 MiB and was still running 3 s later in four of five runs.
  - *Rounding.* `wry`'s `set_bounds` rounds to whole points, so a placeholder at y = 62.5 gets a frame at 63.
- **Memory**, measured as physical footprint on a debug build with one fixture board:

  | | Before opening | Board open | 3 s after closing |
  |---|---|---|---|
  | App process | 94–104 MiB | 131–133 MiB | 80–81 MiB |
  | WebKit GPU | none | 69–72 MiB | about 18 MiB (gone in one run) |
  | WebKit WebContent | none | 33–35 MiB | ended |
  | WebKit Networking | none | 4.4–5.5 MiB | ended |

  One open board costs about 140 MiB in total: about 30 MiB in the app and 110 MiB in WebKit processes. One run that started at 56 MiB is left out of the app figures as an outlier. The first board after login took about 6 s to load while WebKit started its processes, and later boards took 0.4–0.8 s.
- **Not verified.** Screen captures failed because the screen was locked, so the snapshot's pixels and any one-frame lag during a live drag-resize were not checked by eye. The probe checks frames only after a resize settles. Dark mode is also untested.
- **Rules for 4.2 and 4.4.**
  - The board pane's placeholder must paint in every frame that shows it. It must not sit inside a cached view, because a cached view reuses its last paint without running the placeholder's paint callback, and the web view would be hidden.
  - Snap the placeholder to whole points, or set the frame through `objc2` instead of `set_bounds`.
  - While the page has focus, GPUI's focus moves to a focus handle on the board pane, below the window's root and with no text bindings. GPUI's focus must never be cleared.
  - Window shortcuts are window-level GPUI bindings. The app needs an Edit menu with `OsAction` Copy and Select All, and Cut, Paste, Undo and Redo once boards take input, or editing shortcuts never reach the page.
  - Escape always belongs to Wiffletree, so a board cannot use Escape itself.
  - Every overlay opens only after the snapshot arrives, which takes a few frames. The overlay registry should start the snapshot as soon as the overlay is asked for.
  - The prototype uses `with_incognito(true)` for the non-persistent store, a navigation handler that allows only `wiffletree-board://`, and `NewWindowResponse::Deny`. Task 4.2 replaces these with the content rule list and full delegates. It must keep the scheme handler's resolve-then-check confinement, and it must not add an IPC bridge.
  - The private `_webProcessIdentifier` is for the probe only and never for the app.
  - "A closed board holds no WebKit process" applies to the board's WebContent process. The shared GPU process may linger for some seconds after the last board closes.

### 7. Jobs and their runs
A recipe is a YAML file in the Space's storage (`jobs/<slug>.yaml`): name, prompt, schedule (cron fields and an IANA timezone, with the plain-words form the assistant showed), Space, model (optional), sources, output (title and file type, optional), sharing (`one-runner` or `everyone`), runner (GitHub login and machine name) and enabled. The scheduler keeps each job's next run, last run and run history in the database. At a due time it checks the runner, the sources and the provider, starts a fresh thread of role `JobRun`, sends the prompt, and when the turn ends records the outcome and the outputs the run saved with `save_output`. Missed slots fold into one catch-up run at the first moment the host is running after them, as coordinator timers do, but the two share no code path the human can see: timers belong to coordinators, jobs to the user.

### 8. Issue threads replace the issue assistant
Decided by Michael, 2026-10-29: there is one assistant. An issue thread is an assistant thread in the issue's Space (`shared-issues`, "An issue's Space") with an issue, or a tracker's list, attached. It has the thread tools plus `list_issues`, `read_issue`, `search_repositories`, `read_repository_file` and the `propose_*` tools, and it runs with web search and fetch off: Claude without `WebSearch` and `WebFetch`, Codex with `web_search` off. That keeps local reads, which the thread authority already allows, but no way to send what it read off the Mac. Because the thread no longer runs with every built-in tool off, it can run on Codex as well as Claude, on the Assistant row. Proposals are stored on the thread and applied by the human through `shared-issues`' own write commands. `search_repositories` and `read_repository_file` read each repository's base branch at its fetched head with `git grep` and `git show`, bounded in size, off the command thread, and are limited to the Space's repositories. Any thread, not only an issue thread, may `propose_issue` in its own Space.

### 9. Built-in host jobs
Decided by Michael, 2026-10-29, as `brain-compile` recommended: brain compiles run on this scheduler as a built-in, host-owned job kind. The scheduler keeps one table of due work with two kinds, recipes and built-in jobs. A built-in job has an owner (a brain), a due-time rule, its own extra triggers and its own run function; it gets due times, timezones, catch-up, Run now and run records from the scheduler and nothing else. It is not a recipe, not in any Space's Jobs list, has no runner, and no thread or tool can create or change it.

## Overlap with the other open changes, resolved on October 29, 2026

- **`spaces`** carries the Space, its storage and tracker, the shared repository module, the clone root, default repositories for new projects and session owners. This change depends on it and no longer specifies any of them.
- **`shared-issues` (#162).** The tracker is the Space's storage repository (`spaces`). The issue assistant became issue threads here (decision 8); `shared-issues` dropped its `issue-assistant`, `model-routing`, `skills`, `workspace-host` and `agent-lifecycle` deltas. Its phase 3 provides `list_issues`, `read_issue` and the write commands that applying a proposal uses; stage 6 here builds on it.
- **Brain changes (#165, #168).** Decided by Michael, 2026-10-29: PR-gated brain writes supersede defaulted decision (c), so "Brain pushes from several people" and task 5.3 are gone; thread entries use `log_work` and the per-entry contract (`brain-log-capture`). Brain compile is a built-in host job here (decision 9).
- **Shared requirements.** The delta order is in `spaces`' design: "Enforced hierarchy" is spaces → this → brain-compile; "Who runs what" is this → brain-compile; the settings-repository requirements are spaces → this; "Instructions are a fixed core and a skill" is this change alone. Each later delta restates the requirement with every earlier addition.
- **PRD.** Main's out-of-scope line already names shared issues; this change extends it with Spaces, jobs and outputs shared through Git.

## Risks / Trade-offs

- **Codex command limits** → A CLI source on Codex cannot yet be held to read commands; such runs use Claude or do not start. Validate Codex's command rules (`execpolicy` prefix rules with a default of forbidden) in stage 3 and lift the restriction only if they hold.
- **MCP tools are named, not proven read-only** → A source lists the MCP tools it may call; a tool named like a read can still write. The recipe confirmation lists each tool, and the job's page says jobs are trusted to read only through the tools listed.
- **Router mistakes file a topic in the wrong brain** → The threshold, the ask-on-unsure rule and "Change" on each routed message bound it; entries already written stay and are named so the user can move them.
- **Sleep and the host not running** → No wake scheduling. A brief due at 07:00 on a Mac opened at 08:30 runs once at 08:30, marked as catch-up.
- **WKWebView overlays** → see decision 6.
- **Shared instructions are an attack surface** → Whoever can push to a shared Space's storage changes every member's thread instructions. The Space page says so, Space sync pauses edits made outside Wiffletree, and thread authority does not depend on instructions.
- **Personal data in shared outputs** → First-push approval, the "Only on this Mac" default for thread outputs and the confirmation on "Share to Space…".
- **Issue text steering a thread** → Issue threads have no web tools and every write is a proposal the human applies; their pane names the authors of the text they read.

## Migration Plan

Stage by stage, each landing alone after `spaces` (see tasks.md). The upgrade that creates Spaces from today's brains is in `spaces`. Rollback before stage 5 is a database restore; nothing has been pushed.

## Open Questions

- Should threads offer voice input or a global hotkey, as Dots and Duet do? Left out of version 1.
