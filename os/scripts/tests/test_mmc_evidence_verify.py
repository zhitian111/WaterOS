from __future__ import annotations

import copy
import json
import subprocess
import sys
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from mmc_evidence_verify import (EvidenceVerificationError, verify_evidence_file,
                                 verify_manifest)
from remote_debug_client import write_mmc_evidence
from scripts.tests.test_remote_debug_client import MMC_RESPONSE


class MmcEvidenceVerifyTests(unittest.TestCase):
    def _record(self, directory: Path, board: str, name: str, second: int = 0) -> Path:
        path = directory / name
        write_mmc_evidence(path, board, MMC_RESPONSE,
                           captured_at=datetime(2026, 8, 10, 0, 0, second,
                                                tzinfo=timezone.utc))
        return path

    def test_verifies_raw_hash_and_reconstructed_index(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = self._record(Path(temporary), "board-a", "a.json")
            verified = verify_evidence_file(path)
            self.assertEqual(verified.board_id, "board-a")
            self.assertEqual(len(verified.response_sha256), 64)

    def test_rejects_response_hash_index_and_validation_tampering(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            original = self._record(root, "board-a", "original.json")
            base = json.loads(original.read_text(encoding="utf-8"))
            variants = []
            response_changed = copy.deepcopy(base)
            response_changed["response"] = response_changed["response"].replace("csts=0x0", "csts=0x1")
            variants.append(response_changed)
            index_changed = copy.deepcopy(base)
            index_changed["parsed"]["controller"]["csts"] = 1
            variants.append(index_changed)
            validation_changed = copy.deepcopy(base)
            validation_changed["hardware_validation"] = "verified"
            variants.append(validation_changed)
            schema_changed = copy.deepcopy(base)
            schema_changed["schema"] = "future"
            variants.append(schema_changed)
            for index, record in enumerate(variants):
                path = root / f"tampered-{index}.json"
                path.write_text(json.dumps(record), encoding="utf-8")
                with self.subTest(index=index):
                    with self.assertRaises(EvidenceVerificationError):
                        verify_evidence_file(path)

    def test_manifest_reports_missing_and_accepts_complete_matrix(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            expected = [
                {"board_id": board, "scenarios": ["cold-no-card", "cold-card", "warm-card"]}
                for board in ("board-a", "board-b")
            ]
            entries = []
            for board in ("board-a", "board-b"):
                for scenario in ("cold-no-card", "cold-card", "warm-card"):
                    name = f"{board}-{scenario}.json"
                    self._record(root, board, name)
                    entries.append({"board_id": board, "scenario": scenario, "path": name})
            manifest_path = root / "manifest.json"
            manifest = {
                "schema": "wateros-ls2k-mmc-manifest-v1",
                "expected": expected,
                "evidence": entries[:-1],
            }
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            incomplete = verify_manifest(manifest_path)
            self.assertFalse(incomplete.complete)
            self.assertEqual(incomplete.verified, 5)
            self.assertEqual(incomplete.missing, ("board-b/warm-card",))
            manifest["evidence"] = entries
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            complete = verify_manifest(manifest_path)
            self.assertTrue(complete.complete)
            self.assertEqual(complete.expected, 6)

    def test_manifest_rejects_duplicate_mismatch_and_escaping_path(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self._record(root, "board-a", "a.json")
            base = {
                "schema": "wateros-ls2k-mmc-manifest-v1",
                "expected": [{"board_id": "board-a", "scenarios": ["cold"]}],
                "evidence": [{"board_id": "board-a", "scenario": "cold", "path": "a.json"}],
            }
            variants = []
            duplicate = copy.deepcopy(base)
            duplicate["evidence"].append(copy.deepcopy(duplicate["evidence"][0]))
            variants.append(duplicate)
            mismatch = copy.deepcopy(base)
            mismatch["evidence"][0]["board_id"] = "board-b"
            variants.append(mismatch)
            escaping = copy.deepcopy(base)
            escaping["evidence"][0]["path"] = "../outside.json"
            variants.append(escaping)
            for index, manifest in enumerate(variants):
                path = root / f"manifest-{index}.json"
                path.write_text(json.dumps(manifest), encoding="utf-8")
                with self.subTest(index=index):
                    with self.assertRaises(EvidenceVerificationError):
                        verify_manifest(path)

    def test_cli_reports_valid_evidence_and_incomplete_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            evidence = self._record(root, "board-a", "a.json")
            valid = subprocess.run(
                [sys.executable, str(SCRIPTS / "mmc_evidence_verify.py"),
                 "--evidence", str(evidence)],
                check=False, capture_output=True, text=True,
            )
            self.assertEqual(valid.returncode, 0, valid.stderr)
            self.assertTrue(json.loads(valid.stdout)["valid"])

            manifest = root / "manifest.json"
            manifest.write_text(json.dumps({
                "schema": "wateros-ls2k-mmc-manifest-v1",
                "expected": [{"board_id": "board-a", "scenarios": ["cold", "warm"]}],
                "evidence": [{"board_id": "board-a", "scenario": "cold", "path": "a.json"}],
            }), encoding="utf-8")
            incomplete = subprocess.run(
                [sys.executable, str(SCRIPTS / "mmc_evidence_verify.py"),
                 "--manifest", str(manifest)],
                check=False, capture_output=True, text=True,
            )
            self.assertEqual(incomplete.returncode, 1)
            summary = json.loads(incomplete.stdout)
            self.assertFalse(summary["complete"])
            self.assertEqual(summary["missing"], ["board-a/warm"])

    def test_v2_requires_distinct_samples_and_enforces_scenario_fields(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            first = self._record(root, "board-a", "first.json", 1)
            second = self._record(root, "board-a", "second.json", 2)
            manifest_path = root / "manifest-v2.json"
            expected = [{
                "board_id": "board-a",
                "scenario": "cold-card",
                "minimum_samples": 2,
                "assert_fields": {
                    "card": "non-removable",
                    "controller": "ok",
                    "trace": "none",
                },
            }]
            entries = [
                {"board_id": "board-a", "scenario": "cold-card", "path": first.name},
                {"board_id": "board-a", "scenario": "cold-card", "path": second.name},
            ]
            manifest = {
                "schema": "wateros-ls2k-mmc-manifest-v2",
                "expected": expected,
                "evidence": entries[:1],
            }
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            incomplete = verify_manifest(manifest_path)
            self.assertEqual(incomplete.missing, ("board-a/cold-card#2",))
            manifest["evidence"] = entries
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            self.assertTrue(verify_manifest(manifest_path).complete)

            manifest["evidence"] = [entries[0], entries[0]]
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaisesRegex(EvidenceVerificationError, "path more than once"):
                verify_manifest(manifest_path)
            manifest["evidence"] = entries
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")

            duplicate_time = json.loads(second.read_text(encoding="utf-8"))
            duplicate_time["captured_at"] = json.loads(first.read_text(encoding="utf-8"))["captured_at"]
            second.write_text(json.dumps(duplicate_time), encoding="utf-8")
            with self.assertRaisesRegex(EvidenceVerificationError, "distinct captured_at"):
                verify_manifest(manifest_path)

            expected[0]["assert_fields"]["present"] = "1"
            manifest["evidence"] = entries[:1]
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaisesRegex(EvidenceVerificationError, "assertion failed"):
                verify_manifest(manifest_path)


if __name__ == "__main__":
    unittest.main()
