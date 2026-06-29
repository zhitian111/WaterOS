#!/usr/bin/env bash
# 按 os/components 目录树生成第 3 章 LaTeX 占位文件。
# 约定：
#   - 子模块目录名与 crate 路径一致
#   - 聚合文件用短名（如 ipc.tex、waitqueue.tex），不用 index.tex
#   - 叶子：api/api-v0.tex、impl/<variant>.tex
#   - 无 api/impl 分叉的叶子 crate 仅保留 <short>.tex

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)/chapters/chap03/components"
mkdir -p "$ROOT"

write_leaf() {
  local f="$1" title="$2"
  mkdir -p "$(dirname "$f")"
  [[ -f "$f" ]] && return 0
  cat >"$f" <<EOF
% $title
% TODO：事实来源见对应 Cargo.toml、src/lib.rs 与 docs/exports/

EOF
}

write_agg() {
  local f="$1" title="$2" heading="$3"
  shift 3
  mkdir -p "$(dirname "$f")"
  [[ -f "$f" ]] && return 0
  {
    echo "% $title"
    [[ -n "$heading" ]] && echo "\\$heading"
    echo "% TODO"
    for inc in "$@"; do echo "\\input{$inc}"; done
    echo
  } >"$f"
}

P="chapters/chap03/components"

# --- wateros-abi ---
D="$ROOT/wateros-abi"
write_leaf "$D/abi-api/api/api-v0.tex" "abi-api-v0"
write_leaf "$D/abi-impl/impl/dummy.tex" "abi-impl-dummy"
write_leaf "$D/abi-impl/impl/linux-generic64.tex" "abi-impl-linux-generic64"
write_agg "$D/abi-api/abi-api.tex" "abi-api" "subsection{abi-api}" \
  "$P/wateros-abi/abi-api/api/api-v0"
write_agg "$D/abi-impl/abi-impl.tex" "abi-impl" "subsection{abi-impl}" \
  "$P/wateros-abi/abi-impl/impl/dummy" "$P/wateros-abi/abi-impl/impl/linux-generic64"
write_agg "$D/abi.tex" "wateros-abi" "section{wateros-abi}" \
  "$P/wateros-abi/abi-api/abi-api" "$P/wateros-abi/abi-impl/abi-impl"

# --- wateros-base ---
D="$ROOT/wateros-base"
write_leaf "$D/base-config/base-config.tex" "base-config"
write_agg "$D/base-config/base-config.tex" "base-config" "subsection{base-config}"
write_agg "$D/base.tex" "wateros-base" "section{wateros-base}" \
  "$P/wateros-base/base-config/base-config"

# --- wateros-cred ---
D="$ROOT/wateros-cred"
write_leaf "$D/cred-api/api/api-v0.tex" "cred-api-v0"
write_leaf "$D/cred-impl/impl/root.tex" "cred-impl-root"
write_agg "$D/cred-api/cred-api.tex" "cred-api" "subsection{cred-api}" "$P/wateros-cred/cred-api/api/api-v0"
write_agg "$D/cred-impl/cred-impl.tex" "cred-impl" "subsection{cred-impl}" "$P/wateros-cred/cred-impl/impl/root"
write_agg "$D/cred.tex" "wateros-cred" "section{wateros-cred}" \
  "$P/wateros-cred/cred-api/cred-api" "$P/wateros-cred/cred-impl/cred-impl"

# --- wateros-driver ---
D="$ROOT/wateros-driver"
write_leaf "$D/driver-api/api/api-v0.tex" "driver-api-v0"
for impl in dummy qemu-loongarch64-virt qemu-riscv64-opensbi; do
  write_leaf "$D/driver-impl/impl/${impl}.tex" "driver-impl-${impl}"
done
write_agg "$D/driver-impl/driver-impl.tex" "driver-impl" "subsection{driver-impl}" \
  "$P/wateros-driver/driver-impl/impl/dummy" \
  "$P/wateros-driver/driver-impl/impl/qemu-loongarch64-virt" \
  "$P/wateros-driver/driver-impl/impl/qemu-riscv64-opensbi"

