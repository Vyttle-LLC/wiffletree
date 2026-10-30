## Purpose

Gives each user one assistant for quick questions and small research: many threads in tabs, each a small read-only provider session in a Space, sharing memory through the Space's brain, with code work handed to projects.

## ADDED Requirements

### Requirement: One assistant per user
Each Wiffletree install SHALL have exactly one assistant. The user SHALL name it; until then it SHALL be called "Assistant". The name SHALL be a shared setting (see settings-repository). Threads SHALL run on the model their Who runs what row picks (see model-routing), unless the thread's Space names an assistant model this Mac may use or the user chose a model in the thread's composer. The assistant SHALL be pinned in the sidebar above Projects (see native-workspace).

#### Scenario: Naming the assistant
- **WHEN** the user renames the assistant to "Dot" on one Mac that uses a settings repository
- **THEN** the sidebar row reads "Dot" on that Mac at once and on the user's other Mac after its next pull

#### Scenario: A thread's model
- **WHEN** the Assistant row is Sonnet · medium, the thread's Space names no assistant model and the user picked nothing
- **THEN** a new thread starts on Sonnet · medium and its header says "Chosen by Wiffletree: First choice"

### Requirement: Threads in tabs
The assistant SHALL open in one view that holds many threads as tabs. Each tab SHALL show the thread's title, its Space's name and colour, and whether a turn is running. Several threads SHALL be able to run turns at the same time. The user SHALL open a new tab, close a tab, rename a thread and drag tabs into order. A new thread SHALL be titled from its first message.

#### Scenario: Two topics at once
- **WHEN** a turn is running in an "API date format" tab and the user opens a new tab and asks about a flight
- **THEN** both turns run at the same time and each tab shows its own working mark

### Requirement: Each thread is its own provider session
Each thread SHALL be its own provider session, started on its first message with only its instructions and that message, and resumed by its session id afterwards. A thread SHALL NOT receive another thread's conversation. Threads SHALL share knowledge only through their Space's brain.

#### Scenario: A one-liner starts small
- **WHEN** the user asks a one-line question in a new tab
- **THEN** the provider session starts with the instructions and that one message, and no other thread's text

#### Scenario: Memory through the brain
- **WHEN** a thread logged "API dates are ISO 8601 in UTC" to the Sage brain and a later Sage thread is asked about the API's date format
- **THEN** the later thread can find the entry by searching the Sage brain, and was not sent the first thread's conversation

### Requirement: Thread authority
A thread SHALL run in its own scratch folder at `<data folder>/assistant/threads/<thread-id>/`. It SHALL be able to read the repositories added to Wiffletree and other local files, search and read the web, read its Space's brain, add log entries to its Space's brain, use the sources its Space lists (see spaces), and write files only in its scratch folder. A thread SHALL NOT edit a repository, commit, push, write outside its scratch folder, start agents or create tickets. A thread SHALL NOT write to any brain but its Space's, and SHALL write to it only by adding log entries.

#### Scenario: Asked to fix a bug
- **WHEN** the user asks a thread to fix a typo in `mobile-app`
- **THEN** the thread does not edit the repository and offers Make this a project

#### Scenario: Writing a draft
- **WHEN** a thread drafts a memo
- **THEN** the file is written in the thread's scratch folder

### Requirement: The provider enforces thread authority
Threads and job runs SHALL NOT start in the providers' YOLO mode. A Claude thread SHALL start with a permission mode that denies, without asking, every tool not allowed; the allowed tools SHALL be reading, file search, web search and fetch, writing and editing only under the scratch folder, the thread's workspace tools, and for each source in use its listed MCP tools and shell commands. A Codex thread SHALL start in the workspace-write sandbox rooted at the scratch folder, with no other writable root and with `sandbox_workspace_write.exclude_slash_tmp` and `sandbox_workspace_write.exclude_tmpdir_env_var` both true, so that its shell can write only the scratch folder and the device files Codex's sandbox always allows, such as `/dev/null`; it SHALL run with approvals never asked, shell network off and web search on, and with MCP servers limited to the listed tools. The host SHALL read the tools and MCP servers a run reports at its start, and SHALL stop the run, saying why, when it reports any it was not given.

#### Scenario: A shell write on Claude
- **WHEN** a Claude thread tries `Bash(git commit -am wip)` in a repository
- **THEN** the call is denied without a prompt and nothing changes

