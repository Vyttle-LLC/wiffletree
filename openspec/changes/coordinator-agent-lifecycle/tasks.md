## 1. Host

- [x] 1.1 Rely on the (role, focus) lookup skipping archived sessions; verify a second cycle gets fresh verifier sessions and no `verify:` message targets an archived session
- [x] 1.2 Add `retire` in `verification.rs`; archive passed verifiers when a round hands failures to the implementer and every verifier of the cycle when it passes or blocks (cap, blocked verifier, implementer `blocked`); verify the implementer is never archived and a late verdict from an archived verifier is quiet and inert
- [x] 1.3 Add `Host::archive_agent` and the `archive_agent` tool with ownership, mid-turn, open-implementer and needed-verifier refusals; verify each refusal and success for an idle reviewer

## 2. Contracts and docs

- [x] 2.1 Add `archive_agent` to `mcp.rs` tools and the contract test
- [x] 2.2 Update `skills/main-coordinator.md` (v5) and `skills/tester.md` (v4)
- [x] 2.3 Update `docs/DEVELOPMENT.md` archiving paragraph
- [x] 2.4 `cargo fmt --check`, `cargo clippy --all-targets`, `cargo test`
