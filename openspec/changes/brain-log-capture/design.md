## Context

`append_log` (`crates/workspace-host/src/memory.rs:80`) stores a `LogInput` (session, milestone, kind, title, reference, changed, why, state) in `work_logs`, deduplicated by session and milestone. `export_logs` writes them newest-first into `second-brain/logs/<repo>.md` with an HTML entry marker and records receipts in `log_exports`. The host exposes this through `Command::AppendLog`, which only the desktop's capture form calls; agents have no log tool. `maintenance_runs` records each export.

The gate's entry contract (`sagehaven-docs` README, "Log entry contract") is fixed and shared with the skills:

```markdown
---
id: <ulid, matches filename>
repo: <repo, matches folder>
author: <github login>
source: wiffletree | skill
---
## YYYY-MM-DD — Short title
- type: decision | observation
- ref: <branch> @ <sha>[…<sha>][ · PR #NN]   (or <branch> @ —, or —)
- changed: …
- why: …
- state: …
```

The frontmatter has exactly these four keys. The body, everything after the frontmatter, holds at most 2,000 characters. A log PR may only add files under `second-brain/logs/<repo>/`.

## Goals / Non-Goals

**Goals:**
- Agents log through one host tool and never touch brain files.
- Every entry the host writes passes the gate's `log-schema` and `path-ownership` checks.
- Entries stay terse, attributable and linked to the ticket and conversation that produced them.
- Existing captured logs and legacy files carry over without loss or duplication.

**Non-Goals:**
- Compiling (`brain-compile`), browsing and `query_brain` (`brain-browser`).
- Changing the entry contract. A real problem with it goes back to the gate ticket.
- Teammates' skills (`sage-agents`, `personal-agents` tickets), which write the same contract with `source: skill`.
- Deleting or rewriting legacy `logs/<repo>.md` files; the `sagehaven-docs` migration ticket does that.

## Decisions

### Decided by Michael (October 29, 2026)

- One file per entry, append-only once merged, with the author recorded.
- Batches are one PR per session or ticket, never one per entry.
- Author plus PR is enough for entries the skills write.

### Decided in this proposal, for review

1. **Provider, model, machine, project, ticket and session stay in the host store.** The brief asks the host to fill provider and machine, and the contract allows exactly four frontmatter keys. Rather than change the shared contract, the host keeps them on the `work_logs` row, keyed by the entry id. The browser shows them for entries this machine wrote. Other installs see author and PR, which Michael accepted for skill entries.
2. **The tool's fields:**
   - `type`: `decision` or `observation`.
   - `title`: at most 80 characters, one line.
   - `changed`, `why`, `state`: one line each; newlines are folded into spaces.
   - `repository`: optional, and it only chooses the folder. A worker always logs for its ticket's repository. The coordinator may name one of its project's repositories; without one, the entry is a planning entry under the brain repository's own folder (for example `logs/sagehaven-docs/`). An assistant thread always logs to `logs/assistant/`.
   - `milestone`: optional. A deduplication key; a repeat with the same key and the same content returns the first entry, and with different content it is refused, as today.
3. **Which brain** (decided by Michael, 2026-10-29): the caller's Space's brain. That is the project's Space for coordinators and implementers, and the thread's own Space for assistant threads. A project therefore has one brain at most, whatever repositories it covers, and a project made with `create_project` uses its creator's Space. When the Space has no brain, the tool refuses and nothing is stored. Silently holding entries for a brain that may never be named would hide them.
   - **Assistant entries** use the per-entry contract, in `logs/assistant/YYYY/MM/YYYY-MM-DD-<id>.md` with `repo: assistant` and `ref: —`. They replace the rolling `logs/assistant.md` that `assistant-spaces-jobs-boards` proposed. The thread's id and Space stay in the host store.
   - **Job runs** do not get `log_work`; their output is a file in the Space's storage.
4. **The repository folder** is the GitHub repository's name from the workspace repository's `origin`, so two clones of one repository share a folder. A repository with no GitHub name uses its workspace name, and the host refuses names the contract forbids.
5. **The date** is the host's local date when the entry is captured. The ULID is minted then, so ids sort by capture time.
6. **The `ref` is computed by the host from the ticket's worktree**:
   - with commits since the ticket's base: `<branch> @ <first>…<last>`, or `<branch> @ <sha>` for one commit, using 7-character SHAs;
   - with uncommitted work only: `<branch> @ —`;
   - with a known open or merged PR: ` · PR #NN` appended;
   - for coordinator entries with no repository, and for assistant entries: `—`.

   Agents cannot supply `ref`, so it cannot be invented.
