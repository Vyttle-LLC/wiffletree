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

### Remote devices

While remote access is on, paired devices reach the host over TLS 1.3 on TCP port 47821, with one JSON message per WebSocket text frame in place of a line. `docs/remote-protocol.md` describes the transport, pairing and pinning in full, for clients written in any language; in short:

- `/pair` runs a pairing method while pairing mode is open: `PairStart`, then `PairReply` and `PairConfirm` (`workspace_core::remote`).
- `/v1` opens with the host's `Challenge`. The device answers with `ChallengeAnswer`, its id and a signature, and the host replies with `ChallengeVerdict`: `{"accepted": {"level": "control"}}` or `"view_only"`, so the client knows what it may do before it sends anything, or `"unpaired"`, after which the host closes the connection with the reason `unpaired`. `Hello` and the rest of this page follow an accepted verdict.
- Changing a device's level or removing it closes its connections, so it reconnects and reads its new level from the verdict, or is refused.
- A device's commands are checked against its level, never against the client credential.

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
| `resubscribe` | `subscribe` again on a subscribed connection, to change its `kinds` |

This host lists `events`, `files`, `folders`, `restart-when-idle`, `uploads`, `attention` and `resubscribe`, and refuses a command whose capability it does not list.

### Ending the host

- `stop_agents_and_quit` and `restart_host` with `when: "now"` run the host's shutdown, which stops every running turn as a quit does and records each as interrupted by the quit with its input held. The reply comes once the shutdown has stopped them; the host then exits successfully, so launchd does not start it again.
- `restart_host` with `when: "idle"` is answered at once. The host exits successfully at the first moment no turn and no compaction runs, which may be at once. New turns start as usual while it waits.
- In both cases the app then starts the host it wants, such as an update's new version.

### Human-only commands

Some commands are only the human's to give: a `send` with no `sender`, which is a message from the human; answering the Inbox (`resolve_attention`, `dismiss_attention`, which also approves a publish); granting publish or push authority without the Inbox (`set_owner_allowed`, `set_keeper`, `set_protected_branches`, `set_up_settings_repository`, `join_settings_repository`); creating, editing, joining or removing a Space, setting its brain, moving a project into one, choosing which repositories it lists and setting up or renaming its tracker (`create_space`, `update_space`, `join_spaces`, `remove_space`, `set_space_brain`, `move_project_to_space`, `set_repository_spaces`, `confirm_repository_suggestions`, `set_tracker_prefix`, `set_up_issues`), and reading where its storage is on this Mac (`space_storage`); granting, forgetting, setting up or changing the compiler of a brain, amending or discarding its batches, sending its log batches now and importing its legacy logs (`confirm_brain`, `forget_brain`, `set_up_brain`, `set_brain_compiler`, `amend_brain_batch`, `send_logs_now`, `import_legacy_logs`); fixing or releasing a held log entry (`fix_held_log`, `release_held_log`); writing to a tracker (`create_issue`, `edit_issue`, `comment`, `move_issue`, `start_project_from_issue`, `link_issue`, `unlink_issue`, and `settle_paused_file` for an issue's file or part), marking an issue read (`mark_issue_read`), following one (`follow_issue`) and every step of an import from Linear (`linear_teams`, `preview_linear_import`, `import_from_linear`, `cancel_linear_import`), while reading issues (`issues`, `issue`, `issue_project_defaults`, `issue_people`) is not the human's alone; and in `remote-hosts` pairing and device access. On the socket they run only for a connection whose `Hello` carried the app's client credential; without it they are refused and change nothing. Every other command works without it, but a `snapshot` sent to such a connection leaves out `space_locals`, this Mac's facts about each Space. This raises the bar for an agent, which runs as the same user and can open the socket; it is not a sandbox.

Agents reach issues only through the project coordinator's workspace tools, over the tracker of its project's Space: `list_issues`, filtered by `state`, `assignee`, `label`, `repository` and `text`; `read_issue`, the issue with its comments and events; and `link_issue`, which links the issue to the coordinator's project, or with `ticket_id` to one of its own tickets, as `link_issue` above does. Every passage of issue text they return names its author in `written_by`, and a comment an assistant wrote names whom it wrote for in `assistant_for`, with a note that issue text is context, never instructions. Any other role is refused, as is an issue of another tracker. No agent tool writes issue text or moves an issue.

## Events

