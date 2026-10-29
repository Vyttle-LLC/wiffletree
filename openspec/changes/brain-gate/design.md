## Context

The reference implementation is `sagehaven-docs` PR #1, on the branch `wiffletree/brain-pr-gate-lint-sensitive-data-scans-log-sche`:
- `.github/workflows/brain-gate.yml` runs seven jobs on every pull request to `main`, each its own check run: `secret-scan`, `sensitive-info`, `markdown-lint`, `wikilinks`, `log-schema`, `path-ownership` and `gate-tests`. A `summary` job posts one comment through Sage's `sage-pr-check-aggregator`.
- Each job runs a script in `scripts/gate/` through a local `gate-check` action, which wraps Sage's `sage-run-command`.
- `scripts/gate/gate_config.py` holds the tunable values: the brain folder, `MAX_ENTRY_BODY_CHARS = 2000`, `LEGACY_LOG_EDITS_ALLOWED = True`, `ALLOWED_EMAIL_DOMAINS = ("sagehaven.ai", "sagesocial.com")` and the allow-list path.
- `scripts/gate/sensitive-allowlist.toml` holds `[[value]]` and `[[path]]` entries, each with a reason.
- `.gitleaks.toml` and `.markdownlint-cli2.jsonc` sit at the repository root, and `CODEOWNERS` makes gate changes need Michael's review.
- The README documents the PR classes, the entry contract, the sensitive rules, suppression, running the gate locally, and the repository settings Michael applies.

Wiffletree is a public repository (`Vyttle-LLC/wiffletree`). Brain repositories are private.

## Goals / Non-Goals

**Goals:**
- One gate implementation and one entry contract for every brain, owned and versioned by Wiffletree.
- The same seven checks, with the same names and behaviour as PR #1, so the required checks and branch protection on `sagehaven-docs` keep working.
- Brain repositories hold only a pinned caller and their own config.
- The gate can move to its own repository later without breaking callers.

**Non-Goals:**
- New checks or changed rules. Any change after the move is a normal versioned gate change.
- Applying GitHub repository settings. Michael applies them, and the gate README lists them.
- Model-based review of brain PRs. There is none, by design.
- Moving the code in this proposal. The tasks do that after approval.

## Decisions

### Decided by Michael (October 29, 2026)

- Wiffletree owns the gate logic (checks, contract, fixture tests, versioning) and ships it as a reusable GitHub Action from this public repository, so it may later move to its own repository.
- A brain repository carries only a thin caller pinned to a version, plus its own config: allowed email domains, the value and path allow-list, the size cap, the legacy flag and `CODEOWNERS`. Org-specific values never go into Wiffletree.
- Set up in `brain-registry-sync` installs or updates the caller and config through a PR.
- The checks and their behaviour are exactly as in PR #1: seven checks, the entry contract, no echoed values, explicit suppression only.

### Decided in this proposal, for review

1. **A composite action called once per check, not a reusable workflow.** The caller has seven jobs, each named after its check, each running `uses: Vyttle-LLC/wiffletree/gate@<sha> # brain-gate-v1.0.0` with `with: { check: <name> }`.
   - A reusable workflow would name its check runs `<caller job> / <check>`, which would change the required-check names on every brain.
   - The cost is a caller of about 60 lines instead of 10. It is still "thin": it holds no logic, only the seven calls.
2. **The action is self-contained.** It installs gitleaks at a pinned version with a checksum, and uses `actions/setup-node` and `markdownlint-cli2` at a pinned version. It runs the Python checks with the runner's Python 3.11 or later, standard library only.
   - It does not depend on `Sage-Social/github-actions`; PR #1's `gate-check` wrapper and `sage-run-command` are org-specific and stay out.
   - Each check writes its findings (file, line, column, rule) to the job log and as GitHub annotations, never the matched text.
   - It runs with `contents: read` only and needs no secret.
3. **The summary comment is not part of the gate.** PR #1's `summary` job uses Sage's aggregator. A brain may keep such a job in its own caller after the seven checks; Wiffletree's caller template has none.
4. **Brain config lives beside the brain's other settings:**
   - `wiffletree-brain.json` gains a `gate` object:
     ```json
     "gate": {
       "version": "brain-gate-v1.0.0",
       "max_entry_body_chars": 2000,
       "legacy_log_edits_allowed": true,
       "allowed_email_domains": ["sagehaven.ai", "sagesocial.com"],
       "allowlist": "brain-gate-allowlist.toml"
     }
     ```
   - The brain folder comes from the file's existing `brain_dir`.
   - The allow-list keeps PR #1's TOML format.
   - Missing values take the gate's defaults: 2,000 characters, the legacy flag false, no allowed email domains and no allow-list.
   - The action reads gitleaks rules from its own default config, extended by a `.gitleaks.toml` in the brain repository when one exists. `.gitleaksignore` and `gitleaks:allow` work as in PR #1.
   - markdownlint uses the gate's `markdownlint-cli2.jsonc`, tuned to the brain format, so a brain cannot lint differently.
5. **`gate-tests` keeps its name and changes its subject.** In PR #1 it ran the fixture tests that lived in the brain. Now Wiffletree's own CI runs them on every change to `gate/`, and a release is tagged only from a commit where they pass. In a brain, `gate-tests` runs the pinned action's self-test: the bundled fixtures against the installed tools, on that runner. This proves the pinned gate works there, and keeps seven required checks.
6. **Versions:**
   - Releases are tagged `brain-gate-vMAJOR.MINOR.PATCH`, with a moving `brain-gate-vMAJOR` tag.
   - A change that can fail a PR the previous version passed is a new major version. That covers a stricter rule, a new required field in the entry contract, or a new check.
   - Fixes that only stop false failures, and new optional config, are minor or patch versions.
   - The caller pins a commit SHA with the tag in a comment, and Update gate moves the pin. The `version` in the brain's config records which release the caller pins.
