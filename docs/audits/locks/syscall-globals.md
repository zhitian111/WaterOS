# 锁机制审计：syscall 层全局结构（#31–#34）

> Subagent：`syscall-globals`  
> 审计日期：2026-06-25（复核源码）  
> Baseline：单核多线程；`spin::Mutex` 为自旋锁，持锁线程若被抢占则等待方空转  
> 关联清单：`docs/audits/lock-inventory.md` #31–#34

---

## 0. P0 / P1 / Fixed 摘要

| 级别 | ID | 结构 | 问题 | 状态 |
|------|-----|------|------|------|
| **P0** | U-01 | BOUND + UnixSockInner | `bind` 持 Inner+BOUND 调 VFS mknod/metadata | **已修复** — VFS 在无锁区完成，再短持 Inner→BOUND 插入 |
| **P0** | U-02 | FD_TABLE + BOUND | 任务退出未调 `unix_sock::drop_task` | **已修复** — `drop_task_runtime_resources_with_aspace` 已接入 |
| **P0** | U-03 | FD_TABLE | pthread `clone` 不同步 FD_TABLE | **已修复** — `do_clone_thread` 调用 `copy_fds_from_parent` |
| P1 | U-04 | FD_TABLE | `dup`/`dup3`/`fcntl(F_DUPFD*)` 不注册 FD_TABLE | **未修复** |
| P1 | U-08 | FD_TABLE + BOUND | `execve` 终止兄弟线程时未调 `socket_fd`/`unix_sock::drop_task` | **未修复** |
| P1 | U-09 | BOUND + VFS | `bind` VFS 先装后锁存在 TOCTOU：并发 bind 同路径时败者遗留 mknod | **未修复**（非死锁，资源/语义） |
| P1 | C-01 | TIMEX_STATE | `timex_snapshot` 持 TIMEX_STATE 锁读 wall clock | **未修复** |
| P1 | C-02 | TIMEX_STATE | `TIMEX_STATE.offset` 与 `REALTIME_OFFSET_NS` 脱节 | **未修复**（语义，非锁） |
| 低 | U-05 | Inner + BOUND | `recvfrom_unix`/`poll_revents` 嵌套 Inner→BOUND，扩大 BOUND 临界区 | 可接受，顺序一致 |
| 低 | T-01 | TIMES | 无 prune，BTreeMap 长期膨胀 | 资源问题，非锁 bug |

**本轮结论**：原 Top 3 P0（bind 持锁 VFS、exit 未 drop_task、thread clone 未同步）均已修复；剩余主要为 dup 侧车表缺口、execve 兄弟线程清理、bind 竞态与 TIMEX 临界区。

---

## 1. 概述

本组覆盖 syscall 层四个全局带锁结构，均位于 `wateros-syscall/syscall-impl/impl-kernel`：

| # | 结构 | 文件 | 锁类型 | 职责 |
|---|------|------|--------|------|
| 31 | `SOCKET_FD_REGISTRY` | `socket_fd.rs` | `spin::Mutex` | inet socket fd → `SocketRef` + O_NONBLOCK 标志 |
| 32 | `FD_TABLE` / `BOUND` / `UnixSockInner` | `unix_sock.rs` | `spin::Mutex` ×3 | AF_UNIX fd 映射、全局 bind 表、per-socket 状态 |
| 33 | `TIMES` | `stat_times.rs` | `spin::Mutex` | utimensat 写入的 atime/mtime 覆盖表 |
| 34 | `TIMEX_STATE` | `sys/clock.rs` | `spin::Mutex` | adjtimex / clock_adjtime 状态 |

与 **PerTaskFdRegistry**（#5）的关系：VFS fd 表是 socket/unix 的「主索引」；`SOCKET_FD_REGISTRY` 与 `FD_TABLE` 为平行侧车表，在 `socket(2)`/`close(2)`/`clone(2)`/`dup` 等路径上与 fd 表协同更新，但**锁相互独立、无统一顺序**。

---

## 2. SOCKET_FD_REGISTRY（#31）

### 2.1 数据结构

```rust
static SOCKET_FD_REGISTRY: Mutex<SocketFdRegistry>
// SocketFdRegistry: maps, status_flags, owners, ref_counts (BTreeMap)
```

### 2.2 加锁调用点

