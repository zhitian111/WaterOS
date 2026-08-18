# LoongArch64 架构实现离线开发手册

[架构聚合层](../../README.md) · [架构 API](../../arch-api/api-v0/README.md) · [LoongArch MM](../../../../wateros-mm/mm-impl/impl-loongarch64/README.md)

本 crate 实现 WaterOS 在 LoongArch64 上最接近硬件的一层：启动栈、任务切换、异常/中断现场、TLB refill、PGDL/ASID、CRMD/ECFG、StableCounter，以及 FPU/LSX 上下文。syscall、VMA/COW、调度策略、timer deadline、板级 IOCSR IPI 发送协议和设备枚举属于上层。

LoongArch 不能照抄 RISC-V 的 trap 方案：PLV0 内核依靠 DMW0 直接访问内核 RAM/MMIO，用户 trap 处理时通常保留用户 PGDL；CPU id 来自 `CSR.CPUID`，不借用用户 TLS 寄存器；返回指令为 `ertn`，用户栈切换通过 `CSR.SAVE0` 完成。

## 1. 文件与分层

| 文件 | 当前职责 | 不应加入的逻辑 |
|---|---|---|
| `asm/boot.S` | 选择 64 KiB per-CPU boot stack，进入 Rust | mailbox/AP 启动协议 |
| `asm/switch.S` | callee-saved 任务切换、首次任务/用户恢复 | scheduler 选任务 |
| `asm/trap.S` | trap frame、用户/内核栈切换、LSX/FCC、TLB refill | syscall/COW/调度业务 |
| `src/cpu.rs` | 读取 `CSR.CPUID` 并校验逻辑 CPU id | CPU online 生命周期 |
| `src/task.rs` | `LoongArch64ArchTaskContext` Rust ABI | TCB 和内核栈分配 |
| `src/trap.rs` | trap 初始化、cause 解码、frame trait、信号 codec | 具体 fault 修复 |
| `src/paging.rs` | PGDL/ASID token、CRMD 分页模式、本地 `invtlb` | 页表和跨核 shootdown |
| `src/interrupt.rs` | 本 CPU CRMD/ECFG 与 IPI pending 清除 | 目标 CPU 选择、reason 队列 |
| `src/time.rs` | `rdtime.d` 原始 tick | 频率发现、deadline 编程 |

调用边界：

```text
QEMU LoongArch platform profile
  ├─ boot mailbox、AP 唤醒、IOCSR IPI 发送、timer、设备
  └─ wateros-platform
       ├─ 本 crate：ISA/CSR/汇编
       ├─ wateros-mm：三级页表、ASID 生命周期、shootdown
       ├─ wateros-task：TCB、栈、scheduler
       └─ src/trap_handler.rs：syscall/fault/interrupt 路由
```

## 2. 启动与 CPU 身份

### 2.1 `__wateros_arch_boot`

入口 ABI：

- `$r4`：平台规范化后的逻辑 CPU id；
- `$r5/$r6/$r7`：platform boot 参数；
- 每 CPU boot stack 为 64 KiB，数组按 4 KiB 对齐；
- 栈顶为 `base + cpu_id * 64 KiB + 64 KiB`；
- 随后 `bl wateros_kernel_main`，异常返回则循环 `idle 0`。

```text
固件/QEMU 入口
  -> platform profile 解析 mailbox/boot 参数
  -> __wateros_arch_boot(r4=logical_cpu, r5..r7=args)
  -> 选择 per-CPU boot stack
  -> wateros_kernel_main
  -> platform::arch::cpu::init_current_cpu(cpu)
  -> 初始化 MM/trap/task/中断
```

汇编在调用 Rust 前没有边界检查，CPU id 越界会直接选择 boot-stack 数组外地址。平台入口必须保证 `cpu < MAX_CPUS`。

### 2.2 `CSR.CPUID`

`current_cpu_id()` 读取 CSR `0x20`。`init_current_cpu(cpu)` 同时验证：

