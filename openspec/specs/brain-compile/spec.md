# brain-compile Specification

## Purpose
Turn a wiki's pending log entries into its notes without a human running a skill: Macs volunteer to compile, the first awake at the wiki's compile time claims the run, it verifies entries' refs through clones or GitHub, folds what is safe into a page per product, queues conflicts as Inbox cards the human rules on, and lands the result straight on `main`.

## Requirements

### Requirement: An idle run makes no model call
A run that finds no pending entries SHALL take no claim and make no model call, commit or write, and SHALL be recorded as skipped, unless the tip says another Mac landed this due time. A run repeated with unchanged inputs SHALL write nothing.

#### Scenario: Nothing pending
- **WHEN** the morning's run of Vyttle's wiki finds no pending entries
- **THEN** no model starts, nothing is pushed, not even a claim, and the run is recorded "no pending entries · no model call"

### Requirement: Entries fold or queue under the compile procedure
The compile model SHALL run under the bundled compile procedure, whose version SHALL be recorded with the run. Its input SHALL be bounded:
- the procedure;
- `STATUS.md` and `notes/INDEX.md`;
- the pending entries with their verification states;
- the notes the entries name or link to, up to 60,000 characters.

No issue SHALL be in its input. The procedure SHALL queue an entry, with `queue_conflict`, only when it contradicts a `kind: decision` note, when two authors disagree about the same fact, or when it is destructive or deprecating. It SHALL fold every other entry into the notes and `STATUS.md`. It SHALL NOT record a claimed or unverified entry as completed fact, and SHALL NOT change a decision note except from a verified decision entry that does not contradict it, or from a ruling. Notes SHALL keep `title`, `kind`, `updated` and `sources` frontmatter, with `sources` listing entry ids, and SHALL keep every other key they hold; a note from before this procedure SHALL gain `title` and `kind` the first time it changes, and sources that are not entry ids SHALL stay. There SHALL be one page per product, `notes/<Product>.md`, with `repos:` listing the repositories it covers: an entry SHALL fold into the page whose `repos:` lists its repository folder, and a repository on no page SHALL join a product's page when its entries say it belongs there, or else get a page of its own. An issue ID an entry names, such as VYT-42, SHALL be written as plain text, never as a link and never invented, and no issue's state SHALL be recorded. Each claim in a note SHALL cite its entries with a Markdown footnote, whose definition links to the entry files by their paths relative to the note, so that the brain browser can list sources per claim and GitHub renders the links. A reversed decision SHALL become an observation marked superseded, never deleted. `STATUS.md` SHALL stay around 500 words.

#### Scenario: Additive observation
- **WHEN** a verified observation adds that the relay's health check moved to `/healthz`
- **THEN** it is folded into the relay's note with its entry id in `sources` and a footnote on the claim linking to the entry file, and nothing is queued

#### Scenario: Contradicting a decision
- **WHEN** a claimed entry says retries cap at 3 and the decision note says 5
- **THEN** the note keeps 5, and one conflict is queued with the claim, the decision, the evidence and a recommended ruling

#### Scenario: A product's page
- **WHEN** an entry for `pica-api` is pending and `notes/Pica.md` has `repos: [pica-ios, pica-api]`
- **THEN** it folds into `notes/Pica.md`, whose `repos:` stay as they were

#### Scenario: A new repository
- **WHEN** entries arrive for `stockpot-api`, which no page lists and no entry ties to a product
- **THEN** the compile creates `notes/Stockpot Api.md` with `repos: [stockpot-api]`

#### Scenario: An issue named in an entry
- **WHEN** an entry says "fixed the relay crash from VYT-42"
- **THEN** the note says VYT-42 in plain text and does not say whether it is open or done

#### Scenario: A note from an older brain
- **WHEN** a compile changes `notes/Bramble.md`, with `created`, `repos`, `sources: [vyttle-overview.md]` and `tags` and no `title` or `kind`
- **THEN** it gains `title` and `kind`, keeps those four keys and their values, and adds its new entry ids to `sources`

### Requirement: The compile session can only change the wiki
The compile model SHALL run as a session owned by its wiki (see `workspace-host`, "Enforced hierarchy"), in no project or Space and with no hidden project record, in a worktree of the repository's clone at the commit the run read. It SHALL have `queue_conflict` and `finish_compile` and no other write tool, and SHALL have a 30-minute timeout. Before anything is pushed, the host SHALL check:
- only `notes/**` and `STATUS.md` under the brain folder changed;
- every changed note has its frontmatter;
- every changed note keeps each name its `repos:` had;
- every `[[wikilink]]` in a changed file resolves;
- every source id and every footnote in a changed file names an existing entry file, a source that is not shaped as an entry id being left alone.

