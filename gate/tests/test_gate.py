"""Fixture tests for the brain gate.

Each test builds a throwaway Git repository from `fixtures/brain/`, commits it
as the PR base, applies the PR's changes on top, then runs the real check
script against it exactly as CI does.

Run: python3 -m unittest discover --start-directory gate/tests --verbose
Needs gitleaks and markdownlint-cli2 on PATH.
"""

import json
import os
import secrets
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

CHECKS_DIR = Path(__file__).resolve().parents[1] / "checks"
FIXTURES = Path(__file__).resolve().parent / "fixtures"
BRAIN_FIXTURE = FIXTURES / "brain"
CONFIG_FILE = "wiffletree-brain.json"
ALLOWLIST = "brain-gate-allowlist.toml"

FINDINGS, CONFIG_ERROR = 1, 2

ULID = "01jbx4m2k8q6v9z3n5r7t0w2yb"
OTHER_ULID = "01jbx4m2k8q6v9z3n5r7t0w2yc"
LEGACY_LOG = "second-brain/logs/sms-service.md"
EXISTING_ENTRY = f"second-brain/logs/sms-service/2026/10/2026-10-01-{OTHER_ULID}.md"


def entry_path(repo: str = "sms-service", date: str = "2026-10-29", ulid: str = ULID) -> str:
    year, month, _ = date.split("-")
    return f"second-brain/logs/{repo}/{year}/{month}/{date}-{ulid}.md"


def entry_text(
    *,
    ulid: str = ULID,
    repo: str = "sms-service",
    date: str = "2026-10-29",
    changed: str = "Retry failed sends once with backoff.",
) -> str:
    """The `good/decision.md` fixture, with these values in place of its own."""
    return (
        f"---\nid: {ulid}\nrepo: {repo}\nauthor: gate-tester\nsource: wiffletree\n---\n"
        f"## {date} — SMS retry policy\n"
        "- type: decision\n"
        "- ref: feature/sms-retry @ 1a2b3c4…5d6e7f8 · PR #42\n"
        f"- changed: {changed}\n"
        "- why: Carrier errors are often transient.\n"
        "- state: Merged; no deploy yet.\n"
    )


def fixture_entries(kind: str) -> list[Path]:
    return sorted((FIXTURES / "entries" / kind).glob("*.md"))


class GateRepo:
    """A temporary repository whose base commit is the PR's merge base."""

    def __init__(self, *, with_config: bool = True) -> None:
        self.root = Path(tempfile.mkdtemp(prefix="brain-gate-"))
        self.git("init", "--quiet", "--initial-branch=main")
        shutil.copytree(BRAIN_FIXTURE, self.root, dirs_exist_ok=True)
        if not with_config:
            self.delete(CONFIG_FILE)
            self.delete(ALLOWLIST)
        self.gate = CHECKS_DIR.parent
        self.commit("base")
        self.git("tag", "base")

    def git(self, *args: str) -> None:
        subprocess.run(
            ["git", "-c", "user.name=Gate Test", "-c", "user.email=gate@example.org", *args],
            cwd=self.root,
            check=True,
            capture_output=True,
        )

    def write(self, path: str, text: str) -> None:
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def write_gate_config(self, **gate) -> None:
        config = json.loads((BRAIN_FIXTURE / CONFIG_FILE).read_text())
        config["gate"].update(gate)
        self.write(CONFIG_FILE, json.dumps(config))

    def delete(self, path: str) -> None:
        (self.root / path).unlink()

    def commit(self, message: str = "pr") -> None:
        self.git("add", "--all")
        self.git("commit", "--quiet", "--allow-empty", "-m", message)

    def rebase_here(self, message: str) -> None:
        """Commit the working tree into the PR's base."""
        self.commit(message)
        self.git("tag", "--force", "base")

    def become_gate_repo(self) -> None:
        """Put a copy of the gate at gate/ in the base and run that copy, as in Wiffletree's own repository."""
        for part in ("checks", "config"):
            shutil.copytree(CHECKS_DIR.parent / part, self.root / "gate" / part, ignore=shutil.ignore_patterns("__pycache__"))
        self.gate = self.root / "gate"
        self.rebase_here("base with the gate")

    def run(self, *command: str) -> subprocess.CompletedProcess[str]:
        env = {k: v for k, v in os.environ.items() if not k.startswith("GITHUB_")}
        env["GATE_BASE"] = "base"
        return subprocess.run(command, cwd=self.root, env=env, capture_output=True, text=True)

    def check(self, script: str) -> subprocess.CompletedProcess[str]:
        return self.run(sys.executable, str(self.gate / "checks" / script))

    def cleanup(self) -> None:
        shutil.rmtree(self.root)


