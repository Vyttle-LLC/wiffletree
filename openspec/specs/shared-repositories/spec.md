# shared-repositories Specification

## Purpose
Keeps one clone and one sync engine for each Git repository Wiffletree writes to, with the rules for each file chosen by its path and everything a Space shares in one folder, so that settings, Spaces, issues, boards, log entries and the wiki can share a repository, and each Mac scans what it pushes and pulls.

## Requirements

### Requirement: Clones live under one root
The host SHALL keep every clone it writes to under `<data folder>/repos/`: a GitHub repository at `<data folder>/repos/github.com/<owner>/<name>/`, and a local repository at `<data folder>/repos/local-<id>/`. One folder per owner keeps every repository its own folder with no escaping, where a folder named `<owner>-<name>` gave `a-b/c` and `a/b-c` the same one; `github.com` holds a dot, which no GitHub owner's name can, so it never meets an owner's folder or a `local-<id>` one. A clone found in a repository's folder SHALL be used only when its `origin` names that repository. It SHALL keep one clone per repository, whatever the repository is used for on this Mac, and SHALL NOT use a folder the human chose as that clone. A repository MAY be a Space's repository and this Mac's settings repository at once, and neither use SHALL refuse the other. A clone is the host's own, and a file changed in it outside Wiffletree SHALL be paused (see `settings-repository`, "A file changed outside Wiffletree is paused").

#### Scenario: Storage that is also the settings repository
- **WHEN** `Sage-Social/wiffletree-storage` is both this Mac's settings repository and the Sage Space's repository
- **THEN** there is one clone, at `<data folder>/repos/github.com/Sage-Social/wiffletree-storage/`, and one sync engine for it

#### Scenario: Two names that once shared a folder
- **WHEN** this Mac has clones of `a-b/c` and `a/b-c`
- **THEN** they are at `repos/github.com/a-b/c/` and `repos/github.com/a/b-c/`

#### Scenario: Clones an earlier release named `<owner>-<name>`
- **WHEN** Wiffletree starts with the settings repository's clone at `<data folder>/repos/MHohlios-wiffletree-storage/`
- **THEN** it is renamed, by the repository its `origin` names, to `repos/github.com/MHohlios/wiffletree-storage/`; the rename is recorded before it is made and finished at the next start when a start is cut short; a second start moves nothing; and a clone whose new folder is taken, or whose `origin` names no GitHub repository, stays where it is with a log line

#### Scenario: Settings in a brain
- **WHEN** the human chooses `Sage-Social/sagehaven-docs`, which an older Space names as its brain, as the settings repository
- **THEN** it is checked like any other, and may be used

### Requirement: One sync engine per repository, rules by path
Each repository SHALL have one sync engine that fetches, merges and pushes it, with the settings repository's reach, push and backoff rules: Git reaches GitHub as `gh` does, with its login through a credential helper given for each command; choosing the repository is what allows the push; a push goes only to the recorded default branch and URL, contains the remote head, has no tags or submodules, and is never forced; passes run off the thread that serves host commands and back off up to 30 minutes while GitHub is out of reach. Every file the engine syncs SHALL fall under exactly one rule set, chosen by its path, and a file under no rule set SHALL be pulled and never committed. Each commit SHALL hold changes of one rule set only, apart from the one commit that moves a repository into its folder. A rule set whose comparison fails, such as on a file it cannot read, SHALL commit nothing that pass and leave its files as they are, while the repository's other rule sets go on; the failure SHALL be reported for that rule set only. What this machine last synced and wrote of each file SHALL be recorded per repository. A repository SHALL be fetched at the shortest cadence of the rule sets it holds, which is every minute for a repository holding a Space. The settings rule set SHALL cover `wiffletree.json`, `settings/` and `skills/` at the repository's root. Inside the folder, the Space rule set SHALL cover `spaces/` and `compilers/`, apart from what another rule set claims below it; both SHALL compare whole files, pause a file both sides changed, and pause a file changed outside Wiffletree on this machine (see `settings-repository`). The other rule sets inside the folder SHALL be `issues/` with `wiffletree-issues.yaml` (see `shared-issues`), runner-written boards (see `scheduled-jobs`), `second-brain/logs/`, which push without comparing because an entry is written once by one Mac (see `project-memory`), and the wiki, `second-brain/notes/`, `STATUS.md` and `reconcile-queue.md` (see `brain-compile`).

