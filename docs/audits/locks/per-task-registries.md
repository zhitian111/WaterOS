# Per-Task 注册表锁机制审计

> 审计范围：`PerTaskFdRegistry`（#5）、`PerTaskCwdRegistry`（#6）、`PerTaskCredRegistry`（#7）  
> Baseline：单核多线程（UP + 定时器抢占）；`UniprocessorSafeCell` = `RefCell` 运行时独占借用（违反则 panic）  
> 生成时间：2026-06-25（subagent 复核）

---

## 0. P0 / P1 / 已修复摘要

| 级别 | ID | 结构 | 问题 | 状态 |
|------|-----|------|------|------|
| **P0** | **FD-01** / R-PT-01 | `PerTaskFdRegistry` | `with_current_io` take-restore 使 fd 槽在 I/O 窗口为空；`CLONE_FILES` 共享表（`ref_counts > 1`）下 sibling 线程可 dup/close/alloc 同一槽 → restore 失败、`BadFd`、双 close | **已修复** |
| **P0** | R-PT-02 | `PerTaskFdRegistry` | `close_slot` 持 `RefMut` 调用 `handle.close()`（pipe2/socketpair 回滚、`dup3` 覆盖）→ 嵌套 FS 锁 + 延长 fd 表不可用窗口 | **未修复** |
| **P0** | R-PT-03 | `PerTaskFdRegistry` | `copy_fd_table_from_parent` / `flush_all` 持借执行 `duplicate()`/`flush()` → fork/sync 长临界区 + FS 锁交叉 | **未修复** |
| **P0** | R-PT-11 | 三表 | **无 `InterruptGuard`**：`exclusive_access` 可被定时器抢占打断；另一任务再借同一 cell → `RefCell already borrowed` panic（RC-2 延伸） | **未修复** |
| **P1** | R-PT-04 | `PerTaskFdRegistry` | `check_nofile_before_open` 持 fd 借调用 `nofile_rlimit_for_task` → `ProcessRegistry`（`ProcessRegistryInterruptGuard`）嵌套 | **未修复** |
| **P1** | R-PT-05 | `PerTaskCredRegistry` | `cred_or_panic` / `current_credentials()` 无侧表条目 → `panic!` | **未修复** |
| **P1** | R-PT-07 | 三表 | 无文档化全局锁顺序；新 syscall 易引入 A→B / B→A | **未修复** |
| **P1** | R-PT-12 | clone 路径 | 线程 clone 路径**忽略** `CLONE_FILES`/`CLONE_FS` flag，恒 `share_*`（语义偏差，加剧 FD-01 暴露面） | **已修复** |
| **P2** | R-PT-06 | `PerTaskFdRegistry` | `registry.close_cloexec_fds_for_task` 持借 close（死代码）；`fd.rs` 已有 take 后释借的安全包装 | **未修复** |
| **P3** | R-PT-08 | `PerTaskCwdRegistry` | `chdir`/`set_task_cwd` FS 校验与写入非原子（TOCTOU，非锁 bug） | 已知限制 |
| **P3** | R-PT-09 | `PerTaskCredRegistry` | `AccessCheck` 恒 true（权限未生效，非锁 bug） | 已知限制 |
| **P3** | R-PT-10 | 三表 | SMP 未保护（`UniprocessorSafeCell` 假设单核） | 已知限制 |

**已修复 / 已收敛（本子系统）**：无代码变更。下列路径**设计正确**，作为对照：

| 路径 | 模式 |
|------|------|
| `fd.rs::close_fd` | take → **释借** → `handle.close()` |
| `fd.rs::close_cloexec_fds_for_current_task` | take 列表 → **释借** → 逐个 close |
| `fd.rs::drop_task_fd_table` | drain → **释借** → 逐个 close |
| `fd.rs::with_current_io` | take → **释借** → I/O/sleep → **再借** restore（I/O 窗口本身不持 fd 借，但引入 FD-01 空槽竞态） |
| `cwd.rs::chdir_current` | resolve 段借 → **释借** → FS 校验 → 再借写入 |
| cred 读写 / fork / share / drop | 纯内存，持借极短 |
| 三表交叉 | **无** fd+cwd+cred 同时持 `RefMut` 的路径（串行：cwd → fd → cred） |

