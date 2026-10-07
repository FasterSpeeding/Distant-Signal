#!/usr/bin/env python3
"""Compare the api's metric names and SQL text between two trees.

  uv run scripts/diff-api-surface.py diff BASE [HEAD] [--ignore-test-sql]
  uv run scripts/diff-api-surface.py dump [TREE]

The check behind ingest architecture plan phase 1A (spec §5.5): moving code
from `crates/api` into `crates/ds-store` must not change the api's
`/metrics` series names or any SQL it runs. Both are read statically from
the Rust sources of `crates/api` and `crates/ds-store` (src, tests, bins):

- **metrics**: the series names the api's own code registers: the first
  argument of every `counter!`/`gauge!`/`histogram!` (and `describe_*!`)
  and of `metric_name(...)` and `register_operation_counters(...)`
  outside test code. A string literal is taken as is; a constant is
  resolved through its `const NAME: &str = "...";`; `metric_name` adds
  its `distant_signal_` prefix. Anything else (a variable, a `format!`)
  is recorded as its token text, which a move does not change either.
- **sql**: every string literal that looks like SQL (an upper-case SQL
  keyword, or a statement starting with a lower-case one), whitespace
  collapsed, with how often it occurs; plus the contents of `.sql` files
  pulled in with `include_str!`. Literals in test code (`tests/`,
  `benches/`, `#[cfg(test)]` items) are listed separately as test-sql.

Why static, not a scrape of a running api: `/metrics` shows a series only
after its first observation unless it was registered at zero, so a scrape
depends on which code paths ran; and starting the api needs Postgres,
Redis, the line catalogue and auth config. The static list is
deterministic and covers every call site. The axum-prometheus request
metrics (`distant_signal_http_requests_*`) come from the library and the
prefix in `main.rs`, which no move touches.

A tree is a git revision (`wt-batch43`, `HEAD~1`, a commit id; read with
`git show`, so no checkout is made) or `dir:PATH`, a directory: its
git-tracked and untracked-but-not-ignored files when it is a git work tree,
else every file. HEAD defaults to the work tree at the repo root
(`dir:<toplevel>`), so uncommitted changes count.

`diff` prints a unified diff per section and exits 1 when anything
differs, 0 when nothing does. A pure move must exit 0. `--ignore-test-sql`
skips the test-sql section, for a move that has to duplicate a test
fixture's SQL (say why in the commit). `dump` prints one tree's surface.
Exit 2: a git error. Stdlib only.
"""

import argparse
import difflib
import re
import subprocess
import sys
from collections import Counter
from collections.abc import Iterator, Sequence
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath
from typing import Literal, Protocol

CRATES = ("crates/api", "crates/ds-store")
METRIC_MACROS = frozenset(
    {
        "counter",
        "gauge",
        "histogram",
        "describe_counter",
        "describe_gauge",
        "describe_histogram",
    }
)
# Functions whose first argument is a metric name without its prefix.
METRIC_FUNCTIONS = frozenset({"metric_name", "register_operation_counters"})
METRIC_PREFIX = "distant_signal_"
# Upper-case SQL keywords; matching is case-sensitive, so prose rarely hits.
SQL_KEYWORDS = re.compile(
    r"\b(SELECT|INSERT|UPDATE|DELETE|FROM|WHERE|JOIN|VALUES|RETURNING"
    r"|ON CONFLICT|CREATE|ALTER|DROP|TRUNCATE|LOCK|ORDER BY|GROUP BY|LIMIT"
    r"|SET|WITH|UNION|EXISTS|COALESCE|AND|OR|NOT|NULL|IS|IN|AS|CASE|WHEN"
    r"|THEN|END|DISTINCT|INTERVAL|ANALYZE|VACUUM|REFRESH|NOTIFY|GRANT"
    r"|REVOKE|BEGIN|COMMIT|ROLLBACK|SAVEPOINT|COPY|EXPLAIN)\b"
)
SQL_LOWER_START = re.compile(
    r"(select|insert|update|delete|with|create|alter|drop|truncate|lock|set"
    r"|analyze|vacuum|explain)\s"
)
ESCAPES = {"n": "\n", "r": "\r", "t": "\t", "\\": "\\", "0": "\0", "'": "'", '"': '"'}
RAW_STRING = re.compile(r'[bc]?r(#*)"')
PREFIXED_STRING = re.compile(r'[bc]"')

Kind = Literal["ident", "str", "punct", "other"]


class GitError(Exception):
    """A git command failed; its stderr has already been shown."""


@dataclass
class Token:
    """One Rust token; `text` is the value for a string literal."""

    kind: Kind
    text: str
    test: bool = False


