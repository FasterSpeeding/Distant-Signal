"""Tests for scripts/gen-rust-dockerfiles.py.

uv run python -m unittest discover -s scripts/tests
"""

import contextlib
import importlib.util
import io
import re
import sys
import tempfile
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

    def test_api_ships_its_eight_binaries(self) -> None:
        """Api cooks, builds and copies out all eight binaries, ds-migrate too."""
        block = gen.render("api")
        for name in gen.SERVICES["api"]:
            self.assertIn(f"/usr/local/bin/{name} ", block)
        self.assertIn("ds-migrate", gen.SERVICES["api"])
        self.assertEqual(len(gen.SERVICES["api"]), 8)

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


class BuildInputsTests(unittest.TestCase):
    """The per-Dockerfile build-context allowlists."""

    def test_rust_allows(self) -> None:
        """RUST_INPUTS and their contents, minus target/ and env files."""
        for path in (
            "Cargo.toml",
            "crates/api/src/main.rs",
            "reference-data/toc-codes.csv",
            "charts/distant-signal/files/db-grants.yaml",
        ):
            with self.subTest(path=path):
                self.assertTrue(gen.rust_allows(path))
        for path in (
            "docs/README.md",
            "frontend/package.json",
            "charts/distant-signal/values.yaml",
            "crates/api/target/debug/api",
            "crates/api/local.env",
            "Cargo.toml.orig",
        ):
            with self.subTest(path=path):
                self.assertFalse(gen.rust_allows(path))

    def test_every_hidden_input_is_allowed(self) -> None:
        """Each include_str!/build.rs/COPY input is in the Rust context."""
        found = list(gen.hidden_inputs())
        paths = {path for _, path in found}
        # The scan sees the known ones (so it is not silently empty).
        for known in (
            "charts/distant-signal/files/db-grants.yaml",
            "reference-data/tiploc-parent-stations.csv",
            "reference-data/delay-attribution-reasons.tsv",
            "crates/ds-store/migrations",
            "lines",
        ):
            self.assertIn(known, paths)
        for where, path in found:
            with self.subTest(where=where, path=path):
                self.assertTrue(gen.rust_allows(path))

    def test_ignore_file_is_an_allowlist(self) -> None:
        """Everything is ignored first, then RUST_INPUTS let back in."""
        lines = gen.rust_ignore_lines()
        self.assertEqual(lines[0], "*")
        for path in gen.RUST_INPUTS:
            self.assertIn(f"!{path}", lines)

    def test_ignore_path_is_buildkits(self) -> None:
        """BuildKit's name: `<Dockerfile>.dockerignore` beside it."""
        self.assertEqual(
            gen.ignore_path(Path("/r/docker/api.Dockerfile")),
            Path("/r/docker/api.Dockerfile.dockerignore"),
        )


class RuntimeStageTests(unittest.TestCase):
    """The last stage's name, which containers.yml's weekly rebuild uses."""

    def test_unnamed_last_stage_is_a_problem(self) -> None:
        """Only a last stage `AS runtime` passes."""
        with tempfile.TemporaryDirectory() as tmp:
            good = Path(tmp) / "good.Dockerfile"
            good.write_text("FROM a AS chef\nFROM debian@sha256:0 AS runtime\n")
            bad = Path(tmp) / "bad.Dockerfile"
            bad.write_text("FROM a AS runtime\nFROM debian@sha256:0\n")
            problems = gen.runtime_stage_problems({"good": good, "bad": bad})
        self.assertEqual(len(problems), 1)
        self.assertIn("docker/bad.Dockerfile", problems[0])


class RuntimeUpgradeTests(unittest.TestCase):
    """`apt-get upgrade` in the runtime stage's apt RUN."""

    TEXT = (
        "FROM a AS chef\n"
        f"{gen.APT_UPDATE}\n"
        "    && apt-get install -y x\n"
        "FROM debian@sha256:0 AS runtime\n"
        "ARG X=1\n"
        f"{gen.APT_UPDATE}\n"
        "    && apt-get install -y y\n"
    )

    def test_inserted_once_in_the_runtime_stage_only(self) -> None:
        """Added after the runtime stage's update, and idempotent."""
        out = gen.with_runtime_upgrade(self.TEXT, "runtime", Path("x"))
        lines = out.splitlines()
        self.assertEqual(lines.count(gen.APT_UPGRADE), 1)
        self.assertEqual(lines[lines.index(gen.APT_UPGRADE) - 2], "ARG X=1")
        self.assertEqual(gen.with_runtime_upgrade(out, "runtime", Path("x")), out)

    def test_runtime_stage_without_apt_update_is_an_error(self) -> None:
        """A runtime stage whose first RUN isn't the update is rejected."""
        text = "FROM debian@sha256:0 AS runtime\nRUN true\n"
        with self.assertRaises(ValueError):
            gen.with_runtime_upgrade(text, "runtime", Path("x"))


class CommittedFilesTests(unittest.TestCase):
    """The repo's own Dockerfiles."""

    def test_committed_dockerfiles_are_current(self) -> None:
        """`--check` passes on the committed Dockerfiles and ignore files."""
        with contextlib.redirect_stdout(io.StringIO()) as out:
            status = gen.main(["--check"])
        self.assertEqual(status, 0, out.getvalue())


if __name__ == "__main__":
    unittest.main()
