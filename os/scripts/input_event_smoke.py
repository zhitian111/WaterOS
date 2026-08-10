#!/usr/bin/env python3
"""Small host-side evdev/devfs contract smoke without a guest disk image."""

from __future__ import annotations

from dataclasses import dataclass
import struct

EVENT_SIZE = 16
EV_KEY = 0x01
EV_REL = 0x02
REL_X = 0x00
REL_Y = 0x01
KEY_A = 30


@dataclass(frozen=True)
class InputNode:
    index: int
    kind: str

    @property
    def path(self) -> str:
        return f"/dev/input/event{self.index}"

    @property
    def metadata(self) -> tuple[int, int, int, int]:
        return (13, self.index, 0o600, 0x6)  # char | input


def encode_event(event_type: int, code: int, value: int) -> bytes:
    """Encode WaterOS' fixed 16-byte host-endian-independent event record."""
    return struct.pack("<QHHi", 0, event_type, code, value)


def run_smoke() -> None:
    devices: dict[int, InputNode] = {}
    keyboard = InputNode(0, "keyboard")
    mouse = InputNode(1, "mouse")
    devices[keyboard.index] = keyboard
    devices[mouse.index] = mouse
    assert keyboard.path == "/dev/input/event0"
    assert mouse.metadata == (13, 1, 0o600, 0x6)
    assert encode_event(EV_KEY, KEY_A, 1) == bytes.fromhex("000000000000000001001e0001000000")
    assert encode_event(EV_REL, REL_X, 12) == bytes.fromhex("0000000000000000020000000c000000")
    del devices[keyboard.index]
    assert keyboard.index not in devices


if __name__ == "__main__":
    run_smoke()
    print("input event smoke passed")
