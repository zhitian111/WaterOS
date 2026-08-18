# Architecture API v0 离线开发手册

[Platform 总览](../../../README.md) · [Arch 聚合层](../../README.md) · [RISC-V 实现](../../arch-impl/impl-riscv64/README.md) · [LoongArch64 实现](../../arch-impl/impl-loongarch64/README.md)

本 crate 定义 WaterOS 的纯 ISA 契约：CPU 本地初始化、原始时间计数、中断位、内核任务
切换上下文、trap frame、用户 signal 上下文和本地 TLB 操作。它不依赖 SBI、DTB、QEMU
设备、scheduler 或 syscall 实现。

## 1. 模块和职责边界

| 模块 | 契约 | 不负责 |
|---|---|---|
| `cpu` | 当前 CPU 初始化错误类型 | hart 启动、online mask |
| `time` | 读 raw tick、可选频率 | 设置下一 timer deadline |
| `interrupt` | 本 CPU timer/global interrupt 位、save/restore、WFI | PLIC、SBI timer、IPI reason |
| `task` | 最小 callee-saved context 构造/诊断 | TCB、调度策略、栈分配 |
| `trap` | 原因语义、frame 读写、syscall ABI、signal codec | page fault/syscall 业务 |
| `paging` | 本地地址空间 token 与 TLB flush 请求 | PTE、页表锁、远端 shootdown |
| `kernel_trap` | 运行期注册一个不透明 frame handler | 具体 frame downcast/业务分发 |

典型依赖方向：

```text
task/mm/syscall 只依赖 arch API/聚合门面
  -> platform-arch 选择一个 Active* 类型
     -> arch-impl-riscv64 或 arch-impl-loongarch64
        -> 汇编/CSR
```

## 2. CPU 与启动顺序

`ArchCpuInitError::InvalidCpu` 表示逻辑 CPU 超出容量或与硬件入口不符。成功初始化 ISA
本地状态不表示 scheduler 已把 CPU 标记 online。

推荐顺序：

```text
汇编 _start / per-CPU boot stack
  -> init_current_cpu(cpu_id)
  -> arch_boot()/init_trap
  -> MM/内核映射与 ASID 初始化
  -> register_kernel_trap_handler
  -> timer/software interrupt enable
  -> scheduler publish online
  -> global interrupt enable
```

每个 AP 都要执行本核初始化，不能只由 BSP 调用。打开中断前必须同时满足 trap vector、
frame backing、组合 handler、当前 CPU ID 和内核栈均有效。

## 3. 时间 API

`ArchTimeTick(u64)` 是硬件原始单调计数，`ArchTimeFrequency(u64)` 是 tick/s。两者是不同
newtype，避免把 tick 当纳秒。`ArchTime::read_time_frequency` 默认返回 Unsupported；当前
RV 的 `time` CSR 和 LA 的 `rdtime.d` 都不在 arch 层硬编码频率，platform 应从 DTB/板级
配置提供换算。

错误只有 Unsupported 和 Unavailable。raw counter wrap、跨 CPU 同步性、换算溢出和
deadline 编程属于调用层；计算 ns 时使用宽整数/饱和运算，不能先在 u64 相乘。

## 4. 中断 save/disable/restore

`ArchInterruptState(usize)` 是不透明快照。调用者只能保存并交回当前架构的 restore，不能
解析或跨 CPU/跨架构传递。当前实现读取完整 `sstatus`/`CRMD`，但 restore 只根据其中的
SIE/IE 位决定 enable 或 disable，不会回写其它 CSR 字段。

嵌套临界区必须使用：

```rust
let old = arch::interrupt::read_global_interrupt_state()?;
arch::interrupt::disable_global_interrupt()?;
// 本 CPU 不可被普通 IRQ 重入的短临界区
arch::interrupt::restore_global_interrupt_state(old)?;
```

禁止临界区结尾无条件 enable，否则外层原本关闭中断时会被破坏。关闭本 CPU 中断不提供
SMP 互斥；共享数据仍需锁/原子。

timer interrupt enable 只改本 CPU ISA enable 位，不设置下一 deadline。soft interrupt
enable/clear 位于聚合/实现扩展，不在 trait 中；clear 必须在 trap 返回前完成，否则会立即
再次陷入。`wait_for_interrupt` 是 RV `wfi` 或 LA `idle 0`，调用方先建立“检查条件→允许
唤醒→睡眠”的无丢唤醒协议。

