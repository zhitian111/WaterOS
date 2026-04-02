# WaterOS 内核推进 TodoList（接口定义 vs 实现）

目标：**QEMU riscv64** 上运行 **glibc 生态程序（含 busybox）**；文件系统 **ramfs + ext4**；驱动阶段 **仅 virtio**；用户态测试见仓库 `user/`。

本文档将工作拆为 **【接口】**（类型、trait、常量、文档契约）与 **【实现】**（可运行代码），并按 **依赖顺序** 排列。勾选 `[ ]` 表示未完成，`[x]` 表示完成。

---

## 图例

| 标记 | 含义 |
|------|------|
| 【接口】 | 在对应 `*-api/api-v0`（或 `wateros-base`）中定义类型与 trait，不写具体算法 |
| 【实现】 | 在 `impl-*`、平台/架构实现、内核主路径中落地 |
| 依赖 | 必须先完成的步骤编号 |

---

## 阶段 0：ABI 与错误模型（全栈前提）

### 0.1 【接口】Linux riscv64 系统调用与 errno 约定

- **文档/常量（建议放在 `wateros-abi/abi-api/api-v0` 或独立 `abi-linux-riscv64` 模块）**
  - [ ] `SyscallNumber`：`u32` 或 `u64` 的 **newtype**，列出与 **Linux 内核 uapi** 一致的编号（至少覆盖：`read/write/openat/close`、`brk`、`mmap/munmap/mprotect`、`clone/execve/exit`、`wait4`、`futex`、`rt_sigaction/rt_sigprocmask/rt_sigreturn`、`clock_gettime/gettimeofday`、`uname`、`ioctl`、`fcntl`、`pipe2`、`dup/dup2`、`getcwd/chdir`、`prctl`、`set_tid_address`、`set_robust_list` 等——可按里程碑分批追加，但 **编号表只能有一份权威来源**）。
  - [ ] `Errno`：与 Linux 一致的 **正值 errno 枚举或常量**（如 `EINVAL`、`ENOMEM`）。
  - [ ] `UserRet` 或约定说明：**用户可见返回值 = `-errno`** 时的类型表示（通常为 `isize`，负数为错误）。
- **与 `user/` 测试对齐策略**
  - [ ] 列出当前 `user` 中 **非标准号**（若有）与 Linux 官方号差异表，决定「改用户态测试」或「过渡期兼容层」。

**依赖**：无。

---

### 0.2 【接口】`ecall` 参数打包/解包

- **类型**
  - [ ] `SyscallArgs`：从 `a0..a6`（及必要时 `a7` 已作 syscall id）映射的结构体，或固定长度数组 + 访问器。
  - [ ] `SyscallResult`：内核侧 `Result<usize, Errno>` 与用户态 `isize` 互转规则。
- **trait（可选）**
  - [ ] `FromTrapFrame` / `IntoTrapFrame`：从 `TrapFrame` 读写参数（见阶段 1），避免 syscall 分发处散落字段名。

**依赖**：0.1。

---

## 阶段 1：Trap、特权级与 CPU 上下文（`platform-arch-api` + 汇编实现）

### 1.1 【接口】异常与陷入原因

- **类型**
  - [ ] `TrapCause`：枚举 — `UserEcall`、`TimerInterrupt`、`ExternalInterrupt`、`InstructionPageFault`、`LoadPageFault`、`StorePageFault`、`IllegalInstruction` 等（与 RISC-V `scause` 可建立 `From`/`TryFrom`）。
  - [ ] `InterruptStatus` / `PrivMode`：若需要显式建模（可选）。

### 1.2 【接口】Trap 帧与用户寄存器镜像

- **类型**
  - [ ] `GpRegs`：`x1..x31` 中 **需要在 U↔S 切换时保存** 的子集（通常先全量保存便于调试与 signal 扩展）。
  - [ ] `TrapFrame`：`GpRegs` + `sepc` + `sstatus`（及 `scause`/`stval` 若由软件读入）等；字段布局需 **固定**（后续 `sigreturn` 依赖）。
- **trait（可选）**
  - [ ] `UserContext`：抽象「可恢复执行用户线程」的最小操作集（`pc()`、`set_pc`、`sp`、`syscall_nr()` 等）。

