## ADDED Requirements

### Requirement: The wiki links to issues and is not fed by them
Finished tickets and issues SHALL NOT feed the wiki: a compile run SHALL read no issue, and no issue's state SHALL be written into a wiki page by the host. The wiki SHALL link to an issue when a page names its ID (see `brain-browser`).

#### Scenario: An issue moves to Done
- **WHEN** VYT-42 moves to Done
- **THEN** no log entry is written for it and the next compile reads nothing of it; a wiki page that names VYT-42 links to the issue, which shows Done

## MODIFIED Requirements

### Requirement: A tracker's files
A tracker SHALL hold, in its repository's folder (see `shared-repositories`), `wiffletree-issues.yaml`, naming the format, the issue prefix, the seven states in order and the labels, and one folder per issue at `issues/<ID>/`. Each folder SHALL hold `issue.md`, YAML front matter with `id`, `title`, `state`, `priority`, `assignee`, `labels`, `repositories`, `blocks`, `blocked_by`, `related`, `duplicate_of`, `author` and `created`, followed by the Markdown description; `comments/<timestamp>-<login>.md`, one file per comment with `author`, `created`, optional `edited` and optional `assistant_for` front matter and a Markdown body; and `events.jsonl`, one JSON object per line with `at`, `by` and `kind` for changes of state, priority, assignee and links, work started and linked pull requests. Writes SHALL keep fields this release does not know. Front matter SHALL be written with fields in a fixed order so that unchanged fields produce no diff.

#### Scenario: A new issue on disk
- **WHEN** the human files "Theme previews look blurry on Retina displays" in a tracker with prefix CEL and it is numbered 152
- **THEN** `wiffletree/issues/CEL-152/issue.md` holds the title, `state: triage`, the human's login as `author` and the description, and `events.jsonl` is absent or empty

#### Scenario: A field from a newer release
- **WHEN** an `issue.md` has a front matter field `estimate: 3` and the human changes its priority
- **THEN** the written file still has `estimate: 3`, and only the `priority` line changed

### Requirement: The tracker is the Space's storage repository
A Space's tracker SHALL be its repository (see `spaces`, "A storage repository is also the Space's tracker"): the Spaces on this Mac decide which trackers it has, and Spaces sharing a repository share its tracker and prefix. There SHALL be no Trackers section in Settings and no list of trackers kept apart from Spaces. A tracker's repository, who can read it, its prefix and its sync status SHALL be shown in its Spaces' Repository box, with the human's login as `gh` reports it. Choosing, checking and setting up the repository, and its prefix, SHALL follow `spaces`. A local repository SHALL be a local tracker that is never pushed.

#### Scenario: An organization's tracker
- **WHEN** the human chooses `celadon-theme/storage` as a Space's repository and GitHub reports the organization's base permission as Read
- **THEN** the Space's form says in yellow that every member of celadon-theme can read it and its issues, before anything is written

#### Scenario: A personal tracker
- **WHEN** the human files an issue in Personal, whose repository is local
- **THEN** the issue is written to the local repository and never pushed

### Requirement: Sync pushes only the tracker's files to its default branch
`wiffletree-issues.yaml` and `issues/` in the folder SHALL be the issue rule set of their repository's one sync engine (see `shared-repositories`), sharing its clone, reach and push rules with the repository's other rule sets. Choosing a GitHub repository for a Space SHALL be what allows the host to push issues to it, and the confirmation SHALL say so. The host SHALL push only the recorded default branch, to the recorded URL, with no tags and no submodules, only a commit that contains the remote head, and never by force. An issue commit SHALL change only `wiffletree-issues.yaml` and files under `issues/`, in the folder. Before pushing, the files SHALL pass the scans of `shared-repositories`, "Files are scanned before they are pushed". Git SHALL reach the repository as the settings repository does, with `gh`'s login through a credential helper given for each command and no change to Git's configuration. Commits SHALL use the human's Git identity, and their messages SHALL name the issue and what changed.

#### Scenario: Other files in the tracker
- **WHEN** the repository also has a `README.md` a teammate edits on GitHub
- **THEN** sync pulls the edit and never commits a change to `README.md`