- 参数在 `MAX_CPUS` 容量内；
- 硬件 `CSR.CPUID` 与平台传入的逻辑 id 完全相等。

当前实现假设 QEMU virt 的硬件 CPUID 就是 WaterOS 的稠密逻辑 CPU id。若真实平台 CPUID 稀疏或很大，需要新增 platform 映射表，并重新定义 `current_cpu_id` 返回逻辑 id；不能仅放宽上限，否则所有 per-CPU 数组仍会越界。

LoongArch 用户 TLS 使用 `$r2`，不会与 CPU id 冲突；不要移植 RISC-V 的“内核 `tp` 保存 CPU id”逻辑。

## 3. 任务上下文

`LoongArch64ArchTaskContext` 为 `#[repr(C)]`：

```text
offset  Rust 字段  寄存器
0       ra         r1
8       sp         r3
16      s[0]       r22
24..88  s[1..9]    r23..r31
总大小             12 * 8 = 96 字节
```

`__switch(r4=old, r5=new)` 保存 old 的 `r1/r3/r22..r31`，恢复 new 的同一集合，再 `jr r1`。这是协作式切换现场；caller-saved、LSX/FPU/FCC 由正常 ABI 或 trap frame 负责。

首次内核任务：

```text
goto_task_entry(__arch_task_entry, kstack_top, bootstrap_ptr)
  -> ra=entry, sp=kstack_top, r22/s[0]=bootstrap_ptr
__switch
  -> __arch_task_entry
  -> move r4,r22
  -> __wateros_arch_task_entry_trampoline
  -> __wateros_task_runtime_entry
```

首次用户任务：

```text
__switch -> __arch_user_task_entry
  -> __wateros_task_runtime_enter_current_user_task
  -> scheduler::restore_current_trap_frame
  -> 取得 kernel_stack_top
  -> __wateros_arch_restore_user_task(frame*, kernel_stack_top)
  -> CSR.SAVE0=kernel_stack_top
  -> 写 PRMD/ERA/PGDL/ASID/CRMD
  -> 恢复机器现场
  -> ertn
```

修改任务 context 时必须同步 Rust 字段和 `switch.S` 的全部 offset。仅在 trap frame 中增加寄存器不能弥补普通 `__switch` ABI 错位。

## 4. `TrapContext` 精确布局

结构使用 `#[repr(C)]`，固定 832 字节并保持 16 字节栈对齐：

| 字节偏移 | 内容 | 说明 |
|---:|---|---|
| 0..255 | `x[0..31]` | `r0..r31`，每个 8 字节 |
| 256 | `prmd` | trap 前 PLV/中断返回状态 |
| 264 | `era` | exception return address |
| 272 | `estat` | pending 位和 ecode |
| 280 | `badv` | fault 地址 |
| 288 | return token | PGDL 低 48 位 + ASID bit 57:48 |
| 296..807 | `lsx[32]` | 32 个 128-bit `$vr0..$vr31` |
| 808 | `fcsr` | FCSR0 值放入 usize |
| 816 | `fcc` | FCC0..FCC7 打包到 bit0..7 |
| 824 | `reserved` | 显式清零的对齐槽 |

Rust const assert 固定总大小和最后三个字段偏移。`reserved` 必须由汇编写 0，因为 frame 会按值复制进 TCB；留下旧栈字节会让 Rust 从未初始化存储构造 `usize`。

基础浮点寄存器与 LSX 寄存器共享低 64 位，所以 signal capture 从每个 `lsx[i][0]` 派生 `fpregs[i]`。restore 先复制完整 vectors，再用通用 `fpregs` 覆盖低 64 位，确保传统 FP ABI 字段优先。

## 5. Trap 入口

### 5.1 不破坏 r12..r15 的来源判定

入口首先把 `$r12..$r15` 写到 supervisor CSR `SAVE1..SAVE4`，然后读取 PRMD.PPLV：

- PPLV=3：来自用户；
- 其它值：按内核 trap 处理。