### 1.3 【接口】与平台引导的衔接

- **类型**
  - [ ] `PerCpuTrapState` 或 `TrapHandler` 注册表（若多核预留；单核可先 `()`）。

**依赖**：0.1（errno 可先独立）、`wateros-base` 地址类型。

### 1.4 【实现】Trap 向量与 `ecall` 路径

- [ ] `_trap_entry` 汇编保存寄存器 → 调 Rust `trap_handler(cause, frame)`。
- [ ] `ecall from U-mode` 分支进入 **syscall 分发**（阶段 4 接线）。

**依赖**：1.1–1.3。

---

## 阶段 2：物理内存与虚拟内存（`wateros-base` + `mm-api`）

### 2.1 【接口】物理资源与对齐

- **类型（`wateros-base` 或 `mm-api`）**
  - [ ] `PhysFrameId` / `PhysAddr`：已存在则统一 **构造与校验** 规则（对齐到 4KiB）。
  - [ ] `VirtAddr`、`Vpn`、`Ppn`：Sv39 三级索引拆分（若与 `addr.rs` 重复则合并导出）。
  - [ ] `PagePerm`：`R`/`W`/`X`/`U` 位（对应 PTE flags）。
  - [ ] `MapFlags`：`MAP_ANONYMOUS`、`MAP_PRIVATE` 等 **与 Linux mmap 对齐的子集**（先做内核所需最小集）。

### 2.2 【接口】地址空间句柄

- **类型**
  - [ ] `AddressSpaceId`：newtype（PID 可复用或独立 ASID）。
  - [ ] `AddressSpace`：**不透明句柄** 或 trait object 由实现侧定。
- **trait**
  - [ ] `AddressSpaceOps`（或 `AddressSpace` trait）：
    - `map(virt, phys, perm) -> Result`
    - `unmap(virt) -> Result`
    - `protect(virt, perm) -> Result`
    - `satp_value() -> usize` 或等价物（安装页表）。
  - [ ] `PhysicalAllocator`：
    - `alloc_frame() -> PhysFrame`
    - `dealloc_frame(PhysFrame)`（若暂不做引用计数，文档注明 **泄漏 acceptable 的边界**）。

### 2.3 【接口】`brk` 与堆区间

- **类型**
  - [ ] `BrkRegion`：`start`/`current_end`/`max`（或仅 `brk` 边界对）。
- **trait**
  - [ ] `HeapBrk`：`brk(new_end) -> Result<VirtAddr>`，定义 **与 ELF `end` 数据段对齐规则**。

### 2.4 【接口】`mmap` 占位（为 glibc 预留）

- **类型**
  - [ ] `MmapKind`：`Anonymous` | `File { fd, offset }`。
- **trait**
  - [ ] `MmapOps`：`mmap(addr, len, prot, flags, fd, off)` 的最小子集签名（可先 `unimplemented` 在实现层）。

**依赖**：1.x（用户态进出）、`wateros-base`。

### 2.5 【实现】Sv39 页表、内核映射、切换 `satp`

- [ ] 伙伴系统或简单帧分配器。
- [ ] 首进程用户映射：代码/数据/栈。

**依赖**：2.1–2.4。

---

## 阶段 3：任务、调度与资源（`task-api` + `scheduler-api` + IPC 基础类型）

### 3.1 【接口】进程与线程标识

- **类型**
  - [ ] `Pid`、`Tid`：newtype（`NonZeroU32` 等）。
  - [ ] `ProcessState`：`Running`、`Blocked`、`Zombie` 等。

### 3.2 【接口】进程控制块（抽象字段，非具体结构体布局强制）

- **类型**
  - [ ] `Process`：**trait** 或关联类型容器，至少包含：
    - 地址空间句柄
    - 文件描述符表（见阶段 5）
    - 父/子关系（`wait`）
    - 退出码、`thread_group`（为 `clone` 预留）

### 3.3 【接口】调度器

- **trait**
  - [ ] `Scheduler`：`enqueue`、`pick_next`、`yield_cpu`。
  - [ ] `SchedEntity`：可调度单元（先单线程进程即可）。

### 3.4 【接口】与 `waitqueue` 的边界（`ipc-waitqueue` 可与 task 协同）

