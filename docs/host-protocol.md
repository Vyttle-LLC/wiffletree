# Host protocol

How a client talks to a Wiffletree host: the desktop app today, and later `remote-hosts` devices and an iPhone client. The types are in `workspace-core` (`crates/workspace-core/src/host.rs` and `Command` in `lib.rs`); this page is their reference, and a test checks that its command table lists every `Command` with its read or change class.

## Transports

| Transport | Started as | Handshake | Protocol |
| --- | --- | --- | --- |
| `<data folder>/host.sock` | `workspace-host serve --data-dir <dir>` | `Hello` first | 2, or 1 if the client asks for it |
| Standard input and output | `workspace-host --data-dir <dir>` | none | 1 only |

The standard-input mode is for scripts such as `scripts/test_communications.py`: one request per line on standard input, one response per line on standard output, and the host stops when its input closes. Everything else on this page is about the socket.

### The socket

- `serve` creates `host.sock` with mode 0600 inside the data folder, which it keeps at 0700, and removes a socket a previous host left. A socket path longer than the platform allows (103 bytes on macOS) stops the host at start with that reason.
- Each accepted connection is checked with `getpeereid` (`SO_PEERCRED` on Linux). A connection from another user is closed before anything is read.
- A child host, `serve --data-dir <dir> --child`, reads the app's client credential from the first line of its standard input and stops when that input closes. The app keeps the pipe open for as long as it runs, so the credential is never in the host's environment, which agents inherit, or in a file. `workspace_core::ChildHost` starts one this way.
- Without `--child`, the credential is the data folder's login Keychain item, created at the first start with an access list of the host's own binary and the app given with `--app <Wiffletree.app>` (`crates/workspace-host/src/keychain.rs`).
- SIGTERM, SIGINT and SIGHUP run the host's shutdown, which stops running turns as a quit does, then remove the socket.
- A launchd host runs from a versioned copy of its binary, `<data folder>/host/<version>/workspace-host`, which an update of the app does not replace, and starts its agents' helpers from that same copy. Started from a copy, it removes the other version folders under `host/`. A child host runs from the app bundle (or `target/`) and has no copy.

## Framing

Every message is one line of JSON ended by `\n`. A line from the client, `Hello` or a request, is at most 256 KiB; a longer one closes the connection. Lines from the host have no such bound: a `read_file` response carries up to 50 MB as base64, about 67 MiB, so a client accepts host lines of at least 70 MiB. A connection carries:

- from the client: one `Hello`, then requests;
- from the host: one answer to `Hello`, then responses and, after `Subscribe`, events.

Requests are answered in any order; match a response to its request by `id`. Writes never interleave within a line.

```json
{"version": 2, "id": "c4a1…", "command": {"type": "snapshot"}}
{"version": 2, "id": "c4a1…", "result": {…}, "error": null}
```

`command` is tagged by `type` in snake_case, with the fields below. A response has `result` or, when the command failed or was refused, `error`, a sentence for a person. `version` in a response is the request's. `host_ms`, when present, is how long the host took from reading the request to writing the reply.

Types in the command table are Rust's: `String` and `PathBuf` are JSON strings, `Option<T>` is `T` or `null`, `Vec<T>` an array, integers are numbers, and named types are the `workspace-core` structs and enums of that name in their serde form. A field marked `?` may be left out.

## The handshake

The first line a client sends is `Hello`:

```json
{"hello": {"protocols": [1, 2], "app_version": "0.9.0", "client": {"name": "Wiffletree", "kind": "desktop"}, "capabilities": [], "credential": "…"}}
```

- `protocols` is the oldest and newest protocol the client speaks.
- `client.kind` says what the client is, such as `desktop` or `script`; the host only reports it.
- `credential` is the app's client credential. Leave it out unless you are the app.

The host chooses the newest protocol both ranges include. It accepts its own protocol version and the one before it (`PROTOCOLS`, `[1, 2]` today), so an app one release ahead or behind still works. It answers with one of:

```json
{"welcome": {"protocol": 2, "host_id": "…", "host_version": "0.9.0", "epoch": "…", "capabilities": ["events", "files", "folders"], "binary": "/…/workspace-host"}}
{"refused": {"host_protocols": [1, 2], "client_protocols": [3, 4], "host_version": "0.9.0", "working": 2}}
```

