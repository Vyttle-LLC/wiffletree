## Purpose

Provide durable, provider-independent project/task ownership and message routing in a headless Rust host.

## ADDED Requirements

### Requirement: Enforced hierarchy
The host SHALL allow projects without repositories, require each ticket to reference a repository its project uses, and require workers to belong to the project coordinator of the same project and to work on one of its tickets. Multiple tickets SHALL share a repository.

#### Scenario: Invalid worker owner
- **WHEN** a worker is created under a session from another project, or a repository coordinator is created
- **THEN** the host rejects creation without persisting a partial assignment

### Requirement: Durable ordered messages
The host SHALL persist bounded messages before acknowledging queue acceptance, deduplicate stable IDs, reject changed payloads under an existing ID, preserve recipient ordering, paginate transcripts and distinguish queued, delivered, acknowledged and completed receipts.

#### Scenario: Restart and duplicate send
- **WHEN** the host restarts after accepting a message and that message is submitted again
- **THEN** exactly one local record remains and its receipt remains observable

### Requirement: Recovery and attention
The host SHALL retain pending attention bound to the originating operation and allow one terminal answer. Worker blocked state SHALL remain independent from attention. Working sessions SHALL recover as disconnected, with ambiguous accepted work held for reconciliation.

#### Scenario: Replayed approval response
- **WHEN** a resolved request receives another answer after restart
- **THEN** the stale answer is rejected and the original resolution is preserved

### Requirement: Portable local command boundary
The host SHALL expose versioned bounded commands over JSON-lines and build without GPUI. Database ownership SHALL be exclusive per store.

#### Scenario: Competing host
- **WHEN** another process attempts to open an already owned store
- **THEN** it fails visibly instead of becoming a competing writer
