# 系统调用语义审计：G29–G38（内存 / 时间 / 身份杂项）

> 审计范围：清单分组 G29–G38  
> Baseline：Linux syscall 语义（`LinuxGeneric64` 号表，RISC-V 与 LoongArch 共用）  
> 生成时间：2026-06-25

---

## 1. 概述

| 分组 | 范围 | 主要实现文件 | 分发入口 |
|------|------|-------------|----------|
| G29 | `brk` | `sys/brk.rs` | `dispatch_brk` (214) |
| G30 | `mmap`/`munmap`/`mprotect`/`mremap`/`madvise`/`msync` | `sys/mmap.rs` | `dispatch_mmap` 等 (222/215/226/216/233/227) |
| G31 | `mlock`/`munlock`/`mlockall`/`munlockall` | `sys/mmap.rs` | `dispatch_mlock*` (228–231) |
| G32 | `get_mempolicy`/`shmget`/`shmctl`/`shmat`/`shmdt` | `sys/mempolicy.rs`, `sys/shm.rs` | `dispatch_getmempolicy` (236), `dispatch_shm*` (194–197) |
| G33 | `gettimeofday` | `sys/clock.rs` | `dispatch_get_time` (169) |
| G34 | `clock_*`/`nanosleep`/`times`/`setitimer`/`getitimer`/`adjtimex`(171)/`clock_adjtime`(266) | `sys/clock.rs`, `sys/task.rs` | 号表槽 + `dispatch_unknown` 旁路 |
| G35 | cred 全家桶 | `sys/cred.rs` | `dispatch_getuid` 等 (174–177, 144–149, 158) |
| G36 | `capget`/`capset` | `sys/cap.rs` | `dispatch_capget`/`capset` (90/91) |
| G37 | `sysinfo`/`uname`/`prctl`/`getrlimit`/`setrlimit`/`prlimit64`/`umask`/`getrusage`/`getrandom` | `sys/task.rs` | 号表槽 (179/160/167/163–164/261/166/165/278) |
| G38 | `syslog`/`acct`(89) | `sys/syslog.rs`, `sys/acct.rs` | `dispatch_syslog` (116) + `dispatch_unknown` 旁路 |

**下游依赖**：`wateros-mm`（`HeapBrk`/`MmapOps`/`mempolicy`）、`wateros-ipc::shm`、`wateros-cred`、`wateros-klog`、`wateros-task`、`platform::timer`/`wall_clock`。

---

## 2. LoongArch vs RISC-V 差异摘要

两架构共用 **`abi-impl/impl-linux-generic64`** 号表，syscall 编号一致。差异在 MM 后端与用户地址空间是否建立：

| 维度 | RISC-V (`qemu-riscv64-opensbi`) | LoongArch (`qemu-loongarch64-virt`) |
|------|--------------------------------|-------------------------------------|
| MM feature | `mm/impl-sv39` → `Sv39AddressSpace` | `mm/impl-loongarch64` → `LoongArch64AddressSpace` |
| `user_aspace_ptr` | `execve`/ELF 装载后非 0 | 同上（`kernel_elf` 分配 `LoongArch64AddressSpace`） |
| `brk` 真实路径 | `HeapBrk` on Sv39 | `HeapBrk` on LoongArch64（逻辑对称） |
| `brk` 假顶桩 | `user_aspace_ptr==0` 时 `USER_BRK_FAKE` | 同左 |
| `mmap` 族 | 需 `user_aspace_ptr`；**无则 `syscall_unsupported` → panic** | **同左**（非文档所述 `ENOSYS`） |
| `uname` machine 字段 | `"riscv64"` | `"loongarch64"` |
| `user_copy` 调试 | RISC-V 含 `debug_probe_user_virt` | LoongArch 仅 trace satp |
| 号表旁路 syscall | `adjtimex`(171)、`clock_adjtime`(266)、`acct`(89) 经 `dispatch_unknown` | 同左 |

**结论**：内存 syscall 的**架构分叉不在 syscall 层**，而在任务是否经 ELF 装载获得 `user_aspace_ptr`。导出文档写「LoongArch 返回 `ENOSYS`」与**当前代码不符**（实际为内核 panic）；两架构行为一致。

---

## 3. 分组审计

### G29 — `brk` (214)

**Linux 语义**：查询/调整 program break；失败时返回当前 break 并置 `errno=ENOMEM`；`addr==0` 为查询。

**实现**（`sys/brk.rs`）：