---

## 1. 概述

三个注册表均为 **全局单例 + `UniprocessorSafeCell<Registry>`**，按 `task::TaskId` 索引 per-task 状态，并支持 fork 复制 / thread clone 共享（`owners` + `ref_counts` 引用计数模型）。

| 结构 | 全局入口 | 实现体 | 锁类型 |
|------|---------|--------|--------|
| `PerTaskFdRegistry` | `wateros-vfs/src/fd.rs::registry()` | `vfs-impl/impl-fd-session/src/registry.rs` | `UniprocessorSafeCell` |
| `PerTaskCwdRegistry` | `wateros-vfs/src/cwd.rs::registry()` | `vfs-impl/impl-fd-session/src/cwd.rs` | `UniprocessorSafeCell` |
| `PerTaskCredRegistry` | `cred-impl/impl-root/src/lib.rs::registry()` | 同文件内 `PerTaskCredRegistry` | `UniprocessorSafeCell` |

**加锁 API 统一为** `registry().exclusive_access()` → `RefMut<'_, T>`；无显式 `unlock`，作用域结束即释借。

**与 `InterruptGuard` 对比**：调度器（`scheduler-impl`）、`ProcessRegistry`（`ProcessRegistryInterruptGuard`）、帧分配器（`FrameAllocatorInterruptGuard`）均在 `exclusive_access` 前关中断；**三表均无此配对**（§4）。

---

## 2. 原语：`UniprocessorSafeCell`

**文件**：`wateros-base/src/sync/uniprocessor.rs`

```rust
pub fn exclusive_access(&self) -> RefMut<'_, T> {
    match self.inner.try_borrow_mut() {
        Ok(inner) => inner,
        Err(_) => panic!("RefCell already borrowed: {}", type_name::<T>()),
    }
}
```

**语义要点**：

- 非自旋锁；并发/重入 `exclusive_access()` 同一实例 → **立即 panic**（表现为卡死前的硬崩溃）。
- 不同 `UniprocessorSafeCell` 实例之间 **无全局顺序约束**；嵌套借用不同实例合法，但构成隐式锁顺序，需人工保证无环。
- 文档明确：持借期间若睡眠/调度导致另一路径重入同一实例 → panic。
- **UP 抢占**：持 `RefMut` 期间若被 timer tick 切到另一任务，该任务再借同一 cell → panic（R-PT-11）。

---

## 3. PerTaskFdRegistry 审计

### 3.1 数据结构

```rust
pub struct PerTaskFdRegistry {
    tables: BTreeMap<TaskId, Vec<Option<Box<dyn VfsIoHandle>>>>,
    fd_flags: BTreeMap<TaskId, Vec<u8>>,
    owners: BTreeMap<TaskId, TaskId>,
    ref_counts: BTreeMap<TaskId, usize>,
}
```

- `tables` / `fd_flags`：实际 fd 槽与 `FD_CLOEXEC` / `FD_PATH_ONLY` 标志。
- `owners` / `ref_counts`：`CLONE_FILES` 线程共享 fd 表；最后一个引用释放时回收物理表。

### 3.2 加锁入口（`fd.rs` 门面）

| 函数 | 持锁区间 | 说明 |
|------|---------|------|
| `with_current_task` | 整个闭包 | 通用 fd 表操作 |
| `with_current_io` | **分段**：take → **释锁** → I/O → **再借** restore | 刻意缩短持锁（§5） |
| `close_fd` | take 后释锁，再 `handle.close()` | 正确模式 |
| `close_cloexec_fds_for_current_task` | take 列表后释锁，再逐个 close | 正确模式 |
| `drop_task_fd_table` | drain 后释锁，再 close | 正确模式 |
| `flush_all_open_files` | 全程持锁调用 `flush_all` | **持锁期间 FS I/O** |
| `init/copy/share_fd_table_*` | 全程持锁 | fork/clone 路径 |
| `alloc_fd` / `dup_*` / `fcntl` 等 | 经 `with_current_task` 持锁 | syscall 热路径 |

