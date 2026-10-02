#!/usr/bin/env python3
# ruff: noqa: T201  # a CLI whose output (the failures) is stdout
"""Render charts/distant-signal's schedulefeed and check its env and sources.

  scripts/check-schedulefeed-chart.py [--helm HELM] [--baseline DIR]

Renders the chart with schedulefeed on (CI's required-value flags plus
`scheduleFeed.enabled=true scheduleFeed.sftp.authMethod=password`) and
checks:

  - extraEnv override: `scheduleFeed.ingest.extraEnv` replaces the chart's
    entry of the same name in its place (RUST_LOG, still right after
    PROGRESS_STALL_SECS), once, and an extra-only name follows the chart's
    entries;
  - no container in any rendered document repeats an env name.

--baseline DIR renders DIR (a copy of charts/distant-signal from another
commit, e.g. the merge base) and this chart with the same flags, for the
defaults; schedulefeed on; schedulefeed on with the NetworkPolicy and its
egress rules; and values-example.yaml. Every document must be identical
after normalisation (the `data`/`stringData` of every Secret are dropped:
genPrivateKey and randAlphaNum differ between runs). A difference fails
with a unified diff per document. A one-off check for refactors; CI
doesn't run it (on main it would compare main with itself).

Exit 1 with one line per failed check. Needs helm on PATH (or --helm) and
PyYAML (pinned in pyproject.toml's `lint` dependency group).
"""

import argparse
import difflib
import pathlib
import shutil
import subprocess
import sys
from collections.abc import Sequence
from typing import cast

import yaml

REPO = pathlib.Path(__file__).resolve().parent.parent
CHART = REPO / "charts" / "distant-signal"
EXAMPLE = "values-example.yaml"
# CI's required-value flags (the kafka/LLM/SSO settings only satisfy the
# chart's required-value checks).
BASE = (
    "--set",
    "trustConsumer.kafka.brokers=k:9094",
    "--set",
    "trustConsumer.kafka.topic=t",
    "--set",
    "trustConsumer.kafka.saslMechanism=PLAIN",
    "--set",
    "enricher.llm.baseUrl=http://l/v1",
    "--set",
    "enricher.llm.model=m",
    "--set",
    "api.sso.issuerUrl=https://sso.example.com",
    "--set",
    "api.sso.clientId=c",
    "--set",
    "api.sso.clientSecret=s",
    "--set",
    "api.sso.redirectUrl=https://app.example.com/cb",
    "--set",
    "api.sso.postLoginRedirectUrl=https://app.example.com/",
)
ON = (
    "--set",
    "scheduleFeed.enabled=true",
    "--set",
    "scheduleFeed.sftp.authMethod=password",
)
NETPOL = (
    "--set",
    "networkPolicy.enabled=true",
    "--set",
    "networkPolicy.egress.enabled=true",
)
# The value sets --baseline compares; every one is rendered with BASE.
BASELINE_SETS: tuple[tuple[str, tuple[str, ...]], ...] = (
    ("defaults", ()),
    ("schedulefeed on", ON),
    ("schedulefeed on, network policy", (*ON, *NETPOL)),
    ("values-example.yaml", ("-f", EXAMPLE)),
)

type Doc = dict[str, object]
type Container = dict[str, object]
type EnvEntry = dict[str, object]


class Checker:
    """Collects failed checks."""

    def __init__(self, helm: str) -> None:
        """Use `helm` to render."""
        self.helm = helm
        self.failures: list[str] = []

    def check(self, *, ok: bool, message: str) -> None:
        """Record `message` as a failure unless `ok`."""
        if not ok:
            self.failures.append(message)

    def render(self, *args: str, chart: pathlib.Path = CHART) -> tuple[int, str]:
        """Run `helm template` on `chart` with BASE and `args`."""
        args = tuple(str(chart / a) if a == EXAMPLE else a for a in args)
        result = subprocess.run(  # noqa: S603  # helm from PATH or --helm, fixed arguments
            [self.helm, "template", "distant-signal", str(chart), *BASE, *args],
            capture_output=True,
            text=True,
            check=False,
        )
        return result.returncode, result.stdout + result.stderr

    def docs(self, *args: str, chart: pathlib.Path = CHART) -> list[Doc]:
        """Render and parse every non-empty YAML document."""
        code, out = self.render(*args, chart=chart)
        if code != 0:
            self.failures.append(f"render {list(args)} failed: {out.strip()}")
            return []
        return [cast("Doc", d) for d in yaml.safe_load_all(out) if d]


def name_of(doc: Doc) -> str:
    """Return `Kind/name` of a rendered resource."""
    return f"{doc.get('kind')}/{cast('dict[str, str]', doc['metadata'])['name']}"


def containers_of(doc: Doc) -> list[Container]:
    """Return a workload resource's containers (none for anything else)."""
    spec = cast("dict[str, object]", doc.get("spec") or {})
    template = cast("dict[str, object]", spec.get("template") or {})
    pod = cast("dict[str, object]", template.get("spec") or {})
    return cast("list[Container]", pod.get("containers") or [])


