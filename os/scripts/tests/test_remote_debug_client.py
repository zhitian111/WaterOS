from __future__ import annotations

import socket
import sys
import threading
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from remote_debug_client import (MonitorClient, MonitorProtocolError, connect_with_retry,
                                 run_smoke)


class RemoteDebugClientTests(unittest.TestCase):
    def test_complete_smoke_session(self) -> None:
        client_socket, server_socket = socket.socketpair()

        def server() -> None:
            with server_socket:
                server_socket.sendall(
                    b"WaterOS development monitor\r\n"
                    b"protocol=1 auth=none encryption=none hardware=unverified\r\n"
                    b"Type 'help' for commands.\r\nwos> "
                )
                stream = server_socket.makefile("rb")
                responses = {
                    b"hello\n": b"protocol=1 hardware=unverified\r\nwos> ",
                    b"capabilities\n": b"readonly=true auth=none\r\nwos> ",
                    b"ping\n": b"pong\r\nwos> ",
                    b"status\n": (
                        b"tick=7 online_cpus=0x1 heap_used=8 heap_free=9 "
                        b"heap_capacity=17\r\nwos> "
                    ),
                    b"version\n": b"WaterOS 0.1.0\r\nwos> ",
                    b"quit\n": b"bye\r\n",
                }
                for expected, response in responses.items():
                    self.assertEqual(stream.readline(), expected)
                    server_socket.sendall(response)

        thread = threading.Thread(target=server)
        thread.start()
        client = MonitorClient(client_socket)
        try:
            results = run_smoke(client)
            self.assertEqual(
                [result.command for result in results],
                ["hello", "capabilities", "ping", "status", "version", "quit"],
            )
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
                        b"WaterOS development monitor\r\n"
                        b"protocol=1 auth=none encryption=none hardware=unverified\r\n"
                        b"Type 'help' for commands.\r\nwos> "
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


if __name__ == "__main__":
    unittest.main()