The host SHALL then write the cursor and `reconcile-queue.md` itself. A failed check, timeout or crash SHALL end the run as failed, with nothing landed, the cursor unchanged, the claim released when the Mac can still push, and the reason recorded.

#### Scenario: A change outside the wiki
- **WHEN** the compile session edits a file under `logs/`
- **THEN** the run fails before pushing, records the path, and the cursor is unchanged

#### Scenario: A dropped repository
- **WHEN** a compile rewrites `notes/Pica.md` without `pica-studio` in its `repos:`
- **THEN** the run fails before pushing, naming the note and repository

### Requirement: Conflicts are Inbox cards with a ruling
The host SHALL record each queued conflict with a stable id, the rule it hit, the claim's entry id, what it conflicts with, evidence lines and a recommended ruling with one line of reasoning. It SHALL export the open conflicts to `reconcile-queue.md` in the run's commit, one block each, marked `<!-- conflict:<id> -->`. Every install SHALL show the conflicts from `reconcile-queue.md` on `main`:
- as a card in the Inbox of each project in a Space using the folder whose repositories include the claim's repository;
- on the Brains page when no project matches;
- in the brain browser's Conflicts view.

A card SHALL offer Uphold existing, Keep new, Keep both and Defer, mark the recommended one, and link to the evidence.

#### Scenario: A card in two projects
- **WHEN** a conflict's claim is in `sagechat-call-watch-relay` and two projects include that repository
- **THEN** both projects' Inboxes show the card, and a ruling in one clears it from both

### Requirement: Compile runs are recorded and their cost is visible
The host SHALL record each run with its id, trigger, start and end, pending count, processed count, how many entries GitHub could not be asked about, model calls, the model used, the procedure version, the outcome (skipped, waiting, lost to another Mac, compiled elsewhere, failed with reason, or landed), its commit and its usage. A run lost or compiled elsewhere SHALL name the other Mac. The Brains page SHALL show each wiki's last and next run, and the run history SHALL be paginated. A run's model usage SHALL be recorded in the usage ledger under the wiki, shown as "Brain compile · <Space>" in usage reports.

#### Scenario: Run history
- **WHEN** the human opens a wiki's run history after three runs (skipped, failed, landed)
- **THEN** each shows its trigger, counts, outcome and, for the third, its commit and usage

#### Scenario: A run another Mac did
- **WHEN** this Mac wakes with nothing pending, and the tip says a teammate's Mac landed today's run
- **THEN** the history shows one row saying it compiled on that Mac, not a skip, with no usage on this Mac

### Requirement: Volunteer Macs compile
A host SHALL compile a wiki only when this Mac has compiling on for it (see `brain-registry`, "Who compiles a wiki"). Every volunteer SHALL be due at the set time, and the first awake SHALL claim the run. A wiki SHALL have at most one compile run in progress. A wiki in a local repository SHALL compile on its one Mac and SHALL take no claim. Each Mac SHALL use its own Brain compiler row and its own model subscription.

#### Scenario: Two volunteers
- **WHEN** Michael's Mac and a teammate's Mac both compile Vyttle's wiki at 06:00, and the teammate's Mac is asleep
- **THEN** Michael's Mac claims and runs it, and the teammate's Mac, waking at 09:00, reads that the run landed and starts nothing

#### Scenario: A Mac that does not volunteer
- **WHEN** a teammate has compiling off
- **THEN** that Mac never starts a run, and its Brains page shows which Macs compile

### Requirement: Compile runs start on schedule and on demand
Brain compile SHALL be a built-in host job of the scheduler (see `scheduled-jobs`, "Built-in host jobs"), owned by its wiki, with one job per wiki this Mac compiles, and SHALL take its due times, catch-up, Run now and run records from that scheduler. It SHALL be due each day at the folder's compile time in that timezone. A wiki with no compile time SHALL never be due and SHALL compile only on Run now. After sleep, downtime or a quit that missed one or more due times, it SHALL start exactly one catch-up run. Due times SHALL be computed in that timezone, so a daylight-saving change never yields two runs for one date. The human SHALL be able to start a run with Run now on the Brains page or the brain's Status view, on any Mac with compiling on, which SHALL say why when a run is in progress on any Mac. A Mac with compiling off SHALL offer Compile on this Mac instead.

