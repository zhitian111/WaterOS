#!/usr/bin/env python3
"""Real-time QEMU PC trace monitor with Textual TUI."""
from __future__ import annotations

import argparse
import subprocess
import sys
import threading
from collections import deque
from pathlib import Path

_DEBUG_DIR = Path(__file__).resolve().parent
if str(_DEBUG_DIR) not in sys.path:
    sys.path.insert(0, str(_DEBUG_DIR))

from loop_detector import LoopDetection, LoopDetector  # noqa: E402
from pc_trace_parser import parse_qemu_line  # noqa: E402
from symbol_index import SymbolIndex  # noqa: E402

try:
    from textual import work
    from textual.app import App, ComposeResult
    from textual.containers import Horizontal, Vertical
    from textual.widgets import Footer, Header, RichLog, Sparkline, Static
except ImportError as exc:
    print(
        "textual is required. Install manually, e.g.:\n"
        "  pip install -r scripts/requirements-debug.txt\n"
        "  pacman -S python-textual   # Arch Linux",
        file=sys.stderr,
    )
    raise SystemExit(1) from exc


class QemuPcMonitorApp(App):
    """Four-panel TUI: log, PC trace, sparkline chart, loop status."""

    CSS = """
    #top_row {
        height: 1fr;
        min-height: 12;
    }
    #log_panel {
        width: 1fr;
        border: solid green;
    }
    #pc_panel {
        width: 1fr;
        border: solid cyan;
    }
    #chart_panel {
        height: 6;
        border: solid yellow;
        padding: 0 1;
    }
    #loop_status {
        height: 3;
        padding: 0 1;
    }
    #loop_status.loop_ok {
        border: solid white;
    }
    #loop_status.loop_alert {
        border: solid red;
        background: $warning-darken-3;
    }
    Sparkline {
        height: 1fr;
    }
    """

    BINDINGS = [
        ("q", "quit", "Quit"),
        ("c", "clear_logs", "Clear"),
    ]

    def __init__(
        self,
        arch: str,
        elf_path: Path,
        qemu_cmd: list[str],
        sample: int = 1,
        chart_points: int = 120,
        pc_history: int = 80,
    ) -> None:
        super().__init__()
        self.arch = arch
        self.elf_path = elf_path
        self.qemu_cmd = qemu_cmd
        self.sample = max(1, sample)
        self.chart_points = chart_points
        self.pc_history = pc_history
        self.symbol_index = SymbolIndex(elf_path, arch)  # type: ignore[arg-type]
        self.loop_detector = LoopDetector()
        self._chart_data: deque[float] = deque(maxlen=chart_points)
        self._chart_base: int | None = None
        self._sample_counter = 0
        self._proc: subprocess.Popen[str] | None = None
        self._reader_done = threading.Event()

    def compose(self) -> ComposeResult:
        yield Header(show_clock=True)
        with Horizontal(id="top_row"):
            yield RichLog(id="log_panel", highlight=True, markup=False, wrap=True)
            yield RichLog(id="pc_panel", highlight=True, markup=False, wrap=False)
        yield Sparkline([], id="chart_panel")
        yield Static("状态: OK", id="loop_status", classes="loop_ok")
        yield Footer()

    def on_mount(self) -> None:
        self.run_qemu_reader()

    @work(thread=True)
    def run_qemu_reader(self) -> None:
        try:
            self._proc = subprocess.Popen(
                self.qemu_cmd,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                bufsize=1,
            )
        except OSError as exc:
            self.call_from_thread(self._log_message, f"[QEMU launch error] {exc}")
            self._reader_done.set()
            return

        assert self._proc.stdout is not None
        for line in self._proc.stdout:
            self.call_from_thread(self._handle_line, line)
        self._proc.wait()
        code = self._proc.returncode
        self.call_from_thread(self._log_message, f"[QEMU exited with code {code}]")
        self._reader_done.set()

    def _handle_line(self, line: str) -> None:
        parsed = parse_qemu_line(line)
        if parsed.is_trace and parsed.pc is not None:
            self._sample_counter += 1
            if self._sample_counter % self.sample != 0:
                return
            self._handle_pc(parsed.pc)
            return
        text = line.rstrip("\n\r")
        if text:
            log = self.query_one("#log_panel", RichLog)
            log.write(text)

    def _handle_pc(self, pc: int) -> None:
        lookup = self.symbol_index.lookup(pc)
        pc_log = self.query_one("#pc_panel", RichLog)
        pc_log.write(lookup.format_short())

        if self._chart_base is None:
            self._chart_base = pc
        rel = float(pc - self._chart_base)
        self._chart_data.append(rel)
        chart = self.query_one("#chart_panel", Sparkline)
        chart.data = list(self._chart_data)

        detection = self.loop_detector.push(pc)
        self._update_loop_status(detection)

        # Trim PC panel height by clearing when too many lines (RichLog grows unbounded)
        # Textual RichLog has no built-in maxlen; keep last N via periodic trim is costly.
        # Accept growth for debug sessions; user can press 'c' to clear.

    def _update_loop_status(self, detection: LoopDetection | None) -> None:
        status = self.query_one("#loop_status", Static)
        active = detection or self.loop_detector.last_detection
        if active is not None:
            status.update(active.summary())
            status.remove_class("loop_ok")
            status.add_class("loop_alert")
        else:
            status.update("状态: OK")
            status.remove_class("loop_alert")
            status.add_class("loop_ok")

    def _log_message(self, msg: str) -> None:
        self.query_one("#log_panel", RichLog).write(msg)

    def action_clear_logs(self) -> None:
        self.query_one("#log_panel", RichLog).clear()
        self.query_one("#pc_panel", RichLog).clear()
        self._chart_data.clear()
        self._chart_base = None
        self.query_one("#chart_panel", Sparkline).data = []
        self.loop_detector = LoopDetector()
        self._update_loop_status(None)

    def action_quit(self) -> None:
        if self._proc is not None and self._proc.poll() is None:
            self._proc.terminate()
        self.exit()


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="WaterOS QEMU PC trace monitor (TUI)")
    parser.add_argument("--arch", choices=["rv", "la"], required=True)
    parser.add_argument("--elf", type=Path, required=True, help="Path to kernel ELF")
    parser.add_argument(
        "--sample",
        type=int,
        default=1,
        help="Record every Nth TB trace (default: 1)",
    )
    parser.add_argument(
        "qemu_cmd",
        nargs=argparse.REMAINDER,
        help="QEMU launcher after '--', e.g. -- ./scripts/rv_qemu_run_trace_pc.sh",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    cmd = [c for c in args.qemu_cmd if c != "--"]
    if not cmd:
        print("error: missing QEMU command after '--'", file=sys.stderr)
        return 2
    if cmd[0].endswith(".sh"):
        cmd = ["bash", *cmd]
    if not args.elf.is_file():
        print(f"error: ELF not found: {args.elf}", file=sys.stderr)
        return 2

    app = QemuPcMonitorApp(
        arch=args.arch,
        elf_path=args.elf.resolve(),
        qemu_cmd=cmd,
        sample=args.sample,
    )
    app.run()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
