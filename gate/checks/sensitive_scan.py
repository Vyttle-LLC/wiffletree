"""Sensitive-information scan: child PII and credentials in the lines a PR adds.

Findings name the file, line, column and rule, never the matched text, so CI
logs do not copy what they caught. Suppress a false positive with an inline
`<!-- gate:allow <rule>[,<rule>] <reason> -->` marker on the same line, or with
an entry in the allow-list file that `wiffletree-brain.json` names in `gate.allowlist`.

Usage: sensitive_scan.py [--all]   (--all scans every tracked file instead)
"""

import fnmatch
import re
import sys
import tomllib
from dataclasses import dataclass
from typing import Callable, Iterable

import gate_config
from common import (
    GATE_FIXTURES,
    AddedLine,
    added_lines,
    config_error,
    finish,
    read_text,
    report,
    scanning_gate_repo,
    tracked_files,
)


@dataclass(frozen=True)
class Rule:
    id: str
    description: str
    pattern: re.Pattern[str]
    is_match: Callable[[re.Match[str]], bool] = lambda _: True


def _email_outside_allowed_domains(match: re.Match[str]) -> bool:
    domain = match.group(0).rsplit("@", 1)[1].lower()
    return not any(
        domain == allowed or domain.endswith("." + allowed)
        for allowed in gate_config.ALLOWED_EMAIL_DOMAINS
    )


# A CI run or job ID is written right after one of these, as in `run 18234567890`
# or `actions/runs/18234567890`.
_RUN_ID_CONTEXT = re.compile(r"(?:\b(?:runs?|jobs?)/|\b(?:run|job)(?: id)?[ #:]+)$", re.IGNORECASE)


def _plausible_us_phone(match: re.Match[str]) -> bool:
    """Every US-shaped number counts except a separator-free digit run that
    directly follows a run or job context; anything else needs explicit suppression."""
    number = match.group(0)
    return not (number.isdigit() and _RUN_ID_CONTEXT.search(match.string, 0, match.start()))


def _plausible_coordinates(match: re.Match[str]) -> bool:
    return abs(float(match.group(1))) <= 90 and abs(float(match.group(2))) <= 180


STREET_SUFFIXES = (
    "Street|St|Avenue|Ave|Road|Rd|Boulevard|Blvd|Lane|Ln|Drive|Dr|Court|Ct|Way|"
    "Place|Pl|Terrace|Ter|Circle|Cir|Parkway|Pkwy|Highway|Hwy|Trail|Trl"
)

