# Wiffletree

Read `README.md`, `docs/PRD.md`, and `design/README.md` before changing this project.

- `docs/PRD.md` is the canonical product document. The original chat Page is provenance, not a second editable source of truth.
- `design/mockups/current/source.html` is the active interactive study. Update its generated `index.html` using `python3 scripts/build_mockups.py`.
- Preserve the exported preview's sandbox and CSP. Mockup state and sample messages are simulations; do not present them as implemented provider or Git operations.
- Keep project → ticket → agents semantics. A project is independent of code repositories; multiple tasks can share a repo.
- Keep status and escalation separate: a blocked worker does not automatically require human input. The project orchestrator consolidates human decisions.
- Rust and GPUI are the starting stack. Performance budgets in the PRD are targets to validate, not measured guarantees.
- Do not introduce generated provider integrations or duplicate global skills. OpenSpec uses `--tools none`.
- Keep the historical design studies intact unless explicitly updating their documentation.

Vyttle's shared brain is maintained through the configured `brain-query` and `brain-log` skills. Do not commit or push the brain from this repository's sessions.
