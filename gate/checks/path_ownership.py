"""Path ownership: a PR is a log PR, a compile PR or an other-docs PR, never a mix.

- log PR: only adds files under <brain folder>/logs/<repo>/. Entries are
  append-only, so modifying or deleting one fails.
- compile PR: changes only <brain folder>/notes/, STATUS.md and reconcile-queue.md.
- other docs PR: touches nothing under the brain folder.

Edits to legacy <brain folder>/logs/<repo>.md files belong to no class. They
warn while the brain's `gate.legacy_log_edits_allowed` is true and fail otherwise.
"""

from collections import defaultdict

import gate_config
from common import Change, changed_files, finish, report
from log_schema import LOGS_DIR, is_legacy_log

BRAIN = gate_config.BRAIN_DIR
COMPILE_PATHS = (f"{BRAIN}/STATUS.md", f"{BRAIN}/reconcile-queue.md")
COMPILE_PREFIX = f"{BRAIN}/notes/"

LOG, COMPILE, DOCS = "log", "compile", "other docs"


def classify(path: str) -> str | None:
    """The PR class that owns this path, or None if no class may change it."""
    if path.startswith(f"{LOGS_DIR}/"):
        return LOG
    if path.startswith(COMPILE_PREFIX) or path in COMPILE_PATHS:
        return COMPILE
    if path.startswith(f"{BRAIN}/"):
        return None
    return DOCS


def main() -> None:
    failures = warnings = 0
    by_class: dict[str, list[Change]] = defaultdict(list)

    for change in changed_files():
        if is_legacy_log(change.path):
            if gate_config.LEGACY_LOG_EDITS_ALLOWED:
                warnings += 1
                report("warning", "legacy log edited; allowed until the legacy-log migration", change.path)
            else:
                failures += 1
                report("error", "legacy logs are frozen; add a new-layout entry instead", change.path)
            continue

        owner = classify(change.path)
        if owner is None:
            failures += 1
            report("error", f"no PR class may change this path under {BRAIN}/", change.path)
            continue
        if owner == LOG and change.status != "A":
            failures += 1
            verb = "deletes" if change.status == "D" else "modifies"
            report("error", f"log entries are append-only, but this PR {verb} one", change.path)
        by_class[owner].append(change)

    classes = [name for name in (LOG, COMPILE, DOCS) if name in by_class]
    if len(classes) > 1:
        failures += 1
        report(
            "error",
            "this PR mixes " + " and ".join(f"{name} files" for name in classes)
            + "; split it so each PR is only a log, compile or other-docs PR",
        )
        for name in classes:
            print(f"  {name}: " + ", ".join(change.path for change in by_class[name]))
    elif classes:
        print(f"PR class: {classes[0]}")

    finish("path-ownership", failures, warnings)


if __name__ == "__main__":
    main()