class GateTestCase(unittest.TestCase):
    script = ""
    with_config = True

    def setUp(self) -> None:
        self.repo = GateRepo(with_config=self.with_config)
        self.addCleanup(self.repo.cleanup)

    def pr(self, files: dict[str, str] | None = None, deleted: tuple[str, ...] = ()):
        for path, text in (files or {}).items():
            self.repo.write(path, text)
        for path in deleted:
            self.repo.delete(path)
        self.repo.commit()
        return self.repo.check(self.script)

    def assertPasses(self, result) -> None:
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def assertFails(self, result, *expected: str, code: int = FINDINGS) -> None:
        self.assertEqual(result.returncode, code, result.stdout + result.stderr)
        for text in expected:
            self.assertIn(text, result.stdout + result.stderr)

    def assertConfigError(self, result, *expected: str) -> None:
        self.assertFails(result, *expected, code=CONFIG_ERROR)


class SecretScanTest(GateTestCase):
    script = "secret_scan.py"

    @staticmethod
    def token() -> str:
        # Generated per run so no token-shaped string is ever committed here.
        return "ghp_" + secrets.token_hex(18)

    def test_clean_log_pr_passes(self):
        self.assertPasses(self.pr({entry_path(): entry_text()}))

    def test_planted_secret_fails_without_echoing_it_or_its_line(self):
        token = self.token()
        result = self.pr({entry_path(): entry_text(changed=f"Configured token {token} for CI.")})
        self.assertFails(result, f"{entry_path()}:10: possible secret at column", "[rule: github-pat]")
        self.assertNotIn(token, result.stdout + result.stderr)
        self.assertNotIn("Configured token", result.stdout + result.stderr)

    def test_brain_config_extends_the_gate_config(self):
        self.repo.write(".gitleaks.toml", "[allowlist]\ndescription = \"Fake keys\"\npaths = ['''^docs/fake/''']\n")
        self.repo.rebase_here("base with brain gitleaks config")
        self.assertPasses(self.pr({"docs/fake/key.md": f"{self.token()}\n"}))
        # The gate's default rules still apply outside the brain's exemption.
        self.assertFails(self.pr({"docs/real.md": f"{self.token()}\n"}), "[rule: github-pat]")

    def test_brain_config_with_its_own_extend_is_refused(self):
        self.repo.write(".gitleaks.toml", "[extend]\nuseDefault = true\n")
        self.repo.rebase_here("base with brain gitleaks config")
        self.assertConfigError(self.pr({entry_path(): entry_text()}), "must not have an [extend] table")

    def test_gate_fixtures_are_scanned_in_a_brain(self):
        self.assertFails(self.pr({"gate/tests/fixtures/x.md": f"{self.token()}\n"}), "[rule: github-pat]")

    def test_gate_fixtures_are_exempt_in_the_gate_repo(self):
        self.repo.become_gate_repo()
        self.assertPasses(self.pr({"gate/tests/fixtures/x.md": f"{self.token()}\n"}))
        self.assertFails(self.pr({"docs/x.md": f"{self.token()}\n"}), "[rule: github-pat]")

    def test_gitleaks_errors_are_not_echoed(self):
        token = self.token()
        rule = f'[[rules]]\nid = "x"\nregex = "x"\nsecretGroup = "{token}"\n'
        self.repo.write(".gitleaks.toml", rule)
        self.repo.rebase_here("base with a broken brain gitleaks config")
        result = self.pr({entry_path(): entry_text()})
        self.assertConfigError(result, "gitleaks could not scan")
        self.assertNotIn(token, result.stdout + result.stderr)


