from __future__ import annotations

import hashlib
import json
import socket
import sys
import tempfile
import threading
import unittest
from datetime import datetime, timezone
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from remote_debug_client import (MonitorClient, MonitorProtocolError, connect_with_retry,
                                 parse_mmc_evidence, run_smoke, write_mmc_evidence)


MMC_RESPONSE = (
    "ls2k-mmc clock=ok ref_hz=100000000 pll_raw=0x2810000000 gmac_raw=0x800000 "
    "apb_raw=0x300000 apb_hz=250000000 vmmc=implicit-board-supply "
    "vqmmc=implicit-board-supply pinctrl=requires-driver pinmux=ok raw=0x100000 "
    "sdio=1 card_gpio=1 ready=1 card=non-removable "
    "gates=clock:observed-only,vmmc:unverified-hardware,vqmmc:unverified-hardware,"
    "pinctrl:satisfied,card:satisfied,irq:missing proof=0 can_activate=0 blockers=7 "
    "controller=ok carg=0x0 cctl=0x0 csts=0x0 dsts=0x0 int=0x0 idle=1 clean=1 "
    "int_known=0x0 int_unknown=0x0 trace=none assessment=unavailable\r\n"
)


class RemoteDebugClientTests(unittest.TestCase):
    def test_complete_smoke_session(self) -> None:
        client_socket, server_socket = socket.socketpair()

        def server() -> None:
            with server_socket:
                server_socket.sendall(
                    b"WaterOS development monitor\r\nType 'help' for commands.\r\nwos> "
                )
                stream = server_socket.makefile("rb")
                responses = {
                    b"ping\n": b"pong\r\nwos> ",
                    b"status\n": (
                        b"tick=7 online_cpus=0x1 heap_used=8 heap_free=9 "
                        b"heap_capacity=17\r\nwos> "
                    ),
                    b"version\n": b"WaterOS 0.1.0\r\nwos> ",
                    b"devfs\n": b"devfs generation=2 nodes=2 truncated=false paths=/dev/console,/dev/input/event0\r\nwos> ",
                    b"capabilities\n": b"ERR unsupported: capabilities requires loongson2k1000la\r\nwos> ",
                    b"ls2k-mmc\n": (
                        b"ERR unsupported: ls2k-mmc requires loongson2k1000la\r\nwos> "
                    ),
                    b"reboot\n": b"unknown command; type 'help'\r\nwos> ",
                    b"quit\n": b"bye\r\n",
                }
                for expected, response in responses.items():
                    self.assertEqual(stream.readline(), expected)
                    server_socket.sendall(response)

        thread = threading.Thread(target=server)
        thread.start()
        client = MonitorClient(client_socket)
        try:
            results = run_smoke(client, expect_input=True)
            self.assertEqual([result.command for result in results],
                             ["ping", "status", "version", "devfs", "capabilities",
                              "ls2k-mmc", "reboot", "quit"])
        finally:
            client.close()
            thread.join(timeout=2)
        self.assertFalse(thread.is_alive())

    def test_rejects_wrong_banner_and_multiline_command(self) -> None:
        client_socket, server_socket = socket.socketpair()
        with server_socket:
            server_socket.sendall(b"not WaterOS\r\nwos> ")
            client = MonitorClient(client_socket)
            with self.assertRaises(MonitorProtocolError):
                client.receive_banner()
            with self.assertRaises(ValueError):
                client.command("ping\nstatus")
            with self.assertRaisesRegex(ValueError, "128-byte"):
                client.command("x" * 129)
            client.close()

    def test_readiness_reconnects_when_forwarder_accepts_before_guest(self) -> None:
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        listener.listen(2)
        port = listener.getsockname()[1]

        def server() -> None:
            with listener:
                premature, _ = listener.accept()
                premature.close()
                ready, _ = listener.accept()
                with ready:
                    ready.sendall(
                        b"WaterOS development monitor\r\nType 'help' for commands.\r\nwos> "
                    )

        thread = threading.Thread(target=server)
        thread.start()
        client = connect_with_retry("127.0.0.1", port, 3)
        try:
            self.assertIn("WaterOS development monitor", client.receive_banner())
        finally:
            client.close()
            thread.join(timeout=2)
        self.assertFalse(thread.is_alive())

    def test_parses_controller_evidence_and_retains_extensions(self) -> None:
        response = MMC_RESPONSE.replace(" trace=none", " future_field=alpha trace=none")
        evidence = parse_mmc_evidence(response)
        self.assertEqual(evidence.controller["state"], "ok")
        self.assertEqual(evidence.controller["csts"], 0)
        self.assertEqual(evidence.gates["irq"], "missing")
        self.assertEqual(evidence.fields["future_field"], "alpha")

        failed = MMC_RESPONSE.replace(
            "controller=ok carg=0x0 cctl=0x0 csts=0x0 dsts=0x0 int=0x0 idle=1 clean=1 "
            "int_known=0x0 int_unknown=0x0",
            "controller=error:read-dsts carg=0x0 cctl=0x0 csts=0x100 dsts=na int=na",
        )
        failed_evidence = parse_mmc_evidence(failed)
        self.assertEqual(failed_evidence.controller["state"], "error:read-dsts")
        self.assertIsNone(failed_evidence.controller["dsts"])

    def test_rejects_incomplete_duplicate_and_malformed_evidence(self) -> None:
        malformed = (
            MMC_RESPONSE.replace(" proof=0", " proof=2"),
            MMC_RESPONSE.replace(" blockers=7", " blockers=-1"),
            MMC_RESPONSE.replace(" controller=ok", " proof=0 controller=ok"),
            MMC_RESPONSE.replace(" int=0x0", " int=zero"),
            MMC_RESPONSE.replace(" gates=", " omitted_gates="),
            MMC_RESPONSE.replace("irq:missing", "irq:invented"),
            MMC_RESPONSE.replace(" trace=none", " trace=present"),
            MMC_RESPONSE.rstrip("\r\n") + "\n",
        )
        for response in malformed:
            with self.subTest(response=response[-80:]):
                with self.assertRaises(MonitorProtocolError):
                    parse_mmc_evidence(response)

    def test_writes_compact_non_overwriting_evidence_record(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "mmc.json"
            captured_at = datetime(2026, 8, 10, 1, 2, 3, tzinfo=timezone.utc)
            write_mmc_evidence(path, "ls2k1000-board-a", MMC_RESPONSE,
                               captured_at=captured_at)
            raw = path.read_bytes()
            self.assertTrue(raw.endswith(b"\n"))
            self.assertLess(len(raw), 4096)
            record = json.loads(raw)
            self.assertEqual(record["schema"], "wateros-ls2k-mmc-evidence-v1")
            self.assertEqual(record["board_id"], "ls2k1000-board-a")
            self.assertEqual(record["captured_at"], "2026-08-10T01:02:03Z")
            self.assertEqual(record["hardware_validation"], "unverified-observation")
            self.assertEqual(record["response_sha256"],
                             hashlib.sha256(MMC_RESPONSE.encode("utf-8")).hexdigest())
            with self.assertRaises(FileExistsError):
                write_mmc_evidence(path, "ls2k1000-board-a", MMC_RESPONSE,
                                   captured_at=captured_at)
            with self.assertRaises(ValueError):
                write_mmc_evidence(Path(directory) / "bad.json", "bad\nboard", MMC_RESPONSE)
            with self.assertRaises(ValueError):
                write_mmc_evidence(Path(directory) / "space.json", "board with space", MMC_RESPONSE)


if __name__ == "__main__":
    unittest.main()