#### Scenario: A Space edit and a settings edit together
- **WHEN** the human changes Sage's description and a review level within 5 seconds, in a repository that holds both
- **THEN** the engine pushes two commits, one changing only `wiffletree/spaces/sage/space.yaml` and one changing only `settings/review.json`

#### Scenario: A README
- **WHEN** a teammate edits `README.md` in a Space's repository on GitHub
- **THEN** the next pass pulls it and no commit ever changes it

#### Scenario: A Space file that cannot be compared
- **WHEN** a pass of a repository holding settings and Sage cannot read Sage's folder
- **THEN** the settings changes are committed and pushed, Sage's files stay as they are, and only the Space rule set reports the failure

#### Scenario: A log entry and a comment together
- **WHEN** an agent logs a decision and the human comments on VYT-42 within 2 seconds
- **THEN** one commit adds only the entry's file, and another only the comment's

### Requirement: The settings clone moves to the root
At the first start of this release, a machine with a settings repository SHALL move its clone from `<data folder>/shared/` to `<data folder>/repos/github.com/<owner>/<name>/` and record the new path, so that the shared folder is that clone. A machine without one SHALL keep `<data folder>/shared/` as a plain folder. An interrupted move SHALL be finished at the next start, and no shared setting SHALL change. Sync's record from a release that kept one record SHALL become the settings repository's record, once.

#### Scenario: A machine with a settings repository
- **WHEN** Wiffletree starts after the update with `MHohlios/wiffletree-storage` as its settings repository
- **THEN** the clone is at `<data folder>/repos/github.com/MHohlios/wiffletree-storage/`, `<data folder>/shared/` is gone, and every shared setting reads as before

### Requirement: Files are scanned before they are pushed
Before pushing a commit that changes files in the folder, the host SHALL run the `sensitive-info` and `secret-scan` checks on those files on this Mac, at the version this release bundles, with their default rules, inline allow markers and the folder's `allowed_email_domains`. A scan SHALL read only the scanned paths: `spaces/`, `issues/`, `wiffletree-issues.yaml`, `compilers/` and `second-brain/`, in the folder, with no hidden part in the path, and only those files SHALL be written to its scratch repository, so that a file in the repository is never read as the checks' config, in a repository not yet moved too. A file that fails SHALL be held: it SHALL NOT be pushed, this Mac SHALL keep using it, and a card SHALL name the file, the line, the check and the rule, never the matched text. For a held issue or Space file the card SHALL be a workspace note offering to edit it. For a held log entry it SHALL be a card on the project that wrote it, or on the Brains page when none did, offering what `brain-registry` lists for its check. A wiki file that fails SHALL end its compile run (see `brain-compile`). Other files SHALL keep syncing. A check that cannot run SHALL hold the file in the same way, naming what is missing. Local repositories, which are never pushed, SHALL NOT be scanned.

#### Scenario: A phone number in an issue
- **WHEN** the human files an issue whose description holds a phone number in a repository on GitHub
- **THEN** the issue stays on this Mac, is not pushed, and a workspace note names its file and the `sensitive-info` rule without the number

#### Scenario: A held entry
- **WHEN** an agent logs an entry whose ninth line holds a phone number
- **THEN** the entry is not pushed, other entries keep landing, and the project's Inbox shows "A log entry was held on this Mac", with `sensitive-info`, rule `phone`, line 9, and no number