RULES = (
    Rule(
        "phone",
        "phone number (E.164)",
        re.compile(r"(?<![\w+])\+[1-9](?:[ .-]?\d){6,14}(?![\w])"),
    ),
    Rule(
        "phone",
        "phone number (US format)",
        re.compile(r"(?<![\w.+-])(?:1[ .-]?)?(?:\([2-9]\d{2}\)|[2-9]\d{2})[ .-]?[2-9]\d{2}[ .-]?\d{4}(?![\w-])"),
        _plausible_us_phone,
    ),
    Rule(
        "email",
        "email address outside the allowed domains",
        re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}"),
        _email_outside_allowed_domains,
    ),
    Rule(
        "street-address",
        "street address",
        re.compile(rf"\b\d{{1,6}}\s+(?:[A-Z][A-Za-z'.-]*\s+){{1,4}}(?:{STREET_SUFFIXES})\b"),
    ),
    Rule("street-address", "PO box", re.compile(r"\bP\.?\s?O\.?\s+Box\s+\d+", re.IGNORECASE)),
    Rule(
        "gps",
        "GPS coordinates",
        re.compile(r"(?<![\d.])(-?\d{1,2}\.\d{4,})\s*,\s*(-?\d{1,3}\.\d{4,})(?![\d.])"),
        _plausible_coordinates,
    ),
    Rule(
        "gps",
        "labelled latitude or longitude",
        re.compile(r"\b(?:lat|latitude|lng|lon|longitude)\b\s*[:=]\s*-?\d{1,3}\.\d{3,}", re.IGNORECASE),
    ),
    Rule(
        "aws-access-key",
        "AWS access key ID",
        re.compile(r"(?<![A-Z0-9])(?:AKIA|ASIA|ABIA|ACCA|AIDA|AROA|AIPA|ANPA|ANVA|APKA)[A-Z0-9]{16}(?![A-Z0-9])"),
    ),
    Rule(
        "aws-secret-key",
        "AWS secret access key",
        re.compile(r"(?i)aws.{0,20}(?:secret|sk).{0,20}[:=]\s*[\"']?[A-Za-z0-9/+=]{40}(?![A-Za-z0-9/+=])"),
    ),
    Rule("private-key", "private key block", re.compile(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY")),
    Rule(
        "token",
        "service token (GitHub, Slack, Stripe, OpenAI/Anthropic or JWT)",
        re.compile(
            r"\b(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,}"
            r"|xox[abposr]-[A-Za-z0-9-]{10,}|[sr]k_live_[A-Za-z0-9]{16,}"
            r"|sk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}"
            r"|eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,})"
        ),
    ),
    Rule(
        "url-credentials",
        "password embedded in a URL",
        re.compile(r"\b[a-z][a-z0-9+.-]*://[^\s/:@]+:[^\s/@]+@"),
    ),
    Rule(
        "secret-assignment",
        "secret assigned a literal value",
        re.compile(
            r"(?i)\b(?:password|passwd|secret|api[_-]?key|access[_-]?token|auth[_-]?token|client[_-]?secret)\b"
            r"\s*[:=]\s*[\"']?[A-Za-z0-9/+=_.-]{12,}"
        ),
    ),
)

RULE_IDS = {rule.id for rule in RULES}

_MARKER = re.compile(r"<!--\s*gate:allow\s+([a-z0-9-]+(?:\s*,\s*[a-z0-9-]+)*)\s+\S.*?-->")


def inline_allowed_rules(text: str) -> set[str]:
    allowed: set[str] = set()
    for marker in _MARKER.finditer(text):
        allowed.update(rule.strip() for rule in marker.group(1).split(","))
    return allowed


@dataclass(frozen=True)
class Allowlist:
    path_globs: tuple[str, ...]
    values: frozenset[tuple[str, str]]  # (rule id, lowercased matched text)

    def allows_path(self, path: str) -> bool:
        return any(fnmatch.fnmatchcase(path, glob) for glob in self.path_globs)

    def allows_value(self, rule: str, value: str) -> bool:
        return (rule, value.lower()) in self.values


def default_path_globs() -> tuple[str, ...]:
    """The gate's fixtures are exempt in the gate's own repository; in a brain, nothing is."""
    return (GATE_FIXTURES + "*",) if scanning_gate_repo() else ()


def load_allowlist(path: str | None) -> Allowlist:
    if path is None:
        return Allowlist(path_globs=default_path_globs(), values=frozenset())
    try:
        with open(path, "rb") as handle:
            data = tomllib.load(handle)
    except FileNotFoundError:
        config_error(f"{path}: the allow-list named in wiffletree-brain.json does not exist")
    except tomllib.TOMLDecodeError:
        # The parser's message can quote the file's text, so it is not repeated.
        config_error(f"{path}: the allow-list is not valid TOML")
    # Errors name the entry by position, never by its `match`, which may be sensitive.
    for kind, required in (("path", ("glob",)), ("value", ("rule", "match"))):
        for number, entry in enumerate(data.get(kind, []), start=1):
            if not str(entry.get("reason", "")).strip():
                config_error(f"{path}: [[{kind}]] entry #{number} needs a reason")
            if not all(isinstance(entry.get(key), str) for key in required):
                config_error(f"{path}: [[{kind}]] entry #{number} needs {' and '.join(required)}")
    for number, entry in enumerate(data.get("value", []), start=1):
        if entry["rule"] not in RULE_IDS:
            config_error(f"{path}: [[value]] entry #{number} has an unknown rule; known rules: {sorted(RULE_IDS)}")
    return Allowlist(
        path_globs=default_path_globs() + tuple(entry["glob"] for entry in data.get("path", [])),
        values=frozenset((entry["rule"], entry["match"].lower()) for entry in data.get("value", [])),
    )


def scan(lines: Iterable[AddedLine], allowlist: Allowlist) -> int:
    findings = 0
    for line in lines:
        if allowlist.allows_path(line.path):
            continue
        suppressed = inline_allowed_rules(line.text)
        reported: list[tuple[str, int, int]] = []
        for rule in RULES:
            if rule.id in suppressed:
                continue
            for match in rule.pattern.finditer(line.text):
                if not rule.is_match(match) or allowlist.allows_value(rule.id, match.group(0)):
                    continue
                # Two patterns of one rule can catch the same value; report it once.
                if any(
                    rule_id == rule.id and start < match.end() and match.start() < end
                    for rule_id, start, end in reported
                ):
                    continue
                reported.append((rule.id, match.start(), match.end()))
                findings += 1
                report(
                    "error",
                    f"possible {rule.description} at column {match.start() + 1} [rule: {rule.id}]",
                    line.path,
                    line.number,
                )
    return findings


def every_tracked_line() -> Iterable[AddedLine]:
    for path in tracked_files():
        try:
            text = read_text(path)
        except (UnicodeDecodeError, IsADirectoryError, FileNotFoundError):
            continue
        for number, content in enumerate(text.splitlines(), start=1):
            yield AddedLine(path, number, content)


def main() -> None:
    lines = every_tracked_line() if "--all" in sys.argv[1:] else added_lines()
    findings = scan(lines, load_allowlist(gate_config.SENSITIVE_ALLOWLIST))
    if findings:
        allowlist = gate_config.SENSITIVE_ALLOWLIST or "the allow-list that `gate.allowlist` names"
        print(
            "Remove the value, or if it is a false positive add "
            "`<!-- gate:allow <rule> <reason> -->` to the line or an entry to "
            f"{allowlist}. See the brain gate's README, gate/README.md in Vyttle-LLC/wiffletree."
        )
    finish("sensitive-info", findings)


if __name__ == "__main__":
    main()
