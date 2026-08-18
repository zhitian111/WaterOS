# RISC-V64 架构实现离线开发手册

[架构聚合层](../../README.md) · [架构 API](../../arch-api/api-v0/README.md) · [Sv39 MM](../../../../wateros-mm/mm-impl/impl-sv39/README.md)

本 crate 是 WaterOS 的 RISC-V64 S 态机制层。它负责启动栈、CPU 身份、任务上下文切换、trap 机器现场、`satp`/ASID/TLB、本地中断位、`time` CSR 和 SBI IPI 的最薄封装。系统调用分派、调度策略、页错修复、定时器 deadline、IPI reason 队列和页表对象都不属于这里。

本文面向无法联网、无法依赖 agent 的现场开发。修改汇编前先核对本文的 ABI 表，再逐项核对源码中的 const assert 和调用方。

## 1. 文件与职责

| 文件 | 职责 | 不应放入的逻辑 |
|---|---|---|
| `asm/boot.S` | 选择 per-CPU 启动栈，建立内核 `tp`，进入 Rust | DTB 解析、AP 启动协议 |
| `asm/switch.S` | 保存/恢复协作式任务上下文，首次任务入口跳板 | trap 全寄存器保存、调度决策 |
| `asm/trap.asm` | 用户/内核 trap 入口、页表和栈切换、FPU 保存恢复、`sret` | syscall 分派、页错/COW、信号策略 |
| `src/cpu.rs` | 从内核 `tp` 读 CPU id，初始化可信 CPU 槽 | hart 枚举、online 状态 |
| `src/task.rs` | `Riscv64ArchTaskContext` 的 Rust 布局和入口符号 | TCB、runqueue、内核栈分配 |
| `src/trap.rs` | `TrapContext`、cause 解码、通用 trap-frame trait、信号编解码 | 具体 syscall 和中断业务 |
| `src/paging.rs` | `satp`、ASID WARL 探测、本 hart `sfence.vma` | 页表创建、跨核 shootdown |
| `src/interrupt.rs` | 本 hart 的 SIE/STIE/SSIE、SSIP 和 `wfi` | timer deadline、IPI reason |
| `src/time.rs` | 读取 `time` 原始 tick | 频率发现和纳秒换算 |
| `src/ipi.rs` | 把 CPU 位图交给 SBI `send_ipi` | TLB/调度 reason 的存取 |

依赖边界：

```text
machine/OpenSBI profile
  ├─ 规范化 boot 参数、启动 AP、设置 timer、复位
  └─ platform 聚合层
       ├─ 本 crate：ISA/CSR/汇编原语
       ├─ wateros-mm：页表、ASID 分配、shootdown
       ├─ wateros-task：TCB、内核栈、调度器
       └─ src/trap_handler.rs：trap 业务路由
```

## 2. 启动路径与 CPU 身份

### 2.1 `__wateros_arch_boot`

入口约定：

- `a0` 是 platform profile 规范化后的逻辑 CPU id；
- `a1/a2` 是平台启动参数，本汇编不解释；
- 每 CPU 启动栈 64 KiB，基址按 4 KiB 对齐；
- 栈顶是 `base + cpu_id * 64 KiB + 64 KiB`，栈向低地址增长；
- `sstatus.FS` 被置为 Dirty，使内核和 lp64d 用户程序可执行 F/D 指令；
- `tp = a0`，随后调用 `wateros_kernel_main`；若 Rust 意外返回则永久 `wfi`。

完整链路：

```text
OpenSBI/平台入口
  -> platform profile 规范化 hart id 为逻辑 CpuId
  -> __wateros_arch_boot(a0=cpu_id, a1/a2=boot args)
  -> 选择 64 KiB per-CPU boot stack，tp=cpu_id
  -> wateros_kernel_main(...)
  -> platform::arch::cpu::init_current_cpu(cpu_id)
  -> 后续 allocator/log/trap/scheduler 初始化
```

`boot.S` 的栈数组使用编译时 `config::task::MAX_CPUS`。越界 CPU 在进入 Rust 前就会选到数组外，因此平台入口必须先保证 id 已规范化且小于上限；不能指望 `init_current_cpu` 事后补救。

### 2.2 为什么内核用 `tp` 存 CPU id

S 态不能读取 M 态 `mhartid`。WaterOS 规定：运行内核 Rust 时，`tp` 必须是可信逻辑 CPU id；运行用户代码时，`tp/x4` 又是用户 TLS。用户 trap 必须在调用 Rust 前恢复内核 `tp`。

