#!/usr/bin/env python3
"""在滑动窗口中检测重复出现的 PC 序列。"""
from __future__ import annotations

from collections import deque
from dataclasses import dataclass


@dataclass(frozen=True)
class LoopDetection:
    """检测到的 PC 循环。"""

    pattern: tuple[int, ...]
    repeats: int
    pc_min: int
    pc_max: int

    def summary(self) -> str:
        addrs = ", ".join(f"0x{p:016x}" for p in self.pattern)
        return (
            f"可能陷入循环: PC 在 [0x{self.pc_min:016x}..0x{self.pc_max:016x}] "
            f"周期={len(self.pattern)} ({addrs}) 重复>={self.repeats} 次"
        )


class LoopDetector:
    """Sliding-window periodic pattern detector for guest PCs."""

    def __init__(
        self,
        window: int = 128,
        min_period: int = 2,
        min_repeat: int = 4,
    ) -> None:
        self._window = window
        self._min_period = min_period
        self._min_repeat = min_repeat
        self._history: deque[int] = deque(maxlen=window)
        self._last: LoopDetection | None = None

    @property
    def last_detection(self) -> LoopDetection | None:
        return self._last

    def push(self, pc: int) -> LoopDetection | None:
        self._history.append(pc)
        detection = self._detect()
        if detection is not None:
            self._last = detection
        return detection

    def _detect(self) -> LoopDetection | None:
        hist = list(self._history)
        n = len(hist)
        max_period = n // self._min_repeat
        if max_period < self._min_period:
            return None

        best: LoopDetection | None = None
        for period in range(self._min_period, max_period + 1):
            need = period * self._min_repeat
            if n < need:
                continue
            tail = hist[-need:]
            pattern = tuple(tail[:period])
            repeats = 1
            idx = period
            while idx + period <= len(tail):
                chunk = tuple(tail[idx : idx + period])
                if chunk != pattern:
                    break
                repeats += 1
                idx += period
            if repeats < self._min_repeat:
                continue
            pc_min = min(pattern)
            pc_max = max(pattern)
            candidate = LoopDetection(
                pattern=pattern,
                repeats=repeats,
                pc_min=pc_min,
                pc_max=pc_max,
            )
            if best is None or repeats > best.repeats or (
                repeats == best.repeats and period < len(best.pattern)
            ):
                best = candidate
        return best
