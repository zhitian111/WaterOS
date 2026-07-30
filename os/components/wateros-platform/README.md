# wateros-platform

`wateros-platform` 是内核访问硬件环境的边界层。它不实现调度、进程、页表内容或
设备驱动策略；它只把“当前 CPU 的 ISA 原语”和“当前机器/固件提供的服务”组合成
稳定入口。

## 分层

```text
task / mm / syscall / runtime
            │
            ▼
    wateros-platform                 组合层：统一入口、时间换算、IPI reason
       ├── platform-arch             ISA 层：CSR、汇编、trap、TLB、本地中断位
       └── platform-impl             机器层：固件、QEMU 板型、SBI、MMIO、DTB
```

判断一个实现应属于哪层的规则：**更换同一 ISA 的板子仍须修改的，放
`platform-impl`；更换 ISA 才须修改的，放 `platform-arch`。**

例如，RISC-V 的 `sip.SSIP` 清除属于 arch；OpenSBI 的 `send_ipi`、HSM
`hart_start` 属于 QEMU RISC-V/OpenSBI profile；任务重调度原因则属于上层的
`wateros-platform::smp`，不属于任意硬件后端。

## 目录与职责


| 位置                            | 责任                                                              | 不应包含                    |
| --------------------------------- | ------------------------------------------------------------------- | ----------------------------- |
| `platform-api/api-v0`           | 平台 profile 共用的类型与必要 trait：boot 参数、时间、SMP 错误    | CSR、SBI 调用、QEMU 地址    |
| `platform-arch/arch-api/api-v0` | ISA 公共的 trap、任务上下文、分页和中断类型                       | 固件 ABI、设备地址          |
| `platform-arch/arch-impl/*`     | RISC-V / LoongArch 汇编、CSR、本地 interrupt/TLB/trap             | OpenSBI、virtio、调度策略   |
| `platform-impl/impl-*`          | QEMU/固件 profile：boot 参数解释、console、timer、reset、SMP 运输 | trap frame、通用 CSR 语义   |
| `src/`                          | 对内核公开的聚合入口                                              | 具体板子地址或进程/调度状态 |

`src/lib.rs` 保留需要组合多层能力的 `timer`、`console`、`reset` 等入口；较独立的
`boot`、`time`、`smp` 已各自位于同名文件。

## SMP 与 IPI 路径

```text
scheduler 请求远端重调度
  → platform::smp::send_ipi(mask, Reschedule)
  → 发布 pending IPI reason（组合层）
  → profile::smp::send_ipi(mask)（SBI / IOCSR 运输层）

目标 CPU trap
  → platform::smp::clear_ipi()
  → arch::interrupt::clear_soft_interrupt()（本地 CSR / IOCSR）
  → take_pending_ipi()
  → scheduler 处理 Reschedule / TLB shootdown / TaskNotify
```

发送 IPI 与清除本地 pending 位是两个不同职责：前者依赖固件或板级控制器，后者
是目标 CPU 的 ISA 操作。不要在 `platform-impl` 中重新实现 `sip`、`sie`、`satp`
等 arch 原语。

## 添加新平台 profile

1. 复用已有 ISA 的 `platform-arch/arch-impl`；除非 CPU 指令集不同，不新增 arch
   实现。
2. 新建 `platform-impl/impl-<machine>`，按 `boot`、`console`、`timer`、`reset`、
   `smp` 分文件实现机器相关后端。
3. 在根 `Cargo.toml` 和 `wateros-platform` feature 中选择该 profile，确保任一构建
   只启用一个 arch impl 与一个 platform impl。
4. 至少检查 boot、timer、IPI 的错误路径。SMP profile 还必须验证 AP online 之前
   不会被 scheduler 当成可投递目标。

## 当前边界

- RISC-V QEMU profile 使用 OpenSBI 提供 HSM、IPI、timer 与 reset。
- LoongArch QEMU profile 的 mailbox/IPI 运输逻辑仍在 platform profile；本地 IOCSR
  pending 清除及中断使能在 arch interrupt。
- `CpuMask`、online mask、IPI reason 和调度决策不属于该目录的具体 profile；它们由
  聚合层或 `wateros-task` 管理。
