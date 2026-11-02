## 1. Core

- [x] 1.1 `SpaceLocal.followed_roots`, with a serde default and left out when empty.
- [x] 1.2 `Command::SetFollowedRoots`, human only, and `ask::set_followed_roots`, with `docs/host-protocol.md`.
- [x] 1.3 `RepositoryRefresh.joined` and `.not_joined`.

## 2. Host

- [x] 2.1 `scan_roots` returns the roots that found each repository.
- [x] 2.2 `set_followed_roots`: each path a current root, each once.
- [x] 2.3 `add_discovered` appends a first find to each Space following a root that found it, through `update_space`, and reports what joined and what did not.
- [x] 2.4 `remove_repository_root` drops the root from every Space.
- [x] 2.5 `SetFollowedRoots` in the remote access table's Change row.
- [x] 2.6 Tests: joining, several roots, two Spaces on one root, a removal staying out, a hand-added repository, a second clone, root removal, a failed Space write, the human-only guard, no path in `space.yaml`, an older store.

## 3. Desktop

- [x] 3.1 `repository_tree.rs`: grouping, order, filter, tri-state counts, the folder's tick, Follow, the footer. Unit tests apart from rendering.
- [x] 3.2 `repository_chooser.rs`: the dialog as drawn.
- [x] 3.3 `spaces_view.rs`: Following lines, six rows and "and N more", Choose repositories…, the draft, and Save sending the rule.
- [x] 3.4 `main.rs`: toasts for what joined and what did not.

## 4. Docs and checks

- [x] 4.1 `docs/DEVELOPMENT.md` and `docs/PRD.md`.
- [x] 4.2 Formatting, Clippy, tests, `openspec validate --all --strict`.
- [x] 4.3 Run the app on a seeded store, in dark and light, and compare with `ui/mockups.html`.
