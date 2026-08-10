from __future__ import annotations

import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
INVENTORY = ROOT / "docs/tasks/third-party-components.json"


class ThirdPartyInventoryTests(unittest.TestCase):
    def test_inventory_has_unique_pinned_entries_and_evidence(self) -> None:
        entries = json.loads(INVENTORY.read_text(encoding="utf-8"))
        self.assertGreaterEqual(len(entries), 5)
        names = [entry["name"] for entry in entries]
        self.assertEqual(len(names), len(set(names)))
        for entry in entries:
            for field in ("name", "version", "source", "license", "license_evidence", "used_for"):
                self.assertTrue(entry.get(field), f"missing {field}: {entry}")
            evidence = entry["license_evidence"]
            for token in ("os/vendor/",):
                if evidence.startswith(token):
                    path = evidence.split(token, 1)[1].split(" ", 1)[0]
                    self.assertTrue((ROOT / "os/vendor" / path).is_file(), evidence)

    def test_unknown_license_is_explicitly_blocking(self) -> None:
        entries = json.loads(INVENTORY.read_text(encoding="utf-8"))
        unknown = [entry for entry in entries if entry["license"] == "UNKNOWN"]
        self.assertEqual([entry["name"] for entry in unknown], ["another_ext4"])
        self.assertTrue(unknown[0]["license_evidence"].startswith("MISSING:"))


if __name__ == "__main__":
    unittest.main()