| 函数 | 操作 | 持锁区间 |
|------|------|----------|
| `register_with_flags` | lock → register | 短 |
| `lookup` | lock → lookup | 短 |
| `lookup_or_errno` | lock → lookup；**释锁后** `vfs::fd::with_current_io` | 分段 |
| `remove` | lock → remove | 短 |
| `status_flags` / `set_status_flags` | lock → 读写 | 短 |
| `is_nonblocking` | 经 `status_flags`，独立一次 lock | 短 |
| `copy_from_parent` / `share_from_parent` | lock → fork/线程继承 | 短 |
| `drop_task` | lock → release_task | 短 |

### 2.3 与 fd registry 交互

**注册**（`sys/socket.rs` AF_INET）：`vfs::fd::alloc_fd` → `socket_fd::register_with_flags`（两锁分开，无嵌套）。

**关闭**（`sys/close.rs`）：先 `lookup` 记 was_socket → `vfs::fd::close_fd` → `socket_fd::remove`。顺序合理，无交叉持锁。

**dup/fcntl**（`sys/dup.rs`、`sys/fcntl.rs`）：`lookup` + `status_flags` 为**两次独立加锁**；`vfs::fd::dup_fd` 在中间。存在 TOCTOU：fd 表复制与 registry 注册非原子，极端并发下可能短暂不一致（单核下窗口极小）。

**进程/线程退出**（`sys/task.rs::drop_task_runtime_resources_with_aspace`）：`vfs::fd::drop_task_fd_table` 后调用 `socket_fd::drop_task`。**已覆盖**。

**execve 兄弟线程**（`sys/execve.rs`）：仅 `vfs::fd::drop_task_fd_table` + cred/cwd；**未**调用 `socket_fd::drop_task` → 见 U-08。

**clone fork**（`do_clone_fork`）：`copy_fd_table_from_parent` → `copy_from_parent`（深拷贝 maps）。**已覆盖**。

**clone 线程**（`do_clone_thread`）：`share_fd_table_from_parent` → `share_from_parent`（共享 owner + ref_count）。**已覆盖**。

### 2.4 潜在问题

| 严重度 | 问题 | 说明 |
|--------|------|------|
| 低 | `lookup_or_errno` 与 `is_nonblocking` 多次短锁 | 调用方常连续 lookup + status_flags，非死锁但语义非原子 |
| 低 | dup/fcntl 不更新 `unix_sock::FD_TABLE` | 仅影响 AF_UNIX（见 §3.4 U-04），inet 路径正常 |
| P1 | execve 兄弟线程未 `drop_task` | 与 unix 表同源问题 U-08 |
| 语义偏差 | `SocketRef.inner` 另有 per-socket `Mutex` | inet I/O 持 `SocketRef.inner` 与 registry 无顺序约定；当前路径不嵌套 |

---

## 3. FD_TABLE / BOUND / UnixSockInner（#32）

### 3.1 三层锁模型

```
FD_TABLE: Mutex<BTreeMap<(task_id, fd), UnixSockRef>>
BOUND:    Mutex<BTreeMap<Vec<u8>, BoundEntry>>          // 全局 pathname/abstract 绑定
UnixSockInner: Arc<Mutex<UnixSockInner>>               // per-socket 状态
```

### 3.2 锁顺序分析

#### 约定顺序（实际代码）

| 路径 | 顺序 | 嵌套？ |
|------|------|--------|
| `lookup_current` | `FD_TABLE` → 释锁 | 否 |
| `register` | `FD_TABLE` | 否 |
| `unregister` | `FD_TABLE` → `UnixSockInner` → `BOUND` | 是 |
| **`bind`（已修复）** | 无锁 VFS → `UnixSockInner` → `BOUND` | 是，**VFS 在锁外** |
| `listen` / `connect_stream` | `UnixSockInner` → `BOUND` | 是 |
| `accept` 循环 | 短暂 `UnixSockInner` → 释锁 → `BOUND` → sleep | 否（sleep 前已释锁） |
| `sendto_unix` | `UnixSockInner` → 释锁 → `BOUND` | 否 |
| `recvfrom_unix` | `UnixSockInner` → **`BOUND`（持 inner 嵌套）** → 释锁 → sleep | 是 |
| `poll_revents` | `UnixSockInner` → **`BOUND`（嵌套）** | 是 |
| `pop_dgram_packet` | 调用方持 `UnixSockInner` → `BOUND` | 是 |
| `read`/`write` | 持 `UnixSockInner`；dgram write 释 inner 后 `BOUND` | 部分嵌套 |

**结论**：凡同时持有 `UnixSockInner` 与 `BOUND`，顺序均为 **Inner → BOUND**；`unregister` 为 **FD_TABLE → Inner → BOUND**。未发现 **BOUND → Inner** 反向路径，**同对锁无 AB-BA 死锁**。

