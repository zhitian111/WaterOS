#!/usr/bin/env python3
"""只使用 Python 标准库驱动 WaterOS 串口 operator shell 冒烟测试。"""

from __future__ import annotations

import argparse
import os
import pty
import re
import select
import signal
import subprocess
import sys
import time
from pathlib import Path

ANSI = re.compile(rb"\x1b\[[0-?]*[ -/]*[@-~]")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=("rv", "la"), required=True)
    parser.add_argument("--profile", choices=("pre", "final"), default="pre")
    parser.add_argument("--smp", type=int, default=1)
    parser.add_argument("--timeout", type=float, default=120.0)
    parser.add_argument("--mode", choices=("shell", "run"), default="shell")
    parser.add_argument("--script", help="absolute guest script path required by --mode run")
    parser.add_argument("--vim", action="store_true", help="also require a Vim raw-mode save")
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--log", type=Path)
    return parser.parse_args()


class Console:
    def __init__(self, process: subprocess.Popen[bytes], master: int, deadline: float):
        self.process = process
        self.master = master
        self.deadline = deadline
        self.output = bytearray()

    def pump(self, timeout: float = 0.2) -> None:
        readable, _, _ = select.select([self.master], [], [], timeout)
        if readable:
            chunk = os.read(self.master, 65536)
            self.output.extend(chunk)

    def text_lines(self) -> list[bytes]:
        clean = ANSI.sub(b"", bytes(self.output)).replace(b"\r", b"")
        return clean.splitlines()

    def wait_contains(self, needle: bytes) -> None:
        while time.monotonic() < self.deadline and self.process.poll() is None:
            self.pump()
            if needle in self.output:
                return
        raise TimeoutError(f"serial output did not contain {needle!r}")

    def wait_line(self, line: bytes, previous_count: int = 0) -> None:
        while time.monotonic() < self.deadline and self.process.poll() is None:
            self.pump()
            if sum(item == line for item in self.text_lines()) > previous_count:
                return
        raise TimeoutError(f"serial output did not produce line {line!r}")

    def wait_any_line(self, lines: tuple[bytes, ...]) -> bytes:
        while time.monotonic() < self.deadline and self.process.poll() is None:
            self.pump()
            observed = self.text_lines()
            for line in lines:
                if line in observed:
                    return line
        raise TimeoutError(f"serial output did not produce any of {lines!r}")

    def send_line(self, command: bytes) -> None:
        os.write(self.master, command + b"\r")


def main() -> int:
    args = parse_args()
    if not 1 <= args.smp <= 8:
        raise SystemExit("--smp must be in 1..8")
    if args.mode == "run" and (not args.script or not args.script.startswith("/")):
        raise SystemExit("--mode run requires an absolute --script guest path")
    root = Path(__file__).resolve().parents[2]
    stem = f"{args.arch}-{args.profile}"
    kernel = root / f"kernel-{stem}"
    build_target = f"kernel-{stem}"
    run_script = root / "scripts" / f"{args.arch}_{args.profile}_run.sh"
    build_command = ["make", build_target, f"MODE={args.mode}"]
    if args.mode == "run":
        build_command.append(f"SCRIPT={args.script}")
    if not args.no_build:
        subprocess.run(build_command, cwd=root, check=True)

    master, slave = pty.openpty()
    env = os.environ.copy()
    env.update(WOS_SMP=str(args.smp),
               WOS_QEMU_SNAPSHOT="1", WOS_KERNEL=str(kernel))
    process = subprocess.Popen(["bash", str(run_script)], cwd=root, env=env,
                               stdin=slave, stdout=slave, stderr=slave,
                               start_new_session=True, close_fds=True)
    os.close(slave)
    console = Console(process, master, time.monotonic() + args.timeout)
    log_path = args.log or root / f"operator-smoke-{stem}.log"
    try:
        console.wait_contains(b"# ")

        marker = b"__WOS_BASIC_OK__"
        console.send_line(b"echo hello | cat > /tmp/wos-operator.txt; "
                          b"cat /tmp/wos-operator.txt; (sleep 1; echo background) & "
                          b"wait; echo " + marker)
        console.wait_line(marker)

        console.send_line(b"sleep 30")
        time.sleep(1.0)
        os.write(master, b"\x03")
        time.sleep(0.2)
        interrupt_marker = b"__WOS_CTRL_C_OK__"
        console.send_line(b"echo " + interrupt_marker)
        console.wait_line(interrupt_marker)

        # Exercise noncanonical input without depending on an editor being
        # installed in the competition image. `dd` must receive exactly three
        # bytes without a newline; the shell restores canonical mode after it
        # returns so the rest of the smoke test can continue normally.
        raw_marker = b"__WOS_RAW_TTY_OK__"
        console.send_line(b"busybox stty -icanon -echo min 1 time 0; "
                          b"dd bs=1 count=3 of=/tmp/wos-raw.txt 2>/dev/null; "
                          b"busybox stty sane; cat /tmp/wos-raw.txt; echo; echo " + raw_marker)
        time.sleep(0.5)
        os.write(master, b"raw")
        console.wait_line(raw_marker)

        if args.vim:
            probe = b"__WOS_HAS_VIM__"
            missing = b"__WOS_NO_VIM__"
            console.send_line(b"if command -v vim >/dev/null; then echo " + probe +
                              b"; else echo " + missing + b"; fi")
            if console.wait_any_line((probe, missing)) == missing:
                raise RuntimeError("guest image does not contain Vim")
            console.send_line(b"vim /tmp/wos-vim.txt")
            time.sleep(1.0)
            os.write(master, b"iWaterOS raw TTY\x1b:wq\r")
            vim_marker = b"__WOS_VIM_OK__"
            console.send_line(b"cat /tmp/wos-vim.txt; echo " + vim_marker)
            console.wait_line(vim_marker)

        # Exiting the operator shell must cause the supervisor to spawn rescue.
        prompt_count = bytes(console.output).count(b"# ")
        console.send_line(b"exit")
        while bytes(console.output).count(b"# ") <= prompt_count:
            if time.monotonic() >= console.deadline:
                raise TimeoutError("rescue shell prompt did not reappear")
            console.pump()
        rescue = b"__WOS_RESCUE_OK__"
        console.send_line(b"echo " + rescue)
        console.wait_line(rescue)
        print(f"operator smoke passed: arch={args.arch} profile={args.profile} smp={args.smp}")
        return 0
    except (TimeoutError, OSError, RuntimeError) as error:
        print(f"operator smoke failed: {error}", file=sys.stderr)
        return 1
    finally:
        log_path.write_bytes(console.output)
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
        os.close(master)
        print(f"serial log: {log_path}")


if __name__ == "__main__":
    raise SystemExit(main())
