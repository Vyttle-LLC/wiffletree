## Context

`model.md` and `ui/mockups.html` are the source of truth, approved by Michael on November 1, 2026. Nothing below reopens a decision in them. This file settles the mechanisms they leave open and gives a reason for each.

What exists today, in brief:

- `repos.rs` runs one sync engine per repository: `pass` fetches `main`, lets each `RuleSet` compare, commits each rule set apart and pushes. A push refused because `main` moved fetches, merges and tries again.
- `storage.rs` holds a Space's files at the clone's root, with three rule sets: Spaces, boards and issues.
- `scan.rs` writes five gate files to the data folder and runs `sensitive-info` and `secret-scan` in a scratch repository of two commits, reading only what it is given.
- `brains.rs` keeps a separate checkout and sends every change as a batch pull request. `compile.rs` runs on the one host `wiffletree-brain.json` names and writes its cursor into `notes/INDEX.md`.
- `verify_refs.rs` answers from local clones only, and `legacy_logs.rs` reads a brain's `main` through Git.

## Goals / Non-Goals

**Goals:** one repository, one folder and one way to write; every Space has issues, logs and a wiki; compiling needs no chosen host; nothing written before this release is lost.

**Non-Goals:** preventing an edit made outside Wiffletree, which the model accepts as a risk; moving the settings repository; rewriting Git history after a slip; any change to Michael's dotfiles, `personal-agents` or `vyttle-docs`; moving Vyttle or Sage, which are done by hand after this ships.

## Decisions

### 1. The claim is a chain of commits on one branch

Claims live on one branch, `wiffletree-compile-claim`, in the Space's repository. A repository holds one folder, so it holds one wiki and one claim.

- **A claim is a commit with an empty tree** whose message holds JSON: `host`, `machine`, `login`, `run`, `due`, `state` and `at`. `state` is `claimed`, `landed` or `released`. `due` names the scheduled time it is for, such as `2026-11-02T06:00 America/Chicago`, or `run-now:<run id>`.
- **Its parent is the branch's tip**, or it has none for the first claim. Each claim therefore replaces the one before it, expired or not.
- **Claiming is one plain push**, `<sha>:refs/heads/wiffletree-compile-claim`, never forced.

How the two traps are avoided:

- **Two Macs at one commit.** Two pushes of the same commit to a new branch both succeed, because the second has nothing to do. A claim holds this Mac's host and run ids, so no two Macs ever push the same commit. Two children of one tip are never a fast-forward of each other, and two first claims race for the ref's creation, so Git accepts exactly one.
- **Fetch, merge, retry.** A claim does not go through `repos::pass`. A new function, `repos::claim`, pushes once and never merges, and its `Moved` path is "someone else won". The engine only ever fetches and pushes `main`, so it never sees this branch.

After the push, `git ls-remote` reads the tip whatever Git answered. The tip being this commit means the claim was won, which also covers a push that landed when its answer was lost. Any other tip means it was lost. No answer at all means this Mac could not reach GitHub, and it claims nothing.

**Expiry needs no deletion or force.** The tip is free when its state is `released` or `landed`, or when it is `claimed` and its `at` is more than an hour old. Taking over an expired claim is an ordinary child of that claim, so two Macs taking over at once race the same way. The branch only grows, by about two tiny commits a day, and needs no permission beyond pushing. Expiry compares the claim's `at` with this Mac's clock. Macs keep good time, and a few minutes either way does not matter against an hour.

**How a due run goes, and who it stays owed to.**

1. At the due time a volunteer runs a pass, so `main` is fresh, then works out what is pending. Nothing pending ends the run with no claim, commit or model call.
2. It resolves the Brain compiler row. A Mac whose row waits, out of quota, ends the run as skipped and claims nothing.
3. It reads the tip. `landed` with this `due` means another Mac did the work. A live claim means it stays owed. Otherwise it claims.
4. A Mac that lost, or found a live claim, stays owed for that `due`. It reads the tip after each one-minute pull until the tip says `landed` for that `due`, the claim frees, or nothing is pending. That is how the next Mac takes a run a crashed Mac or an empty quota left, and it is one `ls-remote` a minute only while a run is owed.
5. A run that ends without landing pushes `released`, so the next Mac need not wait out the hour. One that lands pushes `landed`. Either push is tried once: if it fails, the hour covers it.

