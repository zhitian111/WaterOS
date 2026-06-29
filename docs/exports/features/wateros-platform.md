# wateros-platform — 已实现功能快照

## 用途

记录 `wateros-platform` 一级组件当前已落地能力、feature 组合与已知缺口。事实来源：`os/components/wateros-platform/**` 源码与各 `Cargo.toml`。

## 子 crate 与职责

| 子 crate | 职责 | 状态 |
|----------|------|------|
| `wateros-platform`（聚合） | 组合 `arch` + `platform-impl`，提供 `timer`/`time`/`console`/`reset`/`wall_clock` | 已实现 |
| `wateros-platform-api-v0` | 板级契约：`PlatformBoot*`、`PlatformTime`、`PlatformConsole`、`PlatformReset`、`PlatformDeadlineTimer` | 已实现 |
| `wateros-platform-arch` | ISA 聚合：`arch_boot`、`time`、`task`、`trap`、`interrupt`、`paging` | 已实现 |
| `wateros-platform-arch-api-v0` | 架构契约：trap 帧 trait、`ArchTime`、`kernel_trap` 路由 | 已实现 |
| `arch-impl-riscv64` | RISC-V trap 向量、`TrapContext`、`switch.S`/`trap.asm`、Sv39 分页原语 | 已实现 |
| `arch-impl-loongarch64` | LoongArch64 trap、`TrapContext`、`switch.S`/`trap.S`、PGDL 分页原语 | 已实现 |
| `arch-impl-dummy` | 占位算术样例，非生产路径 | 已实现（占位） |
| `platform-impl-qemu-riscv64-opensbi` | OpenSBI console/timer/reset、hart+DTB 引导 | 已实现 |
| `platform-impl-qemu-loongarch64-virt` | MMIO UART、Constant Timer、ACPI GED reset | 已实现 |
| `platform-impl-dummy` | 全桩 platform impl | 已实现（占位） |

## Feature 矩阵（聚合层）

| Feature | 效果 |
|---------|------|
| `api-v0` | 启用平台与架构 API v0，向下传递子 crate |
| `default` | `["api-v0"]` |
| `impl-dummy` | 板级占位 impl |
| `impl-qemu-loongarch64-virt` | `api-v0` + `arch/impl-loongarch64` + LoongArch QEMU 板级 |
| `impl-qemu-riscv64-opensbi` | `api-v0` + `arch/impl-riscv64` + RISC-V QEMU + OpenSBI |

`platform-arch` 禁止同时启用 `impl-riscv64` 与 `impl-loongarch64`。

## 已实现能力

### 聚合层组合语义

- **`timer`**：arch `read_time_tick` + 平台 `frequency_hz` 换算 `Duration`，经 `platform-impl` 编程 deadline 中断。
- **`time`**：引导期 `set_frequency_hz` 覆盖 DTB 探测；未设置时回退 `PlatformTimeImpl::time_frequency_hz`。
- **`wall_clock`**：单调时钟纳秒 + `CLOCK_REALTIME` 偏移；`RtcTimeFields` 与 Linux `rtc_time` 互转。
- **`arch::init`**：调用 `arch_boot()` 安装 trap 向量。

### 架构层（按 feature 二选一）

- **Trap**：`__alltraps` 汇编快照 → `trap_entry_rust` → `kernel_trap::invoke_kernel_trap_handler`；组合层注册 handler。
- **任务切换**：`__switch` 保存/恢复 `ArchTaskContext`。
- **中断**：定时器/全局中断开关、`wait_for_interrupt`。
- **分页**：地址空间 token 激活、TLB 刷新、MMU 开关（LoongArch `CRMD.PG`）。
- **信号帧**：`SignalFrameCodec` 在 RISC-V/LoongArch trap 帧上实现捕获与恢复。

### 板级层

- **RISC-V QEMU**：OpenSBI 串口、`system_reset`、SBI timer；timebase 默认 10 MHz。
- **LoongArch QEMU**：16550 MMIO UART、Constant Timer（默认 100 MHz）、ACPI GED 关机/重启。

## 缺口与后续

- `arch::time::read_time_frequency` 在真实 ISA 上常返回 `Unsupported`，频率依赖 platform 层或引导注入。
- `impl-dummy` / `arch-impl-dummy` 不可启动内核，仅编译占位。
- RISC-V FPU 上下文切换未完整，`sstatus.FS` 在返回用户态时保持脏位。
- LoongArch 无 RISC-V `SUM` 类用户页访问准备，`prepare_user_trap_frame_access` 为空操作。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出（注释/inline 任务同步） |