## 5. `ArchTaskContext`

trait 要求 `Clone+Copy+Debug`，提供：

- `zero_init`：全零但不可运行，必须随后完整填充；
- `goto_entry(entry_stub,kstack_top)`：首次 restore 后从 entry_stub 执行；
- `goto_task_entry(...,bootstrap_ptr)`：由实现把不透明 bootstrap 参数编码到约定槽；
- `return_address/stack_pointer`：诊断保存现场损坏。

它只保存 ABI callee-saved 子集，不是完整用户 trap frame。RV 是 ra/sp/s0–s11；LA 是
ra/sp/r22–r31。实际 `__switch` 按结构偏移 load/store，任何字段增删/换序必须同步汇编并
增加 offset/size const assert。

栈映射、向下增长、对齐和 guard page 由 task 层保证。`goto_entry` 不检查 PC/SP；把 0、
未映射或未对齐地址传入会在首次切换时崩溃。

## 6. TrapCause 与 frame 读写

跨架构 `Exception` 只建模 ecall、三类 page fault、illegal、breakpoint 和 Unsupported；
`Interrupt` 建模 timer/external/software。raw scause/ESTAT→语义枚举必须由每个实现显式
完成，API 不提供默认数字映射。

公共变体沿用已有拼写 `SupervisiorTimer/SupervisiorExternel/SupervisiorSoft`。修正拼写
会破坏所有 pattern match，比赛现场不要只改定义；应做全仓迁移或保留兼容别名。

`TrapFrameRead` 提供 raw/semantic cause、fault addr、PC/SP、来源特权级、syscall a0–a5
视图、syscall nr、TLS 和返回 token。`syscall_context()` 只是把 nr+args 打包，不推进 PC。

`TrapFrameWrite` 提供 PC/SP、entry args、返回特权级、syscall ret、TLS 和 token 写入。
`add_user_pc` 的字节数由组合 handler按实际 ISA 指令决定；当前 syscall ecall 两架构均为
4 字节，不能把这一事实硬编码到通用 trait。

`prepare_user_return` 只做 PC+SP+return-to-user，不设置 token、TLS、argc、浮点状态或
用户页表；新任务构造仍需完整调用链。

## 7. Rust frame 与汇编 ABI

`ActiveTrapFrame` 必须 `repr(C)` 且与 trap 汇编逐槽一致。修改时同步检查：

1. 汇编分配总字节数和栈对齐；
2. 每个 GPR/CSR/FPR/vector 保存恢复偏移；
3. Rust struct 顺序、padding、size/offset assert；
4. syscall 参数、返回值、SP、TLS、PC；
5. task/trap frame 构造代码；
6. signal `ucontext` 编解码和 `rt_sigreturn`；
7. GDB/日志 frame dump；
8. 用户/内核 trap 的地址空间切换跳板。

只要一个 offset 错误，症状可能延迟到抢占、浮点或信号返回，不一定在第一次 syscall
出现。仅能链接不是 ABI 验证。

## 8. `SignalMachineContext` 与 codec

该 `repr(C)` 类型是内核组件间的架构中立容器，不是 Linux 用户 ABI：32 GPR、PC、状态、
32 FPR、FCSR、LA FCC 和 32×128-bit vector。RISC-V 把 LA 专属字段置零；LA 同时保存 LSX
并用每个 vector 的低 64 位表达基础 FPR。

`SignalFrameCodec`：

- capture 只抓可写入用户 signal frame 的机器状态；
- restore 必须拒绝非法 PC/状态，强制 x0/r0=0，并只恢复用户可控位；
- prepare handler 按 ISA ABI布置 RA/SP、signal/siginfo/ucontext 参数和 handler PC；
- prepare syscall restart 回退 PC 并恢复 nr+6 args。

用户可见 `ucontext` 必须由 syscall 层逐字段编码，不能 `copy_to_user` 此 Rust struct。恢复
时不能信任用户提供的 status、特权位、内核地址 token 或未对齐 PC。

## 9. 内核 trap hook

`kernel_trap` 用 `AtomicPtr<()>` 保存一个 `extern "C" fn(*mut u8)`：

```text
组合层 register_kernel_trap_handler(handler) [Release]
trap_entry_rust(raw frame)
  -> invoke_kernel_trap_handler
  -> load handler [Acquire]
  -> 未注册：panic
  -> handler(raw frame)，组合层按 ActiveTrapFrame downcast
```