#### bind 修复（U-01，已落地）

```181:211:os/components/wateros-syscall/syscall-impl/impl-kernel/src/unix_sock.rs
pub(crate) fn bind(fd: usize, addr_ptr: usize, addrlen: usize) -> Result<(), ErrNo> {
    // ...
    if !addr.abstract_ns {
        validate_pathname_bind(&addr.key)?;      // 无 BOUND/Inner 锁
        if !addr.key.is_empty() {
            install_pathname_socket(&addr.key)?; // 无 BOUND/Inner 锁
        }
    }
    let mut inner = sock.inner.lock();
    let mut bound = BOUND.lock();
    // 仅表插入，无 VFS
    bound.insert(...);
    inner.bound_key = Some(addr.key);
    Ok(())
}
```

VFS 与 FS/page-cache 锁不再与 `BOUND`/`Inner` 嵌套，**跨子系统死锁风险已消除**。残余 U-09：VFS 安装与 BOUND 插入非原子，并发 bind 同路径时可能遗留 orphan mknod。

#### 与 FD registry 的交互

| 操作 | VFS fd 表 | FD_TABLE | SOCKET_FD_REGISTRY |
|------|-----------|----------|-------------------|
| `socket(AF_UNIX)` | alloc_fd | register | 不使用 |
| `close` | close_fd | unregister（若 was_unix） | 不使用 |
| fork clone | copy_fd_table | copy_fds_from_parent | copy_from_parent |
| thread clone | share_fd_table | **copy_fds_from_parent** | share_from_parent |
| dup/fcntl | dup_fd | **未更新** | 仅 inet 更新 |
| exit / reap | drop_task_fd_table | **drop_task → unregister×N** | drop_task |
| execve 杀线程 | drop_task_fd_table | **未 drop_task** | **未 drop_task** |

**三表无统一锁序**；当前实现依赖「各表独立短临界区 + syscall 层顺序调用」，未形成跨表嵌套死锁；**dup 与 execve 路径侧车表覆盖仍不完整**。

### 3.3 drop_task 生命周期（U-02，已修复）

```109:118:os/components/wateros-syscall/syscall-impl/impl-kernel/src/unix_sock.rs
pub(crate) fn drop_task(task_id: usize) {
    let fds: Vec<usize> = FD_TABLE.lock()...collect();
    for fd in fds {
        unregister(task_id, fd);  // 含 BOUND 清理
    }
}
```

接入点：

```770:776:os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task.rs
fn drop_task_runtime_resources_with_aspace(task_id: TaskId, aspace: usize) {
    // ...
    vfs::fd::drop_task_fd_table(task_id);
    crate::socket_fd::drop_task(task_id);
    crate::unix_sock::drop_task(task_id);  // ✓
    cred::drop_task_cred(task_id);
}
```

覆盖路径：`exit`、线程 reap（`drop_reaped_task_runtime_resources`）、部分 task 清理。  
**缺口**：`execve` 对 `killed_threads` 仅 drop cwd/fd/cred，未调 unix/socket drop_task（U-08）。

### 3.4 持锁与睡眠/调度

| 路径 | 持锁期间 sleep？ | 评估 |
|------|------------------|------|
| `accept` | 否（`socket_blocking_tick` 在锁外） | 正确 |
| `recvfrom_unix` | 否 | 正确 |
| `read`（UnixSocketHandle） | 否 | 正确 |
| **`bind`** | 否 sleep；VFS 已在锁外 | **已修复** |

### 3.5 潜在问题汇总

| 严重度 | ID | 问题 | 状态 |
|--------|-----|------|------|
| ~~**高**~~ | U-01 | `bind` 持 Inner+BOUND 执行 VFS | **已修复** |
| ~~**高**~~ | U-02 | 任务退出未调 `unix_sock::drop_task` | **已修复** |
| ~~**高**~~ | U-03 | pthread clone 不同步 FD_TABLE | **已修复**（`copy_fds_from_parent`） |
| 中 | U-04 | `dup`/`fcntl` 不更新 FD_TABLE | 未修复 |
| 中 | U-08 | execve 兄弟线程未 drop unix/inet 侧车表 | 未修复 |
| 中 | U-09 | bind VFS 先装后锁 TOCTOU / orphan mknod | 未修复 |
| 低 | U-05 | `recvfrom_unix`/`poll_revents` 嵌套 Inner+BOUND | 顺序一致，可接受 |
| 低 | U-07 | 单核 `spin::Mutex` + 非阻塞循环空转 | 全局性问题 |

