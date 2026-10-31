# brain-registry Specification

## Purpose
The brain registry is how Wiffletree knows and writes the brains its Spaces name: each Mac registers every brain once, reads its shared settings from `wiffletree-brain.json` on `main`, keeps its own checkout as the only writer, and sends every change as a batch branch whose pull request auto-merges once the brain gate's required checks pass, with problems shown on the writing project's Inbox or the Brains page.

## Requirements

### Requirement: A Space registers its brain
A brain SHALL be registered on a machine when one of that machine's Spaces names it (see `spaces`, "A Space"). Each brain SHALL be one GitHub repository named `owner/name`, with its push URL, its default branch `main`, the path of the host's checkout and its sync state. Several Spaces MAY name one brain, which is registered once. A brain no Space names any longer SHALL leave the registry, after confirmation, and the host's checkout SHALL be deleted; GitHub SHALL be untouched. Only the human, by editing a Space, SHALL add or remove a brain. Agents SHALL NOT. When the human creates a Space or sets its brain, Wiffletree SHALL suggest a brain from the GitHub owner of the Space's code repositories: the brain another Space already names for that owner, or none. The suggestion SHALL be only pre-filled and never applied without the human's confirmation. Personal and unowned repositories SHALL suggest no brain. The push URL SHALL be found and used as the settings repository's is: the protocol gh is configured with is tried first, with gh's login through a credential helper given for each command only, and no token is stored, logged or put on a command line.

#### Scenario: A Space names a brain
- **WHEN** the human sets the Sage Space's brain to `Sage-Social/sagehaven-docs` and confirms
- **THEN** the host clones it into its own checkout, the Brains page lists it as the Sage Space's brain with its remote and sync state, and it survives a restart

#### Scenario: The owner suggests a brain
- **WHEN** the Vyttle Space names `vyttle-docs`, and the human creates a second Space whose code repositories belong to `Vyttle-LLC`
- **THEN** the new Space's brain is pre-filled with `vyttle-docs`, and nothing is registered until the human confirms

#### Scenario: A personal Space
- **WHEN** the human creates a Space whose only repositories belong to `MHohlios`
- **THEN** no brain is suggested

#### Scenario: An agent cannot add a brain
- **WHEN** an agent's tool call or message asks the host to add or remove a brain
- **THEN** no tool does this, and the registry is unchanged

### Requirement: Naming a brain grants only its batch pushes
Confirming a brain for a Space SHALL be what allows the host to push to that repository, and the confirmation SHALL say so, naming the repository. The grant SHALL allow exactly these operations on that repository: pushing branches named `wiffletree/brain/<login>/<batch-id>`, opening pull requests from them into `main`, turning on auto-merge for those pull requests, closing a batch pull request the human discards, and deleting a branch the host pushed. The allowed owners for ticket PRs SHALL NOT grant this, and adding a brain SHALL NOT allow ticket PRs or settings sync to that repository or its owner. The host SHALL NOT approve, comment on, label or directly merge any pull request.

#### Scenario: An allowed owner without a brain
- **WHEN** `Sage-Social` is allowed for ticket PRs and no Sage brain is registered
- **THEN** the host pushes nothing to `Sage-Social/sagehaven-docs`

#### Scenario: A brain does not allow ticket PRs
- **WHEN** `sagehaven-docs` is a registered brain and a ticket in `sagehaven-docs` is published
- **THEN** the publish follows `ticket-publishing`'s rules for its owner, unchanged by the brain

### Requirement: The host owns a separate checkout of each brain
The host SHALL keep its own clone of each brain at `<data folder>/repos/github.com/<owner>/<name>/`, under the one root of `shared-repositories`, and SHALL NOT use a folder the human chose as that checkout. A brain SHALL NOT also be a storage or settings repository on this Mac, and the checkout SHALL NOT be synced by that module's engine. In that checkout the host SHALL only fetch and fast-forward `main`, and SHALL make every change on a batch branch in a worktree of it. It SHALL write a `.git/wiffletree-managed` marker in the checkout. One host process SHALL write to a checkout at a time. When the checkout's `main` cannot be fast-forwarded, or a file in it differs from what the host last wrote or fetched, the host SHALL stop writing to that brain, keep the change, and raise one item on the Brains page. It SHALL NOT reset, stash or overwrite.

#### Scenario: Fetch after a merge
- **WHEN** a batch PR merges on GitHub
- **THEN** the host fetches and fast-forwards its checkout's `main`, and its next batch starts from that commit

#### Scenario: An edit in the host's checkout
- **WHEN** a file in the host's checkout of `sagehaven-docs` is changed outside Wiffletree
- **THEN** the host stops writing to that brain, leaves the file as it is, and the Brains page names the file and says writing is held