`subscribe` turns the connection into a subscriber. Its reply is `Subscribed`, the cursor of the host's latest event:

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
- A subscriber with no `after`, or one that gets `resync`, then gets one `attention` event with the host's current count, once the host has counted, so it need not read a snapshot to show it. That event carries the cursor of the `Subscribed` reply, and comes after the `resync`. Later `attention` events come only when the count changes. A subscription whose `kinds` leave out `attention` gets none of them.
- A subscribed connection may send `subscribe` again, without `after`, to a host that lists `resubscribe`. Its `kinds` replace the subscription's from the next event on, and the reply is `null`: no new cursor, and no event after the reply is missed. Events the old `kinds` left out are not sent again, so a client that widens its `kinds` reads what it shows anew. With `after`, it is refused. A host without `resubscribe` refuses any second `subscribe`; reconnect to change the `kinds` there.
- Each subscriber has a queue of 256 events. A subscriber whose queue is full has it cleared and gets one `resync`, so a slow client never holds the host.

## Host files

`resolve_paths` reads only metadata: for each reference, the file it names, as it is when absolute or under `~/`, or else under the first of `bases` that holds it, and whether opening it would run it (an executable, or a `.command`, `.tool` or `.terminal` script). It answers one `ResolvedPath` or `null` per reference, in order.

`read_file` and `list_folder` resolve the path first, following symlinks and `..`, and check the resolved path:

