## MODIFIED Requirements

### Requirement: A waiting step shows on the ticket
While a publish step waits for the human, the ticket's card in the Agents panel SHALL show a line saying which step waits for approval. The line SHALL go when the step is approved, denied or withdrawn, including when the ticket is accepted or closed.

#### Scenario: Waiting line
- **WHEN** a ticket's push waits for the human's approval
- **THEN** its card shows that the push waits for approval

#### Scenario: The ticket is finished
- **WHEN** a ticket whose push waits for approval is accepted or closed
- **THEN** its card no longer shows that the push waits for approval
