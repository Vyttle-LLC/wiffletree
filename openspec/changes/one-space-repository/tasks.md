Each group is one ticket, in this order, in `Vyttle-LLC/wiffletree`. The app stays usable after each. `design.md` gives the reason for each mechanism; numbers such as "decision 3" point there.

## 1. The folder

A Space names a repository and a folder, the sync engine works inside the folder, and the one-role rule goes.

- [ ] 1.1 `workspace-core`: `SpaceStorage.root` with a serde default, empty for a repository not yet moved, and the rule for a valid folder.
- [ ] 1.2 `workspace-host`: one `Root` type that joins and strips the prefix. `space_file`, `TRACKER`, `ISSUES`, `scan::scanned`, `recipe_path`, `issue_of`, `has_issues`, and the storage rule sets' roots all go through it. An empty root gives today's paths exactly (decision 3).
- [ ] 1.3 Rule sets: keep `spaces/`, boards and issues inside the folder, add `compilers/` to the Space rule set, and keep the settings rule set at the root. Fetch a repository holding a Space every minute, with or without a tracker.
- [ ] 1.4 `issues_sync`: numbering reads history at the folder and, before the move, at the root.
- [ ] 1.5 `storage::stage`: find the folder by listing the head's tree, skip stubs, and refuse two folders or a folder that is someone else's.
- [ ] 1.6 The move, local: at first start, one commit, with no question, finished after an interrupted start.
- [ ] 1.7 The move, GitHub: `repos::move_into_folder`, one commit with its stubs, its own check that every change is a 100% rename of a claimed path or a stub, and its own loop of up to 5 tries.
- [ ] 1.8 Adopting a move in a pass: read the tree again, move record paths and unpushed files under the folder, and compare as usual.
- [ ] 1.9 Drop `storage::Use` and every refusal between a brain, storage and settings, and the `A brain as storage` checks in the Space and settings flows.
- [ ] 1.10 `wiffletree-issues.yaml`: read and write `compile` and `allowed_email_domains`, keeping unknown keys, and add `compilers/<host id>.yaml` as the one file each Mac writes.
- [ ] 1.11 Space-side tests: two Spaces share one folder; a file outside the folder is pulled and never committed; a settings edit commits at the root beside an issue in the folder; a repository with an empty root behaves exactly as today.
- [ ] 1.12 Finding tests: one folder, a custom nested folder, two folders refused, stubs skipped, a root layout joined as it stands.
- [ ] 1.13 Move tests: local at first start and not again; an interrupted move finished; two Macs moving at once make one commit; an unpushed comment survives adoption; settings stay at the root; a deleted issue's number from before the move is not reused.
- [ ] 1.14 Older host tests: against the release's previous `read_space` and `read_tracker` behaviour, a stub parses and pauses with "Update to use it", not "can't be read", and a `space.yaml` this release writes at the root keeps `instructions` and `format` 1.

## 2. Straight in

Log entries, compiles and rulings go through the sync engine. Batches, auto-merge and the gate's setup go.

