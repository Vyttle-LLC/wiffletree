# The scan checks

Two deterministic checks that Wiffletree bundles and runs on each Mac: before a
push, over the files a sync pass would push, and after a pull, over what it
pulled (see `crates/workspace-host/src/scan.rs`). A file that fails before a
push is held on the Mac and never pushed; a finding after a pull raises an
alert. No check calls a model, and none prints a matched value.

```text
gate/
  checks/          the two checks (Python 3.11+, standard library only) and their config reader
  config/          the default gitleaks config
  tests/           the fixture tests, and fixtures/entries/, the one fixture set for the entry contract
```

## The checks

| Check | What fails it | Runs |
| --- | --- | --- |
| `secret-scan` | gitleaks finds a secret in the commits since the base | `checks/secret_scan.py` |
| `sensitive-info` | A line the change adds holds child PII or a credential (see below) | `checks/sensitive_scan.py` |

Every finding names the file, line where it has one, and rule. No check
prints the matched text or the line around it: gitleaks writes a redacted
report that the check reads, and its error messages are not repeated.

Wiffletree runs both in a scratch repository that holds only the scanned
paths, so no file in a repository, such as a `.gitleaks.toml`, a
`.gitleaksignore` or a `wiffletree-brain.json`, is ever read as a check's
config. Wiffletree writes the scratch repository's `wiffletree-brain.json`
itself, naming only the folder's `allowed_email_domains`.

### Sensitive-information scan

`sensitive-info` scans only the lines a change adds, so existing content is not
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
under `gate/tests/` are exempt only when the checks scan this repository,
whose fixtures plant fake values; anywhere else they are scanned like any
other file.

### Suppressing a false positive

An inline marker on the same line,
`<!-- gate:allow <rule>[,<rule>] <reason> -->`, hides that rule on that line.
It must name the rule and give a reason, and it does not render in Markdown.
gitleaks' own `gitleaks:allow` comment works the same way for `secret-scan`.

In Wiffletree, Not sensitive on a held entry's card adds this marker for a
`sensitive-info` finding; a `secret-scan` finding has no Not sensitive.

## The config

`checks/gate_config.py` reads the `gate` object of `wiffletree-brain.json` at
the scanned repository's root:

| Key | Default | Meaning |
| --- | --- | --- |
| `gate.allowed_email_domains` | none | Email domains `sensitive-info` does not report |
| `gate.allowlist` | none | An allow-list file of exempt values and paths; when named, it must exist |

A missing file, `gate` object or key takes the default. Wiffletree's scratch
repository names only `allowed_email_domains`, so the allow-list is used only
when the checks run by hand.

gitleaks uses `config/gitleaks.toml`, which extends gitleaks' default rules.

## Running a check by hand

Run from the repository's root with Python 3.11 or later, and gitleaks on
`PATH` for `secret-scan`. `GATE_BASE` names the commit to compare with; it
defaults to `origin/$GITHUB_BASE_REF`, then `origin/main`.

```sh
export GATE_BASE=origin/main
python3 <wiffletree>/gate/checks/secret_scan.py
python3 <wiffletree>/gate/checks/sensitive_scan.py     # --all scans every tracked file instead
python3 <wiffletree>/gate/tests/test_gate.py
```

Every check exits with one of these codes:

| Code | Meaning |
| --- | --- |
| 0 | The check passes. Warnings may be printed. |
| 1 | The check has findings, each printed as `ERROR: <file>:<line>: <message>`. |
| 2 | The check could not run: the config, the allow-list or gitleaks itself is unusable. |

The fixture tests build throwaway Git repositories from `tests/fixtures/brain/`
and prove that each check passes good input and fails bad input, and that no
output echoes a planted value. `.github/workflows/brain-gate-tests.yml` runs
them on every change under `gate/`.

## The entry contract's fixtures

`tests/fixtures/entries/` holds good and bad log entries. Wiffletree's log
writer is tested against them in `crates/workspace-host/src/memory.rs`, which
holds the contract's rules.
