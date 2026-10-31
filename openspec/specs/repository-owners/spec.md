# repository-owners Specification

## Purpose
Show which GitHub organizations and users own the workspace's repositories, and let the human choose, on each machine, the owners in whose repositories agents may open pull requests without asking.

## Requirements

### Requirement: A repository's owner comes from its origin remote
The host SHALL take each workspace repository's GitHub owner and name from the push URL of its `origin` remote, read on disk without a network request. Only HTTPS and SSH URLs on `github.com` SHALL name an owner. `origin` SHALL name an owner only when it has exactly one push URL and its fetch URL names the same GitHub repository. A repository with no `origin`, with an `origin` elsewhere, or with an `origin` that fails that test SHALL have no owner, and its row SHALL say why. Each workspace repository SHALL be counted separately, so two folders that are clones of the same GitHub repository count as two repositories of that owner.

#### Scenario: Owner from an SSH remote
- **WHEN** a repository's `origin` is `git@github.com:Vyttle-LLC/wiffletree.git`
- **THEN** its owner is `Vyttle-LLC` and its name is `wiffletree`

#### Scenario: No GitHub remote
- **WHEN** a repository has no `origin`, or its `origin` is on another host
- **THEN** it has no owner and is listed as not on GitHub

#### Scenario: More than one push URL
- **WHEN** a repository's `origin` has two push URLs, the first under an allowed owner
- **THEN** it has no owner, cannot be allowed, and its row says its `origin` pushes to more than one place

#### Scenario: Owners without GitHub access
- **WHEN** `gh` is missing or not signed in
- **THEN** every repository with a `github.com` origin still shows its owner

#### Scenario: Two clones of one repository
- **WHEN** two workspace repositories have the same `origin`
- **THEN** their owner's count includes both

### Requirement: Fork status and visibility are read from GitHub, read-only
The host SHALL ask GitHub, through `gh`, whether each repository with an owner is a fork, what it is a fork of, whether it is public, whether its owner is an organization or a user, and which owner and name GitHub reports for it. It SHALL read all of the workspace's repositories with one request, or with as few requests as their number requires, off the thread that serves host commands, and SHALL NOT write anything to GitHub. It SHALL read a repository again when its answer is missing or more than a day old, or when its `origin` has changed, and SHALL do so when repositories are added and when the human returns to the app. The owner a repository counts under SHALL be the one GitHub reports. When GitHub answers for some repositories of a request and cannot resolve another, the host SHALL record the answers it got and SHALL mark only the unresolved repository as not found, with GitHub's reason. When the read fails altogether, the previous answers SHALL be kept, repositories without an answer SHALL stay unanswered, and the Repositories page SHALL say why.

#### Scenario: One request for the workspace
- **WHEN** the workspace has 34 repositories on GitHub and none has an answer yet
- **THEN** the host makes one GitHub request and records fork status and visibility for all of them

#### Scenario: A transferred repository
- **WHEN** a repository's `origin` names owner `old-org` and GitHub reports it as `new-org/app`
- **THEN** it is listed and checked under `new-org`

#### Scenario: One repository cannot be resolved
- **WHEN** GitHub answers for 33 of the workspace's repositories and reports that the 34th does not exist or cannot be seen
- **THEN** the 33 have their fork status and visibility recorded, and only the 34th is shown as not found on GitHub and asks first

#### Scenario: GitHub cannot be read
- **WHEN** `gh` is not signed in
- **THEN** no fork status or visibility is recorded, the owners block says GitHub could not be checked and why, and nothing is treated as answered

### Requirement: The human chooses the allowed owners on each machine
The host SHALL keep a list of allowed entries in this machine's host settings. An entry SHALL be an owner's login, or `owner/name` for a single fork. The list SHALL be empty until the human adds to it. Only the human's client SHALL be able to change it; no agent tool SHALL change it. Entries SHALL compare without regard to letter case. The list SHALL NOT be synced or exported to any other machine.