#### Scenario: gitleaks missing
- **WHEN** a Space's instructions change on a Mac where `secret-scan` cannot find gitleaks
- **THEN** `space.yaml` is held, and the note says gitleaks is missing

#### Scenario: An allowed domain
- **WHEN** `wiffletree-issues.yaml` lists `sagehaven.ai` in `allowed_email_domains` and an entry names a staff address there
- **THEN** the entry lands, and an address at any other domain is still held

### Requirement: A repository holds one Wiffletree folder
A repository SHALL hold at most one Wiffletree folder, `wiffletree/` by default. The folder SHALL be a relative path of plain names, with no `.`, `..` or hidden part. Everything of a Space SHALL be inside it:

```text
<folder>/
  spaces/<space>/
  issues/<ID>/
  wiffletree-issues.yaml
  compilers/<host id>.yaml
  second-brain/
    STATUS.md
    logs/<repo>/...
    notes/
    reconcile-queue.md
```

A path these specifications give for a Space's, issue, job, board, log or wiki file, such as `spaces/<space>/space.yaml`, SHALL be relative to the folder. A Space's rule sets SHALL pull files outside the folder and never commit them, whatever they are. There are two exceptions, each its own write: the settings rule set's three paths at the root of a repository the human also chose as the settings repository, and the one commit that moves a repository into its folder, with its renames and forwarding stubs. A repository whose named folder exists and holds no Wiffletree files SHALL be refused. A Mac joining a repository SHALL find the folder by listing the tree at the head of its default branch for `<folder>/spaces/<space>/space.yaml` and `<folder>/wiffletree-issues.yaml`, skipping forwarding stubs: one folder found SHALL be the folder, none SHALL offer it as a new repository, and more than one SHALL refuse the repository, naming them. No file SHALL mark the folder from outside it. The folder SHALL be kept on this Mac with the Space and never in `space.yaml`.

#### Scenario: Other documents beside the folder
- **WHEN** a Space's repository is `Vyttle-LLC/vyttle-docs`, and a teammate edits `products/pica/README.md` on GitHub
- **THEN** the next pass pulls the edit, and no commit from Wiffletree ever changes anything outside `wiffletree/`

#### Scenario: A joining Mac
- **WHEN** a Mac adds `Vyttle-LLC/vyttle-wiffletree-storage`, whose tree holds `wiffletree/spaces/vyttle/space.yaml`
- **THEN** it finds the folder `wiffletree/` with no question

#### Scenario: A custom folder
- **WHEN** a repository holds `team/wiffletree/spaces/sage/space.yaml`
- **THEN** a joining Mac finds the folder `team/wiffletree/`

#### Scenario: Two folders
- **WHEN** a repository holds both `wiffletree/spaces/` and `other/spaces/` with Space files
- **THEN** it is refused, naming both, and nothing is written

#### Scenario: A folder that is someone else's
- **WHEN** the user names the folder `docs/` in a repository where `docs/` holds other files
- **THEN** it is refused, saying the folder must be Wiffletree's

### Requirement: Files at a repository's root move into the folder once
A local repository with its Spaces' files at its root SHALL move them into `wiffletree/` at the first start of this release, in one commit, with no question. A GitHub repository with its Spaces' files at its root SHALL work from the root, as an empty folder, until the human presses Move into `wiffletree/` on a Space's page, which SHALL ask first, naming how many files move and that a Mac on an older release stops syncing this repository until it updates. The move SHALL be one commit that renames every file of the repository's rule sets but the settings rule set's into the folder, with no change of content, and adds the forwarding stubs. It SHALL be the one commit that may span rule sets, SHALL hold nothing else, and SHALL be pushed unforced. A refused push SHALL fetch: when the root layout is gone the move is done, and otherwise it SHALL be made again on the new head, up to 5 times. Every Mac on this release SHALL adopt the move in its next pass, moving what it recorded, its alerts included, and any unpushed file under the folder, so that no unpushed change is lost and an open alert follows its file. An interrupted move SHALL be finished at the next start. The cursor in `notes/INDEX.md` SHALL stay as it was, because pending entries are found by id (see `brain-compile`).

