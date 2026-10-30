---
id: 01jbx4m2k8q6v9z3n5r7t0w2yb
repo: sms-service
author: gate-tester
source: wiffletree
---
## 2026-10-29 — SMS retry policy
- type: decision
- ref: feat/sms-retry @ —
- changed: Retry failed sends once with backoff.
  The second try waits two seconds.
- why: Carrier errors are often transient.
- state: Uncommitted on the branch.
