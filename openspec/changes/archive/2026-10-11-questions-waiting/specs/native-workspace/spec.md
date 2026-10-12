## ADDED Requirements

### Requirement: Questions waiting beside an agent's status
An agent's status badge SHALL name the questions it still waits on after its status, as "Working · 1 question waiting" or "Blocked · 2 questions waiting", and its sidebar row SHALL show the short figure "1 question" or "2 questions". The badge text SHALL use the status's own color and the sidebar figure the subtle color, never yellow, because a worker's question goes to its coordinator and yellow is reserved for what needs the human. The question's text SHALL NOT be repeated beside the status. A session with no question waiting SHALL show neither.

#### Scenario: Working with a question
- **WHEN** an implementer reported `blocked` once and its turn is still running
- **THEN** its badge reads "Working · 1 question waiting" and its sidebar row shows "1 question"

#### Scenario: Turn ended unanswered
- **WHEN** that turn ends and the coordinator has not answered
- **THEN** its badge reads "Blocked · 1 question waiting"

#### Scenario: Nothing waiting
- **WHEN** a session has no unanswered `blocked` report
- **THEN** its badge and sidebar row show no question text