`init_current_cpu(cpu)`：

1. 检查 `cpu < MAX_CPUS`，否则返回 `InvalidCpu`；
2. 写内核 `tp`；
3. 把 CPU id 写进该 CPU supervisor-only return frame 的第 38 槽。

它必须在堆分配、日志递归检测和任何 Rust trap handler 之前调用。BSP 与每个 AP 都要调用；这不代表 CPU 已被 scheduler 标记 online。

### 2.3 固定 return-frame 容量

`trap.asm` 为 return frame 静态分配 `320 * 8 = 2560` 字节，即固定支持 8 个 CPU；boot stack 则使用 `MAX_CPUS`。修改 `MAX_CPUS` 时必须同步审计：

- `__wateros_riscv_return_frames` 的 `.zero` 数量；
- trampoline 数据页能否容纳全部 frame；
- `RETURN_FRAME_BYTES` 和 `RETURN_FRAME_CPU_ID_OFFSET`；
- platform 的 hart-id 映射；
- SMP 启动与新上限压力测试。

只改配置会造成跨 CPU frame 越界写，表现可能是随机 trap、CPU id 错乱或用户返回损坏。

## 3. 两种上下文

任务上下文与 trap 上下文用途不同，不能互换。

### 3.1 `Riscv64ArchTaskContext`

```text
offset  字段       寄存器
0       ra         返回地址
8       sp         栈指针
16..104 s[0..11]   s0..s11
总计               14 * 8 = 112 字节
```

`__switch(a0=old, a1=new)`：

1. 将 `sp/ra/s0..s11` 写入 old；
2. 从 new 恢复 `ra/s0..s11/sp`；
3. 清 `sscratch=0`，声明当前是内核上下文；
4. `ret` 到 new 保存的 `ra`。

它只保存 callee-saved 寄存器，因为正常 Rust 调用边界允许 caller-saved 被破坏。FPU 和全部 GPR 属于 trap 抢占现场，由 `TrapContext` 保存。

首次内核任务链路：

```text
goto_task_entry(__arch_task_entry, kstack_top, bootstrap_ptr)
  -> ra=__arch_task_entry, sp=kstack_top, s0=bootstrap_ptr
__switch(...)
  -> __arch_task_entry -> mv a0,s0
  -> __wateros_arch_task_entry_trampoline
  -> __wateros_task_runtime_entry
  -> 发布延迟迁移任务、开中断、执行任务体
```

首次用户任务令 `ra=__arch_user_task_entry`，最终进入 `__wateros_task_runtime_enter_current_user_task`；运行库取出权威 trap frame 和内核栈顶，再调用架构恢复例程。

### 3.2 `TrapContext`

`TrapContext` 是 `#[repr(C)]`，固定 560 字节：

| 字节偏移 | 槽 | 内容 |
|---:|---:|---|
| 0..255 | 0..31 | `x0..x31` |
| 256 | 32 | `sstatus` |
| 264 | 33 | `sepc` |
| 272 | 34 | `scause` |
| 280 | 35 | `stval` |
| 288 | 36 | 返回时写入 `satp` 的 token |
| 296..551 | 37..68 | `f0..f31`，每槽 64 位 |
| 552..555 | 69 低半 | `fcsr`，32 位 |
| 556..559 | 69 高半 | 对齐填充 |

Rust const assert 固定 `fpregs=296`、`fcsr=552`、总大小 560。新增字段绝不能只改 Rust：还要同步汇编偏移、栈调整量、复制槽数、调试 CFI、任务保存区和 signal codec。

## 4. Trap 入口链路

### 4.1 用户态进入

进入用户态前，`sscratch` 指向本 CPU 独占的 trampoline return frame；内核态保持 `sscratch=0`。入口用 `csrrw t0, sscratch, t0` 在不提前破坏用户寄存器的情况下区分来源。

```text
用户指令触发 trap
  -> __alltraps
  -> t0 得到 return-frame 地址，用户 x5 暂存在 sscratch
  -> 在用户 satp 下保存 x0..x31、sstatus/sepc/scause/stval/satp
  -> 清 sscratch，避免嵌套内核 fault 被误判为用户 trap
  -> 从 frame[37] 取 kernel_stack_top，内核栈预留 560 字节
  -> 从 __wateros_riscv_kernel_satp 取 token，切内核 satp
  -> 没有 ASID 时执行全量 sfence.vma
  -> 从 frame[38] 恢复可信内核 tp/CPU id
  -> 把前 37 个 usize 槽复制到内核栈 TrapContext
  -> 保存 f0..f31 与 fcsr
  -> trap_entry_rust(TrapContext*)
  -> arch-api 注册的 kernel trap handler
```

