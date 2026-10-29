## ADDED Requirements

### Requirement: One compiler per brain
A host SHALL compile a brain only when `wiffletree-brain.json` on that brain's `main` names this host's id as the compiler. Every other install SHALL NOT start compile runs for that brain, and its Brains page SHALL show which host compiles it. A brain SHALL have at most one compile run in progress and at most one open compile PR.

#### Scenario: Another install
- **WHEN** RB's install registers `sagehaven-docs`, whose config names Michael's Mac
- **THEN** RB's install never starts a compile run for it, and its Brains page shows "Compiles on: Michael's Mac"

#### Scenario: A compile PR is open
- **WHEN** the nightly run is due while the brain's compile PR #212 is still open
- **THEN** no run starts, and the run is recorded as waiting for PR #212

### Requirement: Compile runs start on schedule, on demand and after a ruling
The compiler host SHALL start a run each day at the config's compile time in the config's timezone. After sleep, downtime or a quit that missed one or more due times, it SHALL start exactly one catch-up run. Due times SHALL be computed in the config's timezone, so a daylight-saving change never yields two runs for one date. The human SHALL be able to start a run with Run now on the Brains page or the brain's Status view, which SHALL say why when a run is in progress or a compile PR is open. The compiler host SHALL start a run when a fetched `main` adds an entry whose `state` begins `Ruling on conflict`.

#### Scenario: Asleep at 2:00
- **WHEN** Michael's Mac sleeps from 1:00 to 7:00 and the compile time is 02:00
- **THEN** one run starts on wake, and none is started twice for that date

#### Scenario: A ruling merges
- **WHEN** a ruling entry from Oscar's install merges on `main` of `sagehaven-docs`
- **THEN** Michael's Mac starts a compile run on its next fetch

### Requirement: Pending entries are found by an entry-ID cursor
The cursor SHALL be stored in `notes/INDEX.md`'s frontmatter as `compiled-through` (a commit on `main`) and `compiled-after` (the ids of entries added after that commit that are already compiled). Pending entries SHALL be the entry files added under `logs/<repo>/` between `compiled-through` and `main`'s head, minus the ids in `compiled-after`. On a brain with no cursor, every entry file SHALL be pending except those the legacy import recorded as compiled. A run SHALL process at most 200 pending entries, oldest id first. Its compile PR SHALL set the cursor, written by the host: when no pending entry remains up to the run's head commit, `compiled-through` becomes that commit and `compiled-after` is emptied; otherwise the processed ids are added to `compiled-after`. The cursor SHALL change only when the compile PR merges.

#### Scenario: Same-day entries
- **WHEN** two entries dated October 29 merge, one before and one after a run
- **THEN** the first is compiled by that run and the second by the next one, and neither is skipped

#### Scenario: A batch that merges late
- **WHEN** a batch opened on October 27 merges on October 29, after a run whose `compiled-through` is October 28's head
- **THEN** its entries are pending in the next run

#### Scenario: A failed run
- **WHEN** a run's compile PR fails a check and is discarded
- **THEN** the cursor on `main` is unchanged and the next run finds the same pending entries

### Requirement: An idle run makes no model call
A run that finds no pending entries SHALL make no model call and no write, and SHALL be recorded as skipped. A run repeated with unchanged inputs SHALL write nothing.

#### Scenario: Nothing pending
- **WHEN** the nightly run of `vyttle-docs` finds no pending entries
- **THEN** no model starts, no branch is pushed, and the run is recorded "no pending entries · no model call"

### Requirement: Refs are verified against the workspace's repositories
Before any model call, the host SHALL verify each pending entry's `ref` against the workspace repositories whose GitHub name matches the entry's repository folder, fetching each such repository at most once per run. An entry SHALL be:
- **verified** when a named commit is reachable from the repository's default branch or the named PR is merged;
- **claimed** when a named commit exists but is not on the default branch, or the named PR is open;
- **unverified** when the `ref` is `—` or `<branch> @ —`, no named anchor is found, or the repository is not on this machine.

A release tag containing a verified commit SHALL be recorded as further evidence. The same verification SHALL be used wherever verification is shown.

#### Scenario: An open PR
- **WHEN** an entry's ref is `wiffletree/retry-telnyx-timeout @ 7d03b1e · PR #92`, the commit exists only on that branch and PR #92 is open
- **THEN** the entry is claimed

#### Scenario: A repository not on this machine
- **WHEN** an entry names a repository that no workspace repository matches
- **THEN** the entry is unverified, with the reason "not on this machine"

### Requirement: Entries fold or queue under the compile procedure
The compile model SHALL run under the bundled compile procedure, whose version SHALL be recorded with the run. Its input SHALL be bounded:
- the procedure;
- `STATUS.md` and `notes/INDEX.md`;
- the pending entries with their verification states;
- the notes the entries name or link to, up to 60,000 characters.

