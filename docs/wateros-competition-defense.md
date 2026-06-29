# WaterOS — 操作系统内核设计与实现

**全国大学生操作系统竞赛（OS Kernel 赛道）参赛项目**

| 项目 | 内容 |
|------|------|
| 参赛队伍 | 山东大学（青岛） |
| 开发语言 | Rust（no_std 环境） |
| 支持架构 | RISC-V64 / LoongArch64 |
| ABI 兼容 | Linux generic 64-bit syscall ABI |
| 内核类型 | 单核、无宏/微内核偏好、组件化分层设计 |

---

## 一、项目概述

WaterOS 是一款基于 **Rust no_std** 环境从零构建的操作系统内核，同时支持 **RISC-V64** 与 **LoongArch64** 双架构。内核采用**组件化 crate 组织**，通过清晰的 API/impl 分层降低模块间耦合，在两种指令集架构上保持一致的用户态对象模型。

### 核心成果

- **双架构完整主路径**：RISC-V64 与 LoongArch64 均具备从引导启动、页表初始化、设备发现、文件系统挂载到用户态程序加载运行的全链路能力
- **Linux ABI 兼容**：采用 Linux generic 64-bit 系统调用约定，已实现 **100+ 个系统调用**，涵盖文件 I/O、进程管理、内存管理、信号、IPC、网络 socket、定时器等领域
- **完整用户态路径**：ELF 装载、地址空间切换、系统调用分发、文件描述符、管道、procfs、socket 已形成连续链路
- **竞赛测例验证**：BusyBox（glibc + musl）55/55 全量通过；basic 测例持续解锁中

### 技术栈与工具链

| 层面 | 技术选型 |
|------|----------|
| 编程语言 | Rust（2021 edition），no_std，禁用默认全局分配器 |
| RISC-V 模拟 | QEMU virt（opensbi bios），Sv39 页表，VirtIO-MMIO |
| LoongArch 模拟 | QEMU virt（LoongArch 固件），三级页表，VirtIO-PCI |
| 存储 | ext4 根文件系统（VirtIO-Blk），page cache |
| 网络 | VirtIO-Net + smoltcp 协议栈 |
| 调试 | QEMU 日志、GDB、PC 监视脚本、符号解析 |

---

## 二、总体架构

### 2.1 分层设计

WaterOS 采用**五层分层架构**，上层模块围绕用户态可见语义组织，底层模块负责吸收架构、固件、中断、页表和设备总线差异。

```
┌─────────────────────────────────────────────────────┐
│                   用户层                              │
│   用户程序 / BusyBox / libc / 测试脚本 / shell        │
├─────────────────────────────────────────────────────┤
│                    ABI 层                             │
│           Linux generic 64-bit syscall ABI            │
├─────────────────────────────────────────────────────┤
│                   内核核心层                           │
│  syscall │ task │ mm │ vfs │ fs │ ipc │ net │ cred  │
├─────────────────────────────────────────────────────┤
│                   驱动层                               │
│       driver │ block │ char │ net │ virtio            │
├─────────────────────────────────────────────────────┤
│                   平台层                               │
│   platform │ arch │ trap │ timer │ firmware           │
├─────────────────────────────────────────────────────┤
│                   机器层                               │
│       RISC-V64 QEMU virt / LoongArch64 QEMU virt      │
└─────────────────────────────────────────────────────┘
```

### 2.2 组件化结构

WaterOS 不采用单体内核设计，而是将功能拆分为 **13 个一级组件**，每个组件遵循统一的 API/impl 范式：

