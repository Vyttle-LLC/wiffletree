## Context

See proposal.md. The existing study leads with chat and a compact project → task → workers tree. macOS has Rust 1.97, Codex CLI 0.160.0 and Claude Code 2.1.283 installed. There is no previous runtime or migration to preserve.

## Goals / Non-Goals

**Goals:** provide a runnable native prototype backed by real durable host records, demonstrate routing and portable memory, measure application overhead, and establish contracts for provider integration.

**Non-Goals:** claim provider execution from simulated responses, implement every PRD feature in one increment, rename the repository, attach the user's work brain implicitly, or claim that host microbenchmarks establish desktop responsiveness.

## Decisions

1. **Three Rust crates:** `workspace-core` for typed domain/policy, `workspace-host` for SQLite and commands, `workspace-desktop` for GPUI. Neutral package names minimize future rename cost. Host CLI uses versioned JSON-lines over stdio, usable without any graphics dependency. Local desktop uses a bounded request channel to one database thread rather than a separate IPC process during this increment. Alternatives: webview UI or early network service add overhead before native feasibility is established.
2. **SQLite WAL, FULL synchronous, indexed keyset queries.** Persist before replying; idempotency conflicts reject reuse with different content. A process lock prevents competing hosts for one database. Recover working sessions as disconnected and uncertain delivery as held, rather than silently replaying side effects. Transcript pages cap at 100; message bodies cap at 64 KiB. UI keeps only the selected page and draft states.
3. **Provider-independent roles:** project coordinator, implementer, tester, reviewer and maintenance. The project coordinator owns tickets directly; the earlier task (repository) orchestrator was retired by `flatten-coordination-hierarchy` and survives only on archived, migrated sessions. Role policies contain a fixed default and allowlisted model/effort profiles for small, standard and complex work. Automatic mode accepts an orchestrator proposal only inside the allowlist; deterministic tiers provide a baseline and explanation. Exact model IDs are user configuration, validated against discovered runtime catalogs when available. No silent provider switching; active sessions retain their provider. Defaults propose Sol medium and Astra high, pending runtime capability discovery, not entitlement claims.
4. **Four active turns globally with per-project limits.** A turn registry releases capacity on completion/crash. Pending input stays durable; only accepted adapter input advances delivered state. The local simulator is explicit and never consumes provider capacity. Attention requests bind project, session, host and operation and resolve once. Blocked does not mean needs-human.
5. **Memory begins with durable source logs.** SQLite stores scoped milestone records and export receipts; Markdown export preserves external content, uses stable entry markers and atomic replacement under a brain lock. Existing source hashes are compared before replacement. Retrieval reads scoped indexed log records. Deterministic export/maintenance records run receipts and performs no model inference. Wiki compilation/conflict interpretation and recurring timezone scheduling remain later work; the UI states this boundary.
6. **Pinned GPUI 0.2.2 and compatible gpui-component 0.5.1.** Reuse the component library's native input behavior for selection, clipboard and IME. Consume canonical Tidal tokens, use virtualized transcript rows and no idle timers. All persistence and Git reads run on the background host thread. Panels open on demand; theme and draft changes avoid provider work.
7. **Read-only Git first.** Optional repository attachments are canonicalized, retain their own configured base, and expose a bounded real history page. No branches, commits or PRs are fabricated as working integrations. Writable worktree orchestration follows adapter feasibility.
8. **Product name and honest performance.** Display “Wiffletree”; historical assets retain their names. Record host timings, hardware, fixture and durability mode; separate desktop, inference, GPU and long-soak gates. Absolute absence of latency cannot be established; the target is imperceptible application overhead.

## Risks / Trade-offs

- [GPUI maintenance and accessibility] → Pin versions, use established input components, document unvalidated accessibility and long-session gates.
- [Catalog differs by runtime/account] → Discover through app-server; unsupported choices fail visibly and remain configured until edited.
- [Crash after runtime acceptance] → Hold ambiguous messages for reconciliation; never promise exactly-once effects.
- [External Markdown writer race] → Brain lock, source hash comparison, atomic write and marker-based recovery; non-cooperating editors still require review after conflicts.
- [Prototype scope] → Clearly label simulator, unavailable quota telemetry and pending production features in UI and morning handoff.

## Migration Plan

Create a versioned schema in an explicit data directory outside code repositories. Use a temporary data directory during development. Commit source and lockfile only; never runtime state or credentials. Removing the executable does not delete stored projects. Schema downgrades fail visibly rather than reinterpret newer data.