#### Scenario: Asleep at 2:00
- **WHEN** a volunteer sleeps from 1:00 to 7:00, the compile time is 02:00, and no other Mac ran it
- **THEN** one run starts on wake, and none is started twice for that date

#### Scenario: Run now during another Mac's run
- **WHEN** the human presses Run now while a teammate's Mac holds a live claim
- **THEN** no run starts, and the page says the wiki is compiling on that Mac

### Requirement: The first Mac awake claims the run
A run on GitHub SHALL be claimed on the branch `wiffletree-compile-claim`, which SHALL hold a chain of commits. A claim SHALL be a commit with an empty tree, whose parent is the branch's tip, or none for the first, and whose message holds its host id, machine name, login, run id, the due time it is for, its state and when it was written; its state SHALL be `claimed`, `landed` or `released`. Because a claim holds its run's id, no two Macs SHALL ever push the same commit. A Mac SHALL claim with one plain push of that commit to that branch, never forced and never through a sync pass: a refused claim SHALL NOT be fetched, merged or pushed again. After the push the Mac SHALL read the tip with `git ls-remote`, whatever the push answered: its own commit SHALL mean the claim was won, another commit SHALL mean it was lost, and no answer SHALL mean it claimed nothing. To read the tip, a Mac SHALL take its SHA from `git ls-remote` and, when its clone lacks that commit, fetch that one branch into a ref outside `refs/heads/`, because a claim's state is in its message and a child needs its parent; that read SHALL come before a claim, and SHALL NOT make a refused claim go again. The sync engine SHALL NOT fetch or push that branch.

A Mac SHALL run a pass, then read the tip, then find what is pending, then resolve its model, and only then claim. A tip of `landed` for this due time SHALL mean the work is done, whatever is pending now, and the Mac SHALL record the run as compiled on that Mac and claim nothing. The tip SHALL be free when its state is `released` or `landed`, or when it is `claimed` and over an hour old.

#### Scenario: Two Macs claim at once
- **WHEN** two Macs wake at 06:00 with the same tip and push their claims in the same second
- **THEN** Git accepts one, the other's push is refused and is not tried again, and the tip shows one claim

#### Scenario: The first claim ever
- **WHEN** two Macs make a wiki's first claims, with no branch on GitHub, at the same commit of `main`
- **THEN** their commits differ, because each holds its own run's id, so exactly one creates the branch

#### Scenario: A claim made on another Mac
- **WHEN** a Mac whose clone has only ever fetched `main` finds a released claim at the tip
- **THEN** it fetches that one branch, reads the claim's state, and claims with a commit whose parent is that tip

#### Scenario: A lost answer
- **WHEN** a claim's push lands on GitHub but its answer is lost
- **THEN** the Mac reads its own commit at the tip and goes on as the winner

#### Scenario: Offline
- **WHEN** a volunteer cannot reach GitHub at the due time
- **THEN** it claims nothing, starts no model, and stays due

### Requirement: A claim expires after an hour
A claim SHALL expire when it is `claimed` and an hour has passed since it was written, by the reading Mac's clock. A Mac SHALL take over an expired claim as an ordinary new claim, a child of the expired one, with no deletion and no force, so that two Macs taking over at once race as two claims do. A Mac that lost a claim, or found a live one, SHALL stay owed that due time: after each one-minute pull it SHALL read the tip, until the tip is `landed` for that due time, the claim is free and it claims, or nothing is pending. A run that ends without landing SHALL push `released`, and one that lands SHALL push `landed`, each tried once, as a child of its own claim, and a refused one SHALL need nothing more. Before landing, a Mac SHALL read the tip, and a tip that is not its own claim SHALL end the run with nothing pushed.

#### Scenario: A crashed claim
- **WHEN** a Mac claims at 06:00 and crashes, and a teammate's Mac stays owed
- **THEN** at about 07:00 the teammate's Mac finds the claim expired, claims as a child of it, and runs

#### Scenario: Two takeovers
- **WHEN** two Macs find one expired claim at once
- **THEN** one takeover lands, and the other is refused and stays owed

