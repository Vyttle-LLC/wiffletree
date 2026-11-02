## MODIFIED Requirements

### Requirement: Human-only commands need the app's credential
On its own socket, the host SHALL run the commands only the human should give, namely sending a message as the human (`Send` with no sender), opening or cancelling pairing mode, turning remote access on or off, changing or removing a device's access, answering the Inbox (`ResolveAttention`, `DismissAttention`, which also approve a publish or a merge), granting publish or push authority without the Inbox (`SetOwnerAllowed`, which lets an owner's PRs open without asking, `SetKeeper`, whose rebases the PR keeper pushes, `SetProtectedBranches`, whose removed entries the host may then merge into, and `SetUpSettingsRepository` and `JoinSettingsRepository`, which choose a repository the host pushes settings to), creating, editing, removing or joining a Space, choosing its repository or setting up its issues, which choose a repository the host pushes Space files and issues to, writing to a tracker (creating, editing, commenting on, moving, assigning or linking an issue, applying a proposal from an assistant thread, settling a paused issue file and importing from Linear), turning compiling on or off for this Mac and setting a wiki's compile time, Protect history, Bring in a brain, Move into the folder, and acting on a held entry or an alert (Edit it, Not sensitive, Discard), only for a connection whose `Hello` carried the app's client credential. For a signed build's launchd host, the host SHALL create that credential at its first start and keep it in a login Keychain item whose access list lets only the signed Wiffletree app and the host read it without asking; any other program SHALL get it only after the human enters the login keychain password in the dialog macOS shows. For a child host, the app SHALL create a fresh credential each launch and pass it through the child's standard-input pipe, never in its environment or in a file. An unsigned development build SHALL always start a child host, never a launchd job. Every other command SHALL work without it. The agents' tool socket SHALL keep requiring each turn's own token.

#### Scenario: An agent opens the host socket
- **WHEN** an agent's shell connects to `host.sock`, sends `Hello` without the credential and answers an Inbox question
- **THEN** the host refuses the answer and the question stays open

#### Scenario: An agent writes as the human
- **WHEN** a connection without the credential sends `Send` with no sender to a coordinator
- **THEN** the host refuses it and stores no message

#### Scenario: The human writes
- **WHEN** the app, which sent the credential in `Hello`, sends the same message
- **THEN** the host stores it

#### Scenario: An agent allows an owner
- **WHEN** a connection without the credential sends `SetOwnerAllowed` for an owner
- **THEN** the host refuses it and the owner is not allowed

#### Scenario: An agent removes a protected branch
- **WHEN** a connection without the credential sends `SetProtectedBranches` without `release/*`
- **THEN** the host refuses it and `release/*` stays protected

#### Scenario: The app answers
- **WHEN** the app, which sent the credential in `Hello`, answers the same question
- **THEN** the host records the answer

#### Scenario: An unsigned build
- **WHEN** an unsigned development build of the app opens with the default data folder
- **THEN** it starts a child host, loads no launchd job, writes no file holding the credential, and passes the credential through the child's standard-input pipe

#### Scenario: Other commands
- **WHEN** a connection without the credential asks for the snapshot
- **THEN** the host answers it

#### Scenario: An agent edits a Space
- **WHEN** a connection without the credential sends a command that changes the Sage Space's instructions or storage repository
- **THEN** the host refuses it and the Space is unchanged

#### Scenario: An agent comments on an issue
- **WHEN** a connection without the credential sends a command that comments on CEL-151 or applies a thread's proposal
- **THEN** the host refuses it and the tracker is unchanged

#### Scenario: An agent reads issues
- **WHEN** a connection without the credential asks for the Issues list
- **THEN** the host answers it

#### Scenario: An agent turns compiling on
- **WHEN** a connection without the credential sends a command that turns compiling on for this Mac, so that a model subscription would be spent
- **THEN** the host refuses it, and no file is written to `compilers/`

#### Scenario: An agent releases a held entry
- **WHEN** a connection without the credential sends Not sensitive for a held entry, or for an alert
- **THEN** the host refuses it, the entry stays held, and the alert stays open

#### Scenario: An agent changes a repository
- **WHEN** a connection without the credential sends Protect history, Bring in a brain or Move into the folder
- **THEN** the host refuses it, and nothing is read from the original or pushed
