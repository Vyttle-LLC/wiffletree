## ADDED Requirements

### Requirement: What is protected
A branch of a repository SHALL be protected when any of these holds: it is the repository's default branch on GitHub; it is the branch of `origin` that the repository's configured base tracks; it matches an entry on the repository's protected-branches list; or GitHub reports that branch protection or a ruleset applies to it. The default branch and the base SHALL be protected without an entry and SHALL NOT be removable. When the base is changed, the old base SHALL stay protected only when it is still the default branch, matches an entry or is protected by GitHub. A base that does not track `origin` SHALL protect no branch.

#### Scenario: Default branch and base
- **WHEN** `Sage-Social/sagechat-cortex` has `main` as its default branch on GitHub, its base is `origin/develop`, and its list is empty
- **THEN** `main` and `develop` are protected and no other branch is

#### Scenario: The base changes
- **WHEN** the human changes that repository's base from `origin/develop` to `origin/main`
- **THEN** `develop` is no longer protected, unless an entry or GitHub protects it

#### Scenario: A branch GitHub protects
- **WHEN** a ruleset on GitHub applies to `release/2026.11` and no entry matches it
- **THEN** `release/2026.11` is protected

### Requirement: Entries are exact names or glob patterns
An entry SHALL be an exact branch name or a glob pattern, matched case-sensitively against the whole name of a branch on `origin` without `refs/heads/`. In a pattern, `*` SHALL match any run of characters except `/`, `**` any run including `/`, `?` one character except `/`, and `[...]` one character of a class, negated by a leading `!` or `^`. The host SHALL refuse an entry, saying why, when it is empty or holds whitespace, when a class is never closed, when it starts with `refs/`, when its first segment names one of the repository's remotes, when an exact name is not a valid branch name, and when it is already on the list or names a branch that is always protected.

#### Scenario: A pattern
- **WHEN** the list holds `release/*`
- **THEN** `release/2026.10` is protected and `release/2026/10` and `releases/x` are not

#### Scenario: Across slashes
- **WHEN** the list holds `release/**`
- **THEN** `release/2026/10` is protected

#### Scenario: An unclosed class
- **WHEN** the human adds `release/[0-9`
- **THEN** it is refused, saying `[` is never closed, and the list is unchanged

#### Scenario: A remote branch
- **WHEN** the human adds `origin/main`
- **THEN** it is refused, saying to name a branch, not a remote branch, and that `main` is already protected

### Requirement: The list stays on this machine and only the human changes it
Each repository's entries SHALL be stored with the repository's record on this machine and SHALL NOT be written to the settings repository or a Space. Only the human's client SHALL be able to change them. No workspace tool SHALL add or remove them. A repository stored before this change SHALL load with no entries.

#### Scenario: An agent cannot change the list
- **WHEN** an agent lists the workspace tools
- **THEN** none of them changes a repository's protected branches

#### Scenario: A second machine
- **WHEN** a machine joins a settings repository from a machine whose repository lists `release/*`
- **THEN** its own repositories' lists are unchanged

### Requirement: The Repositories page shows each repository's protected branches
Each repository row on the Repositories page SHALL have a Protected branches block, collapsed to one line naming the protected branches until opened. Opened, it SHALL show the default branch and the base with a lock and no remove button, each saying why it is always protected; each branch or pattern GitHub protects, marked as read from GitHub and with no remove button; and each entry of the human's with the branches on `origin` it matches now, or that none matches yet, and a remove button. It SHALL have a field to add an entry, showing a refused entry's reason under it, and SHALL say which merge method the next merge would use. It SHALL say that only the human merges into these branches, and that the list stays on this machine. In a repository that does not publish without asking, it SHALL say that Wiffletree merges nothing there. When GitHub could not be read, the block SHALL say so and SHALL show the stored entries and the base.

#### Scenario: The rows
- **WHEN** the human opens the block of `Vyttle-LLC/wiffletree`, whose default branch and base are `main` and whose list holds `release/*`, with `release/2026.10` and `release/2026.11` on origin
- **THEN** it shows `main` with a lock and no remove button, and `release/*` matching those two branches with a remove button

#### Scenario: An owner that is not allowed
- **WHEN** the human opens the block of a repository whose owner is not allowed
- **THEN** it says that Wiffletree merges nothing there

### Requirement: Agents are told each repository's protected branches
`workspace_context` SHALL show, for each repository the project uses, its protected branches and entries, and whether `merge_ticket` can merge there at all.

#### Scenario: Context
- **WHEN** the coordinator reads `workspace_context` for a project using `Vyttle-LLC/wiffletree`
- **THEN** it sees that `main` and `release/*` are protected and that merging is possible there into other branches