#### Scenario: A run that fails
- **WHEN** a run fails its checks at 06:20
- **THEN** it pushes `released`, and the next owed Mac claims within a minute, without waiting for the hour

#### Scenario: A Mac that slept through its claim
- **WHEN** a Mac claims at 06:00, sleeps until 08:00, and another Mac took over at 07:01
- **THEN** on waking it reads a tip that is not its own, ends the run, and pushes nothing

### Requirement: A Mac out of model quota skips
A Mac whose Brain compiler row waits, or has no row, SHALL NOT claim. It SHALL record the run as waiting, saying why (see `model-routing`, "The Brain compiler's row"), and the next Mac SHALL take the run. A run that runs out of quota after claiming SHALL end, push `released` and change nothing, and the next Mac SHALL take the run.

#### Scenario: Out of quota at the due time
- **WHEN** the first Mac awake has spent its quota and a teammate's Mac is also due
- **THEN** the first Mac claims nothing and starts no model, and the teammate's Mac claims and runs

#### Scenario: Out of quota mid-run
- **WHEN** a compile session stops because the provider's quota ran out
- **THEN** nothing lands, the cursor is unchanged, the claim is released, and the next owed Mac takes the run

### Requirement: A compile lands straight on main
After its checks pass, a run's wiki files, cursor and `reconcile-queue.md` SHALL land through the wiki rule set of the repository's sync engine, as one commit on the current head, scanned like any other. A push refused because `main` moved, as when logs landed during the run, SHALL fetch and go again with the same files, with no model call and no change to the cursor. A landing SHALL be all or nothing. It SHALL land nothing and end the run when:
- `main` already holds a cursor at or past the run's, recorded as compiled elsewhere;
- a wiki file the run changed also changed on `main` since the run began, which fails the run and overwrites no one's edit;
- a file fails the scan, which fails the run, naming the file and rule and never the text;
- GitHub rejects the push for a reason another pass will not change, such as a ruleset that requires pull requests, which fails the run with GitHub's reason and releases its claim. A push that could not reach GitHub, or that `main` moved under, waits for the next pass.

A run that lands nothing SHALL leave the cursor on `main` as it was, so the next run finds the same entries. A local repository SHALL commit the same files at once.

#### Scenario: Logs land during the run
- **WHEN** a run compiled from commit `a41f2c0`, and three entries landed before its push
- **THEN** the first push is refused, the pass fetches, one commit lands on the new head with `compiled-through` still at or before `a41f2c0`, and the three entries are pending in the next run

#### Scenario: Two runs after a takeover
- **WHEN** a run lands, and a run that a takeover outran reaches its landing with the same entries
- **THEN** it finds the cursor past its own, lands nothing, and each entry was compiled once

#### Scenario: A hand edit on GitHub
- **WHEN** a teammate edits `notes/Pica.md` on GitHub during a run that also changes it
- **THEN** the run fails, the edit stands, and the next run compiles from it

#### Scenario: A wiki file fails the scan
- **WHEN** a compile writes a phone number into a note
- **THEN** nothing of the run lands, the run's record names the file and rule, and the entries stay pending

#### Scenario: GitHub rejects the push
- **WHEN** a repository's ruleset requires pull requests, and a run's push is rejected with `main` where it was
- **THEN** nothing lands, the run fails naming GitHub's reason, its claim is released, and Run now works again

#### Scenario: A network drop under the push
- **WHEN** a run's push fails on its way to GitHub
- **THEN** the run stays waiting to land, and the next pass pushes the same files

### Requirement: The app says when the wiki is falling behind
With no volunteer, logs SHALL keep landing, and the Brains page and the Space's Repository box SHALL say how many entries wait. The wiki SHALL also count as falling behind when two scheduled times have passed with entries waiting and no run landed, and both places SHALL say so, naming the last run.

#### Scenario: A team on holiday
- **WHEN** the one volunteer's Mac has been off for two days and 12 entries wait
- **THEN** the Brains page says the wiki is falling behind, with the last run and the 12 entries, and logs keep landing

### Requirement: Refs are verified against clones and GitHub
Before any model call, the host SHALL verify each pending entry's `ref`, fetching each workspace repository whose GitHub name matches the entry's repository folder at most once per run. For a folder with no clone on this machine, it SHALL find `owner/name` among the repositories of the Spaces in this folder, trying each when two owners match, and SHALL ask GitHub with `gh`: the default branch, the named commit, whether the default branch contains it, and the named PR. An entry SHALL be:
- **verified** when a named commit is reachable from the repository's default branch or the named PR is merged;
- **claimed** when a named commit exists but is not on the default branch, or the named PR is open;
- **unverified** when the `ref` is `—` or `<branch> @ —`, no named anchor is found, no repository of that name is listed, or GitHub could not be asked.

