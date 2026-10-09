#!/usr/bin/env python3
# ruff: noqa: T201  # a CLI whose output (the findings) is stdout
"""Fail on user-visible copy that breaks docs/style-guide.md's Writing rules.

  uv run scripts/check-copy.py [--extracted FILE]

Run from the repo root after `npm ci` in frontend/ (CI's frontend job).
The strings come from frontend/scripts/extract-copy.mjs, which walks the
app's TypeScript with the TypeScript compiler (the method the 2026-10-09
copy audit used) and prints one JSON record per string literal, JSX text
or heading. `--extracted` reads saved records instead (the unit tests).

Fails (exit 1) on:
  - a banned phrase in a user-facing string: internal vocabulary (ingest,
    aggregator, full coverage, TOC, deployment, allowlist, localStorage,
    "this app"), "--", and marketing words;
  - a Title Case h1 or h2 (a `Title` of order 1 or 2, or a `SectionTitle`
    of order 2), outside the proper nouns and official status names below;
  - a meta description over 160 characters;
  - an app/**/page.tsx with neither `metadata` nor `generateMetadata`.

A string is user-facing when it is JSX text, a JSX attribute people read
(label, title, description, placeholder, aria-label, ...), a literal that
is a JSX child or the value of such an attribute, or the value of a
property or constant named like copy (label, message, description,
METADATA_*, *_LABEL, *_MESSAGE, ...). Log messages, URLs and keys are not.

Findings are GitHub Actions `::error file=...::` lines on stdout. Exit
status: 0 clean, 1 findings, 2 the extractor failed. Stdlib only.
"""

import argparse
import json
import re
import subprocess
import sys
from collections.abc import Iterable, Iterator, Sequence
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FRONTEND = ROOT / "frontend"
EXTRACTOR = FRONTEND / "scripts" / "extract-copy.mjs"

META_DESCRIPTION_MAX = 160

# JSX attributes whose value a person reads or hears.
USER_ATTRS = frozenset(
    {
        "alt",
        "aria-description",
        "aria-label",
        "ariaLabel",
        "description",
        "emptyMessage",
        "endMessage",
        "error",
        "label",
        "nothingFoundMessage",
        "placeholder",
        "title",
    }
)
# Object keys whose value is copy.
USER_PROPS = frozenset(
    {
        "caption",
        "description",
        "error",
        "label",
        "message",
        "note",
        "placeholder",
        "text",
        "title",
        "tooltip",
    }
)
# Constants named like copy: METADATA_DESCRIPTION, ENABLE_ERROR_MESSAGE, ...
USER_DECL = re.compile(
    r"(^|_)(COPY|DESCRIPTION|HINT|LABELS?|MESSAGE|NOTE|SUBLINE|SUMMARY|TITLE|BODY)$"
)

# Files whose strings are not UI copy: the chat model's system prompt, and
# the MCP OAuth plumbing's developer errors.
SKIP_FILES = frozenset(
    {"lib/chatTurn.ts", "lib/mcpOAuthProvider.ts", "lib/mcpInstallLinks.ts"}
)

MARKETING = (
    "seamless",
    "seamlessly",
    "effortless",
    "effortlessly",
    "powerful",
    "revolutionary",
    "supercharge",
    "game-changer",
    "game changer",
    "cutting-edge",
    "best-in-class",
    "world-class",
    "delightful",
    "magical",
    "blazing",
)
BANNED: Sequence[tuple[re.Pattern[str], str]] = (
    (
        re.compile(r"(?<![\w(-])--(?![\w-])"),
        'a literal "--" (write an em dash, or two sentences)',
    ),
    (
        re.compile(r"\bthis app\b", re.IGNORECASE),
        '"this app" (say "Distant Signal" or "we")',
    ),
    (
        re.compile(r"\bingest(ed|ion|s)?\b", re.IGNORECASE),
        '"ingest" (internal vocabulary)',
    ),
    (
        re.compile(r"\baggregator\b", re.IGNORECASE),
        '"aggregator" (internal vocabulary)',
    ),
    (
        re.compile(r"\bfull[- ]coverage\b", re.IGNORECASE),
        '"full coverage" (internal vocabulary)',
    ),
    (re.compile(r"\bTOCs?\b"), '"TOC" (say "operator")'),
    (re.compile(r"\bdeployment\b", re.IGNORECASE), '"deployment" (say "this site")'),
    (
        re.compile(r"\ballow-?list\b", re.IGNORECASE),
        '"allowlist" (internal vocabulary)',
    ),
    (re.compile(r"\blocalStorage\b"), '"localStorage" (say "saved in this browser")'),
    (
        re.compile(
            r"\b(" + "|".join(re.escape(word) for word in MARKETING) + r")\b",
            re.IGNORECASE,
        ),
        "a marketing word",
    ),
)