来自用户时，原用户 sp 暂存到 r15，从 `CSR.SAVE0` 取内核栈顶；来自内核时直接保存当前 sp。两条路径都在选定的内核栈上减 832 字节并汇合到 `.Lsave_context`。

`CSR.SAVE0` 必须在每次返回用户前写为当前任务内核栈顶。若任务迁移后仍指向旧 CPU/旧任务栈，下次用户 trap 会覆盖已释放内存。

### 5.2 完整保存链路

```text
异常/中断
  -> __alltraps
  -> SAVE1..SAVE4 暂存 r12..r15
  -> PRMD.PPLV 判定用户/内核
  -> 用户：SAVE0 切内核栈；内核：沿当前栈
  -> 分配 832 字节 TrapContext
  -> 保存 r0..r31（从 SAVE CSR 恢复原 r12..r15）
  -> 保存 PRMD/ERA/ESTAT/BADV
  -> PGDL + ASID 编码为 return token
  -> 保存 32 个 LSX、FCSR、8 个 FCC，reserved=0
  -> r4=TrapContext*
  -> trap_entry_rust
  -> arch-api 注册的 wateros trap handler
```

与 RISC-V 不同，进入 Rust 前通常不切到 kernel PGDL。DMW0 允许 PLV0 直接访问内核 RAM/MMIO，所以 `user_trap_requires_kernel_address_space()` 返回 false，`prepare_user_trap_frame_access()` 也是 no-op。不要机械增加一次 PGDL 往返和全量 flush。

### 5.3 Rust 业务调用链

```text
trap_entry_rust(frame)
  -> kernel_trap::invoke_kernel_trap_handler
  -> src/trap_handler.rs::wateros_kernel_trap_handler
       ├─ syscall dispatcher
       ├─ page fault / VMA / COW
       ├─ timer rearm + scheduler tick
       ├─ IPI reason + pending clear
       └─ signal delivery / rt_sigreturn
  -> task::restore_current_trap_frame（用户帧）
  -> 返回 trap.S
```

回调必须在首次开中断/进入用户态前注册。未注册时通用 API 会 panic。

## 6. 返回路径

Rust 返回后先写 PRMD/ERA，再按 PPLV 分流。

### 6.1 返回内核

恢复 LSX、FCC、FCSR、通用寄存器，释放 832 字节栈帧后 `ertn`。内核路径不根据 frame[36] 切 PGDL；它应继续使用 trap 入口时的地址空间。

### 6.2 返回用户

1. 从 token 解出 PGDL 低 48 位和 10 位 ASID；
2. 只有值变化时才写 PGDL/ASID；
3. 不在这里 `invtlb`，依赖 MM 的 ASID 分配/复用协议；
4. 设置 `CRMD.PG=1`、清 `CRMD.DA`；
5. 把 frame 结束地址（内核栈顶）写入 `SAVE0`；
6. 恢复 LSX、FCC、FCSR、GPR；
7. 最后从 frame 恢复用户 r3/sp，`ertn`。

`set_return_to_user()` 将 PRMD.PPLV 设 3、PIE 设 1；`set_return_to_kernel()` 只清 PPLV。不要直接信任用户 signal frame 中的 PRMD 特权位。

### 6.3 当前已知的诊断/恢复缺口

- `__wateros_arch_restore_user_task`（首次用户任务/exec/fork 入口）恢复 LSX 和 FCSR，但当前没有恢复 frame[816] 的 FCC；普通 trap-return 会恢复 FCC。依赖浮点比较条件码在首次 `ertn` 前保持的程序需要补齐该路径。
- `trap.S` 设置 CFA 为 `sp+832`，保存的 RA 在 `sp+8`，按布局 RA 相对 CFA 应为 `-824`；源码当前 `.cfi_offset 1, -288` 与布局不一致。这只影响 GDB/DWARF 回溯，不改变实际寄存器恢复。

修复第一项时可复用 `RESTORE_FCC` 逻辑，但该宏当前定义在 `trap.S`，而首次恢复位于 `switch.S`；应选择公共 include/macro 或在 `switch.S` 明确实现 8 位恢复，并做浮点比较跨 exec/fork 测试。修复第二项后用最终 ELF 的 DWARF frame 信息和内核 trap backtrace 验证。