| 一级组件 | 职责 | 核心子 crate |
|----------|------|-------------|
| `wateros-platform` | 平台抽象、arch 差异、trap/timer | `platform-api/api-v0`，`impl-qemu-riscv64-opensbi` |
| `wateros-driver` | 块/字符/网络设备驱动 | `driver-block`，`impl-virtio-mmio`，`impl-virtio-pci` |
| `wateros-mm` | 内存管理、页表、ELF 装载 | `mm-api/api-v0`，`impl-sv39`，`impl-loongarch64` |
| `wateros-task` | 任务/进程/调度/等待 | `task-api/api-v0`，`scheduler-impl/impl-multi-class` |
| `wateros-fs` | 文件系统实现 | `fs-api/api-v0`，`impl-ext4`，`fs-devfs`，`fs-rootfs` |
| `wateros-vfs` | 虚拟文件系统、fd 会话 | `vfs-api/api-v0`，`impl-fs-bridge`，`impl-fd-session` |
| `wateros-ipc` | 进程间通信 | pipe，futex，signal，shm，waitqueue |
| `wateros-syscall` | 系统调用分发表 | `syscall-api/api-v0`，`impl-kernel` |
| `wateros-abi` | ABI 定义、错误码 | `abi-api/api-v0`，`impl-linux-generic64` |
| `wateros-cred` | 进程凭证管理 | `cred-api/api-v0`，`impl-root` |
| `wateros-klog` | 内核日志环 | `klog-api/api-v0`，`klog-impl/klog-ringbuf` |
| `wateros-runtime` | 运行时基础设施 | console，logging，panic，heap-allocator |
| `wateros-base` | 基础类型与配置 | `base-config` |

### 2.3 API/Impl 设计范式

每个一级组件遵循统一的**三层次架构**：

```
component/
├── src/lib.rs          ← 聚合层：统一导出，feature 选择实现
├── component-api/
│   └── api-v0/src/     ← API 契约层：trait、类型、错误、常量
└── component-impl/
    ├── impl-xxx/src/   ← 具体实现层：平台/算法差异
    └── impl-yyy/src/   ← 另一套实现（feature 互斥）
```

#### 设计原则

- **API 层只定义契约**，不实现具体平台逻辑
- **impl 层严格围绕 API 契约实现**，不擅自扩大边界
- **聚合层通过 feature 绑定实现**并给出统一导出名
- 新增 impl 时须同步补齐 `Cargo.toml`、feature 传递链和文档

### 2.4 双架构适配策略

两条架构路径共享上层内核逻辑（task、MM、FS/VFS、IPC、syscall），底层保留各自的实现：

| 能力 | RISC-V64 路径 | LoongArch64 路径 |
|------|--------------|-----------------|
| 引导固件 | OpenSBI + hart id / DTB | QEMU virt argc/argv/envp |
| 页表 | Sv39 / satp / sfence.vma | 三级页表 / CSR / invtlb |
| Trap 处理 | trap frame + sret | trap frame + ertn |
| 设备总线 | VirtIO-MMIO | VirtIO-PCI |
| 帧分配 | `impl-stack`（共享） | `impl-stack`（共享） |
| 用户切换 | `__switch` | `__switch` |
| 页表实现 | `impl-sv39` | `impl-loongarch64` |

两条路径在进入上层内核后使用**同一套对象模型**（任务控制块、地址空间、VFS 文件对象等），最大限度复用代码。

---

## 三、模块详细设计

### 3.1 启动、异常与系统调用入口

#### 启动流程

```
entry
  ↓
清 BSS / 初始化堆 / 日志系统
  ↓
平台初始化（console / timer / device discovery）
  ↓
MM 自检 → 内核页表初始化
  ↓
驱动初始化（DTB 扫描 → virtio-blk 注册 → devfs 刷新）
  ↓
FS/VFS 自检（ext4 根卷挂载 → VFS 桥自检）
  ↓
procfs 挂载
  ↓
调度器初始化 → 创建内核/用户任务
  ↓
启用中断与定时器 → run_first_task
```

#### 异常与中断处理

- 异常、中断和系统调用**统一进入 trap 处理路径**
- `timer interrupt` 推动调度器获得时间片（轮转调度）
- syscall 根据 ABI 编号进入分发表，再调用 task、MM、VFS、IPC 等子系统
- 用户态返回值统一按 **Linux errno 语义**处理
- 信号处理在 trap 返回路径中检查 pending 信号并构造 signal frame

