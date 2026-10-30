# The brain gate

The brain gate is a set of deterministic checks that every pull request to a
brain repository's `main` must pass before it auto-merges. Wiffletree owns the
checks, the log entry contract and their fixture tests, and ships them as this
composite GitHub Action. A brain repository holds only a thin caller pinned to
a release, and its own config. No check calls a model, and none prints a
matched value.

```text
gate/
  action.yml       the composite action; input `check` names one check
  checks/          the checks (Python 3.11+, standard library only) and markdown-lint.sh
  config/          the default gitleaks and markdownlint-cli2 configs
  tests/           the fixture tests and fixtures/, the one fixture set for the entry contract
```

## The checks

Each check is its own job in the caller, so each is its own check run and can
be required.

| Check | What fails it | Runs |
| --- | --- | --- |
| `secret-scan` | gitleaks finds a secret in the PR's commits | `checks/secret_scan.py` |
| `sensitive-info` | A line the PR adds holds child PII or a credential (see below) | `checks/sensitive_scan.py` |
| `markdown-lint` | markdownlint-cli2 errors under `config/markdownlint-cli2.jsonc` | `checks/markdown-lint.sh` |
| `wikilinks` | A `[[link]]` in `notes/` or `STATUS.md` names no existing note | `checks/wikilinks.py` |
| `log-schema` | A file under `logs/` breaks the entry contract or the size cap | `checks/log_schema.py` |
| `path-ownership` | The PR mixes PR classes, or edits or deletes a log entry | `checks/path_ownership.py` |
| `gate-tests` | The pinned gate's self-test fails on this runner | `tests/test_gate.py` |

Every finding names the file, line, column where it has one, and rule. In
GitHub Actions each finding is also an annotation with the file and line. No
check prints the matched text or the line around it: gitleaks writes a
redacted report that the check reads, markdownlint's descriptions and `[…]`
details are dropped, and neither tool's error messages are repeated.

`gate-tests` runs the gate's bundled fixture tests against the tools the action
installed, on the brain's runner. That proves the pinned gate works there. The
same tests run in Wiffletree's CI on every change under `gate/`.

### PR classes

`path-ownership` classifies a PR by the files it changes. `<brain>` is the
brain folder, `second-brain` by default.

- **Log PR:** only *adds* files under `<brain>/logs/<repo>/`. Modifying,
  deleting or moving an entry fails.
- **Compile PR:** changes only `<brain>/notes/**`, `<brain>/STATUS.md` and
  `<brain>/reconcile-queue.md`.
- **Other docs PR:** touches nothing under `<brain>/`.

A PR that mixes classes fails, and so does a change to any other path under
`<brain>/`. Because log PRs only add new files and compile PRs never touch
`logs/`, open PRs cannot conflict with each other, so no merge queue is needed.

**Transition rule:** edits to legacy `<brain>/logs/<repo>.md` files belong to
no class. They only warn while `gate.legacy_log_edits_allowed` is true, and
fail otherwise.

### Log entry contract

Each entry is one file at `<brain>/logs/<repo>/YYYY/MM/YYYY-MM-DD-<id>.md`:

```markdown
---
id: <ulid, matches filename>
repo: <repo, matches folder>
author: <github login>
source: wiffletree | skill
---
## YYYY-MM-DD — Short title
- type: decision | observation
- ref: <branch> @ <sha>[…<sha>][ · PR #NN]   (or <branch> @ —, or —)
- changed: …
- why: …
- state: …
```

`log-schema` enforces these rules:

- `<repo>` starts with a letter or digit and holds only letters, digits, `.`,
  `_` and `-`. `<id>` is a lowercase ULID (26 Crockford base32 characters).
  The `YYYY/MM` folders and the file name's date agree and form a real date.
- The frontmatter has exactly the four keys above, one `key: value` per line.
  `id` and `repo` match the path; `author` is a valid GitHub login.
- The body starts with the `##` heading, whose date matches the file name and
  whose title (after the em dash) is not empty.