#### Scenario: Where the checkout lives
- **WHEN** the Sage Space's brain `Sage-Social/sagehaven-docs` is registered
- **THEN** the host's checkout is `<data folder>/repos/github.com/Sage-Social/sagehaven-docs/`, beside the storage clones

#### Scenario: The human's own clone
- **WHEN** the human edits `~/dev/sagehaven/sagehaven-docs` in Obsidian
- **THEN** the host's checkout and its batches are unaffected, and the human's edit reaches the host only after it merges on `main`

### Requirement: A project's brain comes from its Space
A project SHALL store no brain of its own. A project's brain SHALL be its Space's brain, or none when its Space names none. A project a coordinator creates with `create_project` SHALL belong to the creating project's Space and SHALL use that Space's brain. Assistant threads and job runs SHALL use their own Space's brain. Capture, compile, search and `query_brain` SHALL resolve the brain this way and in no other way. A project whose Space has no brain SHALL have none, and its Memory panel SHALL say so with a link to Settings › Spaces.

#### Scenario: A project in the Sage Space
- **WHEN** a project in the Sage Space works in repositories owned by `Sage-Social` and by `Vyttle-LLC`
- **THEN** its only brain is the Sage Space's brain, `sagehaven-docs`

#### Scenario: A project created by a coordinator
- **WHEN** a coordinator in the Vyttle Space creates a project with `create_project` covering every repository
- **THEN** the new project is in the Vyttle Space and logs only to `vyttle-docs`

#### Scenario: No brain
- **WHEN** a project's Space names no brain
- **THEN** the project has no brain and its Memory panel says so, with a link to Settings › Spaces

### Requirement: The brain config file lives in the brain repository
Each brain's settings that every install must agree on SHALL be read from `wiffletree-brain.json` at the root of the brain repository's `main`: its version, the brain folder (default `second-brain`), the compiler's host id and display name, the compile time and timezone, and the `gate` object that `brain-gate` defines. The host SHALL read only the version merged on `main`, never one on an open branch. A brain without the file SHALL accept log batches, have no compiler, and offer Set up. Set up SHALL open one PR adding the file with this host as compiler, together with the gate's caller, config and `CODEOWNERS` lines as `brain-gate` specifies; that PR waits for the code owner's review. The host SHALL refuse a file that is not valid JSON or names a brain folder outside the repository, and its page SHALL say why. A version newer than the host understands SHALL allow log batches and SHALL NOT compile.

#### Scenario: Reading the compiler
- **WHEN** `main` of `sagehaven-docs` has `wiffletree-brain.json` naming Michael's Mac as compiler
- **THEN** every install that registers `sagehaven-docs` shows "Compiles on: Michael's Mac"

#### Scenario: No config file
- **WHEN** `vyttle-docs` has no `wiffletree-brain.json`
- **THEN** batches still open PRs, the page shows "No compiler" with Set up, and pressing Set up opens one PR adding the file and the gate's caller and config

### Requirement: Changing the compiler host goes through a PR
The Brains page SHALL let the human choose this machine, or another host by id, as a brain's compiler. The choice SHALL open a batch PR that changes only `wiffletree-brain.json`, keeping keys the host does not know. The page SHALL show the change as pending until the PR merges, and the compiler SHALL be the one on `main` until then.

#### Scenario: Moving the compiler
- **WHEN** the human makes this machine the compiler of `sagehaven-docs`
- **THEN** a PR changing only `wiffletree-brain.json` opens and auto-merges, the old compiler keeps compiling until it merges, and afterwards every install shows this machine

### Requirement: Every change reaches main through a batch PR that auto-merges
Every change the host makes to a brain SHALL be committed on its own batch branch from the current `main`, pushed to that branch, opened as one PR into `main`, and set to auto-merge with squash. Commits SHALL use the human's Git identity. The host SHALL NOT push `main`, push tags or force a push, and SHALL push only commits that contain the branch's remote head. When GitHub refuses auto-merge, the PR SHALL stay open and the host SHALL raise one item naming the repository settings to apply. The host SHALL record each batch's stage (queued, PR open, checks, auto-merging, merged, failed, discarded) and keep it across restarts.

#### Scenario: A batch merges
- **WHEN** a batch is pushed and all required checks pass
- **THEN** its PR auto-merges, its stage becomes merged, and the host fetches `main`

#### Scenario: Auto-merge is off
- **WHEN** `vyttle-docs` has auto-merge disabled and a batch PR opens
- **THEN** the PR stays open, its stage stays PR open, and one Inbox item says to enable auto-merge and protect `main` with the gate's checks

#### Scenario: Offline
- **WHEN** GitHub cannot be reached while a batch is ready
- **THEN** the batch stays queued, passes back off up to 30 minutes, and Sync now tries at once

