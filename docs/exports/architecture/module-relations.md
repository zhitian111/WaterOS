# api / impl 接线关系

按组件列出 **api-v0 契约层** 与 **impl 选择链**，便于查 feature 传递与替换点。来源：`os/feature-tree.txt`、`os/Cargo.toml`、各组件 `Cargo.toml`。

## 接线通则

1. 根 `wateros` 通过 `component/feature` 语法启用子 crate feature（如 `mm/impl-sv39`）。
2. 聚合 `src/lib.rs` 用 `#[cfg(feature = "impl-*")]` 选定 `active_impl`，对外只暴露稳定模块名。
3. `api-v0` crate 通常 **无** `[features]`，被上层 `api-v0` feature 以依赖方式链接。
4. 跨组件依赖优先走聚合门面（`vfs::`、`mm::`、`ipc::`），避免根 crate 直接依赖 `*-impl-*`。

---

## wateros-abi

| 层 | crate | 根 feature 选用 |
|----|-------|-----------------|
| api | `wateros-abi-api-v0` | `api-v0`（经 abi 默认或 syscall 传递） |
| impl | `impl-linux-generic64` | 两 QEMU 主线均启用 |
| impl | `impl-dummy` | 仅占位 |

`syscall-api-v0`、`syscall-impl-kernel` 均依赖 `abi/api-v0`。

---

## wateros-platform

| 子系统 | api | impl（二选一 arch + 二选一 board） |
|--------|-----|-------------------------------------|
| 聚合 | `platform-api-v0` | `impl-qemu-riscv64-opensbi` / `impl-qemu-loongarch64-virt` / `impl-dummy` |
| arch | `platform-arch-api-v0` | `impl-riscv64` / `impl-loongarch64` |
| firmware | `platform-firmware-api-v0` | `impl-riscv64-opensbi` / `impl-qemu-loongarch64-uart16550` |

板级 impl 通过 feature 拉 arch impl 与 `runtime-logging` 的控制台后端（`impl-firmware-opensbi` / `impl-firmware-console`）。

---

## wateros-runtime

无独立 api-v0 聚合 crate；子模块各自分叉：

| 子模块 | 契约 | impl 选项 |
|--------|------|-----------|
| console | `runtime-console-api-v0` | `impl-platform-console`（QEMU 主线）、`impl-firmware-*`、`impl-dummy` |
| logging | 无 api crate | `impl-trace` … `impl-error`（级别） |
| panic | 无 | 随 console + firmware 组合 |
| heap | 无 | `impl-buddy-allocator`（默认）/ TLSF |
| serial | 再导出 driver UART | `serial-uart-virt` |

根主线：RISC-V `runtime/impl-warn`；LoongArch `runtime/impl-error`；均开 `runtime/impl-platform-console`。

---

## wateros-mm

| 层 | crate | feature |
|----|-------|---------|
| api | `wateros-mm-api-v0` | `api-v0`（默认） |
| 页表 | `impl-sv39` / `impl-loongarch64` | 根 `impl-sv39` 或 `mm/impl-loongarch64` |
| 页表桩 | `impl-dummy` | 仅契约编译 |
| 帧分配 | `mm-frame-alloctor` → `impl-stack` | mm 默认 |
| 可选 | `vfs-root-read` | 根 `mm/vfs-root-read`，供用户态读根 |

`syscall-impl-kernel`（`fd-session`）拉 `mm/api-v0`。

---

## wateros-task

| 层 | crate | feature |
|----|-------|---------|
| api | `wateros-task-api-v0` | `api-v0`（默认） |
| TCB/registry | `impl-core` | 默认 |
| 调度 | `task-scheduler` → `impl-multi-class`（默认）或 `impl-round-robin` | 互斥 |

`ipc-waitqueue-impl-task`、`vfs-impl-fd-session`、`cred-impl-root` 均依赖 `task/api-v0` 或 `impl-core`。

---

## wateros-syscall

| 层 | crate | feature |
|----|-------|---------|
| api | `wateros-syscall-api-v0` | `api-v0` |
| 内核实现 | `impl-kernel` | 根 `syscall/impl-kernel` |

`impl-kernel` 子 feature 链：

```text
impl-kernel
├── cred-session → cred/impl-root
├── fd-session → vfs/fd-session + vfs/bridge-fs-api + mm + ipc/pipe
├── impl-riscv64 | impl-loongarch64 → abi + ipc arch + task
└── socket-net → driver-network (smoltcp + virtio)
```

