"""Tests for scripts/diff-api-surface.py.

  uv run python -m unittest discover -s scripts/tests

fixtures/api-surface/before is a tiny api; fixtures/api-surface/moved is the
same code after a 1A-style move: the functions, their constant and their
test module in crates/ds-store (reformatted, the include_str! path
adjusted), and a `pub use` shim in the api. The move must show no diff;
edits to SQL text or metric names must.
"""

import importlib.util
import io
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from types import ModuleType
from typing import override

SCRIPT = Path(__file__).resolve().parent.parent / "diff-api-surface.py"
FIXTURES = Path(__file__).resolve().parent / "fixtures" / "api-surface"
BEFORE = FIXTURES / "before"
MOVED = FIXTURES / "moved"
# A fixed identity and no user/system config, so the host's git setup
# (signing, hooks, default branch) cannot leak into the throwaway repos.
GIT_ENV = {
    **os.environ,
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_AUTHOR_NAME": "test",
    "GIT_AUTHOR_EMAIL": "test@example.invalid",
    "GIT_COMMITTER_NAME": "test",
    "GIT_COMMITTER_EMAIL": "test@example.invalid",
}


def _load() -> ModuleType:
    spec = importlib.util.spec_from_file_location("diff_api_surface", SCRIPT)
    if spec is None or spec.loader is None:
        msg = f"cannot load {SCRIPT}"
        raise ImportError(msg)
    module = importlib.util.module_from_spec(spec)
    sys.modules["diff_api_surface"] = module
    spec.loader.exec_module(module)
    return module


surface = _load()


def strings(source: str) -> list[str]:
    """Return the string literals the lexer finds in a Rust snippet."""
    return [t.text for t in surface.Lexer(source).run() if t.kind == "str"]


def run_main(*args: str) -> tuple[int, str]:
    """Run main in-process; return (status, stdout)."""
    out = io.StringIO()
    with redirect_stdout(out):
        status = surface.main(list(args))
    return status, out.getvalue()


class LexerTest(unittest.TestCase):
    """String literals, and the things that look like them but are not."""

    def test_raw_strings_keep_quotes_and_newlines(self) -> None:
        """`r#"…"#` ends only at a quote followed by the same hashes."""
        self.assertEqual(strings('r#"a "b" c"#; r"x"'), ['a "b" c', "x"])
        self.assertEqual(strings('r##"one "# two"##'), ['one "# two'])

    def test_escapes_and_line_continuations_are_decoded(self) -> None:
        """Escapes decode; a backslash-newline continuation drops the indent."""
        source = '"a\\nb \\"q\\" \\x41\\u{e9}"; "SELECT 1 \\\n        FROM t"'
        self.assertEqual(strings(source), ['a\nb "q" Aé', "SELECT 1 FROM t"])

    def test_comments_hold_no_literals(self) -> None:
        """Line, doc and nested block comments are skipped."""
        source = '// "a"\n/// "b"\n/* "c" /* "d" */ "e" */ "f"'
        self.assertEqual(strings(source), ["f"])

    def test_char_literals_and_lifetimes_do_not_open_strings(self) -> None:
        """Char literals (a quote, an escape) and lifetimes open no string."""
        source = "let q = '\"'; let e = '\\''; fn f<'a>(x: &'a str) { \"SELECT 1\" }"
        self.assertEqual(strings(source), ["SELECT 1"])

    def test_byte_and_c_strings_and_raw_identifiers(self) -> None:
        """`b"…"`, `br"…"`, `c"…"` are strings; `r#type` is an identifier."""
        tokens = surface.Lexer('b"x" br#"y"# c"z" r#type b\'q\'').run()
        self.assertEqual([t.text for t in tokens if t.kind == "str"], ["x", "y", "z"])
        self.assertIn("type", [t.text for t in tokens if t.kind == "ident"])


