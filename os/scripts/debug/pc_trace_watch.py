#!/usr/bin/env python3
"""启动 QEMU，仅在 guest PC 变化时输出记录；只依赖标准库，不提供 TUI。"""
from __future__ import annotations

import argparse
import fcntl
import os
import signal
import subprocess
import sys
import threading
from pathlib import Path

_DEBUG_DIR = Path(__file__).resolve().parent
if str(_DEBUG_DIR) not in sys.path:
    sys.path.insert(0, str(_DEBUG_DIR))

from loop_detector import LoopDetector  # noqa: E402
from pc_trace_parser import TRACE_PC_BYTES_RE  # noqa: E402
from qemu_launcher import build_qemu_trace_cmd  # noqa: E402
from symbol_index import SymbolIndex  # noqa: E402

TRACE_READ_SIZE = 256 * 1024
PIPE_SIZE = 1024 * 1024


def _enlarge_pipe(fd: int) -> None:
    try:
        fcntl.fcntl(fd, getattr(fcntl, "F_SETPIPE_SZ", 1031), PIPE_SIZE)
    except OSError:
        pass


def _safe_print(*args, file=None, **kwargs) -> bool:
    """Print without raising if stdout/stderr is gone or non-blocking."""
    if file is None:
        file = sys.stdout
    try:
        print(*args, file=file, **kwargs)
        return True
    except (BrokenPipeError, BlockingIOError, OSError):
        return False


def _drain_stdout(stream, stop: threading.Event) -> None:
    if stream is None:
        return
    while not stop.is_set():
        try:
            chunk = stream.read(65536)
        except OSError:
            break
        if not chunk:
            break


def _iter_trace_pcs(trace_fd: int, stop: threading.Event, sample: int):
    """Yield guest PC from trace fd; ``sample`` = process every Nth trace line."""
    seen = 0
    buf = b""
    trace_in = os.fdopen(trace_fd, "rb", buffering=0, closefd=True)
    try:
        while not stop.is_set():
            chunk = trace_in.read(TRACE_READ_SIZE)
            if not chunk:
                break
            buf += chunk
            while b"\n" in buf:
                line, buf = buf.split(b"\n", 1)
                if not line.startswith(b"Trace"):
                    continue
                m = TRACE_PC_BYTES_RE.search(line)
                if not m:
                    continue
                seen += 1
                if seen % sample != 0:
                    continue
                yield int(m.group(1), 16)
    finally:
        trace_in.close()


def run_watch(arch: str, elf: Path, work_dir: Path, sample: int) -> int:
    _safe_print(f"[pc-watch] loading symbols from {elf} ...", file=sys.stderr, flush=True)
    index = SymbolIndex(elf, arch)  # type: ignore[arg-type]
    loop_det = LoopDetector()
    last_pc: int | None = None
    last_loop_msg: str | None = None
    stop = threading.Event()
    exit_code = 0

    trace_r, trace_w = os.pipe()
    _enlarge_pipe(trace_r)
    _enlarge_pipe(trace_w)
    cmd = build_qemu_trace_cmd(arch, work_dir, trace_fd=trace_w)  # type: ignore[arg-type]

    _safe_print("[pc-watch] starting QEMU (print on PC change only)", file=sys.stderr, flush=True)
    try:
        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            cwd=str(work_dir),
            start_new_session=True,
            pass_fds=(trace_w,),
        )
    except OSError as exc:
        os.close(trace_r)
        os.close(trace_w)
        _safe_print(f"[pc-watch] QEMU failed: {exc}", file=sys.stderr)
        return 1
    os.close(trace_w)

    threading.Thread(
        target=_drain_stdout, args=(proc.stdout, stop), name="stdout-drain", daemon=True
    ).start()

    def on_sig(_sig, _frame) -> None:
        stop.set()

    signal.signal(signal.SIGINT, on_sig)
    signal.signal(signal.SIGTERM, on_sig)

    gen = _iter_trace_pcs(trace_r, stop, sample)
    try:
        for pc in gen:
            if stop.is_set():
                break
            if pc == last_pc:
                continue
            last_pc = pc
            if not _safe_print(index.lookup_fast(pc).format_short(), flush=True):
                stop.set()
                break

            hit = loop_det.push(pc)
            if hit is not None:
                msg = hit.summary()
                if msg != last_loop_msg:
                    if not _safe_print(f"*** {msg}", flush=True):
                        stop.set()
                        break
                    last_loop_msg = msg
    except KeyboardInterrupt:
        stop.set()
    finally:
        stop.set()
        gen.close()
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
        except (ProcessLookupError, OSError):
            try:
                proc.terminate()
            except OSError:
                pass
        exit_code = proc.wait()
        _safe_print(f"[pc-watch] QEMU exited ({exit_code})", file=sys.stderr, flush=True)
    return exit_code


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Run QEMU and print lines only when guest PC changes",
    )
    parser.add_argument("--arch", choices=["rv", "la"], required=True)
    parser.add_argument("--elf", type=Path, required=True)
    parser.add_argument(
        "--sample",
        type=int,
        default=1,
        help="Use every Nth trace TB (default 1 = all TB boundaries)",
    )
    args = parser.parse_args()
    elf = args.elf.resolve()
    if not elf.is_file():
        _safe_print(f"error: ELF not found: {elf}", file=sys.stderr)
        return 2
    return run_watch(args.arch, elf, Path.cwd(), max(1, args.sample))


if __name__ == "__main__":
    raise SystemExit(main())
