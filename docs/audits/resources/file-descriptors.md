# 文件描述符资源生命周期审计

> **分组**：`file-descriptors`  
> **覆盖资源**：#13 PerTaskFdRegistry、#14 FD flags/owner/ref_count、#15 VfsIoHandle、#16 PerTaskCwdRegistry  
> **生成时间**：2026-06-25  
> **搜索范围**：`wateros-vfs/**`、`wateros-syscall/impl-kernel` 中 `openat`/`close`/`dup`/`fcntl`/`clone`/`execve`/`exit` 路径  
> **Baseline**：单核多线程；对照 Linux `RLIMIT_NOFILE`/`EMFILE`/`EBADF`/`FD_CLOEXEC`/`dup`/`fork` 语义  
> **交叉参考**：[`syscall-issues.md`](../syscall-issues.md)、[`resource-inventory.md`](../resource-inventory.md)、[`lock-issues.md`](../lock-issues.md) FD-01

---

## 1. 资源总览

| # | 资源 | 主要类型 | 所属 crate | 硬上限 |
|---|------|---------|-----------|--------|
| 13 | Per-task FD 槽位 | `PerTaskFdRegistry.tables: BTreeMap<TaskId, Vec<Option<Box<dyn VfsIoHandle>>>>` | `vfs-impl-fd-session` | `RLIMIT_NOFILE` 默认 **1024** |
| 14 | FD 标志 / owner / 共享引用 | `fd_flags`、`owners`、`ref_counts` 三个 `BTreeMap` | 同上 | 与 fd 槽同阶 |
| 15 | VfsIoHandle | `Box<dyn VfsIoHandle>`（文件/目录/pipe/字符设备等） | `vfs-api` + 各 impl | 受 rlimit；页缓存文件另受 `open_refs` |
| 16 | Per-task CWD / exe / argv | `PerTaskCwdRegistry` 四表 + owner/ref | `vfs-impl-fd-session` | `PATH_MAX=256` |

**全局单例**：`wateros-vfs/src/fd.rs`、`wateros-vfs/src/cwd.rs` 经 `UniprocessorSafeCell` 暴露注册表。

---

## 2. 分配入口

### 2.1 PerTaskFdRegistry（#13–#14）

| 入口 | 文件 | 说明 |
|------|------|------|
| `ensure_task` / `table_mut` | `registry.rs:43` | 惰性初始化：预填 fd 0–2 为 stdin/stdout/stderr |
| `alloc_fd` / `alloc_fd_for_task` | `registry.rs:198,226` | `openat`、`socket`、`pipe2`、`accept` 等 |
| `dup_fd_for_task` / `dup3_fd_for_task` | `registry.rs:321,355` | `dup`/`dup3`/`fcntl(F_DUPFD*)` |
| `copy_fd_table_from_parent` | `registry.rs:461` | **fork** 路径：逐槽 `duplicate()` |
| `share_fd_table_from_parent` | `registry.rs:501` | **clone 线程**（`CLONE_FILES`）共享 owner 表 |
| `init_child_fd_table` | `registry.rs:456` | 仅默认 stdio；**当前无外部调用方**（spawn 依赖惰性 `ensure_task`） |

**前置依赖**：有效 `task::TaskId`；`alloc*` 前调用 `check_nofile_before_open`（`registry.rs:180`）。

### 2.2 VfsIoHandle（#15）

| 入口 | 文件 | 触发 syscall |
|------|------|-------------|
| `FsBridge::open` → `open_path` | `impl-fs-bridge/lib.rs` | `openat` |
| `pipe_handle_pair` | `handles.rs:228` | `pipe2` |
| `CharDevHandle` / 控制台句柄 | `char_dev_handle.rs`、`handles.rs` | 打开 `/dev/*`、默认 stdio |
| `PagedFileHandle::open` | `paged_handle.rs:96` | 普通大文件；`acquire_open_ref` |
| `BufferedFileHandle` | `file_handle.rs` | 小文件整文件读入堆 |
| `DirectoryHandle` / proc 句柄 | `dir_handle.rs`、`proc_handle.rs` | `O_DIRECTORY`、`/proc` |

### 2.3 PerTaskCwdRegistry（#16）