class Tree(Protocol):
    """A set of files: a git revision or a directory."""

    def files(self) -> list[str]:
        """Return the repo-relative paths under CRATES."""
        ...

    def read(self, path: str) -> str | None:
        """Return a repo-relative file's text, or None if it is absent."""
        ...


def git(cwd: Path, *args: str) -> str:
    """Run git; return stdout. Raises GitError on failure."""
    result = subprocess.run(  # noqa: S603  # fixed git subcommands
        ["git", *args],  # noqa: S607  # git from PATH
        check=False,
        stdout=subprocess.PIPE,
        cwd=cwd,
    )
    if result.returncode != 0:
        raise GitError(" ".join(args))
    return result.stdout.decode("utf-8", errors="replace")


@dataclass
class GitRev:
    """A tree read from a git revision without checking it out."""

    repo: Path
    rev: str

    def files(self) -> list[str]:
        """Return the revision's files under CRATES."""
        out = git(
            self.repo, "ls-tree", "-r", "-z", "--name-only", self.rev, "--", *CRATES
        )
        return sorted(name for name in out.split("\0") if name)

    def read(self, path: str) -> str | None:
        """Return the file at the revision, or None."""
        try:
            return git(self.repo, "show", f"{self.rev}:{path}")
        except GitError:
            return None


@dataclass
class Directory:
    """A tree on disk (the work tree, or a test fixture)."""

    root: Path

    def files(self) -> list[str]:
        """Return the files under CRATES (git's view when it is a work tree)."""
        if (self.root / ".git").exists():
            out = git(
                self.root,
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
                "--",
                *CRATES,
            )
            names = {name for name in out.split("\0") if name}
            return sorted(name for name in names if (self.root / name).is_file())
        return sorted(
            path.relative_to(self.root).as_posix()
            for crate in CRATES
            for path in (self.root / crate).rglob("*")
            if path.is_file()
        )

    def read(self, path: str) -> str | None:
        """Return the file's text, or None."""
        target = self.root / path
        return target.read_text(encoding="utf-8") if target.is_file() else None


@dataclass
class Lexer:
    """A Rust lexer good enough to find string literals and identifiers.

    Handles line, block (nested) and doc comments, raw strings (`r#"…"#`),
    byte and C strings, escapes and line continuations, char literals
    versus lifetimes, and raw identifiers.
    """

    source: str
    pos: int = 0
    tokens: list[Token] = field(default_factory=list)

    def run(self) -> list[Token]:
        """Tokenise the whole source."""
        while self.pos < len(self.source):
            self.step()
        return self.tokens

    def peek(self, offset: int = 0) -> str:
        """Return the character at pos+offset, or '' past the end."""
        index = self.pos + offset
        return self.source[index] if index < len(self.source) else ""

    def step(self) -> None:
        """Consume one token, comment or run of whitespace."""
        char = self.peek()
        if char.isspace():
            self.pos += 1
        elif self.source.startswith("//", self.pos):
            end = self.source.find("\n", self.pos)
            self.pos = len(self.source) if end < 0 else end
        elif self.source.startswith("/*", self.pos):
            self.block_comment()
        elif char == '"':
            self.pos += 1
            self.tokens.append(Token("str", self.quoted('"')))
        elif char == "'":
            self.quote()
        elif char.isalpha() or char == "_":
            self.word()
        elif char.isdigit():
            self.number()
        else:
            self.pos += 1
            self.tokens.append(Token("punct", char))

    def block_comment(self) -> None:
        """Skip a (possibly nested) block comment."""
        depth = 0
        while self.pos < len(self.source):
            if self.source.startswith("/*", self.pos):
                depth += 1
                self.pos += 2
            elif self.source.startswith("*/", self.pos):
                depth -= 1
                self.pos += 2
                if depth == 0:
                    return
            else:
                self.pos += 1

    def quoted(self, quote: str) -> str:
        """Read an escaped literal's body up to its closing quote."""
        out: list[str] = []
        while self.pos < len(self.source):
            char = self.source[self.pos]
            if char == quote:
                self.pos += 1
                break
            if char == "\\":
                out.append(self.escape())
            else:
                out.append(char)
                self.pos += 1
        return "".join(out)

    def escape(self) -> str:
        """Decode the escape at pos (a backslash); return its value."""
        kind = self.peek(1)
        if kind == "\n":  # line continuation: drop the newline and indent
            self.pos += 2
            while self.peek().isspace():
                self.pos += 1
            return ""
        if kind == "x":
            value = chr(int(self.source[self.pos + 2 : self.pos + 4], 16))
            self.pos += 4
            return value
        if kind == "u":
            end = self.source.index("}", self.pos)
            value = chr(int(self.source[self.pos + 3 : end].replace("_", ""), 16))
            self.pos = end + 1
            return value
        self.pos += 2
        return ESCAPES.get(kind, kind)

    def quote(self) -> None:
        """Read a char literal (`'a'`, an escape) or a lifetime/label."""
        if self.peek(1) == "\\":
            self.pos += 1
            self.tokens.append(Token("other", self.quoted("'")))
        elif self.peek(2) == "'":
            self.tokens.append(Token("other", self.peek(1)))
            self.pos += 3
        else:
            start = self.pos
            self.pos += 1
            while self.peek().isalnum() or self.peek() == "_":
                self.pos += 1
            self.tokens.append(Token("other", self.source[start : self.pos]))

    def word(self) -> None:
        """Read an identifier, or a raw/byte/C string or byte char."""
        rest = self.source[self.pos : self.pos + 260]
        if raw := RAW_STRING.match(rest):
            self.pos += raw.end()
            closing = '"' + raw.group(1)
            end = self.source.index(closing, self.pos)
            self.tokens.append(Token("str", self.source[self.pos : end]))
            self.pos = end + len(closing)
        elif PREFIXED_STRING.match(rest):
            self.pos += 2
            self.tokens.append(Token("str", self.quoted('"')))
        elif rest.startswith("b'"):
            self.pos += 1
            self.quote()
        else:
            if rest.startswith("r#"):
                self.pos += 2
            start = self.pos
            while self.peek().isalnum() or self.peek() == "_":
                self.pos += 1
            self.tokens.append(Token("ident", self.source[start : self.pos]))

    def number(self) -> None:
        """Read a numeric literal (its digits, suffix and underscores)."""
        start = self.pos
        while self.peek().isalnum() or self.peek() == "_":
            self.pos += 1
        self.tokens.append(Token("other", self.source[start : self.pos]))