**直接 `registry().exclusive_access()` 的 syscall**：

- `pipe2.rs`、`socketpair.rs`：分配 fd 时持锁；错误回滚调用 `close_fd_for_task` → `close_slot`（见 §3.4）。

### 3.3 实现层持锁危险路径（`registry.rs`）

| 方法 | 持锁期间副作用 |
|------|---------------|
| `close_slot` | `take_fd_for_close` 后 **仍持 `&mut self` 调用 `handle.close()`** |
| `close_fd_for_task` | 委托 `close_slot` |
| `dup3_fd_for_task` | 持锁期间 `close_slot` + `handle.duplicate()` |
| `copy_fd_table_from_parent` | 持锁遍历父表并对每个句柄 `duplicate()` |
| `flush_all` | 持锁对所有句柄 `flush()` |
| `check_nofile_before_open` | 持锁期间调用 `task::nofile_rlimit_for_task` → `ProcessRegistry` |
| `close_cloexec_fds_for_task`（registry 内） | 持锁 `close_slot`；**外部未使用**，`fd.rs` 用 `take_cloexec_fds_for_task` 替代 |

### 3.4 `with_current_io` 设计

```43:64:os/components/wateros-vfs/src/fd.rs
pub fn with_current_io<R>(fd : usize,
                          f : impl FnOnce(&mut (dyn VfsIoHandle + '_)) -> VfsResult<R>)
                          -> VfsResult<R> {
    let task_id = current_task_id()?;
    let mut handle = {
        let mut reg = registry().exclusive_access();
        reg.take_io_for_task(task_id, fd)?
    };
    let result = f(handle.as_mut());
    let restore_result = {
        let mut reg = registry().exclusive_access();
        reg.restore_io_for_task(task_id, fd, handle)
    };
    // ...
}
```

- **优点**：I/O / `poll_wait` / `sleep` 不持有 fd 注册表借用，避免 RefCell 重入 panic。
- **缺陷（FD-01）**：fd 槽在 I/O 窗口内为 `None`；共享 fd 表的 sibling 线程可观察到 `BadFd`、重复分配同一槽位、或 restore 失败（§6.2）。

**主要调用方**（syscall 热路径）：`read`/`write`/`ioctl`/`lseek`/`fstat`/`getdents64`/`sync`/`ftruncate`/`fallocate`/`sendfile`/`mmap`/`path_at`/`faccessat`/`poll_engine.rs`（含 `poll_wait_for_ticks` 阻塞）。

### 3.5 持锁期间 syscall / 睡眠分析

| 路径 | 是否持 fd 借 | 是否可能睡眠/阻塞 |
|------|-------------|------------------|
| `read`/`write`/`ioctl`/`poll` 等经 `with_current_io` | **否**（I/O 窗口无借） | 是（pipe wait、scheduler sleep） |
| `close_fd`（门面） | **否**（close 在释借后） | close 可能触发 FS |
| `pipe2`/`socketpair` 成功路径 | 仅 alloc 瞬间 | 否 |
| `pipe2`/`socketpair` 失败回滚 | **是**（`close_fd_for_task`→`close_slot`） | **是** |
| `fork` → `copy_fd_table_from_parent` | **是**（全程 duplicate） | duplicate 可能触及 page cache |
| `sync` → `flush_all_open_files` | **是** | **是**（块设备/缓存写回） |
| `dup3` 覆盖已打开 newfd | **是** | **是** |

---

## 4. InterruptGuard 使用情况

| 结构 | 是否配对 InterruptGuard | 说明 |
|------|------------------------|------|
| `PerTaskFdRegistry` | **否** | 所有 `registry().exclusive_access()` 裸调用 |
| `PerTaskCwdRegistry` | **否** | 同上 |
| `PerTaskCredRegistry` | **否** | 同上 |
| `ProcessRegistry`（被 fd 表间接嵌套） | **是** | `ProcessRegistryInterruptGuard` + `exclusive_access` |
| `TaskScheduler` | **是** | `InterruptGuard` + `with_scheduler` |

