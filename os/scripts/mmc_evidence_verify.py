#!/usr/bin/env python3
"""Offline integrity and coverage verifier for LS2K1000 MMC evidence records."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from dataclasses import asdict, dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

from remote_debug_client import MonitorProtocolError, parse_mmc_evidence

MAX_EVIDENCE_FILE = 64 * 1024
MAX_MANIFEST_FILE = 64 * 1024
NAME_PATTERN = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}\Z")


class EvidenceVerificationError(RuntimeError):
    pass


@dataclass(frozen=True)
class VerifiedEvidence:
    path: str
    board_id: str
    captured_at: str
    response_sha256: str


@dataclass(frozen=True)
class ManifestSummary:
    expected: int
    verified: int
    missing: tuple[str, ...]

    @property
    def complete(self) -> bool:
        return not self.missing and self.expected == self.verified


def _read_json_object(path: Path, limit: int) -> dict[str, Any]:
    try:
        with path.open("rb") as source:
            raw = source.read(limit + 1)
        if len(raw) > limit:
            raise EvidenceVerificationError(f"{path} exceeds {limit} byte limit")
        value = json.loads(raw.decode("utf-8"))
    except EvidenceVerificationError:
        raise
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise EvidenceVerificationError(f"cannot read JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise EvidenceVerificationError(f"{path} must contain one JSON object")
    return value


def _valid_name(value: object) -> bool:
    return isinstance(value, str) and NAME_PATTERN.fullmatch(value) is not None


def verify_evidence_file(path: Path) -> VerifiedEvidence:
    """Verify one immutable record without trusting its stored parsed index."""
    record = _read_json_object(path, MAX_EVIDENCE_FILE)
    required = {
        "schema", "board_id", "captured_at", "command", "response", "response_sha256",
        "parsed", "hardware_validation",
    }
    if set(record) != required:
        raise EvidenceVerificationError("evidence record has unexpected or missing top-level fields")
    if record["schema"] != "wateros-ls2k-mmc-evidence-v1":
        raise EvidenceVerificationError("unsupported MMC evidence schema")
    if not _valid_name(record["board_id"]):
        raise EvidenceVerificationError("invalid evidence board_id")
    if record["command"] != "ls2k-mmc":
        raise EvidenceVerificationError("unexpected evidence command")
    if record["hardware_validation"] != "unverified-observation":
        raise EvidenceVerificationError("evidence overstates or changes hardware validation")
    captured_at = record["captured_at"]
    if (not isinstance(captured_at, str) or "T" not in captured_at or
            not captured_at.endswith("Z")):
        raise EvidenceVerificationError("captured_at must be UTC with a Z suffix")
    try:
        datetime.fromisoformat(captured_at[:-1] + "+00:00")
    except ValueError as error:
        raise EvidenceVerificationError("captured_at is not valid ISO-8601") from error
    response = record["response"]
    digest = record["response_sha256"]
    if not isinstance(response, str) or not isinstance(digest, str):
        raise EvidenceVerificationError("response and response_sha256 must be strings")
    actual_digest = hashlib.sha256(response.encode("utf-8")).hexdigest()
    if digest != actual_digest:
        raise EvidenceVerificationError("MMC evidence response SHA-256 mismatch")
    try:
        parsed = parse_mmc_evidence(response)
    except MonitorProtocolError as error:
        raise EvidenceVerificationError(f"stored MMC response is invalid: {error}") from error
    expected_index = {
        "fields": parsed.fields,
        "gates": parsed.gates,
        "controller": parsed.controller,
    }
    if record["parsed"] != expected_index:
        raise EvidenceVerificationError("stored parsed index does not match the raw response")
    return VerifiedEvidence(path=str(path),
                            board_id=record["board_id"],
                            captured_at=captured_at,
                            response_sha256=digest)


def _manifest_path(root: Path, value: object) -> Path:
    if not isinstance(value, str) or not value or Path(value).is_absolute():
        raise EvidenceVerificationError("manifest evidence path must be relative")
    candidate = (root / value).resolve()
    try:
        candidate.relative_to(root.resolve())
    except ValueError as error:
        raise EvidenceVerificationError("manifest evidence path escapes its directory") from error
    return candidate


def verify_manifest(path: Path) -> ManifestSummary:
    """Verify all referenced records and report missing board/scenario pairs."""
    manifest = _read_json_object(path, MAX_MANIFEST_FILE)
    if set(manifest) != {"schema", "expected", "evidence"}:
        raise EvidenceVerificationError("manifest has unexpected or missing top-level fields")
    if manifest["schema"] != "wateros-ls2k-mmc-manifest-v1":
        raise EvidenceVerificationError("unsupported MMC evidence manifest schema")
    if not isinstance(manifest["expected"], list) or not isinstance(manifest["evidence"], list):
        raise EvidenceVerificationError("manifest expected and evidence must be arrays")

    expected: set[tuple[str, str]] = set()
    seen_boards: set[str] = set()
    for entry in manifest["expected"]:
        if not isinstance(entry, dict) or set(entry) != {"board_id", "scenarios"}:
            raise EvidenceVerificationError("invalid manifest expected entry")
        board_id = entry["board_id"]
        scenarios = entry["scenarios"]
        if not _valid_name(board_id) or board_id in seen_boards:
            raise EvidenceVerificationError("invalid or duplicate expected board_id")
        if not isinstance(scenarios, list) or not scenarios:
            raise EvidenceVerificationError("each expected board needs at least one scenario")
        seen_boards.add(board_id)
        for scenario in scenarios:
            if not _valid_name(scenario) or (board_id, scenario) in expected:
                raise EvidenceVerificationError("invalid or duplicate expected scenario")
            expected.add((board_id, scenario))
    if not expected:
        raise EvidenceVerificationError("manifest expected set must not be empty")

    verified: set[tuple[str, str]] = set()
    root = path.resolve().parent
    for entry in manifest["evidence"]:
        if not isinstance(entry, dict) or set(entry) != {"board_id", "scenario", "path"}:
            raise EvidenceVerificationError("invalid manifest evidence entry")
        board_id = entry["board_id"]
        scenario = entry["scenario"]
        key = (board_id, scenario)
        if not _valid_name(board_id) or not _valid_name(scenario) or key not in expected:
            raise EvidenceVerificationError("manifest evidence is not in the expected set")
        if key in verified:
            raise EvidenceVerificationError("duplicate evidence for board/scenario pair")
        record = verify_evidence_file(_manifest_path(root, entry["path"]))
        if record.board_id != board_id:
            raise EvidenceVerificationError("manifest board_id does not match evidence record")
        verified.add(key)
    missing = tuple(f"{board}/{scenario}" for board, scenario in sorted(expected - verified))
    return ManifestSummary(expected=len(expected), verified=len(verified), missing=missing)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--evidence", type=Path, help="verify one evidence JSON file")
    group.add_argument("--manifest", type=Path, help="verify a board/scenario manifest")
    args = parser.parse_args()
    try:
        if args.evidence is not None:
            result = asdict(verify_evidence_file(args.evidence))
            result["valid"] = True
        else:
            summary = verify_manifest(args.manifest)
            result = asdict(summary)
            result["complete"] = summary.complete
    except EvidenceVerificationError as error:
        print(f"MMC evidence verification failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True, separators=(",", ":")))
    return 0 if result.get("complete", True) else 1


if __name__ == "__main__":
    raise SystemExit(main())