只保留最后一次注册，没有 compare-exchange、注销和生命周期 guard。启动期只能注册静态
函数；运行时替换需要外部保证旧 handler 依赖的状态永不释放。未注册时 panic 是刻意的，
不能静默跳过 page fault/timer/IPI 后返回原指令。

不透明指针只在本次 trap 栈帧有效，不得缓存、跨线程传递或在 handler 返回后访问。组合
层 downcast 必须与 feature 选择的 `ActiveTrapFrame` 完全一致。

## 10. 本地 TLB 请求

`TlbFlushRange`：

- `All`：本 CPU 全部 translation；
- `AddressSpace { token }`：实现从 token 解析 ASID/root；
- `Page { addr }`：单虚页；
- `Range { start,end }`：通常按半开范围理解，但 API 当前未校验对齐或 start<=end。

实现可以保守扩大，不能缩小。RV Page 使用按 VA `sfence.vma`、AddressSpace 按 ASID，Range
当前退化全量；LA 所有变体当前统一 `invtlb all`。因此上层不能依赖精确 flush 的性能。

这个 API 只操作当前 CPU。完整页回收顺序：

```text
持页表锁撤销/修改 PTE
  -> 记录 active CPU mask
  -> 本地 flush
  -> platform IPI 发送远端 shootdown reason
  -> 各 CPU 本地 flush + ack
  -> 等全部 ack
  -> 才减少最后物理页引用/复用 frame
```

只调用 `flush_tlb_local` 后立即回收在 SMP 上会造成 use-after-free。

## 11. 地址空间 token/ASID

token 编码由实现解释：RV 是完整 satp；LA 用低 48 位 PGDL、高位中的 10-bit ASID。
调用方不得自行位移解析通用 token。`active_address_space_token` 只读当前 CPU。

`initialize_address_space_ids` 返回硬件可用位数：RV 通过 satp WARL 探测并恢复原 token后
全量 fence，要求当前 satp 是有效可执行页表；LA 当前固定返回 10。ASID allocator 必须在
复用编号前完成相应范围/全局 shootdown。

`activate_address_space_token_and_flush` 切 root+ASID 并本地全量 flush。RV 的
`init_paging_disable_mmu/enable_paging` 是 no-op，分页由 satp.MODE 表达；LA 实际切换
CRMD.DA/PG。不能把这两个入口的副作用假定为跨架构相同。

## 12. 新增架构实现清单

1. 在聚合 Cargo.toml 增加互斥 feature 和 optional dependency；
2. 实现 CPU ID/每 CPU 初始化，明确用户 trap 后如何恢复内核 CPU-local 寄存器；
3. 实现 raw tick、global/timer/soft interrupt、save/restore 和 idle；
4. 定义 task context 与 `__switch`，提供 bootstrap/user trampolines；
5. 定义完整 trap frame、入口/返回汇编、cause 解码和 const offset 断言；
6. 实现 syscall ABI、TLS、用户返回权限和 PC 推进；
7. 实现 signal capture/restore/handler/restart，过滤用户 status；
8. 定义 token、ASID 探测/宽度和所有 local TLB range；
9. 接 platform 的 timer deadline、IPI transport、external IRQ；
10. 顶层组合 handler 增加 ActiveTrapFrame 路由并做真实用户态回归。

## 13. 自回归矩阵

- BSP/AP CPU ID、容量边界、用户 trap 后 CPU-local 恢复；
- interrupt 原开/原关两种嵌套 restore，timer/soft 位互不污染；
- raw tick 单调、频率 unsupported 降级、deadline 使用同一单位；
- `__switch` 往返每个 callee-saved、SP 对齐、首次 bootstrap 参数；
- trap frame 所有 offset/size、用户/内核来源、unsupported cause；
- syscall 六参数/nr/负 errno/PC+4、TLS、exec entry；
- page fault fault_addr、COW write fault、illegal/breakpoint；
- FPU/LSX 抢占、signal handler、嵌套 signal、rt_sigreturn、restart；
- token 切换、ASID wrap/reuse、Page/AddressSpace/Range/All flush；
- 两 CPU 页表 shootdown ack 后再 frame reuse；
- handler 未注册明确失败，首次 enable IRQ 前已注册。

本 crate 单独不选 arch impl 时聚合调用无法编译；使用顶层目标检查：

```sh
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```