The procedure SHALL queue an entry, with `queue_conflict`, only when it contradicts a `kind: decision` note, when two authors disagree about the same fact, or when it is destructive or deprecating. It SHALL fold every other entry into the notes and `STATUS.md`. It SHALL NOT record a claimed or unverified entry as completed fact, and SHALL NOT change a decision note except from a verified decision entry that does not contradict it, or from a ruling. Notes SHALL keep `title`, `kind`, `updated` and `sources` frontmatter, with `sources` listing entry ids. Each claim in a note SHALL cite its entries with a Markdown footnote, whose definition links to the entry files by their paths relative to the note, so that the brain browser can list sources per claim and GitHub renders the links. A reversed decision SHALL become an observation marked superseded, never deleted. `STATUS.md` SHALL stay around 500 words.

#### Scenario: Additive observation
- **WHEN** a verified observation adds that the relay's health check moved to `/healthz`
- **THEN** it is folded into the relay's note with its entry id in `sources` and a footnote on the claim linking to the entry file, and nothing is queued

#### Scenario: Contradicting a decision
- **WHEN** a claimed entry says retries cap at 3 and the decision note says 5
- **THEN** the note keeps 5, and one conflict is queued with the claim, the decision, the evidence and a recommended ruling

### Requirement: The compile session can only change the wiki
The compile model SHALL run as a host-owned session in a worktree of the brain checkout, on a compile batch branch. It SHALL have `queue_conflict` and `finish_compile` and no other write tool, and SHALL have a 30-minute timeout. Before anything is pushed, the host SHALL check:
- only `notes/**`, `STATUS.md` and `reconcile-queue.md` under the brain folder changed;
- every note has its frontmatter;
- every `[[wikilink]]` resolves;
- every source id and every footnote names an existing entry file.

The host SHALL then write the cursor and `reconcile-queue.md` itself. A failed check, timeout or crash SHALL end the run as failed, with no PR, the cursor unchanged and the reason recorded.

#### Scenario: A change outside the wiki
- **WHEN** the compile session edits a file under `logs/`
- **THEN** the run fails before pushing, records the path, and the cursor is unchanged

### Requirement: Conflicts are Inbox cards with a ruling
The host SHALL record each queued conflict with a stable id, the rule it hit, the claim's entry id, what it conflicts with, evidence lines and a recommended ruling with one line of reasoning. It SHALL export the open conflicts to `reconcile-queue.md` in the compile PR, one block each, marked `<!-- conflict:<id> -->`. Every install SHALL show the conflicts from `reconcile-queue.md` on `main`:
- as a card in the Inbox of each project in a Space that names the brain whose repositories include the claim's repository;
- on the Brains page when no project matches;
- in the brain browser's Conflicts view.

A card SHALL offer Uphold existing, Keep new, Keep both and Defer, mark the recommended one, and link to the evidence.

#### Scenario: A card in two projects
- **WHEN** a conflict's claim is in `sagechat-call-watch-relay` and two projects include that repository
- **THEN** both projects' Inboxes show the card, and a ruling in one clears it from both

### Requirement: A ruling writes a decision entry and starts a compile
Only the human SHALL rule. Rulings SHALL write a `decision` entry:
- **Uphold existing:** an entry restating the established decision.
- **Keep new:** an entry adopting the claim; the next compile marks the old decision note superseded.
- **Keep both:** an entry from the human's one-line scoping of both.

Each entry's author SHALL be the human, its repository the claim's, and its `state` SHALL begin `Ruling on conflict <id>:`. The host SHALL open the ruling's batch PR at once. **Defer** SHALL write no entry and hide the card until the next compile, which raises it again if it still holds. Merging a ruling entry SHALL start a compile on the compiler host.

#### Scenario: Keep new
- **WHEN** the human rules "Keep new" on the retry-cap conflict
- **THEN** a decision entry beginning `Ruling on conflict <id>:` opens its PR at once, and after it merges the compiler starts a run that marks the 5-retry decision superseded

#### Scenario: Defer
- **WHEN** the human rules "Defer"
- **THEN** no entry is written, the card disappears, and the next compile raises it again if the claim still contradicts the decision

### Requirement: Compile runs are recorded and their cost is visible
The host SHALL record each run with its id, trigger, start and end, pending count, processed count, model calls, the model used, the procedure version, the outcome (skipped, waiting, failed with reason, or PR opened), its PR and its usage. The Brains page SHALL show each brain's last and next run, and the run history SHALL be paginated. A run's model usage SHALL be recorded in the usage ledger under the brain, shown as "Brain compile · <brain>" in usage reports.

#### Scenario: Run history
- **WHEN** the human opens a brain's run history after three runs (skipped, failed, PR opened)
- **THEN** each shows its trigger, counts, outcome and, for the third, its PR and usage