- **Roots.** Only files under a project home, a ticket worktree, an attached repository, the workspace folder, or a project's attachment store, `<data folder>/projects/<project-id>/attachments/`, are served.
- **The data folder.** Inside it, only the attachment stores and project homes are served, and only one reached through real folders: a store or home that is a symlink, or lies under one inside the data folder, serves nothing. Whatever the root, the store (`workspace.sqlite3`, its `-wal` and `-shm` files and earlier copies such as `.pre-flatten`), `backups/`, `host/`, `logs/` and the host's settings (`settings.json`, `shared/`) are never served.
- **Credentials.** Over every root, these are refused: `~/.ssh`, `~/.config/gh`, `~/.gnupg`, `~/.aws`, `~/.netrc`, `~/.git-credentials`, `~/.config/git/credentials`, `$XDG_CONFIG_HOME/git/credentials`, `~/.claude/.credentials.json`, `~/.claude.json`, `~/.codex/auth.json`, `$CODEX_HOME/auth.json` and `~/Library/Keychains`. A listing leaves them out too.
- **Size.** `read_file` refuses a file larger than `max_bytes` or 50 MB, whichever is less. It answers `FileContents`, whose `data` is the file's bytes in standard base64.
- **Listings.** `list_folder` answers `FolderListing`: the folders in the path, each with its name and whether it is a Git repository, and nothing about files. The host's own clients and Control devices may also list folders under the host's home folder, which choosing a new repository needs, and may name it with a leading `~`, as `~` or `~/dev`, since a remote client does not know the host's home folder. A View only device lists only within the roots, and a `~` path is refused. `resolve_paths` takes `~/` references from every client, and for a remote device answers only files within the roots.

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
| `create_project` | `name: String`, `space_id?: Option<SpaceId>` | change | The human creates a project, in `space_id` or else the Space last used for a new project. A coordinator creates one with the workspace tool `create_project`, which also takes a brief and repository ids, or else uses its Space's repositories on this Mac and names the rest in `not_on_this_mac`, and records the coordinator in `Project.created_by`. |
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
| `append_log` | `input: LogInput` | change | The capture form: one entry logged as the project's coordinator would with the workspace tool `log_work`, in the brain of the project's Space. `LogInput` holds `session_id`, `kind`, `title`, `changed`, `why`, `state` and optionally `milestone`, and `repository` or `ticket_id` (one of the project's tickets, whose folder and `ref` the entry then takes and whose milestone prompts it answers; it stays in the coordinator session's batch); the host mints the entry's id and date and sets its `ref`, author and folder. Refused, storing nothing, when the Space has no brain, gh has no login, or the entry's body is over 2,000 characters. Answers with the `WorkLog`. |
| `logs` | `project_id: String`, `before: Option<i64>`, `limit: usize` | read | The project's entries, newest first. A first page (no `before`) also ends with every held entry older than it, so none is out of the human's reach. An entry with no `batch_id` that is not `in_legacy_file` is unsent: it waits for the Space to name a brain. One with `held` names the rule it breaks as it stands and is never sent until fixed; one `released` is kept and never sent. |
| `fix_held_log` | `log_id: String`, `fix: LogFix` | change, human only | Replaces a held entry's `kind`, `title`, `changed`, `why` and `state`, its folder (`repository`) and its `reference`. Refused, changing nothing, while it still breaks the entry contract or the cap. Once it fits it is sent with its batch, or with its ticket's or session's open batch when its own was already sent. Clears the project's held-entries Inbox item once none is held. Answers with the `WorkLog`. |
| `release_held_log` | `log_id: String` | change, human only | Releases a held entry: it stays in the project's memory and is never sent. Clears the project's held-entries Inbox item once none is held. Answers with the `WorkLog`. |
| `create_space` | `name: String`, `description?: String`, `instructions?: String`, `prefix?: Option<String>`, `storage?: Option<String>` | change, human only | Creates a Space with local storage and a local tracker, or, with `storage`, in that private GitHub repository (`owner/name` or its URL), sharing its tracker; without `prefix`, a new tracker gets the one proposed from its name. Refused for a repository that is a brain here or whose prefix another storage repository here uses. Answers with a `SpaceRecord`. |
| `check_storage_repository` | `repository: String` | read | Checks a GitHub repository as a Space's storage, as `check_settings_repository` does, and answers with `StorageCheck`: the Spaces under its `spaces/`, each marked when already added here, and its tracker's prefix. Writes nothing. |
| `join_spaces` | `repository: String`, `spaces: Vec<JoinSpace>` | change, human only | Adds the ticked Spaces of a GitHub storage repository, with where each one's brain is on this Mac and, in `clones`, the folders here that are clones of its code repositories, which join the workspace's repositories; a repository with no clone named stays missing. Refused for a clone of a repository the Space does not list, and when its prefix is used by another storage repository here, telling the human to rename a local one. Answers with the `SpaceRecord`s added, each noting a model this Mac falls back from. |
| `space_storage` | `space_id: SpaceId` | read, human only | What the Space page's Storage and issues box shows: the tracker's prefix and issue count, who GitHub says can read the storage repository, whether it is also the settings repository, where it is on this Mac, and when it last synced and why it failed. Answers with `StorageStatus`. |
| `set_space_brain` | `space_id: SpaceId`, `path: String` | change, human only | From the Space's page: for a GitHub brain, records where it is cloned on this Mac, refused unless the folder's `origin` is that repository; otherwise sets the Space's brain to a local folder, for every project in it. |
| `set_tracker_prefix` | `space_id: SpaceId`, `prefix: String` | change, human only | Changes the prefix of the Space's storage repository's tracker: a local one's at any time, rewriting its issue IDs and references; a GitHub one's only while it has no issue. Answers with the `TrackerConfig`. |
| `set_up_issues` | `space_id: SpaceId`, `prefix: String` | change, human only | Adds `wiffletree-issues.yaml` to a Space's GitHub storage repository that has none, and pushes only that file. Answers with the `TrackerConfig`. |
| `sync_issues` | `space_id?: Option<SpaceId>` | change | Makes a pass of the Space's tracker, or of every GitHub tracker here, due at once, and reads the human's `gh` login when none is known yet; a local tracker never syncs. Answers with nothing. |
| `issues` | `space_id?: Option<SpaceId>`, `search?: Option<String>` | read | The issues of the Space's tracker, or of every tracker here, from the index, as `IssueListing`s ordered by tracker and ID, each with `unread` when it changed since the human last opened it on this Mac. With `search`, only those whose ID, title, description or comments hold each of its words, in any case. The index first reads again each issue whose folder changed on disk, so one added or edited by hand is listed as it is. An issue whose `issue.md` does not parse is listed by its folder's name with `problem`. |
| `issue` | `id: IssueId` | read | One issue, read from its files: the `Issue`, or its raw `issue.md` with the problem when it does not parse, its comments in time order, its events, its paused files and parts, each part as `issue.md#<part>`, and whether the human follows it. The prefix, unique on this Mac, names the tracker. Answers with an `IssueRecord`. |
| `create_issue` | `space_id: SpaceId`, `issue: NewIssue` | change, human only | Files an issue in the Space's tracker, in `issue.state`, Triage by default, authored by the human's `gh` login: numbered at once in a local tracker, and with a temporary ID such as `CEL-new-qxvb` in a GitHub one until a pass numbers it on top of GitHub's head and pushes it. Refused without a `gh` login. Answers with the `Issue`. |
| `edit_issue` | `issue: Issue` | change, human only | Replaces an issue's fields; its ID, state, author, creation and the front matter fields this release does not know stay as they are, and a changed priority or assignee is appended to `events.jsonl`. Answers with the `Issue`. |
| `comment` | `id: IssueId`, `body: String` | change, human only | Adds the human's comment as `comments/<time>-<login>.md`. Answers with the `Comment`. |
| `move_issue` | `id: IssueId`, `state: IssueState` | change, human only | Moves an issue to one of the seven states, appending a `state` event. Answers with the `Issue`. Like `edit_issue` and `comment`, it is refused while a file it would rewrite in a GitHub tracker's clone was changed outside Wiffletree, until the human settles it. |
| `mark_issue_read` | `id: IssueId` | change, human only | Marks an issue read on this Mac as its files are now, in `issues.sql`, and settles its news in the Inbox for every project at once; nothing is written to its tracker. Answers with nothing. |
| `follow_issue` | `id: IssueId`, `follow: bool` | change, human only | Follows an issue on this Mac, or stops following it, in `issues.sql`; without a choice the human follows the issues they filed, are assigned or commented on. New comments on a followed issue reach the Inbox. Nothing is written to its tracker. Answers with nothing. |
| `issue_people` | `space_id: SpaceId` | read | Who an issue in the Space's tracker can be assigned to: the logins its issues name as author, assignee, commenter or in an event, the storage repository's collaborators where GitHub lets this Mac read them, and the human's login, each with its GitHub profile's name once read. Answers from what is kept; on the host's socket an ask also reads the collaborators and unread names through `gh`, off the host thread, at most once an hour per tracker. Answers with `IssuePerson`s ordered by login. |
| `linear_teams` | `space_id: SpaceId`, `key: LinearKey` | change, human only | Begins an import from Linear into the Space's tracker: keeps the Linear personal API key in a login Keychain item only the host reads, until the import ends or `cancel_linear_import`, and reads the teams it can see through Linear's GraphQL API, off the host thread. Refused without a `gh` login, while an import runs, or for a key that is not letters, digits and underscores. The key is never logged and reaches `curl` on its standard input. Answers with `LinearTeam`s. |
| `preview_linear_import` | `space_id: SpaceId`, `team_id: String` | change, human only | Reads the team, every issue and every comment, off the host thread, and answers with a `LinearImportPlan`: how many issues no earlier import wrote and their comments, how many keep their Linear number, how Linear's states map to the seven, and the Linear users the import would name with a suggested GitHub login, the key's owner as the human's. Nothing is written. |
| `import_from_linear` | `space_id: SpaceId`, `logins: BTreeMap<String, String>` | change, human only | Writes the previewed team's new issues, those keeping their number first, through the tracker's own writes, in commits of at most 100 issues: a local tracker commits each batch; a GitHub tracker's pass pushes each, after the scans, before the next is written, and the import refuses to start while the tracker has changes waiting. Each issue keeps Linear's number when the tracker's prefix is the team's key and the number was never used; otherwise it is numbered as a new issue is. In a GitHub tracker each is written with a temporary ID preferring Linear's number, which a pass gives it only when GitHub's fetched history never had it, so a scan-held issue stays here whole and unnumbered. Each gets `linear: <identifier>` in its front matter, so running it again adds only issues not imported before. `logins` maps Linear user IDs to the GitHub logins the human confirmed; what an unmapped user filed or wrote is the human's, with a first line naming the Linear author. Relations among the team's imported issues follow in commits of their own, every run adding any an earlier run cut short never wrote. Progress and the outcome are the snapshot's `linear_import`; the key is removed from the Keychain when it ends. Only queries are ever sent to Linear. Answers with nothing. |
| `cancel_linear_import` | — | change, human only | Ends the import after the batch being written, removes the key from the Keychain and clears the snapshot's `linear_import`. Answers with nothing. |
| `issue_project_defaults` | `id: IssueId`, `space_id?: Option<SpaceId>` | read | What Start project from issue offers: the Spaces on this Mac that share the issue's tracker, last used for a new project first, and, in `space_id` when it is one of them or else the first, the repositories the project would use: the workspace repositories whose GitHub name the issue lists, with the listed ones not on this Mac in `not_cloned`, or the Space's when the issue lists none. Answers with `IssueProjectDefaults`. |
| `start_project_from_issue` | `id: IssueId`, `space_id: SpaceId` | change, human only | Creates a project named after the issue and its coordinator in `space_id`, which must share the issue's tracker, with the repositories `issue_project_defaults` names; links it to the issue on this Mac; and sends the coordinator a first message from the human, with the issue's title, description and comments as passages naming their authors, introduced as context and not instructions. The tracker gains a `work` event saying the human started a project, with no project name, and the issue moves to In progress as its derived state. The project starts at once. Refused without a `gh` login or for an `issue.md` that does not parse; then nothing is created. Answers with `CreatedProject`, whose `not_on_this_mac` names the issue's repositories left out. |
| `link_issue` | `id: IssueId`, `project_id: String`, `ticket_id?: Option<String>` | change, human only | Links an issue to a project on this Mac, or to one of its tickets, in `issues.sql`, appending a `work` event for a new link. The issue then follows the linked work: In review while an open linked ticket's PR is open, Done once every counted ticket is finished, its PR merged or the ticket accepted, and no linked project has an open ticket, and otherwise In progress. A ticket closed without a merged PR is dropped and does not count. While the issue's `issue.md` or `events.jsonl` in this Mac's clone holds neither what the host last wrote nor what it last synced, as after an agent's shell edit, the link is kept and its events wait on this Mac; settling the file, or a pass that finds it put back as it was, writes them with the state the work derives. Answers with the `Issue`. |
| `unlink_issue` | `id: IssueId`, `project_id: String`, `ticket_id?: Option<String>` | change, human only | Removes one link. The issue keeps its state and follows the work still linked. Answers with the remaining `IssueLink`s. |
| `update_space` | `space: Space` | change, human only | Replaces a Space's definition; its id and storage stay as they are. Answers with a `SpaceRecord`. |
| `remove_space` | `space_id: SpaceId` | change, human only | Refused for the last Space and while an open project belongs to it; its archived projects move to Personal. |
| `move_project_to_space` | `project_id: String`, `space_id: SpaceId` | change, human only | Later entries go to the new Space's brain; earlier ones stay where they are. |
| `new_project_defaults` | `space_id?: Option<SpaceId>` | read | New project's Space, `space_id` or else the one last used for a new project, with the workspace repositories whose GitHub name it lists and the ones it lists that are not cloned here. Answers with `SpaceRepositories`. |
| `set_repository_spaces` | `repository: String`, `space_ids: Vec<SpaceId>` | change, human only | Puts the repository named `owner/name` in exactly these Spaces, editing each Space's repositories and its `space.yaml`, which syncs with the Space; none puts it under No Space. The workspace repository is unchanged. |
| `suggest_repository_space` | `repository: String` | read | The Space listing the most repositories of its GitHub owner, or `null` for No Space. Only a suggestion. |
| `repository_suggestions` | — | read | The upgrade's suggested Space for each repository it found, until confirmed: the Space whose projects use that owner's repositories most, else Personal, found by name in any casing, for the human's own `gh` login (`new_personal` while no Personal exists), else none. It runs no `gh`: the host reads the login once, off the host thread at start, and until that read has found it there is no Personal suggestion. Answers with `Vec<RepositorySuggestion>`. |
| `confirm_repository_suggestions` | `choices: Vec<RepositorySpaces>` | change, human only | Applies each choice as `set_repository_spaces` does, creating Personal as a fresh install does for a choice with `new_personal`, and stops offering the upgrade's suggestions. |
| `brains` | — | read | The brains this Mac's Spaces name, registered once each however many Spaces name it: each with its Spaces, grant, the host's checkout under `<data folder>/repos/github.com/<owner>/<name>/`, `main`'s head, any problem, a hold on writing, its `wiffletree-brain.json` as read from `main` (unread, missing, invalid with why, newer, or read), the checks `main` requires (`required_checks`), which alone count for a batch, the gate checks `main` does not require (auto-merge waits until it requires all seven), the legacy pointer's warning, its most recent batch PR however old, with that batch's stage, and `legacy_import` while `main` holds legacy `logs/<repo>.md` files whose import is not done. Then each batch not yet merged or merged in the last day, with its stage, PR, problem and checks: names, conclusions and the files and lines of their annotations, never their text. Then this host as a compiler, its host id and name. Answers with `BrainsPage`. |
| `suggest_space_brain` | `repositories: Vec<String>` | read | The brain to pre-fill for a Space with these code repositories (`owner/name`): the brain another Space names for their GitHub owner, or `null` for the human's own or another user's repositories. Registers nothing. |
| `confirm_brain` | `repository: String` | change, human only | The grant: the host may push `wiffletree/brain/<login>/<batch>` branches to that brain, open PRs from them into `main`, turn on auto-merge for them, close a PR the human discards and delete a branch it pushed. Refused for a brain no Space names or that is storage or settings here. Nothing is pushed to a brain before it. Answers with the `Brain`. |
| `forget_brain` | `repository: String` | change, human only | Forgets a brain no Space names any longer: its batches here, their worktrees and the host's own checkout. GitHub is untouched. |
| `set_up_brain` | `repository: String` | change, human only | Queues one batch adding `wiffletree-brain.json` with this host as compiler, the brain gate's caller pinned to its release, its `gate` config and allow-list, and `CODEOWNERS` lines; files already on `main` are kept. Its PR waits for the code owner's review: the host never turns on auto-merge for it, and the Brains page, not the Inbox, shows it waiting. Refused while the brain has the file, a Set up PR is open, or the gate has no release. Answers with the `BrainBatch`. |
| `set_brain_compiler` | `repository: String`, `compiler: BrainCompiler` | change, human only | Queues a batch that changes only `compiler` in `wiffletree-brain.json`, keeping keys the host does not know. The compiler stays the one on `main` until it merges. Answers with the `BrainBatch`. |
| `amend_brain_batch` | `batch_id: String`, `amendment: BrainAmendment` | change, human only | For a batch whose PR is open and unmerged: `redact` replaces the value of a reported field line with `[removed: <rule>]`, and `not_sensitive` appends `<!-- gate:allow <rule> <reason> -->` to it, each one commit the next pass pushes without force; `discard` closes the PR and deletes the branch the host pushed. Refused once the batch merged. Answers with the `BrainBatch`. |
| `send_logs_now` | `repository: Option<String>` | change, human only | Send now: closes every open log batch of that brain (`owner/name`), or of every brain, so each is committed as one batch of entry files and opens its PR on the brain's next pass. A batch otherwise closes when its ticket is accepted or closed, its coordinator session is archived, 24 hours after its first entry, or at 50 entries. Answers `{"closed": n}`. |
| `import_legacy_logs` | `repository: String` | change, human only | Import legacy logs: reads the brain's legacy `logs/<repo>.md` files on `main` as the host last fetched it and queues each entry not yet on `main` or in an unfinished batch as one entry file with `source: skill`, in log-only batches of at most 200 files. Ids derive from the repository, date, title and text, so a repeat adds nothing; the author is the GitHub login of the commit that added the entry's heading (`git blame`, then `gh api`), or the human's. An oversized entry is shortened to fit, pointing to the legacy file at that commit, and a `ref` the contract does not allow moves into `state`. Entries at or before a file's `compiled-through` marker are recorded as compiled. Legacy files are never changed. With every entry on `main`, the import is recorded as done and no longer offered. Answers with `LegacyImport` (`added`, `batches`, `compiled`, `done`). |
| `brain_logs` | `repository: String`, `project_id?: Option<String>`, `filter?: BrainLogFilter`, `from?: BrainLogsFrom` | read | The brain browser's Logs, from the brain's index (below): 50 entries newest first by id, filtered by `repository`, `type`, `author` and `verification`, and with `project_id` only entries in that project's repositories' folders. `from` is `newest`, or `before`, `after` or `at` an entry id (`at` is the page that starts with it); `older` and `newer` are the ids to page from. Each entry has its five fields, its `ref`'s anchors and verification on this machine, and the PR that added it: for this machine's entries the batch PR and its live stage, with the project, ticket and session in `origin`; otherwise the PR of the squash commit that added the file, or none for a direct push. The newest page also lists, in `pending`, this machine's entries not yet on `main`. Answers with `BrainLogPage`. |
| `brain_notes` | `repository: String`, `project_id?: Option<String>` | read | The note index tree: `notes/INDEX.md`'s headings as groups, the notes INDEX misses in `not_in_index`, each with its `kind`. With `project_id`, only notes whose `sources` include an entry of the project's repositories or that have no repository-specific source, and `hidden` counts the rest. Answers with `BrainNoteTree`. |
| `brain_note` | `repository: String`, `path: String` | read | One note by its path in the brain folder: its frontmatter, Markdown body, `[[wikilinks]]` (a broken one has no `path`), backlinks from notes and `STATUS.md`, and one claim per footnote with its sources, each an entry with its anchors and verification, or `broken`. `sources_readable` is false when a footnote names no source the host can read. Answers with `BrainNote`. |
| `brain_conflicts` | `repository: String`, `project_id?: Option<String>` | read | The open conflicts of `reconcile-queue.md`, one per `<!-- conflict:<id> -->` block, with its title, rule, claim, conflicts-with, evidence and recommendation, and the claim's entry and repository; with `project_id`, only those in the project's repositories. Answers with `Vec<BrainConflict>`. |
| `brain_status` | `repository: String` | read | `STATUS.md` with its wikilinks, the last compile (when a commit on `main` last changed `notes/` or `STATUS.md`; none is "No compile yet"), the entries commits after it added, and the indexed commit. Answers with `BrainStatus`. |
| `search_brain` | `repository: String`, `project_id?: Option<String>`, `query: String` | read | The first 20 notes and entries matching every term of `query`, the last as a prefix, in the project's scope when given: decisions above observations, then by BM25, each with an excerpt whose matched terms are marked. One brain per call. Answers with `Vec<BrainHit>`. |
| `activity` | `project_id: String`, `before: Option<i64>`, `limit: usize` | read |  |
| `git_history` | `repository_id: String`, `skip: usize`, `limit: usize` | read |  |
| `settings` | — | read | Answered with `HostSettings`. |
| `set_workspaces_dir` | `path: String` | change | Checks the folder can be created and written to, saves it and answers with `HostSettings`. Only projects and tickets created afterwards use it. |
| `set_verification` | `verification: VerificationSettings` | change | Validates and saves the verification setting; answers with `HostSettings`. |
| `set_builds_at_once` | `builds: usize` | change | Validates (1–4) and saves "Builds at once" on this machine; answers with `HostSettings`. |
| `set_check_ins` | `child_turns: u32`, `human_turns: u32` | change | Validates and saves both check-in thresholds; answers with `HostSettings`. |
| `set_owner_allowed` | `entry: String`, `allowed: bool` | change, human only | Allows an owner, or a single fork as `owner/name`, to have PRs opened without asking, or stops allowing it; answers with `HostSettings`. Only the human's client sends it. |
| `set_keeper` | `keeper: KeeperSettings` | change, human only | Validates and saves the PR keeper's settings; answers with `HostSettings`. Only the human's client sends it. |
| `save_skill_rules` | `skill: Skill`, `rules: String` | change | Saves the human's own rules for a skill, an empty text removing them, and refuses more than `MAX_SKILL_TEXT` characters; answers with the `SkillState`. Only the human's client sends it. |
| `reset_skill` | `skill: Skill` | change | Returns a skill to its stock text with no rules of the human's own; answers with the `SkillState`. Only the human's client sends it. |
| `restore_skill` | `skill: Skill`, `entry: String` | change | Makes the state a history entry holds current, as a new entry; answers with the `SkillState`. Only the human's client sends it. |
| `check_settings_repository` | `repository: String` | read | Asks GitHub, read-only, about a repository named as `owner/name` or by its `github.com` URL, and checks this machine can read it; answers with `SettingsRepositoryCheck`. A public repository is refused. Writes nothing. |
| `set_up_settings_repository` | `repository: String` | change, human only | Adds this machine's shared settings to a repository that has none, in one pushed commit, and makes the shared folder its clone; answers with `HostSettings`. Only the human's client sends it. |
| `join_settings_repository` | `repository: String`, `with: JoinWith` | change, human only | Joins a repository that holds shared settings, keeping `with`'s; answers with `HostSettings`. Only the human's client sends it, once the human confirms. |
| `sync_now` | — | change | Runs a sync pass with the settings repository, each storage repository and each brain at once; answers with nothing. Only the human's client sends it. |
| `stop_settings_sync` | — | change | Stops syncing and forgets the settings repository chosen, keeping the shared folder as it is and changing nothing on GitHub; answers with `HostSettings`. Only the human's client sends it, once the human confirms. |
| `settle_paused_file` | `path: String`, `settle: Settle`, `repository?: Option<String>` | change, human only | Settles the paused file, or a skill's folder, a workspace note names; answers with nothing. Only the human's client sends it, and the socket requires the app's credential for an issue's file or part, a write to its tracker. A path such as `issues/CEL-148/issue.md#description` names a paused part of an issue: `keep_this_machines` (Keep mine) leaves this Mac's value for the next pass to push, and `use_the_others` (Use theirs) writes the other side's value here. |
| `compare_paused_file` | `path: String`, `repository?: Option<String>` | read | Read-only: this machine's version of a paused file beside the other one, line by line, the other being the last synced version for an edit made outside Wiffletree. For a skill, its rewrite or else its rules. `repository` is the storage repository a note names, `None` for the settings repository. Answers with lines as `SkillStockChanges` does. |
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
| `set_repository_base` | `repository_id: String`, `base: String` | change | Changes a repository's base in place once it resolves to a commit; answers with the `Repository`. Only the human's client sends it. |
| `set_protected_branches` | `repository_id: String`, `entry: String`, `protected: bool` | change, human only | Adds one entry to a repository's protected branches, refusing it with the reason, or removes one, applied to the host's current list; answers with the `Repository`. A removed entry is a branch the host may then merge into, so only the human's client sends it. The entries stay on this machine. |
| `origin_branches` | `repository_id: String` | read | The branches on the repository's `origin` as last fetched, answered with `Vec<String>`, for the branches each protected pattern matches. |
| `leftover_worktrees` | — | read | Read-only listing of leftover worktrees, answered with `Vec<LeftoverWorktree>`. |
| `remove_leftover_worktrees` | `ticket_ids: Vec<String>` | change | Removes the worktrees of the tickets the human selected from the listing, re-checking each first; answered with `Vec<LeftoverRemoval>`. |
| `processes` | `session_id: String` | read | What the agent has left running, or for a coordinator every agent of its project; answered with `Vec<AgentProcesses>`. Groups that have exited are forgotten. |
| `stop_processes` | `session_id: String`, `groups?: Vec<String>` | change | Stops these groups of the Processes panel of `session_id`, or with none named every group it lists, as the human's stop; answered as `Processes`. The agents keep working. |
| `subscribe` | `after?: Option<Cursor>`, `kinds?: Option<Vec<EventKindName>>` | read | Socket only: turns the connection into a subscriber of the host's events, after the events following `after` when it is still in the host's log, or after `resync` otherwise; answered with `Subscribed`. With no cursor only later events come, after an `attention` event with the current count. `kinds` limits the events to those kinds, and `resync`. Sent again on a subscribed connection without `after`, it changes the `kinds` in place and is answered with `null` (`resubscribe`). |
| `ping` | — | read | Socket only: answered with nothing by the connection itself, without waiting for the host's actor. A client that has heard nothing for a while sends it to tell a quiet or busy host from one that is gone. |
| `resolve_paths` | `references: Vec<String>`, `bases: Vec<PathBuf>` | read | Which `references` name a file, each as it is or, when relative, under the first of `bases` that holds it; a `~/` reference is under the host's home folder. Answered with one `Option<ResolvedPath>` per reference, in order. Reads no file's contents. |
| `read_file` | `path: PathBuf`, `max_bytes: u64` | read | Reads a file under the roots `docs/host-protocol.md` lists, refusing one larger than `max_bytes` or `MAX_ATTACHMENT_BYTES`; answered with `FileContents`. |
| `list_folder` | `path: PathBuf` | read | Names the folders in `path` and which are Git repositories; answered with `FolderListing`, whose `path` is the resolved one. A leading `~` is the host's home folder, except for a View only device. |
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