## 7. Trap 初始化与页表硬件

`init_trap()` 操作的是 per-CPU CSR，因此 BSP 和每个 AP 都要执行：

1. `DMW0=0x11`：仅 PLV0 可用的一致可缓存直接映射，不开放 PLV3；
2. `EENTRY=__alltraps`；
3. `TLBRENTRY=__tlb_refill`；
4. `STLBPS=12`、`TLBREHI=12`：4 KiB 页；
5. 写 PWCL/PWCH，配置当前 4 KiB、三级页表 walk；
6. `ASID=0`；
7. `EUEN.FPE|SXE`，允许基础浮点与 LSX；
8. 全量 `invtlb 0,$zero,$zero`。

DMW0 绝不能开放 PLV3，否则用户可绕过 PGDL/TLB 直接访问物理内存。改变页表层级/位宽时必须同步 MM 实现、PWCL/PWCH、PTE 格式、TLB refill 代码和 linker/映射布局。

### 7.1 TLB refill

`__tlb_refill` 按硬件 refill 约定：

```text
把 r12 暂存 CSR.TLBRSAVE
  -> CSR.PGD 取当前 fault 对应的页目录根
  -> lddir ..., level=3
  -> lddir ..., level=1
  -> ldpte 偶/奇两项
  -> tlbfill
  -> 恢复 r12
  -> ertn
```

它不调用 Rust、不能分配内存，也不处理缺页。页表项无效或权限不满足时会进入普通 exception 路径，由 `BADV/ESTAT` 驱动 VMA/COW。修改 refill 时要确认临时寄存器保存、4 KiB 配置、偶/奇 PTE 配对和入口 4 KiB 对齐。

## 8. Cause 解码

解码先看 ESTAT pending，再看 ecode。若 IPI 与 timer 同时 pending，当前优先返回 soft/IPI；处理后下一次 trap 才处理 timer。

| 判据 | 通用 cause | 说明 |
|---|---|---|
| IS bit12 | `SupervisiorSoft` | IPI 优先 |
| IS bit11 | `SupervisiorTimer` | timer 次之 |
| ecode 1/7/8 | `LoadPageFault` | 含 load 类页错 |
| ecode 2/4 | `StorePageFault` | ecode 4 PME 用于写保护/COW |
| ecode 3/6 | `InstructionPageFault` | 取指类页错 |
| ecode 9/12 | `Breakpoint` | 当前归一到 breakpoint |
| ecode 11 | `UserEnvCall` | syscall |
| ecode 13 | `IllegalInstruction` | 非法指令 |
| 其它 | `Unsupported(ecode)` | 保留原 ecode |

`fault_addr()` 返回 BADV。不要只凭 BADV 非零判断页错；IPI/timer 时该 CSR 可保留旧值。

## 9. 系统调用 ABI 与新增实例

LoongArch64 ABI：

- 参数：`r4..r9`；
- syscall 编号：`r11`；
- 返回值：`r4`；
- 用户 sp：`r3`；TLS：`r2`；返回 PC：ERA；
- syscall 指令为固定 4 字节，正常返回由业务层推进 ERA。

新增 `sys_foo(fd, user_ptr, len)`：

```text
r11=SYS_FOO, r4=fd, r5=user_ptr, r6=len, syscall 0
  -> ESTAT ecode=11
  -> syscall_nr() / syscall_args()
  -> 通用 dispatcher
  -> user-copy + 业务模块
  -> set_syscall_ret() 写 r4
  -> ERA += 4
  -> signal/restart
  -> ertn
```

架构层无需为每个 syscall 加 match。编号、参数验证和实现应放 syscall 模块；用户指针必须经过 user-copy。restart 会把 PC 减去指令长度，把原参数写回 r4..r9、编号写回 r11。

至少测试成功、负 errno、无效/跨页指针、六参数、信号打断、并发 close 和 fork 后资源语义。