class SensitiveScanTest(GateTestCase):
    script = "sensitive_scan.py"

    def assertFlags(self, value: str, rule: str):
        result = self.pr({entry_path(): entry_text(changed=f"Saw {value} in the report.")})
        self.assertFails(result, f"[rule: {rule}]", entry_path() + ":10:")
        self.assertNotIn(value, result.stdout + result.stderr)

    def test_clean_log_pr_passes(self):
        self.assertPasses(self.pr({entry_path(): entry_text()}))

    def test_e164_phone_in_log_fails(self):
        self.assertFlags("+18015550123", "phone")

    def test_spaced_e164_phone_fails(self):
        self.assertFlags("+44 20 7946 0958", "phone")

    def test_us_phone_formats_fail(self):
        for number in ("(801) 555-0123", "801-555-0123", "801.555.0123", "(801) 5550123", "8015550123"):
            with self.subTest(number=number):
                self.setUp()
                self.assertFlags(number, "phone")

    def test_outside_email_fails(self):
        self.assertFlags("kid.parent@example.com", "email")

    def test_allowed_email_domains_pass(self):
        result = self.pr({entry_path(): entry_text(changed="Ask rb@example.org or ops@mail.example.net.")})
        self.assertPasses(result)

    def test_street_address_fails(self):
        self.assertFlags("1234 North Maple Street", "street-address")

    def test_gps_coordinates_fail(self):
        self.assertFlags("12.34567, -45.67891", "gps")

    def test_aws_access_key_fails(self):
        self.assertFlags("AKIA" + secrets.token_hex(8).upper(), "aws-access-key")

    def test_separated_number_with_country_code_fails(self):
        self.assertFlags("1-801-555-0142", "phone")

    def test_run_ids_pass(self):
        noise = "run 18234567890, Run ID 8234567890, https://github.com/o/r/actions/runs/18234567890, job #8234567890"
        self.assertPasses(self.pr({entry_path(): entry_text(changed=noise)}))

    def test_digit_runs_outside_run_context_fail(self):
        for value in ("18015550142", "https://example.invalid/contact/8015550142/"):
            with self.subTest(value=value):
                self.setUp()
                self.assertFlags(value, "phone")

    def test_dates_versions_and_shas_pass(self):
        noise = "2026-10-29, v1.0.0.0, 1763 tests, run 34726450148, sha 849c613, 12.35:1 contrast"
        self.assertPasses(self.pr({entry_path(): entry_text(changed=noise)}))

    def test_inline_marker_with_reason_suppresses(self):
        line = "Support line 801-555-0123 <!-- gate:allow phone published support number -->"
        self.assertPasses(self.pr({entry_path(): entry_text(changed=line)}))

    def test_inline_marker_without_reason_does_not_suppress(self):
        line = "Support line 801-555-0123 <!-- gate:allow phone -->"
        self.assertFails(self.pr({entry_path(): entry_text(changed=line)}), "[rule: phone]")

    def test_marker_for_another_rule_does_not_suppress(self):
        line = "Support line 801-555-0123 <!-- gate:allow email wrong rule -->"
        self.assertFails(self.pr({entry_path(): entry_text(changed=line)}), "[rule: phone]")

    def test_allowlist_value_suppresses(self):
        # support@vendor.example is in the fixture brain's allow-list.
        self.assertPasses(self.pr({entry_path(): entry_text(changed="Emailed support@vendor.example.")}))

    def test_allowlist_entry_without_reason_is_rejected(self):
        self.repo.write(ALLOWLIST, '[[value]]\nrule = "phone"\nmatch = "801-555-0123"\n')
        self.repo.commit("base with bad allow-list")
        result = self.repo.check(self.script)
        self.assertConfigError(result, "[[value]] entry #1 needs a reason")
        self.assertNotIn("801-555-0123", result.stdout + result.stderr)

    def test_configured_allowlist_that_is_missing_is_refused(self):
        self.repo.delete(ALLOWLIST)
        self.assertConfigError(self.pr({entry_path(): entry_text()}), "allow-list named in wiffletree-brain.json does not exist")

    def test_without_an_allowlist_nothing_is_allowed(self):
        self.repo.write_gate_config(allowlist=None)
        self.repo.rebase_here("base without an allow-list")
        self.assertFlags("support@vendor.example", "email")

    def test_gate_fixtures_are_scanned_in_a_brain(self):
        self.assertFails(self.pr({"gate/tests/fixtures/x.md": "Call +18025551234.\n"}), "[rule: phone]")

    def test_gate_fixtures_are_exempt_in_the_gate_repo(self):
        self.repo.become_gate_repo()
        self.assertPasses(self.pr({"gate/tests/fixtures/x.md": "Call +18025551234.\n"}))
        self.assertFails(self.pr({"docs/x.md": "Call +18025551234.\n"}), "[rule: phone]")

    def test_added_line_resembling_a_file_header_is_still_scanned(self):
        text = "intro\n++ /dev/null\n++ b/gate/tests/x\nCall +18025551234.\n"
        self.assertFails(self.pr({"docs/note.md": text}), "docs/note.md:4:", "[rule: phone]")

    def test_existing_lines_are_not_rescanned(self):
        self.repo.write("README.md", "# Docs\n\nCall 801-555-0123.\n")
        self.repo.rebase_here("base with legacy number")
        self.assertPasses(self.pr({"README.md": "# Docs\n\nCall 801-555-0123.\n\nMore docs.\n"}))


