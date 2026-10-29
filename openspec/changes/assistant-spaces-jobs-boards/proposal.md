## Why

Wiffletree only knows one kind of work: a project, with a coordinator, tickets and agents. A one-line question such as "what date format did we settle on for the API?" needs a whole new project, and nothing runs on its own, such as a morning brief or a weekly dashboard. OpenAI's ChatGPT Dots and Anthropic's Claude Duet (October 2026) show the shape people now expect from an always-on personal agent: a chat, scheduled work, proactive checks, a place for larger outputs, and approval before acting. Wiffletree can offer the same on the user's own Mac, with the models the user chooses, and can hand real code work to a project when a conversation turns into one.

Work for different organizations also needs to stay apart. Today a project's brain is a per-project attachment; nothing says "this is Sage work, that is personal", and nothing lets a team share its instructions, jobs and dashboards.

## What Changes

- **Spaces**, configured containers like Claude's Projects. A Space has a name, a description of what belongs in it, instructions added to every thread and job in it, a brain (a repository and a folder), a storage repository for its boards and jobs, the code repositories it covers and default models. Every Wiffletree project belongs to a Space, and its coordinator logs to the Space's brain. Spaces are created and edited only by the user, on a settings page.
- **The assistant.** One per user, named by the user, pinned above Projects in the sidebar, running on the model its Who runs what row picks. One window holds many threads in tabs; each thread is its own provider session and shares memory with the others through the brain.
- **Threads are read-only plus a scratch folder.** A thread reads repositories, local files, the web and its Space's brain, adds log entries to that brain, and writes only in its own scratch folder. The provider enforces it. Code changes go through **Make this a project**, which creates a project in the thread's Space with the thread as its brief.
- **A router** runs before each message: a typed decision step with fixed questions (which thread, which Space, new or continue) answered as choices with confidence. Plain host rules act on the answers and ask the user with one-click choices below a threshold (0.8 by default). It runs on a small model on the user's existing providers with structured output; it never writes to a brain.
- **Scheduled jobs.** A job is a saved thread recipe: name, prompt, schedule with timezone, Space, model and the sources it reads. The user asks the assistant in plain words, confirms the recipe it shows, and only then is it saved. Each run is a fresh read-only thread whose result is saved as a file in the Space's storage. Missed runs come back as one catch-up run when the Mac wakes. Before a run, the host checks its sources are present and signed in; jobs never send, post or change anything in other apps.
- **Boards**, a library beside the assistant of every output a thread or job saves, with title, Space, job, last update and a preview. Markdown renders natively, HTML opens in an embedded WKWebView with no network and no navigation, images and PDFs open in Quick Look, and anything else opens in its default app. Each output keeps its Git history.
- **Shared Spaces.** A Space's definition lives in its storage repository, so a teammate joins with "Add Space → from a repository". Only machine-local facts stay local. Edits are commits that follow the settings repository's sync, pause and compare rules. A shared job has one runner by default, whose Mac runs it and pushes the result; a job can instead be a shared template that each member runs privately. The first push of a new shared job's output waits for the runner's approval. Threads are always private.
- **Not included:** a cloud or headless host, connectors built by Wiffletree, a thread that edits a repository, Jev or any router provider that needs an API key, and any change to coordinator timers.

## Capabilities

### New Capabilities
- `assistant`: the assistant, its window and thread tabs, thread sessions, thread authority and scratch folders, brain use and topic filing, Make this a project, and idle and searchable threads.
- `assistant-router`: the typed decision step before each message, its inputs, its host rules and threshold, asking the user, and its provider interface.
- `spaces`: Space definitions, the Spaces settings page, storage repositories, projects in a Space, the upgrade of existing projects, shared Spaces and joining one, and what stays on each machine.
- `scheduled-jobs`: job recipes and how one is created, schedules and catch-up runs, run threads and outputs, sources and their pre-run check, shared jobs and runners, and the first-push approval.
- `boards`: the Boards library, opening outputs by type, the embedded web view and its sandbox, and output history.

### Modified Capabilities
- `model-routing`: Who runs what gains Assistant, Router and Job rows.
- `project-memory`: export goes to the project's Space's brain instead of a per-project attachment; brain pushes from several people merge by entry ID and never force.
- `settings-repository`: the assistant's name and the router's threshold become shared settings; Spaces travel through their storage repositories, not the settings folder.
- `skills`: a fifth skill, Assisting, for threads and job runs; the router gets no skill.
- `workspace-host`: the enforced hierarchy admits assistant threads and job runs, which belong to a Space rather than a project, and requires every project to belong to a Space.
- `native-workspace`: the sidebar pins the assistant and Boards above Projects.

## Impact

- `workspace-core`: new `space.rs`, `assistant.rs` (threads, router answers), `job.rs` and `board.rs`; `selection.rs` (`Job::Assistant`, `Job::Router`, `Job::Job`); `Command` variants for Spaces, threads, jobs and boards.
- `workspace-host`: new `spaces.rs` and `space_sync.rs` (reusing `sync.rs`'s passes), `assistant.rs` (thread sessions, scratch folders, provider restrictions), `router.rs`, `jobs.rs` (scheduler, catch-up, source checks), `boards.rs`; `provider.rs` (restricted launch for threads), `skills.rs` (Assisting), `memory.rs` (Space brains, union merge), store migrations for Spaces and threads.
- `workspace-desktop`: sidebar assistant and Boards rows, `assistant_view.rs` (window, tabs, router questions), `boards_view.rs`, `board_web_view.rs` (WKWebView embedding through `objc2`), `settings/spaces.rs`, `models_view.rs` (three rows).
- Docs: `docs/PRD.md`, `README.md`, `design/README.md`.
- Mockup: `ui/mockups.html`, awaiting the human's approval.
