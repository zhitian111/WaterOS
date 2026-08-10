#!/usr/bin/env python3
"""Run one reproducible BuildStorm QEMU measurement and write result.json.

The runner deliberately uses QEMU's ``-snapshot`` mode instead of creating an
overlay.  Runs with QEMU plugins are diagnostics and are marked ineligible for
wall-clock acceptance in the result.
"""
from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import os
import re
import shlex
import signal
import subprocess
import sys
import time
from pathlib import Path
from typing import Any


OS_ROOT = Path(__file__).resolve().parents[2]
REPO_ROOT = OS_ROOT.parent
JUDGE = REPO_ROOT / "final_test_case/judge/judge_buildstorm-glibc.py"
DEFAULT_OUTPUT_ROOT = OS_ROOT / "tem/perf/buildstorm"
ARCH_CONFIG = {
    "rv": {"qemu": "qemu-system-riscv64", "memory": "16G", "cpus": 8},
    "la": {"qemu": "qemu-system-loongarch64", "memory": "36G", "cpus": 12},
}
COMPILE_RE = re.compile(r"BUILDSTORM_COMPILE\s+([^\r\n]+)")
COUNTER_RE = re.compile(r"BUILDSTORM_PERF_COUNTERS\s+([^\r\n]+)")
PANIC_RE = re.compile(r"(?i)(?:kernel panic|panicked at|panic:)")
SIGSEGV_RE = re.compile(r"(?i)(?:SIGSEGV|segmentation fault)")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def evict_image_cache(path: Path) -> None:
    """Flush writes, then evict only this image from the host page cache."""
    os.sync()
    fd = os.open(path, os.O_RDONLY)
    try:
        if hasattr(os, "posix_fadvise"):
            os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
            return
        libc = ctypes.CDLL(None, use_errno=True)
        rc = libc.posix_fadvise(fd, 0, 0, 4)  # POSIX_FADV_DONTNEED
        if rc:
            raise OSError(rc, os.strerror(rc), str(path))
    finally:
        os.close(fd)


def qemu_argv(arch: str, kernel: Path, image: Path, *, linux_userland: bool = False) -> list[str]:
    config = ARCH_CONFIG[arch]
    common = [
        str(config["qemu"]), "-kernel", str(kernel), "-m", str(config["memory"]),
        "-nographic", "-smp", str(config["cpus"]),
    ]
    if arch == "rv":
        command = common + [
            "-machine", "virt", "-bios", "default",
            "-drive", f"file={image},if=none,format=raw,id=x0",
            "-device", "virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0",
            "-no-reboot", "-device", "virtio-net-device,netdev=net",
            "-netdev", "user,id=net", "-rtc", "base=utc", "-snapshot",
        ]
    else:
        command = common + [
            "-drive", f"file={image},if=none,format=raw,id=x0",
            "-device", "virtio-blk-pci,drive=x0", "-no-reboot",
            "-device", "virtio-net-pci,netdev=net0", "-netdev", "user,id=net0",
            "-rtc", "base=utc", "-snapshot",
        ]
    if linux_userland:
        # WaterOS starts the final workload itself.  The reference Linux kernel
        # instead needs its root device and the same official guest script as
        # PID 1; this avoids prompt timing and keeps the measured script equal.
        command += [
            "-append",
            "root=/dev/vda rw console=ttyS0 init=/glibc/buildstorm_testcode.sh",
        ]
    return command


def plugin_args(arch: str, output_dir: Path, plugins: list[str]) -> tuple[list[str], dict[str, str]]:
    args: list[str] = []
    outputs: dict[str, str] = {}
    for name in plugins:
        script = OS_ROOT / f"scripts/pc-hot/{name}.sh"
        subprocess.run([str(script), arch, "build"], cwd=OS_ROOT, check=True)
        shared_object = OS_ROOT / f"scripts/pc-hot/build/{arch}/{name}-{arch}.so"
        destination = output_dir / f"{name}.txt"
        args += ["-plugin", f"file={shared_object},out={destination}"]
        outputs[name] = str(destination)
    return args, outputs


