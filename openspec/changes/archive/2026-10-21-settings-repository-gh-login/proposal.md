## Why

Checking `owner/name` as the settings repository fails on a Mac whose Git has no HTTPS credentials and whose gh signs in over SSH: the host always turned `owner/name` into an HTTPS URL and `git ls-remote` could not log in. Wiffletree already reads GitHub through gh, so the login exists; it just was not handed to Git.

## What Changes

- **Fallback for `owner/name`.** The host tries the URL of gh's Git protocol first, then the other protocol's, and keeps the first `git ls-remote` can read as the push URL.
- **gh's login over HTTPS, per command.** Every Git command that reaches an HTTPS `github.com` URL of the settings repository (the access check, clone, a sync pass's fetch and push) gets gh's credential helper through `-c` for that command only. Git's config is unchanged and no token is stored, logged or put on a command line.
- **A URL the human names** is used as named, with gh's login when it is HTTPS on `github.com`.
- **One error naming every attempt.** When no URL can be read, the error under the field names each URL, Git's reason, and the next steps.
- **Unchanged.** Sync's own files, its branch and never forcing; the publishing path keeps its own URL rules. The shared `Tools` Git dispatch now finds the Git command after leading `-c` options; publishing never sends any, so its calls are dispatched as before.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `settings-repository`: how the host reaches the settings repository.

## Impact

- `crates/workspace-host/src/sync.rs`: `candidates`, `gh_protocol`, `gh_login`, `run_at`, `first_readable`; clone, fetch and push reach the repository through `run_at`.
- `crates/workspace-host/src/publish.rs`: `subcommand`, used by the default `Tools` dispatch.
- Docs: `docs/DEVELOPMENT.md`.
- No storage, protocol or UI change beyond the error text.