def matching(tokens: Sequence[Token], start: int) -> int:
    """Return the index of the bracket closing the one at `start`."""
    pairs = {"(": ")", "[": "]", "{": "}"}
    stack: list[str] = []
    for index in range(start, len(tokens)):
        token = tokens[index]
        if token.kind != "punct":
            continue
        if token.text in pairs:
            stack.append(pairs[token.text])
        elif stack and token.text == stack[-1]:
            stack.pop()
            if not stack:
                return index
    return len(tokens) - 1


def is_cfg_test(tokens: Sequence[Token], index: int) -> bool:
    """Return whether `#[cfg(test)]` starts at `index`."""
    texts = [token.text for token in tokens[index : index + 7]]
    return texts == ["#", "[", "cfg", "(", "test", ")", "]"]


def mark_test_items(tokens: list[Token]) -> list[str]:
    """Flag the tokens of every `#[cfg(test)]` item (module, fn, impl).

    Returns the names of the `#[cfg(test)] mod NAME;` file modules, whose
    files are test code too.
    """
    file_modules: list[str] = []
    index = 0
    while index < len(tokens):
        if not is_cfg_test(tokens, index):
            index += 1
            continue
        cursor = index
        # Step over this and any further attributes, then to the item's body
        # (or its `;` for a body-less item such as `use`).
        while cursor < len(tokens):
            text = tokens[cursor].text
            if (
                text == "#"
                and cursor + 1 < len(tokens)
                and tokens[cursor + 1].text == "["
            ):
                cursor = matching(tokens, cursor + 1) + 1
            elif text in {"{", ";"}:
                break
            elif text in {"(", "["}:
                cursor = matching(tokens, cursor) + 1
            else:
                cursor += 1
        end = (
            matching(tokens, cursor)
            if cursor < len(tokens) and tokens[cursor].text == "{"
            else cursor
        )
        for token in tokens[index : end + 1]:
            token.test = True
        if tokens[end].text == ";" and [t.text for t in tokens[end - 2 : end - 1]] == [
            "mod"
        ]:
            file_modules.append(tokens[end - 1].text)
        index = end + 1
    return file_modules


def tokenize(path: str, source: str) -> list[Token]:
    """Tokenise a Rust file, flagging test code."""
    return tokenize_with_modules(path, source)[0]