### 3.2 内存管理

#### 地址空间架构

```
内核地址空间             用户地址空间
┌──────────────────┐    ┌──────────────────┐
│  内核段映射       │    │  text / rodata   │ RX
│  设备 MMIO 映射   │    │  data / bss      │ RW
│  全局页表         │    │  heap (brk)      │ RW
│  权限隔离         │    │  mmap 区域       │ RW/RX
│                   │    │  user stack      │ RW
└──────────────────┘    │  argv/envp/auxv  │
                         └──────────────────┘
                              ↑
                         页表映射
                   Sv39 / LoongArch64 三级页表
                              ↑
                        物理页帧
                      frame allocator
```

#### 已实现能力

| 能力 | 状态 | 说明 |
|------|------|------|
| 三级 4 KiB 页表 | ✅ | Sv39 + LoongArch64 三级页表 |
| 内核恒等映射 | ✅ | RAM + MMIO 区域 |
| 用户 ELF 装载 | ✅ | `from_elf_path` / `from_elf_bytes` |
| 用户栈构建 | ✅ | argc/argv/envp/auxv |
| brk / mmap / munmap | ✅ | 用户堆 + 文件/匿名/共享映射 |
| mprotect | ✅ | 运行时权限调整 |
| fork COW | ✅ | 写时复制地址空间 |
| 用户缺页处理 | ✅ | 栈/brk/lazy mmap |
| mremap | ✅ | 地址空间重映射子集 |
| madvise | ✅ | 丢弃页 |
| 用户缓冲区访问 | ✅ | `copy_in`/`copy_out` 跨页检查 |

### 3.3 任务管理与调度

#### 任务生命周期

```
    new ──→ ready ──→ running
               │          │
               ↓          ↓
          sleeping / waiting
               │
               ↓
          exited / zombie
```

#### 任务核心资源

| 资源 | 说明 |
|------|------|
| Trap Frame | 保存用户态寄存器上下文 |
| Address Space | 用户地址空间（页表 + VMA） |
| FD Table | 文件描述符表（支持 dup/CLOEXEC/fork 继承） |
| cwd | 当前工作目录 |
| Cred | 用户/组 ID、capability |
| IPC Resources | 信号、futex、pipe 等 |
| Scheduler Context | 调度优先级、策略、时间片 |

#### 进程创建与资源继承

| 资源 | fork | exec | exit/wait |
|------|------|------|-----------|
| 地址空间 | 复制或重建上下文（COW） | 装载新 ELF | 释放用户映射 |
| fd 表 | 按继承规则复制 | 处理 CLOEXEC | 关闭并释放引用 |
| cwd/root | 继承路径上下文 | 继续沿用 | 随任务释放 |
| cred | 继承身份信息 | 保留权限上下文 | 随任务释放 |
| 子进程关系 | 挂入任务树 | 保持父子关系 | wait 回收退出码 |

#### 调度器设计

- 默认调度策略：`SCHED_OTHER` 轮转（Round-Robin）
- 时间片控制：`config::task::MAX_TICKS_PER_TASK`
- 支持多类调度框架：`impl-multi-class`（FIFO/RR/OTHER 队列骨架）
- 阻塞与等待：WaitQueue、睡眠、超时、条件等待
- 信号驱动：`SIGSTOP`/`SIGCONT` 进程级阻塞与恢复

### 3.4 文件系统与 VFS

#### 层次架构

```
syscall 层：open/read/write/stat/mount/dup/pipe2
     ↓
VFS 层：路径解析 │ mount table │ dentry/inode │ fd session
     ↓
FS 实现层：ext4 │ tmpfs │ procfs │ devfs
     ↓
Page Cache：普通文件缓存 / 脏页写回
     ↓
块设备层：virtio-blk / root image
```

#### VFS 对象关系

