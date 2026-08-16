#!/usr/bin/env python3
"""Run one reproducible WaterOS BuildStorm measurement with QEMU 9.2.1."""

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
DEFAULT_OUTPUT_ROOT = OS_ROOT / "tem/perf/buildstorm-singlecore/runs"
DEFAULT_QEMU_ROOT = Path("/home/zhitian/qemu_9_2_1/qemu-9.2.1/build")
ARCH_CONFIG = {
    "rv": {"qemu": "qemu-system-riscv64", "memory": "16G", "cpus": 8},
    "la": {"qemu": "qemu-system-loongarch64", "memory": "36G", "cpus": 12},
}
FIELD_RE = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)=([^\s]+)")
BUILDSTORM_RE = re.compile(r"BUILDSTORM_RESULT\s+([^\r\n]+)")
PANIC_RE = re.compile(r"(?i)(?:kernel panic|panicked at|fatal kernel trap|panic:)")
SIGSEGV_RE = re.compile(r"(?i)(?:SIGSEGV|segmentation fault)")
CAGENT_PASS_RE = re.compile(r"testcase\s+cagent\s+\S+\s+pass\b", re.IGNORECASE)
CAGENT_FAIL_RE = re.compile(r"testcase\s+cagent\s+\S+\s+fail\b", re.IGNORECASE)
FINAL_MARKER_RE = re.compile(
    rb"BUILDSTORM_RESULT\s+[^\r\n]*mode=multi[^\r\n]*status=OK[^\r\n]*run=OK"
)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(4 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_fields(text: str) -> dict[str, str]:
    return {key: value for key, value in FIELD_RE.findall(text)}


def parse_last_buildstorm_result(log: str) -> dict[str, str] | None:
    matches = BUILDSTORM_RE.findall(log)
    return parse_fields(matches[-1]) if matches else None


def performance_decision(elapsed: float | None) -> str:
    if elapsed is None:
        return "reject_missing_elapsed"
    if elapsed < 500.0:
        return "retain_without_performance_rerun"
    if elapsed < 520.0:
        return "rerun_required"
    return "reject_not_faster_than_baseline"


def qemu_argv(arch: str, kernel: Path, image: Path, qemu_root: Path) -> list[str]:
    config = ARCH_CONFIG[arch]
    qemu = qemu_root / str(config["qemu"])
    if arch == "rv":
        return [
            str(qemu), "-machine", "virt", "-kernel", str(kernel), "-m", "16G",
            "-nographic", "-smp", "8", "-bios", "default", "-drive",
            f"file={image},if=none,format=raw,id=x0", "-device",
            "virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0", "-no-reboot",
            "-device", "virtio-net-device,netdev=net", "-netdev", "user,id=net",
            "-rtc", "base=utc",
        ]
    return [
        str(qemu), "-kernel", str(kernel), "-m", "36G", "-nographic", "-smp", "12",
        "-drive", f"file={image},if=none,format=raw,id=x0", "-device",
        "virtio-blk-pci,drive=x0", "-no-reboot", "-device",
        "virtio-net-pci,netdev=net0", "-netdev", "user,id=net0", "-rtc", "base=utc",
    ]


def qemu_version(binary: Path) -> str:
    completed = subprocess.run(
        [str(binary), "--version"], text=True, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT, check=False,
    )
    first = completed.stdout.splitlines()[0] if completed.stdout else ""
    if completed.returncode != 0 or not re.search(r"QEMU emulator version 9\.2\.1(?:\s|$)", first):
        raise RuntimeError(f"QEMU 9.2.1 is required, observed: {first or 'no output'}")
    return first


def image_metadata(image: Path) -> dict[str, Any]:
    observed_hash = sha256_file(image)
    metadata_path = image.with_suffix(image.suffix + ".json")
    if metadata_path.is_file():
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
        expected_hash = metadata.get("prepared_raw_sha256")
        if expected_hash is not None and expected_hash != observed_hash:
            raise RuntimeError(
                f"prepared image sha256 mismatch: expected {expected_hash}, observed {observed_hash}"
            )
        return {
            "path": str(image),
            "size": image.stat().st_size,
            "sha256": observed_hash,
            "manifest": metadata,
        }
    return {"path": str(image), "size": image.stat().st_size, "sha256": observed_hash}


def evict_image_cache(path: Path) -> None:
    os.sync()
    fd = os.open(path, os.O_RDONLY)
    try:
        if hasattr(os, "posix_fadvise"):
            os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
            return
        libc = ctypes.CDLL(None, use_errno=True)
        rc = libc.posix_fadvise(fd, 0, 0, 4)
        if rc:
            raise OSError(rc, os.strerror(rc), str(path))
    finally:
        os.close(fd)


def create_run_image(master: Path, destination: Path) -> None:
    subprocess.run(
        ["cp", "--reflink=auto", "--sparse=always", str(master), str(destination)],
        check=True,
    )


def plugin_args(arch: str, output_dir: Path, plugins: list[str]) -> tuple[list[str], dict[str, str]]:
    argv: list[str] = []
    outputs: dict[str, str] = {}
    for name in plugins:
        script = OS_ROOT / f"scripts/pc-hot/{name}-{arch}.sh"
        subprocess.run([str(script), "build"], cwd=OS_ROOT, check=True)
        shared_object = OS_ROOT / f"scripts/pc-hot/build/{arch}/{name}-{arch}.so"
        output = output_dir / f"{name}.txt"
        argv.extend(["-plugin", f"file={shared_object},out={output}"])
        outputs[name] = str(output)
    return argv, outputs


def execute(argv: list[str], log_path: Path, timeout_s: float) -> tuple[int | None, bool, bool, float, float]:
    started = time.monotonic()
    last_output = started
    timed_out = False
    stopped_after_result = False
    marker_tail = b""
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
                marker_tail = (marker_tail + chunk)[-32768:]
                if FINAL_MARKER_RE.search(marker_tail):
                    stopped_after_result = True
                    os.killpg(process.pid, signal.SIGTERM)
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL)
                    break
            if time.monotonic() - started >= timeout_s:
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
    return (
        process.returncode,
        timed_out,
        stopped_after_result,
        time.monotonic() - started,
        time.monotonic() - last_output,
    )