**A Mac that wakes after losing its claim.** A compile session stops at 30 minutes, so a live run ends well inside the hour. A Mac that slept through the hour may wake holding an expired claim. Before landing it reads the tip, and a tip that is not its own claim ends the run with nothing pushed. If a takeover slips between that read and the push, the bookmark stops double processing (see 2).

**Local Spaces take no claim.** They have one Mac and no remote.

**Why a branch, not a commit on `main` or a custom ref.** A claim on `main` could not tell "someone claimed" from "a log entry landed", because both refuse a push, and it would add a commit to `main` for a run that does nothing. A ref outside `refs/heads/` is hidden, but GitHub's handling of them is less certain, and a branch is known to work on every plan.

### 2. Compile lands through the engine, and the cursor stays where it was computed

The claim replaces the one-open-compile-PR rule: a live claim is what stops a second run, and there is no state between "running" and "on `main`". `compile_waits`, `open_compile_batch` and the "waiting for PR" outcome go.

A run reads `main` at commit `H0`, compiles in a worktree at `H0`, passes the existing checks, and the host writes the cursor and `reconcile-queue.md` as it does today. The cursor is computed from `H0`: `compiled-through` is `H0` when nothing stays pending up to `H0`, and otherwise the processed ids join `compiled-after`.

Landing is a fourth kind of rule set, **wiki**, over `notes/`, `STATUS.md` and `reconcile-queue.md`. It hands the run's exact files to the next pass, which stages them on the current head, scans them and pushes.

**A push refused because logs landed during the run** is the ordinary case. The storage pass already fetches and goes again whole, up to `issues_sync::PUSH_TRIES`, and no model is involved, because logs and issues never touch the wiki's files. The commit lands on the new head with the cursor still naming `H0`. Entries that landed after `H0` are after `compiled-through`, so they are pending next run. That is right, and it needs no extra code.

**What ends a landing instead:**

- **`main` already holds a cursor at or past the run's** (`at_or_past`, kept). Another Mac compiled these entries after a takeover. The run ends as "already compiled", lands nothing and releases. The bookmark stops double processing, as the model says.
- **A wiki file changed on `main` since `H0`** (`changed_meanwhile`, kept), such as a hand edit on GitHub. The run ends failed, lands nothing and releases, and the next run recompiles from the new wiki. Overwriting the edit would lose it, and a merge of prose is what the compile model is for.
- **A file fails the scan.** A landing is all or nothing: every wiki file is held from this push, the run ends failed naming the file and rule, and the run's output is discarded. The cursor did not move, so the next run sees the same entries.

The cursor moves only when the commit that holds it lands, as it did only when a PR merged.

**Pending is worked out by id, not by a diff.** `entry_paths` lists added paths in `through..head`. The move into the folder renames every entry, so a cursor from before the move would make all of them pending. Pending becomes the entry ids at `head` minus the ids at `through`, each tree listed under its own layout, minus `compiled-after`.

### 3. Paths: a prefix, found by reading the tree

**One field.** `SpaceStorage.root` holds the folder, `wiffletree` by default and empty for a repository not yet moved. `SpaceStorage.folder` keeps its meaning, the Space's own name under `spaces/`. `space.yaml` never names either, as today it never names its storage. Rule sets take paths through one `Root` type that joins and strips the prefix, so `claims`, `scanned`, `space_file`, `TRACKER`, `ISSUES` and the compile paths stop being bare constants. An empty root gives today's paths exactly, so an unmoved repository behaves as it does now.

The rule sets become Spaces, boards, issues, logs and wiki. Logs are add-only by one Mac, so they push without comparing, like a runner's boards. The wiki compares whole files.

**A joining Mac reads the tree.** It clones to staging, as today, lists the head's tree, and looks for `<folder>/spaces/<space>/space.yaml` and `<folder>/wiffletree-issues.yaml`, skipping forwarding stubs (see 4). One folder found is the folder. None means an empty repository, which is offered as new. Two means the repository is refused, naming both, because each repository holds one. There is no marker file at the root, because anything outside the folder is left alone. A root-level layout found this way is joined as it stands, with an empty root.