class SurfaceTest(unittest.TestCase):
    """What the fixture api exposes."""

    def test_metrics_resolve_constants_and_skip_test_code(self) -> None:
        """metric_name(CONST) resolves and is prefixed; test-only metrics are out."""
        found = surface.surface(surface.Directory(BEFORE))
        self.assertEqual(
            found.metrics, {"distant_signal_api_fetch_total", "api_reader_up"}
        )

    def test_sql_is_normalised_and_split_from_test_sql(self) -> None:
        """Whitespace collapses; tests/ and #[cfg(test)] literals are test-sql."""
        found = surface.surface(surface.Directory(BEFORE))
        self.assertEqual(
            sorted(found.sql),
            [
                'INSERT INTO ingest_log (source, "at") VALUES ($1, NOW())',
                "SELECT id FROM stations WHERE crs = $1",
                "SELECT max(at) FROM ingest_log",
            ],
        )
        self.assertEqual(
            sorted(found.test_sql),
            [
                "CREATE TABLE stations (id bigserial PRIMARY KEY, crs text NOT NULL);",
                "INSERT INTO stations (crs) VALUES ('KGX')",
                "SELECT count(*) FROM ingest_log",
            ],
        )

    def test_cfg_test_marks_only_the_item_it_annotates(self) -> None:
        """Code after a #[cfg(test)] item is production code again."""
        tokens = surface.tokenize(
            "crates/api/src/x.rs",
            '#[cfg(test)]\nmod t { fn a() { "SELECT 1"; } }\nfn b() { "SELECT 2"; }',
        )
        flags = {t.text: t.test for t in tokens if t.kind == "str"}
        self.assertEqual(flags, {"SELECT 1": True, "SELECT 2": False})

    def test_the_test_support_feature_cfgs_are_test_code(self) -> None:
        """ds-store's `test-support` fixtures are test code; other cfgs are not."""
        tokens = surface.tokenize(
            "crates/ds-store/src/x.rs",
            '#[cfg(any(test, feature = "test-support"))]\nfn a() { "SELECT 1"; }\n'
            '#[cfg(feature = "test-support")]\nfn b() { "SELECT 2"; }\n'
            '#[cfg(feature = "postgres")]\nfn c() { "SELECT 3"; }\n'
            '#[cfg(any(test, feature = "other"))]\nfn d() { "SELECT 4"; }',
        )
        flags = {
            t.text: t.test for t in tokens if t.kind == "str" and "SELECT" in t.text
        }
        self.assertEqual(
            flags,
            {"SELECT 1": True, "SELECT 2": True, "SELECT 3": False, "SELECT 4": False},
        )

    def test_cfg_test_file_modules_are_test_code(self) -> None:
        """`#[cfg(test)] mod x;` makes x.rs (and its children) test code."""
        files = {
            "crates/api/src/lib.rs": "#[cfg(test)]\nmod support;\npub mod routes;",
            "crates/api/src/support.rs": 'fn s() { "SELECT 1"; }',
            "crates/api/src/routes.rs": '#[cfg(test)]\nmod db;\nfn r() { "SELECT 2"; }',
            "crates/api/src/routes/db.rs": 'mod deep;\nfn d() { "SELECT 3"; }',
            "crates/api/src/routes/db/deep.rs": 'fn e() { "SELECT 4"; }',
        }
        with tempfile.TemporaryDirectory() as tmp:
            for path, text in files.items():
                (Path(tmp) / path).parent.mkdir(parents=True, exist_ok=True)
                (Path(tmp) / path).write_text(text, encoding="utf-8")
            found = surface.surface(surface.Directory(Path(tmp)))
        self.assertEqual(list(found.sql), ["SELECT 2"])
        self.assertEqual(sorted(found.test_sql), ["SELECT 1", "SELECT 3", "SELECT 4"])


