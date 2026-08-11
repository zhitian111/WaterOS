#!/usr/bin/env python3
"""Render a syscall-profile TSV result as a compact Markdown report."""
from __future__ import annotations

import argparse
from collections import defaultdict
from pathlib import Path


def parse(path: Path) -> dict:
    profile: dict = {
        "header": "",
        "syscalls": [],
        "args": defaultdict(list),
        "values": defaultdict(list),
        "returns": {},
        "errors": defaultdict(list),
        "paths": [],
        "reuse": defaultdict(list),
        "path_values": [],
        "diagnostics": [],
    }
    with path.open(encoding="utf-8", errors="replace") as stream:
        for raw_line in stream:
            line = raw_line.rstrip("\n")
            if line.startswith("# syscall-profile"):
                profile["header"] = line.removeprefix("# ")
                continue
            if not line or line.startswith("#"):
                continue
            fields = line.split("\t")
            kind = fields[0]
            if kind == "S":
                profile["syscalls"].append(
                    {"nr": int(fields[1]), "name": fields[2],
                     "count": int(fields[3]),
                     "per_vcpu": [int(value) for value in fields[4:]]}
                )
            elif kind == "A":
                profile["args"][(int(fields[1]), int(fields[2]))].append(
                    (fields[3], int(fields[4]))
                )
            elif kind == "V":
                profile["values"][(int(fields[1]), int(fields[2]))].append(
                    (int(fields[3], 0), int(fields[4]))
                )
            elif kind == "R":
                profile["returns"][int(fields[1])] = (
                    int(fields[2]), int(fields[3])
                )
            elif kind == "E":
                profile["errors"][int(fields[1])].append(
                    (int(fields[2]), int(fields[3]))
                )
            elif kind == "P":
                profile["paths"].append(
                    {"nr": int(fields[1]), "arg": int(fields[2]),
                     "reads": int(fields[3]), "unique": int(fields[4]),
                     "repeats": int(fields[5]), "failures": int(fields[6]),
                     "truncated": int(fields[7])}
                )
            elif kind == "D":
                profile["reuse"][(int(fields[1]), int(fields[2]))].append(
                    (fields[3], int(fields[4]))
                )
            elif kind == "PV":
                profile["path_values"].append(
                    {"nr": int(fields[1]), "arg": int(fields[2]),
                     "count": int(fields[3]), "path": fields[4]}
                )
            elif kind == "X":
                profile["diagnostics"].append(fields[1:])
    profile["syscalls"].sort(key=lambda row: row["count"], reverse=True)
    profile["paths"].sort(key=lambda row: row["reads"], reverse=True)
    return profile


def syscall_names(profile: dict) -> dict[int, str]:
    return {row["nr"]: row["name"] for row in profile["syscalls"]}


def render(profile: dict, top: int) -> str:
    names = syscall_names(profile)
    total = sum(row["count"] for row in profile["syscalls"])
    lines = ["# Syscall profile report", "", f"`{profile['header']}`", ""]
    lines.extend([
        "## Top syscalls", "",
        "| syscall | nr | calls | share |", "|---|---:|---:|---:|",
    ])
    for row in profile["syscalls"][:top]:
        share = row["count"] / total * 100 if total else 0
        lines.append(
            f"| `{row['name']}` | {row['nr']} | {row['count']} | {share:.2f}% |"
        )

    lines.extend([
        "", "## Path reuse", "",
        "| syscall/arg | reads | unique | repeats | repeat rate | read failures |",
        "|---|---:|---:|---:|---:|---:|",
    ])
    for row in profile["paths"]:
        observed = row["unique"] + row["repeats"]
        rate = row["repeats"] / observed * 100 if observed else 0
        name = names.get(row["nr"], "unknown")
        lines.append(
            f"| `{name}`/{row['arg']} | {row['reads']} | {row['unique']} | "
            f"{row['repeats']} | {rate:.2f}% | {row['failures']} |"
        )

    interesting = [(63, 2, "read request"), (64, 2, "write request"),
                   (67, 2, "pread request"), (68, 2, "pwrite request"),
                   (222, 1, "mmap length"), (215, 1, "munmap length")]
    lines.extend(["", "## Size distributions", ""])
    for nr, arg, label in interesting:
        values = profile["args"].get((nr, arg), [])
        if not values:
            continue
        formatted = ", ".join(f"{bucket}={count}" for bucket, count in values)
        lines.append(f"- {label}: {formatted}")

    lines.extend(["", "## Exact enum/flag values", ""])
    value_groups = sorted(profile["values"].items(),
                          key=lambda item: sum(count for _, count in item[1]),
                          reverse=True)
    for (nr, arg), values in value_groups[:top]:
        values = sorted(values, key=lambda item: item[1], reverse=True)[:8]
        formatted = ", ".join(f"0x{value:x}={count}" for value, count in values)
        lines.append(f"- `{names.get(nr, 'unknown')}` arg{arg}: {formatted}")

    lines.extend([
        "", "## Top paths", "",
        "| syscall | calls | path |", "|---|---:|---|",
    ])
    for row in profile["path_values"][:top]:
        escaped = row["path"].replace("|", "\\|")
        lines.append(
            f"| `{names.get(row['nr'], 'unknown')}` | {row['count']} | `{escaped}` |"
        )
    if profile["diagnostics"]:
        lines.extend(["", "## Diagnostics", "", "```text"])
        lines.extend("\t".join(row) for row in profile["diagnostics"])
        lines.append("```")
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("result", type=Path)
    parser.add_argument("--top", type=int, default=20)
    args = parser.parse_args()
    print(render(parse(args.result), args.top), end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

