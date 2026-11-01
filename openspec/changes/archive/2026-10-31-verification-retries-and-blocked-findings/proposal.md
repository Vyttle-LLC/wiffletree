## Why

Two verification reports were lost in real projects. A verifier's report sent before it took its round's input, in a cycle an earlier host held queued, was stored quietly and never counted; retrying it with the same id returned the stored report and still left the verifier pending, so the cycle never ended. And a verifier that reported `blocked` with findings attached put none of them in the ledger or in front of the implementer; the coordinator copied them out of the verifier's session file by hand.

## What Changes

- **A retried verdict counts.** A report retried with the same id is still idempotent, but when it is now a verdict for the verifier's current round and no round counted it yet, the host records it as that verdict.
- **A `blocked` report's findings are recorded.** They enter the ledger as a completed check's would, without clearing any entry. A `blocked` report with an evidenced blocking finding is recorded as failed, with its reason, so its findings go to the implementer and the verifier re-checks in the next round, as for any failed result.
- The `report` tool's description tells verifiers to attach findings to `blocked` too.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `ticket-verification`: retried verdicts, findings on `blocked` reports, and a cycle blocked by a verifier's blocked result rather than by its reported kind.

## Impact

- `workspace-host`: `verification.rs` (`report`, `record_verdict`, `record_findings`), the `report` tool description in `mcp.rs`.