| 入口 | 文件 | 触发路径 |
|------|------|---------|
| `init_task_cwd` / `ensure_task_cwd` | `cwd.rs:56,69` | `on_user_task_spawned*`、首次路径解析 |
| `copy_cwd_from_parent` | `cwd.rs:96` | **fork**（`clone.rs:206`） |
| `share_cwd_from_parent` | `cwd.rs:117` | **clone 线程**（`clone.rs:267`） |
| `set_exe_path` / `set_argv` | `cwd.rs:128,140` | `execve`、spawn |

---

## 3. 回收入口

### 3.1 正常释放

| 事件 | 调用链 | 行为 |
|------|--------|------|
| `close(fd)` | `close.rs` → `vfs::fd::close_fd` → `take_fd_for_close` + `handle.close()` | 槽位置 `None`；pipe 关闭 endpoint；页缓存 `release_open_ref` |
| `dup3` 覆盖目标 fd | `dup3_fd_for_task` → `close_slot(newfd)` | 先关旧句柄再占位 |
| `execve` CLOEXEC | `execve.rs:104` → `close_cloexec_fds_for_current_task` | 逆序取 CLOEXEC 槽并 `close()` |
| `fcntl` 不涉及槽回收 | — | 仅改 flags / 分配新 fd |

### 3.2 任务/进程退出

| 事件 | 调用链 | 顺序 |
|------|--------|------|
| 线程 reap | `task.rs:770` `drop_task_runtime_resources_with_aspace` | shm → **cwd** → **fd** → socket_fd → unix_sock → cred |
| `waitpid` 收尾 | `task.rs:807` `drop_exited_task_resources` | 同上 |
| `execve` 杀兄弟线程 | `execve.rs:89-91` | 逐线程 `drop_task_cwd` + `drop_task_fd_table` |
| pseudo-shell 退出 | `pseudo-shell/lib.rs:201-203` | cwd + fd + cred |

**fd 表释放**：`vfs::fd::drop_task_fd_table` → `drain_task_fd_table` → 若 owner `ref_count==0` 则 `take_table_handles` + 逐句柄 `close()`（`fd.rs:178-185`）。

**cwd 释放**：`drop_task` → `release_owner`；末引用归零时移除 `cwd_tables`/`exe_paths`/`argv_vectors`（`cwd.rs:80`）。

### 3.3 Drop / 内部清理

- `VfsIoHandle` **无**统一 `Drop` 自动 `close`；依赖显式 `close()` 或 `drop_task_fd_table` 批量关闭。
- `PagedFileHandle`：`close()` 内 `sync_dirty` + `release_open_ref`；**`sync_dirty` 失败时不调用 `release_open_ref`**（`paged_handle.rs:344-347`）。
- `registry.close_table`（`registry.rs:168`）仅 `take` 句柄、**不**调 `close()`；仅由未使用的内部路径引用；生产路径走 `vfs::fd::drop_task_fd_table`。

---

## 4. 生命周期状态机

### 4.1 FD 槽位 + 句柄

```text
[未分配] --ensure_task/spawn 惰性--> [槽位存在，0-2 预填 stdio]
[空槽 None] --alloc_fd--> [已占用 Some(handle)]
[已占用] --read/write/ioctl--> [使用中]（共享表时多 TaskId 指向同一 owner 表）
[已占用] --close/take_fd_for_close--> [空槽] + handle.close()
[空槽] --drop_task(ref==0)--> [表项移除] + 所有句柄 close
```

**半初始化状态**：

1. `openat`：`alloc_fd` 成功但 `set_fd_flags`/`set_path_only_fd` 失败 → 槽已占用、syscall 向用户返回错误（**泄漏**，见 §6）。
2. `pipe2`：第二 fd 分配失败会回滚第一 fd（`pipe2.rs:37`）；`copy_to_user` 失败**不回滚**（§6）。
3. `dup3`：覆盖路径先 `close_slot` 再写入，原子性在单核下可接受。

### 4.2 FD 表共享（fork vs clone 线程）

| 场景 | owner 模型 | ref_count |
|------|-----------|-----------|
| 独立任务（fork/spawn） | 子任务自有 owner | 各 1 |
| `CLONE_FILES` 线程 | 子 `owners[child]=parent_owner` | 父 owner +1 |
| 末线程 `drop_task` | `release_owner` 减计数 | 0 时物理移除表并 close 全部 fd |

### 4.3 CWD

