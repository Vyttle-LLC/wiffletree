## MODIFIED Requirements

### Requirement: Only the user creates or edits a Space
Spaces SHALL be created, edited and removed only on the Spaces page of Settings, by the user, apart from the Spaces the host creates at upgrade, the default "Personal" Space, and the repositories a rule the human set adds (see "A Space can follow root folders"). No agent, the Assistant, issue thread, job run or workspace tool SHALL create, change or remove a Space or read its machine-local facts. The Assistant core SHALL say not to edit a Space's files in its storage clone, as an instruction the host does not enforce, as for every YOLO agent. Removing a Space SHALL ask first, SHALL be refused while a non-archived project belongs to it, an Assistant work item included, or when it is the last Space, SHALL move its archived projects to the "Personal" Space, creating it if it was removed, and SHALL leave its repository unchanged. Removing "Personal" itself SHALL be refused while archived projects belong to it, because they would have nowhere to go.

#### Scenario: A fresh install
- **WHEN** Wiffletree starts with no Space, on a new Mac or after an upgrade with no projects
- **THEN** the host creates a Space named "Personal" on this Mac alone, with issues, logs and a wiki, so New project has a Space

#### Scenario: The last Space
- **WHEN** the user removes the only Space left
- **THEN** the removal is refused, saying at least one Space must exist

#### Scenario: An agent asks to change instructions
- **WHEN** a coordinator or the Assistant is asked to add a rule to a Space's instructions
- **THEN** no tool lets it, the Assistant does not edit the Space's file, and it can only suggest the user edit the Space in Settings

#### Scenario: Removing a Space with a project
- **WHEN** the user removes a Space that one open project belongs to
- **THEN** the removal is refused, naming the project

#### Scenario: Removing a Space with running Assistant work
- **WHEN** the user removes Celadon while an Assistant work item runs in it
- **THEN** the removal is refused, naming the work item

#### Scenario: Removing a Space with only archived projects
- **WHEN** the user removes the Woodworking Space, whose two projects are archived, and confirms
- **THEN** the Space is removed, both archived projects belong to Personal, and restoring one later puts it in Personal

#### Scenario: Removing Personal with archived projects
- **WHEN** the user removes Personal, which holds one archived project, while Sage also exists
- **THEN** the removal is refused, saying its archived projects would have nowhere to go, and nothing changes

### Requirement: Sources a Space allows
A Space's sources SHALL list the MCP servers and local CLIs its loops and the Assistant may use, in the format job recipes use (see `scheduled-jobs`), each with its read tools or command prefixes and, optionally, its act tools or command prefixes, some of which the human may mark "Do without asking". Tools and prefixes saved before act lists existed SHALL read as read tools. The Space's page SHALL edit them. Job runs in the Space SHALL use only read tools and prefixes, as their recipe allows; act lists and marks SHALL never reach a loop. The Assistant SHALL use every Space's sources as assistant says ("The Assistant uses apps through Space sources", "Reading is free; acting needs a card"). For an MCP server reached by URL, the Assistant's first turn that meets a tool Wiffletree has not seen SHALL stop before the model acts and start again with that tool denied, and its act tools SHALL be refused for now while its read tools work. Issue threads SHALL use no source. Only the human's client SHALL change a Space's sources, act lists or marks.

#### Scenario: Linear in Sage
- **WHEN** the user adds the Linear MCP server with its list and read tools to Sage's sources
- **THEN** `space.yaml` lists the server and those tools, no other Space's sources change, and the Assistant's next turn is offered those Linear tools and no others

#### Scenario: Gmail with an act tool
- **WHEN** the user adds a Gmail MCP server to Personal with `search_threads` and `read_thread` as read tools and `send_message` as an act tool
- **THEN** the Assistant can read mail at once, its provider is not offered `send_message`, and a send reaches Gmail only through an action card

#### Scenario: Gmail reached by URL
- **WHEN** the same Gmail source is an HTTP MCP server that also offers `delete_thread`, not seen before
- **THEN** the Assistant's first turn stops before the model acts and starts again with `delete_thread` denied, it reads mail with `search_threads` and `read_thread`, and an `act` for `send_message` is refused with no card until a later change supports acting through such a server

#### Scenario: A loop on the same source
- **WHEN** a Personal loop lists the Gmail source
- **THEN** its runs are offered `search_threads` and `read_thread` only

## ADDED Requirements

### Requirement: Connecting a source is one form
A Space's page SHALL offer Add source, which lists the MCP servers configured for this Mac's Claude and Codex and a short list of well-known CLIs found on this Mac, and lets the human add one by hand. Choosing one SHALL start it as its provider would, list its tools, and show each with a suggested kind: read when the server marks it read-only or its name only reads, act otherwise, which the human may change, untick or mark Do without asking. When the server or CLI is not signed in, the form SHALL say so and show the sign-in command or link its own documentation gives, and SHALL check again on Try again; Wiffletree SHALL NOT run an app's sign-in flow itself or store its tokens. For a CLI, the form SHALL say that its act commands wait for a card only because the Assistant is told to run them through `act`, whereas an MCP server's act tools always do. Saving SHALL write the source to `space.yaml`.

#### Scenario: Adding Google Calendar
- **WHEN** the human chooses Add source on Personal and picks a configured Google Calendar MCP server
- **THEN** the form lists its tools with `list_events` as read and `create_event` and `respond_to_invite` as act, and saving writes them to Personal's `space.yaml`

#### Scenario: Not signed in
- **WHEN** the picked CLI reports it is not signed in
- **THEN** the form shows its own sign-in command and Try again, and Wiffletree opens no sign-in page of its own
