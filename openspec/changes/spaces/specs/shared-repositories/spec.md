## Purpose

Keeps one clone and one sync engine for each Git repository Wiffletree writes to, under one folder, with the rules for each file chosen by its path, so that settings, Spaces, issues and boards can share a repository without sharing code paths that disagree.

## ADDED Requirements

### Requirement: Clones live under one root
The host SHALL keep every clone it writes to under `<data folder>/repos/`: a GitHub repository at `<data folder>/repos/github.com/<owner>/<name>/`, and a local storage repository at `<data folder>/repos/local-<id>/`. One folder per owner keeps every repository its own folder with no escaping, where a folder named `<owner>-<name>` gave `a-b/c` and `a/b-c` the same one; `github.com` holds a dot, which no GitHub owner's name can, so it never meets an owner's folder or a `local-<id>` one. A clone found in a repository's folder SHALL be used only when its `origin` names that repository. It SHALL keep one clone per repository, whatever roles the repository has on this Mac, and SHALL NOT use a folder the human chose as that clone. Brains SHALL keep their own clone under the same root (see `brain-registry`). A repository SHALL have one role on this Mac: a brain, or a storage or settings repository, which MAY be the same repository. Choosing a repository for a second role it cannot share SHALL be refused, naming the first.

#### Scenario: Storage that is also the settings repository
- **WHEN** `Sage-Social/wiffletree-storage` is both this Mac's settings repository and the Sage Space's storage
- **THEN** there is one clone, at `<data folder>/repos/github.com/Sage-Social/wiffletree-storage/`, and one sync engine for it

#### Scenario: Two names that once shared a folder
- **WHEN** this Mac has clones of `a-b/c` and `a/b-c`
- **THEN** they are at `repos/github.com/a-b/c/` and `repos/github.com/a/b-c/`

#### Scenario: Clones an earlier release named `<owner>-<name>`
- **WHEN** Wiffletree starts with the settings repository's clone at `<data folder>/repos/MHohlios-wiffletree-storage/`
- **THEN** it is renamed, by the repository its `origin` names, to `repos/github.com/MHohlios/wiffletree-storage/`; the rename is recorded before it is made and finished at the next start when a start is cut short; a second start moves nothing; and a clone whose new folder is taken, or whose `origin` names no GitHub repository, stays where it is with a log line

#### Scenario: Settings in a brain
- **WHEN** the human chooses `Sage-Social/sagehaven-docs`, a registered brain, as the settings repository
- **THEN** it is refused, naming the Space whose brain it is, and nothing is written

### Requirement: One sync engine per repository, rules by path
Each repository SHALL have one sync engine that fetches, merges and pushes it, with the settings repository's reach, push and backoff rules: Git reaches GitHub as `gh` does, with its login through a credential helper given for each command; choosing the repository is what allows the push; a push goes only to the recorded default branch and URL, contains the remote head, has no tags or submodules, and is never forced; passes run off the thread that serves host commands and back off up to 30 minutes while GitHub is out of reach. Every file the engine syncs SHALL fall under exactly one rule set, chosen by its path, and a file under no rule set SHALL be pulled and never committed. Each commit SHALL hold changes of one rule set only. A rule set whose comparison fails, such as on a file it cannot read, SHALL commit nothing that pass and leave its files as they are, while the repository's other rule sets go on; the failure SHALL be reported for that rule set only. What this machine last synced and wrote of each file SHALL be recorded per repository. A repository SHALL be fetched at the shortest cadence of the rule sets it holds. The settings rule set SHALL cover `wiffletree.json`, `settings/` and `skills/`, and the Space rule set `spaces/`, apart from what another rule set claims below it; both SHALL compare whole files, pause a file both sides changed, and pause a file changed outside Wiffletree on this machine (see `settings-repository`). Other changes add the rule sets for `issues/` with `wiffletree-issues.yaml` (see `shared-issues`) and for runner-written boards (see `scheduled-jobs`).

#### Scenario: A Space edit and a settings edit together
- **WHEN** the human changes Sage's description and a review level within 5 seconds, in a repository that holds both
- **THEN** the engine pushes two commits, one changing only `spaces/sage/space.yaml` and one changing only `settings/review.json`

#### Scenario: A README
- **WHEN** a teammate edits `README.md` in a storage repository on GitHub
- **THEN** the next pass pulls it and no commit ever changes it

#### Scenario: A Space file that cannot be compared
- **WHEN** a pass of a repository holding settings and Sage cannot read Sage's folder
- **THEN** the settings changes are committed and pushed, Sage's files stay as they are, and only the Space rule set reports the failure

### Requirement: The settings clone moves to the root
At the first start of this release, a machine with a settings repository SHALL move its clone from `<data folder>/shared/` to `<data folder>/repos/github.com/<owner>/<name>/` and record the new path, so that the shared folder is that clone. A machine without one SHALL keep `<data folder>/shared/` as a plain folder. An interrupted move SHALL be finished at the next start, and no shared setting SHALL change. Sync's record from a release that kept one record SHALL become the settings repository's record, once.

#### Scenario: A machine with a settings repository
- **WHEN** Wiffletree starts after the update with `MHohlios/wiffletree-storage` as its settings repository
- **THEN** the clone is at `<data folder>/repos/github.com/MHohlios/wiffletree-storage/`, `<data folder>/shared/` is gone, and every shared setting reads as before

### Requirement: Files are scanned before they are pushed
Before pushing a commit that changes files under `spaces/`, or under `issues/` or `wiffletree-issues.yaml`, the host SHALL run the brain gate's `sensitive-info` and `secret-scan` checks on those files on this Mac, at the gate version this release bundles, with the gate's default rules and inline allow markers. A file that fails SHALL be held: it SHALL NOT be pushed, this Mac SHALL keep using it, and a workspace note SHALL name the file, the check and the rule, never the matched text, offering to edit it. Other files SHALL keep syncing. A check that cannot run SHALL hold the file in the same way, naming what is missing. Local storage repositories, which are never pushed, SHALL NOT be scanned.

#### Scenario: A phone number in an issue
- **WHEN** the human files an issue whose description holds a phone number in a storage repository on GitHub
- **THEN** the issue stays on this Mac, is not pushed, and a workspace note names its file and the `sensitive-info` rule without the number

#### Scenario: gitleaks missing
- **WHEN** a Space's instructions change on a Mac where `secret-scan` cannot find gitleaks
- **THEN** `space.yaml` is held, and the note says gitleaks is missing