用户页表必须映射 trampoline 代码和数据页，并禁止 U 态访问数据。`set_kernel_trap_satp()` 必须在任何用户态运行前写入有效内核 token；当前 Sv39 初始化在激活 kernel aspace 前完成该步骤。

### 4.2 内核态进入

当 `sscratch=0` 时：

1. 入口交换后从 `sscratch` 取回被临时保存的内核 x5；
2. 当前内核栈直接减 560；
3. 原始内核 `sp` 写入 `x[2]`，所有 GPR/CSR/FPU 写入本栈；
4. 调用相同的 `trap_entry_rust`；
5. 若保存的 `sstatus.SPP=1`，原地恢复并 `sret`。

`.Ltrap_from_kernel_old/.Lsave_context` 是当前控制流不会跳入的遗留代码段。清理前应反汇编确认没有外部落点依赖；功能修改不要以旧路径为依据。

### 4.3 Rust 路由

```text
trap_entry_rust(cx)
  -> arch_api::kernel_trap::invoke_kernel_trap_handler(cx as *mut u8)
  -> src/trap_handler.rs::wateros_kernel_trap_handler
       ├─ 用户帧：取得 TCB 中的权威 trap frame
       ├─ syscall：取 nr/args，分派，写返回值
       ├─ page fault：交给 MM/VMA/COW
       ├─ timer：重装 deadline、scheduler tick
       ├─ soft interrupt：取 IPI reason、清 SSIP
       └─ signal：构造/恢复 signal frame
  -> 将权威用户帧写回栈上 frame
  -> 返回汇编
```

`register_kernel_trap_handler` 必须在首次允许 trap 前调用；当前组合层在 `task::init()` 后注册。未注册时任何 trap 都会 panic。

## 5. Trap 返回与地址空间

### 5.1 返回内核

Rust 返回后汇编恢复 `sstatus/sepc` 并检查 SPP。SPP 为 1 时：

- 原地恢复 FPU/FCSR 和 GPR；
- 写回 frame[36] 的 `satp`；
- 无 ASID 时执行全量 fence；
- 最后恢复被当作 scratch 的 x5/x6 和原始 sp；
- `sret`。

scratch 恢复不可提前，否则 ASID 分支临时值会污染被中断的内核寄存器。

### 5.2 返回用户

SPP 为 0 时跳到 `__wateros_riscv_restore_user_from_frame(a0=frame, a1=kernel_stack_top)`：

1. 在内核页表下恢复 FPU/FCSR；
2. 用内核 `tp` 计算本 CPU 的 320 字节 return frame；
3. 复制前 37 个机器字；
4. frame[37] 写下次 trap 的内核栈顶，frame[38] 写可信 CPU id；
5. 写 `sstatus/sepc`；
6. 写用户 `satp`，无 ASID时 fence；
7. `sscratch=return_frame`；
8. 从用户页表可见的 trampoline 数据恢复 GPR；
9. 最后恢复用户 sp 和 x5，执行 `sret`。

`set_return_to_user()` 清 SPP、清当前 SIE、设置 SPIE，并将 FS 置 Dirty。缺少这些步骤会分别导致回到 S 态、返回准备期间错误开中断、用户中断关闭或浮点异常。

## 6. 系统调用 ABI 与新增实例

RISC-V64 ABI：

- 参数 0..5：`a0..a5 = x10..x15`；
- syscall 编号：`a7 = x17`；
- 返回值：`a0 = x10`；
- PC：`sepc`，正常 `ecall` 返回由业务层推进 4 字节；
- 用户栈：`x2`；TLS：`x4`。

新增三参数 `sys_foo(fd, user_ptr, len)` 时，架构层通常无需改动：

```text
用户 a7=SYS_FOO, a0=fd, a1=user_ptr, a2=len, ecall
  -> scause=8 -> Exception::UserEnvCall
  -> TrapFrameRead::syscall_nr / syscall_args
  -> dispatcher 匹配 SYS_FOO
  -> user-copy / fd-session / 业务实现
  -> set_syscall_ret(UserRet(...))
  -> sepc += 4
  -> signal/restart 检查
  -> sret
```