- Then come exactly the five fields, in the order shown, each with a value.
  A field may continue onto indented lines.
- `ref` is `—` (no branch), `<branch> @ —` (uncommitted work on a branch),
  or `<branch> @ <sha>`, optionally `…<sha>` for a range and
  `· PR #NN` (after a space). SHAs are 7–40 lowercase hex characters.
- The body (everything after the frontmatter, trimmed) holds at most
  `gate.max_entry_body_chars` characters, 2,000 by default.

`tests/fixtures/entries/` holds good and bad entries. Wiffletree's log renderer
is tested against the same files, so the writer and the gate cannot disagree.

### Sensitive-information scan

`sensitive-info` scans only the lines a PR adds, so existing content is not
re-reported. Rules:

| Rule | Catches |
| --- | --- |
| `phone` | E.164 numbers, with or without separators, and US numbers such as `(NXX) NXX-XXXX`, `NXX-NXX-XXXX`, `NXX.NXX.XXXX` or a bare `NXXNXXXXXX` |
| `email` | Email addresses outside `gate.allowed_email_domains` (and their subdomains) |
| `street-address` | A house number, capitalised street name and street suffix; PO boxes |
| `gps` | Decimal latitude/longitude pairs with four or more decimals; labelled `lat`/`lng` values |
| `aws-access-key`, `aws-secret-key` | AWS access key IDs; secret keys assigned next to an `aws…secret` name |
| `private-key` | PEM private key blocks |
| `token` | GitHub, Slack, Stripe live, OpenAI/Anthropic-style and JWT tokens |
| `url-credentials` | A password embedded in a URL |
| `secret-assignment` | `password`, `secret`, `api_key`, `access_token` or similar assigned a literal of 12+ characters |

US numbers need an area code and exchange starting with 2–9. The one built-in
exception is a separator-free digit run written directly after a CI run or job
context: `runs/`, `jobs/`, or the word `run`, `run ID` or `job` followed by a
space, `#` or `:`. Any other ID that looks like a phone number needs an
explicit suppression. Street addresses and coordinates are best effort. Files
under `gate/tests/` are exempt, because the gate's fixtures plant fake values.

### Suppressing a false positive

Suppression is always explicit and visible in the PR diff:

- **Inline marker** for a one-off: put
  `<!-- gate:allow <rule>[,<rule>] <reason> -->` on the same line. It must
  name the rule and give a reason; it hides only that rule on that line, and it
  does not render in Markdown.
- **Allow-list file** for recurring values or paths: the TOML file that
  `gate.allowlist` names. A `[[value]]` entry exempts one exact matched text
  (case-insensitive) for one `rule`; a `[[path]]` entry exempts files by `glob`,
  where `*` also crosses `/`. Every entry needs a `reason`, or the scan stops.
- **gitleaks** false positives use gitleaks' own mechanisms: a
  `gitleaks:allow` comment on the line, a `.gitleaksignore` fingerprint, or an
  allowlist in the brain's `.gitleaks.toml`.

## A brain's config

The brain's settings live in `wiffletree-brain.json` at its repository root:
the brain folder in `brain_dir` and the gate's settings in a `gate` object.

```json
{
  "version": 1,
  "brain_dir": "second-brain",
  "gate": {
    "version": "brain-gate-v1.0.0",
    "max_entry_body_chars": 2000,
    "legacy_log_edits_allowed": true,
    "allowed_email_domains": ["example.org"],
    "allowlist": "brain-gate-allowlist.toml"
  }
}
```

| Key | Default | Meaning |
| --- | --- | --- |
| `brain_dir` | `second-brain` | The brain folder |
| `gate.version` | — | The release the caller pins; the checks do not read it |
| `gate.max_entry_body_chars` | 2000 | The entry body's size cap |
| `gate.legacy_log_edits_allowed` | `false` | Whether edits to legacy logs only warn |
| `gate.allowed_email_domains` | none | Email domains `sensitive-info` does not report |
| `gate.allowlist` | none | The allow-list file; when named, it must exist |

