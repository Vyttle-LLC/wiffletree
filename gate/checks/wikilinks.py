"""Wikilink check: every [[link]] in the notes and STATUS.md names an existing note.

A link target is a note's file name without `.md` (`[[Rollout Status]]`)
or its path inside notes/ (`[[topic/Note]]`). `|alias`, `#heading` and
`^block` suffixes are ignored. Links inside code are not links.
"""

import posixpath
import re

from common import finish, read_text, report, tracked_files
from gate_config import BRAIN_DIR

NOTES_DIR = f"{BRAIN_DIR}/notes"
LINKED_FILES = (f"{NOTES_DIR}/*.md", f"{BRAIN_DIR}/STATUS.md")

_FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})")
_CODE_SPAN = re.compile(r"(`+).*?\1")
_WIKILINK = re.compile(r"!?\[\[([^\[\]]*)\]\]")


def link_targets(notes: list[str]) -> set[str]:
    """Every name a link may use to reach one of the notes."""
    targets = set()
    for path in notes:
        relative = posixpath.relpath(path, NOTES_DIR)
        for name in (relative, posixpath.basename(relative)):
            targets.add(name)
            if name.endswith(".md"):
                targets.add(name[: -len(".md")])
    return targets


def links_in(text: str):
    """Yield (line number, column, link target) for each wikilink outside code."""
    open_fence = None  # the opening fence's run of ` or ~ while inside a code block
    for number, line in enumerate(text.splitlines(), start=1):
        fence = _FENCE.match(line)
        if open_fence is None:
            if fence:
                open_fence = fence[1]
                continue
        else:
            # Only a bare run of the same character, at least as long, closes it.
            if fence and fence[1][0] == open_fence[0] and len(fence[1]) >= len(open_fence) and not line[fence.end() :].strip():
                open_fence = None
            continue
        # Blank out code spans without shifting columns.
        prose = _CODE_SPAN.sub(lambda span: " " * len(span.group(0)), line)
        for match in _WIKILINK.finditer(prose):
            yield number, match.start() + 1, re.split(r"[|#^]", match.group(1), maxsplit=1)[0].strip()


def main() -> None:
    targets = link_targets(tracked_files(f"{NOTES_DIR}/"))
    failures = 0
    for path in tracked_files(*LINKED_FILES):
        for number, column, target in links_in(read_text(path)):
            if target not in targets:
                failures += 1
                # The target is not echoed: it is note text and may be sensitive.
                report("error", f"wikilink at column {column} does not match any note in {NOTES_DIR}/", path, number)
    finish("wikilinks", failures)


if __name__ == "__main__":
    main()
