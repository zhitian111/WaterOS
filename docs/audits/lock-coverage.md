# 带锁数据结构支持范围说明

> 汇总时间：2026-06-25  
> Baseline：**单核多线程**（UP + 定时器抢占）；多核相关实现单独标注，baseline 下不判错  
> 问题详情：`docs/audits/lock-issues.md`  
> 单结构深度分析：`docs/audits/locks/<name>.md`

---

## 1. 图例

| 标记 | 含义 |
|------|------|
| ✅ | 当前 baseline 下路径已正确加锁/释锁，语义与单核假设一致 |
| ⚠️ | 功能可用，但存在抢占、长临界区或 TOCTOU，压测/并发下不可靠 |
| ❌ | 未覆盖、已知错误或未实现 |
| 🔒 | 仅 bring-up / 单线程上下文安全 |
| 🚫 | 多核（SMP）未支持 |

---

## 2. 同步原语层

| 原语 | 文件 | Baseline 语义 | SMP |
|------|------|--------------|-----|
| `UniprocessorSafeCell<T>` | `wateros-base/src/sync/uniprocessor.rs` | 关中断或不可抢占区间内 `exclusive_access` 独占 | 🚫 |
| `spin::Mutex` | 各组件 `use spin::Mutex` | 临界区应短；持锁期间不应被抢占（否则活锁） | 🚫 |
| `spin::RwLock` | `impl-page-cache` per-file | 不可重入；持写锁期间不得再取读/写 | 🚫 |
| `InterruptSafeLockedHeap` | `runtime-heap-allocator` | alloc/dealloc 关中断 + 内层 Mutex | 🚫 |
| `AtomicI64`（`REALTIME_OFFSET_NS`） | `platform/wall_clock.rs` | Relaxed，非锁；与 `TIMEX_STATE` 一致性见 syscall 审计 | ⚠️ |

---

## 3. 按子系统覆盖矩阵

### 3.1 wateros-task

| 结构 | 锁类型 | 已覆盖路径 | 未覆盖 / 不可靠路径 | 文档 |
|------|--------|-----------|-------------------|------|
| `RoundRobinScheduler` / `MultiClassScheduler` | UniprocessorSafeCell | 常规 API：InterruptGuard + `with_scheduler`；`__switch` 前释锁 | wait/sleep 跨 switch 关中断（RC-1）；`run_first_task` 无 guard；RR 策略变更不迁队列 | `locks/scheduler.md` |
| `TaskRegistry` + `WaitQueues` | （调度器内，无独立锁） | 阻塞/唤醒/超时队列在调度器临界区内 | 与 ProcessRegistry 复合操作非原子 | 同上 |
| `ProcessRegistry` | UniprocessorSafeCell | 单次 lookup/rlimit/mark/reap | Spawn/Fork/Clone 登记窗口；Kill/Reap 与 Scheduler 非原子；reap 持锁 MM 释放 | `locks/process-registry.md` |

### 3.2 wateros-vfs

| 结构 | 锁类型 | 已覆盖路径 | 未覆盖 / 不可靠路径 | 文档 |
|------|--------|-----------|-------------------|------|
| `PerTaskFdRegistry` | UniprocessorSafeCell | 单线程进程 open/read/write/close/dup | `CLONE_FILES` + `with_current_io` 竞态；持借 close/duplicate/flush；pipe2 回滚持借 | `locks/per-task-registries.md` |
| `PerTaskCwdRegistry` | UniprocessorSafeCell | getcwd/chdir 短临界区 | chdir FS 校验与写入非原子（语义） | 同上 |
| `AUX_MOUNTS` / `DEVICE_IDS` | spin::Mutex ×2 | 单次 mount/umount/resolve 短临界区 | 并发 mount 同点 TOCTOU；与 Per-FS 锁间接长自旋 | `locks/mount-rootfs.md` |
| `ROOT_FS` 等 | spin::Mutex ×4 | bring-up 顺序 mount | 多字段非原子更新；clear 中间态 | 同上 |
| `GlobalFilePageCache` | Mutex ×3 + RwLock | 未饱和时 read/write 短路径 | **缓存饱和驱逐自死锁**；close/flush；mount_gen rebuild | `locks/page-cache.md` |
| `SharedFs` / `SharedRwFs` | Arc\<Mutex\> | 根卷顺序读写/metadata | aux RO 双实例；truncate 错路由；flush 长持锁 | `locks/shared-fs-handles.md` |

### 3.3 wateros-mm + wateros-runtime

| 结构 | 锁类型 | 已覆盖路径 | 未覆盖 / 不可靠路径 | 文档 |
|------|--------|-----------|-------------------|------|
| `StackFrameAllocator` | UniprocessorSafeCell | bring-up 自检顺序调用 | **运行期抢占下 RefCell panic**；运行期重复 init | `locks/mm-allocators.md` |
| `InterruptSafeLockedHeap` | Mutex + 中断屏蔽 | 常规 alloc/dealloc 闭环 | 嵌套 alloc 自旋；中断状态读失败静默 | 同上 |