driver_sub() {
  local sub="$1" short="$2"
  write_leaf "$D/driver-${sub}/${short}-api/api/api-v0.tex" "driver-${sub}-api-v0"
  local impl_inputs=()
  case "$sub" in
    block)     variants=(block-cache dummy virtio-mmio virtio-pci) ;;
    character) variants=(dummy null-stub rtc-stub) ;;
    network)   variants=(dummy smoltcp virtio-mmio virtio-pci) ;;
  esac
  for v in "${variants[@]}"; do
    write_leaf "$D/driver-${sub}/${short}-impl/impl/${v}.tex" "${short}-impl-${v}"
    impl_inputs+=("$P/wateros-driver/driver-${sub}/${short}-impl/impl/${v}")
  done
  write_agg "$D/driver-${sub}/${short}.tex" "driver-${sub}" "subsection{driver-${sub}}" \
    "$P/wateros-driver/driver-${sub}/${short}-api/api/api-v0" "${impl_inputs[@]}"
}
driver_sub block block
driver_sub character character
driver_sub network network

write_agg "$D/driver-api/driver-api.tex" "driver-api" "subsection{driver-api}" "$P/wateros-driver/driver-api/api/api-v0"
write_agg "$D/driver.tex" "wateros-driver" "section{wateros-driver}" \
  "$P/wateros-driver/driver-api/driver-api" \
  "$P/wateros-driver/driver-block/block" \
  "$P/wateros-driver/driver-character/character" \
  "$P/wateros-driver/driver-network/network" \
  "$P/wateros-driver/driver-impl/driver-impl"

# --- wateros-fs ---
D="$ROOT/wateros-fs"
write_leaf "$D/fs-api/api/api-v0.tex" "fs-api-v0"
for impl in devfs dummy ext4 ext4-rs; do write_leaf "$D/fs-impl/impl/${impl}.tex" "fs-impl-${impl}"; done
write_agg "$D/fs-impl/fs-impl.tex" "fs-impl" "subsection{fs-impl}" \
  "$P/wateros-fs/fs-impl/impl/devfs" "$P/wateros-fs/fs-impl/impl/dummy" \
  "$P/wateros-fs/fs-impl/impl/ext4" "$P/wateros-fs/fs-impl/impl/ext4-rs"

fs_sub() {
  local sub="$1"
  write_leaf "$D/fs-${sub}/${sub}-api/api/api-v0.tex" "fs-${sub}-api-v0"
  for v in dummy kernel; do write_leaf "$D/fs-${sub}/${sub}-impl/impl/${v}.tex" "fs-${sub}-impl-${v}"; done
  write_agg "$D/fs-${sub}/${sub}.tex" "fs-${sub}" "subsection{fs-${sub}}" \
    "$P/wateros-fs/fs-${sub}/${sub}-api/api/api-v0" \
    "$P/wateros-fs/fs-${sub}/${sub}-impl/impl/dummy" \
    "$P/wateros-fs/fs-${sub}/${sub}-impl/impl/kernel"
}
fs_sub devfs; fs_sub procfs; fs_sub rootfs

write_agg "$D/fs-api/fs-api.tex" "fs-api" "subsection{fs-api}" "$P/wateros-fs/fs-api/api/api-v0"
write_agg "$D/fs.tex" "wateros-fs" "section{wateros-fs}" \
  "$P/wateros-fs/fs-api/fs-api" "$P/wateros-fs/fs-devfs/devfs" \
  "$P/wateros-fs/fs-procfs/procfs" "$P/wateros-fs/fs-rootfs/rootfs" "$P/wateros-fs/fs-impl/fs-impl"

# --- wateros-ipc ---
D="$ROOT/wateros-ipc"
write_leaf "$D/ipc-api/api/api-v0.tex" "ipc-api-v0"
write_leaf "$D/ipc-impl/impl/dummy.tex" "ipc-impl-dummy"
write_agg "$D/ipc-impl/ipc-impl.tex" "ipc-impl" "subsection{ipc-impl}" "$P/wateros-ipc/ipc-impl/impl/dummy"
write_agg "$D/ipc-event/event.tex" "ipc-event" "subsection{ipc-event}"
write_agg "$D/ipc-shm/shm.tex" "ipc-shm" "subsection{ipc-shm}"

