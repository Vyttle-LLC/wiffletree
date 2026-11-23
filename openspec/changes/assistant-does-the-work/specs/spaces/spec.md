## MODIFIED Requirements

### Requirement: Only the user creates or edits a Space
Spaces SHALL be created, edited and removed only on the Spaces page of Settings, by the user, apart from the Spaces the host creates at upgrade, the default "Personal" Space, and the repositories a rule the human set adds (see "A Space can follow root folders"). No agent, the Assistant, issue thread, job run or workspace tool SHALL create, change or remove a Space or read its machine-local facts. The Assistant's sandbox (see assistant) SHALL refuse its writes to every Space's storage clone. Removing a Space SHALL ask first, SHALL be refused while a non-archived project belongs to it, an Assistant work item included, or when it is the last Space, SHALL move its archived projects to the "Personal" Space, creating it if it was removed, and SHALL leave its repository unchanged. Removing "Personal" itself SHALL be refused while archived projects belong to it, because they would have nowhere to go.

#### Scenario: A fresh install
- **WHEN** Wiffletree starts with no Space, on a new Mac or after an upgrade with no projects
- **THEN** the host creates a Space named "Personal" on this Mac alone, with issues, logs and a wiki, so New project has a Space

#### Scenario: The last Space
- **WHEN** the user removes the only Space left
- **THEN** the removal is refused, saying at least one Space must exist

#### Scenario: An agent asks to change instructions
- **WHEN** a coordinator or the Assistant is asked to add a rule to a Space's instructions
- **THEN** no tool lets it, the Assistant cannot write the Space's file, and it can only suggest the user edit the Space in Settings

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
A Space's sources SHALL list the MCP servers and local CLIs its loops and the Assistant may use, with the tools or command prefixes each may call, in the format job recipes use (see `scheduled-jobs`). The Space's page SHALL edit them. Job runs in the Space SHALL read them as their recipe allows. The Assistant's conversation SHALL be offered every Space's listed MCP servers with only their listed tools; it runs CLIs as any YOLO agent does, so a CLI listed here limits only job runs. Issue threads SHALL use no source.

#### Scenario: Linear in Sage
- **WHEN** the user adds the Linear MCP server with its list and read tools to Sage's sources
- **THEN** `space.yaml` lists the server and those tools, no other Space's sources change, and the Assistant's next turn is offered those Linear tools and no others
