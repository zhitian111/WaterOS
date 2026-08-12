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

SCRIPTS_DIR = Path(__file__).resolve().parents[1]
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

from source.logging_utils import error as log_error  # noqa: E402
from source.logging_utils import info as log_info  # noqa: E402
from source.argparse_utils import ChineseArgumentParser  # noqa: E402

ANSI = re.compile(rb"\x1b\[[0-?]*[ -/]*[@-~]")


def parse_args() -> argparse.Namespace:
    parser = ChineseArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=("rv", "la"), required=True, help="Guest 架构")
    parser.add_argument("--profile", choices=("pre", "final"), default="pre", help="构建阶段，默认为 pre")
    parser.add_argument("--smp", type=int, default=1, help="Guest CPU 数量，默认为 1")
    parser.add_argument("--timeout", type=float, default=120.0, help="测试超时秒数，默认为 120")
    parser.add_argument("--mode", choices=("shell", "run"), default="shell", help="operator 模式，默认为 shell")
    parser.add_argument("--script", help="run 模式执行的 Guest 绝对路径")
    parser.add_argument("--vim", action="store_true", help="额外验证 Vim raw-mode 保存流程")
    parser.add_argument("--no-build", action="store_true", help="复用已有内核，不执行构建")
    parser.add_argument("--log", type=Path, help="串口日志输出路径")
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

        # 在不依赖比赛镜像安装编辑器的情况下验证非规范输入。`dd` 必须接收三个不带
        # 换行的字节；命令返回后由 Shell 恢复规范模式，使后续冒烟测试能够正常继续
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

        # 退出 operator shell 后，监督进程必须启动救援终端
        prompt_count = bytes(console.output).count(b"# ")
        console.send_line(b"exit")
        while bytes(console.output).count(b"# ") <= prompt_count:
            if time.monotonic() >= console.deadline:
                raise TimeoutError("rescue shell prompt did not reappear")
            console.pump()
        rescue = b"__WOS_RESCUE_OK__"
        console.send_line(b"echo " + rescue)
        console.wait_line(rescue)
        log_info(
            f"operator 冒烟测试通过 arch={args.arch} profile={args.profile} smp={args.smp}",
            component="TEST",
        )
        return 0
    except (TimeoutError, OSError, RuntimeError) as error:
        log_error(f"operator 冒烟测试失败 reason={error}", component="TEST")
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
        log_info(f"串口日志已保存 path={log_path}", component="TEST")


if __name__ == "__main__":
    raise SystemExit(main())