#### Scenario: A local repository
- **WHEN** Wiffletree starts after the update with Personal's repository holding `spaces/personal/`, `issues/` and `wiffletree-issues.yaml` at its root
- **THEN** one commit moves them under `wiffletree/`, a second start moves nothing, and every issue and Space reads as before

#### Scenario: Moving a GitHub repository
- **WHEN** the human presses Move into `wiffletree/` for `Sage-Social/wiffletree-storage` and confirms
- **THEN** one commit renames its Spaces' files, issues and tracker into `wiffletree/`, and adds a forwarding stub for each Space and the tracker

#### Scenario: Two Macs move at once
- **WHEN** two Macs press Move into `wiffletree/` before either pulls
- **THEN** one push lands, the other fetches, finds the root layout gone and adopts the folder, and there is one move commit

#### Scenario: An unpushed comment on another Mac
- **WHEN** a Mac has an unpushed comment on SAGE-151 when it pulls the move
- **THEN** its comment moves under `wiffletree/issues/SAGE-151/`, and the pass that follows at once pushes it there

#### Scenario: A repository that is also the settings repository
- **WHEN** the moved repository is also this Mac's settings repository
- **THEN** `wiffletree.json`, `settings/` and `skills/` stay at the root, unchanged

### Requirement: An older host stops at the move
The move SHALL leave a forwarding stub at each old `spaces/<space>/space.yaml` and at the old `wiffletree-issues.yaml`, holding `format: 2`, `moved_to: <folder>` and the keys an older host must parse, a Space's `id` and `name` and the tracker's `prefix`, so that an older host pauses each as a file written by a newer Wiffletree, keeps its last good copy and tells its human to update. A `space.yaml` inside a folder SHALL stay in `format` 1. Wiffletree SHALL NOT change or remove a stub after the move, and SHALL leave alone anything an older host writes at the root afterwards.

#### Scenario: A teammate on an older release
- **WHEN** a Mac on an older release pulls the move
- **THEN** Sage's `space.yaml` and the tracker pause with "written by a newer Wiffletree. Update to use it.", the Mac keeps its last good Space, and nothing is lost on GitHub

#### Scenario: That teammate updates
- **WHEN** that Mac updates to this release
- **THEN** its next pass finds the folder, skips the stubs and syncs from `wiffletree/`

### Requirement: Each Mac scans what it pulls
After a pass moves a GitHub repository's head, the host SHALL scan every commit from this Mac's mark to the new head, one commit at a time, oldest first, including commits this Mac pushed. Each commit SHALL be compared with its first parent, over the files it changed under the scanned paths, with the checks and rules of "Files are scanned before they are pushed", in a scratch repository, so that no file in the repository can change the scan's rules. A commit that only renames adds nothing to scan. A file Git calls binary SHALL be skipped. Each check SHALL keep its own mark, which SHALL move as each commit finishes, so a quit resumes where it stopped. A joining Mac, and every repository at upgrade, SHALL start its marks at the head then. A mark that is no longer an ancestor of the head SHALL restart at the merge base. The scan SHALL run off the pass and SHALL NOT delay it. Local repositories SHALL NOT be scanned.

#### Scenario: An edit on GitHub
- **WHEN** a teammate adds a phone number to `wiffletree/issues/VYT-52/issue.md` in GitHub's web editor
- **THEN** every Mac that pulls the commit scans it and raises an alert

#### Scenario: Added and removed within a minute
- **WHEN** two commits add a token and remove it, and a Mac pulls both in one pass
- **THEN** the scan of the first commit finds it, and an alert says it reached GitHub

#### Scenario: A file outside the folder
- **WHEN** a commit adds a phone number to a `README.md` outside `wiffletree/`
- **THEN** no alert is raised

