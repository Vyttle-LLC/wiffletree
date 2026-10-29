## Context

See proposal.md for why and `ui/mockups.html` for the proposed look, which awaits the human's approval. The human made 13 decisions: 12 in a grill session and the 13th, the two kinds of shared job, afterwards. They are specified as given and listed below with the requirement that holds each. 6 more (b–g) were left to the recommendations here and are listed under "Defaulted decisions — confirm at reconciliation". Decisions this design had to make on its own are listed under "Decided while writing".

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
| 1 | Local only; organizations share through Git; missed runs coalesce into one catch-up run | spaces "Shared Spaces live in their storage repository"; scheduled-jobs "Missed runs come back as one catch-up run" |
| 2 | One assistant per user, named, on a Who runs what model, pinned above Projects | assistant "One assistant per user"; model-routing "Who runs what"; native-workspace "The sidebar lists projects only" |
| 3 | One window, many threads in tabs, each its own provider session, sharing memory through the brain | assistant "Threads in tabs", "Each thread is its own provider session" |
| 4 | Topic-based filing into the right brain, asking when unsure | assistant "Topics are filed in their Space's brain"; assistant-router |
| 5 | A typed router before each message, on existing providers, Jev later | assistant-router (all requirements) |
| 6 | Read-only plus a scratch folder, enforced by the provider; Make this a project | assistant "Thread authority", "The provider enforces thread authority", "Make this a project" |
| 7 | A job is a saved thread recipe, created by asking and confirmed by the user; content cannot create a schedule | scheduled-jobs "A job is a saved thread recipe", "Only the user saves a job" |
| 8 | No connectors; sources listed and checked before each run; jobs never act in other apps | scheduled-jobs "Sources are checked before each run", "Jobs only read from other apps" |
| 9 | Output format is up to the user and the job | scheduled-jobs "A run's output is a file"; boards |
| 10 | Boards library, open by type, WKWebView without network, Quick Look, history | boards (all requirements); decision 6 below |
| 11 | Spaces and their fields; projects belong to a Space; the user creates Spaces | spaces "A Space", "Only the user creates or edits a Space", "Every project belongs to a Space"; workspace-host "Enforced hierarchy"; project-memory "Safe Markdown export" |
| 12 | Shared Spaces through the storage repository; local-only facts; model fallback; sync rules; privacy | spaces "Shared Spaces live in their storage repository", "Joining a shared Space", "What stays on this machine", "Edits to a shared Space follow the settings repository's rules", "Personal stays personal" |
| 13 | Two kinds of shared job. "One runner, shared result" is the default, for dashboards: the recipe names a runner (its creator by default), only that Mac runs it and pushes, the job's page shows "Runs on ‹name›'s Mac · last run …", Boards shows how stale the output is, and anyone can choose "Take over running", which is a commit. "Everyone runs their own" makes the recipe a shared template, run on each member's Mac with their own sources, with a private result | scheduled-jobs "Shared jobs have one runner or run for everyone" |

## Defaulted decisions — confirm at reconciliation

The human told the coordinator to proceed on these recommendations. Each is specified, so confirming one needs no further spec work, and changing one touches only the requirement named. The brief's item (a), the two kinds of shared job, was confirmed by the human and is decision 13 above.

- **b. The first push of a new shared job's output waits for the runner's approval.** This fails closed for organizations with sensitive data; an organization's rules may forbid personal data in logs and shared files. *Specified in scheduled-jobs "A shared job's first output waits for approval".*
- **c. Concurrent brain writes from several people.** Entries keep stable IDs. The host pulls and rebases before pushing and merges same-file appends as a union by entry ID. It never force-pushes, and a true conflict pauses with the existing compare flow. *Specified in project-memory "Brain pushes from several people".*
- **d. New Who runs what rows: Assistant, Router and Job**, where Job defaults to Assistant's choice. *Specified in model-routing "Who runs what".*
- **e. The router's confidence threshold defaults to 0.8** and is a setting. *Specified in assistant-router "Host rules act on the answers"; settings-repository.*
- **f. Threads live only in the local database and never sync.** Idle threads leave the tab strip but stay searchable. *Specified in assistant "Threads stay on this Mac", "Idle threads leave the tab strip".*
- **g. Coordinator timers stay as they are.** Jobs are a separate, user-owned mechanism. The project-level brain attachment becomes "the project's Space's brain". *Specified in scheduled-jobs "Jobs are not coordinator timers"; project-memory "Safe Markdown export" (modified).*

## Decided while writing

These were not in the brief. Each is the smallest choice that keeps the decisions above consistent; the coordinator should list them for the human.

