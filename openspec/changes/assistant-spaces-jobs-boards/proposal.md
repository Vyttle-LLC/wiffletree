## Why

Wiffletree only knows one kind of work: a project, with a coordinator, tickets and agents. A one-line question such as "what date format did we settle on for the API?" needs a whole new project, and nothing runs on its own, such as a morning brief or a weekly dashboard. OpenAI's ChatGPT Dots and Anthropic's Claude Duet (October 2026) show the shape people now expect from an always-on personal agent: a chat, scheduled work, proactive checks, a place for larger outputs, and approval before acting. Wiffletree can offer the same on the user's own Mac, with the models the user chooses, and can hand real code work to a project when a conversation turns into one.

Work for different organizations also needs to stay apart. That is what Spaces do; they were first proposed here and now ship first as their own change, `spaces`, which this change builds on.

## What Changes

- **Builds on `spaces`.** Every thread, job, board and issue thread belongs to a Space, and reads the Space's instructions, brain, storage, repositories, default models and sources (see `spaces`). Spaces, their settings page, storage repositories, the upgrade from `Project.brain`, shared Spaces and joining one moved to `spaces` on October 29, 2026.
- **The assistant.** One per user, named by the user, pinned above Projects in the sidebar, running on the model its Who runs what row picks. One window holds many threads in tabs; each thread is its own provider session and shares memory with the others through the brain.
- **Threads are read-only plus a scratch folder.** A thread reads repositories, local files, the web and its Space's brain, adds log entries to that brain, and writes only in its own scratch folder. The provider enforces it. Code changes go through **Make this a project**, which creates a project in the thread's Space with the thread as its brief.
- **Issue threads.** The issue assistant of `shared-issues` becomes an issue thread: an assistant thread in the issue's Space with the issue tools (`list_issues`, `read_issue`, `search_repositories`, `read_repository_file` and the `propose_*` proposals the human applies), and with web search and fetch off, because issue text is untrusted. Any thread may propose an issue in its Space, so the path becomes assistant → issue → project.
- **A router** runs before each message: a typed decision step with fixed questions (which thread, which Space, new or continue) answered as choices with confidence. Plain host rules act on the answers and ask the user with one-click choices below a threshold (0.8 by default). It runs on a small model on the user's existing providers with structured output; it never writes to a brain.
- **Scheduled jobs.** A job is a saved thread recipe: name, prompt, schedule with timezone, Space, model and the sources it reads. The user asks the assistant in plain words, confirms the recipe it shows, and only then is it saved. Each run is a fresh read-only thread whose result is saved as a file in the Space's storage. Missed runs come back as one catch-up run when the Mac wakes. Before a run, the host checks its sources are present and signed in; jobs never send, post or change anything in other apps. The scheduler also runs **built-in host jobs** that no one saves, starting with brain compile (`brain-compile`).
- **Boards**, a library beside the assistant of every output a thread or job saves, with title, Space, job, last update and a preview. Markdown renders natively, HTML opens in an embedded WKWebView with no network and no navigation, images and PDFs open in Quick Look, and anything else opens in its default app. Each output keeps its Git history.
- **Shared jobs.** A shared job has one runner by default, whose Mac runs it and pushes the result without comparing; a job can instead be a shared template that each member runs privately. The first push of a new shared job's output waits for the runner's approval. Threads are always private.
- **Not included:** a cloud or headless host, connectors built by Wiffletree, a thread that edits a repository, Jev or any router provider that needs an API key, and any change to coordinator timers.

## Capabilities

### New Capabilities
- `assistant`: the assistant, its window and thread tabs, thread sessions, thread authority and scratch folders, brain use and topic filing, Make this a project, idle and searchable threads, issue threads, proposals and proposed issues.
- `assistant-router`: the typed decision step before each message, its inputs, its host rules and threshold, asking the user, and its provider interface.
- `scheduled-jobs`: job recipes and how one is created, schedules and catch-up runs, run threads and outputs, sources and their pre-run check, shared jobs and runners, the runner's push rule, the first-push approval, and built-in host jobs.
- `boards`: the Boards library, opening outputs by type, the embedded web view and its sandbox, and output history.

### Modified Capabilities
- `model-routing`: Who runs what gains Assistant, Router and Job rows. Delta order: 1 (this) → 2 `brain-compile`.
- `settings-repository`: the assistant's name and the router's threshold become shared settings. Delta order: 1 `spaces` → 2 (this).
- `skills`: a fifth skill, Assisting, for assistant threads, issue threads and job runs; the router gets no skill. The only delta.
- `workspace-host`: the enforced hierarchy admits assistant threads, issue threads and job runs, owned by a Space. Delta order: 1 `spaces` → 2 (this) → 3 `brain-compile`.
- `native-workspace`: the sidebar pins the assistant and Boards above Projects.

## Impact

- Depends on `spaces` for every stage, and on `shared-issues` phase 3 for issue threads (stage 6).
- `workspace-core`: new `assistant.rs` (threads, router answers, proposals), `job.rs` and `board.rs`; `selection.rs` (`Job::Assistant`, `Job::Router`, `Job::Job`); `Role::Assistant`, `Role::JobRun`; `Command` variants for threads, proposals, jobs and boards.
- `workspace-host`: `assistant.rs` (thread sessions, scratch folders, provider restrictions, issue threads), `router.rs`, `jobs.rs` (scheduler, catch-up, source checks, built-in host jobs), `boards.rs` and the boards rule set on `repos.rs`; `provider.rs` (restricted launch for threads), `skills.rs` (Assisting), store migrations for threads.
- `workspace-desktop`: sidebar assistant and Boards rows, `assistant_view.rs` (window, tabs, router questions, proposal cards), `boards_view.rs`, `board_web_view.rs` (WKWebView embedding through `objc2`), `models_view.rs` (three rows), Ask the assistant on the Issues and issue pages.
- Docs: `docs/PRD.md`, `README.md`, `design/README.md`.
- Mockup: `ui/mockups.html`, approved by Michael on October 29, 2026.