#### Scenario: A Mac back after a month
- **WHEN** a Mac pulls 900 commits after a month away
- **THEN** the pass finishes at once, the scan works through them behind it, and a quit halfway resumes from its mark

#### Scenario: A pushed allow-list
- **WHEN** a teammate pushes a `.gitleaks.toml` that allows everything, then a token
- **THEN** the scan does not read that file, and finds the token

#### Scenario: Config files in a repository not yet moved
- **WHEN** a repository with an empty folder gets a `.gitleaksignore`, and a root `wiffletree-brain.json` naming an allow-list in `gate.allowlist`, then a phone number in an issue
- **THEN** neither file reaches the scratch repository, and the phone number is found

### Requirement: An alert is raised once per Mac
A finding in a pulled commit SHALL raise one alert on this Mac for its repository, path, check and rule; a later finding of the same rule in the same file SHALL update that alert and raise no other. The alert SHALL be a card at the top of every project's Inbox in the Spaces using the folder, counted in each and settled once for all on this Mac, and SHALL show on the Brains page. It SHALL name the check, the rule, the file and line, and who pushed it, saying "pushed from GitHub's web editor" when the commit's committer is GitHub, and SHALL NEVER show the matched text. It SHALL say that removing the text adds a new commit and that the old text stays in Git history until someone cleans it, and SHALL offer Open the file, Not sensitive and How to clean history. After any pull that changes the file, the host SHALL scan the file whole at the head, and SHALL close the alert when the rule no longer fires there. Not sensitive SHALL take a reason and close the alert on this Mac, keeping a record so that the same path, rule and content never alert here again, and SHALL write nothing to the repository. Each Mac SHALL raise and clear its own alerts.

#### Scenario: One alert, many commits
- **WHEN** three pulled commits each add a phone number to `issues/VYT-52/issue.md`
- **THEN** this Mac shows one card, naming the `phone` rule, that file and the latest line

#### Scenario: The alert as drawn
- **WHEN** a Mac raises an alert for a phone number added on GitHub
- **THEN** the card says "Something on `main` failed a scan", names `sensitive-info`, rule `phone`, `wiffletree/issues/VYT-52/issue.md` line 4 and GitHub's web editor, and holds no digit of the number

#### Scenario: Someone removes the text
- **WHEN** a teammate removes the line and a Mac pulls that commit
- **THEN** that Mac's alert closes by itself, and the next Mac's closes at its own pull

#### Scenario: An alert through the move
- **WHEN** an alert is open on `issues/VYT-52/issue.md` and the repository moves into `wiffletree/`
- **THEN** the alert stays open and names `wiffletree/issues/VYT-52/issue.md`, and closes only when the rule no longer fires in that file on `main`

#### Scenario: Not sensitive
- **WHEN** the human marks an alert Not sensitive, giving the reason "our shop's public number"
- **THEN** the card closes on this Mac, the repository has no new commit, a teammate's Mac keeps its own card, and a later edit of the file that does not change that content raises nothing here

### Requirement: A pull scan that cannot run says so
A check that cannot run on a pulled commit, such as `secret-scan` with gitleaks missing, SHALL keep its mark where it is, so that nothing is skipped, and SHALL raise one workspace note naming what is missing and saying that what is pulled is not being scanned by that check. The other check SHALL go on. A pull SHALL never be held or refused because of a scan. When the check can run again, the note SHALL close and the check SHALL catch up from its mark.

#### Scenario: gitleaks missing
- **WHEN** a Mac without gitleaks pulls a new commit
- **THEN** `sensitive-info` scans it, the pull completes, and one workspace note says gitleaks is missing and that pulled commits are not being scanned for secrets

#### Scenario: gitleaks arrives
- **WHEN** gitleaks is installed after 12 commits were pulled without it
- **THEN** the note closes, and `secret-scan` scans those 12 commits