**Validity.** A folder is a relative path of one or more plain names, with no `.`, `..` or hidden part. A repository whose folder exists and holds no Wiffletree files is refused, because the folder must be Wiffletree's.

**Moving an existing repository once.** A local repository moves at first start, with no question, because nothing else reads it. A GitHub repository moves when the human presses Move into `wiffletree/` on the Space's page. This is the one write an upgrade would make to a shared repository, `spaces` says an upgrade pushes nothing, and it stops any teammate on an older host, so a person should choose the time. Until then the Space works at the root, with logs and the wiki under `second-brain/` there.

The move is one commit, with its stubs. It renames every path under the rule sets to the folder and changes no content. It is the one commit that spans rule sets, so it has its own check and not `repos::push`'s: every change is a rename at 100% similarity of a claimed path, or a stub. Its push is one attempt inside a loop of its own: when refused, it fetches, and if the root layout is gone the move is done; otherwise it redoes the move on the new head, up to 5 tries.

Every other Mac on this release adopts the move in its next pass: a recorded empty root and a head with no root layout means reading the tree again, moving this Mac's record paths and any unpushed files under the folder, then comparing as usual. Unpushed changes survive because their paths move with the record. A commit that only renames adds nothing new, so the pulled scan skips its renames.

### 4. Upgrade and older hosts

**The store.** Every change is additive: new tables with `CREATE TABLE IF NOT EXISTS`, and `SpaceStorage.root` with a serde default. `user_version` stays, so `ensure!(version <= 10)` still passes and an older build still opens the store. No table is dropped this release. `brains`, `brain_batches`, `legacy_imports` and `legacy_compiled` stay as records the upgrade reads, and no command writes them. A later release drops them. This matches how `flatten`, `spaces::migrate` and `repositories::migrate_to_added` work.

**A Space naming a separate brain and storage.** Nothing moves by itself, and nothing is lost:

- From the first start, its new entries land in its own folder. Its old brain is never written to again.
- `space.yaml`'s `brain` key stays as it was read, as a record of where the old brain is. `Space.brain` is no longer a setting: no screen edits it and nothing resolves through it. The Space's page shows the Repository box with Bring in a brain…, filled in from it.
- A batch queued but not pushed at upgrade re-lands: its entries go through the logs rule set to the Space's folder. A batch with a pull request open is left on GitHub and no longer watched. The Brains page names each such pull request once. If it merges later, Bring in brings its entries, and ids stop any double.
- A completed Bring in drops the `brain` key in the same pass, and deletes the host's own old checkout unless another Space here still names it.

**The `brain` key and older hosts.** A Space created on this release has no `brain` key, which an older host reads as "no brain": it refuses `log_work` and writes nothing anywhere. Every other key keeps its name, `instructions` included, and `format` stays 1 for every `space.yaml` in a folder. Keys this release does not know keep their place, as `extra` does today.

**Older hosts at the move.** An older host's rule sets treat a remote deletion as a pull, so a bare rename would empty its Spaces and let it file issues at a root no one reads. The existing fence is `format`: a pulled file with an unknown one is paused with "written by a newer Wiffletree. Update to use it." So the move commit leaves two forwarding stubs at the old paths:

- `spaces/<space>/space.yaml`, for each Space: `format: 2`, `id`, `name`, `moved_to: <folder>`.
- `wiffletree-issues.yaml`: `format: 2`, `prefix`, `moved_to: <folder>`.

`id`, `name` and `prefix` are there because an older host parses a file before it checks `format`, and a stub without them would read as damaged and not as newer. An older host pauses both, keeps its last good copies and tells its human to update. Its local issue files are removed by the pull, but they are all safe in the folder. It may still file an issue at the root until it updates. That issue stays on GitHub outside the folder, and this release leaves it alone. The stubs stay, because a teammate may update months later. They are outside the folder, and this one commit is the only write that ever touches them. The folder finder skips any file with `moved_to`.

