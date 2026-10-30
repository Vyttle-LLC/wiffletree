"""Log entry check: every file under <brain folder>/logs/ is either a legacy
`<repo>.md` log or a new-layout entry that follows the contract in gate/README.md.
"""

import datetime
import re
from dataclasses import dataclass

import gate_config
from common import finish, read_text, report, tracked_files

LOGS_DIR = f"{gate_config.BRAIN_DIR}/logs"

REPO_NAME = r"[A-Za-z0-9][A-Za-z0-9._-]*"
ULID = r"[0-7][0-9a-hjkmnp-tv-z]{25}"  # lowercase Crockford base32

LEGACY_LOG = re.compile(rf"{re.escape(LOGS_DIR)}/{REPO_NAME}\.md")
ENTRY_PATH = re.compile(
    rf"{re.escape(LOGS_DIR)}/(?P<repo>{REPO_NAME})/(?P<year>\d{{4}})/(?P<month>\d{{2}})/"
    rf"(?P<date>(?P=year)-(?P=month)-\d{{2}})-(?P<id>{ULID})\.md"
)

FRONTMATTER_KEYS = ("id", "repo", "author", "source")
SOURCES = ("wiffletree", "skill")
GITHUB_LOGIN = re.compile(r"[A-Za-z0-9](?:[A-Za-z0-9]|-(?=[A-Za-z0-9])){0,38}")

FIELDS = ("type", "ref", "changed", "why", "state")
TYPES = ("decision", "observation")
HEADING = re.compile(r"## (\d{4}-\d{2}-\d{2}) — (\S.*)")
FIELD = re.compile(r"- ([a-z]+): (\S.*)")
SHA = r"[0-9a-f]{7,40}"
NO_REF = "—"
# `<branch> @ —` records uncommitted work on a branch.
REF = re.compile(rf"\S+ @ (?:{NO_REF}|{SHA}(?:…{SHA})?(?: · PR #\d+)?)")


def is_legacy_log(path: str) -> bool:
    return LEGACY_LOG.fullmatch(path) is not None


@dataclass(frozen=True)
class EntryPath:
    repo: str
    date: str
    id: str


def parse_entry_path(path: str) -> EntryPath | None:
    match = ENTRY_PATH.fullmatch(path)
    if not match:
        return None
    try:
        datetime.date.fromisoformat(match["date"])
    except ValueError:
        return None
    return EntryPath(match["repo"], match["date"], match["id"])


FRONTMATTER_LINE = re.compile(r"([a-z_]+): (\S.*)")

# A problem's message never quotes the entry's text, which may be sensitive;
# the line number points the author at it instead.
Problem = tuple[str, int | None]  # (message, 1-based line number or None)


def entry_problems(entry: EntryPath, text: str) -> list[Problem]:
    lines = text.split("\n")
    if lines[0] != "---" or "---" not in lines[1:]:
        return [("file must start with a `---` frontmatter block closed by `---`", 1)]
    closing = lines.index("---", 1)
    problems = frontmatter_problems(entry, lines[1:closing])
    body_start = closing + 2
    body = "\n".join(lines[closing + 1 :]).strip()
    if len(body) > gate_config.MAX_ENTRY_BODY_CHARS:
        problems.append(
            (f"entry body is {len(body)} characters; the cap is {gate_config.MAX_ENTRY_BODY_CHARS}", None)
        )
    numbered = [(number, line) for number, line in enumerate(lines[closing + 1 :], start=body_start) if line.strip()]
    return problems + body_problems(entry, numbered)


def frontmatter_problems(entry: EntryPath, lines: list[str]) -> list[Problem]:
    problems: list[Problem] = []
    values: dict[str, str] = {}
    for number, line in enumerate(lines, start=2):
        match = FRONTMATTER_LINE.fullmatch(line)
        if not match:
            problems.append(("frontmatter line is not `key: value` with a lowercase key", number))
        elif match[1] in values:
            problems.append((f"frontmatter key `{match[1]}` appears twice", number))
        elif match[1] not in FRONTMATTER_KEYS:
            problems.append((f"frontmatter key `{match[1]}` is not one of: {', '.join(FRONTMATTER_KEYS)}", number))
        else:
            values[match[1]] = match[2].strip()
    missing = [key for key in FRONTMATTER_KEYS if key not in values]
    if missing:
        problems.append((f"frontmatter is missing: {', '.join(missing)}", None))
    if "id" in values and values["id"] != entry.id:
        problems.append(("frontmatter `id` must match the ULID in the file name", None))
    if "repo" in values and values["repo"] != entry.repo:
        problems.append(("frontmatter `repo` must match the repository folder", None))
    if "author" in values and not GITHUB_LOGIN.fullmatch(values["author"]):
        problems.append(("frontmatter `author` must be a GitHub login", None))
    if "source" in values and values["source"] not in SOURCES:
        problems.append((f"frontmatter `source` must be one of: {', '.join(SOURCES)}", None))
    return problems


def body_problems(entry: EntryPath, lines: list[tuple[int, str]]) -> list[Problem]:
    """Check the body's non-blank lines, given with their file line numbers."""
    if not lines:
        return [("entry has no body", None)]
    problems: list[Problem] = []
    heading_line, heading_text = lines[0]
    heading = HEADING.fullmatch(heading_text)
    if not heading:
        problems.append(("first body line must be `## YYYY-MM-DD — Short title`", heading_line))
    elif heading[1] != entry.date:
        problems.append(("heading date must match the date in the file name", heading_line))

    fields: list[tuple[str, str, int]] = []
    for number, line in lines[1:]:
        field = FIELD.fullmatch(line)
        if field:
            fields.append((field[1], field[2].strip(), number))
        elif not line[0].isspace() or not fields:
            problems.append(("unexpected line; only `- field: value` lines and indented continuations follow the heading", number))
    names = tuple(name for name, _, _ in fields)
    if names != FIELDS:
        problems.append((f"fields must be exactly {', '.join(FIELDS)} in that order", None))
    for name, value, number in fields:
        if name == "type" and value not in TYPES:
            problems.append((f"`type` must be one of: {', '.join(TYPES)}", number))
        if name == "ref" and value != NO_REF and not REF.fullmatch(value):
            problems.append(("`ref` must be `<branch> @ <sha>[…<sha>][ · PR #NN]`, `<branch> @ —` or `—`", number))
    return problems


def main() -> None:
    failures = 0
    for path in tracked_files(f"{LOGS_DIR}/"):
        if is_legacy_log(path):
            continue
        entry = parse_entry_path(path)
        if entry is None:
            failures += 1
            report("error", f"not a legacy log or a `{LOGS_DIR}/<repo>/YYYY/MM/YYYY-MM-DD-<ulid>.md` entry", path)
            continue
        for message, line in entry_problems(entry, read_text(path)):
            failures += 1
            report("error", message, path, line)
    finish("log-schema", failures)


if __name__ == "__main__":
    main()
