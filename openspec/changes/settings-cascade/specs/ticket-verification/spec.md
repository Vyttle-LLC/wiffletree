## ADDED Requirements

### Requirement: The Review depth Behavior can require full reviews
When the Review depth Behavior resolves to "full review for every ticket" for a ticket's repository in its project's Space (see settings-cascade), `verify_ticket` SHALL refuse a level whose kind is re-check, starting nothing, and the refusal SHALL name the Behavior and tell the coordinator to verify at a full review level. Full reviews SHALL still count against the cycle cap. The PR keeper's re-check of its own rebase is not a `verify_ticket` call and SHALL keep its rule.

#### Scenario: A re-check refused
- **WHEN** Sage says "full review for every ticket" and the coordinator calls `verify_ticket` naming a re-check level for a Sage ticket
- **THEN** the call is refused naming Review depth, and no verifier starts

#### Scenario: A full review
- **WHEN** the same coordinator names a full review level
- **THEN** the cycle starts as before
