## MODIFIED Requirements

### Requirement: Every project belongs to a Space
Every Wiffletree project SHALL belong to exactly one Space, shown and changeable in the project's Overview. New project SHALL show a Space, defaulting to the Space last used for a new project, and SHALL show that Space's repositories as plain rows that cannot be unticked, listing the ones not cloned on this Mac as such. Changing the Space in New project SHALL replace those rows with the new Space's and keep what the user added. The user MAY add other workspace repositories to the project with Add another repository…. A project's coordinator SHALL log to its Space's brain. Moving a project to another Space SHALL ask first, naming the repositories it gains, loses and keeps; SHALL change where its future entries go, leaving earlier entries where they are; and SHALL never be refused for its repositories or tickets.

#### Scenario: A project's log
- **WHEN** a coordinator in a project of the Vyttle Space records a decision
- **THEN** the entry goes to the Vyttle Space's brain

#### Scenario: Repositories from the Space
- **WHEN** the user opens New project with Sage last used, Sage lists `Sage-Social/mobile-app`, `Sage-Social/watch-app` and `Sage-Social/api`, and only the first two are workspace repositories here
- **THEN** mobile-app and watch-app are plain rows, api is listed as not cloned here, and no personal repository is listed

#### Scenario: Another Space
- **WHEN** the user added `simion/termic` in New project and switches its Space from Sage to Personal
- **THEN** the Sage rows give way to Personal's and termic stays, marked as added to this project

#### Scenario: Move says what it does
- **WHEN** the user moves a Personal project with one added repository and an open ticket in `pica-api` to Vyttle
- **THEN** the dialog lists what it gains from Vyttle, what it loses from Personal, and keeps the added repository and `pica-api`, naming the ticket

## ADDED Requirements

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
- **THEN** it reaches it only through Add to this project, and moving Space keeps it

#### Scenario: A repository in several Spaces
- **WHEN** two Spaces list a repository
- **THEN** a project reads only its own Space's list

#### Scenario: A Space entry not cloned here
- **WHEN** a Space lists a repository not cloned on this Mac
- **THEN** Overview shows it as not cloned here and it is unusable, until it joins the workspace, when it appears by itself

#### Scenario: A Space that lists nothing here
- **WHEN** a project's Space lists no repository on this Mac and nothing was added to the project
- **THEN** the project uses nothing, and Overview says so and says to add to the Space or the project

### Requirement: Overview shows where each repository comes from
A project's Overview SHALL group its repositories: "From <Space>" with Edit on Repositories, holding the Space's repositories here and its entries not cloned here; "Added to this project", each with Remove; and "An open ticket works here", naming the ticket, shown only when that applies. A repository SHALL appear once, in the first group that applies. The Add menu SHALL offer Add to this project…, Add repositories… and Attach one with a custom base…. Add to this project SHALL show the Space's repositories ticked and disabled and the others tickable, and SHALL NOT change the Space.

#### Scenario: Removing an added repository with an open ticket
- **WHEN** the human removes an added repository an open ticket works in
- **THEN** the repository moves to "An open ticket works here" and the ticket carries on

#### Scenario: Adding from Overview
- **WHEN** the human uses Add repositories… from a project's Overview for a repository its Space does not list
- **THEN** the repository joins the workspace and is added to that project

### Requirement: The upgrade to inherited repositories runs once
At the first start of this release, the host SHALL set each project's added repositories, archived projects included, to the repositories it named plus those of its open tickets, minus those its Space lists on this Mac. A project that used every repository named none, and an empty selection named none. The upgrade SHALL run once, in one transaction, and SHALL NOT change any Space's list. A store of any earlier schema SHALL still open.

#### Scenario: A project with every repository and open tickets elsewhere
- **WHEN** a Personal project used every repository and has open tickets in mudling, which only Vyttle lists
- **THEN** it uses Personal's repositories and has mudling added

#### Scenario: A selection inside its Space
- **WHEN** a Vyttle project named wiffletree and wiffletree-website, both of which Vyttle lists
- **THEN** nothing is added and it uses all of Vyttle's repositories on this Mac

#### Scenario: An archived project
- **WHEN** an archived Personal project has an open ticket in wiffletree
- **THEN** it has wiffletree added
