## Context

Fourteen follow-ups: eleven from reviews of PR #51 and PR #46, and three test gaps. Most are a few lines. This records the choices that are not obvious from the code.

## Decisions

### 1. A finished ticket withdraws only a waiting request
`finish_ticket` calls `withdraw_request` inside the same savepoint that archives the agents and changes the ticket's state, and before the agents are archived, so an implementer that asked still gets the request's one message. A step that is already running is left alone: its own recheck fails on the finished ticket and it reports that itself. `resolve_attention` opened a transaction, which cannot nest in that savepoint, so it now uses a savepoint like the other helpers a ticket's close or accept calls.

### 2. The waiting card is built in one place
The desktop redrew a waiting item with `publish_ask(...).unwrap_or(EndedPullRequest)`, which the host never did. That fallback moves into `workspace_core::waiting_card`, so the card for an owner allowed while the item waits has a test. The push note then asks one question: will the pull request step ask? It does where the owner is still not allowed, and always after an earlier pull request ended.

### 3. "Approved both steps" is a count, not a guess
The request counts the Inbox items the human approved (`approvals`). The success message says "The human approved both steps." only at two: the push item and the pull request item. One approval that covers both steps after a blocked review, and a push approved before the owner was allowed, are one approval and do not say it.

*Alternative:* derive it from why the last step asked. Rejected because the rules can change between the two steps, which is exactly the case that made the card wrong.

### 4. "Not on GitHub yet" from what the host knows
The card is drawn without asking GitHub. It says "not on GitHub yet" when the host has never pushed the ticket's branch and the watcher has read no pull request for it. A branch an agent's shell pushed before the host published tickets is the one case this gets wrong; the push step still reads GitHub's branch before it sends anything.

### 5. The commit count comes from the request's size
The size the request froze begins with its commit count ("1 commit · 1 file changed …"), counted from the commit the host last pushed. The update card's title reads the count from there instead of storing it twice.

### 6. A signalled leftover that still runs is its own outcome
`stop_leftover` returned `Running` both for a group it never signalled and for one it signalled whose members survived. It now returns `Signalled` for the second. Startup records the stop time for it, since the provider itself was stopped, keeps holding the session, and words the note for what happened. The host still sends no second signal once the leader is gone: nothing confirms the group then.

### 7. Leftover notes are settled by the scheduler pass
Each pass looks at every open leftover-process note, settles those whose group is gone, and arms the existing two-second wake-up while one still runs. Before, only a session with a turn due was looked at, and startup leaves an interrupted session disconnected with its input held.

*Alternative:* settle at snapshot time. Rejected because a read would then write.

### 8. The login-shell test waits before the timeout starts
The test failed under load when the 200 ms timeout fired before the shell had started its child and written its pid. `resolve_after` runs a hook once the shell is running and before the timeout begins; the test waits there for the pid. The real timeout and the real kill of the process group still run in the test.

*Alternative:* a longer timeout. Rejected because it only makes the race rarer.

## Risks / Trade-offs

- While a leftover-process note is open and its process runs, the host wakes every two seconds for one small query and a signal-0 probe, where it used to wake only for a session with a turn due.
- `provider::process_start` becomes public so the integration test can record a real start time. It only reads.
