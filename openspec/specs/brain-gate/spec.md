# brain-gate Specification

## Purpose
Keep the checks that guard a Space's repository in one place: Wiffletree owns the `secret-scan` and `sensitive-info` checks and the log entry contract, bundles them into the app, and runs them on each Mac over what it pushes and pulls, never echoing a matched value.

## Requirements

### Requirement: No check echoes a matched value
No check SHALL print a matched secret or sensitive value, or the entry text around it, in its log or outputs. A finding SHALL report only the file, line, column and rule. gitleaks SHALL run with redaction.

#### Scenario: A planted key
- **WHEN** a file to be pushed holds a fake AWS access key
- **THEN** `secret-scan` or `sensitive-info` fails, and what the host records holds the file, line and rule but not the key

### Requirement: The entry contract is the gate's
The app SHALL define the log entry contract, with fixtures in `gate/tests/fixtures/entries/`:
- **Path:** `second-brain/logs/<repo>/YYYY/MM/YYYY-MM-DD-<id>.md` in the folder, where `<repo>` starts with a letter or digit and holds only letters, digits, `.`, `_` and `-`, and `<id>` is a lowercase ULID. The folders and the file name's date agree and form a real date.
- **Frontmatter:** exactly `id` (matching the file name), `repo` (matching the folder), `author` (a valid GitHub login, or on a local repository this Mac's account name) and `source` (`wiffletree` or `skill`).
- **Body:** a `## YYYY-MM-DD — Title` heading whose date matches the file name and whose title is not empty, then exactly the fields `type` (`decision` or `observation`), `ref`, `changed`, `why` and `state`, in that order, each with a value, optionally continued on indented lines.
- **`ref`:** `—`, `<branch> @ —`, or `<branch> @ <sha>` with an optional `…<sha>` range and ` · PR #NN`, with SHAs of 7–40 lowercase hex characters.
- **Size:** at most 2,000 characters in the body after the frontmatter.

Wiffletree's log writer SHALL be tested against those fixtures in this repository. The host SHALL refuse to write an entry that breaks the contract, and a pulled entry that breaks it SHALL be listed as a teammate's text and not refused.

#### Scenario: An oversize entry
- **WHEN** an entry's body would hold 2,001 characters
- **THEN** the host refuses to write it, saying how many characters to cut

#### Scenario: Malformed frontmatter
- **WHEN** a fixture's frontmatter has a fifth key
- **THEN** the writer's contract test rejects it

### Requirement: Suppression is inline and visible
A sensitive-information finding SHALL be suppressed only by an inline marker `<!-- gate:allow <rule>[,<rule>] <reason> -->` on the same line, which names the rule and gives a reason, and hides only that rule on that line, or for the `email` rule by a domain in the folder's `allowed_email_domains`. gitleaks findings SHALL be suppressed only by `gitleaks:allow` on the line. Files in the repository SHALL NOT change what the scan on a Mac reads. No check SHALL skip a finding silently.

#### Scenario: A marker without a reason
- **WHEN** a line carries `<!-- gate:allow phone -->`
- **THEN** the finding is still reported

#### Scenario: A pushed allow-list
- **WHEN** a repository holds a `brain-gate-allowlist.toml` from an older brain
- **THEN** the scan on a Mac does not read it

### Requirement: Wiffletree owns the scan's checks
The scan's checks, default rule configs, entry contract fixtures and tests SHALL live in this repository under `gate/`, and the app SHALL bundle the files its scan runs: `checks/common.py`, `checks/gate_config.py`, `checks/sensitive_scan.py`, `checks/secret_scan.py` and `config/gitleaks.toml`. The checks SHALL be deterministic: they make no model calls, use no secret and need no network. They SHALL NOT contain any organization's values. Nothing SHALL run on GitHub, and the app SHALL fetch no check from GitHub. The other checks, their configs and the composite action SHALL be removed. A commit a repository pinned before SHALL stay reachable in this repository's history.

#### Scenario: The scan on a Mac
- **WHEN** the host scans a log entry
- **THEN** it runs the bundled checks with Python on this Mac, and nothing asks GitHub

#### Scenario: No org values in Wiffletree
- **WHEN** `gate/` is searched for `sagehaven.ai`, `sagesocial.com` or any organization's allow-list value
- **THEN** none is found outside fixtures that use fake values