write_leaf "$D/ipc-waitqueue/waitqueue-api/api/api-v0.tex" "waitqueue-api-v0"
write_leaf "$D/ipc-waitqueue/waitqueue-impl/impl/task.tex" "waitqueue-impl-task"
write_agg "$D/ipc-waitqueue/waitqueue.tex" "ipc-waitqueue" "subsection{ipc-waitqueue}" \
  "$P/wateros-ipc/ipc-waitqueue/waitqueue-api/api/api-v0" \
  "$P/wateros-ipc/ipc-waitqueue/waitqueue-impl/impl/task"

write_leaf "$D/ipc-pipe/pipe-api/api/api-v0.tex" "pipe-api-v0"
write_leaf "$D/ipc-pipe/pipe-impl/impl/ringbuf.tex" "pipe-impl-ringbuf"
write_agg "$D/ipc-pipe/pipe.tex" "ipc-pipe" "subsection{ipc-pipe}" \
  "$P/wateros-ipc/ipc-pipe/pipe-api/api/api-v0" "$P/wateros-ipc/ipc-pipe/pipe-impl/impl/ringbuf"

write_leaf "$D/ipc-futex/futex-api/api/api-v0.tex" "futex-api-v0"
for v in dummy task; do write_leaf "$D/ipc-futex/futex-impl/impl/${v}.tex" "futex-impl-${v}"; done
write_agg "$D/ipc-futex/futex.tex" "ipc-futex" "subsection{ipc-futex}" \
  "$P/wateros-ipc/ipc-futex/futex-api/api/api-v0" \
  "$P/wateros-ipc/ipc-futex/futex-impl/impl/dummy" "$P/wateros-ipc/ipc-futex/futex-impl/impl/task"

write_leaf "$D/ipc-signal/signal-api/api/api-v0.tex" "signal-api-v0"
write_leaf "$D/ipc-signal/signal-impl/impl/dummy.tex" "signal-impl-dummy"
write_agg "$D/ipc-signal/signal.tex" "ipc-signal" "subsection{ipc-signal}" \
  "$P/wateros-ipc/ipc-signal/signal-api/api/api-v0" "$P/wateros-ipc/ipc-signal/signal-impl/impl/dummy"

write_agg "$D/ipc-api/ipc-api.tex" "ipc-api" "subsection{ipc-api}" "$P/wateros-ipc/ipc-api/api/api-v0"
write_agg "$D/ipc.tex" "wateros-ipc" "section{wateros-ipc}" \
  "$P/wateros-ipc/ipc-api/ipc-api" "$P/wateros-ipc/ipc-waitqueue/waitqueue" \
  "$P/wateros-ipc/ipc-pipe/pipe" "$P/wateros-ipc/ipc-futex/futex" \
  "$P/wateros-ipc/ipc-shm/shm" "$P/wateros-ipc/ipc-signal/signal" \
  "$P/wateros-ipc/ipc-event/event" "$P/wateros-ipc/ipc-impl/ipc-impl"

# --- wateros-klog ---
D="$ROOT/wateros-klog"
write_leaf "$D/klog-api/api/api-v0.tex" "klog-api-v0"
write_leaf "$D/klog-impl/impl/ringbuf.tex" "klog-ringbuf"
write_agg "$D/klog-api/klog-api.tex" "klog-api" "subsection{klog-api}" "$P/wateros-klog/klog-api/api/api-v0"
write_agg "$D/klog-impl/klog-impl.tex" "klog-impl" "subsection{klog-impl}" "$P/wateros-klog/klog-impl/impl/ringbuf"
write_agg "$D/klog.tex" "wateros-klog" "section{wateros-klog}" \
  "$P/wateros-klog/klog-api/klog-api" "$P/wateros-klog/klog-impl/klog-impl"

# --- wateros-mm ---
D="$ROOT/wateros-mm"
write_leaf "$D/mm-api/api/api-v0.tex" "mm-api-v0"
write_leaf "$D/mm-impl/common/common.tex" "mm-impl-common"
for impl in dummy loongarch64 sv39; do write_leaf "$D/mm-impl/impl/${impl}.tex" "mm-impl-${impl}"; done
write_agg "$D/mm-impl/mm-impl.tex" "mm-impl" "subsection{mm-impl}" \
  "$P/wateros-mm/mm-impl/common/common" \
  "$P/wateros-mm/mm-impl/impl/dummy" "$P/wateros-mm/mm-impl/impl/loongarch64" \
  "$P/wateros-mm/mm-impl/impl/sv39"