- `host_id` stays the same for a data folder across restarts; `epoch` is new each time the host starts.
- After `welcome`, every request must use the chosen `protocol`; another version is refused.
- After `refused`, which says how many agents are in a turn, the connection may send only `restart_host`. Every other command is refused, so an updated app can always restart an older host.
- Any line before `Hello` is refused with the error `Send Hello first` and runs nothing; the connection can still send `Hello`.

`Hello`, both answers and `restart_host` keep the same shape in every protocol version.

### Capabilities

A client never sends a command for a capability the host did not list.

| Capability | Commands |
| --- | --- |
| `events` | `subscribe` |
| `files` | `resolve_paths`, `read_file` |
| `folders` | `list_folder` |
| `restart-when-idle` | `restart_host` with `when: "idle"` |
| `uploads` | `upload_attachment`, and `send`'s `uploads` |
| `attention` | `subscribe`'s `kinds`, and the `attention` event |

This host lists `events`, `files`, `folders`, `restart-when-idle`, `uploads` and `attention`, and refuses a command whose capability it does not list.

### Ending the host

- `stop_agents_and_quit` and `restart_host` with `when: "now"` run the host's shutdown, which stops every running turn as a quit does and records each as interrupted by the quit with its input held. The reply comes once the shutdown has stopped them; the host then exits successfully, so launchd does not start it again.
- `restart_host` with `when: "idle"` is answered at once. The host exits successfully at the first moment no turn and no compaction runs, which may be at once. New turns start as usual while it waits.
- In both cases the app then starts the host it wants, such as an update's new version.

### Human-only commands

Some commands are only the human's to give: a `send` with no `sender`, which is a message from the human; answering the Inbox (`resolve_attention`, `dismiss_attention`, which also approves a publish); granting publish or push authority without the Inbox (`set_owner_allowed`, `set_keeper`, `set_up_settings_repository`, `join_settings_repository`); and in `remote-hosts` pairing and device access. On the socket they run only for a connection whose `Hello` carried the app's client credential; without it they are refused and change nothing. Every other command works without it. This raises the bar for an agent, which runs as the same user and can open the socket; it is not a sandbox.

## Events

`subscribe` turns the connection into a subscriber; a connection subscribes at most once. Its reply is `Subscribed`, the cursor of the host's latest event:

```json
{"version": 2, "id": "…", "result": {"cursor": {"epoch": "…", "seq": 41}}, "error": null}
```

Events then arrive beside responses:

```json
{"event": {"cursor": {"epoch": "…", "seq": 42}, "kind": "steps", "sessions": ["…"]}}
```

| `kind` | Carries | Fetch again |
| --- | --- | --- |
| `state` | nothing | `snapshot` |
| `steps` | `sessions`, the ids whose steps changed | `steps` for those sessions |
| `progress` | nothing | `progress` |
| `usage` | nothing | `usage` and `quotas` |
| `attention` | `count`, the things that need the human: attention items and workspace notes, as the Inbox counts them | nothing; sent when the count changes |
| `resync` | nothing | take a fresh `snapshot`, then continue from this event's cursor |

Events say only what to fetch again, never workspace state, so a missed event can leave a value stale but never wrong. The host emits at most one event of each kind for each thing that happens.

### Cursors

- A `Cursor` is `{epoch, seq}`. `seq` grows by one per event within an epoch; `0` is before the first.
- The host keeps the last 1,024 events in memory and persists none.
- `subscribe` with `after` set to a cursor of the current epoch still in that log first sends every later event in order, then live events.
- Any other cursor, including one from an earlier epoch or one the log no longer holds, gets `resync` first.
- With no `after`, only events after the subscription come; take a snapshot after the reply.
- Each subscriber has a queue of 256 events. A subscriber whose queue is full has it cleared and gets one `resync`, so a slow client never holds the host.

## Host files

`resolve_paths` reads only metadata: for each reference, the file it names, as it is when absolute or under `~/`, or else under the first of `bases` that holds it, and whether opening it would run it (an executable, or a `.command`, `.tool` or `.terminal` script). It answers one `ResolvedPath` or `null` per reference, in order.

