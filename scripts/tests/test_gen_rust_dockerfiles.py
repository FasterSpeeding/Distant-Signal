"""Tests for scripts/gen-rust-dockerfiles.py.

uv run python -m unittest discover -s scripts/tests
"""

import contextlib
import importlib.util
import io
import re
import sys
import unittest
from pathlib import Path
from types import ModuleType

SCRIPT = Path(__file__).resolve().parent.parent / "gen-rust-dockerfiles.py"


def load_script() -> ModuleType:
    """Import the hyphenated script as a module."""
    spec = importlib.util.spec_from_file_location("gen_rust_dockerfiles", SCRIPT)
    if spec is None or spec.loader is None:
        raise ImportError(SCRIPT)
    module = importlib.util.module_from_spec(spec)
    sys.modules["gen_rust_dockerfiles"] = module
    spec.loader.exec_module(module)
    return module


gen = load_script()

COOK = re.compile(r"^ +cargo chef cook (.*); \\$", re.MULTILINE)
BUILD = re.compile(
    r"^ +cargo build (.*) > /tmp/cargo-build\.log 2>&1; \\$", re.MULTILINE
)


class RenderTests(unittest.TestCase):
    """The generated stages."""

    def test_cook_and_build_take_the_same_flags(self) -> None:
        """Per profile, the cook's cargo flags are exactly the build's."""
        for service in gen.SERVICES:
            with self.subTest(service=service):
                block = gen.render(service)
                cooks = [
                    flags.replace(" --recipe-path recipe.json", "")
                    for flags in COOK.findall(block)
                ]
                builds = BUILD.findall(block)
                self.assertEqual(len(cooks), 2)  # release, then debug
                self.assertEqual(cooks, builds)

    def test_one_service_per_cargo_call(self) -> None:
        """Never a workspace-wide cook or build (rustls feature unification)."""
        for service in gen.SERVICES:
            with self.subTest(service=service):
                block = gen.render(service)
                for forbidden in ("--workspace", "--bins", "--all"):
                    self.assertNotIn(forbidden, block)
                for flags in BUILD.findall(block):
                    bins = re.findall(r"--bin (\S+)", flags)
                    self.assertEqual(tuple(bins), tuple(gen.SERVICES[service]))

    def test_api_ships_its_five_binaries(self) -> None:
        """Api cooks, builds and copies out all five binaries."""
        block = gen.render("api")
        for name in gen.SERVICES["api"]:
            self.assertIn(f"/usr/local/bin/{name} ", block)
        self.assertEqual(len(gen.SERVICES["api"]), 5)

    def test_no_target_cache_mount(self) -> None:
        """A target/ cache mount would leave the cook layer empty."""
        for service in gen.SERVICES:
            with self.subTest(service=service):
                self.assertNotIn("target=/app/target", gen.render(service))


class SpliceTests(unittest.TestCase):
    """Replacing the marked block."""

    def test_replaces_only_between_the_markers(self) -> None:
        """Text outside the markers is kept."""
        text = f"head\n{gen.BEGIN}\nold\n{gen.END}\ntail\n"
        self.assertEqual(
            gen.splice(text, "new\n", Path("x")),
            "head\nnew\ntail\n",
        )

    def test_missing_markers_are_an_error(self) -> None:
        """A Dockerfile without exactly one block is rejected."""
        for text in ("no markers\n", f"{gen.END}\n{gen.BEGIN}\n"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                gen.splice(text, "new\n", Path("x"))


class CommittedFilesTests(unittest.TestCase):
    """The repo's own Dockerfiles."""

    def test_committed_dockerfiles_are_current(self) -> None:
        """`--check` passes on the committed docker/*.Dockerfile."""
        with contextlib.redirect_stdout(io.StringIO()) as out:
            status = gen.main(["--check"])
        self.assertEqual(status, 0, out.getvalue())


if __name__ == "__main__":
    unittest.main()