class DirectoryDiffTest(unittest.TestCase):
    """`diff` between directory trees."""

    @override
    def setUp(self) -> None:
        """Copy the moved fixture to a scratch tree the test may edit."""
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.head = Path(tmp.name) / "head"
        shutil.copytree(MOVED, self.head)

    def edit(self, path: str, old: str, new: str) -> None:
        """Replace text in a file of the scratch tree."""
        target = self.head / path
        text = target.read_text(encoding="utf-8")
        self.assertIn(old, text)
        target.write_text(text.replace(old, new), encoding="utf-8")

    def diff(self, *extra: str) -> tuple[int, str]:
        """Diff the before fixture against the scratch tree."""
        return run_main("diff", f"dir:{BEFORE}", f"dir:{self.head}", *extra)

    def test_a_pure_move_shows_no_diff(self) -> None:
        """Moving code into ds-store with a shim left behind changes nothing."""
        status, out = self.diff()
        self.assertEqual(status, 0, out)
        self.assertIn("surface unchanged (metrics 2, sql 3, test-sql 3)", out)

    def test_a_changed_query_is_reported(self) -> None:
        """A changed SQL literal fails with a diff of the sql section."""
        self.edit("crates/ds-store/src/freshness.rs", "max(at)", "min(at)")
        status, out = self.diff()
        self.assertEqual(status, 1)
        self.assertIn("-1\tSELECT max(at) FROM ingest_log", out)
        self.assertIn("+1\tSELECT min(at) FROM ingest_log", out)

    def test_a_duplicated_query_is_reported(self) -> None:
        """A query that now occurs twice changes its count."""
        self.edit(
            "crates/api/src/data/queries.rs",
            "pub fn public_reader",
            'pub fn twice() { let _ = "SELECT max(at) FROM ingest_log"; }\n'
            "pub fn public_reader",
        )
        status, out = self.diff()
        self.assertEqual(status, 1)
        self.assertIn("+2\tSELECT max(at) FROM ingest_log", out)

    def test_a_renamed_metric_is_reported(self) -> None:
        """A changed metric constant fails with a diff of the metrics section."""
        self.edit("crates/ds-store/src/freshness.rs", "api_fetch_total", "api_fetches")
        status, out = self.diff()
        self.assertEqual(status, 1)
        self.assertIn("-distant_signal_api_fetch_total", out)
        self.assertIn("+distant_signal_api_fetches", out)

    def test_test_sql_changes_can_be_ignored(self) -> None:
        """--ignore-test-sql skips only the test-sql section."""
        self.edit("crates/api/tests/ingest.rs", "count(*)", "count(1)")
        self.assertEqual(self.diff()[0], 1)
        status, out = self.diff("--ignore-test-sql")
        self.assertEqual(status, 0, out)


class GitRevisionTest(unittest.TestCase):
    """`diff` with git revisions, run as CI and the movers do."""

    @override
    def setUp(self) -> None:
        """Create a repo whose first commit is the before fixture."""
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.repo = Path(tmp.name)
        shutil.copytree(BEFORE / "crates", self.repo / "crates")
        self.git("init", "-q")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "base")

    def git(self, *args: str) -> None:
        """Run git in the repo."""
        subprocess.run(  # noqa: S603  # fixed git subcommands
            ["git", *args],  # noqa: S607  # git from PATH
            check=True,
            capture_output=True,
            cwd=self.repo,
            env=GIT_ENV,
        )

    def script(self, *args: str) -> tuple[int, str]:
        """Run the script from the repo root; return (status, stdout)."""
        result = subprocess.run(  # noqa: S603  # this repo's script
            [sys.executable, str(SCRIPT), *args],
            check=False,
            capture_output=True,
            text=True,
            cwd=self.repo,
            env=GIT_ENV,
        )
        return result.returncode, result.stdout + result.stderr

    def move(self) -> None:
        """Replace the work tree's crates with the moved fixture."""
        shutil.rmtree(self.repo / "crates")
        shutil.copytree(MOVED / "crates", self.repo / "crates")

    def test_the_uncommitted_move_matches_the_base_commit(self) -> None:
        """HEAD defaults to the work tree, untracked ds-store files included."""
        self.move()
        status, out = self.script("diff", "HEAD")
        self.assertEqual(status, 0, out)

    def test_two_commits_compare_without_a_checkout(self) -> None:
        """BASE and HEAD as revisions read through git."""
        self.move()
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "move")
        status, out = self.script("diff", "HEAD~1", "HEAD")
        self.assertEqual(status, 0, out)
        self.assertIn("surface unchanged", out)

    def test_an_unknown_revision_exits_2(self) -> None:
        """A git failure is not a pass."""
        status, _ = self.script("diff", "no-such-rev")
        self.assertEqual(status, 2)

    def test_dump_lists_every_section(self) -> None:
        """`dump` prints the three headed sections."""
        status, out = self.script("dump", "HEAD")
        self.assertEqual(status, 0, out)
        self.assertIn("## metrics (2)\napi_reader_up\n", out)
        self.assertIn("## sql (3)\n", out)
        self.assertIn("## test-sql (3)\n", out)


if __name__ == "__main__":
    unittest.main()