| 路径 | 状态 | 说明 |
|------|------|------|
| `user_aspace_ptr != 0` | 部分实现 | 委托 `mm::HeapBrk::brk`；饥渴扩页/缩页 |
| `user_aspace_ptr == 0` | 桩 | `USER_BRK_FAKE` 单调递增假顶（初值 `0x0120_0000`） |
| `brk(0)` 查询 | 已实现 | 两路径均支持 |
| 扩页失败 | **语义偏差** | `HeapBrk::brk` 失败时返回**当前 break 作为成功值**，未设 `ENOMEM` |
| 缩页 `addr < current` | 部分 | 真实路径 MM 支持缩页；假桩路径**拒绝缩页**（返回当前假顶） |
| `with_user_aspace_mut` 失败 | 已实现 | 返回 `ENOMEM` |

**可靠性**：假桩路径下 `brk` 扩顶不分配物理页，用户写越界可能 page fault 或破坏其他映射——仅适用于无页表 bring-up。

**收敛建议**：`HeapBrk::brk` 失败改为 `UserRet::from_error(ENOMEM)` 且返回值仍为当前 break（Linux 惯例）；假桩路径对 `addr!=0` 打 warn 并返回 `ENOSYS` 或保持桩但文档化。

---

### G30 — `mmap` 族 (222/215/226/216/233/227)

**Linux 语义**：虚拟内存映射/保护/重映射/建议/同步。

#### `mmap` (222)

| 能力 | 状态 |
|------|------|
| 匿名 `MAP_PRIVATE\|ANONYMOUS` | 已实现（饥渴映射） |
| 文件 `MAP_SHARED`/`MAP_PRIVATE` | 部分：SHARED  eager 读入；PRIVATE  lazy loader |
| `MAP_FIXED`、页对齐 offset | 已实现 |
| `MAP_GROWSDOWN`、`MAP_HUGETLB` 等 | 未识别 → MM `EINVAL` |
| 无 `user_aspace_ptr` | **P0：panic**（`syscall_unsupported`） |

#### `munmap` / `mprotect` / `mremap`

- 同需 `user_aspace_ptr`；无则 **panic**。
- `mremap`：MM 子集（匿名私有 grow/shrink、`MREMAP_MAYMOVE`/`MREMAP_FIXED`）；文件映射/共享 `mremap` 受限。
- `MmError::Unsupported` 经 `mm_err_to_errno` → **panic**。

#### `madvise` (233)

- 校验页对齐与 advice 枚举；**全部已知 advice 无操作成功**（Linux 允许部分为 hint）。

#### `msync` (227)

- 校验 `addr` 页对齐与 flags 组合；**不访问映射、不写回文件，恒成功**。

**收敛建议**：

1. 无 `user_aspace_ptr`：`warn` + 返回 `-ENOSYS`（替换 `syscall_unsupported`）。
2. `msync` 对文件 `MAP_SHARED`：暂返回 `-ENOSYS` 或 warn + `-EINVAL`（flags 含 `MS_SYNC` 时）。
3. `MmError::Unsupported`：映射为 `-EINVAL`/`-ENOSYS`，禁止 panic。

---

### G31 — `mlock*` (228–231)

| 调用 | 行为 |
|------|------|
| `mlock`/`munlock` | 仅校验页对齐与 `len>0`；**不锁页**，恒成功 |
| `mlockall` | 校验 flags 非 0 且为已知位；**不锁**，恒成功 |
| `munlockall` | 无操作成功 |

**语义**：对 LTP 探测可接受；与 Linux「锁页防换出」不符。建议 warn 一次或文档标注 stub。

---

### G32 — `get_mempolicy` + SysV SHM

#### `get_mempolicy` (236)

- 单节点 bring-up：`MPOL_DEFAULT` + nodemask 节点 0。
- `MPOL_F_ADDR`：校验地址已映射（需 `user_aspace_ptr`）；无 aspace 时 `EFAULT`。
- 未实现 `set_mempolicy`、多节点 NUMA。

#### `shmget` (194) / `shmctl` (195) / `shmat` (196) / `shmdt` (197)

| 调用 | 状态 |
|------|------|
| `shmget` | 已实现：`IPC_PRIVATE`/`IPC_CREAT`/`IPC_EXCL`；段上限 4MiB；**已存在 key 不校验 size**（与 Linux `IPC_CREAT` 无 `EXCL` 时 size 校验有偏差） |
| `shmctl` | **仅 `IPC_RMID`**；其余 cmd → `ENOSYS` |
| `shmat` | 已实现：经 `MmapOps` 预留 VA + 替换为共享物理页；fork 继承（`fork_task_attachments`） |
| `shmdt` | 已实现：detach + unmap |
| 无 `user_aspace_ptr` | `shmat`/`shmdt` → `EFAULT`（非 panic） |

