## ADDED Requirements

### Requirement: Issue assistant sessions belong to a tracker
An issue assistant session SHALL belong to its tracker's host-owned project, not to any coordinator's project or ticket. No coordinator tool SHALL create, message, archive or resume it. It SHALL run a turn only when the human sends it a message, SHALL keep its conversation while its tracker is chosen, and SHALL be archived with its conversation kept when the human removes the tracker from this Mac. Stopping a tracker's sync SHALL NOT stop a turn already running.

#### Scenario: A coordinator archives an assistant
- **WHEN** a coordinator calls `archive_agent` with an issue assistant's session id
- **THEN** the host refuses it, as for any session that is not an agent of the caller's tickets

#### Scenario: Removing a tracker
- **WHEN** the human removes tracker CEL from this Mac
- **THEN** its assistant conversations are archived and kept, and no assistant turn starts for it again