trap 路径：`platform-arch` → 根 `trap_handler` → `syscall::dispatch_syscall_from_trap`。

---

## wateros-vfs

| 层 | crate | 根启用 |
|----|-------|--------|
| api | `wateros-vfs-api-v0` | `api-v0` |
| 后端 | `impl-fs-bridge` | `vfs-bridge` → `bridge-fs-api` |
| fd/cwd | `impl-fd-session` | RISC-V 主线 `fd-session`；LoongArch 经 vfs `impl-loongarch64` |
| 页缓存 | `impl-page-cache` | bridge 默认依赖 |
| 桩 | `impl-dummy` | 无 bridge 时 |

`impl-fs-bridge` 依赖 `wateros-fs`；`impl-fd-session` 依赖 `task` + `ipc/pipe`。

---

## wateros-fs

| 子系统 | api | impl |
|--------|-----|------|
| 聚合 | `fs-api-v0` | — |
| 块 FS | — | `impl-ext4-rs`（默认）/ `impl-ext4` |
| devfs | `fs-devfs-api-v0` | `impl-kernel` |
| procfs | `fs-procfs-api-v0` | `impl-kernel` |
| rootfs | `fs-rootfs-api-v0` | `impl-kernel` |

`driver::init_after_boot` → `fs::devfs::refresh`；`vfs::impl-fs-bridge` 消费 `fs::api` 与 rootfs 句柄。

---

## wateros-driver

| 子系统 | api | 平台 impl 选用 |
|--------|-----|----------------|
| 聚合 | `driver-api-v0` | — |
| block | `driver-block-api-v0` | RISC-V: `impl-virtio-mmio`；LA: `impl-virtio-pci`；共用 `impl-block-cache` |
| character | `character-api-v0` | DTB UART + stub |
| network | `network-api-v0` | RISC-V: `impl-virtio-mmio`；LA: `impl-virtio-pci`；栈 `impl-smoltcp` |
| 板级 | — | `impl-qemu-riscv64-opensbi` / `impl-qemu-loongarch64-virt` |

`syscall/socket-net` 与 `driver-network` 的 `socket_handles` 对接 VFS fd。

---

## wateros-ipc

| 子系统 | api | impl | 根 `ipc/all` |
|--------|-----|------|:------------:|
| 聚合 api | `ipc-api-v0` | `impl-dummy`（占位） | api-v0 |
| waitqueue | `ipc-waitqueue-api-v0` | `impl-task` | 默认 |
| pipe | `ipc-pipe-api-v0` | `impl-ringbuf` + arch waitqueue | pipe |
| futex | `ipc-futex-api-v0` | `impl-task` | futex |
| shm | 单 crate | — | shm |
| signal | `ipc-signal-api-v0` | 逻辑在聚合 crate | signal |

arch feature：`impl-riscv64` / `impl-loongarch64` 传递 pipe 与 waitqueue 的 arch 子 feature。

---

## wateros-cred

| 层 | crate | feature |
|----|-------|---------|
| api | `cred-api-v0` | `api-v0` |
| 运行时 | `impl-root` | 根 `dep:cred` + syscall `cred-session` |

生命周期 hook 由 syscall（fork/clone/exec/reap）调用，不反向依赖 task crate 实现。

---

## wateros-klog

| 层 | crate | feature |
|----|-------|---------|
| api | `klog-api-v0` | `default` |
| 存储 | `impl-ringbuf` | `default` |
| 时间/任务 | — | `platform-timer`、`task-api` |

`syscall::syslog` 写入同一环；与 `runtime-logging` 独立。

---

## 根 feature → 组件速查

| 根 feature | 主要传递 |
|------------|----------|
| `qemu-riscv64-opensbi` | platform/arch/mm/driver/fs/ipc/vfs/cred/syscall/klog 全套 RISC-V 链 |
| `qemu-loongarch64-virt` | 同上 LoongArch 链；日志级别 `impl-error` |
| `vfs-bridge` | `dep:vfs` + `vfs/bridge-fs-api` |
| `impl-sv39` | `mm/impl-sv39`（根 crate cfg 亦可见） |
| `pseudo-shell` | `dep:pseudo_shell` |
| `bringup-ltp-*` | 仅根 cfg，不改组件 impl |

完整展开见 `os/feature-tree.txt`（`make -C os feature-tree` 可再生成）。
