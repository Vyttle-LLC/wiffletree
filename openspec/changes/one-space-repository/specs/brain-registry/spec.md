## ADDED Requirements

### Requirement: A Space's brain is its folder's second-brain
A Space's brain SHALL be `second-brain/` in its repository's folder, holding its logs, its wiki and its reconcile queue. Every Space SHALL have one, nothing SHALL register it, and no Space SHALL name a brain. Spaces in one folder SHALL share one brain. These specifications' "a Space's brain" SHALL mean this folder, and "wiki" SHALL mean its `notes/` and `STATUS.md`. A brain SHALL be read and written only through the repository's one clone and sync engine (see `shared-repositories`). No agent SHALL add, remove or move a brain.

#### Scenario: A new Space's brain
- **WHEN** the human creates Vyttle in `Vyttle-LLC/vyttle-wiffletree-storage`
- **THEN** its brain is `wiffletree/second-brain/` there, with no field to fill in and no separate clone

#### Scenario: An agent cannot add a brain
- **WHEN** an agent's tool call or message asks the host to add or remove a brain
- **THEN** no tool does this

### Requirement: Choosing the repository grants its pushes
Choosing a Space's repository SHALL be what allows the host to push to it, and the confirmation SHALL say so. The grant SHALL allow pushing the default branch, inside the folder, and pushing the claim branch `wiffletree-compile-claim` (see `brain-compile`), and nothing else. The allowed owners for ticket PRs SHALL NOT grant this, and choosing a repository SHALL NOT allow ticket PRs to it or its owner. The host SHALL open no pull request on it.

#### Scenario: An allowed owner
- **WHEN** `Sage-Social` is allowed for ticket PRs and no Space here uses `Sage-Social/sagehaven-docs`
- **THEN** the host pushes nothing to `Sage-Social/sagehaven-docs`

#### Scenario: A Space's repository does not allow ticket PRs
- **WHEN** `Vyttle-LLC/vyttle-docs` is a Space's repository and a ticket in it is published
- **THEN** the publish follows `ticket-publishing`'s rules for its owner, unchanged

### Requirement: Every change to a brain lands through the sync engine
Every change the host makes to a brain SHALL go through the repository's sync engine and land straight on `main`: log entries within seconds, compile results when they pass their checks, and rulings as log entries. The host SHALL open no branch or pull request for a brain, turn on no auto-merge and watch no pull request. In a local repository each change SHALL be committed at once. A push refused because `main` moved SHALL fetch and go again, as for issues.

#### Scenario: An entry lands
- **WHEN** an agent logs a decision in the Vyttle Space
- **THEN** within seconds one commit on `main` adds the entry's file, and every other Mac has it within a minute

#### Scenario: Offline
- **WHEN** GitHub cannot be reached while an entry waits
- **THEN** it stays on this Mac, passes back off up to 30 minutes, Sync now tries at once, and it lands at the first pass that reaches GitHub

### Requirement: A held entry can be edited, marked or discarded
A log entry that fails the scan SHALL stay on this Mac, in a held state that survives restarts, and its card (see `shared-repositories`, "Files are scanned before they are pushed") SHALL offer:
- Edit it, which changes the entry here, scans it again and lands it once it passes;
- for a finding of `sensitive-info` only, Not sensitive, with a reason, which appends the inline marker `<!-- gate:allow <rule> <reason> -->` to that line and lands it;
- Discard, which removes the entry from this Mac's clone, keeps its row as discarded in the store, and pushes nothing.

The card SHALL close when the entry lands or is discarded. Only the human SHALL do these, and no agent tool SHALL. The host SHALL NOT change an entry once it has landed.

#### Scenario: Not sensitive
- **WHEN** the human marks line 9 not sensitive with the reason "fixture call ID"
- **THEN** `<!-- gate:allow phone fixture call ID -->` is appended to line 9, the next scan passes, the entry lands, and the card closes

