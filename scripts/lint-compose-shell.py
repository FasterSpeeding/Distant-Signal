#!/usr/bin/env python3
# ruff: noqa: T201  # a CLI whose output (the findings) is stdout
"""ShellCheck the inline shell in the tracked docker-compose*.yml files.

  scripts/lint-compose-shell.py

Checks, per service, the snippets Compose hands to a shell:
  - `healthcheck.test`: ["CMD-SHELL", "<script>"], or a bare string (which
    Compose treats as CMD-SHELL); both run as `/bin/sh -c <script>`.
  - `command` / `entrypoint`: ["sh" | "bash" (or /bin/...), "-c", "<script>"],
    or the same as a string (Compose splits it shell-style, as here).
Each snippet goes to shellcheck (repo .shellcheckrc: every optional check,
style severity) with the matching --shell. Compose interpolation is undone
first: `$$` is a literal `$` for the shell; a single-`$` reference is
substituted by Compose before the shell sees it, so it becomes a plain word.
The container's own variables (the service's `environment:` keys) are
declared on an `export` line ahead of the snippet (so not SC2154), which
puts the snippet itself on line 2 of shellcheck's report.

Opt out of a finding the usual way: a `# shellcheck disable=SCxxxx # reason`
line at the start of the snippet.

Exit 1 if shellcheck reports anything. Needs PyYAML and shellcheck (both
pinned in pyproject.toml's `lint` dependency group).
"""

import pathlib
import re
import shlex
import subprocess
import sys
from collections.abc import Iterator

import yaml

ROOT = pathlib.Path(__file__).resolve().parent.parent
SHELLS = {"sh": "sh", "/bin/sh": "sh", "bash": "bash", "/bin/bash": "bash"}
# Compose interpolation: `$$` escapes a literal `$`; `$VAR` / `${VAR...}` is
# Compose's own substitution.
INTERPOLATION = re.compile(r"\$\$|\$\{[^}]*\}|\$[A-Za-z_][A-Za-z0-9_]*")


def shell_text(compose: str) -> str:
    """Return the text the shell receives once Compose has interpolated."""
    return INTERPOLATION.sub(
        lambda m: "$" if m.group() == "$$" else "compose_interpolated", compose
    )


def shell_c(argv: object) -> tuple[str, str] | None:
    """Return (shell, script) if argv is `<sh|bash> -c <script>`, else None."""
    if isinstance(argv, str):
        argv = shlex.split(argv)
    if (
        isinstance(argv, list)
        and len(argv) >= 3  # noqa: PLR2004  # shell, -c, script
        and argv[0] in SHELLS
        and argv[1] == "-c"
        and isinstance(argv[2], str)
    ):
        return SHELLS[argv[0]], argv[2]
    return None


def environment(service: dict[str, object]) -> list[str]:
    """Return the variable names a service's `environment:` sets."""
    env = service.get("environment") or {}
    if isinstance(env, dict):
        return [str(key) for key in env]
    if isinstance(env, list):
        return [str(item).split("=", 1)[0] for item in env]
    return []


def snippets(path: pathlib.Path) -> Iterator[tuple[str, str, str]]:
    """Yield (location, shell, lint input) for each inline shell in a file."""
    doc = yaml.safe_load(path.read_text())
    services = (doc or {}).get("services") or {}
    for name, service in services.items():
        where = f"{path.relative_to(ROOT)}: services.{name}"
        env = environment(service)
        prefix = f"export {' '.join(env)}\n" if env else ""
        found: list[tuple[str, str, str]] = []
        test = (service.get("healthcheck") or {}).get("test")
        if isinstance(test, str):
            found.append(("healthcheck.test", "sh", test))
        elif isinstance(test, list) and test[:1] == ["CMD-SHELL"]:
            found.append(("healthcheck.test", "sh", str(test[1])))
        for key in ("entrypoint", "command"):
            shell_script = shell_c(service.get(key))
            if shell_script is not None:
                found.append((key, *shell_script))
        for key, shell, script in found:
            yield f"{where}.{key}", shell, prefix + shell_text(script) + "\n"


def main() -> int:
    """Lint every snippet; return 1 if any has a finding."""
    listed = subprocess.run(
        ["git", "ls-files", "--", "docker-compose*.yml"],  # noqa: S607
        cwd=ROOT,
        capture_output=True,
        check=True,
        text=True,
    ).stdout.split()
    status = 0
    count = 0
    for name in listed:
        for where, shell, script in snippets(ROOT / name):
            count += 1
            # Fixed argv; the snippet only goes to shellcheck's stdin.
            result = subprocess.run(  # noqa: S603
                [  # noqa: S607  # the pinned shellcheck on PATH
                    "shellcheck",
                    f"--rcfile={ROOT / '.shellcheckrc'}",
                    f"--shell={shell}",
                    "-",
                ],
                input=script,
                capture_output=True,
                check=False,
                text=True,
            )
            if result.returncode != 0:
                status = 1
                print(f"{where}:\n{result.stdout}{result.stderr}")
    print(f"{count} inline shell snippet(s) checked")
    return status


if __name__ == "__main__":
    sys.exit(main())
