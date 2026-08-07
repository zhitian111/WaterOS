from __future__ import annotations

import argparse
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

import wateros_debug
from wateros_debug import DebugToolError, resolve_connection


class DebugSessionTests(unittest.TestCase):
    def arguments(self) -> argparse.Namespace:
        return argparse.Namespace(
            arch=None, elf=None, host=None, port=None, serial_log=None
        )

    def test_active_session_restores_connection(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            elf = root / "kernel-rv-pre-gdb"
            elf.write_bytes(b"elf")
            session = root / "session.json"
            session.write_text(
                json.dumps(
                    {
                        "version": wateros_debug.SESSION_VERSION,
                        "pid": 42,
                        "arch": "rv",
                        "profile": "pre",
                        "elf": str(elf),
                        "build_id": "build",
                        "host": "127.0.0.1",
                        "port": 1235,
                        "serial_log": str(root / "serial.log"),
                    }
                )
            )
            with (
                patch.object(wateros_debug, "ACTIVE_SESSION", session),
                patch.object(wateros_debug, "process_alive", return_value=True),
                patch.object(wateros_debug, "local_build_id", return_value="build"),
            ):
                connection = resolve_connection(self.arguments())
            self.assertEqual(connection.arch, "rv")
            self.assertEqual(connection.port, 1235)
            self.assertEqual(connection.elf, elf)

    def test_stale_session_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            session = Path(directory) / "session.json"
            session.write_text(
                json.dumps(
                    {
                        "version": wateros_debug.SESSION_VERSION,
                        "pid": 42,
                    }
                )
            )
            with (
                patch.object(wateros_debug, "ACTIVE_SESSION", session),
                patch.object(wateros_debug, "process_alive", return_value=False),
                self.assertRaises(DebugToolError),
            ):
                resolve_connection(self.arguments())


if __name__ == "__main__":
    unittest.main()