### 3.4 wateros-ipc

| 结构 | 锁类型 | 已覆盖路径 | 未覆盖 / 不可靠路径 | 文档 |
|------|--------|-----------|-------------------|------|
| `FutexHub` | spin::Mutex | wait/wake/requeue 释锁闭环 | wait 继承 RC-1；wake 持锁跨调度 | `locks/ipc-futex-signal-shm.md` |
| `SignalRegistry` | spin::Mutex | rt_sig*、kill、itimer 基本路径 | sigsuspend/timedwait RC-1；kill vs send 语义差 | 同上 |
| `ShmRegistry` | spin::Mutex | shmget/shmdt/exit 清理 | **shmat TOCTOU**；fork 映射失败未回滚 | 同上 |
| `KernelPipe` | UniprocessorSafeCell | 单任务单 fd 读写 | **多任务共享 pipe RefCell panic**；wake_one 饿死 | `locks/ipc-pipe.md` |

### 3.5 wateros-fs

| 结构 | 锁类型 | 已覆盖路径 | 未覆盖 / 不可靠路径 | 文档 |
|------|--------|-----------|-------------------|------|
| `DEVFS` | spin::Mutex | 静态节点 lookup | refresh 长临界区；clear 空窗 | `locks/fs-aux.md` |
| `DEV_NODES` | spin::Mutex | 注册/列举 | 与 DEVFS 叠加争用 | 同上 |
| ProcfsLookups ×3 | spin::Mutex | 回调注册（init） | **持锁执行 cwd/VFS 回调** | 同上 |
| `EXT4_SMALL_READ_CACHE` | spin::Mutex | 单任务顺序小读 | 全局争用；与块锁顺序交叉 | 同上 |

### 3.6 wateros-driver

| 结构 | 锁类型 | 已覆盖路径 | 未覆盖 / 不可靠路径 | 文档 |
|------|--------|-----------|-------------------|------|
| `BLOCK_DEVICES` | spin::Mutex | 注册/枚举/clone Arc 后释表锁 | 与 ext4 cache 交叉锁序 | `locks/driver-block-char.md` |
| `CachingBlockDevice` 包装 | Arc\<Mutex\> | 单线程顺序 I/O | 整次 VirtIO 长持锁 | 同上 |
| `CHARACTER_DEVICES` | spin::Mutex | 注册/按索引访问 | 写路径 UART 自旋持锁 | 同上 |
| `NETWORK_DEVICES` | spin::Mutex | 注册/clone | 与 NETWORK_STACK 顺序使用 | `locks/driver-network.md` |
| `NETWORK_STACK` | spin::Mutex | 单任务顺序 socket | **全局锁覆盖整轮 poll** | 同上 |
| `SocketHandle.inner` | Arc\<Mutex\> | get/set 快照 | poll 路径多次加锁 | 同上 |
| 平台 probe 静态量 | spin::Mutex | 🔒 bring-up 单线程 probe | 调度后重入 probe；重复 test 双注册 | `locks/platform-probe.md` |
| `UART_GLOBAL` | spin::Mutex | 🔒 初始化 | 持锁阻塞读 | 同上 |

### 3.7 wateros-syscall 全局量

| 结构 | 锁类型 | 已覆盖路径 | 未覆盖 / 不可靠路径 | 文档 |
|------|--------|-----------|-------------------|------|
| `SOCKET_FD_REGISTRY` | spin::Mutex | inet socket fd 映射 | 与 unix 表独立，无嵌套死锁 | `locks/syscall-globals.md` |
| `FD_TABLE` / `BOUND` / `UnixSockInner` | spin::Mutex ×3 | 单进程 AF_UNIX 基本操作 | **bind 嵌套 VFS**；退出未清理；**pthread 不同步 FD_TABLE** | 同上 |
| `TIMES` | spin::Mutex | utimens 更新/查询 | 短临界区，低风险 | 同上 |
| `TIMEX_STATE` | spin::Mutex | adjtimex 基本字段 | ADJ_OFFSET 与原子时钟未同步 | 同上 |

### 3.8 wateros-cred + wateros-klog

| 结构 | 锁类型 | 已覆盖路径 | 未覆盖 / 不可靠路径 | 文档 |
|------|--------|-----------|-------------------|------|
| `PerTaskCredRegistry` | UniprocessorSafeCell | fork/exec 生命周期短操作 | 缺条目 panic；AccessCheck 未生效 | `locks/per-task-registries.md` |
| `KLOG` | spin::Mutex | bring-up 单线程 record | **run_first_task 后 syslog 可抢占自旋**；IRQ 重入 | `locks/klog.md` |