A release tag containing a verified commit, in a local clone, SHALL be recorded as further evidence. A run SHALL record how many entries GitHub could not be asked about, and the Brains page SHALL show that number. Each commit and PR SHALL be asked about once per run. The same verification SHALL be used wherever verification is shown.

#### Scenario: An open PR
- **WHEN** an entry's ref is `wiffletree/retry-telnyx-timeout @ 7d03b1e · PR #92`, the commit exists only on that branch and PR #92 is open
- **THEN** the entry is claimed

#### Scenario: A repository not cloned here
- **WHEN** an entry names `pica-api`, Vyttle lists `Vyttle-LLC/pica-api`, it is not cloned on the compiling Mac, and GitHub says the commit is on its default branch
- **THEN** the entry is verified, and the wiki may state it as fact

#### Scenario: A login that cannot read the repository
- **WHEN** GitHub refuses the compiling Mac's login for `Vyttle-LLC/pica-api`
- **THEN** the entry is unverified with the reason "GitHub could not be asked", it is not stated as fact, and the run's record counts it

#### Scenario: A folder no Space lists
- **WHEN** an entry's folder is `assistant`
- **THEN** the entry is unverified, and GitHub is not asked

### Requirement: A ruling lands straight in
Only the human SHALL rule. Rulings SHALL write a `decision` entry:
- **Uphold existing:** an entry restating the established decision.
- **Keep new:** an entry adopting the claim; the next compile marks the old decision note superseded.
- **Keep both:** an entry from the human's one-line scoping of both.

Each entry's author SHALL be the human, its repository the claim's, and its `state` SHALL begin `Ruling on conflict <id>:`. It SHALL land as any log entry does, and the next compile SHALL pick it up. **Defer** SHALL write no entry and hide the card until the next compile, which raises it again if it still holds.

#### Scenario: Keep new
- **WHEN** the human rules "Keep new" on the retry-cap conflict
- **THEN** a decision entry beginning `Ruling on conflict <id>:` lands within seconds, and the next compile marks the 5-retry decision superseded

#### Scenario: Defer
- **WHEN** the human rules "Defer"
- **THEN** no entry is written, the card disappears, and the next compile raises it again if the claim still contradicts the decision

### Requirement: Pending entries are found by a cursor of entry ids
The cursor SHALL be stored in `notes/INDEX.md`'s frontmatter as `compiled-through` (a commit on `main`) and `compiled-after` (the ids of entries added after that commit that are already compiled). Pending entries SHALL be the ids of entry files under `logs/<repo>/` at `main`'s head that are not at `compiled-through`, reading each commit's tree in its own layout, so that the move into the folder makes nothing pending, minus the ids in `compiled-after`. On a brain with no `compiled-through`, every entry file SHALL be pending, minus the ids in `compiled-after`. A run SHALL process at most 200 pending entries, oldest id first. Its commit SHALL set the cursor, written by the host from the commit it read: when no pending entry remains up to that commit, `compiled-through` becomes that commit and `compiled-after` is emptied; otherwise the processed ids are added to `compiled-after`. The cursor SHALL change only when the run lands.

#### Scenario: Same-day entries
- **WHEN** two entries dated October 29 land, one before and one after a run began
- **THEN** the first is compiled by that run and the second by the next one, and neither is skipped

#### Scenario: A failed run
- **WHEN** a run fails its checks and lands nothing
- **THEN** the cursor on `main` is unchanged and the next run finds the same pending entries

#### Scenario: Across the move
- **WHEN** `compiled-through` names a commit from before the repository moved into `wiffletree/`, and two entries landed after the move
- **THEN** only those two entries are pending

#### Scenario: Entries brought in
- **WHEN** Bring in a brain brings 191 entries, of which 180 the original had compiled
- **THEN** 11 are pending, on every Mac

#### Scenario: Entries already in the folder at Bring in
- **WHEN** a Space's folder holds 12 uncompiled entries, and Bring in a brain then brings 180 entries the original had compiled
- **THEN** those 12 stay pending, and only the 180 are read as compiled