```
syscall
  │
  └── fd session (per-task fd table)
        │
        └── VfsIoHandle (file object)
              │
              ├── mount table (路径解析/根卷/挂载点)
              │     │
              │     ├── ext4_rs (根文件系统)
              │     ├── tmpfs
              │     ├── procfs (/proc 视图)
              │     └── devfs (/dev 设备节点)
              │
              ├── page cache (普通文件缓存/脏页写回)
              │     │
              │     └── block device (virtio-blk)
              │
              └── inode/dentry (元数据)
```

#### 已实现能力

| 子系统 | 能力 |
|--------|------|
| **ext4** | RO 路径（ext4-view）+ RW 路径（ext4plus beta）；自检包含读回校验 |
| **procfs** | `/proc/<pid>/stat`、`status`、`cmdline`、`/proc/meminfo`、`/proc/mounts` 等只读视图 |
| **devfs** | virtio-blk 设备自动注册为 `/dev/vblkN` |
| **rootfs** | 全局根卷管理，`probe` + `supports` 模式选择 FS 实现 |
| **VFS** | 路径规范化、挂载表、fd 会话（stdin/stdout/stderr）、cwd |
| **Page Cache** | 文件页缓存、写回机制、逻辑 size 覆盖 |

### 3.5 系统调用兼容层

#### 设计策略

采用 **Linux generic 64-bit syscall 编号和参数约定**，对所有系统调用实现语义转换：

```
用户态 ecall
  → syscall id 解码
  → 参数提取（6 寄存器参数）
  → 合法性检查（user pointer / fd / flags）
  → 调用内核子系统（task / mm / vfs / fs / ipc / net）
  → 返回值包装（数值或 -errno）
```

#### 已实现系统调用分类（100+）

| 类别 | 系统调用示例 | 数量 |
|------|-------------|------|
| **文件 I/O** | `read`/`write`/`pread64`/`pwrite64`/`readv`/`writev`/`openat`/`close`/`dup`/`dup3`/`pipe2`/`lseek`/`sendfile`/`fcntl`/`flock`/`ioctl` | ~20 |
| **路径/VFS** | `getcwd`/`chdir`/`mkdirat`/`unlinkat`/`renameat2`/`symlinkat`/`readlinkat`/`utimensat`/`getdents64`/`mount`/`umount2`/`stat`/`fstat`/`statx`/`statfs`/`truncate`/`fallocate` | ~25 |
| **内存管理** | `brk`/`mmap`/`munmap`/`mprotect`/`mremap`/`madvise`/`mlock`/`munlock`/`get_mempolicy` | ~12 |
| **进程/线程** | `clone`/`clone3`/`execve`/`exit`/`exit_group`/`waitpid`/`waitid`/`getpid`/`getppid`/`gettid`/`sched_*`/`setpgid`/`getpgid`/`unshare` | ~20 |
| **凭证** | `getuid`/`setuid`/`geteuid`/`getgid`/`setgid`/`getgroups`/`setgroups`/`capget`/`capset` | ~15 |
| **信号/同步** | `futex`/`rt_sigaction`/`rt_sigprocmask`/`kill`/`tkill`/`tgkill`/`nanosleep`/`clock_nanosleep` | ~10 |
| **多路复用** | `poll`/`ppoll`/`select`/`pselect6`/`epoll_create`/`epoll_ctl`/`epoll_wait` | ~8 |
| **网络** | `socket`/`bind`/`listen`/`accept4`/`connect`/`sendto`/`recvfrom`/`sendmsg`/`recvmsg`/`setsockopt`/`getsockopt`/`shutdown` | ~15 |
| **时间/杂项** | `clock_gettime`/`gettimeofday`/`syslog`/`sysinfo`/`uname`/`prctl`/`getrandom` | ~10 |

#### 未支持路径处理策略

- 未实现的 flag、非法 fd、越界用户指针和权限错误返回**明确 errno**
- 未知系统调用号：dummy impl 返回 `-ENOSYS`
- `impl-kernel` 对未实现槽位采用 **panic** 策略（bring-up 阶段快速暴露缺失）

### 3.6 IPC、网络与诊断

#### 进程间通信