class MarkdownLintTest(GateTestCase):
    script = "markdown_lint.py"

    def test_brain_style_passes(self):
        self.assertPasses(self.pr({entry_path(): entry_text()}))

    def test_lint_error_fails(self):
        self.assertFails(self.pr({"README.md": "# Docs\n\n\n\nToo many blanks.\n"}), "ERROR: README.md:3: markdownlint [rule: MD012/")

    def test_findings_never_quote_the_file(self):
        result = self.pr({"README.md": "# Docs\n\n#Planted heading\n"})
        self.assertFails(result, "README.md:3: markdownlint at column 1 [rule: MD018/")
        self.assertNotIn("Planted", result.stdout + result.stderr)

    def test_reference_details_are_not_echoed(self):
        text = "# Docs\n\nSee [the report][Planted 801-555-0123].\n\n[unused planted ref]: https://example.org\n"
        result = self.pr({"README.md": text})
        self.assertFails(result, "README.md:3: markdownlint at column 5 [rule: MD052/", "README.md:5: markdownlint at column 1 [rule: MD053/")
        self.assertNotIn("planted", (result.stdout + result.stderr).lower())
        self.assertNotIn("801-555-0123", result.stdout + result.stderr)

    def test_paths_with_spaces_are_reported(self):
        result = self.pr({"second-brain/notes/topic/Beta Note.md": "# Beta Note\n\n\n\nText.\n"})
        self.assertFails(result, "ERROR: second-brain/notes/topic/Beta Note.md:3: markdownlint [rule: MD012/")

    def test_brain_markdownlint_configs_are_ignored(self):
        self.repo.write(".markdownlint.json", '{"default": false}\n')
        self.repo.write(".markdownlint-cli2.jsonc", '{"config": {"default": false}, "ignores": ["**"]}\n')
        self.repo.write("second-brain/.markdownlint.json", '{"default": false}\n')
        self.repo.rebase_here("base with brain markdownlint configs")
        self.assertFails(self.pr({"second-brain/notes/Gamma.md": "# Gamma\n\n\n\nText.\n"}), "[rule: MD012/")


class WikilinksTest(GateTestCase):
    script = "wikilinks.py"

    def test_resolving_links_pass(self):
        self.assertPasses(self.pr({"second-brain/notes/Gamma.md": "See [[Alpha]] and [[topic/Beta Note|beta]].\n"}))

    def test_broken_link_in_note_fails(self):
        result = self.pr({"second-brain/notes/Gamma.md": "See [[Missing Note]].\n"})
        self.assertFails(result, "second-brain/notes/Gamma.md:1: wikilink at column 5")
        self.assertNotIn("Missing Note", result.stdout + result.stderr)

    def test_broken_link_in_status_fails(self):
        self.assertFails(self.pr({"second-brain/STATUS.md": "# Status\n\n[[Nope]]\n"}), "second-brain/STATUS.md:3:")

    def test_links_inside_code_are_ignored(self):
        text = "Write `[[Like This]]`.\n\n```md\n[[Also Not A Link]]\n```\n"
        self.assertPasses(self.pr({"second-brain/notes/Gamma.md": text}))

    def test_fence_closes_only_on_its_own_delimiter(self):
        text = "~~~markdown\n```\n~~~\n\nSee [[Missing Note]].\n"
        self.assertFails(self.pr({"second-brain/notes/Gamma.md": text}), "Gamma.md:5: wikilink")