# Capitalised words an h1/h2 may carry after its first word: proper nouns,
# official status names (lib/severity.ts) and acronyms are handled apart.
PROPER_WORDS = frozenset(
    {
        "Anthropic",
        "Claude",
        "Closure",
        "Cross",
        "Delay",
        "Delays",
        "Desktop",
        "Distant",
        "Good",
        "Kings",
        "London",
        "Minor",
        "National",
        "Network",
        "Part",
        "Rail",
        "Repay",
        "Service",
        "Severe",
        "Signal",
        "Suspended",
        "Waterloo",
    }
)


@dataclass(frozen=True)
class Record:
    """One extracted string (see extract-copy.mjs for the fields)."""

    file: str
    line: int
    kind: str
    text: str
    attr: str | None = None
    prop: str | None = None
    decl: str | None = None
    element: str | None = None
    order: int | None = None


@dataclass(frozen=True)
class Finding:
    """A rule broken at a place in frontend/."""

    file: str
    line: int
    message: str

    def annotation(self) -> str:
        """Return the GitHub Actions error line for this finding."""
        return f"::error file=frontend/{self.file},line={self.line}::{self.message}"


def parse_records(lines: Iterable[str]) -> list[Record]:
    """Parse the extractor's JSON lines into records."""
    records = []
    for raw in lines:
        if not raw.strip():
            continue
        data = json.loads(raw)
        records.append(
            Record(
                file=str(data["file"]),
                line=int(data["line"]),
                kind=str(data["kind"]),
                text=str(data["text"]),
                attr=data.get("attr"),
                prop=data.get("prop"),
                decl=data.get("decl"),
                element=data.get("element"),
                order=data.get("order"),
            )
        )
    return records


def is_user_facing(record: Record) -> bool:
    """Say whether a person reads this string (see the module docstring)."""
    if record.file in SKIP_FILES or record.kind == "heading":
        # A heading's parts are checked as their own jsx/string records.
        return False
    if record.kind == "jsx":
        return True
    if record.attr is not None:
        facing = record.attr in USER_ATTRS
    elif record.prop is not None:
        facing = record.prop in USER_PROPS
    elif record.decl is not None:
        facing = USER_DECL.search(record.decl) is not None
    else:
        facing = record.element is not None
    return facing


def banned_phrases(records: Iterable[Record]) -> Iterator[Finding]:
    """Yield a finding per banned phrase in a user-facing string."""
    for record in records:
        if not is_user_facing(record):
            continue
        # CSS custom properties are not copy.
        text = re.sub(r"var\(--[\w-]+\)", "", record.text)
        for pattern, why in BANNED:
            if pattern.search(text):
                yield Finding(
                    record.file,
                    record.line,
                    f"banned phrase: {why}: {record.text[:80]!r}",
                )


def is_top_heading(record: Record) -> bool:
    """Say whether this is an h1 or h2 (`Title` is order 1, `SectionTitle` 2)."""
    if record.kind != "heading":
        return False
    if record.element == "Title":
        return (record.order or 1) <= 2  # noqa: PLR2004  # h2
    if record.element == "SectionTitle":
        return (record.order or 2) <= 2  # noqa: PLR2004  # h2
    return False


