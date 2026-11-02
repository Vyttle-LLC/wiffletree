## MODIFIED Requirements

### Requirement: Topics are filed in their Space's brain
The user SHALL NOT have to choose where to file anything. Each thread SHALL belong to one Space, and its log entries SHALL go to that Space's brain as per-entry files under `logs/assistant/YYYY/MM/`, with `repo: assistant`, written through `log_work` and landing straight on `main` (see `project-memory`), with the thread's id and Space kept in the host store. When a message starts a new topic, the router (see assistant-router) SHALL send it to the thread and Space it belongs to, asking the user when unsure.

#### Scenario: Personal then Vyttle
- **WHEN** the user asks about a dentist appointment and then, in the same tab, about Vyttle's pricing page
- **THEN** the second message goes to a Vyttle thread, and its log entries go to the Vyttle brain, not the personal brain

#### Scenario: Remembering in Personal
- **WHEN** the user asks a thread in Personal, on this Mac alone, to remember something
- **THEN** it logs an entry in Personal's brain, and nothing is pushed
