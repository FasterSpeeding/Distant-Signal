#!/usr/bin/env python3
# ruff: noqa: T201  # a CLI that reports what it runs on stdout
"""Run the frontend's visual regression screenshots in the pinned container.

  uv run scripts/visual-tests.py [--update] [-- PLAYWRIGHT_ARGS...]

Runs Playwright's `visual` project (frontend/e2e/visual.spec.ts) inside
mcr.microsoft.com/playwright at the version frontend/package-lock.json
locks @playwright/test to. The screenshots are only comparable when the
fonts and the renderer match, so they are always taken in that image: on
a developer's machine and in CI's frontend-e2e job alike.

Only the test runner runs in the container. It reaches the api and the
frontend (`npm run build && npm run start`) on the host through host
networking, at E2E_BASE_URL (default http://127.0.0.1:3000). Every E2E_*
variable in this environment is passed through, for the spec's fixture
ids and session cookie. frontend/node_modules must exist (`npm ci`).

The files travel as tar streams on the container's stdin and stdout, not
through a bind mount, so this also works with a remote podman service
that can't see this filesystem. In: the spec, the config, the baselines
and the three Playwright packages. Out: test-results/ (the diff images)
and e2e-report/, and with --update the new baselines. Playwright's own
output goes to stderr.

--update rewrites the baselines in frontend/e2e/visual-baselines/ instead
of comparing against them. Review the new images before committing them.

The container runtime is docker, or podman when docker is missing;
CONTAINER_RUNTIME overrides it. The exit status is Playwright's.
"""

import argparse
import io
import json
import os
import shlex
import shutil
import subprocess
import sys
import tarfile
from collections.abc import Mapping, Sequence
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FRONTEND = ROOT / "frontend"
LOCKFILE = FRONTEND / "package-lock.json"
IMAGE = "mcr.microsoft.com/playwright"
DEFAULT_BASE_URL = "http://127.0.0.1:3000"
WORKDIR = "/work"
BASELINES = "e2e/visual-baselines"
# What the visual project needs, relative to frontend/. The config and the
# spec import nothing but @playwright/test.
INPUTS = (
    "package.json",
    "playwright.config.ts",
    "e2e/visual.spec.ts",
    BASELINES,
    "node_modules/@playwright/test",
    "node_modules/playwright",
    "node_modules/playwright-core",
)
# What comes back, relative to frontend/: the diff images and the report.
OUTPUTS = ("test-results", "e2e-report")
# Variables passed into the container besides E2E_*: CI turns on the
# config's retries.
PASSTHROUGH = ("CI",)


def playwright_version(lockfile: Mapping[str, object]) -> str:
    """Return the @playwright/test version a package-lock.json pins."""
    packages = lockfile.get("packages")
    if not isinstance(packages, dict):
        msg = "package-lock.json has no `packages` table"
        raise TypeError(msg)
    entry = packages.get("node_modules/@playwright/test")
    if not isinstance(entry, dict) or not isinstance(entry.get("version"), str):
        msg = "package-lock.json doesn't lock @playwright/test"
        raise TypeError(msg)
    version: str = entry["version"]
    return version


def image_for(version: str) -> str:
    """Return the Playwright image (Ubuntu 24.04) for a Playwright version."""
    return f"{IMAGE}:v{version}-noble"


def runtime(env: Mapping[str, str]) -> str:
    """Pick the container runtime: CONTAINER_RUNTIME, docker, then podman."""
    chosen = env.get("CONTAINER_RUNTIME")
    if chosen:
        return chosen
    for name in ("docker", "podman"):
        if shutil.which(name):
            return name
    msg = "neither docker nor podman is on PATH; set CONTAINER_RUNTIME"
    raise FileNotFoundError(msg)


def container_env(env: Mapping[str, str]) -> dict[str, str]:
    """Return the variables to pass into the container, with E2E_BASE_URL set."""
    passed = {k: v for k, v in env.items() if k.startswith("E2E_") or k in PASSTHROUGH}
    passed.setdefault("E2E_BASE_URL", DEFAULT_BASE_URL)
    return passed


def inner_script(*, update: bool, extra: Sequence[str]) -> str:
    """Return the container's shell: unpack, run Playwright, pack the results.

    stdout carries only the results tar, so Playwright writes to stderr.
    """
    playwright = [
        "node",
        "node_modules/@playwright/test/cli.js",
        "test",
        "--project=visual",
        *(["--update-snapshots"] if update else []),
        *extra,
    ]
    outputs = [*OUTPUTS, *([BASELINES] if update else [])]
    return "; ".join(
        [
            f"mkdir -p {WORKDIR} && cd {WORKDIR} && tar -x",
            f"{shlex.join(playwright)} >&2",
            "status=$?",
            # Only the outputs the run created; tar fails on a missing one.
            (
                f"for d in {shlex.join(outputs)}; do [ -e $d ] && echo $d; done"
                " | tar -c -T -"
            ),
            "exit $status",
        ]
    )


def run_command(
    *,
    runtime_name: str,
    image: str,
    env: Mapping[str, str],
    update: bool,
    extra: Sequence[str],
) -> list[str]:
    """Build the `<runtime> run ...` argv for the visual project's run."""
    argv = [runtime_name, "run", "--rm", "--interactive", "--init"]
    argv += ["--network=host", "--ipc=host"]
    for key in sorted(env):
        argv += ["--env", f"{key}={env[key]}"]
    argv += [image, "sh", "-c", inner_script(update=update, extra=extra)]
    return argv


def input_tar(frontend: Path) -> bytes:
    """Tar the run's inputs, relative to frontend/."""
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w") as tar:
        for name in INPUTS:
            path = frontend / name
            if path.exists():
                tar.add(path, arcname=name)
    return buffer.getvalue()


def unpack_outputs(data: bytes, frontend: Path) -> None:
    """Replace frontend/'s outputs with the ones in the results tar."""
    if not data:
        return
    with tarfile.open(fileobj=io.BytesIO(data)) as tar:
        tops = {name.split("/", 1)[0] for name in tar.getnames()}
        for top in (*OUTPUTS, BASELINES):
            if top.split("/", 1)[0] in tops and (frontend / top).is_dir():
                shutil.rmtree(frontend / top)
        tar.extractall(frontend, filter="data")


def main(argv: Sequence[str] | None = None) -> int:
    """Run the visual project in the container and return its exit status."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--update",
        action="store_true",
        help="rewrite the baselines instead of comparing against them",
    )
    parser.add_argument(
        "playwright_args",
        nargs="*",
        help="extra arguments for `playwright test` (after --)",
    )
    args = parser.parse_args(argv)

    if not (FRONTEND / "node_modules" / "@playwright" / "test").is_dir():
        print(
            "frontend/node_modules is missing; run `npm ci` in frontend/ first",
            file=sys.stderr,
        )
        return 2
    lock = json.loads(LOCKFILE.read_text(encoding="utf-8"))
    command = run_command(
        runtime_name=runtime(os.environ),
        image=image_for(playwright_version(lock)),
        env=container_env(os.environ),
        update=args.update,
        extra=args.playwright_args,
    )
    # Print the command without the variables' values: one is a session cookie.
    shown = [
        a.split("=", 1)[0] if a.startswith(("E2E_", "CI=")) else a for a in command
    ]
    print("==", shlex.join(shown), flush=True)
    result = subprocess.run(  # noqa: S603  # argv built by run_command
        command, input=input_tar(FRONTEND), stdout=subprocess.PIPE, check=False
    )
    unpack_outputs(result.stdout, FRONTEND)
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
