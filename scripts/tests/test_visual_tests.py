"""Tests for scripts/visual-tests.py and scripts/paths-changed.py.

uv run python -m unittest discover -s scripts/tests
"""

import importlib.util
import io
import json
import subprocess
import sys
import tarfile
import tempfile
import unittest
from collections.abc import Callable, Sequence
from pathlib import Path
from types import ModuleType

SCRIPTS = Path(__file__).resolve().parent.parent
Git = Callable[[Sequence[str]], subprocess.CompletedProcess[str]]
FRONTEND = ["frontend/"]


def _load(name: str, filename: str) -> ModuleType:
    path = SCRIPTS / filename
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        msg = f"cannot load {path}"
        raise ImportError(msg)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


vt = _load("visual_tests", "visual-tests.py")
pc = _load("paths_changed", "paths-changed.py")


class PlaywrightVersionTest(unittest.TestCase):
    """The container image follows the locked @playwright/test version."""

    def test_reads_the_locked_version(self) -> None:
        """The lockfile's version picks the noble image of that version."""
        lock = {"packages": {"node_modules/@playwright/test": {"version": "1.62.1"}}}
        self.assertEqual(vt.playwright_version(lock), "1.62.1")
        self.assertEqual(
            vt.image_for("1.62.1"), "mcr.microsoft.com/playwright:v1.62.1-noble"
        )

    def test_the_repo_lockfile_has_a_version(self) -> None:
        """The real frontend/package-lock.json locks @playwright/test."""
        lock = json.loads(vt.LOCKFILE.read_text(encoding="utf-8"))
        self.assertRegex(vt.playwright_version(lock), r"^\d+\.\d+\.\d+$")

    def test_missing_entry_fails(self) -> None:
        """A lockfile without @playwright/test is an error, not a guess."""
        with self.assertRaises(TypeError):
            vt.playwright_version({"packages": {}})


class ContainerEnvTest(unittest.TestCase):
    """Only the spec's variables reach the container."""

    def test_passes_e2e_and_ci_only_and_defaults_the_base_url(self) -> None:
        """E2E_* and CI pass; anything else stays out; the base URL defaults."""
        env = vt.container_env({"E2E_TRAIN_UID": "W1", "CI": "true", "SECRET": "x"})
        self.assertEqual(
            env,
            {
                "E2E_TRAIN_UID": "W1",
                "CI": "true",
                "E2E_BASE_URL": "http://127.0.0.1:3000",
            },
        )

    def test_keeps_a_given_base_url(self) -> None:
        """A set E2E_BASE_URL wins over the default."""
        env = vt.container_env({"E2E_BASE_URL": "http://127.0.0.1:4000"})
        self.assertEqual(env["E2E_BASE_URL"], "http://127.0.0.1:4000")

    def test_runtime_override(self) -> None:
        """CONTAINER_RUNTIME picks the runtime outright."""
        self.assertEqual(vt.runtime({"CONTAINER_RUNTIME": "nerdctl"}), "nerdctl")