**可靠性**：`ShmRegistry` 全局 `spin::Mutex`；`shmat` 失败时回滚 unmap，路径完整。

---

### G33–G34 — 时间类

#### `gettimeofday` (169)

- 读 `CLOCK_REALTIME`（`realtime_ns()`）；`tv==NULL` 合法成功。
- 微秒由纳秒截断，无 `timezone` 参数（Linux 已废弃，可接受）。

#### `clock_gettime` (113) / `clock_settime` (112) / `clock_getres` (114) / `clock_nanosleep` (115)

| clock_id | gettime | settime | getres | nanosleep |
|----------|---------|---------|--------|-----------|
| `REALTIME` / `REALTIME_COARSE` | ✓ | settime 仅 REALTIME | ✓ | ✓ |
| `MONOTONIC` / `RAW` / `COARSE` | ✓ | `EPERM` | ✓ | ✓ |
| `PROCESS_CPUTIME_ID` | tick×周期 | `EPERM` | ✓ | **不支持**（`EINVAL`） |
| 其他 id | `EINVAL` | `EPERM`（非 REALTIME） | `EINVAL` | `EINVAL` |

**问题**：

- `clock_settime(REALTIME)`：**无 `CAP_SYS_TIME`/root 检查**，任意进程可改墙钟。
- 睡眠精度：基于 `SCHED_TIMER_PERIOD_MS`（约 10ms）tick 量化；`nanosleep`/`clock_nanosleep` 可被信号 `EINTR` 打断并写 `rem`。
- `clock_nanosleep` 绝对模式成功后写 `rem=0`（符合 Linux）。

#### `nanosleep` (101) / `times` (153)

- `nanosleep`：单调时钟 tick 睡眠；`req=={0,0}` 立即返回。
- `times`：`utime`=当前任务 tick 数；`stime/cutime/cstime=0`；返回值=系统 tick（非 Linux 的 clock ticks 语义细节偏差）。

#### `setitimer` (103) / `getitimer` (102)

- 委托 `ipc::signal` 定时器表；`ITIMER_REAL`/`VIRTUAL`/`PROF` 由 `valid_itimer` 约束。
- `setitimer` 在设置前若 `old_value!=NULL` 先写 default 再覆盖（短暂不一致窗口，低优先级）。

#### `adjtimex` (171) / `clock_adjtime` (266)

- **旁路**：不在主号表槽位，由 `dispatch_unknown` 按裸号分发。
- 读写 `TIMEX_STATE`；写模式需 `euid==0`。
- **`ADJ_SETOFFSET` 等不改变 `realtime_ns()`**；仅存储 offset/freq 元数据，与 Linux NTP 语义差距大。
- `clock_adjtime`：`clock_id` 须为 `REALTIME`，否则 `EINVAL`。

**收敛建议**：`clock_settime` 非 root → `EPERM`；`adjtimex` 写路径若 modes 含未实现位 → warn + 忽略或 `EINVAL`。

---

### G35 — cred 全家桶

| 调用 | 状态 |
|------|------|
| `getuid`/`geteuid`/`getgid`/`getegid` | 已实现 → `wateros-cred` |
| `getgroups` | 部分：固定 1 个 supplementary gid `[0]`；**负 size/null 指针/copy 失败 → panic** |
| `setuid`/`setgid`/`setreuid`/`setregid`/`setresuid`/`setresgid` | 已实现：**impl-root 无权限检查**，任意任务可改 ID |
| `-1` 参数 | `usize::MAX` / `u32::MAX` 解析为 `None`（保持） |

**语义**：bring-up 全 root + privileged set*id；**无 Linux capability/权限模型**。`execve` 不应用 `S_ISUID`（`on_exec` TODO）。

**收敛建议**：非 root `setuid(0)` → `EPERM`；`getgroups` 错误路径返回 `EINVAL`/`EFAULT`，禁止 panic；`size < ngroups` → `EINVAL`。

---

### G36 — `capget` / `capset` (90/91)