def tokenize_with_modules(path: str, source: str) -> tuple[list[Token], list[str]]:
    """Tokenise a Rust file, flagging test code.

    Also returns the paths of the files its `#[cfg(test)] mod NAME;`
    declarations load (NAME.rs or NAME/mod.rs beside a `mod.rs`, `lib.rs`
    or `main.rs`, else under a directory named after the file).
    """
    tokens = Lexer(source).run()
    parts = PurePosixPath(path).parts
    if "tests" in parts or "benches" in parts:
        for token in tokens:
            token.test = True
        return tokens, []
    file = PurePosixPath(path)
    owner = file.parent
    if file.name not in {"mod.rs", "lib.rs", "main.rs"}:
        owner /= file.stem
    paths = [
        (owner / candidate).as_posix()
        for name in mark_test_items(tokens)
        for candidate in (f"{name}.rs", f"{name}/mod.rs")
    ]
    return tokens, paths


def is_sql(text: str) -> bool:
    """Return whether a string literal looks like SQL."""
    return bool(SQL_KEYWORDS.search(text) or SQL_LOWER_START.match(text.lstrip()))


def normalise(text: str) -> str:
    """Collapse whitespace runs to one space and trim."""
    return " ".join(text.split())


def string_consts(tokens: Sequence[Token]) -> dict[str, set[str]]:
    """Map `const`/`static` NAME to the string literals assigned to it."""
    found: dict[str, set[str]] = {}
    for index, token in enumerate(tokens):
        if token.text not in {"const", "static"} or index + 1 >= len(tokens):
            continue
        name = tokens[index + 1]
        cursor = index + 2
        while cursor < len(tokens) and tokens[cursor].text not in {"=", ";", "{"}:
            cursor += 1
        if (
            name.kind == "ident"
            and cursor + 2 < len(tokens)
            and tokens[cursor].text == "="
            and tokens[cursor + 1].kind == "str"
            and tokens[cursor + 2].text == ";"
        ):
            found.setdefault(name.text, set()).add(tokens[cursor + 1].text)
    return found


def first_argument(tokens: Sequence[Token], open_paren: int) -> list[Token]:
    """Return the tokens of the first argument after `(` at `open_paren`."""
    close = matching(tokens, open_paren)
    depth = 0
    argument: list[Token] = []
    for token in tokens[open_paren + 1 : close]:
        if token.kind == "punct" and token.text in "([{":
            depth += 1
        elif token.kind == "punct" and token.text in ")]}":
            depth -= 1
        elif token.text == "," and depth == 0:
            break
        argument.append(token)
    return argument


def describe(argument: Sequence[Token], consts: dict[str, set[str]]) -> str:
    """Render a metric-name argument: its literal, constant's value, or text."""
    while argument and argument[0].text == "&":
        argument = argument[1:]
    if len(argument) == 1 and argument[0].kind == "str":
        return argument[0].text
    path = [token.text for token in argument if token.text != ":"]
    if (
        argument
        and all(token.kind == "ident" or token.text == ":" for token in argument)
        and path[-1] in consts
    ):
        return "|".join(sorted(consts[path[-1]]))
    return (
        "<"
        + "".join(
            f'"{token.text}"' if token.kind == "str" else token.text
            for token in argument
        )
        + ">"
    )


def metric_names(tokens: Sequence[Token], consts: dict[str, set[str]]) -> Iterator[str]:
    """Yield the metric names registered by non-test code."""
    for index, token in enumerate(tokens[:-2]):
        if token.test or token.kind != "ident":
            continue
        after = [tokens[index + 1].text, tokens[index + 2].text]
        if token.text in METRIC_MACROS and after == ["!", "("]:
            argument = first_argument(tokens, index + 2)
            if any(arg.text == "metric_name" for arg in argument):
                continue  # recorded at the metric_name call itself
            yield describe(argument, consts)
        elif token.text in METRIC_FUNCTIONS and after[0] == "(":
            yield METRIC_PREFIX + describe(first_argument(tokens, index + 1), consts)


def included_sql(
    tree: Tree, path: str, tokens: Sequence[Token]
) -> Iterator[tuple[bool, str]]:
    """Yield (test, text) for each `.sql` file pulled in with `include_str!`."""
    for index, token in enumerate(tokens[:-3]):
        if (
            token.text == "include_str"
            and tokens[index + 1].text == "!"
            and tokens[index + 2].text == "("
            and tokens[index + 3].kind == "str"
            and tokens[index + 3].text.endswith(".sql")
        ):
            target = PurePosixPath(path).parent / tokens[index + 3].text
            resolved = PurePosixPath(*_resolve(target.parts))
            text = tree.read(resolved.as_posix())
            body = text if text is not None else f"<missing {resolved}>"
            yield token.test, body


def _resolve(parts: Sequence[str]) -> list[str]:
    """Resolve `.` and `..` in a relative path's parts."""
    out: list[str] = []
    for part in parts:
        if part == "..":
            if out:
                out.pop()
        elif part != ".":
            out.append(part)
    return out