def parse_compile(log: str) -> dict[str, str] | None:
    matches = COMPILE_RE.findall(log)
    if not matches:
        return None
    fields: dict[str, str] = {}
    for token in matches[-1].split():
        if "=" in token:
            key, value = token.split("=", 1)
            fields[key] = value
    return fields


def parse_perf_counters(log: str) -> dict[str, int]:
    """Parse one or more diagnostic counter snapshots from the serial log."""
    counters: dict[str, int] = {}
    for snapshot in COUNTER_RE.findall(log):
        for token in snapshot.split():
            if "=" not in token:
                continue
            key, value = token.split("=", 1)
            try:
                counters[key] = int(value, 0)
            except ValueError:
                continue
    return counters


def run_judge(log_path: Path) -> dict[str, Any]:
    completed = subprocess.run(
        [sys.executable, str(JUDGE), str(log_path)], cwd=REPO_ROOT,
        text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
    )
    parsed = None
    try:
        parsed = json.loads(completed.stdout)
    except json.JSONDecodeError:
        pass
    return {
        "command": [sys.executable, str(JUDGE), str(log_path)],
        "exit_code": completed.returncode,
        "results": parsed,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


def git_sha() -> str | None:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=REPO_ROOT, text=True,
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, check=False,
    )
    return result.stdout.strip() if result.returncode == 0 else None


def qemu_version(binary: str) -> str | None:
    try:
        result = subprocess.run(
            [binary, "--version"], text=True, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT, check=False,
        )
    except OSError:
        return None
    return result.stdout.splitlines()[0] if result.stdout else None


