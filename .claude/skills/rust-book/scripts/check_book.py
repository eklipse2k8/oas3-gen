#!/usr/bin/env python3
"""Validate the oas3-gen mdBook in `book/`.

Builds the book into a temporary directory with `mdbook build`, then scans every
page under `book/src/`.

Errors (exit status 1):
  - the build fails
  - a local link points to a missing file or a missing anchor
  - a code fence has no language label
  - a line contains an emoji

Warnings (review by hand):
  - dismissive or promotional words such as "simply" or "ergonomic"
  - `//` comments inside Rust code blocks
  - a space between inline code and the punctuation after it
  - a docs.rs link whose version differs from the one in `Cargo.lock`

Usage:
  python3 -I .claude/skills/rust-book/scripts/check_book.py [PAGE.md ...]

Page arguments limit warnings to those pages. Errors always cover the whole
book, because moving or renaming a section can break links on other pages.
"""

from __future__ import annotations

import html
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
BOOK = REPO / "book"
SRC = BOOK / "src"

FENCE_OPEN = re.compile(r"^\s*(`{3,}|~{3,})\s*([^`\s]*)")
INLINE_CODE = re.compile(r"(`+)(.+?)\1")
LINK_TARGET = re.compile(r"\]\(\s*<?([^)\s>]+)>?(?:\s+\"[^\"]*\")?\s*\)")
LINK_DEFINITION = re.compile(r"^\s*\[[^\]]+\]:\s*<?([^\s>]+)>?")
EXTERNAL = re.compile(r"^[a-z][a-z0-9+.-]*:", re.IGNORECASE)
HTML_ID = re.compile(r"\bid=\"([^\"]+)\"")
EMOJI_RANGES = ((0x1F000, 0x1FAFF), (0x2600, 0x27BF), (0x2B00, 0x2BFF), (0xFE0F, 0xFE0F))
EMOJI = re.compile("[" + "".join(f"{chr(low)}-{chr(high)}" for low, high in EMOJI_RANGES) + "]")
FLAGGED_WORDS = re.compile(
    r"\b(obviously|simply|just|ergonomic\w*|comprehensive|seamless\w*|powerful"
    r"|shines?|the developer|the consumer)\b",
    re.IGNORECASE,
)
RUST_COMMENT = re.compile(r"(^|\s)//(?![/!])")
SPACE_BEFORE_PUNCTUATION = re.compile(r"` [.,;:!?)]")
DOCS_RS = re.compile(r"https://docs\.rs/([A-Za-z0-9_-]+)/(\d[^/\s)]*)/")


class Report:
    """Collects findings and prints them grouped by severity."""

    def __init__(self) -> None:
        self.errors: list[str] = []
        self.warnings: list[str] = []
        self.notes: list[str] = []

    def print(self) -> int:
        for message in self.errors:
            print(f"error: {message}")
        for message in self.warnings:
            print(f"warning: {message}")
        for message in self.notes:
            print(f"note: {message}")
        print(f"{len(self.errors)} error(s), {len(self.warnings)} warning(s)")
        return 1 if self.errors else 0


def build(destination: Path, report: Report) -> bool:
    """Builds the book into `destination` and returns whether HTML is available."""
    mdbook = shutil.which("mdbook")
    if mdbook is None:
        report.notes.append("mdbook not found on PATH; skipped the build and anchor checks")
        return False
    result = subprocess.run(
        [mdbook, "build", str(BOOK), "--dest-dir", str(destination)],
        capture_output=True,
        text=True,
        check=False,
    )
    for line in result.stderr.splitlines():
        if "WARN" in line or "ERROR" in line:
            report.warnings.append(f"mdbook: {line.strip()}")
    if result.returncode != 0:
        report.errors.append(f"mdbook build failed:\n{result.stderr.strip()}")
        return False
    return True


class AnchorIndex:
    """Reads the element ids of each built page on first use."""

    def __init__(self, html_root: Path | None) -> None:
        self.html_root = html_root
        self.cache: dict[Path, set[str] | None] = {}

    def ids(self, page: Path) -> set[str] | None:
        if page not in self.cache:
            built = self.html_root / page.relative_to(SRC).with_suffix(".html")
            self.cache[page] = (
                {html.unescape(value) for value in HTML_ID.findall(built.read_text(encoding="utf-8"))}
                if built.exists()
                else None
            )
        return self.cache[page]