1. **Every Space has a storage repository, local by default.** A Space's storage is a Git repository. A new personal Space gets a local repository in the data folder with no remote, so outputs still have Git history and nothing leaves the Mac. Choosing a GitHub repository is what makes a Space sync, and a Space whose storage other people can read is a shared Space. This is how "a personal Space never syncs anywhere the user did not choose" holds. A host with no Space creates "Personal" this way, so a fresh install can start a project or a thread, and the last Space cannot be removed. Every job in a Space stored on GitHub has a runner, including a Space only one user carries between their own Macs, so one Mac runs it.
2. **Private outputs live in a local board store.** Thread outputs, and the results of "Everyone runs their own" jobs, go to `<data folder>/boards/`, a local Git repository with a folder per Space, and are listed on Boards as "Only on this Mac". "Share to Space…" copies one into the Space's storage after a confirmation naming who can read it.
3. **Spaces have a Sources list.** The brief gives recipes a list of sources; an interactive thread needs the same to answer "what's open in Linear?". A Space lists the sources its threads may use, in the recipe's format; a recipe's list replaces the Space's for its runs. Both are checked the same way.
4. **A Space's default models cover threads and jobs only.** Project roles keep Who runs what. A Space can name an assistant model and a job model; a model this Mac lacks falls back to its rows, with a note.
5. **The upgrade creates Spaces from today's brains.** Each distinct `Project.brain` folder becomes a Space named after the folder, with that brain and a local storage repository; projects without a brain go to a Space named "Personal" with no brain. The human renames, merges or describes them on the Spaces page. Nothing is pushed.
6. **A fifth skill, Assisting**, gives threads and job runs a way of working the human can edit, after a fixed Assistant core that holds the authority rules and tools. The router gets no skill and runs like comment sorting, with tools off.
7. **Recipe changes need approval again.** A change to a shared job's prompt or sources makes its next output wait for the runner's approval, as the first did.
8. **Jobs run only while the host runs.** The host does not start at login today. The Jobs list says so and offers "Start Wiffletree at login", off until the user turns it on.
9. **A job whose sources include a CLI runs only on a provider that can hold that CLI to the source's read commands.** Claude can, through its tool permission rules. Codex is limited to the provider-wide sandbox until its command rules are validated (see Risks); until then, such a run falls to the next choice that can, or does not start and says why.
10. **Router failure asks the user.** A router that fails, times out or answers out of shape is treated as "unsure", never as "continue the current thread", so nothing is filed in a brain by default.

## Decisions

### 1. Spaces are defined in their storage repository
A Space is the unit of configuration and of sharing. Its definition is a file in its storage repository:

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

`space.yaml`'s brain is a repository identity (`owner/name` or "local") and a folder; its code repositories are identities, never paths. One storage repository can hold several Spaces (a folder each), and two Spaces may name the same brain. Sage's existing settings repository, `Sage-Social/wiffletree-storage`, can be the storage repository of the Sage Space: settings sync commits only `wiffletree.json`, `settings/` and `skills/`, Space sync commits only `spaces/`, each from its own clone, so neither changes the other's files and both already pull commits they did not make.

Each machine keeps its own facts in the database, keyed by Space: where each repository and the brain are cloned, which models it may use, and whether the Space is enabled here. The storage clone lives at `<data folder>/storage/<owner>-<name>/`, or `<data folder>/storage/local-<id>/` for a local one.

### 2. Space sync reuses settings sync, file by file
Same as settings: choosing the repository is the permission to push; the check refuses public repositories and says who can read; pushes go only to the recorded default branch, contain the remote head, are never forced, have no tags or submodules, and change only `spaces/`; passes run off the host's command thread, pull at launch and every 5 minutes, push 5 seconds after a change, back off to 30 minutes while GitHub is out of reach; a file changed on two machines pauses with Compare, Keep this Mac's and Use the other machine's; a file changed outside Wiffletree is paused until the human syncs it. One difference: an output under `boards/` written by its job's runner is not compared against other machines, because only the runner writes it; a push refused because the remote moved fetches and pushes again. Another machine's output that lands while this Mac had a local edit pauses like any file.

### 3. The assistant is a session that belongs to a Space, not a project
Each thread is a provider session with role `Assistant`, a Space, a scratch folder `<data folder>/assistant/threads/<thread-id>/` and no project or ticket. Job runs are the same with role `JobRun` and their job. The enforced hierarchy admits these two roles outside projects; they have no parent coordinator, use no coordination tools, and they have no reports to send. The human is their only parent: a thread turn needs no check-in because the human is watching it, and a job run that has run 30 minutes puts a note on the assistant's sidebar row, every 30 minutes after, with Stop.

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