def git_sha() -> str | None:
    completed = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=REPO_ROOT, text=True,
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, check=False,
    )
    return completed.stdout.strip() if completed.returncode == 0 else None


def summarize_log(log: str, timed_out: bool) -> dict[str, Any]:
    result = parse_last_buildstorm_result(log)
    elapsed: float | None = None
    if result:
        try:
            elapsed = float(result.get("elapsed_s", ""))
        except ValueError:
            pass
    terminals = list(BUILDSTORM_RE.finditer(log))
    terminal_offset = terminals[-1].start() if terminals else len(log)
    panic = PANIC_RE.search(log)
    sigsegv = SIGSEGV_RE.search(log)
    cagent_complete = "#### OS COMP TEST GROUP END cagent-glibc ####" in log
    cagent_passes = len(CAGENT_PASS_RE.findall(log))
    cagent_failed = bool(CAGENT_FAIL_RE.search(log))
    required = {
        "cagent": cagent_complete and cagent_passes >= 10 and not cagent_failed,
        "toolchain": bool(re.search(r"TOOLCHAIN_RESULT\s+status=OK\b", log)),
        "minibuild": bool(re.search(r"MINIBUILD_RESULT\s+status=OK\b", log)),
        "buildstorm": bool(
            result
            and result.get("mode") == "multi"
            and result.get("status") == "OK"
            and result.get("rc") == "0"
            and result.get("run") == "OK"
        ),
    }
    fatal_before_result = bool(
        (panic and panic.start() < terminal_offset) or (sigsegv and sigsegv.start() < terminal_offset)
    )
    passed = all(required.values()) and not timed_out and not fatal_before_result
    return {
        "status": "passed" if passed else "failed",
        "required_markers": required,
        "cagent_passes": cagent_passes,
        "buildstorm": result,
        "guest_elapsed_s": elapsed,
        "performance_decision": performance_decision(elapsed) if passed else "reject_functional_failure",
        "panic": bool(panic),
        "sigsegv": bool(sigsegv),
        "fatal_before_result": fatal_before_result,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=sorted(ARCH_CONFIG), required=True)
    parser.add_argument("--kernel", type=Path, required=True)
    parser.add_argument("--image", type=Path, required=True, help="prepared raw-image master")
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--timeout", type=float, required=True)
    parser.add_argument("--output-root", type=Path, default=DEFAULT_OUTPUT_ROOT)
    parser.add_argument("--qemu-root", type=Path, default=DEFAULT_QEMU_ROOT)
    parser.add_argument("--plugin", choices=("pc-hot", "wait-hot"), action="append", default=[])
    parser.add_argument("--keep-run-image", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", args.run_id):
        parser.error("--run-id must contain only letters, digits, dot, underscore, and dash")
    kernel = args.kernel.resolve()
    master = args.image.resolve()
    for label, path in (("kernel", kernel), ("image", master)):
        if not path.is_file():
            parser.error(f"{label} is not a file: {path}")
    output_dir = (args.output_root / args.run_id).resolve()
    try:
        output_dir.mkdir(parents=True, exist_ok=False)
    except FileExistsError:
        parser.error(f"run output already exists: {output_dir}")
    run_image = output_dir / "run-image.raw"
    command = qemu_argv(args.arch, kernel, run_image, args.qemu_root.resolve())
    qemu = Path(command[0])
    metadata: dict[str, Any] = {
        "schema_version": 1,
        "run_id": args.run_id,
        "arch": args.arch,
        "git_sha": git_sha(),
        "kernel": {"path": str(kernel), "sha256": None},
        "image_master": None,
        "run_image": str(run_image),
        "qemu_version": None,
        "command": command,
        "command_shell": shlex.join(command),
        "timeout_s": args.timeout,
        "diagnostic_plugins": args.plugin,
        "wall_clock_eligible": not args.plugin and not args.dry_run,
        "cpu_affinity": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None,
    }
    result_path = output_dir / "result.json"
    try:
        metadata["kernel"]["sha256"] = sha256_file(kernel)
        metadata["image_master"] = image_metadata(master)
        metadata["qemu_version"] = qemu_version(qemu)
        if args.dry_run:
            metadata["status"] = "dry_run"
            result_path.write_text(
                json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8"
            )
            print(result_path)
            return 0
        create_run_image(master, run_image)
        extra, plugin_outputs = plugin_args(args.arch, output_dir, args.plugin)
        command.extend(extra)
        metadata["command"] = command
        metadata["command_shell"] = shlex.join(command)
        metadata["plugin_outputs"] = plugin_outputs
        serial_log = output_dir / "serial.log"
        metadata["serial_log"] = str(serial_log)
        evict_image_cache(run_image)
        returncode, timed_out, stopped, host_wall, silence = execute(command, serial_log, args.timeout)
        log = serial_log.read_text(encoding="utf-8", errors="replace")
        metadata.update(summarize_log(log, timed_out))
        metadata.update({
            "qemu_exit_code": returncode,
            "timed_out": timed_out,
            "stopped_after_result": stopped,
            "host_wall_s": round(host_wall, 3),
            "last_serial_silence_s": round(silence, 3),
        })
    except (OSError, RuntimeError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        metadata.update({"status": "failed", "error": str(error)})
    finally:
        if run_image.exists() and not args.keep_run_image:
            run_image.unlink()
        result_path.write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(result_path)
    return 0 if metadata.get("status") == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