```text
[无记录] --init/ensure--> ["/" 或设定路径]
[fork] --copy_cwd_from_parent--> 子 owner 独立副本
[CLONE_VM 线程] --share_cwd--> 共享 owner 字符串
[drop_task ref==0] --> 移除 cwd/exe/argv
```

`get_cwd` 在表项缺失时回退 `"/"`（`cwd.rs:61-66`），**已 drop 的任务 id 读 cwd 不会报错**，可能掩盖测试中的 use-after-registry 问题（P2）。

### 4.4 VfsIoHandle（页缓存文件）

```text
open --> acquire_open_ref (+1)
dup/fork duplicate --> 新句柄再 acquire_open_ref (+1)
close 成功 --> release_open_ref (-1) --> 归零 purge 页缓存元数据
close 失败（sync_dirty）--> open_ref 不释放（§6 P0）
```

---

## 5. 账本稳定性结论

| 资源 | 结论 | 说明 |
|------|------|------|
| #13 FD 槽位 | **部分稳定** | 正常 close/exit 成对；错误路径 partial alloc 有泄漏；rlimit 检查覆盖 alloc/dup |
| #14 flags/owner/ref | **部分稳定** | fork 复制 flags；dup 新槽 flags=0；共享表 refcount 与 drop 逻辑一致 |
| #15 VfsIoHandle | **部分稳定** | 页缓存 `open_ref` 在 close 失败时泄漏；fork/dup 为句柄级 Clone 非 Linux file description |
| #16 CWD | **稳定** | fork/copy/share/drop 闭环；PATH_MAX 截断时 fork 回退 `/` |

**整体**：**部分稳定** — 主路径（open → close → exit）可用；错误回滚与页缓存配对是主要风险。

---

## 6. 耗尽与失败处理

| 场景 | 当前行为 | Linux 对照 | 差距 |
|------|---------|-----------|------|
| 打开数 ≥ rlimit | `VfsError::TooManyOpenFiles` → `-EMFILE` | `EMFILE` | 一致 |
| 无进程上下文 | `VfsError::NoTask` | — | 内核内部错误 |
| 坏 fd | `BadFd` → `-EBADF` | `EBADF` | 一致 |
| `dup` 触顶 | 同 `check_nofile_before_open` | `EMFILE` | 一致 |
| `dup3` 覆盖已开 fd | 不计入新增打开数 | 一致 | 一致 |
| spawn 未调 `init_child_fd_table` | 首次 fd 操作惰性建表 | — | 可接受 |
| `setrlimit(NOFILE)` | `task.rs` 默认 1024；`nofile_rlimit_for_task` 读取 | 可配置 | 硬编码默认，可改 rlimit |

**禁止路径**：未发现 fd 耗尽后静默截断或 panic；`with_current_io` 在共享 fd 表上返回 `Unsupported`（已收敛 FD-01）。

---

## 7. 跨资源耦合

| 耦合 | 说明 |
|------|------|
| **页缓存 #17–18** | `PagedFileHandle` open/dup/close 驱动 `acquire_open_ref`/`release_open_ref`；close 失败泄漏 open_ref |
| **socket_fd #28** | `dup`/`fcntl`/`close` 同步旁路表；fork `copy_from_parent`、线程 `share_from_parent` |
| **unix_sock #29** | `close` 仅 `unregister` 当前 task_id；`CLONE_FILES` 共享 fd 表时兄弟线程表项可能残留（P1） |
| **pipe #20** | `duplicate` 共享 `Arc<PipeEndpoint>`；close 递减端点引用 |
| **fork 地址空间 #4** | `clone.rs` 先 `fork_user_aspace` 再复制 fd/cwd；aspace 失败则不复制 fd |
| **execve** | 杀兄弟线程 → drop 其 fd/cwd；当前任务 `close_cloexec`；**不**重置非 CLOEXEC fd |
| **shm** | `drop_task_attachments` 在 fd drop **之前**（`task.rs:771`） |

---

## 8. Syscall 路径摘要

### 8.1 `openat`（`openat.rs`）

1. 路径解析 → `prepare_open_path`（symlink follow / `O_NOFOLLOW`）
2. `backend.open` → `alloc_fd`
3. 可选 `set_fd_flags(O_CLOEXEC)`、`set_path_only_fd(O_PATH)`
4. **缺口**：步骤 3 失败未 `close_fd` 回滚（FD-P0-01）

