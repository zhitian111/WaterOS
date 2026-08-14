# wateros-platform

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-platform` 是内核访问硬件环境的边界层。它不实现调度、进程、页表内容或设备驱动
策略；它只把“当前 CPU 的 ISA 原语”和“当前机器/固件提供的服务”组合成稳定入口。

## 模块分层


| 层       | 路径                                                                     | 职责                                                                                                     |
| ---------- | -------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| 聚合门面 | `src/lib.rs` 及 `src/{boot,time,smp,wall_clock}.rs`                      | 组合 arch 与 platform-impl，统一入口、时间换算、IPI reason；`timer`/`console`/`reset` 保留多层组合入口。 |
| 平台 API | `platform-api/api-v0/`                                                   | 平台 profile 共用的类型与 trait：boot 参数、时间、SMP、console、timer、reset。                           |
| 架构 API | `platform-arch/arch-api/api-v0/`                                         | ISA 公共的 trap、任务上下文、分页、中断、时间与 kernel_trap 类型。                                       |
| 架构实现 | `platform-arch/arch-impl/impl-{riscv64,loongarch64}/`              | RISC-V / LoongArch 汇编、CSR、本地 interrupt/TLB/trap。                                                  |
| 平台实现 | `platform-impl/impl-{qemu-riscv64-opensbi,qemu-loongarch64-virt}/` | QEMU/固件 profile：boot 参数解释、console、timer、reset、SMP 运输。                                      |

## 实现说明

- 分层归属规则：**更换同一 ISA 的板子仍须修改的，放 `platform-impl`；更换 ISA 才须修改的，
  放 `platform-arch`。**
- 例：RISC-V 的 `sip.SSIP` 清除属于 arch；OpenSBI 的 `send_ipi`、HSM `hart_start` 属于 QEMU
  RISC-V/OpenSBI profile；任务重调度原因属于 `wateros-platform::smp`，不属于任意硬件后端。
- 发送 IPI 与清除本地 pending 位是两个不同职责：前者依赖固件或板级控制器（profile），后者
  是目标 CPU 的 ISA 操作（arch `interrupt::clear_soft_interrupt`）。不要在 `platform-impl`
  中重新实现 `sip`、`sie`、`satp` 等 arch 原语。
- `active_impl` 按构建 feature 选一：`impl-qemu-riscv64-opensbi`、`impl-qemu-loongarch64-virt`
  为 QEMU profile；`impl-jh7110-visionfive2`、`impl-loongson2k1000la` 为板级占位
  （任务 04 骨架，任务 05/09 填充真实实现）；任一构建只启用一个 arch impl 与一个
  platform impl。
- RISC-V QEMU profile 使用 OpenSBI 提供 HSM、IPI、timer 与 reset；LoongArch QEMU profile 的
  mailbox/IPI 运输在 platform profile，本地 IOCSR pending 清除及中断使能在 arch interrupt。
- `init_when_boot(dtb_pa)` 保存平台持有的 DTB 物理指针；`memory::kernel_layout()` 给出
  RAM/MMIO 布局契约（`physical_ram_end_exclusive()` 由它派生），供恒等映射与帧分配器使用。
- `timer` 用 arch tick 与平台 Hz 换算时刻，再经平台后端编程下一次定时器中断；错误变体区分
  `Arch` / `Platform` / `DeadlineTimer` 三层来源。
- `CpuMask`、online mask、IPI reason 和调度决策不属于具体 profile；由聚合层或 `wateros-task`
  管理。

## 调用链路

启动早期：

```text
boot 早期
  -> platform::init_when_boot(dtb_pa)     保存 DTB 物理指针
  -> arch::init()                          安装 trap 向量（开全局中断之前）
  -> memory::kernel_layout()                解析 RAM/MMIO 布局（恒等映射 / 帧分配）
```

SMP 与 IPI：

```text
scheduler 请求远端重调度
  -> platform::smp::send_ipi(mask, Reschedule)
  -> 发布 pending IPI reason（组合层，Release）
  -> profile::smp::send_ipi(mask)（SBI / IOCSR 运输层）

目标 CPU trap
  -> platform::smp::clear_ipi()
  -> arch::interrupt::clear_soft_interrupt()（本地 CSR / IOCSR）
  -> take_pending_ipi()
  -> scheduler 处理 Reschedule / TLB shootdown / TaskNotify
```

定时器：

```text
timer 请求
  -> 用 arch tick + 平台 Hz 换算目标时刻
  -> 经平台后端编程下一次定时器中断
