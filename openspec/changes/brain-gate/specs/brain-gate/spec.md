## ADDED Requirements

### Requirement: Wiffletree owns and ships the gate
The brain gate's checks, default rule configs, entry contract and fixture tests SHALL live in this repository under `gate/`. They SHALL be published as a composite GitHub Action that takes the input `check`, naming one of the seven checks. The action SHALL be deterministic: it makes no model calls, installs its tools at pinned versions, uses no secret, and needs only `contents: read`. It SHALL NOT depend on any organization's actions or contain any organization's values. Org-specific values SHALL live only in the brain repository that uses them.

#### Scenario: A brain uses the action
- **WHEN** a brain's caller runs `uses: Vyttle-LLC/wiffletree/gate@<sha>` with `check: log-schema`
- **THEN** the job checks the PR's log entries using only the gate's code and that brain's config

#### Scenario: No org values in Wiffletree
- **WHEN** `gate/` is searched for `sagehaven.ai`, `sagesocial.com` or any organization's allow-list value
- **THEN** none is found outside fixtures that use fake values

### Requirement: Seven checks, each its own check run
Every pull request to a brain's `main` SHALL run seven checks, each as its own job and check run with exactly these names: `secret-scan`, `sensitive-info`, `markdown-lint`, `wikilinks`, `log-schema`, `path-ownership` and `gate-tests`. Their behaviour SHALL be as in `sagehaven-docs` PR #1:
- **`secret-scan`:** gitleaks over the commits the PR adds, with findings redacted.
- **`sensitive-info`:** scans only the lines the PR adds, with the rules `phone`, `email` (outside the allowed domains and their subdomains), `street-address`, `gps`, `aws-access-key`, `aws-secret-key`, `private-key`, `token`, `url-credentials` and `secret-assignment`.
- **`markdown-lint`:** markdownlint-cli2 with the gate's config.
- **`wikilinks`:** every `[[link]]` under `notes/` and in `STATUS.md` names an existing note.
- **`log-schema`:** every file under `logs/<repo>/` meets the entry contract and the size cap.
- **`path-ownership`:** described in its own requirement.
- **`gate-tests`:** runs the pinned gate's self-test against the bundled fixtures with the installed tools.

#### Scenario: Names unchanged after the move
- **WHEN** `sagehaven-docs` switches from PR #1's in-repository scripts to the caller
- **THEN** its PRs report the same seven check names, and `main`'s required checks need no change

#### Scenario: A phone number in a log
- **WHEN** a log PR adds an entry line holding a US phone number
- **THEN** `sensitive-info` fails, naming the file, line, column and rule `phone`

### Requirement: No check echoes a matched value
No check SHALL print a matched secret or sensitive value, or the entry text around it, in its log, annotations or outputs. A finding SHALL report only the file, line, column and rule. gitleaks SHALL run with redaction.

#### Scenario: A planted key
- **WHEN** a PR adds a line holding a fake AWS access key
- **THEN** `secret-scan` or `sensitive-info` fails, and the job log and annotations hold the file, line and rule but not the key

### Requirement: The entry contract is the gate's
The gate SHALL define the log entry contract:
- **Path:** `<brain folder>/logs/<repo>/YYYY/MM/YYYY-MM-DD-<id>.md`, where `<repo>` starts with a letter or digit and holds only letters, digits, `.`, `_` and `-`, and `<id>` is a lowercase ULID. The folders and the file name's date agree and form a real date.
- **Frontmatter:** exactly `id` (matching the file name), `repo` (matching the folder), `author` (a valid GitHub login) and `source` (`wiffletree` or `skill`).
- **Body:** a `## YYYY-MM-DD — Title` heading whose date matches the file name and whose title is not empty, then exactly the fields `type` (`decision` or `observation`), `ref`, `changed`, `why` and `state`, in that order, each with a value, optionally continued on indented lines.
- **`ref`:** `—`, `<branch> @ —`, or `<branch> @ <sha>` with an optional `…<sha>` range and ` · PR #NN`, with SHAs of 7–40 lowercase hex characters.
- **Size:** at most the configured number of characters in the body after the frontmatter, 2,000 by default.

Wiffletree's log writer SHALL be tested against the gate's fixtures in this repository.

#### Scenario: An oversize entry
- **WHEN** a log PR adds an entry whose body holds 2,001 characters and the brain's cap is 2,000
- **THEN** `log-schema` fails for that file

#### Scenario: Malformed frontmatter
- **WHEN** an entry's frontmatter has a fifth key
- **THEN** `log-schema` fails for that file

