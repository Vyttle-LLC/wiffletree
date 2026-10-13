## ADDED Requirements

### Requirement: A compaction leaves one line in the conversation
A compaction that succeeds SHALL add one entry to the agent's conversation reading "Context compacted · ‹before› → ‹after›", with the provider's own before and after counts printed in the Context card's number format, and a second line "Kept: ‹instructions›" only when instructions were given. When the provider reported no counts the entry SHALL read "Context compacted". A compaction that fails SHALL add one entry reading "Compaction failed · context unchanged", and one that the host's stopping cut off SHALL add one reading "Compaction interrupted". A compaction that found nothing to compact, and a request that was cancelled, SHALL add no entry. The entry SHALL be durable and SHALL be shown as a single line at the place a finished turn's summary takes, not as a message bubble; a failure SHALL use the failure color. It SHALL NOT be a reply, SHALL NOT be input to any turn, SHALL NOT wake or message any agent, and SHALL have no turn summary and no turn history of its own.

#### Scenario: Compacted with a note
- **WHEN** an agent's compaction succeeds with the provider reporting 812,000 tokens before and 5,000 after, with the instructions "The failing test names and the migration plan"
- **THEN** its conversation shows "Context compacted · 812k → 5k" and beneath it "Kept: The failing test names and the migration plan"

#### Scenario: The card counts more than the line
- **WHEN** that compaction's card is measured again at 41k, which also counts the system prompt and tools
- **THEN** the line still reads "812k → 5k"

#### Scenario: Compacted without a note
- **WHEN** a compaction succeeds with no instructions
- **THEN** the entry has no "Kept:" line

#### Scenario: Failed
- **WHEN** an agent's compaction fails
- **THEN** its conversation shows "Compaction failed · context unchanged" in the failure color

#### Scenario: Nothing to compact
- **WHEN** a compaction ends because there was too little to summarize
- **THEN** the conversation gains no entry

#### Scenario: Order around a waiting message
- **WHEN** a compaction requested mid-turn runs while the human's message waits
- **THEN** the conversation shows the turn's reply, then the compaction line, then the turn that takes the message

#### Scenario: After a restart
- **WHEN** the host restarts after a compaction succeeded
- **THEN** the conversation still shows its line at the same place