| 机制 | 状态 | 说明 |
|------|------|------|
| **Pipe** | ✅ | 统一接入 fd 读写路径；阻塞/非阻塞、EOF、BrokenPipe |
| **Futex** | ✅ | 支持用户态等待与唤醒；`FUTEX_WAIT`/`FUTEX_WAKE`/`FUTEX_REQUEUE` |
| **Signal** | ✅ | 进程共享 disposition/pending + 线程私有 mask/pending；`rt_sigreturn` 完整路径 |
| **Shared Memory** | ✅ | SysV shm 子集；共享内存对象和映射生命周期管理 |
| **WaitQueue** | ✅ | 条件等待、超时唤醒 |
| **Eventfd** | ✅ | 事件通知机制 |

#### 网络栈

- **VirtIO-Net**：设备识别 + smoltcp 协议栈集成
- **Socket**：通过 fd 表向用户态暴露；支持 INET socket 全族 + `AF_UNIX`（pathname/abstract）
- **协议支持**：TCP/UDP 基础通信能力

#### 诊断能力

| 能力 | 说明 |
|------|------|
| **klog** | 内核消息环组件，支持 desc + 变长正文环；适配 Linux `syslog(2)` |
| **syslog** | 系统调用 `__NR_syslog`（116） |
| **自检链** | 各组件提供 `test()` 入口，贯穿启动流程 |
| **QEMU 日志** | int/cpu 日志输出，辅助追踪异常路径 |
| **PC 监视** | PC 变动监视脚本，仅当 PC 跳变时打印一行 |
| **符号解析** | 地址 → 内核符号查询 |

### 3.7 驱动与设备路径

```
syscall：read / write / ioctl / socket / mount
    ↓
对象层：console fd │ block device │ socket fd │ procfs view
    ↓
驱动抽象：char device │ block device │ net device │ irq handler
    ↓
VirtIO：virtio-blk │ virtio-net │ queue │ descriptor │ interrupt
    ↓
平台总线：RISC-V VirtIO-MMIO / LoongArch64 VirtIO-PCI
```

- **字符设备**：console、日志读写
- **块设备**：virtio-blk 承接根文件系统和 page cache
- **网络设备**：VirtIO-Net 接入 smoltcp 协议栈，socket 通过 fd 表暴露
- **平台层**：处理设备发现（DTB 扫描/硬编码槽位）、中断和总线差异

---

## 四、项目特色

### 4.1 双架构主路径完整

RISC-V64 与 LoongArch64 **都具备**完整的启动、页表、设备、文件系统和用户态运行路径，共享同一套上层内核逻辑，两条路径仅在平台/arch 层分岔。

### 4.2 组件边界清晰

平台、驱动、MM、task、FS/VFS、syscall、IPC 等 13 个一级组件通过统一的 **API/impl 范式**连接，依赖关系显式表达在 `Cargo.toml` 和 `feature-tree.txt` 中，新增/切换实现不需要改动上层代码。

### 4.3 用户态路径完整

从 ELF 装载、地址空间切换、syscall 分发、fd 操作、pipe 通信、procfs 查看到 socket 网络，已形成**连续的用户程序运行链路**。

### 4.4 Linux ABI 兼容方向明确

系统调用编号、参数约定、返回值语义、errno 和文件对象语义按 **Linux generic 64-bit ABI** 收敛，已实现 100+ 个系统调用，覆盖竞赛测例所需的核心接口。

### 4.5 可诊断性强

自检链贯穿启动流程，每个一级组件均提供统一的 `test()` 入口；日志、QEMU 输出、PC 监视和脚本入口覆盖从启动到用户程序运行的全过程。

### 4.6 Rust 语言优势利用

- **内存安全**：所有权、借用、生命周期在无 GC 环境下的零成本抽象
- **no_std**：完全控制硬件，无运行时开销
- **enum 错误处理**：系统调用返回值的类型安全编码
- **cfg feature 条件编译**：双架构代码的干净分离

---

## 五、测试与验证

### 5.1 竞赛测例通过情况

