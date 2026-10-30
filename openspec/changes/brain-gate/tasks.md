No dependency on the other brain changes, or on `spaces`, for steps 1–3, which may start now. `spaces` task 4.4 runs `sensitive-info` and `secret-scan` from `gate/` on this Mac, so it needs steps 1–2. Step 4 needs `brain-registry-sync`.

## 1. Move the gate into Wiffletree

- [ ] 1.1 Copy `scripts/gate/` (`common.py`, the five Python checks and the two shell checks), `.gitleaks.toml`, `.markdownlint-cli2.jsonc` and `scripts/gate/tests/` from `sagehaven-docs` PR #1 into `gate/checks/`, `gate/config/` and `gate/tests/`. Move the fixtures into `gate/tests/fixtures/`.
- [ ] 1.2 Replace `gate_config.py` with a reader for `wiffletree-brain.json`'s `brain_dir` and `gate` object and its defaults. Read the allow-list from the configured path. Extend the gate's default gitleaks config with the brain's `.gitleaks.toml` when present.
- [ ] 1.3 Prove the fixture tests pass with behaviour unchanged: the same findings, file/line/column/rule output and no echoed values. Add tests for the defaults and for a missing allow-list.

## 2. The action and its CI

- [ ] 2.1 `gate/action.yml`: a composite action with input `check`. It installs gitleaks (pinned, checksum) and markdownlint-cli2 (pinned) as needed, runs the check, and writes annotations (file, line, rule) without text. It has no Sage actions.
- [ ] 2.2 The `gate-tests` self-test mode: run the bundled fixtures against the installed tools.
- [ ] 2.3 `.github/workflows/brain-gate-tests.yml`: run the fixture tests on changes under `gate/`. Document the release tagging in `gate/README.md`.
- [ ] 2.4 `gate/README.md`: the contract, the rules, suppression, running locally, the caller template, the config keys and the repository settings to apply (from PR #1's README).

## 3. Shared fixtures

- [ ] 3.1 Point `brain-log-capture`'s renderer tests at `gate/tests/fixtures/` instead of copies.

## 4. Set up and update

- [ ] 4.1 `brain-registry-sync` Set up: add the caller, the `gate` config, the allow-list and the `CODEOWNERS` lines in one PR. Update gate opens a PR with only the pins and version. Show the pinned and latest versions on the Brains page, warn on a contract mismatch and hold batches.

## 5. Release and adopt

- [ ] 5.1 Tag `brain-gate-v1.0.0` once the tests pass. Hand the `sagehaven-docs` switch to its follow-up ticket: caller, config values from `gate_config.py`, the allow-list renamed, and the Sage summary job kept in the caller.
- [ ] 5.2 Run `openspec validate --strict` and the gate fixture tests before ready_for_testing.