### 8.2 `close`（`close.rs`）

1. 记录是否 socket/unix
2. `vfs::fd::close_fd`（取句柄 + `close()`）
3. `socket_fd::remove`、`unix_sock::unregister`

### 8.3 `dup` / `dup3`（`dup.rs`）

1. VFS 层 dup/duplicate 句柄
2. socket 旁路表 `register_with_flags`；`dup3` 覆盖时 `remove` 旧 socket 项

### 8.4 `fcntl`（`fcntl.rs`）

- `F_GETFD`/`F_SETFD`：registry `fd_flags`（仅 `FD_CLOEXEC`）
- `F_GETFL`/`F_SETFL`：`with_current_io` → `open_status_flags`；socket 走旁路表
- **共享 fd 表**：`with_current_io` → `-ENOSYS`/`Unsupported`（FD-P1-03）

### 8.5 `fork` / `clone`（`clone.rs`）

- **fork**：`copy_cwd_from_parent` + `copy_fd_table_from_parent`（逐 fd `duplicate()`，**失败静默跳过**）
- **线程**：`share_*_from_parent`；unix_sock 仍 `copy_fds_from_parent`（非 share）

### 8.6 `exit` / 线程 reap（`task.rs`）

- `drop_task_runtime_resources_with_aspace` 统一回收 cwd、fd 表、socket 注册

---

## 9. 潜在问题列表

### P0（泄漏 / 账本破坏 / 可累积耗尽）

| ID | 资源 | 问题 | 位置 |
|----|------|------|------|
| **FD-P0-01** | #13 | `openat`/`open_tmpfile` 在 `alloc_fd` 成功后，若 `set_fd_flags(O_CLOEXEC)` 或 `set_path_only_fd` 失败，**不向用户返回 fd 号且未 close 回滚**，槽位与 `VfsIoHandle` 泄漏直至任务退出 | `openat.rs:85-99,131-140` |
| **FD-P0-02** | #13/#15 | `pipe2` 在 `copy_to_user` 失败时返回 `-EFAULT`，**已分配的 read/write fd 未关闭** | `pipe2.rs:58-66` |
| **FD-P0-03** | #15/#18 | `PagedFileHandle::close` 在 `sync_dirty()` 失败时**提前返回**，不调用 `release_open_ref`；`close_fd` 仍丢弃句柄 → **页缓存 open_ref 永久泄漏**，元数据无法 purge | `paged_handle.rs:344-347`、`fd.rs:79-86` |

### P1（语义偏差 / 部分回滚 / 共享表）

| ID | 资源 | 问题 | 位置 |
|----|------|------|------|
| **FD-P1-01** | #13/#15 | `copy_fd_table_from_parent` 中 `duplicate()` 失败时**静默跳过**该 fd，子进程 fd 表不完整且无错误码 | `registry.rs:472-477` |
| **FD-P1-02** | #15 | fork/dup 为句柄级 `Clone`（独立 `offset`），非 Linux **共享 file description**；父子/	dup 后 seek 不同步 | `paged_handle.rs:442`、`file_handle.rs:239` |
| **FD-P1-03** | #13/#14 | `CLONE_FILES` 共享 fd 表时 `fcntl(F_SETFL)`/`with_current_io` 返回 `Unsupported` | `fd.rs:50-54`、`fcntl.rs:91-96` |
| **FD-P1-04** | #13 + unix_sock | 共享 fd 表下 `close` 清空公共槽，但 `unix_sock::unregister` 仅移除**当前线程**表项，兄弟线程 `(tid,fd)` 项可能残留 | `close.rs:17-20`、`unix_sock.rs:80-90` |
| **FD-P1-05** | #14 | `close_cloexec_fds_for_task`（registry 内部）用 `let _ = close_slot`，**忽略 close 错误** | `registry.rs:518-522` |
| **FD-P1-06** | #16 | fork 时父 cwd 长度 ≥ `PATH_MAX` 静默回退子 cwd 为 `/` | `cwd.rs:104-106` |

### P2（文档 / 测试 / 性能）