**风险（R-PT-11）**：在 UP + 定时器抢占下，任务 A 持 fd `RefMut`（如 `copy_fd_table_from_parent` 循环 duplicate）被抢占 → 任务 B 任意 fd 操作 → **`RefCell already borrowed: PerTaskFdRegistry` panic**。

**与 `with_current_io` 的关系**：I/O 窗口**已释借**，poll/read 阻塞本身不持 fd 借；FD-01 是**语义竞态**（空槽），不是 InterruptGuard 缺失的直接后果。但 pipe2 回滚、fork duplicate、flush 等长持借路径同时受 R-PT-02/03 与 R-PT-11 影响。

**收敛建议**：

1. 门面层引入 `FdRegistryInterruptGuard`（与 `ProcessRegistryInterruptGuard` 同构），包裹所有运行期 `exclusive_access`；或
2. 长 I/O 路径统一 take → 释借 → 操作（消除 R-PT-02/03）后再评估是否仍需 guard。

---

## 5. CLONE_FILES 与共享表模型

### 5.1 引用计数语义

`share_fd_table_from_parent` / `share_cwd_from_parent` / `share_cred` 均：

1. `owners.insert(child, owner)` — 子 task 指向共享 owner 的物理表；
2. `ref_counts[owner] += 1` — 最后一个引用归零时才 `drop` 物理表。

`take_io_for_task` / `restore_io_for_task` 操作的是 **owner 级物理表**，故 `CLONE_FILES` 共享表下所有 sibling 看到同一槽位数组。

### 5.2 clone 路径实际行为

**进程 fork**（`clone.rs::do_clone_fork` 路径）：

```
cwd::copy_cwd_from_parent   [cwd 借]
fd::copy_fd_table_from_parent [fd 借, duplicate×N]
cred::fork_cred             [cred 借]
```

**线程 clone**（`CLONE_VM | CLONE_THREAD` → `do_clone_thread`）：

```267:271:os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/clone.rs
    vfs::cwd::share_cwd_from_parent(child_id, parent_id);
    vfs::fd::share_fd_table_from_parent(child_id, parent_id);
    // ...
    cred::share_cred(parent_id, child_id);
```

**关键发现（R-PT-12）**：线程路径**不检查** `clone_flags` 中的 `CLONE_FILES` / `CLONE_FS`；凡 `CLONE_VM|CLONE_THREAD` 均无条件 `share_*`。glibc `pthread_create` 通常带 `CLONE_FILES|CLONE_FS`，故常见场景与 Linux 一致；但 flag 未设置时 WaterOS 仍共享，与 Linux 语义不符。

### 5.3 FD-01 触发场景

```
线程 A（共享 fd 表，ref_counts > 1）:
  poll/read → with_current_io(3)
    → take_io: tables[owner][3] = None
    → poll_wait_for_ticks → sleep          [无 fd 借]

线程 B（同 owner）:
  dup(3) / close(3) / open 复用槽 3
    → A 的 restore_io_for_task(3) 失败（槽已占或 BadFd）
    → warn + 返回错误
```

`poll_engine.rs` 注释已说明 take 导致同 fd 重扫 `POLLNVAL` 风险；根因是 take-restore 非 Linux fdtable 引用计数模型。

### 5.4 任务回收顺序

`drop_task_runtime_resources_with_aspace`（`task.rs`）：

```
cwd::drop_task_cwd → fd::drop_task_fd_table → cred::drop_task_cred
```

与 fork 顺序一致：**cwd → fd → cred**。

---

## 6. PerTaskCwdRegistry 审计

### 6.1 数据结构

```rust
pub struct PerTaskCwdRegistry {
    cwd_tables: BTreeMap<TaskId, String>,
    exe_paths: BTreeMap<TaskId, String>,
    argv_vectors: BTreeMap<TaskId, Vec<String>>,
    owners: BTreeMap<TaskId, TaskId>,
    ref_counts: BTreeMap<TaskId, usize>,
}
```

