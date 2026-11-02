## MODIFIED Requirements

### Requirement: A project uses its Space's repositories
A project SHALL use a repository when any of these holds: its Space lists the repository and it is in this Mac's workspace, matched by GitHub identity; the human added it to this project; or an open ticket of this project works there, whether or not the project is archived. The first SHALL be worked out when it is read, so that a Space edit, including one synced from another Mac, takes effect with no write to projects. A project SHALL NOT use fewer repositories than its Space lists on this Mac, and no setting SHALL give a project every workspace repository. Only the human SHALL change a project's added repositories or a Space's list. New tickets in a repository an open ticket holds SHALL be allowed until the last open one there closes, when it SHALL leave the project unless it was added.

#### Scenario: A repository joins a Space
- **WHEN** the human adds `Vyttle-LLC/wiffletree` to Vyttle, or that edit arrives from another Mac
- **THEN** every project in Vyttle can create a ticket there at once, and no project is written

#### Scenario: A repository leaves a Space
- **WHEN** the human removes a repository from a Space while a project in it has an open ticket there
- **THEN** that ticket's agents and reviewers still start, a new ticket there is allowed, and once the last open ticket there closes the repository leaves the project unless it was added

#### Scenario: A repository in no Space
- **WHEN** a project needs a repository that no Space lists
- **THEN** it reaches it only through Add to this project, or a coordinator's `request_repository` the human approves in the Inbox, and moving Space keeps it

#### Scenario: An addition the Space later lists
- **WHEN** the human added a repository, its Space later lists it, the human edits the project's other additions, and the Space then drops it
- **THEN** the project still uses it

#### Scenario: A repository in several Spaces
- **WHEN** two Spaces list a repository
- **THEN** a project reads only its own Space's list

#### Scenario: A Space entry not cloned here
- **WHEN** a Space lists a repository not cloned on this Mac
- **THEN** Overview shows it as not cloned here and it is unusable, until it joins the workspace, when it appears by itself

#### Scenario: A Space that lists nothing here
- **WHEN** a project's Space lists no repository on this Mac and nothing was added to the project
- **THEN** the project uses nothing, and Overview says so and says to add to the Space or the project