**`wiffletree-brain.json`.** It is read once, from the brain's clone, at upgrade or Bring in, and never written or deleted, because it stays with the old brain. Three things carry over:

- `compile` becomes `compile` in `wiffletree-issues.yaml`.
- `gate.allowed_email_domains` becomes `allowed_email_domains` there.
- A Mac named as `compiler` becomes a volunteer.

The rest ends with it: the other `gate` keys, the compiler's name and `version`.

**Where those settings live.** The compile time and the allowed domains are facts of one folder, and Spaces in a folder share them, so they go in `wiffletree-issues.yaml`, which already holds what a folder's Spaces share and keeps keys older hosts do not know. That adds no file to the model's tree. Who volunteers cannot live there: a whole-file rule set would pause two Macs turning compiling on at once. Each volunteer gets one file, `compilers/<host id>.yaml`, holding its machine name and login. Only that Mac writes it, so it can never conflict. "Last ran Tuesday" comes from the claim chain, so the file holds no time.

### 5. Scanning what is pulled

**Which commits.** Every commit between this Mac's mark and the new head, one at a time, oldest first, including this Mac's own, so nothing rests on trusting who wrote it. Each commit is scanned against its first parent, using the existing `Scanner::scan(clone, parent, files)` over the files it changed inside the folder. This is why it runs commit by commit: a tree diff across a whole range would miss a phone number added and then removed in the same minute, which still reached GitHub. Scanning in a scratch repository, as `scan.rs` does, also means a `.gitleaks.toml` or allow-list someone pushed cannot silence it.

**Marks.** `scan_marks(repository, check, commit)` holds one mark per check and moves commit by commit, so a quit resumes. A joining Mac, and every repository at upgrade, starts its marks at the head then, because the scan looks forward: a first scan of old history would alert on every old file at once. A mark that is no longer an ancestor of the head, after a rewrite, restarts at the merge base.

**Off the pass.** The scan runs as its own off-thread step after a pass moved the head. A Mac back from a month away never delays its pulls.

**Scope.** Inside the folder only. Files outside it are not Wiffletree's, and alerts on them would be noise.

**Once per Mac.** `scan_alerts` has one row per repository, path, check and rule, with the line, commit and who pushed it. A second finding of the same rule in the same file updates the row, and a teammate's Mac raises its own. The card sits at the top of every project's Inbox in the Spaces using the folder, as a workspace note, and on the Brains page. It names the check, rule, file and line, and never the matched text. "Pushed from GitHub's web editor" is shown when the commit's committer is `GitHub <noreply@github.com>`, and otherwise the author's name.

**Clearing.** After any pull that changes an alert's file, the file at the head is scanned whole. When the rule no longer fires there, the alert closes by itself on each Mac, and the card has already said the old text stays in history. **Not sensitive** takes a reason and closes the alert on this Mac only, keeping the row so the same path, rule and blob never alert here again. It writes nothing to the repository: an entry is never changed, and a shared allow-list is a way to silence a scan.

**gitleaks missing.** The pre-push rule stays, so a file is held, naming what is missing. A pull cannot be held, because the file is already on `main`, so the check that cannot run keeps its mark where it is, and one workspace note says what is missing and that what is pulled is not being scanned for secrets. `sensitive-info` carries on with its own mark. When gitleaks arrives the note closes and `secret-scan` catches up from its mark, so nothing was skipped. A commit whose scan fails for any other reason is treated the same.

**Binary files.** Board outputs can be images or PDFs, which the checks cannot read as UTF-8. The pulled scan skips a file Git calls binary rather than reporting "cannot run" forever. The pre-push rule for them stays as it is.

### 6. Checking refs through GitHub

`Clones` gains a second kind of entry. For a log folder with no local clone, the name is looked up in the `repositories` of the Spaces in this folder, by repository name, to give `owner/name`. A name listed under two owners tries each, and the first that holds the commit answers.

For that repository, `verify_refs` asks `gh`:

- `repos/<owner>/<name>` for the default branch.
- `repos/<owner>/<name>/commits/<sha>` to expand a short SHA.
- `repos/<owner>/<name>/compare/<default>...<sha>`, where `behind` or `identical` means on the default branch and `ahead` or `diverged` means elsewhere.

