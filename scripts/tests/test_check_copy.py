"""Tests for scripts/check-copy.py, on hand-written extractor records.

  uv run python -m unittest discover -s scripts/tests

`--extracted` and `parse_records` take the extractor's JSON lines, so no
test here runs node.
"""

import importlib.util
import io
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from types import ModuleType

SCRIPT = Path(__file__).resolve().parent.parent / "check-copy.py"


def _load() -> ModuleType:
    spec = importlib.util.spec_from_file_location("check_copy", SCRIPT)
    if spec is None or spec.loader is None:
        msg = f"cannot load {SCRIPT}"
        raise ImportError(msg)
    module = importlib.util.module_from_spec(spec)
    sys.modules["check_copy"] = module
    spec.loader.exec_module(module)
    return module


copy = _load()


def record(**fields: object) -> str:
    """Build one extractor JSON line, by default JSX text in app/x.tsx."""
    data: dict[str, object] = {
        "file": "app/x.tsx",
        "line": 1,
        "kind": "jsx",
        "text": "",
    }
    data.update(fields)
    return json.dumps(data)


def findings(*lines: str) -> list[str]:
    """Return the messages check() finds in these records."""
    return [finding.message for finding in copy.check(copy.parse_records(lines), None)]


class BannedPhrases(unittest.TestCase):
    """Internal vocabulary, "--" and marketing words in user-facing copy."""

    def test_flags_internal_vocabulary_in_jsx_text(self) -> None:
        """Check that it flags internal vocabulary in jsx text."""
        messages = findings(record(text="Everything this app has ever ingested."))
        self.assertTrue(any('"this app"' in message for message in messages))
        self.assertTrue(any('"ingest"' in message for message in messages))

    def test_flags_a_literal_double_hyphen_but_not_a_css_variable(self) -> None:
        """Check that it flags a literal double hyphen but not a css variable."""
        self.assertTrue(
            findings(
                record(text="Reconnect to keep chatting -- you'll be asked again.")
            )
        )
        self.assertEqual(
            findings(
                record(
                    kind="string",
                    text="var(--mantine-color-anchor)",
                    attr="style",
                    prop="color",
                )
            ),
            [],
        )

    def test_reads_attributes_people_read_and_skips_the_rest(self) -> None:
        """Check that it reads attributes people read and skips the rest."""
        self.assertTrue(
            findings(
                record(
                    kind="attr", attr="description", text="The aggregator runs hourly."
                )
            )
        )
        self.assertEqual(
            findings(record(kind="attr", attr="href", text="/api/tocs")), []
        )

    def test_skips_log_messages_and_keys(self) -> None:
        """Check that it skips log messages and keys."""
        # A literal passed to a call (log.warn) has no copy context.
        self.assertEqual(
            findings(
                record(
                    kind="string",
                    text="Could not resolve the TOC list from the aggregator",
                )
            ),
            [],
        )

    def test_reads_constants_named_like_copy(self) -> None:
        """Check that it reads constants named like copy."""
        self.assertTrue(
            findings(
                record(
                    kind="string",
                    decl="NO_STATUS_BODY",
                    text="once the aggregator has run",
                )
            )
        )
        self.assertEqual(
            findings(record(kind="string", decl="STORAGE_KEY", text="localStorage")), []
        )

    def test_flags_marketing_words(self) -> None:
        """Check that it flags marketing words."""
        self.assertTrue(findings(record(text="A seamless way to track trains.")))

    def test_toc_is_case_sensitive(self) -> None:
        """Check that it toc is case sensitive."""
        self.assertTrue(findings(record(text="National Rail TOCs")))
        self.assertEqual(findings(record(text="Click the stop button.")), [])


class TitleCaseHeadings(unittest.TestCase):
    """h1 and h2 in sentence case, official names allowed."""

    def test_flags_a_title_case_h1(self) -> None:
        """Check that it flags a title case h1."""
        messages = findings(
            record(kind="heading", element="Title", order=1, text="Network Status")
        )
        self.assertEqual(len(messages), 1)
        self.assertIn("Status", messages[0])

    def test_allows_proper_nouns_status_names_and_acronyms(self) -> None:
        """Check that it allows proper nouns status names and acronyms."""
        for text in (
            "Connect Claude to Distant Signal",
            "Live UK rail status",
            "Lines with a Good Service",
            "{} history",
        ):
            with self.subTest(text=text):
                self.assertEqual(
                    findings(
                        record(kind="heading", element="Title", order=1, text=text)
                    ),
                    [],
                )

    def test_checks_section_titles_but_not_h3s(self) -> None:
        """Check that it checks section titles but not h3s."""
        self.assertTrue(
            findings(
                record(
                    kind="heading", element="SectionTitle", text="Your Tracked Trains"
                )
            )
        )
        self.assertEqual(
            findings(
                record(
                    kind="heading",
                    element="SectionTitle",
                    order=3,
                    text="Your Tracked Trains",
                )
            ),
            [],
        )


class MetaDescriptions(unittest.TestCase):
    """Page descriptions fit a search result."""

    def test_flags_a_description_over_160_characters(self) -> None:
        """Check that it flags a description over 160 characters."""
        long = "x" * 161
        self.assertTrue(
            findings(record(kind="string", decl="METADATA_DESCRIPTION", text=long))
        )
        self.assertEqual(
            findings(
                record(kind="string", decl="METADATA_DESCRIPTION", text="x" * 160)
            ),
            [],
        )


class PagesWithoutMetadata(unittest.TestCase):
    """Every page names itself."""

    def test_flags_a_page_with_no_metadata_export(self) -> None:
        """Check that it flags a page with no metadata export."""
        with tempfile.TemporaryDirectory() as tmp:
            frontend = Path(tmp)
            (frontend / "app" / "a").mkdir(parents=True)
            (frontend / "app" / "b").mkdir(parents=True)
            (frontend / "app" / "a" / "page.tsx").write_text(
                "export const metadata = {};\n", encoding="utf-8"
            )
            (frontend / "app" / "b" / "page.tsx").write_text(
                "export default function B() {}\n", encoding="utf-8"
            )
            found = list(copy.pages_without_metadata(frontend))
        self.assertEqual([finding.file for finding in found], ["app/b/page.tsx"])


class Main(unittest.TestCase):
    """The CLI's output and exit status."""

    def test_prints_github_annotations_and_fails(self) -> None:
        """Check that it prints github annotations and fails."""
        with tempfile.NamedTemporaryFile(
            "w", suffix=".jsonl", delete=False, encoding="utf-8"
        ) as handle:
            handle.write(record(line=7, text="Something from this app") + "\n")
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            status = copy.main(["--extracted", handle.name])
        Path(handle.name).unlink()
        self.assertEqual(status, 1)
        self.assertIn("::error file=frontend/app/x.tsx,line=7::", out.getvalue())

    def test_passes_clean_copy(self) -> None:
        """Check that it passes clean copy."""
        with tempfile.NamedTemporaryFile(
            "w", suffix=".jsonl", delete=False, encoding="utf-8"
        ) as handle:
            handle.write(record(text="Couldn't load this train. Try again.") + "\n")
        with redirect_stdout(io.StringIO()):
            status = copy.main(["--extracted", handle.name])
        Path(handle.name).unlink()
        self.assertEqual(status, 0)


if __name__ == "__main__":
    unittest.main()