### 6.2 加锁入口（`cwd.rs` 门面）

| 函数 | 持锁区间 | 持锁期间副作用 |
|------|---------|---------------|
| `init_task_cwd` / `on_user_task_spawned` | 短 | 纯内存 |
| `set_task_cwd` | 先 FS 校验（**无 cwd 借**），再借 | 仅字符串写入 |
| `chdir_current` | **两段**：resolve 时借 → 释借 → FS 校验 → 再借写入 | 中间 FS 不持 cwd 借 |
| `resolve_for_current_task` | 全程借 | 纯字符串解析 |
| `write_cwd_to_buf` / `current_exe_path` / `current_argv` | 全程借 | 纯内存拷贝 |
| `lookup_argv_for_task` / `lookup_exe_for_task` | 全程借 | procfs 回调入口 |
| `copy/share_cwd_from_parent` | 全程借 | fork/clone |
| `drop_task_cwd` | 全程借 | 任务回收 |

### 6.3 与 open 路径解析的耦合

- 启动时 `register_open_path_resolver(resolve_for_current_task)`。
- `openat` 典型顺序：`resolve_open_path`（cwd 借）→ `backend.open`（FS 锁）→ `alloc_fd`（fd 借）——**三者顺序串行，无同时持有 cwd+fd 借**。

### 6.4 持锁期间 syscall / 睡眠分析

| 路径 | 是否持 cwd 借 | 是否可能睡眠 |
|------|-------------|-------------|
| `getcwd` / `chdir` resolve 段 | 短 | 否 |
| `chdir` FS 校验段 | **否** | 可能（FS/page cache） |
| `set_task_cwd` FS 校验 | **否** | 可能 |
| procfs `argv_for` 回调 | **是**（仅内存） | 否 |
| `resolve_for_current_task` | **是** | 否 |

**procfs 锁顺序**：读 `/proc/pid/cmdline` 时 `ARGV_LOOKUP`（`spin::Mutex`）→ 回调 → cwd 借。PROC-01 已修复（释 Mutex 后调回调）；当前为 **procfs_mutex → cwd 借**，反向路径未见。

---

## 7. PerTaskCredRegistry 审计

### 7.1 数据结构

```rust
pub struct PerTaskCredRegistry {
    creds: BTreeMap<TaskId, ProcessCredentials>,
    owners: BTreeMap<TaskId, TaskId>,
    ref_counts: BTreeMap<TaskId, usize>,
}
```

### 7.2 加锁入口（`impl-root/src/lib.rs`）

| 函数 | 持锁区间 |
|------|---------|
| `on_user_task_spawned` | 短；插入 `ProcessCredentials::ROOT` |
| `fork_cred` / `share_cred` | fork/clone |
| `on_exec` | no-op（TODO setuid） |
| `drop_task_cred` | 任务回收 |
| `current_credentials_for` | 读 |
| `set_resuid` / `set_resgid` | 写 |
| `has_cap` / `may_access_inode` | 读（当前恒 `true`） |

**`wateros-cred/src/lib.rs` 门面**：每次 mutation/read 独立 `exclusive_access()`，无跨调用持借。

### 7.3 错误处理

- `cred_or_panic` / `cred_mut_or_panic`：无侧表条目 → **`panic!`**，非返回错误。
- `current_credentials()`：无当前任务 → `expect` panic。

### 7.4 持锁期间 syscall / 睡眠分析

| 路径 | 持 cred 借 | 睡眠 |
|------|-----------|------|
| `getuid`/`getgid`/`faccessat` cred 检查 | 极短 | 否 |
| `setuid`/`setgid` | 短 | 否 |
| fork/clone/exec/drop | 短 | 否 |

cred 表操作均为纯内存；**无持 cred 借睡眠路径**。

---

## 8. 三表交叉分析

### 8.1 观测到的全局顺序（无嵌套双借）

当前代码中 **未发现** 同时持有任意两个注册表 `RefMut` 的路径。多表操作均为 **串行**：

