## Why

The repository has a detailed PRD and interactive study but no executable application. Establish a native, measurable foundation combining project/task orchestration with durable Agentic OS memory. The first development increment must make ownership, recovery and bounded model selection concrete before adding broad integrations.

## What Changes

- Build a portable Rust host, SQLite persistence, typed local command interface and native GPUI macOS client.
- Persist projects, repository attachments, task/worker sessions, conversations, message receipts, attention and model policies. Keep blocked status separate from human escalation.
- Add fixed per-role provider/model/effort selection and opt-in orchestrator selection within role allowlists and concurrency limits, with an inspectable routing explanation.
- Add provider-neutral structured work logs, scoped retrieval and restart-safe Markdown export into an explicitly attached brain. Add deterministic maintenance runs that do no model work.
- Validate Codex app-server initialization and model discovery without inference. Supply a clearly labeled local simulation adapter for the native prototype; real Claude/Codex execution remains a following feasibility increment.
- Measure host overhead with large persisted fixtures; document native feasibility evidence and remaining release gates.
- Use Wiffletree as the product name. Historical studies retain their original names as provenance.

## Capabilities

### New Capabilities
- `workspace-host`: durable hierarchy, messaging, attention, paginated queries and restart recovery.
- `model-routing`: role defaults, bounded automatic selection, provider capability validation and turn concurrency.
- `project-memory`: durable work logs, idempotent Markdown export, scoped retrieval and maintenance receipts.
- `native-workspace`: GPUI chat-first client with task tree, retained drafts, on-demand panels and explicit simulation state.

### Modified Capabilities
None; no existing application specs.

## Impact

Adds Cargo workspace and pinned Rust dependencies. GPUI is isolated from the headless host. SQLite is the authoritative application store; Markdown remains separately inspectable memory. No credentials are stored, no external brain is attached implicitly, and no Git writes or provider inference occur merely by opening the application. This increment is an initial working prototype, not fulfillment of the entire first-release PRD.