```

## 实现功能

### platform-api / 平台 API

`platform-api/api-v0/src/` 下的文件：

- `boot.rs`：引导参数解释。
- `time.rs`：平台时间频率（`PlatformTime`），不等于 arch `time` CSR 读频率。
- `smp.rs`：`HartStatus`、`IpiKind`、`PlatformSmp` 与 `PlatformSmpResult`。
- `console.rs` / `reset.rs` / `timer.rs`：以具体后端函数提供的平台能力，不为无实现的空 trait
  保留抽象层。
- `lib.rs`：按 `boot`/`console`/`reset`/`smp`/`time`/`timer` 组织平台 API 模块导出。

### platform-arch / 架构层

`arch-api/api-v0/src/` 下的 ISA 公共类型文件：

- `cpu.rs`：当前 CPU 标识与本核早期初始化结果类型。
- `interrupt.rs`：定时器与全局中断在 ISA 层的开关原语。
- `kernel_trap.rs`：组合层 trap 路由（单入口 + 运行期注册，避免 `arch-impl` 直接依赖
  `task`/`syscall`）。
- `paging.rs`：分页 CSR 相关类型。
- `task.rs`：任务初次运行与切换的架构上下文构造。
- `time.rs`：单调时间计数与可选频率查询（不含 `set_timer`/SBI）。
- `trap.rs`：异常与中断 trap 帧、原因解码、syscall ABI 读写接口。
- `lib.rs`：模块聚合。

`arch-impl/impl-riscv64/src/` 下的文件：

- `cpu.rs`：当前 CPU id 查询。
- `interrupt.rs`：`sie`/`sstatus` 级中断开关。
- `ipi.rs`：核间中断——通过 SBI `send_ipi` 向目标 hart 发送 Supervisor Soft Interrupt
  （`send_ipi(cpu_mask)`，错误归一化为 `IpiError`）。
- `paging.rs`：`satp` 读写与 `sfence.vma`。
- `task.rs`：任务上下文与进入桩函数符号。
- `time.rs`：`time` CSR 读 tick；频率查询返回不支持（由 platform 层提供 Hz）。
- `trap.rs`：trap 向量、`TrapContext` 与 `trap_entry_rust`（转入 `arch-api::kernel_trap`）。
- `lib.rs`：汇编入口（`boot.S`/`trap.asm`/`switch.S`）与模块聚合；`init_trap` 安装 `stvec`。

`arch-impl/impl-loongarch64/src/` 下的文件（无独立 `ipi.rs`；IPI 运输在 profile、本地
pending 清除在 `interrupt.rs`）：

- `cpu.rs` / `interrupt.rs`（本地 IOCSR pending 清除与中断使能）/ `paging.rs` / `task.rs` /
  `time.rs` / `trap.rs` / `lib.rs`（`trap.S`/`switch.S`、`TrapContext`、LoongArch64 原因码解码、
  `init_trap`）。

### platform-impl / 机器层

`impl-qemu-riscv64-opensbi/src/` 下的文件：

- `boot.rs`：解释 OpenSBI 启动参数（`a0`/`a1` 分别承载 hart id 与 DTB 物理地址）。
- `console.rs`：OpenSBI console（early console）后端。
- `dtb.rs`：平台持有的引导 DTB 物理指针（`store` / `dtb_pa`）。
- `memory.rs`：QEMU RISC-V 物理内存布局（`kernel_memory_layout` 契约 +
  `physical_ram_end_exclusive` 派生）。
- `reset.rs`：OpenSBI system reset 后端。
- `smp.rs`：SBI HSM（`hart_start`/`hart_get_status`）与 IPI/remote fence 运输
  （`QemuRiscv64OpenSbiSmp`）。
- `time.rs`：QEMU RISC-V timebase-frequency fallback。
- `timer.rs`：OpenSBI timer 后端（经 SBI 设置下次中断时刻）。
- `lib.rs`：模块聚合与 `asm/_start.S` 平台 shim。

`impl-qemu-loongarch64-virt/src/` 下的文件（结构相同）：

- `boot.rs` / `console.rs` / `dtb.rs` / `memory.rs` / `reset.rs` / `smp.rs`（mailbox/IPI 运输）
  / `time.rs` / `timer.rs` / `lib.rs`。


### 聚合门面 / src/

- `lib.rs`：`active_impl` 选择；`init_when_boot` / `dtb_pa` / `memory::kernel_layout`
  （及派生的 `physical_ram_end_exclusive`）；`timer`/`console`/`reset` 组合入口；
  `arch` 再导出与 `arch::init()`。
- `boot.rs`：启动参数与引导上下文。
- `time.rs`：平台时间频率注入（`set_frequency_hz`）与回退。
- `smp.rs`：跨架构通用的待处理 IPI reason（`PENDING_IPI`）与 `send_ipi`/`clear_ipi`/
  `take_pending_ipi`。
- `wall_clock.rs`：墙上时钟相关。

### 添加新平台 profile

1. 复用已有 ISA 的 `platform-arch/arch-impl`；除非 CPU 指令集不同，不新增 arch 实现。
2. 新建 `platform-impl/impl-<machine>`，按 `boot`、`console`、`timer`、`reset`、`smp` 分文件
   实现机器相关后端。
3. 在根 `Cargo.toml` 和 `wateros-platform` feature 中选择该 profile，确保任一构建只启用一个
   arch impl 与一个 platform impl。
4. 至少检查 boot、timer、IPI 的错误路径。SMP profile 还必须验证 AP online 之前不会被
   scheduler 当成可投递目标。
