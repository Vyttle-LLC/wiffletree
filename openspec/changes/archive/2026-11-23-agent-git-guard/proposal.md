## Why

The fixed core tells every agent never to run `git push` and never to touch a pull request from its shell, and nothing enforces it. Claude runs with `--dangerously-skip-permissions` and Codex with approvals and the sandbox bypassed, both with the human's `git` and `gh` credentials, so an agent can close or merge a pull request with one command. The Skills page admits this in its "Not enforced" note.

The same rule also blocks work that is fine. An implementer cannot push its own branch to save work in progress, and nothing can create a branch on origin, yet `create_ticket`'s `base` must already be there. The human creates every feature branch by hand.

The human approved this direction on November 23, 2026: guard agents' `git` and `gh`, let implementers push their own ticket branch, and give coordinators a tool that creates a branch.

## What Changes

- **A guard on every agent's `git` and `gh`.** Each agent turn, on Claude and Codex, finds Wiffletree's `git` and `gh` first on its `PATH`. They run the real binary for what is allowed and refuse everything else with one line naming the Wiffletree tool to use instead. The host's own Git and GitHub calls, an evaluator's run and a sorting run are unchanged.
- **`git` passes everything but pushes.** A push is allowed only for an implementer, only to `origin`, only to its own ticket's branch, and only when that branch is not protected and the human allowed the repository's owner. No tags, deletes, `--all` or `--mirror`, and force only as `--force-with-lease`. Coordinators, verifiers and the PR keeper's sessions cannot push.
- **`gh` passes reads only.** Viewing and listing pull requests, issues, runs, the repository and searches pass, and `gh api` passes only as a GET. Every pull request, issue, release, repository, secret, variable and workflow write is refused, and so is anything the guard does not know.
- **Codex shells skip the login shell** (`allow_login_shell=false`), which would otherwise move the guard behind the human's profile folders.
- **Implementers may push their own branch at any time.** Opening and updating the pull request stays with `publish_ticket`. Publishing already adopts a head the branch once had here; the change says so for an agent's push and tests it. The PR keeper stops reporting a pull request as diverged when its head is a commit the ticket's branch once had here.
- **A new coordinator tool, `create_branch`.** The host creates a branch on origin from the repository's base or another branch on origin, refuses protected, existing and invalid names, and returns the branch to pass as `create_ticket`'s `base`.
- **Instructions and the Skills page say what is enforced.** The contract (v14), the coordinator's core (v17) and the implementer's core (v6) describe the guard, the push an implementer may make and `create_branch`. The Skills page, its mockup, the README and FEATURES replace "Not enforced" with: the guard refuses these commands, but it is a guardrail, not a sandbox.

## Capabilities

### New Capabilities

- `agent-git-guard`: the `git` and `gh` an agent's shell runs, and what they refuse.
- `branch-creation`: a coordinator creating a branch on origin with `create_branch`.

### Modified Capabilities

- `ticket-publishing`: an implementer's own push is adopted; the instructions and the documented limit describe the guard.
- `pr-keeper`: a pull request head the ticket's branch once had here is not divergence.
- `skills`: the fixed core's push rule and the Skills page's note.

## Impact

- `workspace-host`: a new `guard.rs` (argument parsing and decisions), dispatch on the program name in `main.rs`, the guard folder and the turn's environment in `provider.rs` and `service.rs`, `Host::push_branch` in `publish.rs`, the `create_branch` tool in `mcp.rs` with `branches.rs`, a reflog check in the keeper's ancestry (`checkout.rs`) and `fetch_due`, and `communication.md`, `coordinator.md`, `implementer.md`.
- `workspace-desktop`: the Skills page's note and enforced lists.
- `design/mockups/current/skills.html`, `README.md`, `docs/FEATURES.md`, `docs/DEVELOPMENT.md`.
- Out of scope, as a follow-up: removing the human's credentials from agents' shells. Until then a full path to the real `git` or `gh`, or `curl` with the token, is not stopped.