### 3.6 当前支持范围

| 路径 | 加锁 | 备注 |
|------|------|------|
| socket/close/register | ✓ | 正常 |
| bind/listen/connect/accept | ✓ | bind VFS 已移出临界区 |
| dgram sendto/recvfrom | ✓ | 阻塞路径正确释锁后 sleep |
| stream read/write | ✓ | endpoint I/O 在 inner 内 |
| poll (unix) | ✓ | 嵌套 BOUND |
| fork 继承 | ✓ | copy_fds_from_parent |
| thread 继承 | ✓ | copy_fds_from_parent（共享 Arc inner） |
| 进程/线程 exit/reap | ✓ | drop_task 已接入 |
| execve 杀兄弟线程 | ✗ | 侧车表泄漏 |
| dup 继承 | ✗ | FD_TABLE 未更新 |

### 3.7 收敛建议

| ID | 建议 |
|----|------|
| U-04 | dup/fcntl 成功路径对 `is_unix_fd(oldfd)` 调用 `unix_sock::register(newfd, sock.clone())` |
| U-08 | `execve` 的 `killed_threads` 循环增加 `socket_fd::drop_task` + `unix_sock::drop_task` |
| U-09 | bind 失败于 BOUND 冲突时 unlink mknod；或全程用「先 BOUND 占位再 VFS」并定义失败回滚 |

---

## 4. TIMES（#33）

### 4.1 结构

```rust
static TIMES: Mutex<BTreeMap<FileKey, FileTimes>>
// FileKey = (dev_major, dev_minor, inode)
```

### 4.2 调用点

| 函数 | 路径 | 持锁 |
|------|------|------|
| `set` | `sys/utimensat` → `apply_times` | lock → insert/update → 释锁 |
| `apply_stat` | `sys/fstat` / fstatat | lock → get → 释锁 |
| `apply_statx` | fstatx 路径 | 同上 |

### 4.3 分析

- **持锁闭环**：所有路径均在单临界区内完成，无 sleep、无嵌套其它 syscall 全局锁。
- **与 VFS 交互**：`utimensat` 先 `metadata`（无 TIMES 锁），再 `stat_times::set`；`fstat` 先取 metadata 再 `apply_stat`。无死锁。
- **无删除接口**：inode 删除后表项永久保留 → 内存增长（T-01，非锁 bug）。
- **并发**：`set` 与 `apply_stat` 竞争同键；Mutex 保证原子性。

### 4.4 潜在问题

| 严重度 | ID | 问题 |
|--------|-----|------|
| 低 | T-01 | 无 prune，长期运行 BTreeMap 膨胀 |
| 低 | T-02 | `apply_stat` 与 `set` 非同一快照，极端并发下 stat 可能略滞后 utimensat（可接受） |

---

## 5. TIMEX_STATE（#34）

### 5.1 结构

```rust
static TIMEX_STATE: Mutex<TimexState>
```

关联：`REALTIME_OFFSET_NS`（`wateros-platform/wall_clock.rs`，`AtomicI64`）为 **实际** CLOCK_REALTIME 偏移；`TIMEX_STATE.offset` **未参与** `realtime_ns()`。

### 5.2 调用点

| 入口 | 函数 | 加锁 |
|------|------|------|
| `sys_adjtimex` | `do_adjtimex(CLOCK_REALTIME, …)` | 见下 |
| `sys_clock_adjtime` | `do_adjtimex(clock_id, …)` | 同上 |
| 读路径 | `clock_gettime` / `gettimeofday` 等 | **不持** TIMEX_STATE 锁 |

### 5.3 `do_adjtimex` 持锁区间

```313:321:os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/clock.rs
let snapshot = {
    let mut state = TIMEX_STATE.lock();
    if write_only_mode {
        if let Err(e) = update_timex_state(&mut state, timex) {
            return UserRet::from_error(e);
        }
    }
    timex_snapshot(*state)   // 持锁期间 clock_id_to_ns → realtime_ns()
};
match copy_to_user_struct(timex_ptr, &snapshot) { ... }  // 释锁后 copy_to_user ✓
```

- `update_timex_state`：纯内存，无 sleep。
- `timex_snapshot` 持锁读 wall clock（C-01）：无 sleep，但延长临界区。
- `copy_to_user_struct` 在锁外：**正确**（避免 page fault 持锁）。

### 5.4 潜在问题

