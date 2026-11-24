## 1. The guard

- [x] 1.1 `guard.rs`: parse `git` (global options, aliases, push refspecs and options) and `gh` (`-R`/`--repo`, the allow-list, `api`'s method) into allow or refuse with the one-line message.
- [x] 1.2 `main.rs`: started as `git` or `gh`, run the guard, exec the real binary from `WIFFLETREE_REAL_GIT`/`_GH`, adding `--no-follow-tags --recurse-submodules=no` to an allowed push.
- [x] 1.3 Tests for every refusal and allowed case in the agent-git-guard spec, including `git -C dir push`, `git -c k=v push`, an alias and `gh -R owner/repo pr close`.

## 2. Launch

- [x] 2.1 The host links `<data folder>/guard/git` and `gh` to its own binary at start and resolves its real `git` and `gh`.
- [x] 2.2 `configure`: after the evaluator's return, prepend the guard folder to `PATH`, set the real binaries and, for an implementer of a ticket whose branch is not protected and whose owner is allowed, `WIFFLETREE_PUSH_BRANCH`; Codex gets `allow_login_shell=false`, on `exec` and the app-server.
- [x] 2.3 Tests of the environment, of Codex's setting on a turn and a compaction, and that a verifier, coordinator, protected branch and owner not allowed get no push branch.
- [x] 2.4 Live check on each installed provider that `command -v git` and `command -v gh` resolve into the guard folder, and that `gh pr close` and `git push origin main` are refused while the implementer's own push succeeds; recorded in the design.

## 3. Publishing and the keeper

- [x] 3.1 Publish tests: GitHub's branch pushed by the implementer at the published commit and at an ancestor; the amended earlier tip was already covered.
- [x] 3.2 The keeper's ancestry notes a PR head the ticket branch's reflog holds, and `fetch_due` skips it; test the push-then-amend case sends no diverged notice.

## 4. create_branch

- [x] 4.1 The coordinator tool in `mcp.rs` and `branches.rs`, its checks on the host thread and its Git off it, replying `{repository_id, branch, start, commit}`.
- [x] 4.2 Tests: default start, a given start, then a ticket with it as `base`; each refusal; a non-coordinator refused.

## 5. Instructions and text

- [x] 5.1 `communication.md` v14, `coordinator.md` v17, `implementer.md` v6, and `tests/contracts.rs`.
- [x] 5.2 The Skills page's note and enforced lists in `skills_view.rs`.
- [x] 5.3 `design/mockups/current/skills.html`, `README.md`, `docs/FEATURES.md`, `docs/DEVELOPMENT.md`.

## 6. Finish

- [x] 6.1 `cargo fmt`, `cargo clippy` and the tests of the touched crates as CI runs them.
- [x] 6.2 Archive this change and commit it with the code.