## The brain index and `query_brain`

The brain reads (`brain_logs`, `brain_notes`, `brain_note`, `brain_conflicts`, `brain_status`, `search_brain`) go through one SQLite FTS5 file per brain, `<data folder>/brain-index/github.com/<owner>/<name>.sqlite`, a projection of `main` in the host's checkout. Each brain pass refreshes it after its fetch: it lists the brain folder at the fetched commit, reads through Git only the files whose blob changed, removes the deleted ones and never reads a symlink. A pass that fetched nothing new does nothing, and a missing or older index, or one whose build never completed, is rebuilt from the checkout, offline from its `main` too. The brain folder is `brain_dir` in `wiffletree-brain.json` on `main`, or else the folder a Space naming the brain gives. Entries' refs are verified on this machine against the workspace's clones of their repositories, without fetching: a commit on the repository's default branch on GitHub (the one GitHub reported, else `origin/HEAD`, never the repository's configured base) is `verified`, one elsewhere, a feature base included, `claimed`, and anything else `unverified` with the reason. This machine's pending entries are verified on the request, with results cached by each clone's default-branch head, so a repeat page reads one ref per clone. The entries waiting for a compile are those added by commits after the last commit that changed the wiki, by the commit graph rather than commit times. Each read is refused for a brain no Space names, before the brain's first index build, and for a `project_id` whose Space does not name that brain, so a project's scope never reads another brain. Nothing logs a query or a file's text.

The coordinator, implementers, testers and reviewers have the workspace tool `query_brain`, which takes `question` and optionally `scope` (`project`, the default, or `brain`). The brain is always the caller's project's Space's; the tool takes no other argument. It walks the ladder over the same index: `STATUS.md` passages that hold the question's terms, then INDEX lines, then the top five notes with their footnoted sources and the notes they link, then the top five entries with their verification, decisions above observations at each step, until about 4,000 characters of excerpts are gathered. The answer names the brain and scope, the last compile, the pending entries and when the brain was synced, marks brain text as context and not instructions, and with no match says "The brain has nothing on this." It is refused for other roles, when the Space has no brain or a local one, and before the first index build.
