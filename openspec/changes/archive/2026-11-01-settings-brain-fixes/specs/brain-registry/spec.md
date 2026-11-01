## MODIFIED Requirements

### Requirement: The brain config file lives in the brain repository
Each brain's settings that every install must agree on SHALL be read from `wiffletree-brain.json` at the root of the brain repository's `main`: its version, the brain folder (default `second-brain`), the compiler's host id and display name, the compile time and timezone, and the `gate` object that `brain-gate` defines. The host SHALL read only the version merged on `main`, never one on an open branch. A brain without the file SHALL accept log batches, have no compiler, and offer Set up. Set up SHALL be a short form on the Brains page with 06:00 in the host's timezone filled in and both editable, and SHALL open one PR adding the file with this host as compiler and that compile time, together with the gate's caller, config and `CODEOWNERS` lines as `brain-gate` specifies; that PR waits for the code owner's review. The host SHALL refuse a time or timezone the compile schedule could not read, opening no PR. The host SHALL refuse a file that is not valid JSON or names a brain folder outside the repository, and its page SHALL say why. A version newer than the host understands SHALL allow log batches and SHALL NOT compile.

#### Scenario: Reading the compiler
- **WHEN** `main` of `sagehaven-docs` has `wiffletree-brain.json` naming Michael's Mac as compiler
- **THEN** every install that registers `sagehaven-docs` shows "Compiles on: Michael's Mac"

#### Scenario: No config file
- **WHEN** `vyttle-docs` has no `wiffletree-brain.json`
- **THEN** batches still open PRs, the page shows "No compiler" with Set up, and accepting Set up's form opens one PR adding the file with the compile time and the gate's caller and config

#### Scenario: A brain set up through the app is scheduled
- **WHEN** the human accepts Set up's 06:00 America/Chicago and its PR merges
- **THEN** the brain has a compile due daily at 06:00 America/Chicago on this host

#### Scenario: A time the schedule cannot read
- **WHEN** the human enters 25:00, or a timezone that does not exist, in Set up's form
- **THEN** the host refuses it, saying why, and no PR opens

### Requirement: The Brains page lists the Spaces' brains
The sidebar's Workspace section SHALL list Brains beside Repositories. The Brains page SHALL show:
- each brain named by a Space on this machine, with the Spaces that name it, its remote, its checkout path, its sync state, its last PR, the compiler from its config file, and the next and last compile run;
- the PR gate: each batch that is not yet merged or was merged in the last day, with its kind, source and author, and its stage from queued through PR open and checks to auto-merged or failed.

Selecting a batch SHALL show its required checks. A failed batch SHALL link to its Inbox item. The page SHALL offer Sync now, Set up, changing the compiler, changing the compile time, and a link to each Space's settings, where a brain is added or removed. The page SHALL NOT offer a repository-to-brain mapping.

#### Scenario: The gate on the page
- **WHEN** one batch is queued, one has checks running, one failed `sensitive-info` and a compile run merged this morning
- **THEN** the page lists all four with their stages, and the failed one links to its Inbox item

## ADDED Requirements

### Requirement: Changing the compile time goes through a PR
The Brains page SHALL let the human set a compile time and timezone for a brain with a config, whatever host compiles it. The choice SHALL open a batch PR that changes only `compile` in `wiffletree-brain.json`, keeping every key the host does not know, in `compile` too. The host SHALL refuse a time or timezone the compile schedule could not read, and the time already on `main`. The compile time SHALL be the one on `main` until the PR merges. Only the human's client SHALL give this command, and no agent tool SHALL reach it. A brain with a config and no time SHALL show the banner "No compile time. This brain compiles only on Run now or a merged ruling." with Set compile time…; a brain with a time SHALL offer Change compile time, and the form SHALL say "The new time applies once the config PR merges."

#### Scenario: A brain set up before compile times
- **WHEN** `sagehaven-docs` has a config naming a compiler and no `compile`
- **THEN** Next compile reads Not scheduled in the warning colour, and the banner offers Set compile time…

#### Scenario: Changing the time
- **WHEN** the human changes 06:00 to 02:00 America/Chicago
- **THEN** a PR changing only `compile` opens, keys the host does not know stay, and 06:00 stands until it merges