A missing file, `gate` object or key takes the default. Organization values
live only in the brain repository, never in Wiffletree.

gitleaks uses the gate's `config/gitleaks.toml`, which extends gitleaks'
default rules. A brain's own `.gitleaks.toml`, when present, extends that in
turn; it must not have an `[extend]` table of its own. markdownlint always uses
the gate's config, so every brain lints alike.

## The caller

A brain repository runs the gate from `.github/workflows/brain-gate.yml`. Pin
the action to a release's commit SHA, with the tag in a comment, in every job:

```yaml
# The brain gate (Vyttle-LLC/wiffletree/gate). Each job is its own required check.
name: Brain gate

on:
  pull_request:
    branches: [main]
    types: [opened, synchronize, reopened]

permissions:
  contents: read

concurrency:
  group: brain-gate-${{ github.event.pull_request.number }}
  cancel-in-progress: true

jobs:
  secret-scan:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
        with:
          fetch-depth: 0
      - uses: Vyttle-LLC/wiffletree/gate@<sha> # brain-gate-v1.0.0
        with:
          check: secret-scan

  sensitive-info:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
        with:
          fetch-depth: 0
      - uses: Vyttle-LLC/wiffletree/gate@<sha> # brain-gate-v1.0.0
        with:
          check: sensitive-info

  markdown-lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: Vyttle-LLC/wiffletree/gate@<sha> # brain-gate-v1.0.0
        with:
          check: markdown-lint

  wikilinks:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: Vyttle-LLC/wiffletree/gate@<sha> # brain-gate-v1.0.0
        with:
          check: wikilinks

  log-schema:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: Vyttle-LLC/wiffletree/gate@<sha> # brain-gate-v1.0.0
        with:
          check: log-schema

  path-ownership:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
        with:
          fetch-depth: 0
      - uses: Vyttle-LLC/wiffletree/gate@<sha> # brain-gate-v1.0.0
        with:
          check: path-ownership

  gate-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: Vyttle-LLC/wiffletree/gate@<sha> # brain-gate-v1.0.0
        with:
          check: gate-tests
```

`secret-scan`, `sensitive-info` and `path-ownership` compare the PR with its
base, so they need the full history. The action needs only `contents: read`
and no secret. It installs gitleaks 8.30.1 (checksum-verified) and
markdownlint-cli2 0.24.0, and runs on Linux x64 or arm64 runners with Python
3.11 or later. A brain may add jobs of its own after the seven, such as a
summary comment; they are not part of the gate.

Add `CODEOWNERS` lines so a PR cannot quietly weaken the gate and auto-merge:

```text
/.github/                    @<code-owner>
/wiffletree-brain.json       @<code-owner>
/brain-gate-allowlist.toml   @<code-owner>
/.gitleaks.toml              @<code-owner>
/.gitleaksignore             @<code-owner>
```

## Running a check locally

Run from the repository's root with Python 3.11 or later, and gitleaks or
markdownlint-cli2 on `PATH` for the checks that use them. `GATE_BASE` names
the commit to compare with; it defaults to `origin/$GITHUB_BASE_REF`, then
`origin/main`.

```sh
export GATE_BASE=origin/main
python3 <wiffletree>/gate/checks/secret_scan.py
python3 <wiffletree>/gate/checks/sensitive_scan.py     # --all scans every tracked file instead
bash    <wiffletree>/gate/checks/markdown-lint.sh
python3 <wiffletree>/gate/checks/wikilinks.py
python3 <wiffletree>/gate/checks/log_schema.py
python3 <wiffletree>/gate/checks/path_ownership.py
python3 -m unittest discover --start-directory <wiffletree>/gate/tests --verbose
```

Every check exits with one of these codes:

| Code | Meaning |
| --- | --- |
| 0 | The check passes. Warnings may be printed. |
| 1 | The check has findings, each printed as `ERROR: <file>:<line>: <message>`. |
| 2 | The check could not run: the config, the allow-list or a brain `.gitleaks.toml` is unusable, or gitleaks itself failed. |

