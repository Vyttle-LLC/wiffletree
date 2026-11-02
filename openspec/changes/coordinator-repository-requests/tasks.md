## 1. Core

- [x] 1.1 `REPOSITORY_REQUEST_PREFIX`, a host notice and part of `is_permission`.
- [x] 1.2 `RepositoryRequest` and `Attention.repository_request`, with a serde default and left out when none.

## 2. Host

- [x] 2.1 `request_repository`: the tool, coordinator only, with every no-card case.
- [x] 2.2 Reserve the prefix from `ask_user` ids.
- [x] 2.3 `answer_attention`: one transaction, called from `ResolveAttention`, with `Delivery::PastArchive` for an archived coordinator.
- [x] 2.4 `refresh_github` after approving a folder.
- [x] 2.5 `ensure_uses`, and `coordinator.md` as v15.
- [x] 2.6 Tests: Mudling's case end to end, the folder case, a followed root, deny, each no-card case, a worker refused, a restart, each way approval can fail.

## 3. Desktop

- [x] 3.1 The Inbox card as drawn, light and dark.

## 4. Docs

- [x] 4.1 `docs/host-protocol.md`, `docs/FEATURES.md`, `docs/PRD.md`.