- [ ] 2.1 Logs rule set: add-only, pushing 2 seconds after the last change with no comparing, in commits that only add entry files.
- [ ] 2.2 `memory.rs`: `log_work` and the capture form write an entry into the clone and hand it to the engine. Remove batching. Refuse only for a GitHub Space with no `gh` login. Use this Mac's account name as `author` in a local repository. Write planning entries under `planning`.
- [ ] 2.3 `memory.rs`: milestone prompts for every Space; refusals for "no brain" go.
- [ ] 2.4 `scan.rs`: `scanned` covers the whole folder, `Held` gains its line, and the scratch repository holds a generated `wiffletree-brain.json` naming only `allowed_email_domains`.
- [ ] 2.5 Held entries: a state that survives restarts, and the card on the writing project or the Brains page, offering Edit it, Not sensitive and Discard, and never showing the matched text.
- [ ] 2.6 Rulings: `rule_conflict` writes an entry through the same path. Stop writing `ruling_triggers`, and remove `ruling_merged`.
- [ ] 2.7 `brain_index`, the Status, Logs and Memory reads: read `<folder>/second-brain` from the repository's clone, and replace batch and PR stages with held and waiting.
- [ ] 2.8 Compile stays on its one host for now, reading the folder's `second-brain` from the clone. Replace its batch with the wiki rule set's landing in 3.4.
- [ ] 2.9 Upgrade: a Space naming a brain keeps its `brain` key as read, no screen edits it, and nothing resolves through it. Queued batches land in the folder. Pull requests left open are named once on the Brains page. `compile`, `allowed_email_domains` and the compiler carry over once from `wiffletree-brain.json` when its clone is here. `work_logs` rows land. Every table stays, unwritten, and `user_version` does not change (decision 4).
- [ ] 2.10 Remove from `brains.rs`: batches, publishing, PR watching, batch Inbox items, amending, Set up, Update gate, the compiler and compile-time PRs, the separate checkout and its marker, the legacy pointer, and `suggest_space_brain`, with their commands and `workspace-core` types (decision 9).
- [ ] 2.11 Remove from `gate/`: `action.yml`, `checks/log_schema.py`, `checks/markdown_lint.py`, `checks/path_ownership.py`, `checks/wikilinks.py`, and the two markdownlint config files, with their tests. Keep what `scan.rs` bundles, its tests and `tests/fixtures/entries/`. Point `.github/workflows/brain-gate-tests.yml` at the tests directly.
- [ ] 2.12 Move the contract's rules from `log_schema.py` into the writer's test in `memory.rs`, so every fixture is still checked.
- [ ] 2.13 Landing tests: one entry lands in seconds; a burst is one commit; a push refused because `main` moved fetches and lands; offline waits and lands; a local Space commits and pushes nothing.
- [ ] 2.14 Held entry tests: a phone number is held and never pushed, others keep landing, no text reaches a card or record, Not sensitive lands it with its marker, Edit rescans, Discard pushes nothing, and it survives a restart. A check that cannot run holds with what is missing.
- [ ] 2.15 Upgrade tests, on stores of every schema the repository's fixtures cover: a store opens; an earlier build's checks still accept its `user_version`; **a Space with a separate brain and storage** keeps its `brain` key, logs to its folder and never writes to the old brain; a queued batch lands; an open pull request is named once; a Space with no brain gains logs and a wiki.
- [ ] 2.16 Run `python3 gate/tests/test_gate.py` for what stays.

## 3. Volunteer compile

Claims, the on-this-Mac choice, checking claims through GitHub, product pages, and links to tickets.