---

## 4. 按使用场景的支持度

| 场景 | 总体评估 | 主要风险 |
|------|----------|----------|
| bring-up 单线程自检 | ✅ | 引导路径少数无 InterruptGuard |
| 单进程 busybox 基本 syscall | ⚠️ | 页缓存饱和、长 FS 持锁、klog 争用 |
| 多线程 pthread（共享 fd） | ❌ | fd take_io 竞态；unix FD_TABLE 不同步；pipe RefCell |
| futex/poll 阻塞等待 | ❌ | RC-1 中断未释放 |
| 网络 socket 并发 | ❌ | NETWORK_STACK 全局锁 |
| ext4 多文件并发 RW | ❌ | 页缓存 P0；cache 锁序；块设备长持锁 |
| mount/umount 并发 | ⚠️ | TOCTOU；mount_gen 与页缓存 |
| AF_UNIX pathname | ⚠️ | bind 嵌套 VFS；退出泄漏 |
| SYSV shm | ⚠️ | shmat TOCTOU |
| procfs 热读 | ⚠️ | 持锁回调 |
| SMP / 多 hart | 🚫 | 全部 UniprocessorSafeCell / UP 假设 |

---

## 5. 锁顺序约定（当前实践）

无全局文档化锁层级；审计观察到的**常见无环顺序**：

```
InterruptGuard（关中断）
  → Scheduler UniprocessorSafeCell
  → ProcessRegistry UniprocessorSafeCell
  → PerTaskFd / Cwd / Cred UniprocessorSafeCell
  → AUX_MOUNTS / ROOT_FS / DEVICE_IDS（全局 Mutex，互不嵌套）
  → SharedFs Mutex（per 实例）
  → GlobalFilePageCache.files Mutex → entry RwLock → state Mutex
  → BLOCK_DEVICES（clone Arc 后释放）→ per-device Mutex
  → FutexHub / SignalRegistry / ShmRegistry（互不嵌套）
```

**已知违规/风险边**：

| 边 | 风险 |
|----|------|
| unix `bind`：BOUND+Inner → VFS → SharedFs | AB-BA 与 FS |
| ext4 write：EXT4_CACHE → BlockDevice；read 反向 | AB-BA |
| procfs：LOOKUP → cwd registry | 重入 procfs 死锁 |
| fd `check_nofile`：fd → ProcessRegistry | RefCell 嵌套 panic 窗口 |
| FutexHub wake：FutexHub → Scheduler wait queue | 长持 futex 锁 |

---

## 6. 与 Linux/预期语义对照（baseline 视角）

本表描述「单核多线程内核应保证的最低锁语义」，**非** Linux 完整语义：

| 预期 | 当前实现 |
|------|----------|
| 持锁不睡眠（mutex 语义） | spin 锁 + RefCell：持锁 wait 实际违反（靠关中断部分缓解） |
| 同线程可重入同一把锁 | ❌ spin Mutex / RwLock 均不可重入 |
| 阻塞 syscall 不阻止 timer 投递 | ❌ RC-1 |
| fork 后子进程资源一致 | ⚠️ fd/cwd/cred 基本覆盖；unix FD_TABLE 线程 clone 未覆盖 |
| 进程退出释放所有内核侧车资源 | ⚠️ unix BOUND 未清理 |
| 文件页缓存 close 不卡死 | ❌ PC-01 |
| 并发 pipe 读写 | ❌ IPC-01 |

---

## 7. 单结构文档索引

| Subagent 分组 | 文件 |
|--------------|------|
| scheduler | `locks/scheduler.md` |
| process-registry | `locks/process-registry.md` |
| per-task-registries | `locks/per-task-registries.md` |
| mm-allocators | `locks/mm-allocators.md` |
| mount-rootfs | `locks/mount-rootfs.md` |
| page-cache | `locks/page-cache.md` |
| shared-fs-handles | `locks/shared-fs-handles.md` |
| ipc-futex-signal-shm | `locks/ipc-futex-signal-shm.md` |
| ipc-pipe | `locks/ipc-pipe.md` |
| fs-aux | `locks/fs-aux.md` |
| driver-block-char | `locks/driver-block-char.md` |
| driver-network | `locks/driver-network.md` |
| platform-probe | `locks/platform-probe.md` |
| syscall-globals | `locks/syscall-globals.md` |
| klog | `locks/klog.md` |

---

## 8. 后续维护

- 修复或收敛某路径后：更新本文档对应行 + `lock-issues.md` 状态列
- 新增带锁结构：先入 `lock-inventory.md`，再补单结构文档与本文档矩阵
- 高优先级修复跟踪：`docs/roadmap/todolist.md`「锁机制审计」小节