write_leaf "$D/mm-frame-alloctor/frame-alloctor-api/api/api-v0.tex" "frame-alloctor-api-v0"
for v in dummy stack; do write_leaf "$D/mm-frame-alloctor/frame-alloctor-impl/impl/${v}.tex" "frame-alloctor-impl-${v}"; done
write_agg "$D/mm-frame-alloctor/frame-alloctor.tex" "mm-frame-alloctor" "subsection{mm-frame-alloctor}" \
  "$P/wateros-mm/mm-frame-alloctor/frame-alloctor-api/api/api-v0" \
  "$P/wateros-mm/mm-frame-alloctor/frame-alloctor-impl/impl/dummy" \
  "$P/wateros-mm/mm-frame-alloctor/frame-alloctor-impl/impl/stack"

write_agg "$D/mm-api/mm-api.tex" "mm-api" "subsection{mm-api}" "$P/wateros-mm/mm-api/api/api-v0"
write_agg "$D/mm.tex" "wateros-mm" "section{wateros-mm}" \
  "$P/wateros-mm/mm-api/mm-api" "$P/wateros-mm/mm-frame-alloctor/frame-alloctor" \
  "$P/wateros-mm/mm-impl/mm-impl"

# --- wateros-platform ---
D="$ROOT/wateros-platform"
write_leaf "$D/platform-api/api/api-v0.tex" "platform-api-v0"
for impl in dummy qemu-loongarch64-virt qemu-riscv64-opensbi; do
  write_leaf "$D/platform-impl/impl/${impl}.tex" "platform-impl-${impl}"
done
write_agg "$D/platform-impl/platform-impl.tex" "platform-impl" "subsection{platform-impl}" \
  "$P/wateros-platform/platform-impl/impl/dummy" \
  "$P/wateros-platform/platform-impl/impl/qemu-loongarch64-virt" \
  "$P/wateros-platform/platform-impl/impl/qemu-riscv64-opensbi"

write_leaf "$D/platform-arch/arch-api/api/api-v0.tex" "platform-arch-api-v0"
for v in dummy loongarch64 riscv64; do write_leaf "$D/platform-arch/arch-impl/impl/${v}.tex" "arch-impl-${v}"; done
write_agg "$D/platform-arch/arch.tex" "platform-arch" "subsection{platform-arch}" \
  "$P/wateros-platform/platform-arch/arch-api/api/api-v0" \
  "$P/wateros-platform/platform-arch/arch-impl/impl/dummy" \
  "$P/wateros-platform/platform-arch/arch-impl/impl/loongarch64" \
  "$P/wateros-platform/platform-arch/arch-impl/impl/riscv64"

write_agg "$D/platform-api/platform-api.tex" "platform-api" "subsection{platform-api}" \
  "$P/wateros-platform/platform-api/api/api-v0"
write_agg "$D/platform.tex" "wateros-platform" "section{wateros-platform}" \
  "$P/wateros-platform/platform-api/platform-api" \
  "$P/wateros-platform/platform-arch/arch" \
  "$P/wateros-platform/platform-impl/platform-impl"

# --- wateros-pseudo-shell ---
write_agg "$ROOT/wateros-pseudo-shell/pseudo-shell.tex" "wateros-pseudo-shell" \
  "section{wateros-pseudo-shell}"

# --- wateros-runtime ---
D="$ROOT/wateros-runtime"
write_leaf "$D/runtime-console/console-api/api/api-v0.tex" "runtime-console-api-v0"
for v in dummy platform-console; do write_leaf "$D/runtime-console/console-impl/impl/${v}.tex" "console-impl-${v}"; done
write_agg "$D/runtime-console/console.tex" "runtime-console" "subsection{runtime-console}" \
  "$P/wateros-runtime/runtime-console/console-api/api/api-v0" \
  "$P/wateros-runtime/runtime-console/console-impl/impl/dummy" \
  "$P/wateros-runtime/runtime-console/console-impl/impl/platform-console"