`read_file` and `list_folder` resolve the path first, following symlinks and `..`, and check the resolved path:

- **Roots.** Only files under a project home, a ticket worktree, an attached repository, the workspace folder, or a project's attachment store, `<data folder>/projects/<project-id>/attachments/`, are served.
- **The data folder.** Inside it, only the attachment stores and project homes are served, and only one reached through real folders: a store or home that is a symlink, or lies under one inside the data folder, serves nothing. Whatever the root, the store (`workspace.sqlite3`, its `-wal` and `-shm` files and earlier copies such as `.pre-flatten`), `backups/`, `host/`, `logs/` and the host's settings (`settings.json`, `shared/`) are never served.
- **Credentials.** Over every root, these are refused: `~/.ssh`, `~/.config/gh`, `~/.gnupg`, `~/.aws`, `~/.netrc`, `~/.git-credentials`, `~/.config/git/credentials`, `$XDG_CONFIG_HOME/git/credentials`, `~/.claude/.credentials.json`, `~/.claude.json`, `~/.codex/auth.json`, `$CODEX_HOME/auth.json` and `~/Library/Keychains`. A listing leaves them out too.
- **Size.** `read_file` refuses a file larger than `max_bytes` or 50 MB, whichever is less. It answers `FileContents`, whose `data` is the file's bytes in standard base64.
- **Listings.** `list_folder` answers `FolderListing`: the folders in the path, each with its name and whether it is a Git repository, and nothing about files. The host's own clients may also list folders under the host's home folder, which choosing a new repository needs.

All three run off the host's main thread.

## Commands

Every `Command`, by `type`. A read changes nothing in the store, so no event follows it.