#### Scenario: An issue and a Space edit together
- **WHEN** the human comments on SAGE-151 and edits Sage's description within 2 seconds
- **THEN** one commit changes only the comment's file and another only `wiffletree/spaces/sage/space.yaml`

#### Scenario: The remote moved first
- **WHEN** a teammate pushed after this Mac's last pull and this Mac pushes a comment
- **THEN** GitHub refuses the push, nothing is overwritten, and the same pass fetches, merges and pushes again

### Requirement: IDs are allocated when an issue reaches the tracker
A new issue SHALL be numbered when it is pushed: one more than the highest number with the tracker's prefix that any `issues/<ID>/` path has had in the default branch's history up to the fetched remote head, at the folder or, before the repository's move, at its root, so that a deleted issue's number is never handed out again. Until then it SHALL carry a temporary ID, shown as such. When a push is refused because the remote moved, the pass SHALL fetch, merge, renumber this Mac's new issues and rewrite their temporary IDs in every file this Mac wrote, then push again, up to 5 times per pass. A number once pushed SHALL never change or be reused. A local tracker, which is never pushed, SHALL number an issue when it is filed, by the same rule over its own history.

#### Scenario: Two people file at once
- **WHEN** two Macs file an issue in the same minute against a tracker whose highest issue is CEL-151, and the other Mac pushes first as CEL-152
- **THEN** this Mac's push is refused, the pass fetches, numbers its issue CEL-153 and pushes it

#### Scenario: A deleted issue's number
- **WHEN** a tracker has CEL-151 and CEL-152, a teammate deletes `issues/CEL-152/` in an ordinary commit, and a Mac that never saw CEL-152 files an issue
- **THEN** the new issue is numbered CEL-153, and CEL-152 is not reused

#### Scenario: A number deleted before the move
- **WHEN** CEL-152 was deleted from the repository's root, the repository then moved into `wiffletree/`, and CEL-151 is the highest issue left
- **THEN** the next issue is numbered CEL-153

#### Scenario: Filed offline
- **WHEN** the human files an issue with no network and links it from CEL-151 as related
- **THEN** the issue shows a temporary ID, and when it is pushed as CEL-154 the folder, its `id` and CEL-151's `related` field all say CEL-154

### Requirement: Import from Linear
A Space's Repository box SHALL offer Import from Linear… for its tracker. It SHALL take a Linear personal API key, which the host SHALL keep in the Keychain only until the import ends and SHALL NOT log; list the Linear teams the key reads; and, for the team the human chooses, show how many issues and comments would be written and how Linear's states map to the seven states before anything is written. The import SHALL write issues, comments, labels, priorities, assignees mapped to GitHub logins the human confirms, and relations, in commits of at most 100 issues, keeping Linear's number when the prefix matches the team's key and the number is free. It SHALL record each issue's Linear identifier in its front matter, so that running it again adds only issues not yet imported. It SHALL read Linear only and SHALL NOT write to it, and there SHALL be no ongoing sync.

#### Scenario: Importing a team
- **WHEN** the human imports Linear team CEL with 240 issues into an empty tracker with prefix CEL
- **THEN** three commits add 240 issues keeping their numbers, the API key is removed from the Keychain, and nothing in Linear changes

#### Scenario: An unmapped Linear user
- **WHEN** a Linear comment's author has no confirmed GitHub login
- **THEN** the comment is written with the importing human's login as author and a first line naming the Linear author

### Requirement: The limits of shared issues are documented
The README SHALL say that issues live in a Space's repository, a private GitHub repository the team chooses or a local one that stays on the Mac, in a folder Wiffletree keeps to; that choosing it is what lets Wiffletree push to it; that issue text is scanned for personal data and secrets before it is pushed and again by each Mac that pulls it; that a slip stays in Git history until someone cleans it; that issues can be read by everyone with access, including an organization's members; that anyone who can push can edit any issue; that tickets, agents and conversations are never written to it; and that an agent's shell can still push to it with the human's credentials, which sync does not spread.

#### Scenario: README
- **WHEN** a reader looks up shared issues in the README
- **THEN** it says where issues live, who can read and edit them, that they stay in history, and what never leaves the Mac
