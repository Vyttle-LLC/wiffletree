## ADDED Requirements

### Requirement: The approval names the review level
A publish approval's Review row SHALL name the level of the ticket's latest verification cycle and SHALL say when that cycle was a re-check. When the latest cycle is a re-check, the approval SHALL also show a "Last full review" row naming the most recent passed full review of the ticket: its cycle, its level, the commit it verified and its verifiers. A cycle recorded without a level SHALL be shown without one and SHALL count as a full review. The level SHALL NOT change whether the human is asked: a passed re-check SHALL publish as any passed review does, and a review that ended blocked SHALL need the human's approval at every level.

#### Scenario: After a full review
- **WHEN** a push waits for approval and the ticket's latest cycle is cycle 1, a passed "Full" review by Tests, Correctness and Regressions at the commit being pushed
- **THEN** the item's Review row reads "Cycle 1 · Full passed at this commit · Tests, Correctness, Regressions", and it has no "Last full review" row

#### Scenario: After a re-check
- **WHEN** a push waits for approval, cycle 1 was a passed "Full" review at `9e03c1b`, and the latest cycle is cycle 2, a passed "Light" re-check by Correctness at the commit being pushed
- **THEN** the Review row reads "Cycle 2 · Light re-check passed at this commit · Correctness"
- **AND** a "Last full review" row reads "Cycle 1 · Full passed at 9e03c1b · Tests, Correctness, Regressions"

#### Scenario: A re-check that ended blocked
- **WHEN** `publish_ticket` is called in an allowed owner's repository for a ticket whose latest cycle, a "Light" re-check, ended blocked
- **THEN** nothing is sent, and the approval says the review ended blocked, names it as a Light re-check, lists the open findings and shows the "Last full review" row

#### Scenario: A passed re-check in an allowed owner's repository
- **WHEN** a ticket in an allowed owner's repository has an open pull request, its latest cycle is a passed re-check at HEAD, and `publish_ticket` is called
- **THEN** the host pushes the commit, and no Inbox item is raised

#### Scenario: A cycle from before levels
- **WHEN** a push waits for approval and the ticket's latest cycle passed and was recorded without a level
- **THEN** the Review row reads as before, "Cycle 1 passed at this commit" with its verifiers, and there is no "Last full review" row