def locked_versions() -> dict[str, set[str]]:
    """Maps each crate in `Cargo.lock`, with hyphens as underscores, to its locked versions."""
    versions: dict[str, set[str]] = {}
    lock = REPO / "Cargo.lock"
    if not lock.exists():
        return versions
    name = None
    for line in lock.read_text(encoding="utf-8").splitlines():
        if line.startswith("name = "):
            name = line.split('"')[1].replace("-", "_")
        elif line.startswith("version = ") and name is not None:
            versions.setdefault(name, set()).add(line.split('"')[1])
            name = None
    return versions


def display(path: Path) -> str:
    return str(path.relative_to(REPO)) if path.is_relative_to(REPO) else str(path)


def check_link(target: str, page: Path, where: str, anchors: AnchorIndex, report: Report) -> None:
    if EXTERNAL.match(target):
        return
    path, _, anchor = target.partition("#")
    destination = (page.parent / path).resolve() if path else page
    if not destination.exists():
        report.errors.append(f"{where}: link target `{target}` does not exist")
        return
    if not anchor or anchors.html_root is None:
        return
    if destination.suffix != ".md" or not destination.is_relative_to(SRC):
        return
    ids = anchors.ids(destination)
    if ids is None:
        report.errors.append(f"{where}: `{display(destination)}` is not listed in SUMMARY.md")
    elif anchor not in ids:
        report.errors.append(f"{where}: anchor `#{anchor}` not found in {display(destination)}")


def check_page(
    page: Path, anchors: AnchorIndex, locked: dict[str, set[str]], warn: bool, report: Report
) -> None:
    fence: str | None = None
    language = ""
    for number, line in enumerate(page.read_text(encoding="utf-8").splitlines(), 1):
        where = f"{display(page)}:{number}"
        if EMOJI.search(line):
            report.errors.append(f"{where}: emoji")
        if fence is not None:
            if line.strip().startswith(fence) and set(line.strip()) == {fence[0]}:
                fence = None
            elif warn and language == "rust" and RUST_COMMENT.search(line):
                report.warnings.append(f"{where}: comment inside a Rust code block")
            continue
        opening = FENCE_OPEN.match(line)
        if opening:
            fence, info = opening.groups()
            language = info.split(",")[0]
            if not language:
                report.errors.append(f"{where}: code fence has no language label")
            continue
        prose = INLINE_CODE.sub("", line)
        for target in LINK_TARGET.findall(prose):
            check_link(target, page, where, anchors, report)
        definition = LINK_DEFINITION.match(prose)
        if definition:
            check_link(definition.group(1), page, where, anchors, report)
        if not warn:
            continue
        for word in FLAGGED_WORDS.findall(prose):
            report.warnings.append(f"{where}: flagged wording \"{word}\"")
        if SPACE_BEFORE_PUNCTUATION.search(line):
            report.warnings.append(f"{where}: space between inline code and punctuation")
        for crate, version in DOCS_RS.findall(prose):
            expected = locked.get(crate.replace("-", "_"))
            if expected and version not in expected:
                report.warnings.append(
                    f"{where}: docs.rs link uses {crate} {version}; "
                    f"Cargo.lock has {', '.join(sorted(expected))}"
                )


def resolve_page(argument: str) -> Path | None:
    for candidate in (Path(argument), SRC / argument):
        if candidate.is_file():
            return candidate.resolve()
    return None


def main(arguments: list[str]) -> int:
    report = Report()
    focus: set[Path] = set()
    for argument in arguments:
        page = resolve_page(argument)
        if page is None:
            print(f"error: no page named `{argument}`", file=sys.stderr)
            return 2
        focus.add(page)
    locked = locked_versions()
    with tempfile.TemporaryDirectory() as temporary:
        html_root = Path(temporary)
        anchors = AnchorIndex(html_root if build(html_root, report) else None)
        for page in sorted(SRC.rglob("*.md")):
            check_page(page.resolve(), anchors, locked, not focus or page.resolve() in focus, report)
    return report.print()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