- **trait**
  - [ ] `WaitQueue`：`wait()` / `wake_one()` / `wake_all()` 的 **无锁语义说明**（实现可后补）。

### 3.5 【实现】最小调度 + `yield` 测通

- [ ] 与 `user` 测试 `sys_yield` 对齐。

**依赖**：2.5、1.4。

---

## 阶段 4：系统调用层（`abi-api` + 内核 `syscall/` 模块）

### 4.1 【接口】统一分发入口

- **类型**
  - [ ] `SyscallDispatchError`：未知号、坏参数、权限等。
- **trait**
  - [ ] `SyscallHandler`：`fn handle(&mut ProcessContext, nr, args) -> SyscallResult`（`ProcessContext` 为 task 与 trap 的粘合类型，可定义在 `abi-api` 或 `task-api`）。

### 4.2 【接口】按子系统划分的 handler 表

- **类型**
  - [ ] `SyscallTable`：`*const SyscallHandler` 或 `fn` 指针表（长度 = 最大 syscall 号策略：稀疏表 vs 二分）。

### 4.3 【实现】第一批 syscall（建议顺序）

- [ ] `write`、`exit`、`yield`（或 `sched_yield`）、`brk`、`gettimeofday`、`uname`。
- [ ] 未实现：返回 `-ENOSYS`。

**依赖**：0.2、3.5、4.1–4.2。

---

## 阶段 5：VFS 与文件描述符（`vfs-api`）

### 5.1 【接口】文件系统无关层

- **类型**
  - [ ] `InodeId`、`DevId`、`FileMode`。
  - [ ] `Path`：借用路径 `&str` 或内部 buffer 策略（`no_std`）。
  - [ ] `OpenFlags`：`O_RDONLY`/`O_WRONLY`/`O_RDWR`/`O_CREAT`/`O_DIRECTORY` 等子集。

### 5.2 【接口】核心对象 trait

- **trait**
  - [ ] `Inode`：`metadata()`、`lookup(name)`、`read_at`/`write_at`（或 `File` 上）。
  - [ ] `File`：`read`、`write`、`seek`（`lseek`）。
  - [ ] `Filesystem`：`mount`、`root_inode()`。
  - [ ] `DentryCache`（可选）：或 `PathResolver`：`resolve(path) -> Inode`。

### 5.3 【接口】每进程 fd 表

- **类型**
  - [ ] `Fd`：`usize` newtype（0..`RLIMIT_NOFILE`）。
- **trait**
  - [ ] `FdTable`：`get`、`insert`、`dup`、`close`。

**依赖**：3.2（进程资源容器）。

### 5.4 【实现】ramfs 根 + 打开静态 ELF

- [ ] `openat`、`read`、`close` syscall。
- [ ] 从 ramfs 读入 ELF 到内存并映射执行（阶段 2 接线）。

**依赖**：5.1–5.3、2.5。

---

## 阶段 6：可执行文件与进程执行（`wateros-abi` 或 `task-api` 子模块）

### 6.1 【接口】ELF 与加载

- **类型**
  - [ ] `ElfHeader`、`ProgramHeader`：**视图类型**（`from_bytes` 校验魔数）。
  - [ ] `AuxvEntry`、`AT_NULL`、`AT_PAGESZ`、`AT_PHDR`、`AT_ENTRY` 等常量。
  - [ ] `LoadPolicy`：如何处理 `PT_LOAD` 重叠、对齐。

### 6.2 【接口】`execve` 语义

- **trait**
  - [ ] `Execve`：`load_elf(path, argv, envp, current_mm) -> Result<NewUserMain>`。

### 6.3 【实现】静态 ELF `execve`；再 `clone`/`wait4`

- [ ] 与用户态 `exec` 测试对齐（syscall 号需 **Linux 官方**）。

**依赖**：5.4、4.3、0.1。

---

## 阶段 7：文件系统具体实现（`fs-api` + `impl-*`）

### 7.1 【接口】ramfs

- **trait**
  - [ ] `RamfsOps`：`create`、`unlink`、`mkdir`（busybox 需要基础文件操作）。

### 7.2 【接口】ext4（只读先行）