- `capget`：版本 1/2/3；**恒返回 `CAP_CHOWN|CAP_SETPCAP`**（与当前 euid 无关）。
- `capset`：校验 permitted 关系后**成功但不持久化**。
- `prctl(PR_CAPBSET_READ/DROP)` 委托 `cap_bset_*`（读恒 1，drop 无操作）。

**语义**：仅供 LTP 探测；与真实 capability 子系统不符。

---

### G37 — 身份 / 资源杂项

| 调用 | 状态 | 备注 |
|------|------|------|
| `sysinfo` (179) | 部分 | 固定 `totalram`/`freeram`（`QEMU_VIRT_PHYS_RAM_SIZE` 及一半）；`uptime`=tick；`procs=1` |
| `uname` (160) | 部分 | 固定 `sysname/release/version`；**machine 按架构编译期分支** |
| `prctl` (167) | 桩 | `PR_SET_NAME` 无操作；`PR_GET_NAME` 空串；`PR_SET_NO_NEW_PRIVS` 无操作；其余 `ENOSYS` |
| `getrlimit`/`setrlimit` (163/164) | 部分 | 每进程覆盖表 + 默认值；未强制 mmap/brk 限额 |
| `prlimit64` (261) | 部分 | **仅 `pid==0`（当前进程）**；其他 pid → `ESRCH` |
| `umask` (166) | 已实现 | 全局 `CURRENT_UMASK`，初值 `022` |
| `getrusage` (165) | 部分 | `RUSAGE_SELF`/`THREAD` 用 tick 伪造；`RUSAGE_CHILDREN` 全零 |
| `getrandom` (278) | 桩 | **xorshift 伪随机**（非密码学安全）；允许 `GRND_*` flags |

---

### G38 — `syslog` / `acct`

#### `syslog` (116)

- 写优先 action：`copy_from_user` → `klog::syscall::dispatch_kernel`。
- 读 action：内核缓冲 → `copy_to_user`。
- **P0**：`len>0` 且 `buf==NULL`（写或读）→ **`panic!`**。

#### `acct` (89)

- 需 root；`path==NULL` 关闭 accounting。
- 非 NULL：路径解析 + 元数据校验（须为可写普通文件），保存 accounting 输出路径。
- 进程退出路径写入 Linux `struct acct` v0 兼容记录：`ac_comm`、uid/gid、btime、exit status 等；CPU/IO/内存统计仍为最小值。
- **缺口**：未支持 `CONFIG_BSD_PROCESS_ACCT_V3` / `struct acct_v3`，也未实现完整资源统计与磁盘空间阈值行为。

---

## 4. 潜在问题清单（按严重度）

### P0 — 可导致 panic / 卡死 / 严重静默错误

| ID | 分组 | 问题 | 位置 | 建议 |
|----|------|------|------|------|
| P0-1 | G30 | 无 `user_aspace_ptr` 时 `mmap`/`munmap`/`mprotect`/`mremap` **panic** | `mmap.rs` `syscall_unsupported` | warn + `-ENOSYS` |
| P0-2 | G30 | `MmError::Unsupported` → panic | `mm_util.rs` | 映射 `-EINVAL`/`-ENOSYS` |
| P0-3 | G35 | `getgroups` 异常参数 / copy 失败 → panic | `cred.rs` | 返回 `-EINVAL`/`-EFAULT` |
| P0-4 | G38 | `syslog` 空指针 + 非零 len → panic | `syslog.rs` | 返回 `-EFAULT` |
| P0-5 | G29 | `brk` 扩页失败返回成功且无 `ENOMEM` | `brk.rs` L20–23 | 失败设 `errno=ENOMEM` |

### P1 — 语义错误 / 错误码不符 / 误导性成功

| ID | 分组 | 问题 | 建议 |
|----|------|------|------|
| P1-1 | G30 | `msync` 不刷盘/不写回 | warn + 对 SHARED 文件映射返回 `-ENOSYS` |
| P1-2 | G30 | `madvise`/`mlock*` 全 no-op 成功 | 文档化；可选 warn |
| P1-3 | G34 | `clock_settime` 无 root 检查 | 非 root → `EPERM` |
| P1-4 | G34 | `adjtimex` 不改墙钟 | 文档化；或实现 `ADJ_SETOFFSET` |
| P1-5 | G34 | 睡眠/时钟分辨率 ~10ms tick | 文档化；LTP 超时需容忍 |
| P1-6 | G35 | set*id 无权限模型 | 引入 `EPERM` 规则 |
| P1-7 | G36 | `capget`/`capset` 与真实 cred 脱节 | 文档化 stub |
| P1-8 | G32 | `shmctl` 除 `IPC_RMID` 外 `ENOSYS` | 明确拒绝并 warn |
| P1-9 | G37 | `getrandom` 非密码学随机 | 文档化；生产需真 RNG |
| P1-10 | G37 | `getgroups` 未检查 `size < ngroups` | 返回 `-EINVAL` |
| P1-11 | G38 | `acct` 仅校验路径 | 文档化 no-op |
| P1-12 | G29 | 假 `brk` 桩可“扩堆”无物理页 | 限制于无 aspace 任务 |
| P1-13 | — | 导出文档写 mmap LoongArch `ENOSYS` 与代码不符 | 同步 `wateros-syscall.md` |