| 测例集 | 状态 | 说明 |
|--------|------|------|
| **BusyBox (glibc)** | ✅ 55/55 | 2026-06-10 全量通过 |
| **BusyBox (musl)** | ✅ 55/55 | 2026-06-10 全量通过 |
| **basic 测例** | 🔄 持续解锁 | 已启用 8 个核心测例（clone/fork/wait/waitpid/getpid/getppid/exit/execve） |
| **lua 脚本** | 🔄 待解锁 | 路径已接线，待取消注释 |
| **benchmark** | 🔄 待解锁 | 依赖 basic 和 lua 全通后验证 |

### 5.2 自检体系

每个一级组件提供 `test()` 统一入口，由聚合层串联 API 层测试、子组件测试和当前激活 impl 的测试，在 bring-up 阶段形成稳定的自检链：

```
os/src/main.rs
  ├── mm::test_with_range(...)      ← 内存子系统自检
  ├── fs::init() / fs::test()       ← 文件系统自检
  ├── vfs::test()                   ← VFS 自检（含 RW 读回校验）
  ├── task::test()                  ← 调度器自检
  ├── driver::test()                ← 驱动自检
  └── ipc::test()                   ← IPC 自检
```

### 5.3 锁机制审计

2026-06-25 完成全内核锁机制审计，已修复以下关键问题：

| 问题 | 状态 | 描述 |
|------|------|------|
| PC-01 页缓存驱逐重入死锁 | ✅ 已修复 | `GlobalFilePageCache` entry RwLock |
| RC-1 调度路径未释放中断 | ✅ 已修复 | wait/sleep 跨 `__switch` |
| SFA-1 帧分配无中断保护 | ✅ 已修复 | `StackFrameAllocator` |
| IPC-01 跨任务管道竞争 | ✅ 已修复 | `KernelPipe` 跨任务访问 |
| SHM-01 shmat TOCTOU | ✅ 已修复 | `ShmRegistry` |
| KLOG-01 可抢占下 spin 锁 | ✅ 已修复 | 内核日志环 |
| U-02/U-03 退出清理 | ✅ 已修复 | unix socket + pthread clone fd 表 |

---

## 六、总结与展望

### 6.1 当前成果

WaterOS 经过持续开发，已形成一个**功能完整、架构清晰、双架构可运行**的操作系统内核，在 Rust 操作系统生态中具备以下优势：

1. **组件化架构成熟**：13 个一级组件均采用统一的 API/impl 范式组织，新增架构或实现只需新增 impl crate 并在聚合层注册
2. **Linux ABI 兼容性良好**：100+ 系统调用的实现覆盖了竞赛测例所需的核心接口集
3. **双架构务实落地**：不仅是概念验证，RISC-V64 和 LoongArch64 都具备从引导到用户程序运行的全链路能力
4. **代码质量有保障**：完整的自检链、锁机制审计、统一的编码规范

### 6.2 后续完善方向

| 方向 | 优先级 | 说明 |
|------|--------|------|
| **basic 测例全解锁** | 高 | 将 20+ 注释态测例逐项取消注释并修复边界失败 |
| **LoongArch 验证覆盖** | 高 | 补齐验证覆盖，与 RISC-V 对齐赛题评测环境 |
| **线程与信号完善** | 中 | 实时信号队列、altstack、job control |
| **调度策略丰富** | 中 | FIFO/RR 真实抢占语义、多级反馈队列 |
| **ext4 写路径强化** | 中 | 目录树一致性、并发策略、事务语义 |
| **网络栈完善** | 中 | socket 生命周期、错误处理、并发场景 |
| **SMP 支持** | 低 | per-CPU run-queue、核间同步 |
| **文档同步** | 持续 | 按实际代码状态增量刷新架构文档 |

---

## 七、致谢

感谢大赛组委会提供展示和交流的平台，感谢指导老师的悉心指导，感谢团队成员的共同努力。

---

**WaterOS** — 用 Rust 构建的下一代操作系统内核  
山东大学（青岛）  
2025-2026