| 严重度 | ID | 问题 |
|--------|-----|------|
| 中 | C-01 | `timex_snapshot` 在持 TIMEX_STATE 锁时读 wall clock |
| 中 | C-02 | `TIMEX_STATE.offset/freq` 与 `REALTIME_OFFSET_NS` 脱节（语义偏差） |
| 低 | C-03 | `clock_adjtime` 非 REALTIME clock_id 返回 EINVAL，无额外锁风险 |

### 5.5 收敛建议

| ID | 建议 |
|----|------|
| C-01 | 锁内仅 update + copy `TimexState` 值，释锁后再 `clock_id_to_ns` 填 `UserTimex.time` |
| C-02 | `ADJ_OFFSET` 同步写入 `REALTIME_OFFSET_NS`，或 adjtimex 明确为 stub |

---

## 6. 跨结构死锁矩阵

|  | SOCKET_FD | FD_TABLE | BOUND | UnixSockInner | TIMES | TIMEX | PerTaskFdRegistry | VFS/FS 锁 |
|--|-----------|----------|-------|---------------|-------|-------|-------------------|-----------|
| SOCKET_FD | — | 无嵌套 | 无 | 无 | 无 | 无 | 顺序调用 | 无 |
| FD_TABLE | 顺序 | — | unregister 序 | unregister 序 | 无 | 无 | close/lookup 顺序 | 无 |
| BOUND | 无 | 无 | — | Inner→BOUND | 无 | 无 | 无 | **bind：VFS 已在锁外** |
| TIMES | 无 | 无 | 无 | 无 | — | 无 | 无 | metadata 在锁外 |
| TIMEX | 无 | 无 | 无 | 无 | 无 | — | 无 | 无 |

**原 P0 跨子系统死锁（bind 嵌套 VFS）已消除**；当前主要风险为侧车表生命周期缺口（dup、execve）与 bind TOCTOU，非典型 AB-BA 死锁。

---

## 7. 高优先级修复列表（当前）

| 优先级 | ID | 结构 | 问题 | 影响 |
|--------|-----|------|------|------|
| P1 | U-04 | FD_TABLE | dup/fcntl 不注册 unix 侧车表 | dup 后 AF_UNIX 专用 syscall 失效 |
| P1 | U-08 | FD_TABLE + SOCKET_FD | execve 杀线程未 drop_task | BOUND/FD_TABLE/registry 泄漏 |
| P1 | U-09 | BOUND + VFS | bind TOCTOU orphan mknod | pathname 永久 EADDRINUSE / 脏节点 |
| P1 | C-01 | TIMEX_STATE | 持锁 snapshot | 高并发 adjtimex 互斥延长（低概率卡死感） |

---

## 8. 附录：lock/unlock 等价 API 索引

### socket_fd.rs

- `SOCKET_FD_REGISTRY.lock()` — 全部 public API（约 10 处）

### unix_sock.rs

- `FD_TABLE.lock()` — `is_unix_fd`, `register`, `unregister`, `copy_fds_from_parent`, `drop_task`, `lookup_current`
- `BOUND.lock()` — `unregister`, `bind`, `listen`, `connect_stream`, `accept`, `sendto_unix`, `recvfrom_unix`, `pop_dgram_packet`, `deliver_dgram`, `poll_revents`
- `sock.inner.lock()` / `Arc<Mutex<UnixSockInner>>` — 几乎所有 unix 操作及 `VfsIoHandle` impl

### stat_times.rs

- `TIMES.lock()` — `set`, `apply_stat`, `apply_statx`

### sys/clock.rs

- `TIMEX_STATE.lock()` — `do_adjtimex` 唯一写/读快照路径

---

## 9. 审计结论

- **TIMES**：持锁闭环完整，无 sleep-on-lock；风险低（T-01 资源膨胀）。
- **TIMEX_STATE**：持锁闭环完整；`copy_to_user` 在锁外正确；C-01/C-02 为 P1 优化/语义项。
- **SOCKET_FD_REGISTRY**：exit/reap/fork/thread 覆盖完整；**execve 兄弟线程**与 dup 路径仍有缺口。
- **Unix socket 三锁**：Inner→BOUND 顺序一致；**bind 已移 VFS 出临界区**；**drop_task 与 thread clone 已接入**；剩余为 dup、execve 清理与 bind 竞态。

建议主 agent 将 U-04/U-08 并入 `docs/audits/lock-issues.md`；`lock-coverage.md` 标注 dup 与 execve 路径为「部分覆盖」。