class LogSchemaTest(GateTestCase):
    script = "log_schema.py"

    def test_good_fixture_entries_pass(self):
        for fixture in fixture_entries("good"):
            with self.subTest(fixture=fixture.name):
                self.setUp()
                self.assertPasses(self.pr({entry_path(): fixture.read_text()}))

    def test_bad_fixture_entries_fail(self):
        problems = json.loads((FIXTURES / "entries" / "bad" / "problems.json").read_text())
        self.assertEqual(sorted(problems), [fixture.name for fixture in fixture_entries("bad")])
        for fixture in fixture_entries("bad"):
            with self.subTest(fixture=fixture.name):
                self.setUp()
                self.assertFails(self.pr({entry_path(): fixture.read_text()}), *problems[fixture.name])

    def test_template_matches_the_good_fixture(self):
        self.assertEqual(entry_text(), (FIXTURES / "entries" / "good" / "decision.md").read_text())

    def test_errors_never_echo_entry_text(self):
        phone = "+18015550123"
        text = entry_text().replace("source: wiffletree\n", f"source: wiffletree\nCall {phone}: now\n")
        text = text.replace("- why:", f"Call {phone}\n- why:")
        result = self.pr({entry_path(): text})
        self.assertFails(result, ":6: frontmatter line", ":12: unexpected line")
        self.assertNotIn(phone, result.stdout + result.stderr)

    def test_body_at_cap_passes_and_over_cap_fails(self):
        filler_at_cap = "x" * (2000 - len(self.body(entry_text(changed=""))))
        self.assertPasses(self.pr({entry_path(): entry_text(changed=filler_at_cap)}))
        result = self.pr({entry_path(): entry_text(changed=filler_at_cap + "x")})
        self.assertFails(result, "2001 characters; the cap is 2000")

    def test_brain_sets_the_cap(self):
        self.repo.write_gate_config(max_entry_body_chars=100)
        self.repo.rebase_here("base with a smaller cap")
        self.assertFails(self.pr({entry_path(): entry_text()}), "the cap is 100")

    def test_bad_paths_fail(self):
        for path in (
            "second-brain/logs/sms-service/2026/10/2026-10-29-NOT-A-ULID.md",
            "second-brain/logs/sms-service/2026/09/2026-10-29-" + ULID + ".md",
            "second-brain/logs/sms-service/2026/02/2026-02-30-" + ULID + ".md",
            "second-brain/logs/sms-service/notes.txt",
        ):
            with self.subTest(path=path):
                self.setUp()
                self.assertFails(self.pr({path: entry_text()}), "not a legacy log")

    @staticmethod
    def body(text: str) -> str:
        return text.split("---\n", 2)[2].strip()


class PathOwnershipTest(GateTestCase):
    script = "path_ownership.py"

    def test_log_pr_passes(self):
        result = self.pr({entry_path(): entry_text(), entry_path(repo="cortex-service"): entry_text(repo="cortex-service")})
        self.assertPasses(result)
        self.assertIn("PR class: log", result.stdout)

    def test_compile_pr_passes(self):
        result = self.pr(
            {
                "second-brain/notes/Alpha.md": "Rewritten.\n",
                "second-brain/STATUS.md": "# Status\n",
                "second-brain/reconcile-queue.md": "# Queue\n",
            },
            deleted=("second-brain/notes/INDEX.md",),
        )
        self.assertPasses(result)
        self.assertIn("PR class: compile", result.stdout)

    def test_other_docs_pr_passes(self):
        self.assertPasses(self.pr({"README.md": "# Docs\n\nMore.\n", "docs/guide.md": "# Guide\n"}))

    def test_mixed_log_and_compile_pr_fails(self):
        result = self.pr({entry_path(): entry_text(), "second-brain/STATUS.md": "# Status\n"})
        self.assertFails(result, "mixes log files and compile files")

    def test_mixed_compile_and_docs_pr_fails(self):
        result = self.pr({"second-brain/STATUS.md": "# Status\n", "README.md": "# Changed\n"})
        self.assertFails(result, "mixes compile files and other docs files")

    def test_log_pr_modifying_an_entry_fails(self):
        result = self.pr({EXISTING_ENTRY: entry_text(ulid=OTHER_ULID, date="2026-10-01", changed="Edited.")})
        self.assertFails(result, "append-only, but this PR modifies one")

    def test_log_pr_deleting_an_entry_fails(self):
        self.assertFails(self.pr(deleted=(EXISTING_ENTRY,)), "append-only, but this PR deletes one")

    def test_moving_an_entry_fails(self):
        self.repo.git("mv", EXISTING_ENTRY, entry_path(ulid=OTHER_ULID, date="2026-10-02"))
        self.assertFails(self.pr(), "deletes one")

    def test_unowned_brain_path_fails(self):
        self.assertFails(self.pr({"second-brain/scratch.md": "x\n"}), "no PR class may change")

    def test_legacy_log_edit_only_warns(self):
        result = self.pr({LEGACY_LOG: (BRAIN_FIXTURE / LEGACY_LOG).read_text() + "\nMore.\n"})
        self.assertPasses(result)
        self.assertIn(f"WARNING: {LEGACY_LOG}: legacy log edited", result.stdout)

    def test_legacy_log_edit_alongside_compile_still_passes(self):
        result = self.pr({LEGACY_LOG: "# changed\n", "second-brain/notes/Alpha.md": "Rewritten.\n"})
        self.assertPasses(result)

    def test_legacy_log_edit_fails_once_the_flag_flips(self):
        self.repo.write_gate_config(legacy_log_edits_allowed=False)
        self.repo.rebase_here("base with the legacy flag off")
        self.assertFails(self.pr({LEGACY_LOG: "# changed\n"}), "legacy logs are frozen")