| `type` | Fields | Class | Notes |
| --- | --- | --- | --- |
| `discover_repositories` | `folders: Vec<String>` | read |  |
| `import_repositories` | `project_id: Option<String>`, `paths: Vec<String>`, `roots?: Vec<String>`, `dismissed?: Vec<String>` | change | Adds repositories to the workspace; a project with a custom selection also gains them. `roots` are parent folders to keep checking for new repositories; `dismissed` are repositories found in them that the user chose not to add. |
| `refresh_repositories` | — | change | Adds repositories that appeared in the root folders since the last check. |
| `remove_repository_root` | `path: String` | change |  |
| `set_project_repositories` | `project_id: String`, `repository_ids: Option<Vec<String>>` | change | `None` lets the project use every workspace repository. |
| `remove_repository` | `repository_id: String` | change |  |
| `snapshot` | — | read |  |
| `archive` | — | read | Read-only: the history a `Snapshot` leaves out; answered with `Archive`. |
| `set_live` | `project_id: String`, `enabled: bool` | change |  |
| `configure_session` | `session_id: String`, `profile: ModelProfile` | change |  |
| `reconcile_session` | `session_id: String`, `retry: bool` | change |  |
| `compact_context` | `session_id: String`, `instructions: Option<String>` | change | Queues a compaction of a Claude session's context; only the human sends it. |
| `cancel_compaction` | `session_id: String` | change | Withdraws a compaction that has not started. |
| `dismiss_compaction` | `session_id: String` | change | Clears the notice the last compaction left. |
| `quotas` | — | read |  |
| `refresh_codex_quota` | — | change | Reads Codex's account limits, records them and answers with every reading. Claude's arrive with its turns. |
| `inspect_provider` | `provider: Provider` | read | What a provider's CLI says about itself on the host: a `ProviderReport`. |
| `measure_context` | `session_id: String`, `profile: ModelProfile` | read | How full a session's context window is, measured with `profile`: a `ContextUsage`. |
| `inspect_attachments` | `paths: Vec<PathBuf>` | read | Whether each file can be attached to a message: the `Attachment` it would be, or why not. |
| `usage` | `days: u16`, `project_id: Option<String>`, `provider: Option<Provider>`, `timezone: String` | read |  |
| `create_ticket` | `coordinator_id: String`, `repository_id: String`, `title: String`, `brief: String` | change | `coordinator_id` must be a project coordinator whose project uses `repository_id`. |
| `assign_ticket` | `ticket_id: String`, `role: Role`, `profile: ModelProfile`, `instruction: String`, `focus?: Option<String>` | change | The human assigns an agent; `profile` is never limited by the Who runs what table. |
| `create_project` | `name: String` | change |  |
| `rename_project` | `project_id: String`, `name: String` | change |  |
| `move_projects` | `project_ids: Vec<String>`, `before?: Option<String>` | change | Moves the projects, kept in their relative order, to just before `before`, or to the end of the order. |
| `attach_repository` | `project_id: Option<String>`, `path: String`, `base: String` | change | Adds one repository with an explicit base, updating the base if it is already known. |
| `create_session` | `project_id: String`, `parent_id: String`, `repository_id: Option<String>`, `name: String`, `role: Role`, `profile: ModelProfile` | change |  |
| `set_status` | `session_id: String`, `status: Status` | change |  |
| `set_archived` | `session_id: String`, `archived: bool` | change | Archives or restores a session together with the sessions it owns. |
| `send` | `id: String`, `sender: Option<String>`, `recipient: String`, `body: String`, `attachments?: Vec<PathBuf>`, `uploads?: Vec<String>` | change, human only | Human only with no `sender`, which is a message from the human; an agent's message names its sender. Sent again with the same `id`, it is stored once. `uploads` names files a remote device sent with `upload_attachment`, attached as `attachments` are. |
| `messages` | `session_id: String`, `before: Option<i64>`, `limit: usize` | read |  |
| `steps` | `session_id: String`, `run_id: Option<String>`, `after: u64` | read | One run's steps changed after `after`: `run_id` names the run, or `None` for the session's latest. Answered with a `StepPage`. |
| `files` | `session_id: String`, `whole_tree?: bool` | read | The files a ticket agent touched, or every agent's on a coordinator's open tickets; answered with `Vec<TicketFiles>`. `whole_tree` also lists each worktree's files. |
| `progress` | `after: u64` | read | The progress entries changed after `after`; `0` asks for the whole board. Answered with a `ProgressPage`. |
| `run_summaries` | `session_id: String`, `run_ids: Vec<String>` | read | Duration and step counts for up to a page of the session's runs. |
| `set_model_selection` | `selection: Box<ModelSelection>`, `verification?: Option<VerificationSettings>` | change | Saves the Who runs what table, the switch threshold and the guide together; answered with the saved `ModelSelection` at its next revision. The saved one-time notes are kept. |
| `dismiss_model_notes` | — | change | Clears the Models page's one-time notes; answered with the `ModelSelection`. |
| `resolve_attention` | `id: String`, `answer: String` | change, human only |  |
| `dismiss_attention` | `id: String` | change, human only | Clears a question from the inbox without answering or waking its asker. |
| `append_log` | `input: LogInput` | change |  |
| `logs` | `project_id: String`, `before: Option<i64>`, `limit: usize` | read |  |
| `attach_brain` | `project_id: String`, `path: String` | change |  |
| `export_logs` | `project_id: String` | change |  |
| `runs` | `project_id: String` | read |  |
| `activity` | `project_id: String`, `before: Option<i64>`, `limit: usize` | read |  |
| `git_history` | `repository_id: String`, `skip: usize`, `limit: usize` | read |  |
| `settings` | — | read | Answered with `HostSettings`. |
| `set_workspaces_dir` | `path: String` | change | Checks the folder can be created and written to, saves it and answers with `HostSettings`. Only projects and tickets created afterwards use it. |
| `set_verification` | `verification: VerificationSettings` | change | Validates and saves the verification setting; answers with `HostSettings`. |
| `set_check_ins` | `child_turns: u32`, `human_turns: u32` | change | Validates and saves both check-in thresholds; answers with `HostSettings`. |
| `set_owner_allowed` | `entry: String`, `allowed: bool` | change, human only | Allows an owner, or a single fork as `owner/name`, to have PRs opened without asking, or stops allowing it; answers with `HostSettings`. Only the human's client sends it. |
| `set_keeper` | `keeper: KeeperSettings` | change, human only | Validates and saves the PR keeper's settings; answers with `HostSettings`. Only the human's client sends it. |
| `save_skill_rules` | `skill: Skill`, `rules: String` | change | Saves the human's own rules for a skill, an empty text removing them, and refuses more than `MAX_SKILL_TEXT` characters; answers with the `SkillState`. Only the human's client sends it. |
| `reset_skill` | `skill: Skill` | change | Returns a skill to its stock text with no rules of the human's own; answers with the `SkillState`. Only the human's client sends it. |
| `restore_skill` | `skill: Skill`, `entry: String` | change | Makes the state a history entry holds current, as a new entry; answers with the `SkillState`. Only the human's client sends it. |
| `check_settings_repository` | `repository: String` | read | Asks GitHub, read-only, about a repository named as `owner/name` or by its `github.com` URL, and checks this machine can read it; answers with `SettingsRepositoryCheck`. A public repository is refused. Writes nothing. |
| `set_up_settings_repository` | `repository: String` | change, human only | Adds this machine's shared settings to a repository that has none, in one pushed commit, and makes the shared folder its clone; answers with `HostSettings`. Only the human's client sends it. |
| `join_settings_repository` | `repository: String`, `with: JoinWith` | change, human only | Joins a repository that holds shared settings, keeping `with`'s; answers with `HostSettings`. Only the human's client sends it, once the human confirms. |
| `sync_now` | — | change | Runs a sync pass with the settings repository at once; answers with nothing. Only the human's client sends it. |
| `stop_settings_sync` | — | change | Stops syncing and forgets the settings repository chosen, keeping the shared folder as it is and changing nothing on GitHub; answers with `HostSettings`. Only the human's client sends it, once the human confirms. |
| `settle_paused_file` | `path: String`, `settle: Settle` | change | Settles the paused file, or a skill's folder, a workspace note names; answers with nothing. Only the human's client sends it. |
| `compare_paused_file` | `path: String` | read | Read-only: this machine's version of a paused file beside the other one, line by line, the other being the last synced version for an edit made outside Wiffletree. For a skill, its rewrite or else its rules. Answers with lines as `SkillStockChanges` does. |
| `skill_history` | `skill: Skill` | read | Read-only: a skill's history, newest first, each entry with why it could not be read when it cannot. |
| `start_skill_rewrite` | `skill: Skill` | change | Replaces the human's rules with a rewrite of the stock text, starting as the stock text and the rules, based on the stock text; answers with the `SkillState`. Only the human's client sends it. |
| `save_skill_rewrite` | `skill: Skill`, `text: String` | change | Saves a rewrite, an empty one included, and refuses more than `MAX_SKILL_TEXT` characters; answers with the `SkillState`. Only the human's client sends it. |
| `mark_skill_up_to_date` | `skill: Skill` | change | Bases a rewrite on this release's stock text, changing nothing else; answers with the `SkillState`. Only the human's client sends it. |
| `undo_skill_merge` | `skill: Skill`, `entry: String` | change | Writes back exactly the rewrite and base from before the merge the "Merged" history entry `entry` names, as a new entry; answers with the `SkillState`. Only the human's client sends it. |
| `merge_skill_update` | `skill: Skill` | change | Merges this release's stock text into an out-of-date rewrite, or tries again after a failed try; passages that overlap go to the PR keeper's model. Refused while a question about the rewrite waits. Answers with the `SkillState`. Only the human's client sends it. |
| `answer_skill_question` | `skill: Skill`, `wordings: Vec<Wording>` | change | Merges this release's stock text into an out-of-date rewrite with the human's wording for each passage that overlaps, in order, or with a question waiting, for each passage in question and the model's text for the rest; answers with the `SkillState`. Only the human's client sends it. |
| `dismiss_skill_merge` | `skill: Skill`, `entry: String` | change | Hides the note of the merge `entry` names on this machine; answers with the `SkillState`. Only the human's client sends it. |
| `skill_merge_review` | `skill: Skill`, `entry: String` | read | Read-only: the rewrite before and after the merge `entry` names, line by line, each change with where it came from. |
| `skill_stock_changes` | `skill: Skill` | read | Read-only: what changed, line by line, from a rewrite's base to this release's stock text. |
| `set_repository_checks` | `repository_id: String`, `checks: Option<RepositoryChecks>` | change | Saves or, with none, removes a repository's checks command; answers with the `Repository`. Only the human's client sends it. |
| `leftover_worktrees` | — | read | Read-only listing of leftover worktrees, answered with `Vec<LeftoverWorktree>`. |
| `remove_leftover_worktrees` | `ticket_ids: Vec<String>` | change | Removes the worktrees of the tickets the human selected from the listing, re-checking each first; answered with `Vec<LeftoverRemoval>`. |
| `processes` | `session_id: String` | read | What the agent has left running, or for a coordinator every agent of its project; answered with `Vec<AgentProcesses>`. Groups that have exited are forgotten. |
| `stop_processes` | `session_id: String`, `groups?: Vec<String>` | change | Stops these groups of the Processes panel of `session_id`, or with none named every group it lists, as the human's stop; answered as `Processes`. The agents keep working. |
| `subscribe` | `after?: Option<Cursor>`, `kinds?: Option<Vec<EventKindName>>` | read | Socket only: turns the connection into a subscriber of the host's events, after the events following `after` when it is still in the host's log, or after `resync` otherwise; answered with `Subscribed`. With no cursor only later events come. `kinds` limits the events to those kinds, and `resync`. |
| `ping` | — | read | Socket only: answered with nothing by the connection itself, without waiting for the host's actor. A client that has heard nothing for a while sends it to tell a quiet or busy host from one that is gone. |
| `resolve_paths` | `references: Vec<String>`, `bases: Vec<PathBuf>` | read | Which `references` name a file, each as it is or, when relative, under the first of `bases` that holds it; a `~/` reference is under the host's home folder. Answered with one `Option<ResolvedPath>` per reference, in order. Reads no file's contents. |
| `read_file` | `path: PathBuf`, `max_bytes: u64` | read | Reads a file under the roots `docs/host-protocol.md` lists, refusing one larger than `max_bytes` or `MAX_ATTACHMENT_BYTES`; answered with `FileContents`. |
| `list_folder` | `path: PathBuf` | read | Names the folders in `path` and which are Git repositories; answered with `FolderListing`. |
| `core_instructions` | — | read | The fixed core instructions the host gives agents, each with its text; answered with `Vec<CoreText>`, each part's `name`, `audience`, `version` and `text`. The snapshot's `core_instructions` carries the same parts without their text. |
| `stop_agents_and_quit` | — | change | Stops every running turn as a quit does, then ends the host; answered with nothing once the turns are stopped. Only the human's client sends it. |
| `restart_host` | `when: RestartWhen` | change | Ends the host so the app can start the newest installed version: `now` as `stop_agents_and_quit` does, `idle` once no turn and no compaction runs, answered at once. The only command a connection may send after a refused `Hello`. |
| `remote_access` | — | read, human only | This Mac's remote access: the switch, its name and addresses, pairing mode with its code, and the paired devices; answered with `RemoteAccess`. Only the host itself, never a remote device. |
| `set_remote_access` | `enabled: bool` | change, human only | Turns remote access on or off; answered with `RemoteAccess`. Off closes the listener and every remote connection; paired devices stay paired. |
| `open_pairing` | — | change, human only | Opens pairing mode with a new 4-digit code for 2 minutes; answered with `RemoteAccess`. |
| `cancel_pairing` | — | change, human only | Closes pairing mode; answered with `RemoteAccess`. |
| `set_device_level` | `device_id: String`, `level: AccessLevel` | change, human only | Changes what a paired device may do and closes its open connections; answered with `RemoteAccess`. |
| `remove_device` | `device_id: String` | change, human only | Forgets a paired device and closes its open connections; answered with `RemoteAccess`. |
| `upload_attachment` | `upload_id: String`, `name: String`, `offset: u64`, `bytes: String` | change | Stores a piece of at most 128 KiB, base64 in `bytes`, of a file a remote device attaches. A piece is stored only at the upload's end, so one sent again changes nothing; answered with `Uploaded`, the offset of the next piece. An upload no `send` names is deleted after an hour. |
