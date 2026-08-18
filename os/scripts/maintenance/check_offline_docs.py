#!/usr/bin/env python3
"""Validate the offline-development documentation without external packages.

The checker deliberately verifies objective structure only: required manuals,
crate-local README coverage, balanced fenced blocks, and local Markdown links.
The optional content audit applies conservative offline-maintenance heuristics;
it still does not claim that prose is semantically correct, so reviewers must
compare descriptions with source code.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path


OS_ROOT = Path(__file__).resolve().parents[2]
COMPONENTS = OS_ROOT / "components"
OFFLINE_DOCS = OS_ROOT / "docs" / "offline-development"

README_NAMES = ("README.md", "readme.md", "Readme.md")
REQUIRED_OFFLINE_DOCS = (
    "README.md",
    "adding-a-syscall.md",
    "architecture-and-call-chains.md",
    "boot-and-bringup.md",
    "component-change-checklists.md",
    "console-tty-klog-debug-gui.md",
    "data-structure-lifetimes.md",
    "debugging-and-regression.md",
    "device-storage-network-runtime.md",
    "feature-cookbook.md",
    "source-navigation-index.md",
    "testing-playbook.md",
)
REQUIRED_COMPONENTS = (
    "wateros-base",
    "wateros-cred",
    "wateros-debug",
    "wateros-driver",
    "wateros-fs",
    "wateros-gui",
    "wateros-ipc",
    "wateros-klog",
    "wateros-mm",
    "wateros-network",
    "wateros-platform",
    "wateros-runtime",
    "wateros-syscall",
    "wateros-task",
    "wateros-tty",
    "wateros-utils",
    "wateros-vfs",
)

# Markdown inline links. Images use the same target syntax and are checked too.
LINK_RE = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")
FENCE_RE = re.compile(r"^\s*(```|~~~)")

# A crate manual intended for offline repair should cover all of these ideas.
# Each tuple contains Chinese/English spellings accepted for one concept.  This
# is intentionally a conservative heuristic, not a prose style requirement.
CONTENT_CONCEPTS = (
    ("调用", "流程", "入口", "->", "→", "flow"),
    ("锁", "并发", "所有权", "生命周期", "lock", "lifetime", "smp"),
    ("错误", "失败", "限制", "风险", "error", "failure"),
    ("回归", "测试", "验证", "self_test", "cargo check", "make check"),
)
MIN_CRATE_README_NONSPACE_CHARS = 240


@dataclass(frozen=True)
class Issue:
    path: Path
    message: str


def display(path: Path) -> str:
    try:
        return str(path.relative_to(OS_ROOT))
    except ValueError:
        return str(path)


def local_readme(directory: Path) -> Path | None:
    return next((directory / name for name in README_NAMES if (directory / name).is_file()), None)


def markdown_files() -> list[Path]:
    paths: set[Path] = set()
    if COMPONENTS.is_dir():
        paths.update(COMPONENTS.rglob("README.md"))
        paths.update(COMPONENTS.rglob("readme.md"))
        paths.update(COMPONENTS.rglob("Readme.md"))
    if OFFLINE_DOCS.is_dir():
        paths.update(OFFLINE_DOCS.rglob("*.md"))
    return sorted(paths)


def link_target(raw: str) -> str:
    """Return the path part of a Markdown target, omitting an optional title."""
    value = raw.strip()
    if value.startswith("<") and ">" in value:
        return value[1 : value.index(">")]
    # Repository documentation does not use whitespace inside unescaped paths;
    # the remainder, if present, is a Markdown title.
    return value.split(maxsplit=1)[0]


def check_crate_content(path: Path, issues: list[Issue]) -> None:
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError):
        # check_markdown reports the detailed read failure later.
        return
    folded = text.casefold()
    nonspace = len(re.sub(r"\s+", "", text))
    if nonspace < MIN_CRATE_README_NONSPACE_CHARS:
        issues.append(
            Issue(
                path,
                "content audit: crate README is too short for offline maintenance "
                f"({nonspace} < {MIN_CRATE_README_NONSPACE_CHARS} non-space characters)",
            )
        )
    if not re.search(r"^#\s+\S", text, flags=re.MULTILINE):
        issues.append(Issue(path, "content audit: missing level-1 title"))
    for alternatives in CONTENT_CONCEPTS:
        if not any(keyword.casefold() in folded for keyword in alternatives):
            issues.append(
                Issue(
                    path,
                    "content audit: missing concept group " + "/".join(alternatives),
                )
            )


def check_required_layout(issues: list[Issue], content_audit: bool) -> tuple[int, int]:
    for name in REQUIRED_OFFLINE_DOCS:
        path = OFFLINE_DOCS / name
        if not path.is_file():
            issues.append(Issue(path, "required offline manual is missing"))

    for name in REQUIRED_COMPONENTS:
        directory = COMPONENTS / name
        if not directory.is_dir():
            issues.append(Issue(directory, "required component directory is missing"))
        elif local_readme(directory) is None:
            issues.append(Issue(directory, "required component overview README is missing"))

    crates = sorted(COMPONENTS.rglob("Cargo.toml")) if COMPONENTS.is_dir() else []
    documented = 0
    for manifest in crates:
        readme = local_readme(manifest.parent)
        if readme is None:
            issues.append(Issue(manifest.parent, "component crate has no local README"))
        else:
            documented += 1
            if content_audit:
                check_crate_content(readme, issues)
    return len(crates), documented


def check_markdown(path: Path, issues: list[Issue]) -> None:
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        issues.append(Issue(path, f"cannot read UTF-8 Markdown: {error}"))
        return

    fences = {"```": 0, "~~~": 0}
    for line in text.splitlines():
        match = FENCE_RE.match(line)
        if match:
            fences[match.group(1)] += 1
    for marker, count in fences.items():
        if count % 2:
            issues.append(Issue(path, f"unbalanced {marker} fenced blocks ({count} markers)"))

    for match in LINK_RE.finditer(text):
        target = link_target(match.group(1))
        if not target or target.startswith("#"):
            continue
        lower = target.lower()
        if "://" in target or lower.startswith(("mailto:", "data:", "app:")):
            continue
        target = target.split("#", 1)[0]
        if not target:
            continue
        resolved = (path.parent / target).resolve()
        if not resolved.exists():
            issues.append(Issue(path, f"broken local link: {target}"))


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--quiet",
        action="store_true",
        help="print only failures (the exit status still reports success/failure)",
    )
    parser.add_argument(
        "--content-audit",
        action="store_true",
        help=(
            "also require every crate README to cover flow, concurrency/lifetime, "
            "failure boundaries, and regression testing"
        ),
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    issues: list[Issue] = []
    crate_count, documented_count = check_required_layout(issues, args.content_audit)
    markdown = markdown_files()
    for path in markdown:
        check_markdown(path, issues)

    if issues:
        for issue in issues:
            print(f"DOC-ERROR {display(issue.path)}: {issue.message}", file=sys.stderr)
        print(
            f"offline docs FAILED: issues={len(issues)} crates={documented_count}/{crate_count} "
            f"markdown={len(markdown)}",
            file=sys.stderr,
        )
        return 1

    if not args.quiet:
        print(
            f"offline docs OK: crates={documented_count}/{crate_count} "
            f"markdown={len(markdown)} links/fences=valid"
            + (" content-audit=valid" if args.content_audit else "")
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
