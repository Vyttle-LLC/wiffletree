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
The user SHALL NOT have to choose where to file anything. Each thread SHALL belong to one Space, and its log entries SHALL go to that Space's brain under `logs/assistant.md`, each carrying the thread's id and Space. When a message starts a new topic, the router (see assistant-router) SHALL send it to the thread and Space it belongs to, asking the user when unsure. A thread in a Space with no brain SHALL add no log entries and SHALL say so when asked to remember something.

#### Scenario: Personal then Vyttle
- **WHEN** the user asks about a dentist appointment and then, in the same tab, about Vyttle's pricing page
- **THEN** the second message goes to a Vyttle thread, and its log entries go to the Vyttle brain, not the personal brain

### Requirement: Make this a project
A thread SHALL offer Make this a project. It SHALL ask the thread to write a project brief from the conversation and show it to the user, who SHALL be able to edit it and SHALL confirm it. Confirming SHALL create a Wiffletree project in the thread's Space, whose first human message is the brief with a link back to the thread, and whose conversation files are copies of the scratch files the user ticks. The thread SHALL stay open and read-only, and SHALL show a link to the project.

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