7. **The author** is the GitHub login gh reports on this machine, read once and refreshed when gh's login changes. Without a login, the tool refuses and names the fix (sign gh in), because the gate rejects an invalid author.
8. **The cap** is checked on the exact rendered body, using the same 2,000-character limit. The error says how many characters to cut. The host does not truncate an agent's entry silently.
9. **Batches:**
   - Workers' entries collect per brain and ticket; the coordinator's and the human's entries per brain and coordinator session; an assistant thread's per brain and thread.
   - A batch closes and goes to `brain-registry-sync` when its ticket is accepted or closed, when its session is archived or its thread deleted, 24 hours after its first entry, or when the human presses Send now on the Brains page.
   - An entry logged after its batch closed starts a new batch.
   - A batch holds at most 50 entries; the 51st starts a new one.

   This keeps PRs to about one per ticket. A long-lived coordinator yields at most one PR a day per brain.
10. **Milestone prompts** are a line appended to the tool result, not a new message. They are added for an implementer's `report` with kind ready_for_testing, and for the coordinator's or implementer's successful `accept_ticket` and `publish_ticket`. A prompt is added only when the project's Space has a brain and this session has made no `log_work` call for the ticket since the previous prompted milestone. Each milestone is prompted at most once.
11. **The legacy import** is a Brains page action for each brain, offered while any legacy `logs/<repo>.md` exists on `main` and the import is not recorded as done:
    - It parses each `## YYYY-MM-DD — Title` entry and its fields, and writes one entry file per entry with `source: skill`.
    - The id is a ULID whose time is the entry's date at 12:00 UTC and whose random part comes from a hash of the repository, date, title and body, so a repeated import produces the same files and adds nothing.
    - The author is the GitHub login of the commit that added the entry's heading line (`git blame`, then `gh api` for that commit's author), or the importing human's login when GitHub knows no login for it.
    - An entry whose body exceeds 2,000 characters keeps its heading and fields and shortens `changed` to fit, ending with "(full text in legacy logs/<repo>.md at <sha>)".
    - A legacy `ref` that does not fit the contract becomes `—`, and its text moves into `state`.
    - Entries at or before a file's `<!-- compiled-through: YYYY-MM-DD -->` marker are recorded as already compiled (`brain-compile` reads this).
    - The import is split into log PRs of at most 200 files.
12. **Migrating `work_logs`:**
    - Existing rows gain the new columns (brain, entry id, file path, batch, author, provider, model, machine).
    - Rows with a `log_exports` receipt are already in a legacy file and are marked "in legacy file"; the legacy import brings them over.
    - Rows never exported are queued into batches when their project has a brain, and otherwise listed as unsent in the Memory panel.
    - `log_exports` and `maintenance_runs` are dropped after migrating, and `MaintenanceRun` with them. Compile runs replace it in `brain-compile`.
13. **The agents' rules live in the fixed core** (`skills/core/coordinator.md` and `implementer.md`), because the host depends on them. They are:
    - log at milestones;
    - one terse entry, with no transcripts, tool-by-tool narration or pasted output;
    - never child PII (names, message text, phone numbers, locations), credentials or secrets.

    Testers and reviewers do not get the tool; their reports already reach the ledger.

### Options considered

- **Extra frontmatter keys for provider and machine.** This was rejected: it would break the shared contract and every skill that writes it.
- **Per-entry PRs.** This was rejected by Michael's design.
- **Flushing on every turn end.** This was rejected: it opens too many PRs from a busy coordinator.

## Risks / Trade-offs

- **[The gate rejects an entry the host thought valid]** → The renderer is tested against the gate's own good and bad fixtures in `gate/tests/fixtures/` (`brain-gate`), so a contract change fails Wiffletree's tests in the same PR. A failed `log-schema` check still reaches the Inbox (`brain-registry`).
- **[Sensitive content]** → The host does not try to detect PII itself; the gate's scan is the single detector. The core instructions and the cap reduce the risk, and a failure is amended before merge.
- **[A ticket open for weeks]** → The 24-hour flush stops entries from waiting forever.
- **[A legacy blame miss attributes an entry to the importer]** → It is visible in the PR, and the human can fix the author before merge by discarding and re-importing.