### Requirement: Path ownership classifies each PR
`path-ownership` SHALL classify a PR by the files it changes:
- **log PR:** only adds files under `<brain folder>/logs/<repo>/`;
- **compile PR:** changes only `<brain folder>/notes/**`, `<brain folder>/STATUS.md` and `<brain folder>/reconcile-queue.md`;
- **other docs PR:** touches nothing under the brain folder.

It SHALL fail a PR that mixes classes, that modifies, deletes or moves a log entry, or that changes any other path under the brain folder. Edits to legacy `logs/<repo>.md` files SHALL only warn while the brain's legacy flag is on, and fail once it is off.

#### Scenario: A mixed PR
- **WHEN** a PR adds a log entry and changes a note
- **THEN** `path-ownership` fails, naming both classes

#### Scenario: Legacy edits after the flag
- **WHEN** the brain's legacy flag is off and a PR edits `logs/simpatico-sage.md`
- **THEN** `path-ownership` fails

### Requirement: Suppression is explicit and visible
A sensitive-information finding SHALL be suppressed only in two ways:
- by an inline marker `<!-- gate:allow <rule>[,<rule>] <reason> -->` on the same line, which names the rule and gives a reason, and hides only that rule on that line;
- by an entry in the brain's allow-list file: `[[value]]` for one exact matched text and one rule, or `[[path]]` for a file glob. Every entry has a reason, and the scan fails on an entry without one.

gitleaks findings SHALL be suppressed only by gitleaks' own mechanisms: `gitleaks:allow`, `.gitleaksignore` or the brain's `.gitleaks.toml`. No check SHALL skip a finding silently.

#### Scenario: A marker without a reason
- **WHEN** a line carries `<!-- gate:allow phone -->`
- **THEN** the finding is still reported

#### Scenario: An allow-list entry without a reason
- **WHEN** the brain's allow-list has a `[[value]]` entry with no `reason`
- **THEN** `sensitive-info` fails, naming the allow-list entry

### Requirement: A brain repository carries only a pinned caller and its config
A brain repository SHALL hold:
- `.github/workflows/brain-gate.yml`: one job per check, each calling the action pinned to a release by commit SHA, on pull requests to `main`;
- a `gate` object in `wiffletree-brain.json`, with the gate version, the size cap, the legacy flag, the allowed email domains and the allow-list path;
- the allow-list file;
- optionally `.gitleaks.toml` and `.gitleaksignore`;
- `CODEOWNERS` lines that make changes to the caller, the config, the allow-list and the gitleaks files need its owner's review.

A missing config value SHALL take the gate's default: 2,000 characters, the legacy flag off, no allowed email domains and no allow-list. The caller MAY add jobs of its own, such as a summary comment, which are not part of the gate.

#### Scenario: Sage's values
- **WHEN** `sagehaven-docs` sets `allowed_email_domains` to `sagehaven.ai` and `sagesocial.com` in its config
- **THEN** addresses at those domains pass, and no other brain's scan allows them

### Requirement: Gate releases are versioned and tested
Gate releases SHALL be tagged `brain-gate-vMAJOR.MINOR.PATCH`, with a moving `brain-gate-vMAJOR` tag. A change that can fail a PR the previous release passed SHALL be a new major version. The fixture tests SHALL run in Wiffletree's CI on every change under `gate/`, and a release SHALL be tagged only from a commit where they pass. Each tag SHALL stay published if the gate later moves to another repository.

#### Scenario: A stricter rule
- **WHEN** a change makes `sensitive-info` report a pattern it did not report before
- **THEN** it is released as a new major version, and brains pinned to the old one are unaffected until they update

### Requirement: Set up installs and updates the gate through a PR
Set up for a brain SHALL open one PR adding the caller pinned to the current gate release, the `gate` config, an empty allow-list and the `CODEOWNERS` lines, with the owner the human chooses. When a newer release than a brain's pin exists, the Brains page SHALL show both versions and offer Update gate. Update gate SHALL open a PR changing only the caller's pins and the config's `version`. Both PRs SHALL go through the brain's batch flow and SHALL wait for the code owner's review. When this Wiffletree writes an entry contract the brain's pinned gate does not accept, the Brains page SHALL warn and hold that brain's log batches until the gate is updated.

#### Scenario: Setting up vyttle-docs
- **WHEN** the human sets up `vyttle-docs`, which has no gate
- **THEN** one PR adds the caller, the `gate` config, an empty allow-list and `CODEOWNERS` lines, and it waits for the owner's review

#### Scenario: An update is available
- **WHEN** `sagehaven-docs` pins `brain-gate-v1.0.0` and `brain-gate-v1.1.0` is released
- **THEN** the Brains page shows both, and Update gate opens a PR changing only the pins and the config's version
