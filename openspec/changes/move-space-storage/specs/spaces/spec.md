## ADDED Requirements

### Requirement: A Space's storage can move
The Repository box of every Space's page SHALL offer Move…, beside Move into `wiffletree/` for a repository not yet moved, opening a dialog that offers This Mac or A GitHub repository, starting on the side the Space is not on. For GitHub it SHALL use New Space's check, SHALL keep Move disabled until a check passes, and SHALL check again at the moment of Move. After a check it SHALL say who can read the target and what moves, naming every Space stored in the same repository. All the Spaces on this Mac in one storage repository SHALL move together, because they share its tracker. Only the human SHALL move storage: no agent, thread, job run or workspace tool SHALL start a move.

#### Scenario: Two Spaces in one repository
- **WHEN** Sage and Sage Watch share a storage repository and the human moves Sage
- **THEN** the dialog names both, and both move

#### Scenario: An agent tries to move storage
- **WHEN** an agent's socket sends `move_storage`
- **THEN** it is refused as a command only the human gives

### Requirement: What a move refuses
A move SHALL be refused, with its own sentence and with nothing written, for: a public repository; a prefix another storage repository on this Mac uses, not counting the storage being moved; a target with a tracker when the storage being moved has issues; a target already holding a Space in the same folder under its Wiffletree folder's `spaces/`; storage that is also this Mac's settings repository; a job run in progress in a Space that would move; an import from Linear running; and issue changes of a GitHub storage repository not yet pushed. Storage with no issues SHALL take on the target's tracker and prefix. A target SHALL NOT be refused for holding only what this Mac pushed in a move of the same storage that ended before it switched.

#### Scenario: A repository a Space names as its brain
- **WHEN** the human checks `Vyttle-LLC/vyttle-docs`, which an older Space names as its brain
- **THEN** it is checked like any other repository, and the Space may move there

#### Scenario: No issues, and a target with a tracker
- **WHEN** a local Space with no issues moves to a repository whose tracker uses SAGE
- **THEN** the Space takes on SAGE and the local tracker's prefix is gone

#### Scenario: A move cut short, tried again
- **WHEN** a move's push reached GitHub, the host stopped before the switch, and the human moves the Space to that repository again
- **THEN** the folder and tracker it pushed do not refuse the move, and only what changed since is pushed

### Requirement: A move is all or nothing
A move SHALL carry the Spaces' Wiffletree folder (see `shared-repositories`): their Space folders and, unless the Spaces take on the target's tracker, its issues, tracker, `compilers/` and `second-brain/`, read from the folder they are in and written to the folder the target holds, or else `wiffletree/`, which the Spaces then record. It SHALL copy the files, run the `sensitive-info` and `secret-scan` checks over them, push them, and only after GitHub has them switch the Space. A held scan or a failed push SHALL leave the Space where it was and the dialog SHALL say why with no match text. It SHALL never force a push, and SHALL write only the target's default branch. No sync pass SHALL run during a move. A file of the storage changed during the move SHALL end it with the Space where it was. Every reference to the storage repository SHALL change in one transaction, which SHALL also record the folder renames still to make, and the next start SHALL finish those, so a stop at any step ends fully moved or fully not moved. Issue IDs, read state, derived state, links, jobs and boards SHALL carry on unchanged. A "One runner" job with no runner SHALL name this Mac as it moves to GitHub.

#### Scenario: A local Space moves to GitHub
- **WHEN** a local Space with issues, a job and a board moves to an empty private repository
- **THEN** the page shows the repository, its readers and Sync now, and the issues keep their IDs and read state, and the job and board carry on

#### Scenario: A repository at its root moves to an empty one
- **WHEN** Sage's repository still holds its files at its root and the human moves Sage to an empty private repository
- **THEN** they land under `wiffletree/`, with its logs, wiki and `compilers/`, and Sage records the folder

#### Scenario: A target that holds a folder
- **WHEN** a local Space with no issues moves to a repository whose tracker is in `team/storage/`
- **THEN** its Space folder lands in `team/storage/spaces/`, and it takes on that tracker

#### Scenario: A held file
- **WHEN** the scan finds a possible token in an issue during a move
- **THEN** nothing is pushed, the Space stays where it was, and the dialog names the file and the rule, never the match

#### Scenario: A root check during a move
- **WHEN** a root check adds a repository to a Space that follows the root while that Space's storage moves
- **THEN** the move ends at the switch with the Space where it was and listing the repository, and moving it again carries the repository to the new storage

#### Scenario: Following after a move
- **WHEN** a Space that follows a root folder moves
- **THEN** it keeps following that root, and a repository first found there joins it in its new storage

#### Scenario: A stop at the switch
- **WHEN** the host stops after the switch's transaction and before its renames
- **THEN** the next start makes the renames and the Space is fully moved

### Requirement: A move deletes nothing
Old local storage SHALL move to the backups folder with its Git history. An old GitHub clone SHALL stay on this Mac and stop syncing, unless it has another role, which it SHALL keep. GitHub's old repository SHALL NOT be written. Moving from one GitHub repository to another SHALL say that teammates keep reading the old repository until they add the Space from the new one. Moving to this Mac SHALL make a new local repository, say that later changes on either side do not reach the other, and leave GitHub unchanged.

#### Scenario: Leaving GitHub
- **WHEN** the human moves a shared Space to this Mac
- **THEN** a new local repository holds it, GitHub's repository is unchanged, and this Mac stops syncing it

#### Scenario: The old local folder
- **WHEN** a local Space moves to GitHub
- **THEN** its old folder is in `backups/` with its commits