- [ ] 3.1 The on-this-Mac choice: `compilers/<host id>.yaml` written and removed by that Mac, as human-only commands, and the compile job registered for each wiki this Mac compiles, keyed by repository, on the scheduler.
- [ ] 3.2 `repos::claim`: the empty-tree commit, one plain push, the `ls-remote` read, no fetch, merge or retry. `read_tip` for its state, including expiry. `pass` never touches the branch (decision 1).
- [ ] 3.3 The run: a pass, then pending, then the model, then the tip; owed Macs read the tip after each pull; `landed` and `released` tried once; the tip read again before landing. Local repositories skip all of it.
- [ ] 3.4 Wiki rule set and landing: exact files staged on the current head, scanned, retried with the pass, all or nothing, with `at_or_past` and `changed_meanwhile` kept. Remove `open_compile_batch` and `compile_waits`' PR states (decision 2).
- [ ] 3.5 `entry_paths`: pending by id across layouts.
- [ ] 3.6 `verify_refs`: `Clones` gains a GitHub-only entry from the Spaces' repository lists, asking `gh` for the default branch, commit and compare, with `pull_state` kept, a run's count of what could not be asked, and the same path for the index (decision 6).
- [ ] 3.7 Compile procedure v2: products, `repos:`, keeping frontmatter, issue IDs, and `PROCEDURE_VERSION` 2 (decision 10).
- [ ] 3.8 `check_changes`: a changed note keeps its `repos:` names, and only ULID-shaped sources are checked.
- [ ] 3.9 `brain_markdown`: issue links by the tracker's prefix and known IDs, with `link_target` opening the issue. The local prefix rename rewrites `notes/` and `STATUS.md`.
- [ ] 3.10 Run records: lost to another Mac, compiled elsewhere, landed with its commit, and usage shown by Space.
- [ ] 3.11 Falling behind: no volunteer with entries waiting, or two scheduled times with no landed run.
- [ ] 3.12 **Two Macs claiming at once** test, against a real bare repository: two distinct commits raced from one tip, and two first claims with no branch at one `main` commit. Exactly one wins, and the loser makes no second push, fetch or merge.
- [ ] 3.13 A test that a refused claim is never retried, by counting pushes, and that a landed push with a lost answer reads as won.
- [ ] 3.14 **A crashed claim** test: a claim 59 minutes old is live, and one 61 minutes old is taken over as its child with no force; two takeovers race to one winner; an owed Mac takes it at its next pull; a Mac that slept through its claim lands nothing.
- [ ] 3.15 Quota tests: out of quota before the claim claims nothing; mid-run releases and the next owed Mac runs.
- [ ] 3.16 Idle test: nothing pending pushes nothing, not even a claim, and makes no model call.
- [ ] 3.17 **A push refused during compile** tests: logs land during a run, the retry adds no model call, the commit lands on the new head with the cursor from the run's commit, and those logs are pending next run.
- [ ] 3.18 Landing failures: a cursor already past lands nothing and each entry compiles once; a hand edit fails the run and stands; a scan failure lands nothing; the cursor is unchanged in each.
- [ ] 3.19 GitHub ref tests, with a fake `gh`: on the default branch, on a side branch, 404, a merged PR, an open PR, a 403 giving unverified and counted, two owners of one name, a short SHA, and a folder no Space lists asking nothing.
- [ ] 3.20 Product tests: entries fold by `repos:`, a dropped `repos:` name fails the run, and an older note gains `title` and `kind` and keeps its keys. Issue link tests: a known ID links; another prefix, no such issue, code and an existing link do not.

## 4. Alerts

Each Mac scans what it pulls, and raises the Inbox card.

- [ ] 4.1 `scan_marks` and `scan_alerts`, created if missing. Marks start at the head at join and at upgrade, and restart at the merge base after a rewrite.
- [ ] 4.2 The pull scan: off the pass, commit by commit against the first parent through `Scanner::scan`, inside the folder, skipping binary files and pure renames, with each check's mark moving as it goes (decision 5).
- [ ] 4.3 The alert: one row per repository, path, check and rule; the workspace card in each project of the folder's Spaces and on the Brains page; who pushed it, with GitHub's web editor named; Open the file, Not sensitive and How to clean history.
- [ ] 4.4 Clearing: a whole-file scan at the head after a pull changes the file, and Not sensitive recorded here with a reason, writing nothing to the repository.
- [ ] 4.5 A check that cannot run: its mark stays, one workspace note, and it catches up.
- [ ] 4.6 **A slipped scan** tests: an edit made outside Wiffletree raises one card, which names the rule and never the text; added and removed between two pulls still alerts; a pushed `.gitleaks.toml` does not silence it; a file outside the folder raises nothing.
- [ ] 4.7 Once-per-Mac tests: three commits, one card; two Macs each raise their own; removal closes each at its own pull; Not sensitive closes here only and survives an unrelated edit; a new finding in the file raises anew.
- [ ] 4.8 Mark tests: a quit mid-range resumes; a joining Mac scans no old history; a rewrite restarts at the merge base; 900 commits do not delay a pass.
- [ ] 4.9 gitleaks tests: missing, the pull completes, `sensitive-info` runs, one note; arriving, the note closes and `secret-scan` catches up.

## 5. Setup screens

