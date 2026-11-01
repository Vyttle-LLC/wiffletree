## ADDED Requirements

### Requirement: A Space's storage can move
The Storage and issues box of every Space's page SHALL offer Move storage…, opening a dialog that offers This Mac or A GitHub repository, starting on the side the Space is not on. For GitHub it SHALL use New Space's check, SHALL keep Move disabled until a check passes, and SHALL check again at the moment of Move. After a check it SHALL say who can read the target and what moves, naming every Space stored in the same repository. All the Spaces on this Mac in one storage repository SHALL move together, because they share its tracker. Only the human SHALL move storage: no agent, thread, job run or workspace tool SHALL start a move.

#### Scenario: Two Spaces in one repository
- **WHEN** Sage and Sage Watch share a storage repository and the human moves Sage
- **THEN** the dialog names both, and both move

#### Scenario: An agent tries to move storage
- **WHEN** an agent's socket sends `move_storage`
- **THEN** it is refused as a command only the human gives

### Requirement: What a move refuses
A move SHALL be refused, with its own sentence and with nothing written, for: a public repository; a repository that is a brain on this Mac, naming the brain's Space; a prefix another storage repository on this Mac uses, not counting the storage being moved; a target with a tracker when the storage being moved has issues; a target already holding a Space in the same folder under `spaces/`; storage that is also this Mac's settings repository; a job run in progress in a Space that would move; an import from Linear running; and issue changes of a GitHub storage repository not yet pushed. Storage with no issues SHALL take on the target's tracker and prefix. A target SHALL NOT be refused for holding only what this Mac pushed in a move of the same storage that ended before it switched.

#### Scenario: A brain
- **WHEN** the human checks `Vyttle-LLC/vyttle-docs`, the Vyttle Space's brain
- **THEN** the dialog says it is Vyttle's brain, Move stays disabled and nothing is written

#### Scenario: No issues, and a target with a tracker
- **WHEN** a local Space with no issues moves to a repository whose tracker uses SAGE
- **THEN** the Space takes on SAGE and the local tracker's prefix is gone

#### Scenario: A move cut short, tried again
- **WHEN** a move's push reached GitHub, the host stopped before the switch, and the human moves the Space to that repository again
- **THEN** the folder and tracker it pushed do not refuse the move, and only what changed since is pushed

### Requirement: A move is all or nothing
A move SHALL copy the files, run the `sensitive-info` and `secret-scan` checks over them, push them, and only after GitHub has them switch the Space. A held scan or a failed push SHALL leave the Space where it was and the dialog SHALL say why with no match text. It SHALL never force a push, and SHALL write only the target's default branch. No sync pass SHALL run during a move. A file of the storage changed during the move SHALL end it with the Space where it was. Every reference to the storage repository SHALL change in one transaction, which SHALL also record the folder renames still to make, and the next start SHALL finish those, so a stop at any step ends fully moved or fully not moved. Issue IDs, read state, derived state, links, jobs and boards SHALL carry on unchanged. A "One runner" job with no runner SHALL name this Mac as it moves to GitHub.

#### Scenario: A local Space moves to GitHub
- **WHEN** a local Space with issues, a job and a board moves to an empty private repository
- **THEN** the page shows the repository, its readers and Sync now, and the issues keep their IDs and read state, and the job and board carry on

#### Scenario: A held file
- **WHEN** the scan finds a possible token in an issue during a move
- **THEN** nothing is pushed, the Space stays where it was, and the dialog names the file and the rule, never the match

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

## MODIFIED Requirements

### Requirement: The Spaces page
Settings SHALL have a Spaces section listing every Space with its name, whether it is shared, its storage and brain, and its repositories. A Space's page SHALL edit every field of the Space and SHALL show a Storage and issues box with the storage repository, the Space's folder in it, the tracker's prefix and issue count, who can read the storage repository and therefore its issues, the other Spaces on this Mac that share the storage repository, whether it is also this Mac's settings repository, its sync status, Sync now and Move storage…. For a shared Space the page SHALL say "Anyone who can push to ‹repository› can change these instructions for everyone in this Space". The section SHALL offer Add Space with "New" and "From a repository".

#### Scenario: A shared Space's page
- **WHEN** the user opens the Sage Space, whose storage is `Sage-Social/wiffletree-storage` with prefix SAGE
- **THEN** the page says every member of Sage-Social can read it and its issues, shows prefix SAGE, and says that anyone who can push to it can change these instructions for everyone in this Space

#### Scenario: A personal Space's page
- **WHEN** the user opens Personal, whose storage is local
- **THEN** the Storage and issues box says the storage is local with no remote and that only the user, on this Mac, can read its issues
