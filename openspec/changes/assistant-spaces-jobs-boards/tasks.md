Six stages, each a ticket or a few, each landing on its own. Every stage needs `spaces`, which ships first and carries the Space model, its store and upgrade, storage repositories, Settings › Spaces, shared Spaces and joining (formerly tasks 1.1–1.4, 1.10, 5.1 and 5.2 here). A stage may start once the one before it has merged; stage 4's web view prototype (4.1) may start at any time; stage 6 needs stage 1 and `shared-issues` phase 3. Stage 1 ships threads without brain tools; task 1.8 adds `log_work` and `query_brain` when `brain-log-capture` and `brain-browser` land.

## 1. Assistant threads, choosing the Space by hand

- [ ] 1.1 Core: `assistant.rs` (`Thread`), `Role::Assistant`, `Job::Assistant`; thread commands.
- [ ] 1.2 Host: thread sessions owned by their Space (`SessionOwner::Space`), with scratch folders and the restricted launch for Claude and Codex; check the tools and MCP servers a run reports. Verify a denied shell write, a refused write outside scratch, a scratch write and an unexpected MCP server.
- [ ] 1.3 Host: the Assistant core, the Assisting skill and Space instructions; thread workspace tools (`save_output`, `suggest_space`, the project brief); the brain tools come in 1.8. Verify instruction order and that coordination tools are not offered.
- [ ] 1.4 Host: Make this a project, in the thread's Space with its repositories; the private board store for `save_output`.
- [ ] 1.5 Models: the Assistant row, its upgrade copy, and Space default models with fallback notes.
- [ ] 1.6 Desktop: sidebar's pinned assistant row; assistant view with tabs, Space chip in the composer, idle threads leaving the strip, search, delete; the assistant's name in Settings.
- [ ] 1.7 Docs: README and DEVELOPMENT notes on threads' authority.
- [ ] 1.8 Host: the brain tools, each once its change has landed, so stage 1 does not wait for them: `log_work` to the Space's brain after `brain-log-capture` (task 7.1, moved here from its task 2.4), and `query_brain` on the Space's brain after `brain-browser` (task 8.1, moved here from its task 2.1). Until then a thread asked to remember something says it cannot yet. Verify the instruction order and tool list before and after each lands.

## 2. The router

- [ ] 2.1 Core: `RouterInput`, `RouterAnswer`, `Choice`; `Job::Router`.
- [ ] 2.2 Host: `RouterProvider` with Claude (`--json-schema`) and Codex (`--output-schema`) runs started with tools off; discard answers from runs that report tools.
- [ ] 2.3 Host: the host rules, the threshold setting in `settings/assistant.json`, held messages that survive a restart, Change. Verify each unsure path (low confidence, mismatched Space, unknown id, timeout, spent router, bad shape) and that nothing reaches a brain before the user answers.
- [ ] 2.4 Models: the Router row, its upgrade default, and "never waits".
- [ ] 2.5 Desktop: routed-message line with Change; inline question with one-click choices and "Somewhere else…"; threshold in Settings.

## 3. Jobs

- [ ] 3.1 Core: `job.rs` (recipe, built-in job kind, run record), `Role::JobRun`, `Job::Job`.
- [ ] 3.2 Host: recipe files in the Space's storage; the human-only save command; the recipe card a thread proposes. Verify no tool or job run can save, enable or change a job.
- [ ] 3.3 Host: scheduler with timezones, daylight-saving rules and one catch-up run after sleep or a stopped host; pause, resume, Run now, delete; built-in host jobs with an owner, their own triggers and run function, listed in no Space. Verify a built-in job's catch-up and that no command can create or change one.
- [ ] 3.4 Host: source declarations and the pre-run check for MCP servers and CLIs (gh, linctl, sentry-cli, aws profiles); the run record naming what is missing.
- [ ] 3.5 Host: run threads with only their sources; outputs stored at stable paths as commits; the 30-minute note and attention counts.
- [ ] 3.6 Validate Codex command rules for CLI sources; until they hold, a CLI-source run moves to the next choice that can limit it or does not start.
- [ ] 3.7 Models: the optional Job row.
- [ ] 3.8 Desktop: recipe card with schedule in words and next runs; job page; "Start Wiffletree at login".