#### Scenario: Held by secret-scan
- **WHEN** an entry is held by a gitleaks rule, or because a check could not run
- **THEN** the card offers Edit it and Discard, and no Not sensitive

#### Scenario: Discard
- **WHEN** the human discards a held entry
- **THEN** it is never pushed, nothing about it reaches GitHub, and the card closes

#### Scenario: After landing
- **WHEN** an entry has landed and the human opens it
- **THEN** no edit or discard action is offered

### Requirement: Who compiles a wiki
Compiling SHALL be a choice each Mac makes for a wiki: on, a Mac SHALL publish `compilers/<host id>.yaml` in the folder, holding its machine name and login, and off, it SHALL remove that file. Only that Mac SHALL write its file, so two Macs choosing at once never conflict. Any Mac that can push to the repository MAY turn compiling on, and no Mac SHALL be chosen for another. The compile time SHALL be `compile` in `wiffletree-issues.yaml`, an hour and minute with an IANA timezone, shared by the folder's Spaces; the human SHALL set it on the Brains page, it SHALL land as an ordinary Space-side commit, and the host SHALL refuse a time or timezone the schedule could not read. Only the human's client SHALL give either command, and no agent tool SHALL reach them.

#### Scenario: Turning compiling on
- **WHEN** a teammate ticks Also on this Mac while joining Vyttle
- **THEN** a commit adds one file to `wiffletree/compilers/`, and every Mac's Brains page lists that Mac

#### Scenario: Changing the time
- **WHEN** the human changes 06:00 to 02:00 America/Chicago
- **THEN** one commit changes only `compile` in `wiffletree-issues.yaml`, keys the host does not know stay, and every volunteer is due at the new time once it pulls

#### Scenario: A time the schedule cannot read
- **WHEN** the human enters 25:00, or a timezone that does not exist
- **THEN** the host refuses it, saying why, and nothing is written

### Requirement: Bring in a brain
The Repository box SHALL offer Bring in a brain…, which SHALL take a folder on this Mac, a clone of a brain, and the brain folder in it, `second-brain` by default, and for a Space from an earlier release SHALL be filled in from its `brain` key. It SHALL read that folder's files as they are on this disk, committed or not, and never GitHub. The form SHALL show what it brings in, the wiki pages with `STATUS.md`, and the log entries with how many files they come from, each with its date and repository; what it leaves behind, the original folder as it is along with anything else in it, such as `archive/`, `automation/` and `raw/`, saying it can be deleted once the human is happy; how many entries are shortened to fit the cap, whose full text stays in the original; and a warning naming how many files are not on GitHub and how many were never committed.

It SHALL NOT change the original: it SHALL write nothing there, and its only Git commands there SHALL be reads that take no lock. Entry files that meet the contract SHALL be copied byte for byte. Each legacy `logs/<repo>.md` SHALL become one entry file per entry, with `source: skill` and an id derived from its repository, date, title and body, so that bringing in again adds no file. Its author SHALL be the GitHub login of the commit that added its heading, or the importing human's login when none is known, as for a file never committed. An entry over the cap SHALL keep its heading and fields, with `changed` shortened to fit and ending with a pointer to the legacy file. A `ref` that does not fit the contract SHALL become `—`, with its text moved into `state`.

Wiki pages, `STATUS.md` and `reconcile-queue.md` SHALL be copied byte for byte, keeping `repos:` and every other frontmatter key, except that the old cursor lines leave a copied `notes/INDEX.md`. A page that already exists with other text SHALL be left and listed as skipped. Entries the original had compiled, by its cursor or its `compiled-through` markers, SHALL be added to `compiled-after` in the folder's `notes/INDEX.md`, so that every Mac reads them as compiled, and the rest SHALL be pending. Bring in SHALL NOT set `compiled-through` or change a cursor the folder already has, so an entry already in the folder stays pending until a compile folds it. Bring in SHALL be the one time its `wiffletree-brain.json` is read: `compile` and `allowed_email_domains` SHALL be carried from it when the folder's file has none, and a Mac it names as compiler that is doing the Bring in SHALL become a volunteer. Everything SHALL land through the sync engine, scanned, in commits of at most 200 files, and a file that fails SHALL be held with its card. When it finishes, the Space's `brain` key SHALL leave `space.yaml`, and the host's own checkout of that brain SHALL be deleted unless another Space here still names it. Only the human SHALL start it.