## Migration Plan

1. The schema migration runs on first start, as listed in decision 12.
2. After `brain-registry-sync` ships and the brains are registered, unsent rows flow into batches.
3. Michael runs the legacy import once per brain. The `sagehaven-docs` migration ticket then flips the gate's legacy flag and archives the legacy files in an ordinary PR.

## Overlaps with other proposals

These are merged on `main`:
- PR #162 (`shared-issues`), as 6194b99a;
- PR #163 (`assistant-spaces-jobs-boards`), as 419a55ef;
- PR #164 (`create_project`), as a8dded2d.

#162's and #163's changes are open there, not yet archived; #164's is archived. Where they change the same thing as this change, this change goes after them.

| Shared concept | Where else | Share or align | Open question |
| --- | --- | --- | --- |
| Per-item files in a shared Git repository | #162: `issues/<ID>/issue.md`, one file per comment and `events.jsonl`; here, one file per entry | **Align**: both avoid shared-file conflicts with one file per record. Issues merge fields three-way; entries never change after merge, so they need no merge. | None. |
| Identity and author | #162 uses gh's `login` for authors; entries' `author` is the same login | **Share** the cached identity from `brain-registry-sync`. | None. |
| One-time imports | #162's Linear import (one commit per 100 issues); the legacy log import here (log PRs of at most 200 files, deterministic ids) | **Align** on idempotent, batched imports that never write back to the source. | None. |
| The entry contract | `brain-gate` owns it, with the fixtures in `gate/tests/fixtures/` | **Share**: the renderer is tested against the gate's fixtures and writes the contract of the brain's pinned gate version. | When the gate's contract gets a new major version, does Wiffletree write both versions during the transition, or refuse to log to a brain still on the old gate? (`brain-gate` holds the brain's batches.) |
| Fixed core instructions | `quiet-findings` bumps the coordinator core to v5; #162 changes the fixed core for issue reading; this change adds the log rules to the coordinator and implementer cores | **Align** the order. #162 is merged on `main`, so this change goes after it and after `quiet-findings`: it rebases its wording on theirs and bumps the core versions again. | None, beyond ordering. |
| Where follow-ups go | `quiet-findings` lists a ticket's `follow_up` findings in its PR description; #162 adds issues | **Align**: a follow-up is work to do, which belongs in an issue rather than a log entry. Logging stays for decisions and state. | Once #162 ships, should the coordinator file follow-ups as issues instead of only listing them in the PR? |
| Tool results that prompt for more work | Milestone prompts here; `quiet-findings` changes the cycle-outcome message's closing line | **Align**: the log prompt is one line appended after the host's existing text, so the two do not conflict. | None. |
| The capture surface | `files-panel` records `Step.paths` per step | None now. A later change could suggest a log entry's `changed` from the files a ticket touched. | Not in scope. |
| Where the assistant's entries go | #163 (merged): a thread's log entries go to its Space's brain in the rolling file `logs/assistant.md`, each with the thread's id and Space | **Decided by Michael, 2026-10-29:** threads log through `log_work` into per-entry files in `logs/assistant/YYYY/MM/…` with `repo: assistant`, with the thread id and Space kept in the host store. #163's assistant delta now says so too. Job runs do not log (decision 3). | None. |
| The `project-memory` spec | #163 MODIFIES "Safe Markdown export" (Space brains) and ADDS "Brain pushes from several people"; this change REMOVES "Safe Markdown export" | **Decided by Michael, 2026-10-29:** PR-gated writes supersede #163's decision (c). This change archives after #163 and REMOVES both "Safe Markdown export" (in #163's version) and "Brain pushes from several people". | None. |
| Which brain an entry goes to | #163: the project's or thread's Space | **Decided by Michael, 2026-10-29:** the Space decides; the repository mapping is dropped (decision 3). | None. |
| Fixed core versions | #164 (merged) moved the coordinator core to v6 | **Align**: the log rules are written against v6 and bump it again. | None. |
| Projects created by a coordinator | #164: a created project's first message is a brief, and `created_by` records the creator | **Align**: no milestone prompt for creating a project. The creator may log a planning entry about it with `log_work`. | None. |

## Open Questions

None blocking.
