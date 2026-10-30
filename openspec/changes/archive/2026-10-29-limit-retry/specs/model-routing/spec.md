## ADDED Requirements

### Requirement: A turn that fails on a usage limit is retried at the reset
A turn whose provider fails it after a quota reading during that turn said the provider's limit is reached, a window reported rejected or at 100% whose reset is ahead or unknown, SHALL be a limit failure, whatever the switch threshold. When no such reading came during the turn, the host SHALL decide from the provider's error text, and SHALL record which of the two it used. A turn stopped by the human or its parent, a turn a quit interrupted, a PR keeper's turn and the turn of a session paused at its end SHALL NOT be limit failures.

A limit failure SHALL hold its input and interrupt the session as any failed turn does, and SHALL record on the session's runtime when it failed and when the provider's limit resets, if known. The host SHALL then release the held input as Retry does, with the same note that the turn was interrupted, once the limit has reset: at the known reset, or, when the reset is unknown, at the first quota reading of that provider since the failure that no longer says the limit is reached. It SHALL wake at the reset and look again on every new quota reading. The retried turn SHALL run in the same session on its pinned model. A session paused during the wait SHALL wait for the human. Retry SHALL run the input at once and Skip SHALL drop it, and either SHALL end the wait. The wait SHALL survive a host restart.

The session's parent SHALL be told once, by the turn's result message, that the agent hit its provider's usage limit and is retried automatically at the reset, and that nothing is needed from it; it SHALL NOT be asked to resume the agent. A verifier in a running cycle SHALL keep the cycle open and run in its round again after the retry. While the session waits, its conversation SHALL keep the held-input warning and its Retry and Skip, and SHALL say that it waits for the provider's limit to reset and when it is retried.

#### Scenario: Claude's session limit
- **WHEN** an implementer's Claude turn streams a reading whose 5-hour window is rejected with a reset at 19:00, then fails with "You've hit your session limit"
- **THEN** its input is held, its conversation says it is retried automatically at 19:00, the coordinator gets one message saying nothing is needed, and at 19:00 the input is retried with the interrupted-turn note

#### Scenario: Unknown reset
- **WHEN** a turn fails after a rejected reading with no reset time
- **THEN** it waits until a later reading of that provider no longer says the limit is reached, then it is retried

#### Scenario: Only the error text says so
- **WHEN** a turn fails with an error naming a usage limit and no reading came during the turn
- **THEN** it is a limit failure that waits for the provider's next reading

#### Scenario: Skip during the wait
- **WHEN** the human presses Skip while a limit failure waits
- **THEN** its input is dropped and nothing is retried at the reset

#### Scenario: Another failure
- **WHEN** a turn fails because its provider exited, or the human stops it while the limit is reached
- **THEN** its input is held for a manual Retry or Skip as before

#### Scenario: A verifier hits the limit
- **WHEN** a verifier's turn in a running cycle fails on a usage limit
- **THEN** the cycle stays open, and after the reset the verifier's round message is queued again and its round starts it

#### Scenario: A restart during the wait
- **WHEN** the host restarts while a limit failure waits for a reset
- **THEN** the input is still retried at the reset