#### Scenario: Files that are not on GitHub
- **WHEN** the human brings in `~/dev/vyttle/vyttle-docs`, whose `second-brain/` holds 17 changed or untracked files, 11 of them never committed
- **THEN** the form says so, shows 10 wiki pages and 191 log entries from 17 files, and every entry lands, including those in the 11 files

#### Scenario: The original is left alone
- **WHEN** the bring-in finishes
- **THEN** `git status` in `~/dev/vyttle/vyttle-docs` shows exactly what it showed before, its index and every file are unchanged, and `archive/`, `automation/` and `raw/` were not read

#### Scenario: Bringing in again
- **WHEN** an older host logged two more entries to the old brain and the human brings it in again
- **THEN** only those two files land, and no page is overwritten

#### Scenario: A held entry during bring-in
- **WHEN** one of 191 entries fails `sensitive-info`
- **THEN** 190 land, the one stays on this Mac with its card, and bringing in again does not raise it twice

#### Scenario: A transcript over the cap
- **WHEN** a legacy `pica-api` entry holds 18,000 characters
- **THEN** its entry file fits the cap, keeps its heading and fields, points to the legacy file, and the form counted it as shortened

#### Scenario: Product pages survive
- **WHEN** `notes/Pica.md` has `repos: [pica-ios, pica-api]` and sources that are file paths
- **THEN** it arrives byte for byte, and the next compile keeps its `repos:`

### Requirement: Waiting work carries over from an older brain
At the first start of this release, entries of a brain batch that was queued and not pushed SHALL land through the logs rule set in the Space's folder. A batch with a pull request open SHALL be left on GitHub and no longer watched, and the Brains page SHALL name each such pull request once. The host's records of batches and brains SHALL stay in the store, unwritten, so an earlier build still opens it. A pull request that merges later SHALL reach the wiki through Bring in a brain, where ids stop any double.

#### Scenario: A queued batch
- **WHEN** Sage has a batch of two entries queued for `sagehaven-docs` when the update lands
- **THEN** both entries land in Sage's folder, and nothing is pushed to `sagehaven-docs`

#### Scenario: An open pull request
- **WHEN** batch PR #214 is open at the upgrade
- **THEN** the Brains page names #214 once, and it stays on GitHub for the human to merge or close

### Requirement: The Brains page shows each wiki
The sidebar's Workspace section SHALL list Brains beside Repositories. The Brains page SHALL show one group per wiki on this Mac, named after its Space, or its Spaces when several share it, with Run now and:
- the compile time, with Change;
- the last run, with when, on which Mac, how many entries and how many conflicts;
- the next run, "on the first of these Macs awake";
- each Mac that compiles it, with this Mac's tick to turn compiling on or off and, for another Mac, when it last ran.

When no Mac compiles a wiki and entries wait, the page SHALL say so with how many, offering Compile on this Mac, and SHALL say the same in the Space's Repository box. The page SHALL also show the alerts, held entries and pull requests left at upgrade that have no project to go to. It SHALL open the brain browser for each wiki and keep the conflict cards. It SHALL NOT show a pull request gate list, Set up, Update gate or a repository-to-brain mapping, and it SHALL NOT warn about the second-brain skills.

#### Scenario: Two Macs
- **WHEN** Michael's Mac and a teammate's Mac compile Vyttle's wiki, and the last run landed at 06:04 on Michael's Mac
- **THEN** the group reads "Today at 06:04 on Michael's Mac · 9 entries · 1 conflict", "Tomorrow at 06:00, on the first of these Macs awake", and lists both Macs, the teammate's with "Last ran Tuesday"