### 7. Jobs and their runs
A recipe is a YAML file in the Space's storage (`jobs/<slug>.yaml`): name, prompt, schedule (cron fields and an IANA timezone, with the plain-words form the assistant showed), Space, model (optional), sources, output (title and file type, optional), sharing (`one-runner` or `everyone`), runner (GitHub login and machine name) and enabled. The scheduler keeps each job's next run, last run and run history in the database. At a due time it checks the runner, the sources and the provider, starts a fresh thread of role `JobRun`, sends the prompt, and when the turn ends records the outcome and the outputs the run saved with `save_output`. Missed slots fold into one catch-up run at the first moment the host is running after them, as coordinator timers do, but the two share no code path the human can see: timers belong to coordinators, jobs to the user.

### 8. Brains shared by several people
A Space's brain may be a repository other people also push to, such as the Sage brain. Logs are append-mostly Markdown files of entries with stable IDs. Before pushing, the brain writer fetches and rebases; a log file changed on both sides merges as the union of entries by ID, ordered as the file orders them (newest first). An entry ID present on both sides with different content, or a change outside entries on both sides, is a true conflict: the push pauses and a workspace note offers the settings repository's Compare and choices. Nothing is force-pushed. Compiled notes keep today's single-writer rule.

## Overlap with shared issues (PR #162)

PR #162 is merged on main (6194b99a) as the change `openspec/changes/shared-issues`, not yet archived: `proposal.md`, `design.md`, `tasks.md`, `ui/mockups.html` and deltas under `specs/` for `shared-issues`, `issue-assistant`, `model-routing`, `skills`, `workspace-host`, `agent-lifecycle`, `host-process` and `pr-watch`. This change does not edit it. Points to reconcile:

- **Organization Git repositories as shared storage.** Both changes put team data in a private GitHub repository synced by reusing the settings repository's rules, each with its own clone and its own paths (`shared-issues/design.md` decisions 2 and 3: a tracker clone at `<data folder>/trackers/<owner>-<name>/` holding `wiffletree-issues.toml` and `issues/`). A tracker repository and a Space's storage repository could be the same repository with `issues/` beside `spaces/`; the reconciliation should decide whether a Space names its tracker, and whether the two syncs share one clone or keep separate clones as both changes propose. Shared issues accept edits made outside Wiffletree; Space sync pauses them, as settings do, because a Space's instructions reach every member's agents.
- **Two assistants.** `shared-issues/specs/model-routing` adds an optional "Issue assistant" row to Who runs what, Claude only, and `shared-issues/specs/issue-assistant` defines the role; this change's `specs/model-routing` adds "Assistant", "Router" and "Job" rows to the same requirement. The issue assistant could become a thread of this assistant in the tracker's Space, with issue tools; or the names should at least be distinct on the Models page.
- **Skills and hierarchy.** Both changes modify `skills` "Instructions are a fixed core and a skill" (`shared-issues/specs/skills` gives the issue assistant its own core and no skill; this change adds Assisting) and `workspace-host` "Enforced hierarchy" (`shared-issues/specs/workspace-host` admits the issue assistant's tracker sessions; this change admits threads and job runs and requires a Space for each project). Each delta is written against the main spec, so whichever change is archived second must restate its requirement with both additions.
- **PRD.** Main's out-of-scope line already names shared issues; this change extends it with Spaces, jobs and outputs shared through Git.

## Risks / Trade-offs

- **Codex command limits** → A CLI source on Codex cannot yet be held to read commands; such runs use Claude or do not start. Validate Codex's command rules (`execpolicy` prefix rules with a default of forbidden) in stage 3 and lift the restriction only if they hold.
- **MCP tools are named, not proven read-only** → A source lists the MCP tools it may call; a tool named like a read can still write. The recipe confirmation lists each tool, and the job's page says jobs are trusted to read only through the tools listed.
- **Router mistakes file a topic in the wrong brain** → The threshold, the ask-on-unsure rule and "Change" on each routed message bound it; entries already written stay and are named so the user can move them.
- **Sleep and the host not running** → No wake scheduling. A brief due at 07:00 on a Mac opened at 08:30 runs once at 08:30, marked as catch-up.
- **WKWebView overlays** → see decision 6.
- **Shared instructions are an attack surface** → Whoever can push to a shared Space's storage changes every member's thread instructions. The Space page says so, Space sync pauses edits made outside Wiffletree, and thread authority does not depend on instructions.
- **Personal data in shared outputs** → First-push approval, the "Only on this Mac" default for thread outputs and the confirmation on "Share to Space…".

## Migration Plan

Stage by stage, each landing alone (see tasks.md). The stage 1 upgrade creates Spaces from today's brains (decided while writing, 5) and moves `Project.brain` into the Space; a project keeps reading its brain through its Space, and `log_exports` keeps its rows, so nothing is exported twice. Rollback before stage 5 is a database restore; nothing has been pushed.

## Open Questions

- Should the issue assistant of `shared-issues` become a thread kind of this assistant? (See Overlap.)
- Should threads offer voice input or a global hotkey, as Dots and Duet do? Left out of version 1.
