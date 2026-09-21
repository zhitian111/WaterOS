# 20 AP 上线前完成机器拓扑发现（修真机 AP PLIC NotFound）

## 任务内容

修复真机 AP 2/3/4 上线 panic：`[smp] AP init_current_cpu failed cpu=2/3/4:
NotFound`。

根因：AP 在 `ap_main` 中调用 `driver::machine().init_current_cpu` →
`irq::initialize_current_hart` → `topology::with_topology`；而板级拓扑
（含 PLIC 描述）由 BSP 的 `init_services_after_boot` 在 **AP 全部上线之后**
才发现并存储。AP 上线时拓扑尚不存在，一律 `NotFound`。QEMU virt 的
`init_current_cpu` 是默认 no-op，因此只有 JH7110 暴露该时序问题。

## 实施方案

在共享 RISC-V64 OpenSBI 入口的 BSP 路径中，`init_after_boot(...)` 之后、
`AP_BOOT_READY.store(true)`（放行 AP）**之前**先调用一次
`driver::machine().init_after_boot()`，由 BSP 完成 DTB 拓扑发现与存储。
AP 自旋等待 `AP_BOOT_READY`，放行后必然能看到拓扑。

两个平台实现的 `init_after_boot` 均幂等（AtomicBool 守卫，失败重置），
`init_services_after_boot` 的后续调用自动成为 no-op，QEMU 行为不变。

## 涉及文件 / CodeGraph 查询

- `os/src/main.rs`（`riscv64_opensbi_entry::wateros_kernel_main`）

CodeGraph：

```bash
codegraph explore "init_services_after_boot"
codegraph explore "initialize_current_hart"
codegraph explore "with_topology"
```

## 验收方式

- [ ] `make jh7110_check` / `make rv_check` 通过。
- [ ] QEMU virt 回归无变化（早调用幂等，后续路径相同）。
- [ ] 真机 AP 2/3/4 完成 PLIC 初始化并 online，日志出现
      `enabled supervisor external interrupts hart=2/3/4 context=...`。

## 验收命令

```bash
cd os
make jh7110_check
make rv_check
make jh7110_uimage && make jh7110_bootdir
cd ../user && make disk ARCH=rv PACKAGE=minimal IMAGE_SIZE_MB=64 \
  DISK_SIZE_MB=192 BOOT_DIR=../os/build/jh7110-boot BOOT_SIZE_MB=64
cd ../os && make run ARCH=rv PROFILE=pre SDCARD=../user/build/images/wateros-rv.img
git diff --check
```

## 验证环境

- L0 宿主机：check/构建。✅
- L1 QEMU virt：回归。✅
- L3 真机：AP 2/3/4 上线（本次真机已复现 AP NotFound）。🔴→✅

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `os/src/main.rs` `riscv64_opensbi_entry::wateros_kernel_main`：在
    `init_after_boot(...)` 之后、`AP_BOOT_READY.store(true)` 之前先调用
    `driver::machine().init_after_boot()`，由 BSP 完成 DTB 拓扑（PLIC 等）
    发现与存储后再放行 AP。
- 验收结果：
  - `make jh7110_check` / `make rv_check`：通过。
  - QEMU virt 回归：`init_after_boot complete` → `/dev/vda4` → login
    全链路通过；早调用幂等，后续 `init_services_after_boot` 走 no-op。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期 AP 2/3/4 `enabled supervisor external interrupts hart=2/3/4
    context=...` 并 online，BSP 的 PLIC 初始化随后也通过；
  - 多核串口交叠、MMC fail-closed 仍为已知后续项。
