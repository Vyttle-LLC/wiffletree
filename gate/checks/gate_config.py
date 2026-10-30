"""The brain's gate settings, read from `wiffletree-brain.json` at the repository root.

The brain folder is the file's `brain_dir`; everything else is its `gate` object.
A missing file, object or key takes the gate's default.
"""

import json
from typing import Any

from common import config_error

CONFIG_FILE = "wiffletree-brain.json"


def _load() -> dict[str, Any]:
    try:
        with open(CONFIG_FILE, encoding="utf-8") as handle:
            data = json.load(handle)
    except FileNotFoundError:
        return {}
    except (json.JSONDecodeError, UnicodeDecodeError):
        config_error(f"{CONFIG_FILE} is not valid JSON")
    if not isinstance(data, dict):
        config_error(f"{CONFIG_FILE} must hold a JSON object")
    return data


def _setting(table: dict[str, Any], name: str, kind: type, default: Any, description: str) -> Any:
    """`table`'s value for the last part of the dotted `name`, or `default`."""
    value = table.get(name.rsplit(".", 1)[-1], default)
    # JSON booleans are Python ints, so a count must refuse them explicitly.
    if not isinstance(value, kind) or (kind is int and isinstance(value, bool)):
        config_error(f"{CONFIG_FILE}: `{name}` must be {description}")
    return value


_file = _load()
_gate: dict[str, Any] = _setting(_file, "gate", dict, {}, "an object")

def _brain_dir(value: str) -> str:
    """The brain folder as a plain relative path: no `./`, `//` or trailing `/`."""
    parts = [part for part in value.split("/") if part not in ("", ".")]
    if value.startswith("/") or not parts or ".." in parts:
        config_error(f"{CONFIG_FILE}: `brain_dir` must be a folder inside the repository")
    return "/".join(parts)


BRAIN_DIR: str = _brain_dir(_setting(_file, "brain_dir", str, "second-brain", "a string"))

# A new-layout log entry's body (everything after its frontmatter) may hold at
# most this many characters.
MAX_ENTRY_BODY_CHARS: int = _setting(_gate, "gate.max_entry_body_chars", int, 2000, "a whole number")

# Transition rule: while true, edits to legacy `<brain>/logs/<repo>.md` files
# warn instead of failing.
LEGACY_LOG_EDITS_ALLOWED: bool = _setting(_gate, "gate.legacy_log_edits_allowed", bool, False, "true or false")

_domains = _setting(_gate, "gate.allowed_email_domains", list, [], "a list of domains")
if not all(isinstance(domain, str) for domain in _domains):
    config_error(f"{CONFIG_FILE}: `gate.allowed_email_domains` must be a list of domains")
# Email addresses at these domains (or their subdomains) are not reported.
ALLOWED_EMAIL_DOMAINS: tuple[str, ...] = tuple(domain.lower() for domain in _domains)

# The sensitive scan's allow-list file, or None for no allow-list.
SENSITIVE_ALLOWLIST: str | None = _gate.get("allowlist")
if SENSITIVE_ALLOWLIST is not None and not isinstance(SENSITIVE_ALLOWLIST, str):
    config_error(f"{CONFIG_FILE}: `gate.allowlist` must be a path")