class DefaultsTest(GateTestCase):
    """A brain without `wiffletree-brain.json` gets the gate's defaults."""

    with_config = False

    def test_no_email_domain_is_allowed(self):
        self.script = "sensitive_scan.py"
        result = self.pr({entry_path(): entry_text(changed="Ask rb@example.org.")})
        self.assertFails(result, "[rule: email]")

    def test_no_allowlist_is_needed(self):
        self.script = "sensitive_scan.py"
        self.assertFails(self.pr({entry_path(): entry_text(changed="Emailed support@vendor.example.")}), "[rule: email]")

    def test_brain_folder_and_cap_default(self):
        self.script = "log_schema.py"
        self.assertPasses(self.pr({entry_path(): entry_text()}))
        filler = "x" * (2001 - len(LogSchemaTest.body(entry_text(changed=""))))
        self.assertFails(self.pr({entry_path(): entry_text(changed=filler)}), "the cap is 2000")

    def test_legacy_log_edits_fail(self):
        self.script = "path_ownership.py"
        self.assertFails(self.pr({LEGACY_LOG: "# changed\n"}), "legacy logs are frozen")


class ConfigTest(GateTestCase):
    script = "log_schema.py"

    def test_unusable_config_is_refused(self):
        for text, expected in (
            ("{", "is not valid JSON"),
            ('{"gate": {"max_entry_body_chars": true}}', "`gate.max_entry_body_chars` must be a whole number"),
            ('{"gate": {"allowed_email_domains": "example.org"}}', "`gate.allowed_email_domains` must be a list"),
            ('{"brain_dir": "/second-brain"}', "`brain_dir` must be a folder inside the repository"),
            ('{"brain_dir": "../second-brain"}', "`brain_dir` must be a folder inside the repository"),
            ('{"brain_dir": "docs/../../x"}', "`brain_dir` must be a folder inside the repository"),
            ('{"brain_dir": ""}', "`brain_dir` must be a folder inside the repository"),
            ('{"brain_dir": "./"}', "`brain_dir` must be a folder inside the repository"),
        ):
            with self.subTest(text=text):
                self.setUp()
                self.repo.write(CONFIG_FILE, text)
                self.assertConfigError(self.pr(), expected)

    def test_brain_dir_is_normalized(self):
        for brain_dir in ("./second-brain", "second-brain/", "./second-brain//"):
            with self.subTest(brain_dir=brain_dir):
                self.setUp()
                self.repo.write(CONFIG_FILE, json.dumps({"brain_dir": brain_dir}))
                self.repo.rebase_here("base with brain_dir")
                self.assertPasses(self.pr({entry_path(): entry_text()}))
                result = self.pr({"second-brain/logs/sms-service/notes.txt": "x\n"})
                self.assertFails(result, "second-brain/logs/sms-service/notes.txt: not a legacy log")


if __name__ == "__main__":
    unittest.main()