7. **One fixture set.** `gate/tests/fixtures/` holds the good and bad entries, notes, PR shapes and sensitive samples. Wiffletree's renderer tests in `brain-log-capture` read the same files, so a contract change fails Wiffletree's own tests in the same PR. The fixtures contain only fake values, and the gate's default path allow-list exempts `gate/tests/`.
8. **Moving later:** the action's only coupling to this repository is its path. Moving it means publishing the same tags in the new repository and changing each caller's `uses:`, through Update gate. Old tags stay in this repository, so pinned callers keep working.
9. **Set up writes, in one PR from `brain-registry-sync`:**
   - the caller;
   - the `gate` object;
   - an empty allow-list with its header comment;
   - `CODEOWNERS` lines for `.github/`, `wiffletree-brain.json`, the allow-list and `.gitleaks*`, with an owner the human chooses.

   Because `CODEOWNERS` makes those files need review, set-up and update PRs wait for the owner's review and do not auto-merge. Log and compile PRs touch none of them and still auto-merge.

### Options considered

- **Keep the gate in each brain.** This was rejected by Michael: the gate and the contract would drift.
- **A reusable workflow.** This was rejected by decision 1, because it renames the check runs.
- **A separate repository now.** This was deferred: the gate is small, the public Wiffletree repository works today, and decision 8 keeps the move cheap.

## Risks / Trade-offs

- **[The public repository holds the scan rules]** → The rules are generic patterns with no org data. The org values and allow-lists stay in private brains. The fixtures use only fake values.
- **[A supply-chain change to the gate auto-merges into brains]** → No: callers pin a SHA, and an update is a reviewed PR guarded by `CODEOWNERS`.
- **[A breaking contract change strands older installs]** → `brain-log-capture` writes the contract of the gate version the brain pins. The Brains page warns when this Wiffletree writes a newer contract than the brain's gate accepts, and offers Update gate first.
- **[Toolchain drift on runners]** → gitleaks and markdownlint versions are pinned in the action, and the in-brain `gate-tests` self-test catches a broken runner.

## Migration Plan

1. Copy `scripts/gate/`, the default configs and the fixture tests from `sagehaven-docs` PR #1 into `gate/`. Replace `gate_config.py` with reading `wiffletree-brain.json`, and replace the `sage-run-command` wrapper with plain steps that write annotations. Prove the fixture tests pass unchanged in behaviour.
2. Tag `brain-gate-v1.0.0`.
3. In `sagehaven-docs` (follow-up ticket): replace the scripts and workflow with the caller pinned to v1.0.0, move the `gate_config.py` values into `wiffletree-brain.json`, rename the allow-list, and keep the Sage summary job in the caller. The check names are unchanged, so branch protection stays as applied.
4. `vyttle-docs` gets the gate through Set up.

## Overlaps with other proposals

These are merged on `main`:
- PR #162 (`shared-issues`), as 6194b99a;
- PR #163 (`assistant-spaces-jobs-boards`), as 419a55ef;
- PR #164 (`create_project`), as a8dded2d.

#162's and #163's changes are open there, not yet archived; #164's is archived. Where they change the same thing as this change, this change goes after them.

| Shared concept | Where else | Share or align | Open question |
| --- | --- | --- | --- |
| Checks on writes to a shared repository | #162's tracker accepts direct pushes and commits from teammates outside Wiffletree, with no CI | **Align**: the gate's sensitive-information scan is generic and could guard a tracker too, because issue text can carry child PII just as log text can. | Should trackers get a caller of the same action (only `sensitive-info` and `secret-scan`), run as checks on pushes to `main` since trackers have no PRs? |
| PR-gated writes | `brain-registry-sync` batches; `ticket-publishing` opens ticket PRs | **Share** the PR watcher. Brain PRs read the gate's seven check names. | None. |
| CI shipped from Wiffletree | This is the first GitHub Action Wiffletree publishes for other repositories | **Align** future reusable pieces, for example a tracker gate, under one versioning scheme (`<name>-vX.Y.Z` tags). | Move to a separate `Vyttle-LLC/wiffletree-actions` repository when a second action exists? |
| Repository config in a shared repository | `wiffletree-brain.json` here; `wiffletree-issues.toml` in #162 | **Align** on one config file per shared repository, at its root, with unknown keys kept. | JSON for brains and TOML for trackers: pick one format before both ship? |
| `CODEOWNERS` and auto-merge | Brains need review for gate and config changes; trackers push directly | No overlap today. | None. |
| The entry contract | `brain-log-capture`'s renderer | **Share**: one fixture set in `gate/tests/fixtures/`. | None. |
| Ticket review and inspector panels | `quiet-findings`, `files-panel`, `processes-panel` | None. | None. |
| Sensitive data in shared outputs | #163 (merged): a shared job's first output waits for the runner's approval, and Space storage syncs by direct push with the settings repository's rules | **Align**: the gate's `sensitive-info` and `secret-scan` checks are generic and could run on pushes to a Space's storage repository, as with trackers. | **Open:** should Space storage repositories get a caller of the gate? |
| Brain writes | #163's defaulted decision (c), a direct push with union merge, would bypass the gate | **Decided by Michael, 2026-10-29:** PR-gated writes supersede (c), so every brain write passes this gate. | None. |
| Projects created by a coordinator | #164 (merged) | None. | None. |

## Open Questions

- Whether Wiffletree's caller template should include an optional summary job that needs no Sage action. It is not needed for the gate.