def schedulefeed(docs: Sequence[Doc]) -> Doc | None:
    """Return the schedulefeed Deployment, if rendered."""
    for doc in docs:
        if doc.get("kind") == "Deployment" and name_of(doc).endswith("-schedulefeed"):
            return doc
    return None


def container(docs: Sequence[Doc], name: str) -> Container:
    """Return the schedulefeed Deployment's container `name` (empty if absent)."""
    deployment = schedulefeed(docs)
    found = [c for c in containers_of(deployment or {}) if c.get("name") == name]
    return found[0] if found else {}


def env_list(c: Container) -> list[EnvEntry]:
    """Return a container's env, in order."""
    return cast("list[EnvEntry]", c.get("env") or [])


def env(c: Container) -> dict[str, EnvEntry]:
    """Return a container's env, by name."""
    return {str(e["name"]): e for e in env_list(c)}


def normalise(docs: Sequence[Doc]) -> dict[str, str]:
    """Each document as YAML, by `Kind/name`, minus Secret data."""
    out: dict[str, str] = {}
    for doc in docs:
        kept = doc
        if doc.get("kind") == "Secret":
            kept = {k: v for k, v in doc.items() if k not in {"data", "stringData"}}
        out[name_of(doc)] = yaml.safe_dump(kept, sort_keys=True)
    return out


def compare(c: Checker, label: str, old: Sequence[Doc], new: Sequence[Doc]) -> None:
    """Fail with a unified diff for every document that differs."""
    before, after = normalise(old), normalise(new)
    for key in sorted(before.keys() | after.keys()):
        a, b = before.get(key, ""), after.get(key, "")
        if a != b:
            diff = difflib.unified_diff(
                a.splitlines(keepends=True),
                b.splitlines(keepends=True),
                f"{label}: {key} (before)",
                f"{label}: {key} (after)",
            )
            c.failures.append(f"{label}: {key} differs\n{''.join(diff)}")


def check_baseline(c: Checker, base: pathlib.Path) -> None:
    """Every BASELINE_SETS render of `base` equals this chart's."""
    for label, args in BASELINE_SETS:
        compare(c, label, c.docs(*args, chart=base), c.docs(*args))


def check_no_duplicate_env(c: Checker, label: str, docs: Sequence[Doc]) -> None:
    """No container in any document repeats an env name."""
    for doc in docs:
        for ctr in containers_of(doc):
            names = [str(e["name"]) for e in env_list(ctr)]
            dupes = sorted({n for n in names if names.count(n) > 1})
            c.check(
                ok=not dupes,
                message=f"{label}: {name_of(doc)} {ctr.get('name')} repeats {dupes}",
            )


def check_extra_env(c: Checker) -> None:
    """scheduleFeed.ingest.extraEnv replaces by name, in place; extras follow."""
    docs = c.docs(
        *ON,
        "--set",
        "scheduleFeed.ingest.extraEnv[0].name=RUST_LOG",
        "--set-string",
        "scheduleFeed.ingest.extraEnv[0].value=trace",
        "--set",
        "scheduleFeed.ingest.extraEnv[1].name=EXTRA_ONLY",
        "--set-string",
        "scheduleFeed.ingest.extraEnv[1].value=x",
    )
    check_no_duplicate_env(c, "ingest extraEnv", docs)
    names = [str(e["name"]) for e in env_list(container(docs, "ingest"))]
    values = env(container(docs, "ingest"))
    c.check(
        ok=names.count("RUST_LOG") == 1 and values["RUST_LOG"].get("value") == "trace",
        message=f"ingest extraEnv: RUST_LOG not replaced: {values.get('RUST_LOG')}",
    )
    at = names.index("RUST_LOG") if "RUST_LOG" in names else 0
    c.check(
        ok=at > 0 and names[at - 1] == "PROGRESS_STALL_SECS",
        message=f"ingest extraEnv: RUST_LOG moved: {names}",
    )
    c.check(
        ok=bool(names) and names[-1] == "EXTRA_ONLY",
        message=f"ingest extraEnv: the extra-only entry isn't last: {names}",
    )


def main(argv: Sequence[str] | None = None) -> int:
    """Run every check; return the exit status."""
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--helm", default=shutil.which("helm") or "helm", help="helm binary"
    )
    parser.add_argument(
        "--baseline",
        type=pathlib.Path,
        metavar="DIR",
        help="also check DIR (another commit's charts/distant-signal) renders "
        "identically",
    )
    args = parser.parse_args(argv)
    c = Checker(args.helm)

    check_no_duplicate_env(c, "schedulefeed on", c.docs(*ON))
    check_extra_env(c)
    if args.baseline is not None:
        check_baseline(c, args.baseline)

    for failure in c.failures:
        print(f"FAIL: {failure}")
    if not c.failures:
        print("schedulefeed chart: all checks passed")
    return 1 if c.failures else 0


if __name__ == "__main__":
    sys.exit(main())