PRs use the existing `pull_state`. States stay verified, claimed and unverified, so the wiki's rules do not change. A 404 means no such commit. A refused or unreachable GitHub gives unverified, with the reason "GitHub could not be asked", and the run records how many entries that covers, so a Mac whose login cannot read a repository does not quietly weaken the wiki. Release tags stay evidence from local clones only, because no cheap API answers "which tag contains this commit".

Each commit is asked about once per call, and a run holds at most 200 entries, so a run makes at most a few hundred requests. The brain index uses the same path under its existing cap (`RECHECKED_PRS`), with verified and closed answers kept, so the browser and compile read the same. A log folder that matches no listed repository, such as `assistant` or `planning`, stays unverified with the reason "no repository of that name", as today's "not on this machine" did.

### 7. Bring in a brain

`legacy_logs.rs` is reused and not replaced. `parse`, `heading`, `legacy_id`, `fitted`, `blame` and `commit_login` stay, and what changes is where the text comes from and where the entries go.

- **The source is a folder on this Mac**, not a commit. `LegacyRead` reads the clone's working files with `fs`, so uncommitted and untracked files count. Symbolic links are refused. `archive/`, `automation/` and `raw/` are not read. A `.git` is not required.
- **Read only.** Nothing writes to the source. Its Git commands are `blame` and `log`, run with `GIT_OPTIONAL_LOCKS=0` so not even the index is refreshed. No fetch, stash, add or commit happens there.
- **Authors.** A tracked file uses `git blame --porcelain --contents <file>`, so a changed line is attributed to no commit. Any entry with no commit behind it takes the importing human's login, as today.
- **The pointer.** `fitted` names the commit, or "this Mac's uncommitted copy", and the form says how many entries were shortened and that their full text stays in the original.
- **Per-entry files** under `logs/<repo>/YYYY/MM/` that meet the contract are copied byte for byte with their ids. A legacy `logs/<repo>.md` becomes entry files with its derived ids, as today.
- **Wiki pages** under `notes/`, `STATUS.md` and `reconcile-queue.md` are copied byte for byte, so `repos:` and every other key survive. A page that already exists with other text is left and listed as skipped. The old cursor lines are removed from `notes/INDEX.md`, because their commits do not exist here.
- **The cursor.** Entries the source had compiled, by its cursor or legacy `compiled-through` marker, go in `compiled-after` in the folder's `notes/INDEX.md`, with `compiled-through` set to the head before the first bring-in commit. The rest are pending. Keeping this in the file, not in `legacy_compiled`, means every Mac reads the same bookmark. The next compile that empties pending empties the list.
- **Landing.** Files go through the logs and wiki rule sets, in commits of at most 200 files, each scanned. A file that fails is held with an Inbox card. Running it again adds nothing, since ids and page checks make it idempotent, so it also picks up entries an older host logged to the old brain afterwards.

`import_legacy_logs`, `offers_legacy_import` and the Brains page offer go, replaced by the Space page's button. `LegacyImport` batches go with the batch flow.

### 8. The settings repository stays as it is

It keeps `wiffletree.json`, `settings/` and `skills/` at the repository's root, whether or not the repository also holds a folder. Nothing in the model forces it to move, and moving it would touch every Mac's shared folder. Two things change in `settings-repository`: the refusal of a repository that is a brain goes with the one-role rule, and a line says settings stay at the root.

So when one repository is both, the settings rule set commits at the root and the Space's rule sets inside the folder. "Anything outside the folder is left alone" is a statement about a Space. The settings repository is a separate choice the human made, and it writes only those three paths.

### 9. What is removed, and what stays

**From the code:**

- In `brains.rs`: batches, publishing, PR watching, Inbox items for batches, amending, Set up, Update gate, the compiler and compile-time PRs, the separate checkout, its `wiffletree-managed` marker, the legacy pointer's warnings and `suggest_space_brain`. What stays moves toward the Brains page's read model: the list of wikis, the index refresh and `new_ulid`.
- In `compile.rs`: `open_compile_batch`, `compile_waits`, ruling triggers and `compiles_here`.
- In `memory.rs`: batching.
- In `workspace-core`: `BrainBatch`, `BrainStage`, the gate types and their commands.