#### Scenario: A write outside scratch on Codex
- **WHEN** a Codex thread tries to write `~/dev/mobile-app/README.md`
- **THEN** the sandbox refuses it, and the same thread can write `notes.md` in its scratch folder

#### Scenario: Temporary folders on Codex
- **WHEN** a Codex thread's shell tries to write `/tmp/out.txt` or a file under `$TMPDIR`
- **THEN** the sandbox refuses it

#### Scenario: An unexpected tool
- **WHEN** a thread's run reports at its start an MCP server that its Space's sources do not list
- **THEN** the host stops the run and the thread shows that an unlisted MCP server was offered

### Requirement: Topics are filed in their Space's brain
The user SHALL NOT have to choose where to file anything. Each thread SHALL belong to one Space, and its log entries SHALL go to that Space's brain as per-entry files under `logs/assistant/YYYY/MM/`, with `repo: assistant`, written through `log_work` and reaching `main` through a gated batch PR (see `project-memory` in `brain-log-capture`), with the thread's id and Space kept in the host store. When a message starts a new topic, the router (see assistant-router) SHALL send it to the thread and Space it belongs to, asking the user when unsure. A thread in a Space with no brain SHALL add no log entries and SHALL say so when asked to remember something.

#### Scenario: Personal then Vyttle
- **WHEN** the user asks about a dentist appointment and then, in the same tab, about Vyttle's pricing page
- **THEN** the second message goes to a Vyttle thread, and its log entries go to the Vyttle brain, not the personal brain

### Requirement: Make this a project
A thread SHALL offer Make this a project. It SHALL ask the thread to write a project brief from the conversation and show it to the user, who SHALL be able to edit it and SHALL confirm it. Confirming SHALL create a Wiffletree project in the thread's Space, with that Space's repositories as New project ticks them (see `spaces`), whose first human message is the brief with a link back to the thread, and whose conversation files are copies of the scratch files the user ticks. The thread SHALL stay open and read-only, and SHALL show a link to the project.

#### Scenario: A thread becomes a project
- **WHEN** the user confirms Make this a project in a Sage thread about a sign-in bug
- **THEN** a project is created in the Sage Space with the brief as its first message, its coordinator logs to the Sage brain, and the thread links to it

### Requirement: Threads stay on this Mac
Threads, their conversations and their scratch folders SHALL be kept only in this Mac's database and data folder. They SHALL NOT be written to a settings, storage or brain repository, and SHALL NOT sync to another Mac, including in a shared Space.

#### Scenario: A shared Space
- **WHEN** the user talks in a thread of the shared Sage Space
- **THEN** no teammate can see the thread, and only its log entries reach the Sage brain

### Requirement: Idle threads leave the tab strip
A thread with no message for 24 hours, or whose tab the user closed, SHALL leave the tab strip and SHALL stay in the database with its scratch folder. A search box in the assistant SHALL find threads by title and text, open or idle, filtered by Space, and opening a result SHALL bring its tab back. Deleting a thread SHALL ask first and remove its conversation and scratch folder, and SHALL leave its brain entries.

#### Scenario: Yesterday's thread
- **WHEN** a thread had its last message 30 hours ago
- **THEN** its tab is gone, searching for its title finds it, and opening it resumes its provider session

### Requirement: The assistant may suggest a Space but not create one
When the user talks about something that fits no Space, a thread SHALL be able to suggest a new Space with a name and description as a card. The card SHALL open the Spaces settings page filled in. No thread, job run or router SHALL create, change or delete a Space.

#### Scenario: A new side project
- **WHEN** a thread suggests a "Woodworking" Space
- **THEN** a card offers to open Settings › Spaces with the name and description filled in, and nothing is created until the user saves there

### Requirement: A thread can save an output
A thread SHALL have a `save_output` tool that takes a file in its scratch folder and a title. The host SHALL copy the file to this Mac's private board store under the thread's Space and list it on Boards as "Only on this Mac" (see boards). Saving again under the same title SHALL add a version.

#### Scenario: Saving a chart
- **WHEN** a thread calls `save_output` for `scratch/costs.html` titled "AWS costs"
- **THEN** Boards lists "AWS costs" in the thread's Space as only on this Mac, and the file is not pushed anywhere