## 4. Boards with the web view

- [x] 4.1 Prototype: a WKWebView over a GPUI placeholder through `wry`'s `build_as_child` (fallback: `objc` binding), following bounds on resize and pane moves, hidden when unpainted, still image under GPUI overlays, focus and window shortcuts. Measure memory with one board open and closed. Report before 4.4 builds on it.
- [ ] 4.2 Desktop: the `wiffletree-board://` scheme reading only the output's folder, the CSP header, the content rule list blocking every other scheme, navigation and new-window refusal, non-persistent data store. Verify a fetch, a socket, a link and a redirect all stay inside.
- [ ] 4.3 Host: board listing from storage repositories and the private store, versions from Git history.
- [ ] 4.4 Desktop: Boards library (filters, search, previews, stale and runner labels, jobs list); open by type (native Markdown, web view, Quick Look, default app); version picker; Share to Space….

## 5. Shared jobs and runners

- [ ] 5.1 Host: the boards rule set on `spaces`' repository engine: runner outputs pushed without comparing, refetch on a moved remote, pause on a local change. Verify in a storage repository that is also the settings repository.
- [ ] 5.2 Host: shared jobs, one runner or everyone; Take over running as a commit; staleness; first-push approval and re-approval after a prompt or sources change.
- [ ] 5.3 Desktop: runner and approval in Boards and the job page.
- [ ] 5.4 Docs: README on shared jobs, who can change instructions, and what agents' shells can still do.

## 6. Issue threads

- [ ] 6.1 Core: issue threads (a thread with an issue or a tracker's list attached), `Proposal` and its kinds, `Command::ApplyProposal` and `DiscardProposal`.
- [ ] 6.2 Host: issue threads with web search and fetch off on Claude and Codex, and the tools `list_issues`, `read_issue`, `search_repositories`, `read_repository_file` and `propose_*`; `propose_issue` for every thread in its own Space; one thread per issue and Space and one per tracker list and Space, resumed only in its own Space. Verify a tracker two Spaces share opens a separate thread in each. Verify no web tool, a refused `link_issue` and `create_ticket`, and that the Space's tracker is the only one listed.
- [ ] 6.3 Host: `search_repositories` and `read_repository_file` over the Space's repositories at their base branch's fetched head (`git grep`, `git show`), off the command thread, bounded in size.
- [ ] 6.4 Host: proposals stored with the thread; Apply runs `shared-issues`' human write commands as the human, with `assistant_for` on comments; a proposal whose issue changed shows the new diff.
- [ ] 6.5 Desktop: Ask the assistant on the issue page and the Issues page; proposal cards (Apply, Edit, Discard) and the authors notice in the thread; "Assistant for ‹name›" on applied comments.

## 7. Assistant threads log work (moved from brain-log-capture)

- [ ] 7.1 Give assistant threads `log_work` (`project-memory` delta here): entries in `logs/assistant/YYYY/MM/` with `repo: assistant` and `ref: —`, a `repository` or `ticket_id` refused, the thread's id and Space in the store, never `logs/assistant.md`. Batch a thread's entries per brain and thread, and close the batch when the thread is deleted. Job runs get no `log_work`. Test a thread entry against the gate fixtures in `gate/tests/fixtures/`, and the batch closing on deletion.

## 8. Assistant threads query the brain (moved from brain-browser)

- [ ] 8.1 Give assistant threads, issue threads and job runs `query_brain` (`brain-browser` delta here) on their own Space's brain, always with the whole-brain scope, since a thread has no project: `tools_for` and `brain_index::queries_brain` list them, and the brain resolves through `owner_brain` for a Space-owned session. A `scope` of `project` from a thread is answered over the whole brain. The search overlay in a thread searches the thread's Space's brain, and a thread whose Space has no brain gets the "no brain" overlay. Test that a thread in one Space never reads another Space's brain, and the thread's scope.
