"""Secret scan: gitleaks over the commits this PR adds.

gitleaks runs with the gate's config/gitleaks.toml, extended by the brain's
`.gitleaks.toml` when one exists. `.gitleaksignore` and `gitleaks:allow` work
as gitleaks defines them. gitleaks writes its findings, redacted, to a JSON
report outside the repository; this check prints only each finding's file,
line, column and rule, never the secret or the line around it.
"""

import json
import os
import re
import subprocess
import tempfile
from pathlib import Path

from common import GATE_FIXTURES, base_ref, config_error, finish, read_text, report, scanning_gate_repo

GATE_CONFIG = Path(__file__).resolve().parents[1] / "config" / "gitleaks.toml"
BRAIN_CONFIG = ".gitleaks.toml"
_EXTEND_TABLE = re.compile(r"^\s*\[extend\]", re.MULTILINE)


def config_path(scratch: Path) -> Path:
    """The gate's config, extended by the brain's config or, in the gate's own
    repository, by an exemption for the gate's fixtures."""
    if scanning_gate_repo():
        extension = f"[allowlist]\ndescription = \"Gate fixtures\"\npaths = ['''^{GATE_FIXTURES}''']\n"
    elif os.path.exists(BRAIN_CONFIG):
        extension = read_text(BRAIN_CONFIG)
        if _EXTEND_TABLE.search(extension):
            config_error(f"{BRAIN_CONFIG} must not have an [extend] table; the gate extends its own config with this file")
    else:
        return GATE_CONFIG
    combined = scratch / "gitleaks.toml"
    # A TOML basic string is written like a JSON string.
    combined.write_text(f"{extension}\n\n[extend]\npath = {json.dumps(str(GATE_CONFIG))}\n", encoding="utf-8")
    return combined


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="brain-gate-") as scratch:
        findings_file = Path(scratch) / "findings.json"
        result = subprocess.run(
            [
                "gitleaks", "git",
                "--config", str(config_path(Path(scratch))),
                "--log-opts", f"{base_ref()}..HEAD",
                "--redact",
                "--report-format", "json",
                "--report-path", str(findings_file),
                "--exit-code", "0",
                "--no-banner",
                "--no-color",
                ".",
            ],
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            # Only gitleaks' own errors reach here; findings go to the report.
            # Its message can quote the config's or the commits' text, so it is not repeated.
            config_error(f"gitleaks could not scan (exit {result.returncode}); run it locally to see why")
        findings = json.loads(findings_file.read_text(encoding="utf-8") or "[]")

    for finding in findings:
        report(
            "error",
            f"possible secret at column {finding['StartColumn']} [rule: {finding['RuleID']}]",
            finding["File"],
            finding["StartLine"],
        )
    finish("secret-scan", len(findings))


if __name__ == "__main__":
    main()
