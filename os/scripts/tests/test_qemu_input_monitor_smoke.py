import unittest
from pathlib import Path

from qemu_input_monitor_smoke import build_command, response_is_success


class QemuInputMonitorSmokeTests(unittest.TestCase):
    def test_command_uses_hmp_unix_socket(self):
        command = build_command("qemu-system-test", Path("/tmp/monitor.sock"))
        self.assertIn("-monitor", command)
        self.assertIn("unix:/tmp/monitor.sock,server=on,wait=off", command)
        self.assertIn("sendkey", "sendkey a")

    def test_response_parser_rejects_monitor_errors(self):
        self.assertTrue(response_is_success(b"(qemu)"))
        self.assertFalse(response_is_success(b"Error: unknown command"))


if __name__ == "__main__":
    unittest.main()