def execute(argv: list[str], log_path: Path, timeout: float) -> tuple[int | None, bool, bool, float, float]:
    started = time.monotonic()
    last_output = started
    timed_out = False
    stopped_after_result = False
    marker_tail = b""
    saw_toolchain = False
    saw_minibuild = False
    with log_path.open("wb", buffering=0) as log:
        process = subprocess.Popen(
            argv, cwd=OS_ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        assert process.stdout is not None
        os.set_blocking(process.stdout.fileno(), False)
        while process.poll() is None:
            try:
                chunk = process.stdout.read(65536)
            except BlockingIOError:
                chunk = None
            if chunk:
                log.write(chunk)
                last_output = time.monotonic()
                marker_tail = (marker_tail + chunk)[-16384:]
                saw_toolchain = saw_toolchain or b"BUILDSTORM_TOOLCHAIN ok" in marker_tail
                saw_minibuild = saw_minibuild or b"BUILDSTORM_MINIBUILD ok" in marker_tail
                if (
                    saw_toolchain
                    and saw_minibuild
                    and re.search(rb"BUILDSTORM_COMPILE\s+[^\r\n]*mode=multi[^\r\n]*ok=true", marker_tail)
                ):
                    # Linux panics when the test script used as PID 1 exits;
                    # WaterOS may also remain at its operator loop.  The final
                    # result marker is the protocol's terminal condition.
                    stopped_after_result = True
                    os.killpg(process.pid, signal.SIGTERM)
                    process.wait(timeout=5)
                    break
            if time.monotonic() - started >= timeout:
                timed_out = True
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                break
            time.sleep(0.05)
        while True:
            try:
                chunk = process.stdout.read(65536)
            except BlockingIOError:
                chunk = None
            if not chunk:
                break
            log.write(chunk)
            last_output = time.monotonic()
        process.stdout.close()
    return process.returncode, timed_out, stopped_after_result, time.monotonic() - started, time.monotonic() - last_output


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=sorted(ARCH_CONFIG), required=True)
    parser.add_argument("--kernel", type=Path, required=True)
    parser.add_argument("--image", type=Path, required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--timeout", type=float, required=True)
    parser.add_argument("--output-root", type=Path, default=DEFAULT_OUTPUT_ROOT)
    parser.add_argument("--plugin", choices=("pc-hot", "wait-hot"), action="append", default=[])
    parser.add_argument(
        "--linux-userland", action="store_true",
        help="boot a reference Linux kernel and run the official guest script as init",
    )
    parser.add_argument("--dry-run", action="store_true", help="write metadata without starting QEMU")
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", args.run_id):
        parser.error("--run-id must contain only letters, digits, dot, underscore, and dash")
    kernel = args.kernel.resolve()
    image = args.image.resolve()
    for label, path in (("kernel", kernel), ("image", image)):
        if not path.is_file():
            parser.error(f"{label} is not a file: {path}")
    output_dir = (args.output_root / args.run_id).resolve()
    try:
        output_dir.mkdir(parents=True, exist_ok=False)
    except FileExistsError:
        parser.error(f"run output already exists: {output_dir}")

    argv = qemu_argv(args.arch, kernel, image, linux_userland=args.linux_userland)
    extra_args, plugin_outputs = plugin_args(args.arch, output_dir, args.plugin)
    argv += extra_args
    serial_log = output_dir / "serial.log"
    metadata: dict[str, Any] = {
        "schema_version": 1,
        "run_id": args.run_id,
        "arch": args.arch,
        "linux_userland": args.linux_userland,
        "diagnostic_plugins": args.plugin,
        "wall_clock_eligible": not args.plugin and not args.dry_run,
        "kernel": {"path": str(kernel), "sha256": sha256_file(kernel)},
        "image": {"path": str(image), "sha256": sha256_file(image)},
        "git_sha": git_sha(),
        "qemu_version": qemu_version(argv[0]),
        "cpu_affinity": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None,
        "command": argv,
        "command_shell": shlex.join(argv),
        "timeout_s": args.timeout,
        "serial_log": str(serial_log),
        "plugin_outputs": plugin_outputs,
    }
    result_path = output_dir / "result.json"
    if args.dry_run:
        metadata["status"] = "dry_run"
        result_path.write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")
        print(result_path)
        return 0

    metadata["status"] = "running"
    result_path.write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")
    evict_image_cache(image)
    returncode, timed_out, stopped_after_result, host_wall, silence = execute(argv, serial_log, args.timeout)
    log = serial_log.read_text(errors="replace")
    compile_fields = parse_compile(log)
    perf_counters = parse_perf_counters(log)
    judge = run_judge(serial_log)
    required = {
        "toolchain": bool(re.search(r"BUILDSTORM_TOOLCHAIN\s+ok\b", log)),
        "minibuild": bool(re.search(r"BUILDSTORM_MINIBUILD\s+ok\b", log)),
        "compile": bool(compile_fields and compile_fields.get("mode") == "multi" and compile_fields.get("ok") == "true"),
    }
    successful_result = re.search(
        r"BUILDSTORM_COMPILE\s+[^\r\n]*mode=multi[^\r\n]*ok=true", log
    )
    terminal_offset = successful_result.start() if successful_result else len(log)
    panic_match = PANIC_RE.search(log)
    sigsegv_match = SIGSEGV_RE.search(log)
    fatal_before_result = bool(
        (panic_match and panic_match.start() < terminal_offset)
        or (sigsegv_match and sigsegv_match.start() < terminal_offset)
    )
    elapsed = None
    if compile_fields:
        try:
            elapsed = float(compile_fields.get("elapsed_s", ""))
        except ValueError:
            pass
    metadata.update({
        "status": "passed" if all(required.values()) and not timed_out and not fatal_before_result else "failed",
        "qemu_exit_code": returncode,
        "stopped_after_result": stopped_after_result,
        "timed_out": timed_out,
        "stalled": timed_out and silence >= min(120.0, args.timeout / 4),
        "last_serial_silence_s": round(silence, 3),
        "host_wall_s": round(host_wall, 3),
        "guest_elapsed_s": elapsed,
        "compile": compile_fields,
        "perf_counters": perf_counters,
        "required_markers": required,
        "panic": bool(panic_match),
        "sigsegv": bool(sigsegv_match),
        "fatal_before_result": fatal_before_result,
        "judge": judge,
    })
    result_path.write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")
    print(result_path)
    return 0 if metadata["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