def title_case_words(text: str) -> list[str]:
    """Return the capitalised words after the first that sentence case lowers."""
    words = re.findall(r"[A-Za-z][A-Za-z'\u2019-]*", text.replace("{}", " "))
    return [
        word
        for word in words[1:]
        if word[0].isupper()
        and not word.isupper()
        and word not in PROPER_WORDS
        and not re.search(r"[A-Z]", word[1:])
    ]


def title_case_headings(records: Iterable[Record]) -> Iterator[Finding]:
    """Yield a finding per h1/h2 in Title Case."""
    for record in records:
        if not is_top_heading(record):
            continue
        words = title_case_words(record.text)
        if words:
            yield Finding(
                record.file,
                record.line,
                f"heading not in sentence case ({', '.join(words)}): {record.text!r}",
            )


def long_meta_descriptions(records: Iterable[Record]) -> Iterator[Finding]:
    """Yield a finding per page description over META_DESCRIPTION_MAX."""
    for record in records:
        if not record.file.startswith("app/") and record.file != "lib/pageMetadata.ts":
            continue
        is_description = record.decl in {
            "METADATA_DESCRIPTION",
            "SITE_DESCRIPTION",
        } or (
            record.prop == "description"
            and record.element is None
            and record.attr is None
        )
        length = len(record.text.replace("{}", ""))
        if is_description and length > META_DESCRIPTION_MAX:
            yield Finding(
                record.file,
                record.line,
                f"meta description is {length} characters"
                f" (max {META_DESCRIPTION_MAX}): {record.text[:60]!r}",
            )


METADATA_EXPORT = re.compile(
    r"^export (const metadata\b|(async )?function generateMetadata\b)", re.MULTILINE
)


def pages_without_metadata(frontend: Path) -> Iterator[Finding]:
    """Yield a finding per app/**/page.tsx exporting no metadata."""
    for page in sorted((frontend / "app").rglob("page.tsx")):
        if not METADATA_EXPORT.search(page.read_text(encoding="utf-8")):
            rel = page.relative_to(frontend).as_posix()
            yield Finding(
                rel,
                1,
                "page has no `metadata` or `generateMetadata` export"
                " (title and description)",
            )


def extract(frontend: Path) -> list[str]:
    """Run the extractor and return its JSON lines."""
    result = subprocess.run(  # noqa: S603  # fixed argv, no shell
        ["node", str(EXTRACTOR.relative_to(frontend))],  # noqa: S607  # node from PATH, as CI's setup-node puts it
        cwd=frontend,
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.splitlines()


def check(records: Sequence[Record], frontend: Path | None) -> list[Finding]:
    """Return every finding, in a stable order."""
    findings = [
        *banned_phrases(records),
        *title_case_headings(records),
        *long_meta_descriptions(records),
        *(pages_without_metadata(frontend) if frontend is not None else ()),
    ]
    return sorted(
        set(findings), key=lambda finding: (finding.file, finding.line, finding.message)
    )


def main(argv: Sequence[str] | None = None) -> int:
    """Check the copy; print findings; return the exit status."""
    parser = argparse.ArgumentParser(
        description=__doc__.splitlines()[0] if __doc__ else None
    )
    parser.add_argument(
        "--extracted",
        type=Path,
        help="read saved extractor output instead of running it",
    )
    parser.add_argument(
        "--frontend",
        type=Path,
        default=FRONTEND,
        help="the frontend directory (default: %(default)s)",
    )
    args = parser.parse_args(argv)

    if args.extracted is not None:
        lines = args.extracted.read_text(encoding="utf-8").splitlines()
        frontend = None
    else:
        try:
            lines = extract(args.frontend)
        except (OSError, subprocess.CalledProcessError) as error:
            stderr = getattr(error, "stderr", "") or ""
            print(
                "::error::extract-copy.mjs failed (run `npm ci` in frontend/"
                f" first): {error} {stderr}"
            )
            return 2
        frontend = args.frontend

    findings = check(parse_records(lines), frontend)
    for finding in findings:
        print(finding.annotation())
    if findings:
        print(
            f"{len(findings)} copy finding(s); see docs/style-guide.md, Writing.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