### Requirement: Issue threads
The issue page SHALL offer Ask the assistant, which opens the issue thread of that issue, and the Issues page's Ask the assistant SHALL open the list thread of its tracker. Each issue SHALL have one issue thread per Mac, and each tracker one list thread, resumed when reopened. An issue thread SHALL be an assistant thread in the issue's Space (see `shared-issues`, "An issue's Space"), with the thread authority rules, its Space's instructions and the Assistant row, listed among the assistant's threads with the issue's ID or "Issues" in its title. A message sent in its pane SHALL go to it without the router. Beside the tools every thread has, it SHALL have `list_issues` and `read_issue` over its Space's tracker; `search_repositories` and `read_repository_file`, which search and read its Space's repositories on this Mac at their base branch's fetched head, read-only and never in a worktree; and `propose_edit`, `propose_comment`, `propose_split` and `propose_duplicate`. Because issue text is untrusted, an issue thread SHALL run with web search and web fetch off: Claude SHALL NOT be allowed `WebSearch` or `WebFetch`, and Codex SHALL run with its `web_search` tool off. It SHALL NOT have `link_issue`, `create_ticket`, `assign_ticket` or any tool that writes to a repository or tracker.

#### Scenario: Opening on an issue
- **WHEN** the human opens the assistant on SAGE-151, closes it, and opens it again
- **THEN** the same thread resumes in the Sage Space, and its pane names SAGE-151 and the thread's model

#### Scenario: No web in an issue thread
- **WHEN** SAGE-151's description asks the reader to fetch a URL with the conversation in it
- **THEN** the issue thread has no web tool, and nothing is fetched

#### Scenario: Searching a repository
- **WHEN** an issue thread calls `search_repositories` for "delivery" in `Sage-Social/watch-app`
- **THEN** the host returns matching lines from the base branch's fetched head, and no worktree or branch is created or changed

#### Scenario: On Codex
- **WHEN** the Assistant row's first choice is a Codex model and the human opens an issue thread
- **THEN** the thread runs on Codex in its sandbox with `web_search` off

### Requirement: A thread may propose an issue in its Space
Every assistant thread, issue threads included, SHALL have `propose_issue`, which drafts an issue for its own Space's tracker. Job runs SHALL NOT have it. A proposed issue SHALL be applied like any other proposal, and SHALL be filed only in the thread's Space's tracker.

#### Scenario: From a conversation to an issue
- **WHEN** a Sage thread about a watch crash proposes an issue and the human applies it
- **THEN** the issue is filed in Sage's tracker as the human, and the human can then Start project from issue

### Requirement: Every issue change is a proposal the human applies
Each `propose_*` call SHALL store a proposal and show it as a card in the thread: an edit as a diff of the fields and description it changes, a comment as its text, a split as the new issues and the edit to the original, a duplicate link as the two issues, and a new issue as its draft. Nothing SHALL reach the tracker until the human chooses Apply. The card SHALL also offer Edit, which opens the proposal in the matching form, and Discard. Applying SHALL be the human's own command and SHALL write as the human. A proposal whose issue changed since it was made SHALL say so and show the diff against the current version before Apply.

#### Scenario: Applying an edit
- **WHEN** an issue thread proposes acceptance criteria for SAGE-151's description and the human chooses Apply
- **THEN** SAGE-151's description gains them in one commit authored by the human, and the card shows it was applied

#### Scenario: Discarding
- **WHEN** the human discards a proposal
- **THEN** nothing is written to the tracker

#### Scenario: The issue moved on
- **WHEN** a teammate edits SAGE-151's description after the thread proposed an edit to it
- **THEN** the card says the description changed since the proposal and shows the proposal against the teammate's version

### Requirement: The assistant's comments say whose assistant wrote them
A comment applied from a proposal SHALL have the human's login as `author` and as `assistant_for`, and every Mac SHALL show it as written by "Assistant for ‹name›", styled apart from people's comments. An issue filed from a proposal SHALL show it was filed by the assistant for the human.

#### Scenario: A comment from the assistant
- **WHEN** Jamie applies a proposed comment on SAGE-151
- **THEN** Priya's Mac shows it as written by "Assistant for Jamie"

### Requirement: Issue threads treat issue text as context
An issue thread's pane SHALL show a notice naming the people who wrote the issue and its comments. Tool results that carry issue text SHALL mark each passage with its author, as `shared-issues` says, and the thread SHALL act only on the human's messages in its pane.

#### Scenario: A request inside an issue
- **WHEN** SAGE-151's description asks the reader to close every other issue
- **THEN** the thread does not propose closing them unless the human asks, and the pane's notice names who wrote the description