## 10. 信号上下文

capture 保存 GPR、ERA、PRMD、FPR 低半、FCSR、FCC 和完整 LSX。restore：

- 拒绝 PC=0；
- 要求 PC 4 字节对齐（`pc & 3 == 0`）；
- 强制 `r0=0`；
- 恢复完整 vector，再用 `fpregs` 覆盖每个 vector 的低 64 位；
- 恢复 FCSR/FCC；
- 调用 `set_return_to_user()`，不信任用户提供的特权状态。

handler 寄存器：`r1=restorer`、`r3=frame_sp`、`r4/r5/r6=signal/siginfo/ucontext`、ERA=handler。

修改 LSX/LASX 支持时要同步 frame 大小、16/32 字节对齐、EUEN、汇编宏、signal ABI、fork/exec 和调度压力测试。

## 11. PGDL、ASID、MMU 与 TLB

### 11.1 WaterOS token

```text
bit 47:0   PGDL 值
bit 57:48  10-bit ASID
其余位     当前应为 0/忽略
```

`active_address_space_token()` 分别读 PGDL 和 ASID 后编码。`initialize_address_space_ids()` 当前固定返回 10，没有像 RISC-V 一样做 WARL 探测；移植到 ASID 宽度不同的硬件前必须改为平台数据或可靠探测。

### 11.2 本地 flush 当前是保守实现

`Page`、`Range`、`AddressSpace`、`All` 四种请求全部执行：

```asm
invtlb 0, $zero, $zero
```

因此语义正确但代价高，调用方不能假定已经按地址/ASID 精确刷新。若优化，先按 LoongArch 指令定义实现各 op，再覆盖非法 VA、ASID 复用、全量 rollover 和 SMP shootdown。

所有 `invtlb` 仍只作用本 CPU。页表写入、远端 reason 发布、IPI、远端 flush、ack、页框释放的顺序与 RISC-V 同样不可省略。

### 11.3 MMU 模式切换

- `init_paging_disable_mmu()`：设置 `CRMD.DA=1`、清 `CRMD.PG`，状态变化时全量 invtlb；
- `activate_address_space_token_and_flush()`：写 PGDL、ASID并全量 invtlb；
- `enable_paging()`：设置 `PG=1`、清 `DA`；
- trap 用户返回也保证 `PG=1, DA=0`，但依赖 ASID 协议而不主动 invtlb。

关闭 MMU 前必须确保当前代码、栈和必要 MMIO 在直接地址模式/DMW 下仍可访问，否则 CSR 写入后的下一条取指就会失败。

## 12. 中断、时间和 IPI

### 12.1 中断位

- `CRMD.IE` bit2：全局中断；
- `ECFG` bit11：timer 中断使能；
- `ECFG` bit12：IPI 中断使能；
- idle 指令：`idle 0`。

irq-save 读取完整 CRMD，但 restore 只恢复 IE bit2。opaque state 不是完整 CSR 写回凭据。

`clear_soft_interrupt()` 读取 IOCSR `IPI_STATUS(0x1000)`，将当前所有 pending bits 原样写到 `IPI_CLEAR(0x100c)`。必须先保证 software reason 已被可靠取走，否则硬件 pending 清除后可能丢失唤醒；反过来不清会形成中断风暴。

本 arch crate 没有 IPI send 模块。LoongArch 目标选择/mailbox transport 位于 QEMU platform profile；聚合层的通用业务 IPI 应调用 `platform::smp::send_ipi`，不要调用仅为 RISC-V SBI 提供的 arch IPI façade。

### 12.2 StableCounter

`rdtime.d` 同时给出 tick 和 counter id；当前只返回 tick，忽略 counter id。频率查询返回 `Unsupported`，由 platform 提供 Hz。调度 slice 为 `10_000_000` 原始 tick，不是纳秒，也不能与 RISC-V slice 数值直接比较。

## 13. 常见故障表

