from __future__ import annotations

import unittest
from pathlib import Path


INVENTORY = (
    Path(__file__).resolve().parents[3]
    / "docs"
    / "drivers"
    / "driver-reuse-and-license-matrix.md"
)


class DriverInventoryTests(unittest.TestCase):
    def test_inventory_has_license_and_hardware_status_for_each_component(self) -> None:
        text = INVENTORY.read_text(encoding="utf-8")
        self.assertIn("| 组件/适配层 | 当前用途 | 来源与许可证证据 | 可复用范围 | 状态与下一步 |", text)
        rows = [line for line in text.splitlines() if line.startswith("|")]
        self.assertGreaterEqual(len(rows), 8)
        data_rows = rows[2:]
        for row in data_rows:
            self.assertEqual(row.count("|"), 6, row)
            self.assertRegex(row, r"许可证|MIT|0BSD|Apache|自有代码")
            self.assertIn("UNVERIFIED_ON_HARDWARE", row)

    def test_inventory_is_not_an_implicit_hardware_claim(self) -> None:
        text = INVENTORY.read_text(encoding="utf-8")
        self.assertIn("不能因为 API 相似就把 QEMU 驱动标记为真实板驱动", text)


if __name__ == "__main__":
    unittest.main()
