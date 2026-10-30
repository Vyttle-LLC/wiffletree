"""Markdown lint: markdownlint-cli2 with the gate's config, so every brain lints alike.

The tracked Markdown files are copied into a scratch folder and linted there,
so no `.markdownlint*` file in the brain can change the gate's rules. The
gate's output formatter writes each finding's file, line, column, severity and
rule as JSON; markdownlint's own output, which can quote the files' text, is
discarded.
"""

import json
import os
import shutil
import tempfile
from pathlib import Path

from common import config_error, finish, report, run_tool, tracked_files

GATE_CONFIG = Path(__file__).resolve().parents[1] / "config" / "markdownlint-cli2.jsonc"


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="brain-gate-") as scratch:
        files = Path(scratch) / "files"
        for path in tracked_files("*.md"):
            if os.path.isfile(path) and not os.path.islink(path):
                (files / path).parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, files / path)
        files.mkdir(exist_ok=True)

        findings_file = Path(scratch) / "findings.json"
        result = run_tool(
            ["markdownlint-cli2", "--config", str(GATE_CONFIG)],
            cwd=files,
            env={**os.environ, "BRAIN_GATE_MARKDOWNLINT_REPORT": str(findings_file)},
        )
        if result.returncode not in (0, 1) or not findings_file.exists():
            config_error(f"markdownlint-cli2 could not lint (exit {result.returncode}); run it locally to see why")
        findings = json.loads(findings_file.read_text(encoding="utf-8"))

    for finding in findings:
        column = f" at column {finding['column']}" if finding["column"] else ""
        report(finding["severity"], f"markdownlint{column} [rule: {finding['rule']}]", finding["file"], finding["line"])
    errors = sum(finding["severity"] == "error" for finding in findings)
    finish("markdown-lint", errors, len(findings) - errors)


if __name__ == "__main__":
    main()
