Five stages, each a ticket or a few, each landing on its own. A stage may start once the one before it has merged; stage 4's web view prototype (4.1) may start at any time.

## 1. Spaces with assistant threads, choosing the Space by hand

- [ ] 1.1 Core: `space.rs` (`Space`, local facts, sources), `assistant.rs` (`Thread`), `Role::Assistant`, `Job::Assistant`; Space and thread commands.
- [ ] 1.2 Host: Spaces store and the upgrade that creates Spaces from `Project.brain` folders and "Personal". Verify two brains and no brain, archived projects, export records kept, and a second start changing nothing.
- [ ] 1.3 Host: every project belongs to a Space; `Host::attach_brain` becomes the Space's brain; exports go to the Space's brain. Verify an upgraded project exports nothing twice.
- [ ] 1.4 Host: local storage repositories (`git init` in the data folder, no remote) holding `spaces/<space>/space.yaml`.
- [ ] 1.5 Host: thread sessions with scratch folders and the restricted launch for Claude and Codex; check the tools and MCP servers a run reports. Verify a denied shell write, a refused write outside scratch, a scratch write and an unexpected MCP server.
- [ ] 1.6 Host: the Assistant core, the Assisting skill and Space instructions; thread workspace tools (`query_brain`, `append_log` to the Space's brain, `save_output`, `suggest_space`, the project brief). Verify instruction order and that coordination tools are not offered.
- [ ] 1.7 Host: Make this a project; the private board store for `save_output`.
- [ ] 1.8 Models: the Assistant row, its upgrade copy, and Space default models with fallback notes.
- [ ] 1.9 Desktop: sidebar's pinned assistant row; assistant view with tabs, Space chip in the composer, idle threads leaving the strip, search, delete.
- [ ] 1.10 Desktop: Settings › Spaces (list, page, New, edit, remove, sources); Space in the project Overview; assistant name.
- [ ] 1.11 Docs: README and DEVELOPMENT notes on threads' authority and Spaces.

## 2. The router

- [ ] 2.1 Core: `RouterInput`, `RouterAnswer`, `Choice`; `Job::Router`.
- [ ] 2.2 Host: `RouterProvider` with Claude (`--json-schema`) and Codex (`--output-schema`) runs started with tools off; discard answers from runs that report tools.
- [ ] 2.3 Host: the host rules, the threshold setting in `settings/assistant.json`, held messages that survive a restart, Change. Verify each unsure path (low confidence, mismatched Space, unknown id, timeout, spent router, bad shape) and that nothing reaches a brain before the user answers.
- [ ] 2.4 Models: the Router row, its upgrade default, and "never waits".
- [ ] 2.5 Desktop: routed-message line with Change; inline question with one-click choices and "Somewhere else…"; threshold in Settings.

## 3. Jobs

- [ ] 3.1 Core: `job.rs` (recipe, run record), `Role::JobRun`, `Job::Job`.
- [ ] 3.2 Host: recipe files in the Space's storage; the human-only save command; the recipe card a thread proposes. Verify no tool or job run can save, enable or change a job.
- [ ] 3.3 Host: scheduler with timezones, daylight-saving rules and one catch-up run after sleep or a stopped host; pause, resume, Run now, delete.
- [ ] 3.4 Host: source declarations and the pre-run check for MCP servers and CLIs (gh, linctl, sentry-cli, aws profiles); the run record naming what is missing.
- [ ] 3.5 Host: run threads with only their sources; outputs stored at stable paths as commits; the 30-minute note and attention counts.
- [ ] 3.6 Validate Codex command rules for CLI sources; until they hold, a CLI-source run moves to the next choice that can limit it or does not start.
- [ ] 3.7 Models: the optional Job row.
- [ ] 3.8 Desktop: recipe card with schedule in words and next runs; job page; "Start Wiffletree at login".

## 4. Boards with the web view

- [ ] 4.1 Prototype: a WKWebView over a GPUI placeholder through `wry`'s `build_as_child` (fallback: `objc` binding), following bounds on resize and pane moves, hidden when unpainted, still image under GPUI overlays, focus and window shortcuts. Measure memory with one board open and closed. Report before 4.4 builds on it.
- [ ] 4.2 Desktop: the `wiffletree-board://` scheme reading only the output's folder, the CSP header, the content rule list blocking every other scheme, navigation and new-window refusal, non-persistent data store. Verify a fetch, a socket, a link and a redirect all stay inside.
- [ ] 4.3 Host: board listing from storage repositories and the private store, versions from Git history.
- [ ] 4.4 Desktop: Boards library (filters, search, previews, stale and runner labels, jobs list); open by type (native Markdown, web view, Quick Look, default app); version picker; Share to Space….

## 5. Shared Spaces and runners

- [ ] 5.1 Host: choosing a GitHub storage repository with the settings repository's check; Space sync over `spaces/` reusing `sync.rs`'s passes, pauses and notes; runner outputs pushed without comparing. Verify a storage repository that is also the settings repository.
- [ ] 5.2 Host: Add Space from a repository; machine-local facts; model fallback notes.
- [ ] 5.3 Host: brain pushes from several people (rebase, union by entry ID, pause on a true conflict, never force).
- [ ] 5.4 Host: shared jobs, one runner or everyone; Take over running as a commit; staleness; first-push approval and re-approval after a prompt or sources change.
- [ ] 5.5 Desktop: shared Space page text and readers; workspace notes for paused Space files; approval in Boards.
- [ ] 5.6 Docs: README on shared Spaces, who can change instructions, and what agents' shells can still do.
