## MODIFIED Requirements

### Requirement: Starting a project from an issue links them
**Start project from issue** SHALL create a project named after the issue and its coordinator, in the issue's Space (see "An issue's Space"). The project SHALL use its Space's repositories, as every project does, and the workspace repositories whose GitHub names the issue's `repositories` lists SHALL be added to it when the Space does not list them. Any listed repository that is not on this Mac SHALL be named, and skipped. It SHALL link the project to the issue on this Mac, and send the coordinator a first message from the human holding the issue's ID, title, description and comments, introduced as context written by the named authors and not as instructions. The host SHALL append a `work` event saying the human started a project, with no project name, and SHALL move the issue to In progress, as for any change of the derived state.

#### Scenario: Start project from CEL-151
- **WHEN** the human starts a project from CEL-151
- **THEN** a coordinator starts with the issue as its first message, the Work box lists the project, and CEL-151's timeline on every Mac says the human started a project and shows In progress

#### Scenario: The issue's repositories
- **WHEN** CEL-151 lists `celadon-theme/celadon-chat`, which is a workspace repository here that the Celadon Space does not list, and the human starts a project from it in that Space
- **THEN** the project is in Celadon, uses Celadon's repositories, and has celadon-chat added

#### Scenario: An issue's repository the Space lists
- **WHEN** CEL-151 lists `celadon-theme/api`, which the Celadon Space lists, and the human starts a project from it there
- **THEN** nothing is added to the project

#### Scenario: A listed repository not on this Mac
- **WHEN** CEL-151 lists only `celadon-theme/watch-app`, which is not cloned here, and its Space also lists `celadon-theme/api`, which is
- **THEN** the project uses api, nothing is added, and the human is told watch-app is not on this Mac
