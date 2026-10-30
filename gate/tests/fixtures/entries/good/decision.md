---
id: 01jbx4m2k8q6v9z3n5r7t0w2yb
repo: sms-service
author: gate-tester
source: wiffletree
---
## 2026-10-29 — SMS retry policy
- type: decision
- ref: feature/sms-retry @ 1a2b3c4…5d6e7f8 · PR #42
- changed: Retry failed sends once with backoff.
- why: Carrier errors are often transient.
- state: Merged; no deploy yet.