#### Scenario: No volunteer
- **WHEN** no Mac compiles Sage's wiki and 31 entries wait
- **THEN** the page says "No Mac compiles Sage's wiki. 31 entries are waiting." with Compile on this Mac, and logs keep landing

## MODIFIED Requirements

### Requirement: A project's brain comes from its Space
A project SHALL store no brain of its own. A project's brain SHALL be its Space's brain. A project a coordinator creates with `create_project` SHALL belong to the creating project's Space and SHALL use that Space's brain. Assistant threads and job runs SHALL use their own Space's brain. Capture, compile, search and `query_brain` SHALL resolve the brain this way and in no other way.

#### Scenario: A project in the Sage Space
- **WHEN** a project in the Sage Space works in repositories owned by `Sage-Social` and by `Vyttle-LLC`
- **THEN** its only brain is the Sage Space's brain

#### Scenario: A project created by a coordinator
- **WHEN** a coordinator in the Vyttle Space creates a project with `create_project` that adds a repository outside Vyttle
- **THEN** the new project is in the Vyttle Space and logs only to Vyttle's folder

#### Scenario: No brain
- **WHEN** a project's Space is Personal, which named no brain in an earlier release
- **THEN** the project logs to Personal's own brain, on this Mac alone, and nothing is pushed

## REMOVED Requirements

### Requirement: The Brains page lists the Spaces' brains
**Reason**: Its registry, gate list and config actions are gone.
**Migration**: See "The Brains page shows each wiki".

### Requirement: A Space registers its brain
**Reason**: A Space no longer names a brain; every Space has one in its folder.
**Migration**: See "A Space's brain is its folder's second-brain" and `spaces`, "A Space from an earlier release upgrades without losing anything".

### Requirement: Naming a brain grants only its batch pushes
**Reason**: There are no batches or brain pull requests.
**Migration**: See "Choosing the repository grants its pushes".

### Requirement: The host owns a separate checkout of each brain
**Reason**: The brain is in the Space's repository, which has one clone and one engine.
**Migration**: An old checkout stays until Bring in a brain finishes.

### Requirement: The brain config file lives in the brain repository
**Reason**: The compiler goes, and the compile time and allowed domains are facts of the folder.
**Migration**: `wiffletree-brain.json` is read once at Bring in, never written, and left with the old brain. `compile` and `allowed_email_domains` move to `wiffletree-issues.yaml`.

### Requirement: Changing the compiler host goes through a PR
**Reason**: Volunteer Macs replace the named compiler.
**Migration**: See "Who compiles a wiki".

### Requirement: Every change reaches main through a batch PR that auto-merges
**Reason**: Writes land straight on `main`, as Michael approved on November 1, 2026.
**Migration**: See "Every change to a brain lands through the sync engine" and "Waiting work carries over from an older brain".

### Requirement: Brain PRs are watched without a model
**Reason**: There are no brain pull requests to watch.
**Migration**: None. A pull request left open at upgrade is the human's.

### Requirement: A failed check or conflict raises an Inbox item
**Reason**: Checks run on the Mac before a push, and nothing waits on GitHub.
**Migration**: See `shared-repositories`, "Files are scanned before they are pushed" and "An alert is raised once per Mac".

### Requirement: An unmerged batch can be amended
**Reason**: A batch no longer exists; a held entry has not left the Mac.
**Migration**: See "A held entry can be edited, marked or discarded".

### Requirement: The legacy safety-net never writes a managed brain
**Reason**: The safety-net task and pointer are retired, and the host has no separate checkout to mark.
**Migration**: Michael's dotfiles remove the pointer and tasks, as `design.md` lists.

### Requirement: Earlier brain attachments are migrated
**Reason**: An upgrade from those releases now ends in a Space with its own brain, and needs no grant.
**Migration**: A local brain folder on a Space is offered through Bring in a brain.

### Requirement: Changing the compile time goes through a PR
**Reason**: There is no config PR.
**Migration**: See "Who compiles a wiki".
