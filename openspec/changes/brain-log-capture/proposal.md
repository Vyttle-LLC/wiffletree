## Why

Agents write today's work logs by editing `second-brain/logs/<repo>.md` directly through the `brain-log` skill. Wiffletree's own capture (`append_log` and `export_logs` in `crates/workspace-host/src/memory.rs`) prepends to the same rolling files. When several machines and people prepend to one shared file, Git conflicts are guaranteed. Entries also drift from terse: the latest `simpatico-sage` entry is a multi-thousand-word transcript.

Michael decided on October 29, 2026 that brains are team-shared and that every change reaches `main` through a PR. The brain gate (built in `sagehaven-docs` PR #1, moving into Wiffletree in `brain-gate`) defines a per-entry file contract and a 2,000-character cap. Wiffletree has to write that contract, through a host tool, in batches that `brain-registry-sync` turns into PRs.

## What Changes

- **A `log_work` MCP tool** for the project coordinator, implementers and assistant threads.
  - The brain is always the caller's Space's brain (Michael, 2026-10-29), so a project never spans brains.
  - The agent gives only the type, a title and the `changed`, `why` and `state` text. The host fills in the rest:
    - the entry's id and date;
    - the repository folder and `ref`, from the ticket's worktree: branch, commits since the base, and PR;
    - the author (the human's GitHub login) and `source: wiffletree`;
    - the provider, model, machine, Space, project, ticket, session and thread, which stay in the host's store because the entry contract allows exactly four frontmatter keys.
- **Assistant thread entries use the same contract**, in `logs/assistant/YYYY/MM/…` with `repo: assistant`, instead of the rolling `logs/assistant.md` that `assistant-spaces-jobs-boards` proposed.
- **PR-gated writes supersede #163's decision (c)** (Michael, 2026-10-29). This change removes #163's `project-memory` requirement "Brain pushes from several people".
- **The entry contract from the gate, exactly.** One file per entry at `second-brain/logs/<repo>/YYYY/MM/YYYY-MM-DD-<ulid>.md`, never edited after merge. Its `ref` is `—`, `<branch> @ —` or `<branch> @ <sha>[…<sha>][ · PR #NN]`. The host refuses an entry whose body exceeds 2,000 characters, saying how much to cut.
- **Milestone prompts.** When an implementer reports ready_for_testing, or the coordinator accepts or publishes a ticket, the tool result asks for one `log_work` call if the session has not logged since the last milestone and the project's Space has a brain.
- **Batching.** Entries collect into one batch per brain and ticket, per brain and coordinator session, or per brain and assistant thread. A batch opens its PR when its ticket is accepted or closed, when its session is archived, after 24 hours, or on Send now. It is never one PR per entry.
- **A one-time legacy import** per brain. It converts `logs/<repo>.md` entries into per-entry files in log-only PRs, with deterministic ids so a repeated import adds nothing. Entries already compiled are recorded as compiled. Legacy files are left for the migration ticket.
- **Migration of the existing capture code.** `work_logs` gains the new fields. Rows never exported are queued into batches. `export_logs`, `log_exports`, `maintenance_runs` and the legacy renderer are removed. The human's capture form in the Memory panel writes through the same path as `log_work`.
- **Agents are told the rules.** The coordinator and implementer core instructions say when to log, how tersely, and never to log child PII, secrets or transcripts.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `project-memory`: capture moves to `log_work` and per-entry files in batches. The Markdown export, the export-only maintenance and #163's "Brain pushes from several people" requirements are removed. Milestone prompts, batching, the legacy import and the agent rules are added.

## Impact

- `crates/workspace-host/src/memory.rs`: `append_log` becomes the shared capture path behind `log_work` and the capture form. `export_logs`, `export_batch`, `render_log`, `insert_entries` and the lock file are removed. A new per-entry renderer and the legacy importer are added.
- `crates/workspace-host/src/mcp.rs`: the `log_work` tool for the coordinator and implementer roles.
- `crates/workspace-host/src/live.rs` and `publish.rs`: milestone prompts in the results of `report` (ready_for_testing), `accept_ticket` and `publish_ticket`.
- `crates/workspace-host/src/schema.sql` and a migration: new `work_logs` columns. `log_exports` and `maintenance_runs` are dropped after migrating.
- `crates/workspace-host/skills/core/coordinator.md` and `implementer.md`: the log rules, with a version bump.
- `crates/workspace-desktop/src/memory_view.rs`: the capture form writes through the new path, and the export button is removed.
- Batches are handed to `brain-registry-sync`'s batch flow; the gate's `log-schema` and `path-ownership` checks must pass on every log PR.
