## ADDED Requirements

### Requirement: The settings repository is reached as gh reaches GitHub
For a repository named as `owner/name`, the host SHALL try its SSH and HTTPS URLs on `github.com`, the protocol gh is configured with for `github.com` first and HTTPS when gh names none, and SHALL keep as the push URL the first one `git ls-remote` can read. A URL the human names SHALL be the only one tried. Every Git command that reaches an HTTPS `github.com` settings repository, including the access check, the clone and sync's fetch and push, SHALL use gh's login through a credential helper given for that command only. The host SHALL NOT change Git's configuration, and SHALL NOT store, log or put on a command line a token. When no URL can be read, the error SHALL name each URL tried with Git's reason, and suggest adding an SSH key, signing gh in, or checking that the repository exists and can be read.

#### Scenario: gh uses SSH and Git has no HTTPS login
- **WHEN** the human checks `owner/name`, gh's Git protocol is SSH and the SSH URL can be read
- **THEN** the SSH URL is the push URL, and set-up and sync pull and push over it without gh's helper

#### Scenario: No usable SSH key
- **WHEN** the SSH URL cannot be read and gh is signed in
- **THEN** the HTTPS URL is the push URL, and its check, clone, fetch and push each pass gh's credential helper for that command

#### Scenario: A named URL
- **WHEN** the human names the repository by its URL
- **THEN** only that URL is tried and kept

#### Scenario: Nothing can be read
- **WHEN** neither URL can be read
- **THEN** the error names both URLs with Git's reason for each and suggests the next steps, and nothing is written