@dataclass
class Surface:
    """What a tree exposes: metric names and SQL text."""

    metrics: set[str] = field(default_factory=set)
    sql: Counter[str] = field(default_factory=Counter)
    test_sql: Counter[str] = field(default_factory=Counter)

    def sections(self) -> dict[str, list[str]]:
        """Return each section's lines, sorted."""
        return {
            "metrics": sorted(self.metrics),
            "sql": [f"{n}\t{text}" for text, n in sorted(self.sql.items())],
            "test-sql": [f"{n}\t{text}" for text, n in sorted(self.test_sql.items())],
        }


def surface(tree: Tree) -> Surface:
    """Collect a tree's surface from its `.rs` files."""
    parsed: dict[str, list[Token]] = {}
    test_files: set[str] = set()
    for path in tree.files():
        if path.endswith(".rs") and (text := tree.read(path)) is not None:
            parsed[path], children = tokenize_with_modules(path, text)
            test_files.update(children)
    # Files loaded by `#[cfg(test)] mod NAME;`, and any modules below them.
    for path, tokens in parsed.items():
        if any(
            path == f or path.startswith(f.removesuffix(".rs") + "/")
            for f in test_files
        ):
            for token in tokens:
                token.test = True
    consts: dict[str, set[str]] = {}
    for tokens in parsed.values():
        for name, values in string_consts([t for t in tokens if not t.test]).items():
            consts.setdefault(name, set()).update(values)
    result = Surface()
    for path, tokens in parsed.items():
        result.metrics.update(metric_names(tokens, consts))
        literals = [
            (t.test, t.text) for t in tokens if t.kind == "str" and is_sql(t.text)
        ]
        for test, text in [*literals, *included_sql(tree, path, tokens)]:
            (result.test_sql if test else result.sql)[normalise(text)] += 1
    return result


def open_tree(spec: str, repo: Path) -> Tree:
    """Return the tree a command-line argument names."""
    if spec.startswith("dir:"):
        return Directory(Path(spec.removeprefix("dir:")))
    git(repo, "rev-parse", "--verify", "--quiet", f"{spec}^{{commit}}")
    return GitRev(repo, spec)


def render(found: Surface) -> str:
    """Return a surface as text, one `## section` block each."""
    blocks = [
        f"## {name} ({len(lines)})\n" + "".join(f"{line}\n" for line in lines)
        for name, lines in found.sections().items()
    ]
    return "".join(blocks)


def diff(base: Surface, head: Surface, *, ignore_test_sql: bool) -> list[str]:
    """Return unified-diff lines for every section that changed."""
    out: list[str] = []
    head_sections = head.sections()
    for name, base_lines in base.sections().items():
        if name == "test-sql" and ignore_test_sql:
            continue
        out.extend(
            difflib.unified_diff(
                base_lines,
                head_sections[name],
                fromfile=f"base {name}",
                tofile=f"head {name}",
                lineterm="",
            )
        )
    return out


def main(argv: Sequence[str] | None = None) -> int:
    """Run `diff` or `dump`; see the module docstring for exit statuses."""
    parser = argparse.ArgumentParser(
        description="Compare the api's metric names and SQL text between two trees."
    )
    commands = parser.add_subparsers(dest="command", required=True)
    diff_parser = commands.add_parser("diff", help="compare BASE with HEAD")
    diff_parser.add_argument("base", help="git revision, or dir:PATH")
    diff_parser.add_argument("head", nargs="?", help="default: the work tree")
    diff_parser.add_argument(
        "--ignore-test-sql",
        action="store_true",
        help="do not compare SQL in test code",
    )
    dump_parser = commands.add_parser("dump", help="print one tree's surface")
    dump_parser.add_argument("tree", nargs="?", help="default: the work tree")
    args = parser.parse_args(argv)

    try:
        repo = Path(git(Path.cwd(), "rev-parse", "--show-toplevel").strip())
        work_tree = f"dir:{repo}"
        if args.command == "dump":
            sys.stdout.write(render(surface(open_tree(args.tree or work_tree, repo))))
            return 0
        base = surface(open_tree(args.base, repo))
        head = surface(open_tree(args.head or work_tree, repo))
    except GitError as err:
        sys.stderr.write(f"git {err} failed\n")
        return 2
    changes = diff(base, head, ignore_test_sql=args.ignore_test_sql)
    for line in changes:
        sys.stdout.write(f"{line}\n")
    counts = ", ".join(
        f"{name} {len(lines)}" for name, lines in head.sections().items()
    )
    if changes:
        sys.stdout.write(f"surface changed ({counts})\n")
        return 1
    sys.stdout.write(f"surface unchanged ({counts})\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