New Space, repository checks, the Space page, Bring in a brain, joining, and the Brains page, as `ui/mockups.html` draws them.

- [ ] 5.1 New Space: the one form, Create with `gh` as private, a repository that already holds a folder fixing its folder, prefix and time, and compile on this Mac by default.
- [ ] 5.2 Repository checks: private and who can read it, direct push read from rulesets and branch protection, and the warning for history.
- [ ] 5.3 Protect history: the ruleset through `gh`, GitHub's refusal shown, and the warning kept.
- [ ] 5.4 The Space page: one Repository box, the four Brain rows gone, and Sync now, Import from Linear…, Bring in a brain…, Move… and Move into `wiffletree/`.
- [ ] 5.5 Bring in a brain, reader: `legacy_logs.rs` reads a folder on this Mac, `blame --contents` with `GIT_OPTIONAL_LOCKS=0`, the pointer for an uncommitted copy, and no write there (decision 7).
- [ ] 5.6 Bring in a brain, landing: entries byte for byte or derived, pages byte for byte, skipped pages listed, the cursor in `compiled-after`, commits of at most 200 files, scanned, and the `brain` key and old checkout dropped at the end.
- [ ] 5.7 Bring in a brain, form: what it brings, what it leaves, shortened entries, and the warning for files not on GitHub, filled in from a Space's `brain` key.
- [ ] 5.8 Joining: one repository, the folder found, Spaces with counts, code repositories only, and Also on this Mac off by default.
- [ ] 5.9 The Brains page: groups by wiki, Macs with this Mac's tick, Change, Run now, the no-volunteer banner, and what has no project to go to. Remove the PR gate list, Set up, Update gate and the warnings for the old skills.
- [ ] 5.10 Check tests: a public repository, a repository requiring pull requests, a usable one with history unprotected, and a repository of other documents.
- [ ] 5.11 Bring in tests, on a repository with changed, untracked and never-committed files: all land; `git status`, the index and every file in the original are byte-identical after; again adds nothing; `repos:` survives; a held entry lands nothing twice; already-compiled entries read as compiled on a second Mac.
- [ ] 5.12 Run the app on a seeded store, dark and light, and compare each screen with `ui/mockups.html`.

## 6. Words and docs

The agents' instructions, README and specs.

- [ ] 6.1 Fixed core: `implementer.md`, `coordinator.md` and `communication.md` lose the batch and pull request wording, with new versions.
- [ ] 6.2 Stock skills and tool descriptions: `log_work`'s and `query_brain`'s text in `mcp.rs`, and any line naming a retired skill.
- [ ] 6.3 Remove the "save this as a note" idea wherever Wiffletree's text carries it.
- [ ] 6.4 `README.md`, `docs/FEATURES.md`, `docs/PRD.md`, `docs/DEVELOPMENT.md`, `docs/host-protocol.md`, the safety section's brain-branch line, and `gate/README.md`, cut to what remains. Say what the model's accepted risks are: anyone with write access can change anything, a slip stays in history, every issue change is a commit, no volunteer means no wiki updates, and Protect history needs an admin.
- [ ] 6.5 Add "How to clean history", which the alert's button opens.
- [ ] 6.6 Add a line to `docs/DEVELOPMENT.md` naming the tables a later release can drop.
- [ ] 6.7 A test that no fixed core or stock text names a retired skill or says an entry reaches the brain in a pull request.
- [ ] 6.8 Run `openspec validate --all --strict`, archive `choose-space-repositories` first, then this change, and update each changed spec's Purpose by hand.

## Not in these tickets

- The "Move a Space's storage" ticket in Wiffletree: Spaces drops its brain refusal and carries the folder (see `design.md`, Dependencies).
- Moving Vyttle and Sage, and turning off the safety-net task, are done by hand after this ships.
- Michael's dotfiles, `personal-agents` and `vyttle-docs` need the changes `design.md` lists. Commit the 17 files in `vyttle-docs/second-brain/` before anything else.
