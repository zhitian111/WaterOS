# Platform Architecture 聚合层手册

[Platform 总览](../README.md) · [Arch API](arch-api/api-v0/README.md)

该 crate 把版本化 ISA API 和恰好一个具体实现组合成内核统一的 `arch::*` 门面。上层无需
到处写 `cfg(target_arch)`，但必须通过正确 feature 选择与最终目标一致的实现。

## 1. feature 真值表

| feature | 结果 |
|---|---|
| 默认 | `api-v0 + impl-riscv64` |
| `api-v0 + impl-riscv64` | 完整 RV 门面 |
| `api-v0 + impl-loongarch64` | 完整 LA 门面 |
| 两个 impl 同时启用 | `compile_error!` |
| 只有 `api-v0` | 类型契约可见，但调用 `Arch*Impl` 的函数无法编译 |
| impl 没有 `api-v0` | 大部分公共模块不完整，不是受支持的内核组合 |

Cargo feature 会在依赖图中取并集。若某个依赖偷偷保留 default features，LA 构建可能把
RV impl 也启用并触发互斥错误；所有中间聚合 crate 都应 `default-features=false` 后显式
透传当前架构。

裸 `cargo check` 使用默认 RV，不代表 LA 正确；裸 `--no-default-features --features api-v0`
也会因为没有 Active impl 失败。以顶层 Makefile 的架构 feature 组合为权威。

## 2. 公共门面

```text
platform_arch
  ├─ arch_boot()
  ├─ cpu::{current_cpu_id, init_current_cpu}
  ├─ time::{read_time_tick, read_time_frequency}
  ├─ interrupt::{timer/global/soft enable, save/restore, clear, wfi}
  ├─ task::ActiveArchTaskContext
  ├─ trap::{ActiveTrapFrame, access preparation, slice ticks, ...}
  ├─ paging::{token, ASID init, local flush, MMU mode}
  └─ ipi::send_ipi
```

门面函数通常只是静态分发，不增加锁、校验或错误转换。安全前置条件仍来自实现和调用者。

## 3. `arch_boot` 和 trap 注册

`arch_boot` 是 `#[no_mangle]` 极早入口，在当前架构调用 `init_trap`。RV 设置 stvec；LA 还
设置 EENTRY/TLBRENTRY/page-walk/DMW/FPU-LSX。它不注册组合业务 handler，也不初始化
timer deadline、PLIC/设备或 scheduler。

完整顺序必须是：安装向量 → 建立可访问的 frame/栈/MM 状态 → 注册
`api_v0::kernel_trap::register_kernel_trap_handler` → 打开具体中断源 → 最后打开全局中断。
任何提前 IRQ 都会进入未注册 handler 并 panic。

## 4. 活动类型

`task::ActiveArchTaskContext` 和 `trap::ActiveTrapFrame` 是编译期别名，不是 trait object。
组合层拿到 `*mut u8` trap frame 后转成 ActiveTrapFrame，必须保证构建 feature 与产生 frame
的汇编相同。

RV/LA context 和 trap frame 大小不同，不能写入跨架构持久格式、网络包或共享磁盘镜像。
用户 signal frame 应通过 `SignalFrameCodec` 转成 Linux UAPI，而不是复制活动 Rust struct。

## 5. 用户 trap 地址空间差异

统一门面暴露：

- `user_trap_requires_kernel_address_space()`：RV=true、LA=false；
- `prepare_user_trap_frame_access()`：RV 设置 SUM，LA 当前 no-op；
- RV 专属 `set_kernel_trap_satp(token)`：发布 trampoline 在用户页表下切回的内核 satp；
- `return_address_space_token`：frame 返回时应激活的 token。

组合 handler 必须按上述能力判断，不能把 RV 的“先切内核 satp”机械复制到 LA。LA 的
PLV0 DMW0 保证内核 RAM/MMIO 可访问并可保留用户 PGDL；同时必须确认 DMW 不向 PLV3 开放。

## 6. timer slice 与频率

`trap::timer_slice_ticks()` 当前 RV=1,250,000 raw time ticks，LA=10,000,000 StableCounter
ticks。它们不是纳秒，也不是同一墙钟时长。platform timer 重装必须使用同一 counter 的
单位；调度器若要表达毫秒，应通过板级频率转换，而不是跨架构比较常量。

arch interrupt 只 enable timer source，真正设置 deadline 属于 platform profile/SBI/板级
timer。遗漏任一侧都会表现为“中断位开了但永不 tick”或“deadline pending 但被 mask”。

## 7. IPI 边界

`arch::ipi::send_ipi(CpuMask)` 只有 RV 调到 SBI transport；非 RV 当前直接 Unsupported，
尽管 LA 平台另有 IOCSR IPI 运输。业务层应优先走 platform `smp` 的 reason mailbox+transport
协议，而不是直接调用这个不对称兼容入口。

arch interrupt 的 `clear_soft_interrupt` 只清当前 CPU 硬件 pending，不读取/消费业务 reason。
正确接收顺序要保证 reason 与硬件清除之间没有丢 IPI，并在 trap 返回前处理新到 reason。

## 8. paging 边界

聚合层只读写当前 CPU token/CSR 和本地 TLB。PTE 构造、页表锁、active CPU mask、IPI、ack
及 frame 回收均在 MM/platform SMP。

`flush_tlb_local(range)` 允许实现扩大：RV 支持 page/ASID，range 退全量；LA 当前全部全量。
调用方不得因请求了 Page 就假设只刷新一页，也不得把本地 flush 当成远端 shootdown。

`init_paging_disable_mmu/enable_paging` 在 RV 是 no-op，在 LA 改 CRMD.DA/PG。它们主要服务
LA 早期页表建立，不能作为通用“确保 RV MMU 已关闭/开启”的断言。

## 9. 修改调用链示例：新增 trap 原因

以用户 misaligned load 为例：

1. 在 API `Exception` 增加语义变体，或明确仍归 Unsupported；
2. RV scause 和 LA ESTAT ecode 分别映射，不能共用数字；
3. 顶层 kernel trap handler 增加分支，区分用户/内核来源；
4. 若转成 signal，调用 `force_thread_signal(SIGBUS/SIGSEGV)` 并保留 fault address；
5. 若模拟指令，成功后按实际指令长度推进 PC；失败不能原地返回形成 trap loop；
6. 更新 signal siginfo code、日志/GDB 解码和两架构测试；
7. 验证 handler 未安装时默认终止，而内核态异常明确 panic。

## 10. 修改检查表

- trap frame/context：汇编 offset、Rust repr(C)、构造、signal、syscall、GDB 全同步；
- interrupt：原开/原关嵌套 restore、timer/soft 位互不覆盖；
- paging：kernel/user token、ASID wrap、COW 父/子 flush、远端 ack；
- CPU local：BSP/AP、用户 trap 后 tp/CPU ID 恢复；
- FPU/vector：trap、preemption、signal capture/restore；
- feature：默认 feature 泄漏、双 impl compile_error、两架构顶层构建。

```sh
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

运行验证至少包括 syscall、timer 抢占、软件 IPI、用户 page fault、COW、signal/rt_sigreturn、
TLS、浮点/LSX 和多任务地址空间切换。汇编能链接不等于 ABI 正确。