- **类型**
  - [ ] `Ext4Super` 视图字段（块大小、inode 数等）— 可先为 **读接口** 服务。
- **trait**
  - [ ] `Ext4ReadOnlyFs`：`read_inode`、`read_extent`（名称可自定，但要与 `Filesystem` 对接）。

### 7.3 【实现】ramfs 完整最小集；ext4 只读

**依赖**：5.4、块设备（阶段 8）。

---

## 阶段 8：块设备与 virtio-blk（`driver-api` / `block-api`）

### 8.1 【接口】块请求

- **类型**
  - [ ] `Sector` / `Lba`：`u64` newtype。
  - [ ] `Bio` 或 `BlockReq`：`start_lba`、`blocks`、`buffer`（物理或内核虚拟）。

### 8.2 【接口】块设备

- **trait**
  - [ ] `BlockDevice`：`submit_sync(req) -> Result`（首版同步即可）。
  - [ ] `BlockDeviceOps`：容量查询、`logical_block_size`。

### 8.3 【接口】virtio（仅 MMIO transport）

- **类型**
  - [ ] `VirtioMmioRegs`：寄存器偏移常量。
  - [ ] `Virtqueue`：描述符环视图（与实现 crate 类型可分层：`api` 只放 **契约**，寄存器布局可放 `wateros-driver-block` 子模块）。

### 8.4 【实现】virtio-blk + 简单块缓存

**依赖**：1.4（中断或轮询）、2.1（DMA/缓冲用内核虚拟地址策略需在文档说明）。

---

## 阶段 9：IPC 与 glibc 关键路径（`ipc-api` 及子 crate）

### 9.1 【接口】futex

- **类型**
  - [ ] `FutexKey`：用户虚拟地址 + `AddressSpaceId`（或 mm 命名空间 id）。
  - [ ] `FutexOp`：`WAIT`、`WAKE` 等 Linux 子集。

### 9.2 【接口】pipe

- **trait**
  - [ ] `Pipe`：实现为一对 `File` 或独立类型，能挂到 `FdTable`。

### 9.3 【接口】信号（可先 stub）

- **类型**
  - [ ] `Sigset`、`SigAction`、`SigInfo` 的最小占位。
- **trait**
  - [ ] `SignalDelivery`：`enqueue`、`deliver_to_user`（返回 `-EINTR` 或排队）。

### 9.4 【实现】按 strace 迭代补齐

- [ ] `futex`、`rt_sig*`、`clone`、`set_tid_address`、`set_robust_list` 等直至 **动态链接 glibc busybox**。

**依赖**：4.x、6.x、5.x、3.x。

---

## 阶段 10：字符设备与控制台统一（`driver-character` + devfs）

### 10.1 【接口】字符设备

- **trait**
  - [ ] `CharDevice`：`read`、`write`、`ioctl` 子集。

### 10.2 【接口】devfs（或 devtmpfs）

- **trait**
  - [ ] `Devfs`：`mknod`、`lookup("/dev/console")`。

### 10.3 【实现】

- [ ] 将 **SBI 控制台** 或 **virtio-console** 接到 `fd 0/1/2`。

**依赖**：5.x。

---

## 依赖链小结（执行顺序）

```
0 ABI/errno
 → 1 Trap/TrapFrame
 → 2 MM/Sv39/brk/mmap 契约
 → 3 Task/Scheduler
 → 4 Syscall 分发 + 首批调用
 → 5 VFS + fd
 → 6 ELF + execve
 → 7 ramfs + ext4(RO)
 → 8 virtio-blk
 → 9 futex/信号/clone 等 glibc 缺口
 → 10 /dev 与控制台
```

---

## 维护说明

- **接口变更**：在对应 `api-v0` 的 `CHANGELOG` 或本文件顶部增加「修订记录」。
- **实现并行**：阶段 8 可与阶段 7 部分并行，但 **ext4 必须晚于块设备接口稳定**。
- **权威 syscall 表**：只维护一份；`user/` 与内核分发共用同一编号源（可通过生成脚本或共享 crate，仍属「接口源」）。

---

*文档版本：1.0 — 与仓库 `os/components/*/api-v0` 结构对应，可按实际 crate 名微调 trait 所在包。*
