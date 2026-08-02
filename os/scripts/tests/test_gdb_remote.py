from __future__ import annotations

import sys
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from gdb_remote_snapshot import GdbRemote, RemoteError


class FakeSocket:
    def __init__(self, response: bytes) -> None:
        self.response = bytearray(response)
        self.sent: list[bytes] = []

    def recv(self, size: int) -> bytes:
        del size
        data = bytes(self.response)
        self.response.clear()
        return data

    def sendall(self, data: bytes) -> None:
        self.sent.append(data)


def remote_with_packet(packet: bytes) -> tuple[GdbRemote, FakeSocket]:
    remote = GdbRemote.__new__(GdbRemote)
    socket = FakeSocket(packet)
    remote.sock = socket
    remote._buffer = bytearray()
    return remote, socket


class GdbRemotePacketTests(unittest.TestCase):
    def test_packet_frame_checksum(self) -> None:
        self.assertEqual(GdbRemote._frame("?"), b"$?#3f")

    def test_read_packet_acknowledges_valid_checksum(self) -> None:
        remote, socket = remote_with_packet(b"+$OK#9a")
        self.assertEqual(remote._read_packet(), "OK")
        self.assertEqual(socket.sent, [b"+"])

    def test_read_packet_rejects_bad_checksum(self) -> None:
        remote, socket = remote_with_packet(b"$OK#00")
        with self.assertRaises(RemoteError):
            remote._read_packet()
        self.assertEqual(socket.sent, [b"-"])


if __name__ == "__main__":
    unittest.main()
