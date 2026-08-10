from __future__ import annotations

import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from input_event_smoke import encode_event, run_smoke  # noqa: E402


class InputEventSmokeTests(unittest.TestCase):
    def test_keyboard_mouse_and_dynamic_node_contract(self) -> None:
        run_smoke()

    def test_records_are_fixed_16_bytes(self) -> None:
        self.assertEqual(len(encode_event(1, 30, 1)), 16)
        self.assertEqual(len(encode_event(2, 0, -12)), 16)


if __name__ == "__main__":
    unittest.main()