离线实现检查：

1. 在 syscall number/dispatcher 层加编号，不要在 `trap.rs` 写业务分支；
2. 用户指针必须经过 user-copy/VMA 校验；
3. 明确失败的负 errno 和成功返回；
4. 明确阻塞调用的锁释放、信号中断和 restart；
5. 正常路径只推进一次 PC；restart 由 `prepare_syscall_restart` 回退指令并恢复 nr/args；
6. 测试非法地址、零长度、跨页、并发 close、信号打断和 fork 后使用。

## 7. Cause 解码

| 类型 | code | 通用枚举 |
|---|---:|---|
| interrupt | 1 | `SupervisiorSoft` |
| interrupt | 5 | `SupervisiorTimer` |
| interrupt | 9 | `SupervisiorExternel` |
| exception | 2 | `IllegalInstruction` |
| exception | 3 | `Breakpoint` |
| exception | 8 | `UserEnvCall` |
| exception | 12 | `InstructionPageFault` |
| exception | 13 | `LoadPageFault` |
| exception | 15 | `StorePageFault` |

其它 code 保留为 `Unsupported(raw)`。`fault_addr()` 返回 `stval`；是否是有效 VA 取决于 cause，业务层必须先判断类型。

新增 cause 时更新通用 API 枚举（如确需新语义）、本文件解码、业务路由、日志和测试。不要单独修正现有公开枚举的 `Supervisior*` 拼写，除非同步全仓迁移。

## 8. 信号机器上下文

capture 保存 GPR、PC、status、32 个 FPR 和 FCSR；RISC-V 没有通用容器中的 FCC/LSX，相关字段写 0。

恢复约束：

- PC 非 0；
- PC 至少 2 字节对齐，当前检查 `pc & 1 == 0`，兼容压缩指令；
- 强制 `x0=0`；
- 不恢复用户提供的 status，而由 `set_return_to_user()` 重建受控特权位；
- handler 使用 `x1=restorer`、`x2=frame_sp`、`a0/a1/a2=signal/siginfo/ucontext`。

扩展 vector 状态时要同步通用上下文、用户 ABI frame、trap 保存恢复、exec 初始值、fork 克隆和相关测试。只扩 Rust 结构会导致越界或跨任务泄漏。

## 9. `satp`、ASID 与 TLB

架构 token 是完整 `satp`：MODE、ASID、PPN。纯架构层不校验页表有效性，调用方负责生命周期。

`initialize_address_space_ids()`：

1. 读取原 `satp`；
2. 保持 MODE/PPN，将 ASID 候选位写 1；
3. 读回 WARL 结果并统计实现位数；
4. 恢复原 token，全量 `sfence.vma`；
5. 将“是否有 ASID”写入 trampoline 快速切换开关。

调用时必须已运行在有效页表上。该开关只决定 trap 切换能否省略 fence，不负责 ASID 分配、回收或 generation rollover。

本地 flush：

| 范围 | 指令 |
|---|---|
| `Page { addr }` | `sfence.vma addr, x0` |
| `AddressSpace { token }` | 提取 ASID，`sfence.vma x0, asid` |
| `Range { .. }` | 当前退化为全量 fence |
| `All` | 全量 fence |

这些只影响当前 hart。跨核修改 PTE 的完整协议：

```text
在页表锁/生命周期协议下修改 PTE
  -> 记录 active CPU mask
  -> 本 CPU flush
  -> 发布 TlbShootdown reason
  -> release 顺序发布 pending
  -> platform::smp::send_ipi(remote_mask, TlbShootdown)
  -> 远端 acquire 读取 reason并本地 fence
  -> ack；若要释放页框则等待全部 ack
```

fork/COW 把父 PTE 改只读也必须 shootdown，否则旧 TLB 仍可能允许写。页框只能在相关 CPU 完成 flush 后回收。

`activate_address_space_token_and_flush` 总是写 token 后全量 fence。`init_paging_disable_mmu` 和 `enable_paging` 是 no-op，因为分页模式由 `satp.MODE` 决定。

## 10. 中断、时间与 IPI

### 10.1 本 hart 中断

- timer source：`sie.STIE`；software source：`sie.SSIE`；
- global：`sstatus.SIE`；清 soft interrupt：`csrc sip, 1<<1`；
- idle 等待：`wfi`。