for sub in heap-allocator logging panic serial; do
  write_agg "$D/runtime-${sub}/${sub}.tex" "runtime-${sub}" "subsection{runtime-${sub}}"
done
write_agg "$D/runtime.tex" "wateros-runtime" "section{wateros-runtime}" \
  "$P/wateros-runtime/runtime-console/console" \
  "$P/wateros-runtime/runtime-heap-allocator/heap-allocator" \
  "$P/wateros-runtime/runtime-logging/logging" \
  "$P/wateros-runtime/runtime-panic/panic" \
  "$P/wateros-runtime/runtime-serial/serial"

# --- wateros-syscall ---
D="$ROOT/wateros-syscall"
write_leaf "$D/syscall-api/api/api-v0.tex" "syscall-api-v0"
write_leaf "$D/syscall-impl/impl/kernel.tex" "syscall-impl-kernel"
write_agg "$D/syscall-api/syscall-api.tex" "syscall-api" "subsection{syscall-api}" \
  "$P/wateros-syscall/syscall-api/api/api-v0"
write_agg "$D/syscall-impl/syscall-impl.tex" "syscall-impl" "subsection{syscall-impl}" \
  "$P/wateros-syscall/syscall-impl/impl/kernel"
write_agg "$D/syscall.tex" "wateros-syscall" "section{wateros-syscall}" \
  "$P/wateros-syscall/syscall-api/syscall-api" "$P/wateros-syscall/syscall-impl/syscall-impl"

# --- wateros-task ---
D="$ROOT/wateros-task"
write_leaf "$D/task-api/api/api-v0.tex" "task-api-v0"
write_leaf "$D/task-impl/impl/core.tex" "task-impl-core"
write_leaf "$D/task-scheduler/scheduler-api/api/api-v0.tex" "scheduler-api-v0"
for v in multi-class round-robin; do write_leaf "$D/task-scheduler/scheduler-impl/impl/${v}.tex" "scheduler-impl-${v}"; done
write_agg "$D/task-scheduler/scheduler.tex" "task-scheduler" "subsection{task-scheduler}" \
  "$P/wateros-task/task-scheduler/scheduler-api/api/api-v0" \
  "$P/wateros-task/task-scheduler/scheduler-impl/impl/multi-class" \
  "$P/wateros-task/task-scheduler/scheduler-impl/impl/round-robin"
write_agg "$D/task-api/task-api.tex" "task-api" "subsection{task-api}" "$P/wateros-task/task-api/api/api-v0"
write_agg "$D/task-impl/task-impl.tex" "task-impl" "subsection{task-impl}" "$P/wateros-task/task-impl/impl/core"
write_agg "$D/task.tex" "wateros-task" "section{wateros-task}" \
  "$P/wateros-task/task-api/task-api" "$P/wateros-task/task-impl/task-impl" \
  "$P/wateros-task/task-scheduler/scheduler"

# --- wateros-utils ---
write_agg "$ROOT/wateros-utils/utils.tex" "wateros-utils" "section{wateros-utils}"

# --- wateros-vfs ---
D="$ROOT/wateros-vfs"
write_leaf "$D/vfs-api/api/api-v0.tex" "vfs-api-v0"
for impl in dummy fd-session fs-bridge page-cache; do write_leaf "$D/vfs-impl/impl/${impl}.tex" "vfs-impl-${impl}"; done
write_agg "$D/vfs-api/vfs-api.tex" "vfs-api" "subsection{vfs-api}" "$P/wateros-vfs/vfs-api/api/api-v0"
write_agg "$D/vfs-impl/vfs-impl.tex" "vfs-impl" "subsection{vfs-impl}" \
  "$P/wateros-vfs/vfs-impl/impl/dummy" "$P/wateros-vfs/vfs-impl/impl/fd-session" \
  "$P/wateros-vfs/vfs-impl/impl/fs-bridge" "$P/wateros-vfs/vfs-impl/impl/page-cache"
write_agg "$D/vfs.tex" "wateros-vfs" "section{wateros-vfs}" \
  "$P/wateros-vfs/vfs-api/vfs-api" "$P/wateros-vfs/vfs-impl/vfs-impl"

echo "Component skeleton generated under $ROOT"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
python3 "$SCRIPT_DIR/annotate-tex-files.py"