**From `gate/`:** `action.yml`, `checks/log_schema.py`, `checks/markdown_lint.py`, `checks/path_ownership.py`, `checks/wikilinks.py`, `config/markdownlint-cli2.jsonc`, `config/markdownlint-formatter.cjs`, and their tests and fixtures. `.github/workflows/brain-gate-tests.yml` stays, renamed for what it runs, and calls the tests directly.

**What stays because `scan.rs` bundles it:** `checks/common.py`, `checks/gate_config.py`, `checks/sensitive_scan.py`, `checks/secret_scan.py` and `config/gitleaks.toml`. Their tests stay. So do `tests/fixtures/entries/`, which `memory.rs` tests the log writer against, with the contract's rules moved from `log_schema.py` into that test. `gate_config.py` reads `wiffletree-brain.json` from its working folder. The scratch repository holds a generated one naming only the folder's `allowed_email_domains`, so the scripts do not change. `gate/README.md` is cut down to what remains.

Brains that pin the action by commit keep working, because Git keeps those commits. Nothing fetches the gate from GitHub.

### 10. Product pages and issue links

**The compile procedure, v2.** Vyttle's real notes are per product, with frontmatter of `created`, `updated`, `repos`, `sources` and `tags`, no `title` or `kind`, and sources that are file paths. Today's `check_changes` would fail the first compile that touched one. The procedure gains:

- **One page per product**, `notes/<Product>.md`, with `repos:` listing the repository names it covers. An entry folds into the page whose `repos:` lists its folder.
- **A repository on no page** joins an existing product's page when its entries say it belongs there, and otherwise gets a page of its own with `repos: [<name>]`.
- **Keep every frontmatter key.** A note from before this procedure gains `title` and `kind`, `observation` unless it records a decision, the first time it is changed. `repos:`, `created`, `tags` and any others are kept.
- **Issues are named, not summarized.** An ID an entry names, such as VYT-42, is written as plain text, never as a link and never invented. No issue is input, and the wiki does not record an issue's state, because the issue holds that.

**The host's checks follow.** `check_changes` gains one rule: a changed note keeps every name its `repos:` had. Its source check applies only to ULID-shaped sources, so old paths stay valid until a page is rewritten.

**The viewer.** `brain_markdown::prepare` gains the wiki's tracker prefix and the set of issue IDs from its tracker. In text nodes, never in code or inside a link, it turns each whole-word `<PREFIX>-<n>` naming an issue that exists into a link, by the same word rule as `rename_ids`, and `link_target` opens it on the Issues page. An ID of another prefix, or with no issue, stays plain text. A local tracker's rename rewrites IDs in `notes/` and `STATUS.md` too, and leaves log entries alone. The project scope also shows a note whose `repos:` names one of the project's repositories.

### Decisions on points the model left open

- **Rulings.** The model says the next compile picks a ruling up, so a ruling no longer starts a run. `ruling_triggers` stops being written. Every volunteer starting a run on the same pull would need a claim for no gain.
- **Falling behind.** The wiki is falling behind when no Mac volunteers and entries wait, or when two scheduled times have passed with entries waiting and no run landed. Two scheduled times is the rule stale Boards already use.
- **Allowed email domains.** On-Mac scans today use no allowed domains, and logs would start holding every staff address Sage's gate allowed. One key in `wiffletree-issues.yaml` carries them, and nothing else of a brain's allow-list comes across. Inline markers remain the way to allow one line.
- **An author without `gh`.** The entry contract asks for a login. A local Space needs no GitHub, so with no `gh` login its author is the Mac's account name, and an entry for a GitHub repository still needs `gh`.
- **Planning entries.** A coordinator's planning entry went in a folder named after the brain repository. A local repository has no such name, so these go in `planning`.
- **The branch name `main`.** The model says `main`. The engine pushes the recorded default branch, so a repository whose default is another name works the same, and the checks read that branch.

### Dependencies

