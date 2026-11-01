## 1. Host

- [x] 1.1 `report` checks a retried id's content, then counts it when it is now an uncounted verdict for the current round.
- [x] 1.2 `record_verdict` records a `blocked` report with an evidenced blocking finding as failed, and records a `blocked` report's findings without clearing open entries.
- [x] 1.3 The `report` tool description names `blocked`.

## 2. Tests

- [x] 2.1 A verdict sent while its cycle was queued counts when retried after the cycle starts, once.
- [x] 2.2 A blocked verdict with an evidenced blocking finding goes to the implementer; one without records its findings and leaves open entries open.