---

## 5. 覆盖范围总表

| Syscall | Nr | 实现 | Linux 对齐度 |
|---------|-----|------|-------------|
| brk | 214 | 部分 | 中（失败 errno；假桩） |
| mmap | 222 | 部分 | 中（匿名+文件子集；无 aspace panic） |
| munmap | 215 | 部分 | 中高 |
| mprotect | 226 | 部分 | 中高 |
| mremap | 216 | 部分 | 低中（子集） |
| madvise | 233 | 桩 | 低（全 no-op） |
| msync | 227 | 桩 | 低（全 no-op） |
| mlock/unlock/all | 228–231 | 桩 | 低 |
| get_mempolicy | 236 | 部分 | 低（单节点） |
| shmget/ctl/at/dt | 194–197 | 部分 | 中（ctl 极简） |
| gettimeofday | 169 | 部分 | 中高 |
| clock_get/set/getres/nanosleep | 112–115 | 部分 | 中 |
| nanosleep | 101 | 部分 | 中 |
| times | 153 | 部分 | 低中 |
| set/getitimer | 103/102 | 部分 | 中 |
| adjtimex | 171 | 部分 | 低（旁路；无墙钟） |
| clock_adjtime | 266 | 部分 | 低 |
| get/set*id, getgroups | 144–149, 158, 174–177 | 部分 | 低（无权限） |
| capget/set | 90/91 | 桩 | 低 |
| sysinfo/uname/prctl | 179/160/167 | 部分/桩 | 低中 |
| get/setrlimit, prlimit64 | 163–164/261 | 部分 | 中 |
| umask/getrusage/getrandom | 166/165/278 | 部分/桩 | 中/低 |
| syslog | 116 | 部分 | 中（空指针 panic） |
| acct | 89 | 桩 | 低（旁路） |

---

## 6. 高优先级收敛列表（建议主 agent 统一风格）

```text
warn 格式建议：
[syscall] <name>(nr=<n>) reject: <reason> args=[a0=.., a1=.., ...]
```

| 优先级 | 调用 | 条件 | 返回 |
|--------|------|------|------|
| P0 | mmap/munmap/mprotect/mremap | `user_aspace_ptr==0` | `-ENOSYS` |
| P0 | getgroups | `size<0` / null list / copy 失败 | `-EINVAL`/`-EFAULT` |
| P0 | syslog | null buf && len>0 | `-EFAULT` |
| P0 | brk (mm) | `HeapBrk::brk` Err | 当前 break + `-ENOMEM` |
| P1 | clock_settime | `euid!=0` | `-EPERM` |
| P1 | msync | 任意（当前无实现） | 保持 0 或改 `-ENOSYS` + warn |
| P1 | mm 路径 | `MmError::Unsupported` | `-ENOSYS`（非 panic） |

---

## 7. 参考代码锚点

```51:56:os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/brk.rs
pub(crate) fn sys_brk(addr: usize) -> UserRet {
    if let Some(handle) = current_user_aspace_handle() {
        return sys_brk_mm(handle, addr);
    }
    sys_brk_fake(addr)
}
```

```59:62:os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mmap.rs
pub(crate) fn sys_mmap(args : SyscallArgs) -> UserRet {
    let Some(handle) = current_user_aspace_handle() else {
        syscall_unsupported("mmap: no user_aspace_ptr");
    };
```

```699:707:os/components/wateros-syscall/syscall-impl/impl-kernel/src/lib.rs
        if syscall_nr == SYS_ADJTIMEX {
            return sys::sys_adjtimex(args).0;
        }
        if syscall_nr == SYS_CLOCK_ADJTIME {
            return sys::sys_clock_adjtime(args).0;
        }
        if syscall_nr == SYS_ACCT {
            return sys::sys_acct(args).0;
```