- **The "Move a Space's storage" ticket** in Wiffletree: Spaces refuses a repository that is a brain. That rule goes. Move must carry the folder, as `<folder>/`, with its logs, wiki and `compilers/`, and set `SpaceStorage.root` on the destination. Move then becomes how Vyttle reaches GitHub. Nothing here edits that work.
- **`choose-space-repositories`**, whose code is merged, has an open delta on `spaces`. "Only the user creates or edits a Space" and "What stays on this machine" here carry its wording, so archive it first.
- **Michael's dotfiles and `personal-agents`, after Vyttle runs:** remove `brain-log`, `brain-query`, `brain-compile` and `brain-reconcile`, the log nudge hook, the scheduled prune and safety-net tasks, and the `~/.config/claude-second-brain` pointer, and edit the Vyttle instructions that name them. `brain-journal` stays.
- **`vyttle-docs`, before anything else:** commit the 17 changed or untracked files in `second-brain/`, as `model.md` asks. Bring in is built so they would survive anyway, but one disk is one disk.
- **`sagehaven-docs`, when Sage moves:** close PR #1. Sage's repository is still to be chosen.

### Capabilities left out

Each was read on `origin/main`.

- **`model-routing`.** "The Brain compiler's row" still holds: each volunteer resolves its own Mac's row, and the waiting and fallback rules are unchanged. What a run does when the row waits is in `brain-compile`.
- **`assistant-router`.** It names a brain only as where entries stay, which remains true of a Space's brain.
- **`boards`.** Paths under `spaces/` become relative to the folder through `shared-repositories`, and nothing else differs.
- **`ticket-verification`.** Its one "batch" is a batch of messages. **`host-process`** has no brain, compile or storage requirement.
- **`workspace-host`'s build gate** is unrelated; only "Enforced hierarchy" changes.
- **`coordination-migration`, `coordinator-updates`, `files-panel`, `leftover-worktree-cleanup`, `native-workspace`, `pr-keeper`, `pr-watch`, `processes-panel`, `protected-branches`, `remote-access`, `remote-client`, `repository-owners`, `ticket-merging`, `ticket-publishing`, `ticket-workspaces`, `usage-cost`, `work-progress`, `work-stream`.** None names a brain, a batch or a storage repository. `pr-watch` is only cited by the removed brain watch's backoff rule.

## Risks / Trade-offs

- **A teammate on an older host stops at the move** → they get "Update to use it" and keep their last good Space. An issue they file before updating sits outside the folder, unread.
- **Clock skew moves expiry** → a few minutes against an hour, and the bookmark stops double processing if a takeover is early.
- **A landing lost to a wiki hand edit wastes a run's model time** → rare, and the alternative is overwriting a person's edit.
- **A claim branch is a second thing that can be refused** by an organization's ruleset on branch creation → the first claim fails with GitHub's reason on the Brains page. Compiling stops and nothing else does.
- **A Mac whose `gh` login can't read a code repository leaves entries unverified** → the run says how many, and the model's rule still keeps them from being stated as fact.
- **`Not sensitive` is per Mac**, so each teammate may see an alert once → this is what "once per Mac" costs, and it writes nothing shared that could silence a scan.
- **The model's accepted risks stand:** anyone with write access can change anything, and a slip stays in Git history.

## Migration Plan

Each group in `tasks.md` ships as one ticket, in order, and the app stays usable between them.

1. **The folder** ships with brains still working, and an unmoved repository behaves as it does today.
2. **Straight in** is the switch: from it on, a Space's writes go to its folder. It carries the upgrade of a Space naming a separate brain.
3. **Volunteer compile**, then **alerts**, then **setup screens**, then **words and docs**.

Rollback: an older build still opens the store. A repository already moved stays moved, and the older build sees the stubs and says to update, so rolling back a moved repository means updating again. Nothing that moved is deleted.

After this ships, by hand: Vyttle creates `Vyttle-LLC/vyttle-wiffletree-storage`, moves its Space there, brings in `vyttle-docs/second-brain/`, and imports from Linear. Sage follows once Vyttle has run for a while. Personal stays on this Mac and gains logs and a wiki.

## Open Questions

None. Every mechanism above is chosen, and no approved decision had to change.
