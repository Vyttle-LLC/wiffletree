## 1. Core types and the shared rule

- [ ] 1.1 In `workspace-core/src/lib.rs`, add `GitHubRepository` (`owner`, `name`, `facts`), `GitHubFacts` (`organization`, `fork`, `parent`, `public`, `checked_at`) and `Repository.github` (`#[serde(default, skip_serializing_if = "Option::is_none")]`). Verify that a stored repository without the field decodes and that the types round-trip.
- [ ] 1.2 Add `HostSettings.allowed_owners` (`#[serde(default)]`, set in `HostSettings::new`), `Snapshot.allowed_owners` and `Snapshot.github_error` (both `#[serde(default)]`), and `Command::SetOwnerAllowed { entry, allowed }`. Verify that a `settings.json` and a snapshot saved before this change decode with an empty list.
- [ ] 1.3 Add `PublishStep`, `PublishRequest` (`caller`, `commit`, `step`, `title`, `body`, `waiting`, `blocked_review_approved`), `Ticket.publishing` and `Ticket.pushed`. Verify that a stored ticket without the fields decodes.
- [ ] 1.4 Create `workspace-core/src/owners.rs` with `owner_entry`, `is_allowed` and `owner_entries`. Verify with unit tests for the repository-owners scenarios "An allowed owner's repository", "A fork under an allowed owner", "GitHub has not answered", "Base on another remote", "Two clones of one repository" and "A repository that is not on GitHub", and that entries compare without regard to case.
- [ ] 1.5 Make `Attention::is_permission` true for `publish:` operations and add `publish:` to `HOST_NOTICE_PREFIXES`. Verify that `host_notice("publish:t:c")` is "Wiffletree" and that a `publish:` attention is a permission.

## 2. Reading owners and GitHub facts

- [ ] 2.1 In `workspace-host/src/repositories.rs`, add `read_github(repositories, gh)`: read each repository's `origin` push URL on disk with `pr_watch::github_repository`, then ask GitHub about the distinct repositories with one `gh api graphql` call per 50, passing owners and names only as raw-string `-f` variables. Verify with a fake `gh` that 34 repositories make one call, 51 distinct ones make two, a repository without a GitHub `origin` makes none, a fork and its parent are parsed, and GitHub's `nameWithOwner` replaces the URL's owner.
- [ ] 2.2 Add `Host::github_inputs()` (repositories whose answer is missing, older than a day, or whose `origin` changed) and `Host::record_github(reading)`, which saves only changed repositories and keeps the last error for `Snapshot.github_error`. Verify the scenarios "One request for the workspace", "A transferred repository" and "GitHub cannot be read", and that a failed read keeps earlier answers.
- [ ] 2.3 In `service.rs`, run the read on its own thread after a repository refresh (`:628`, `:727`) and after repositories are imported, report back with a new `Event::GitHub`, and never run two reads at once. Verify with actor tests beside the PR-watch ones: a panicking read frees the next one, and a refresh with nothing stale makes no `gh` call.

## 3. The allowed-owners setting

- [ ] 3.1 Add `settings.rs::set_owner_allowed(entry, allowed)`, saved through `save_settings`, and dispatch `Command::SetOwnerAllowed` in `lib.rs`. Fill `Snapshot.allowed_owners` and `Snapshot.github_error` in `Host::snapshot`. Verify the scenarios "Nothing is allowed at first", "The human allows an owner" (survives reopening the host) and "The human removes an owner".
- [ ] 3.2 In `live.rs::agent_context`, add each repository's `github` entry and `publishes_without_asking`. Verify the scenario "An agent reads the rule", and with a test over `mcp::tools()` and `agent_tool` that no tool changes the list ("An agent cannot allow an owner").

## 4. `publish_ticket`: the tool and its checks

- [ ] 4.1 Add `publish_ticket { ticket_id, title?, body? }` to `mcp.rs::tools` with a description stating who may call it, that the workspace runs the push, when the human is asked and that the result arrives as a message. Verify the tool list contains it and no tool that merges, approves, closes or comments ("No merge tool").
- [ ] 4.2 Create `workspace-host/src/publish.rs` with `Host::publish_ticket(caller, ticket_id, title, body)` and its local checks, in the order of design decision 6, making `live.rs::check_verified_head` `pub(crate)`. Verify the ticket-publishing scenarios "Verifier refused", "Missing title", "Never reviewed", "Review still running", "HEAD moved past the reviewed commit", "Squash after review", "Wrong branch" and "Not on GitHub", each sending nothing.
- [ ] 4.3 Decide authority: run without asking only when the latest cycle passed and `owners::is_allowed` holds; otherwise ask. Verify "Allowed owner, passed review" starts the steps with no attention item, and that an allowed owner's fork, an unanswered repository and a blocked cycle all ask.

## 5. Pushing, opening the PR and reporting