| 场景 | 顺序 |
|------|------|
| `fork`（`clone.rs`） | cwd → fd → socket/unix → **cred** |
| `clone` 线程（`CLONE_VM`） | cwd(share) → fd(share) → **cred(share)** |
| `execve` | cwd(resolve) → … → fd(cloexec) → **cred(on_exec)** → cwd(set exe/argv) |
| 任务回收（`task.rs::drop_task_runtime_resources`） | cwd → fd → **cred** |
| `openat` | cwd(resolve) → FS → fd(alloc) |
| `faccessat` | fd/metadata 或 cwd(resolve) → FS → **cred**（串行） |

**建议约定锁顺序**（与现有 fork/drop 一致）：**cwd → fd → cred → ProcessRegistry**。禁止在持 fd 借时调用可能重入 fd 的 VFS 操作。

### 8.2 fd 与 cwd 的间接耦合（非双借，但有语义风险）

1. **`resolve_path_at(AT_FDCWD)`**：cwd 借（resolve）— 与 fd 无交叠。
2. **`resolve_path_at(dirfd)`**：`with_current_io(dirfd)`（fd take/restore）→ 字符串 `resolve_against_cwd` — **顺序**，非嵌套。
3. **`with_current_io` 空槽窗口**（FD-01，§5.3）。

### 8.3 fd/cwd 与 ProcessRegistry 的嵌套

- `check_nofile_before_open` 在 **持 fd 借** 时调用 `task::nofile_rlimit_for_task` → `process_task_snapshot` → `lookup_task` → `with_process_registry`（`ProcessRegistryInterruptGuard` + 另一 `UniprocessorSafeCell`）。
- 顺序：**fd → ProcessRegistry**。若未来 ProcessRegistry 回调内 open fd，将 RefCell panic。
- 当前未见反向路径。

### 8.4 fd 与下层 FS 锁的嵌套

持 fd 借期间可能再获取：

- `GlobalFilePageCache` / `SharedFs` 的 `spin::Mutex`（`duplicate`/`close`/`flush`/`read`）
- procfs 的 `ARGV_LOOKUP` `Mutex`（经 fd I/O 间接触发，通常 **已释 fd 借**）

**风险**：`copy_fd_table_from_parent`、`flush_all`、`close_slot`（pipe2 回滚）在持 fd 借时进入 FS 锁域；若 FS 路径反向需要 fd 表，可能 **死锁（spin 自旋）或 RefCell panic**。

---

## 9. 当前实际支持范围

### 9.1 已较可靠路径

- 单线程进程内：`open`/`read`/`write`/`close`/`dup`/`fcntl`/`getcwd`/`chdir`（无 sibling 共享 fd）。
- `with_current_io` 路径：**单线程 task** 下 I/O 不触发 RefCell 重入。
- fork（进程）：cwd/fd/cred 串行复制，各表独立持借。
- execve：cloexec 经 take 后释借再 close；cwd 更新 exe/argv 短持借。
- cred 读写：纯内存，持借极短。

### 9.2 未完整覆盖 / 不可靠路径

- **`CLONE_FILES` 共享表线程** + `poll`/`read`/`write` 阻塞 + 并发 `dup`/`close`/`fcntl` 同一 fd（FD-01）。
- **fork** 期间父进程继续运行并操作 fd（持借 duplicate 全表时阻塞整个 fd 表）。
- **`sync`/`fsync` 全局 flush** 持 fd 借写回所有打开文件。
- **pipe2/socketpair 分配失败回滚** 持借 close。
- **cred 侧表未初始化** 的 task 上调用 `current_credentials()`。
- **无 InterruptGuard** 的长持借路径在抢占下 panic（R-PT-11）。
- **SMP**（未实现）。

---

## 10. 收敛建议

### FD-01 / R-PT-01（with_current_io 共享表竞态）

