"""Git and reporting helpers shared by the gate checks.

Every check runs from the repository root and compares HEAD against the pull
request's base: `$GATE_BASE` if set, else `origin/$GITHUB_BASE_REF`, else
`origin/main`.

Exit codes: 0 when the check passes, 1 when it has findings, 2 when the
brain's gate config cannot be used.
"""

import os
import re
import subprocess
import sys
from dataclasses import dataclass
from typing import Iterator, NoReturn


def run_tool(command: list[str], **options) -> subprocess.CompletedProcess[str]:
    """Run a tool the gate needs, stopping with exit code 2 when it is not installed."""
    try:
        return subprocess.run(command, capture_output=True, text=True, **options)
    except FileNotFoundError:
        config_error(f"{command[0]} is not installed or not on PATH")


def git(*args: str) -> str:
    result = run_tool(["git", "-c", "core.quotePath=false", *args])
    if result.returncode != 0:
        # Git's message can quote paths or refs, so it is not repeated.
        config_error(f"git {args[0]} failed (exit {result.returncode})")
    return result.stdout


def base_ref() -> str:
    explicit = os.environ.get("GATE_BASE")
    if explicit:
        return explicit
    return "origin/" + (os.environ.get("GITHUB_BASE_REF") or "main")


def pr_range() -> str:
    """The commits this pull request adds, measured from the merge base."""
    return f"{base_ref()}...HEAD"


@dataclass(frozen=True)
class Change:
    status: str  # A (added), M (modified) or D (deleted)
    path: str


def changed_files() -> list[Change]:
    # Without rename detection a rename shows as a delete plus an add, so a
    # moved log entry counts as a deletion.
    output = git("diff", "--name-status", "--no-renames", "-z", pr_range())
    fields = output.split("\0")
    changes = []
    for status, path in zip(fields[0::2], fields[1::2]):
        changes.append(Change("M" if status == "T" else status, path))
    return changes


# The repository the gate itself lives in: Wiffletree, or wherever it moves.
GATE_REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.realpath(__file__))))
# Its fixtures plant fake values on purpose, so they are exempt there, and only there.
GATE_FIXTURES = "gate/tests/"


def scanning_gate_repo() -> bool:
    """Whether the repository being checked is the gate's own."""
    return os.path.realpath(git("rev-parse", "--show-toplevel").strip()) == GATE_REPO_ROOT


def tracked_files(*pathspecs: str) -> list[str]:
    return [path for path in git("ls-files", "-z", "--", *pathspecs).split("\0") if path]


@dataclass(frozen=True)
class AddedLine:
    path: str
    number: int
    text: str


_HUNK = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@")


def added_lines() -> Iterator[AddedLine]:
    """Every line the pull request adds, with its line number in the new file."""
    diff = git("diff", "--no-color", "--no-ext-diff", "--no-renames", "--unified=0", pr_range())
    path = None
    number = 0
    # `+++` names the file only in a file's header, before its first hunk;
    # inside a hunk it is an added line that starts with `++`.
    in_header = False
    for line in diff.splitlines():
        if line.startswith("diff --git "):
            in_header = True
        elif in_header and line.startswith("+++ "):
            target = line[4:]
            path = None if target == "/dev/null" else _unquote(target)[2:]
        elif line.startswith("@@"):
            in_header = False
            number = int(_HUNK.match(line).group(1))
        elif not in_header and line.startswith("+") and path is not None:
            yield AddedLine(path, number, line[1:])
            number += 1


def _unquote(path: str) -> str:
    """Undo Git's C-style quoting of unusual path names."""
    if not path.startswith('"'):
        return path
    raw = path[1:-1].encode("latin-1").decode("unicode_escape")
    return raw.encode("latin-1").decode("utf-8")


def read_text(path: str) -> str:
    with open(path, encoding="utf-8") as handle:
        return handle.read()


_IN_ACTIONS = os.environ.get("GITHUB_ACTIONS") == "true"


def report(level: str, message: str, path: str | None = None, line: int | None = None) -> None:
    """Print a finding; in GitHub Actions it also becomes an annotation."""
    location = path or ""
    if path and line:
        location += f":{line}"
    print(f"{level.upper()}: {location + ': ' if location else ''}{message}")
    if _IN_ACTIONS:
        props = ",".join(
            part for part in (path and f"file={_escape_property(path)}", line and f"line={line}") if part
        )
        text = _escape_data(message)
        print(f"::{level} {props}::{text}" if props else f"::{level}::{text}")


def _escape_data(text: str) -> str:
    """Escape a workflow command's message as GitHub documents."""
    return text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")


def _escape_property(text: str) -> str:
    """Escape a workflow command's property value as GitHub documents."""
    return _escape_data(text).replace(":", "%3A").replace(",", "%2C")


def config_error(message: str) -> NoReturn:
    """Stop with exit code 2: the gate cannot run on this config."""
    print(f"ERROR: {message}", file=sys.stderr)
    if _IN_ACTIONS:
        print(f"::error::{_escape_data(message)}")
    sys.exit(2)


def finish(check: str, failures: int, warnings: int = 0) -> None:
    summary = f"{check}: {failures} failure(s), {warnings} warning(s)"
    print(summary)
    sys.exit(1 if failures else 0)