| ID | 问题 |
|----|------|
| **FD-P2-01** | `init_child_fd_table` 无调用方；spawn 完全依赖惰性 `ensure_task` |
| **FD-P2-02** | `BufferedFileHandle` fork/dup 复制整文件 `Vec` 到堆，大文件多 fd 时内存放大（与页缓存路径无关） |
| **FD-P2-03** | `get_cwd` 对已 drop 任务回退 `"/"`，不利于检测 registry 泄漏 |
| **FD-P2-04** | 与 [`syscall-issues.md`](../syscall-issues.md) P0-16：LTP fast-exit 旁路 `openat` 入口，与真实 fd 生命周期无关但影响测试 |

---

## 10. 收敛建议

对 **FD-P0-01/02**：在错误返回前调用 `vfs::fd::close_fd(fd)` 或 `close_fd_for_task`；`warn!` 含 `task_id`、`fd`、操作名。

对 **FD-P0-03**：`close()` 使用 `let _ = self.release_open_ref_if_held()` 放在 `sync_dirty` 之后且**无论 sync 成败**执行；或 `Drop for PagedFileHandle` 保底 release（仍应先 sync）。

对 **FD-P1-01**：`duplicate` 失败时 `warn!` + 向 fork 返回 `-EMFILE`/`-ENOMEM` 或中止子进程创建；不应静默子表缺失。

对 **FD-P1-03/04**：长期改 fd 表为 `Arc`+内部锁或 per-fd 引用；短期文档标注 `CLONE_FILES`+unix socket 限制。

**warn 格式**（与 syscall 审计一致）：

```text
warn!("[vfs-fd] {op} failed: {detail} task_id={:?} fd={} used={}/{}",
      task_id, fd, used, limit);
```

---

## 11. 修复任务草案

| 优先级 | 标题 | 文件 | 验收标准 |
|--------|------|------|---------|
| P0 | openat 标志设置失败回滚 fd | `openat.rs` | `O_CLOEXEC`/`O_PATH` 任一步失败时槽位恢复为空；无泄漏；`self_test` 或单元测模拟失败 |
| P0 | pipe2 EFAULT 回滚 fd 对 | `pipe2.rs` | `copy_to_user` 失败时两个 fd 均已 close；重复 pipe2 不因泄漏触顶 EMFILE |
| P0 | PagedFileHandle close 保证 release_open_ref | `paged_handle.rs` | `sync_dirty` 失败时 `open_refs` 仍递减；页缓存 `release_open_ref_purges_when_last_handle_closes` 类测试通过 |
| P1 | fork duplicate 失败可观测 | `registry.rs`、`clone.rs` | 失败打 warn；可选 fork 整体失败返回 `-ENOMEM` |
| P1 | 共享表 close 同步 unix_sock | `close.rs`、`unix_sock.rs` | `CLONE_FILES` 下 close 清除 owner 下所有线程的 unix 表项 |
| P2 | 调用 spawn 时 `init_child_fd_table` | `user_bringup_common.rs` 等 | 首 syscall 前 fd 0–2 行为确定 |

---

## 12. 与 syscall/锁审计交叉项

| 外部 ID | 关联 |
|---------|------|
| IO-P1-03/04 | `fcntl` GETFL/SETFL 已收敛至 `VfsIoHandle`（非共享表） |
| IO-P1-05/06 | `dup3(fd,fd)`、`pipe2 O_CLOEXEC` 已收敛 |
| P0-13 | `openat` symlink follow 已实现；本审计覆盖 follow 后 open 的 fd 生命周期 |
| FD-01 / lock-issues | 共享 fd 表 `with_current_io` 拒绝 take-restore；本审计 FD-P1-03 为语义后果 |
| sockets 组 #28–29 | socket/unix 旁路表与 fd 槽位生命周期需联合修复 FD-P1-04 |

---

## 13. 审计摘要

- **主路径**（open → close → exit/fork 复制）结构清晰，`RLIMIT_NOFILE` 检查覆盖 alloc/dup，`drop_task_fd_table` 在末引用时批量 `close()`。
- **主要风险**集中在 **错误路径未回滚 fd 槽**（openat、pipe2）与 **页缓存 open_ref 与 close 失败不对齐**（可累积导致缓存元数据泄漏）。
- **Linux 语义差距**：file description 级共享（offset、锁）未实现；`CLONE_FILES` 与 `fcntl`/unix_sock 仍有缺口。
- **建议优先落地**：§11 中三项 P0 修复，再处理 fork 静默丢 fd（P1）。
