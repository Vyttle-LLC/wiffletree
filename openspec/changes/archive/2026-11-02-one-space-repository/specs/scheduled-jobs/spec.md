## MODIFIED Requirements

### Requirement: Built-in host jobs
The scheduler SHALL also run built-in host jobs, which Wiffletree defines and no one saves, such as brain compile (see `brain-compile`). A built-in job SHALL use the scheduler's due times in an IANA timezone, its daylight-saving rules, one catch-up run after missed times, Run now and run records, and SHALL add its own conditions. It SHALL belong to its owner, a wiki for brain compile, not to a Space; it SHALL NOT be a recipe, SHALL NOT appear in a Space's Jobs list, and SHALL NOT be created, changed or deleted by any user, tool, thread or job run. A wiki's job SHALL exist on each Mac with compiling on, and being due on several Macs SHALL NOT mean several runs, because a claim decides which Mac runs it. Its runs SHALL follow its own rules for sessions and models, not the thread authority rules.

#### Scenario: A compile after sleep
- **WHEN** a wiki's compile is due at 02:00 America/New_York, a volunteer sleeps from 01:00 to 07:00, and no other Mac has run it
- **THEN** the scheduler starts one catch-up compile run at 07:00, and no Space's Jobs list shows it

#### Scenario: Due on two Macs
- **WHEN** a wiki's compile is due at 06:00 and two volunteers are awake
- **THEN** the scheduler on each starts the job, one claims the run, and the other records that it lost the claim