| 症状 | 优先检查 |
|---|---|
| 用户第一次运行就异常 | SAVE0 栈顶、PRMD/ERA、PGDL/ASID、CRMD.PG/DA |
| 用户 trap 覆盖别的任务栈 | 任务迁移/切换后 SAVE0 是否更新 |
| 用户可读内核物理地址 | DMW0 是否错误开放 PLV3 |
| COW 一直重复 fault | PME ecode=4 是否归 StorePageFault；PTE dirty/write 位 |
| fork 后偶发旧映射 | ASID 复用和跨核 shootdown/ack |
| LSX 跨任务串值 | EUEN、832 字节 frame、全部 vr 保存恢复 |
| 首次恢复后浮点条件异常 | `__wateros_arch_restore_user_task` 未恢复 FCC 的已知缺口 |
| GDB trap 回溯错误 | 当前 `.cfi_offset 1,-288` 与 frame 布局不符 |
| IPI 风暴 | IOCSR pending 是否清；reason 是否已处理 |
| timer/IPI 同时到达遗漏 timer | cause 优先级与第二次 trap、timer pending 清除 |
| AP 启动 InvalidCpu | mailbox 逻辑 id 与 CSR.CPUID 不一致 |
| 改页表层级后 refill 崩溃 | PWCL/PWCH、lddir 层级、PTE 格式未同步 |

调试 trap 时记录 `ESTAT/ERA/BADV/PRMD/PGDL/ASID/CPUID/SAVE0`。在内核栈尚未建立前不要调用会使用栈、锁或 allocator 的 Rust 日志。

## 14. ABI 修改清单

修改 `TrapContext`：

- Rust `#[repr(C)]` 字段与 const assert；
- `TRAP_CONTEXT_SIZE`、全部 GPR/CSR/LSX/FCSR/FCC offset；
- SAVE/RESTORE 宏和首次用户恢复；
- CFI 的 CFA/RA offset；
- task 保存区、signal ABI、fork/exec 初始 frame；
- 16 字节（LASX 时可能更高）对齐。

修改页表格式：

- MM 的 PTE 位与三级索引；
- PWCL/PWCH/STLBPS/TLBREHI；
- `__tlb_refill` 的 `lddir/ldpte`；
- token PGDL 掩码、ASID 位宽；
- DMW 与内核链接/映射地址；
- shootdown 和 ASID rollover。

修改 CPU/SMP：同步 boot stack 数量、CPUID 映射、platform mailbox、IOCSR 发送、per-CPU 数组和 CPU mask 宽度。

## 15. 回归矩阵

静态门禁：

```bash
cd os
cargo fmt --all -- --check
make check ARCH=la PROFILE=pre
python3 scripts/maintenance/check_offline_docs.py
```

宿主编译不能证明 CSR 和汇编运行语义。QEMU/硬件至少运行：

| 类别 | 用例 | 判据 |
|---|---|---|
| boot | SMP=1、2、8 | CPUID 匹配、独立栈、全部 online |
| switch | 高频 yield/迁移 | r1/r3/r22..r31 完整 |
| syscall | 六参数 + errno | r4..r9/r11、ERA 推进正确 |
| page walk | 多层边界映射和 unmapped fault | refill 正确、BADV/ecode 正确 |
| COW | 父子跨 CPU 写同页 | PME 被修复、内容隔离、无旧 TLB |
| FP/LSX | 多线程向量+比较条件并抢占 | vr/FCSR/FCC 不串值 |
| signal | handler、rt_sigreturn、restart | PC/SP/TLS/LSX/FCC 恢复 |
| IPI | reschedule + TLB reason 并发 | reason 不丢、pending 不风暴 |
| ASID | 强制复用/rollover | 无跨地址空间旧翻译 |
| MMU | disable/build/enable 序列 | 每阶段代码栈可访问 |
| fork | forkheavy 长压 | 栈/页表/ASID 可持续回收 |

修改汇编后反汇编最终 ELF，并检查 `.eh_frame`/DWARF frame 信息、入口对齐、实际指令和所有 offset。链接成功只能作为第一道门禁。
