import unittest
from pathlib import Path

from qemu_input_monitor_smoke import build_command, build_qmp_command, response_is_success


class QemuInputMonitorSmokeTests(unittest.TestCase):
    def test_command_uses_hmp_unix_socket(self):
        command = build_command("qemu-system-test", Path("/tmp/monitor.sock"))
        self.assertIn("-monitor", command)
        self.assertIn("unix:/tmp/monitor.sock,server=on,wait=off", command)
        self.assertIn("sendkey", "sendkey a")

    def test_response_parser_rejects_monitor_errors(self):
        self.assertTrue(response_is_success(b"(qemu)"))
        self.assertFalse(response_is_success(b"Error: unknown command"))

    def test_qmp_command_has_virtio_input_devices(self):
        command = build_qmp_command("qemu-system-test", Path("/tmp/qmp.sock"))
        self.assertIn("virtio-keyboard-pci", command)
        self.assertIn("virtio-tablet-pci", command)
        self.assertIn("unix:/tmp/qmp.sock,server=on,wait=off", command)


if __name__ == "__main__":
    unittest.main()