#### Scenario: Nothing is allowed at first
- **WHEN** the app is updated or first started
- **THEN** no owner is allowed and every repository asks first

#### Scenario: The human allows an owner
- **WHEN** the human ticks `Sage-Social`
- **THEN** `Sage-Social` is saved in this machine's host settings and stays allowed after a restart

#### Scenario: The human removes an owner
- **WHEN** the human unticks `Sage-Social`
- **THEN** its repositories ask first from the next publish on

#### Scenario: An agent cannot allow an owner
- **WHEN** any agent calls any workspace tool
- **THEN** no tool changes the allowed list

### Requirement: Which repositories publish without asking
A repository SHALL publish without asking only when all of these hold: its `origin` names an owner; GitHub has answered for it; its base tracks `origin`; and its entry is in the allowed list. A repository that is not a fork SHALL use its owner as its entry. A fork SHALL use `owner/name` as its entry, and an allowed owner SHALL NOT cover it. Every other repository SHALL ask first. There SHALL be no per-repository override of an allowed owner.

#### Scenario: An allowed owner's repository
- **WHEN** `Vyttle-LLC` is allowed and `Vyttle-LLC/wiffletree` is not a fork
- **THEN** it publishes without asking

#### Scenario: A fork under an allowed owner
- **WHEN** `MHohlios` is allowed and `MHohlios/gpui-component` is a fork
- **THEN** it asks first until the human allows `MHohlios/gpui-component` itself

#### Scenario: GitHub has not answered
- **WHEN** an allowed owner's repository has no answer from GitHub yet
- **THEN** it asks first

#### Scenario: Base on another remote
- **WHEN** a repository's base is `upstream/main` or a local branch with no remote
- **THEN** it does not publish without asking, whatever is allowed

### Requirement: The Repositories page lists owners to allow
The Repositories page SHALL show, above the repository list, a block titled "Open pull requests without asking". It SHALL list each owner with a tick box, whether it is an organization or a user, how many workspace repositories it owns and whether they are private or public. It SHALL list each fork separately, with its own tick box, its parent and its visibility. It SHALL list repositories that are not on GitHub without a tick box. It SHALL state in one line that Wiffletree never approves, closes or comments on a pull request and never merges into a protected branch, and SHALL say that the list stays on this machine. The page SHALL call a ticked owner "allowed" and SHALL NOT call it approved or the behavior automatic. Each repository row SHALL say which rule applies to it: without asking, asks first, or not on GitHub.

#### Scenario: Owners with counts
- **WHEN** the workspace has 29 Sage-Social repositories, all private, and one public Vyttle-LLC repository
- **THEN** the block shows "Sage-Social", an organization with 29 private repositories, and "Vyttle-LLC" with 1 public repository, each with a tick box

#### Scenario: A fork is listed on its own
- **WHEN** the workspace has `MHohlios/gpui-component`, a fork of `longbridge/gpui-component`
- **THEN** it has its own row and tick box naming its parent, and it is not counted under MHohlios

#### Scenario: A repository that is not on GitHub
- **WHEN** a repository has no GitHub remote
- **THEN** it is listed without a tick box and its row says it is not on GitHub

#### Scenario: Repository row
- **WHEN** `Vyttle-LLC` is allowed and `celadon-theme` is not
- **THEN** the wiffletree row says pull requests are opened without asking and the celadon-website row says it asks first

#### Scenario: The line about what Wiffletree never does
- **WHEN** the human opens the Repositories page
- **THEN** the block says that Wiffletree never approves, closes or comments on a pull request and never merges into a protected branch

### Requirement: Agents are told each repository's rule
`workspace_context` SHALL show, for each repository the project uses, its GitHub owner and name, whether it is a fork, whether it is public, and whether it publishes without asking.

#### Scenario: An agent reads the rule
- **WHEN** an implementer in a repository whose owner is not allowed calls `workspace_context`
- **THEN** its repository entry names the owner and says it does not publish without asking