1. 检测 `effective_owner` 对应 `ref_counts > 1` 时，**不要 take 槽位**；改为持借 dispatch（仅当 handle I/O 不递归 fd 表），或引入 per-fd 引用计数。
2. 短期收敛：`ref_counts > 1` 且需阻塞 I/O 时 `log::warn!` + 返回 `EOPNOTSUPP`/`EIO`。
3. warn 格式：`[vfs-fd] shared fd table take_io unsupported task_id={:?} fd={} owner_ref={}`。

### R-PT-02 / R-PT-03（持 fd 借做 close/duplicate/flush）

1. 统一采用 **take → 释借 → 操作 → 再借写回** 模式（与 `fd.rs::close_fd` 一致）。
2. `copy_fd_table_from_parent`：先在持借下收集 fd 列表，**释借后** batch duplicate，再持借安装子表。
3. `flush_all`：收集句柄克隆或 fd 列表，释借后 flush；或文档标注仅 bring-up 调用。
4. `pipe2`/`socketpair` 回滚：改为 `take_fd_for_close` + 释借 + `close()`。

### R-PT-11（无 InterruptGuard）

1. 三表门面层添加与 `ProcessRegistryInterruptGuard` 同构的 RAII；或
2. 先消除长持借路径（R-PT-02/03），再评估短路径是否需要。

### R-PT-05（cred panic）

1. `current_credentials_for` 返回 `Option`/`Result`；syscall 映射 `ESRCH`/`EINVAL`。
2. spawn 路径 assert 改为 warn + 安全失败。

### R-PT-04（nofile + ProcessRegistry）

1. `check_nofile_before_open` 前先 **释借** 查询 rlimit，或缓存 per-task limit。

### R-PT-12（clone flag）

1. `do_clone_thread` 按 `CLONE_FILES`/`CLONE_FS` 选择 `share_*` vs `copy_*`。

### 文档化锁顺序

在 `wateros-vfs` / `wateros-cred` impl-guide 注明：**cwd → fd → cred → ProcessRegistry**。

---

## 11. 调用链速查

### fork（`sys_clone` 进程）

```
task::fork_current
→ vfs::cwd::copy_cwd_from_parent      [cwd 借]
→ vfs::fd::copy_fd_table_from_parent  [fd 借, duplicate×N]
→ cred::fork_cred                     [cred 借]
```

### poll 阻塞（`poll_engine.rs`）

```
scan_pollfds (无 fd 借)
→ poll_wait_pipe_fds
  → vfs::fd::with_current_io          [take → 释借]
    → handle.poll_wait_for_ticks      [可能 wait_queue/sleep，无 fd 借]
  → restore                           [再借]
→ task::sleep_for_ticks               [无三表借]
```

### openat

```
resolve_open_path → cwd::resolve_for_current_task  [cwd 借]
backend.open                                       [FS 锁]
vfs::fd::alloc_fd                                  [fd 借]
```

---

## 12. 审计结论

三表均依赖 `UniprocessorSafeCell`，**单核单路径重入同一表会 panic**；**均未配对 InterruptGuard**（R-PT-11）。当前多表访问以 **串行** 为主，**无 fd+cwd+cred 同时持借** 的代码路径。

主要风险：

1. **`with_current_io` + 共享 fd 表**（FD-01 / P0）；
2. **registry 内部持借执行 close/duplicate/flush**（R-PT-02/03 / P0）；
3. **无 InterruptGuard 的长临界区**（R-PT-11 / P0，RC-2 延伸）；
4. **cred panic 与 fd→ProcessRegistry 嵌套**（R-PT-04/05 / P1）。

`poll`/`read`/`write` 等 syscall **不在持 fd 注册表借时睡眠**（`with_current_io` 释借设计正确）；但 **pipe2 回滚、fork duplicate、sync flush** 仍在持借时可能阻塞，且可被抢占放大为 panic。

---

## 附录：与 lock-inventory #5 / #6 / #7 对应

| 清单 # | 结构 | 本文档 |
|--------|------|--------|
| 5 | `PerTaskFdRegistry` | §3、§5、FD-01 |
| 6 | `PerTaskCwdRegistry` | §6 |
| 7 | `PerTaskCredRegistry` | §7 |