- [ ] 5.1 Add `runtime::git_push(path, args)` on `bounded_output` with a 10-minute deadline and `GIT_TERMINAL_PROMPT=0`. Verify with a unit test that a failing push returns Git's error line and a hanging one is killed at a short test deadline.
- [ ] 5.2 In `publish.rs`, add the push step: read GitHub's branch head with `git ls-remote`, then push, push with `--force-with-lease` over the commit in `Ticket.pushed`, or refuse. Verify against a local bare repository standing in for `origin`: the scenarios "New branch", "Rebased after the host's push" and "Someone else's commits", plus a fast-forward and a no-op at the same commit.
- [ ] 5.3 Add the open step: `gh pr create --repo --head --base --title --body` through `runtime::gh_output`, with the base branch taken from the repository's base. Verify with a fake `gh` that the arguments are passed as a list, a title containing shell characters arrives unchanged, and the PR URL is read from the output.
- [ ] 5.4 In `service.rs`, handle `publish_ticket` in `Event::Tool` beside `stop_agents`, run steps on a panic-safe thread like `spawn_pull_request_check`, and handle a new `Event::Published` with `Host::record_published`: save `Ticket.pushed`, record `ticket_pushed` or `pull_request_opened`, start or ask for the next step, and send the caller one `publish:<ticket>:<commit>` message. Verify the scenarios "First publish", "Update", "Success", "Push fails" and "Waiting status" with a fake Git and `gh`.
- [ ] 5.5 Clear a step that was running when the host stopped and tell the caller it was interrupted. Verify the scenario "Interrupted by a restart", including that a second call opens no second PR.

## 6. Approvals

- [ ] 6.1 When the owner is not allowed, save the request on the ticket and raise a `publish:<ticket>:<step>:<commit>` attention item with a host-written prompt. Handle its answer in `service.rs` beside parked permissions: `approved` re-runs the checks and starts the step, anything else clears the request and messages the caller. Verify the scenarios "Push waits for approval", "The pull request is a second approval", "A later push asks again", "Denied", "A coordinator's word is not approval", "HEAD moves while a card waits" and "Approval after a restart".
- [ ] 6.2 Make a repeated call for the same commit return the waiting status, and a call for a newer commit withdraw the old item and raise a new one. Verify the scenario "Newer commit" and that the human cannot dismiss, and the coordinator cannot close, a `publish:` item.
- [ ] 6.3 Ask whenever the latest cycle ended blocked. In an allowed owner's repository raise one item for the commit and, once approved, set `blocked_review_approved` so the commit's steps run without another item; elsewhere mark each step's item as following a blocked review. Verify the scenarios "Blocked review in an allowed owner's repository", "Approved despite the blocked review", "Blocked review where the owner is not allowed" and "The next commit".
- [ ] 6.4 Add a helper that lists a ticket's open findings (ledger entries neither fixed nor settled) and the verifiers that could not check. Verify with a ledger holding open, untriaged, fix-now, fixed, follow-up and won't-fix entries, and the scenario "A verifier could not check".

## 7. Watcher

- [ ] 7.1 Add `pr_watch::Watch::check_now(repository)` and call it from `record_published`. Verify the pr-watch scenario "After the host opens a PR" with explicit times, and that the other cadence tests still pass.

## 8. Desktop

- [ ] 8.1 In `repositories_view.rs::repositories_page`, add the "Open pull requests without asking" block above the list from `owners::owner_entries`: a tick box per owner and per fork that sends `Command::SetOwnerAllowed`, repositories not on GitHub without a box, the line "Wiffletree never approves, merges, closes or comments on a pull request.", the note that the list stays on this Mac, and the reason when GitHub could not be read. Verify with a unit-tested helper for the row text ("Owners with counts", "A fork is listed on its own") and against `design/mockups/current/repository-owners.html`, and that the page never says "approved" or "automatic".
- [ ] 8.2 Add the rule label to each repository row: without asking, asks first, or not on GitHub. Verify the scenario "Repository row" with a unit-tested helper.
- [ ] 8.3 In `inspector.rs::attention_card`, draw `publish:` items from the ticket's `publishing`, its verification record and ledger, and the repository's facts: the push card, the pull request card with the full title and description, and the blocked-review notice with its findings. Buttons use the existing `resolve_attention` with `approved` and `denied`. Verify against the mockup's four cards by driving a seeded store with `scripts/drive_desktop.py`.
- [ ] 8.4 In `tree.rs` and `sidebar.rs::ticket_row`, add the line shown while a step waits for the human. Verify the scenario "Waiting line" with a unit test of the text helper, and that a ticket with no waiting step keeps its row.

## 9. Instructions

- [ ] 9.1 Replace the open-pr sentences of `skills/communication.md` line 12 with the paragraph in design decision 10 and bump it to v7. Verify with a test over `provider::skill` for every role that the text contains `publish_ticket`, does not contain "open-pr", and gives no case in which a coordinator may merge.
- [ ] 9.2 Add the publishing sentence to `skills/main-coordinator.md` (v11) and `skills/implementer.md` (v5). Verify the scenarios "No pull request skill" and "Merging".

## 10. Docs and checks

- [ ] 10.1 Correct `README.md` lines 51, 81 and 87 as the ticket-publishing "The limit is documented" requirement says. Verify that the README no longer says Wiffletree never pushes or opens pull requests, and still warns that agents can run any command.
- [ ] 10.2 Update `docs/DEVELOPMENT.md` ("Current limits" and a section on owners and publishing, including the `gh` and Git credential requirements and what is not prevented) and add a short paragraph to `docs/PRD.md`. Verify that the text matches the specs and uses "allowed" and "without asking".
- [ ] 10.3 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace`, `python3 scripts/test_communications.py` and `openspec validate --all --strict`, and paste the output in the report.
- [ ] 10.4 With the human's go-ahead for that one run, publish a throwaway ticket in a repository the human names: once with its owner not allowed (both cards), once allowed. Report what was pushed and the PR's URL, and leave closing the PR to the human. Verify nothing else was written to GitHub.