`markdown-lint.sh` exits with markdownlint-cli2's own code, which is 1 for
findings and 2 for an error.

The fixture tests build throwaway Git repositories from `tests/fixtures/brain/`
and prove that each check passes good input and fails bad input, and that no
output echoes a planted value.

## Repository settings

The caller only reports. These settings make it the gate. Apply them after the
caller has run once on a PR, so GitHub knows the check names.

1. **Settings → General → Pull Requests:** turn on **Allow auto-merge** and
   **Automatically delete head branches**.
2. **Settings → Branches → Add branch protection rule** for `main`:
   - **Require a pull request before merging**, with **Required approvals** set
     to 0 and **Require review from Code Owners** on. Log and compile PRs need
     no review; PRs that change the gate or its config need the code owner's.
   - Under it, turn on **Allow specified actors to bypass required pull
     requests** and add the code owner. GitHub never counts an author's own
     approval, so a sole code owner could not otherwise merge their own gate
     changes. Every other author still needs their review. Required status
     checks still apply to them.
   - **Require status checks to pass before merging**, requiring `secret-scan`,
     `sensitive-info`, `markdown-lint`, `wikilinks`, `log-schema`,
     `path-ownership` and `gate-tests`, each from GitHub Actions.
   - Leave **Require branches to be up to date before merging** off. Path
     ownership already keeps open PRs from conflicting, and requiring it would
     force a rebase of every queued log PR after each merge.
   - Turn on **Do not allow bypassing the above settings**. The named bypass
     above is a separate, explicit allowance, so this setting is expected to
     leave it working. Check it on the code owner's first gate PR: if GitHub
     does not offer them the bypass merge, turn this setting off
     (`enforce_admins: false`). Force pushes and deletions stay blocked.

The same settings through the GitHub CLI, with `OWNER/REPO` and `<code-owner>`
filled in:

```sh
gh repo edit OWNER/REPO --enable-auto-merge --delete-branch-on-merge

gh api --method PUT repos/OWNER/REPO/branches/main/protection --input - <<'EOF'
{
  "required_status_checks": {
    "strict": false,
    "checks": [
      { "context": "secret-scan", "app_id": 15368 },
      { "context": "sensitive-info", "app_id": 15368 },
      { "context": "markdown-lint", "app_id": 15368 },
      { "context": "wikilinks", "app_id": 15368 },
      { "context": "log-schema", "app_id": 15368 },
      { "context": "path-ownership", "app_id": 15368 },
      { "context": "gate-tests", "app_id": 15368 }
    ]
  },
  "enforce_admins": true,
  "required_pull_request_reviews": {
    "required_approving_review_count": 0,
    "require_code_owner_reviews": true,
    "bypass_pull_request_allowances": { "users": ["<code-owner>"] }
  },
  "restrictions": null,
  "allow_force_pushes": false,
  "allow_deletions": false
}
EOF
```

`15368` is the GitHub Actions app, so only Actions can satisfy the checks.

## Releases

Releases are tagged `brain-gate-vMAJOR.MINOR.PATCH`, with a moving
`brain-gate-vMAJOR` tag.

- A change that can fail a PR the previous release passed is a new major
  version: a stricter rule, a new required field in the entry contract, or a
  new check.
- A fix that only stops false failures, or new optional config, is a minor or
  patch version.

`.github/workflows/brain-gate-tests.yml` runs the fixture tests on every change
under `gate/`. Tag a release only from a commit on `main` where that workflow
passed:

```sh
git fetch origin
git tag -a brain-gate-v1.2.3 -m "Brain gate v1.2.3" <commit>
git tag -f -a brain-gate-v1 -m "Brain gate v1" <commit>
git push origin brain-gate-v1.2.3
git push -f origin brain-gate-v1
```

Callers pin the release's commit SHA, and Wiffletree's Update gate moves the
pin through a reviewed PR. Old tags stay published, including if the gate
moves to its own repository.