`read_global_interrupt_state()` 保存完整原始 `sstatus`，但 restore **只恢复 SIE bit 1**。它是 irq-save 状态票据，不是写回整个 CSR 的 API；写回完整值会破坏 SPP/SPIE/FS。

### 10.2 时间

`read_time_tick()` 读 CSR `time`，只是原始 tick。频率查询返回 `Unsupported`，Hz 由 platform/DTB 提供。当前 slice 是 `1_250_000` tick，不能当纳秒，也不能假定与 LoongArch 的 `10_000_000` tick 等时长。

### 10.3 SBI IPI

`send_ipi(CpuMask)` 直接构造 `HartMask(mask, base=0)`。它隐含逻辑 CPU 位与 SBI hart id 从 0 起直接对应；稀疏 hart id 必须由 platform 层映射。

SBI 错误保留为 `IpiError::Firmware(error)`。此接口只制造 SSIP，不携带业务原因；`platform::smp` 必须先发布 reason，接收端处理 reason 并在返回前清 SSIP。清 SSIP 不等于清 reason 队列。

## 11. 常见故障

| 症状 | 优先检查 |
|---|---|
| 用户一 `ecall` 就页错 | trampoline 映射、kernel satp、frame[37] 栈顶 |
| 随机 CPU id | frame[38] 恢复 tp、CPU id 越界 |
| SMP/fork 后旧数据 | COW 写保护后的 shootdown/ack |
| signal 返回 SIGILL | PC 对齐、FCSR/FPR、FS 位 |
| timer trap 后寄存器随机 | x5/x6 恢复顺序、frame 偏移、栈空间 |
| 用户 TLS 损坏 | 是否误把用户 tp 当 CPU id |
| 开启 ASID 后才坏 | token ASID、回收 generation、跳过 fence 条件 |
| soft interrupt 风暴 | reason 处理后是否清 SSIP |
| 增加 CPU 后越界 | 固定 8 个 return frame 与 trampoline 容量 |
| 浮点跨线程串值 | 进入 Rust 前是否保存完整 FPU |

先记录 `scause/sepc/stval/sstatus/satp/cpu_id` 再分类。不要在尚未恢复可信 `tp` 或尚未切内核页表时调用 Rust 日志。

## 12. ABI 修改同步清单

修改 `TrapContext`：

- Rust 字段、`#[repr(C)]` 和 const assert；
- 汇编全部保存/恢复偏移与 `addi sp, ±560`；
- CFI、用户 frame 复制边界；
- task trap-frame 保存区；
- signal、fork/exec 初始帧和 syscall 寄存器映射；
- GDB/崩溃转储工具（若读取固定布局）。

修改 `TaskContext`：

- Rust 字段顺序与 `switch.S`；
- 首次入口参数寄存器；
- scheduler 保存区大小/对齐；
- 两任务高频 yield、SMP 迁移、退出后栈回收测试。

修改 CPU 上限：同步 boot stack、return frame、trampoline 页、hart 映射、CPU mask、IPI reason 和 per-CPU 数组。

## 13. 回归矩阵

静态门禁：

```bash
cd os
cargo fmt --all -- --check
make check ARCH=rv PROFILE=pre
python3 scripts/maintenance/check_offline_docs.py
```

架构汇编不能在宿主 x86/macOS 上直接运行；`cargo check` 只证明类型和目标汇编可编译，不证明现场语义。QEMU/硬件至少执行：

| 类别 | 用例 | 判据 |
|---|---|---|
| boot | SMP=1、2、8 | 每核唯一 id/栈，全部 online |
| switch | 高频 yield + SMP 迁移 | callee-saved、sp、ra 完整 |
| syscall | 六参数测试 syscall | nr/参数/返回与 PC 正确 |
| fault | 读/写/取指 fault、COW | `stval`/cause 正确，无旧可写 TLB |
| FPU | 多线程不同浮点状态并抢占 | FPR/FCSR 不串任务 |
| signal | handler、rt_sigreturn、restart | PC/SP/TLS/FPU 恢复 |
| IPI | reschedule + shootdown | reason 不丢，SSIP 不风暴 |
| ASID | 强制复用/rollover | 不读取旧地址空间翻译 |
| fork | `stress-ng --forkheavy` | 无泄漏、无 stale TLB、持续回收 |

修改 trap 后还应反汇编最终 ELF，确认符号确实位于 trampoline section、跳转未超范围、偏移与源码一致。链接成功不等于 ABI 正确。