### Requirement: Brain PRs are watched without a model
The host SHALL watch each open batch PR with read-only `gh` requests and no model, about every 60 seconds while any batch PR of that brain is open and about every 5 minutes otherwise, backing off after failures as `pr-watch` does. It SHALL read each required check's name, conclusion, and the file and line of its annotations. It SHALL NOT copy check output, titles or annotation messages into the store, a message or the UI.

#### Scenario: A failed sensitive scan
- **WHEN** `sensitive-info` fails with an annotation at line 9 of an added entry
- **THEN** the host records the check as failed with that file and line, and stores no text from the check's output

### Requirement: A failed check or conflict raises an Inbox item
When a batch PR's required check fails, its push is refused, it conflicts with `main`, or GitHub refuses auto-merge, the host SHALL raise one Inbox item on the project that wrote the batch, or on the Brains page when no project did. The item SHALL name the brain, the PR, the failed check or reason, and each reported file and line, SHALL say that auto-merge waits, and SHALL list what the human can do. It SHALL never show a matched value. A batch SHALL have at most one open item at a time. The item SHALL be replaced when the batch's state changes, and cleared when the batch merges or is discarded.

#### Scenario: The sensitive scan fails
- **WHEN** batch PR #214 from the ticket "Call relay: retry on Telnyx timeout" fails `sensitive-info` on line 9 of one entry
- **THEN** that project's Inbox shows one item naming PR #214, the check, the file and line 9, with Redact the line, Not sensitive and Discard the batch, and no matched text

#### Scenario: Checks pass after a fix
- **WHEN** the human redacts the line and every check then passes
- **THEN** the item is cleared and the batch auto-merges

### Requirement: An unmerged batch can be amended
For a batch whose PR is open and unmerged, the human SHALL be able to:
- redact a reported line, which replaces that field's value with `[removed: <rule>]`;
- mark it not sensitive with a reason, which appends the gate's inline marker `<!-- gate:allow <rule> <reason> -->` to that line;
- discard the batch, which closes its PR and deletes the branch the host pushed.

A redaction or marker SHALL be one new commit on the batch branch, pushed without force, after which the checks run again. Agents SHALL NOT amend or discard a batch. The host SHALL NOT change a file once its batch has merged.

#### Scenario: Not sensitive
- **WHEN** the human marks line 9 not sensitive with the reason "fixture call ID"
- **THEN** one commit appends `<!-- gate:allow phone fixture call ID -->` to line 9, and the checks run again

#### Scenario: After the merge
- **WHEN** a batch has merged and the human opens one of its entries
- **THEN** no redact or discard action is offered for it

### Requirement: The legacy safety-net never writes a managed brain
The host SHALL mark its checkout as managed with `.git/wiffletree-managed`, and SHALL NOT register a folder that the legacy pointer `~/.config/claude-second-brain` names as its checkout. The Brains page SHALL warn when the legacy pointer names the human's clone of a registered brain, saying that the safety-net must open PRs instead of pushing to `main` for that brain.

#### Scenario: Pointer to the human's clone
- **WHEN** the legacy pointer names `~/dev/sagehaven/sagehaven-docs` and `sagehaven-docs` is registered
- **THEN** the Brains page shows the warning, and the host's checkout under `<data folder>/repos/` is unaffected

### Requirement: Earlier brain attachments are migrated
At the first start of this release, each Space whose brain is a local folder, as the `spaces` change's upgrade left an earlier `Project.brain`, SHALL be resolved: a folder whose `origin` names a GitHub repository SHALL become that Space's brain repository. The host SHALL NOT push to it until the human confirms the grant on the Brains page. A folder without a GitHub `origin` SHALL leave its Space with no brain, and the Brains page SHALL say why. `attach_brain` SHALL be removed from the host's commands.

#### Scenario: An attached project
- **WHEN** a stored project has the brain `~/dev/vyttle/vyttle-docs`, whose origin is `Vyttle-LLC/vyttle-docs`
- **THEN** after the upgrade its Space names `Vyttle-LLC/vyttle-docs`, and the Brains page asks the human to confirm the grant before any batch is pushed

### Requirement: The Brains page lists the Spaces' brains
The sidebar's Workspace section SHALL list Brains beside Repositories. The Brains page SHALL show:
- each brain named by a Space on this machine, with the Spaces that name it, its remote, its checkout path, its sync state, its last PR, the compiler from its config file, and the next and last compile run;
- the PR gate: each batch that is not yet merged or was merged in the last day, with its kind, source and author, and its stage from queued through PR open and checks to auto-merged or failed.

Selecting a batch SHALL show its required checks. A failed batch SHALL link to its Inbox item. The page SHALL offer Sync now, Set up, changing the compiler, and a link to each Space's settings, where a brain is added or removed. The page SHALL NOT offer a repository-to-brain mapping.

#### Scenario: The gate on the page
- **WHEN** one batch is queued, one has checks running, one failed `sensitive-info` and a compile run merged this morning
- **THEN** the page lists all four with their stages, and the failed one links to its Inbox item