class CommandTest(unittest.TestCase):
    """The `run` argv, its inner script and the tars in and out."""

    def _command(self, *, update: bool) -> list[str]:
        result: list[str] = vt.run_command(
            runtime_name="docker",
            image="img:v1",
            env={"E2E_BASE_URL": "http://h"},
            update=update,
            extra=["-g", "home page"],
        )
        return result

    def test_compares_by_default(self) -> None:
        """Interactive (stdin carries the tar), host networking, then sh -c."""
        argv = self._command(update=False)
        self.assertEqual(argv[:3], ["docker", "run", "--rm"])
        self.assertIn("--interactive", argv)
        self.assertIn("--network=host", argv)
        self.assertIn("E2E_BASE_URL=http://h", argv)
        self.assertEqual(argv[-4:-1], ["img:v1", "sh", "-c"])
        script = argv[-1]
        self.assertIn(
            "node node_modules/@playwright/test/cli.js test --project=visual"
            " -g 'home page' >&2",
            script,
        )
        self.assertNotIn("--update-snapshots", script)
        self.assertNotIn("e2e/visual-baselines", script)

    def test_update_rewrites_and_returns_baselines(self) -> None:
        """--update passes --update-snapshots and tars the baselines back."""
        script = self._command(update=True)[-1]
        self.assertIn("--update-snapshots", script)
        self.assertIn("e2e/visual-baselines", script)
        self.assertTrue(script.endswith("exit $status"))

    def test_input_tar_holds_what_exists(self) -> None:
        """The inputs are relative to frontend/ and missing ones are skipped."""
        with tempfile.TemporaryDirectory() as tmp:
            frontend = Path(tmp)
            (frontend / "e2e").mkdir()
            (frontend / "e2e" / "visual.spec.ts").write_text("x", encoding="utf-8")
            (frontend / "package.json").write_text("{}", encoding="utf-8")
            with tarfile.open(fileobj=io.BytesIO(vt.input_tar(frontend))) as tar:
                names = tar.getnames()
        self.assertEqual(sorted(names), ["e2e/visual.spec.ts", "package.json"])

    def test_unpack_replaces_only_returned_outputs(self) -> None:
        """A returned test-results/ replaces the old one; others stay."""
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode="w") as tar:
            data = b"png"
            info = tarfile.TarInfo("test-results/a/diff.png")
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
        with tempfile.TemporaryDirectory() as tmp:
            frontend = Path(tmp)
            (frontend / "test-results").mkdir()
            (frontend / "test-results" / "stale.png").write_bytes(b"old")
            (frontend / "e2e" / "visual-baselines").mkdir(parents=True)
            (frontend / "e2e" / "visual-baselines" / "home.png").write_bytes(b"b")
            vt.unpack_outputs(buffer.getvalue(), frontend)
            self.assertFalse((frontend / "test-results" / "stale.png").exists())
            self.assertTrue((frontend / "test-results" / "a" / "diff.png").exists())
            self.assertTrue(
                (frontend / "e2e" / "visual-baselines" / "home.png").exists()
            )


def _git(results: dict[str, int], diff_out: str = "") -> Git:
    """Fake git: each subcommand exits with results[sub] (default 0)."""

    def git(args: Sequence[str]) -> subprocess.CompletedProcess[str]:
        code = results.get(args[0], 0)
        out = diff_out if args[0] == "diff" else ""
        return subprocess.CompletedProcess(list(args), code, out, "")

    return git


class PathsChangedTest(unittest.TestCase):
    """paths-changed.py answers true whenever it can't tell."""

    def test_unknown_base_counts_as_changed(self) -> None:
        """An empty or all-zero base (a new branch) counts as changed."""
        self.assertTrue(pc.changed("", FRONTEND, _git({})))
        self.assertTrue(pc.changed("0" * 40, FRONTEND, _git({})))

    def test_diff_output_decides(self) -> None:
        """Any changed file under the paths means changed; none means not."""
        self.assertTrue(pc.changed("abc", FRONTEND, _git({}, "frontend/a.tsx\n")))
        self.assertFalse(pc.changed("abc", FRONTEND, _git({}, "")))

    def test_failed_fetch_or_diff_counts_as_changed(self) -> None:
        """A base that can't be fetched or diffed counts as changed."""
        self.assertTrue(pc.changed("abc", FRONTEND, _git({"cat-file": 1, "fetch": 1})))
        self.assertTrue(pc.changed("abc", FRONTEND, _git({"diff": 128})))

    def test_fetches_a_missing_base_then_diffs(self) -> None:
        """A base missing from a shallow checkout is fetched, then compared."""
        self.assertFalse(pc.changed("abc", FRONTEND, _git({"cat-file": 1})))


if __name__ == "__main__":
    unittest.main()
