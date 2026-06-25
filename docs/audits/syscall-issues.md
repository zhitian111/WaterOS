# 系统调用潜在问题清单（文档 A）

> **生成时间**：2026-06-25  
> **Baseline**：Linux asm-generic 64 位 syscall 语义  
> **事实来源**：`docs/audits/syscall/*.md`（5 组 subagent 审计）  
> **范围**：`wateros-syscall/impl-kernel` 已注册/已路由的全部 syscall（146 `sys_*` + `rt_sigreturn` trap 路径）

---

## 1. 审计摘要

| 严重度 | 数量（去重后） | 典型表现 |
|--------|----------------|----------|
| **P0** | 18 | 永久阻塞、内核 panic、exec/clone 不可恢复语义错误 |
| **P1** | 42+ | 与 Linux 明显不一致、误导性成功、有界阻塞替代真阻塞 |
| **P2** | 30+ | stub/no-op、文档偏差、测试旁路 |

**最高危根因（与测例卡死相关）**：

1. **进程模型**：非 leader `fork` + `waitpid` 唤醒队列错位（`process-exec.md` §2.3/§2.6）
2. **futex**：`WAIT` 与 `WAKE` key/bitset 不一致 → 永久睡眠（`signal-socket-poll.md` §3.1）
3. **VFS 锁序**：`fsync`/页缓存与 ext4 并发 → 长时间自旋似卡死（`vfs-path.md` §2.1）
4. **execve 失败路径**：加载前已杀兄弟线程，原映像不可恢复（`process-exec.md` §2.5）
5. **无用户地址空间时 mmap 族 panic**（`mm-time-cred.md` §G30）

---

## 2. P0 问题清单

| ID | Syscall / 组合 | 问题 | 建议收敛 |
|----|----------------|------|----------|
| **P0-01** | `clone`/`fork` + `waitpid` | 非 leader 线程 fork 时子进程 `parent_id` 指向调用线程，`waitpid` 在 leader 的 `ChildExit` 队列睡眠 → **父永久阻塞** | 仅 leader 允许 fork（`-EPERM`）；或统一以 leader 为 wait 键 |
| **P0-02** | `clone`/`clone3` flags | `CLONE_VFORK`、`CLONE_VM` 单独出现等大量 flag **未校验即走 fork**，共享/阻塞语义错误 | fork 路径 `SUPPORTED_FORK_FLAGS` 白名单；其余 `warn!` + `-EINVAL` |
| **P0-03** | `execve` | `terminate_other_threads_for_exec()` 在 `load_program_from_path` **之前**；加载失败时兄弟线程已销毁，**原映像不可恢复** | 将杀线程移到加载成功之后 |
| **P0-04** | `mmap`/`munmap`/`mprotect`/`mremap` | `user_aspace_ptr==0` 时 `syscall_unsupported` → **内核 panic** | `warn!` + `-ENOSYS` |
| **P0-05** | `MmError::Unsupported` | 经 `mm_err_to_errno` → **panic** | 映射为 `-EINVAL`/`-ENOSYS` |
| **P0-06** | `getgroups` | 负 size、空指针、copy 失败 → **panic** | `-EFAULT`/`-EINVAL` |
| **P0-07** | `syslog` | `len>0` 且 `buf==NULL` → **panic** | `-EFAULT` |
| **P0-08** | `futex` `FUTEX_WAIT` | 无超时 + wake **key 不一致**（private/shared、bitset 混用）→ **永久睡眠** | 文档化约定；wake 失败 `warn`；实现 bitset 或拒绝非全 bitset |
| **P0-09** | `futex` `WAIT_BITSET`/`WAKE_BITSET` | **`bitset` 参数被忽略** | 非 `0xffffffff` 时 `warn` + `-ENOSYS` |
| **P0-10** | `get_robust_list` | **ABI 2 参数**，Linux 为 3 参数 `(pid, head**, len*)` → libc 调用错乱 | 修正 ABI 或 `warn` + `-ENOSYS` |
| **P0-11** | `rt_sigsuspend` | 依赖 `interrupt_task` 打断 waitqueue；信号 pending 但未 interrupt 时 **长时间睡眠** | 所有 `send_*` 路径保证 interrupt |
| **P0-12** | `ppoll`/`pselect6` | **`sigmask` 未应用**（仅校验指针） | ~~实现原子 mask~~ → **2026-06-25 已实现** `begin_poll_sigmask` |
| **P0-13** | `openat` | **不 follow symlink** → `-EISDIR`，与 Linux 严重不符 | ~~中期实现 follow~~ → **2026-06-25 已实现** `resolve_final_symlink`；`O_NOFOLLOW` → `-ELOOP` |
| **P0-14** | `fsync`/`fdatasync`/`sync` + 并发 I/O | 页缓存 flush 与 ext4 **锁序**风险 → 用户态永久阻塞 | `O_SYNC`/`O_DSYNC` 于 `openat` 拒绝；flush 失败 `warn`；**锁序仍待审计** |
| **P0-15** | `mount` | 块设备路径 **同步重 I/O 无 yield**；`umount2` 忽略 flags | 拒绝未支持 `MS_*` flag；`warn` + `-EINVAL` |
| **P0-16** | LTP `cgroup_regression_loop_fast_exit` | `openat`/`mount`/`mkdirat`/`unlinkat` 入口子进程 **直接 exit(0)**，篡改 wait 语义 | feature 门控；默认走真实 syscall（**暂缓**） |
| **P0-17** | `connect` | 阻塞循环 **无 EINTR**；256 tick 后 `ETIMEDOUT`（非 Linux） | ~~实现 EINTR~~ → **2026-06-25** 真阻塞 + `EINTR` |
| **P0-18** | AF_UNIX `accept` | stream socket **真无限 sleep**（与 INET 有界行为不一致） | ~~统一超时~~ → **2026-06-25** 真阻塞 + `EINTR`（与 INET 对齐） |

---

## 3. P1 问题清单（节选）

### 3.1 I/O 与 fd（详见 `syscall/io-fd.md`）

| ID | 问题 |
|----|------|
| IO-P1-01 | 阻塞 socket `read` 4096 tick 后 `EAGAIN`（非 Linux 无限阻塞） | **已收敛**：真阻塞 + `EINTR` |
| IO-P1-02 | 阻塞 socket `write` 128 tick 后 `EAGAIN` | **已收敛**：真阻塞 + `EINTR` |
| IO-P1-03 | `fcntl(F_SETFL)` 对 pipe/VFS fd **O_NONBLOCK 静默 no-op** |
| IO-P1-04 | `fcntl(F_GETFL)` 非 socket 返回固定 `O_RDWR` |
| IO-P1-05 | `dup3(fd, fd, 0)` 返回 `-EINVAL`（Linux 应成功） |
| IO-P1-06 | `pipe2` 不支持 `O_CLOEXEC` |
| IO-P1-07 | TTY 无数据时 `read` → `EINVAL`（应为 `EAGAIN` 或阻塞） |
| IO-P1-08 | `sendfile` 不经 socket 旁路 |
| IO-P1-09 | `ioctl` 大量 request → `ENOTTY` 无 warn |

### 3.2 VFS / 路径（详见 `syscall/vfs-path.md`）

| ID | 问题 |
|----|------|
| VFS-P1-01 | `openat` 忽略 `O_EXCL`/`O_NOFOLLOW`/`O_NONBLOCK`/`O_SYNC` 等 | `O_NOFOLLOW`/`O_SYNC`/`O_DSYNC` 已处理；`O_EXCL` 等待 |
| VFS-P1-02 | `st_uid`/`st_gid` 恒 0；时间戳除 `utimensat` 旁路表外为 0 |
| VFS-P1-03 | `statfs` 硬编码假容量 |
| VFS-P1-04 | `faccessat`/`faccessat2` 无 owner 匹配；`AT_SYMLINK_NOFOLLOW` 未生效 |
| VFS-P1-05 | `renameat2` 拒绝所有非零 flags |
| VFS-P1-06 | `umount2` 忽略 flags，无繁忙检测 |
| VFS-P1-07 | `utimensat` 仅内存旁路，不持久化 |
| VFS-P1-08 | `getcwd` 内核缓冲 256 字节 |
| VFS-P1-09 | `fdatasync` 与 `fsync` 同路径 |
| VFS-P1-10 | `fallocate` `KEEP_SIZE` 扩展时 `-EOPNOTSUPP` |

### 3.3 进程 / 执行（详见 `syscall/process-exec.md`）

| ID | 问题 |
|----|------|
| PROC-P1-01 | `wait4` 忽略 `rusage`；`pid==0`/`<-1` 直接 `-EINVAL` |
| PROC-P1-02 | `setsid`/`setpgid` stub 却返回成功 |
| PROC-P1-03 | `kill` 不支持 `pid<=0` 进程组语义 |
| PROC-P1-04 | `sched_setattr`/`getattr`(274/275) 仅 `dispatch_unknown` 旁路 |
| PROC-P1-05 | `execve` argv/envp EFAULT **静默截断** |
| PROC-P1-06 | `compat_exec_load_path` 硬编码 busybox 重定向 |

### 3.4 内存 / 时间 / 身份（详见 `syscall/mm-time-cred.md`）

| ID | 问题 |
|----|------|
| MM-P1-01 | `msync`/`madvise`/`mlock*` 校验后 no-op 成功 |
| MM-P1-02 | `brk` 扩页失败返回当前 break 作成功值，无 `ENOMEM` |
| MM-P1-03 | `clock_settime` 无 `CAP_SYS_TIME` |
| MM-P1-04 | cred `set*id` 无权限模型 |
| MM-P1-05 | `shmctl` 仅 `IPC_RMID` |
| MM-P1-06 | `getrandom` 伪随机 |
| MM-P1-07 | 导出文档写 LoongArch mmap `ENOSYS` 与代码不符（实际 panic） |

### 3.5 信号 / socket / poll（详见 `syscall/signal-socket-poll.md`）

| ID | 问题 |
|----|------|
| SIG-P1-01 | `accept` 阻塞有 tick 上限 → `EAGAIN` | **已收敛**：真阻塞 + `EINTR` |
| SIG-P1-02 | `recvfrom` TCP 超时返回 `EINTR`（Linux 通常 `EAGAIN`） |
| SIG-P1-03 | `robust_exit_cleanup` 仅 private key wake |
| SIG-P1-04 | socket `poll` 靠 1-tick sleep 轮询 |
| SIG-P1-05 | `rt_sigaction` 不读用户 `restorer` |

---

## 4. 高优先级收敛列表（统一风格）

对确认未完整支持的路径：**入口判断 → `warn!` → 明确错误返回**。

### 4.1 warn 格式约定

```text
warn!("[syscall] {name}(nr={nr}) unsupported: {detail} args=[{:#x},{:#x},{:#x},{:#x},{:#x},{:#x}]",
      name, nr, a0, a1, a2, a3, a4, a5);
```

`{detail}` 应包含：flag 名、cmd/op 名、或「reason」短句。

### 4.2 错误码约定

| 场景 | 错误码 |
|------|--------|
| 未实现的 syscall 号 / op | `-ENOSYS` |
| 未支持的 flag/参数组合 | `-EINVAL` |
| 功能已知但刻意不做 | `-EOPNOTSUPP` |
| 需要权限 | `-EPERM` |
| 用户指针非法 | `-EFAULT` |
| 路径/资源不存在 | `-ENOENT` |
| 阻塞资源暂不可用（收敛后） | `-EAGAIN` |
| 不应对用户返回的路径（原 panic） | 改为上表对应码，**禁止 panic** |

### 4.3 建议优先落地的收敛（按测例卡死风险排序）

| 优先级 | syscall | 条件 | 动作 |
|--------|---------|------|------|
| 1 | `mmap` 族 | `user_aspace_ptr==0` | `warn` + `-ENOSYS`（替换 panic） |
| 2 | `clone` | fork 路径 flags ∉ 白名单 | `warn` + `-EINVAL` |
| 3 | `fork` | 调用者非 leader | `warn` + `-EPERM` |
| 4 | `execve` | 调整线程终止顺序 | 代码重构（见 P0-03） |
| 5 | `futex` | `bitset != 0xffffffff` | `warn` + `-ENOSYS` |
| 6 | `get_robust_list` | 任意调用 | 修正 3 参数 ABI 或 `-ENOSYS` |
| 7 | `openat` | 目标为 symlink | follow（`path_at::resolve_final_symlink`）；`O_NOFOLLOW` → `-ELOOP` |
| 8 | `mount` | `MS_BIND`/`MS_SHARED` 等 | `warn` + `-EINVAL` |
| 9 | `getgroups`/`syslog` | 非法参数 | 错误码替代 panic |
| 10 | LTP fast-exit | 非测试构建 | 禁用旁路（**暂缓**） |

---

## 5. 测试旁路与生产语义冲突

以下路径在 bring-up/LTP 中**故意偏离** Linux 语义，审计时需与生产路径区分：

| 机制 | 文件 | 影响 syscall |
|------|------|-------------|
| `cgroup_regression_loop_fast_exit_if_standalone` | `ltp_cgroup_helper.rs` | `openat`, `mount`, `mkdirat`, `unlinkat` |
| `ltp_fuzz_sigsuspend_worker_fast_exit_if_standalone` | `ltp_cgroup_helper.rs` | `rt_sigsuspend` |
| `compat_exec_load_path` | `execve.rs` | `execve` 路径重定向 |

**建议**：`#[cfg(feature = "ltp-compat")]` 或运行时开关；默认关闭。

---

## 6. 详细审计文档索引

| 文档 | 覆盖组 |
|------|--------|
| [`syscall/io-fd.md`](syscall/io-fd.md) | G01–G07 + sendfile |
| [`syscall/vfs-path.md`](syscall/vfs-path.md) | G08–G20 |
| [`syscall/process-exec.md`](syscall/process-exec.md) | G21–G28 |
| [`syscall/mm-time-cred.md`](syscall/mm-time-cred.md) | G29–G38 |
| [`syscall/signal-socket-poll.md`](syscall/signal-socket-poll.md) | G39–G46 |

---

## 7. 后续行动

- [x] **2026-06-25 首轮收敛**（见 §7.1）
- [x] **2026-06-25 第二轮收敛**（见 §7.2；LTP fast-exit **未动**）
- [ ] VFS 页缓存/ext4 锁序（P0-14 剩余）
- [ ] 将未收敛 P0 回填 `docs/roadmap/todolist.md`
- [x] 同步 `docs/exports/features/wateros-syscall.md`
- [ ] 与锁机制审计交叉验证 VFS-P0-01

### 7.1 已收敛（2026-06-25 代码落地）

| 原 ID | syscall | 修改文件 | 行为 |
|-------|---------|----------|------|
| P0-04 | `mmap` 族 | `mm_util.rs`, `mmap.rs` | 无 aspace → `warn` + `-ENOSYS` |
| P0-05 | mm 通用 | `mm_util.rs` | `Unsupported` → `-ENOSYS` |
| P0-02 | `clone` | `clone.rs` | fork flags 白名单（`CSIGNAL`） |
| P0-01 | `clone` | `clone.rs` | 非 leader fork → `-EPERM` |
| P0-03 | `execve` | `execve.rs` | 加载成功后再杀兄弟线程 |
| P0-08/09 | `futex` | `futex.rs` | 非全 bitset → `-ENOSYS` |
| P0-10 | `get_robust_list` | `robust.rs` | 三参数 ABI |
| P0-06 | `getgroups` | `cred.rs` | panic → `-EINVAL`/`-EFAULT` |
| P0-07 | `syslog` | `syslog.rs` | panic → `-EFAULT` |
| P0-15 | `mount` | `mount.rs` | 传播类 `MS_*` → `-EINVAL` |

### 7.2 已收敛（2026-06-25 第二轮；LTP 旁路未改）

| 原 ID | syscall | 修改文件 | 行为 |
|-------|---------|----------|------|
| P0-12 | `ppoll`/`pselect6` | `ipc-signal`, `poll_engine.rs`, `poll_multiplex.rs` | 阻塞期间临时替换线程 sigmask，`Drop` 恢复 |
| P0-13 | `openat` | `path_at.rs`, `openat.rs` | follow 末端 symlink；`O_NOFOLLOW` → `-ELOOP` |
| P0-14（部分） | `openat`/`fsync` | `openat.rs`, `sync.rs` | `O_SYNC`/`O_DSYNC` → `-EINVAL`；flush 失败 `warn` |
| P0-17 | `connect` | `connect.rs`, `socket_block.rs` | 真阻塞等待；信号可中断 → `EINTR` |
| P0-18 | `accept`（INET/UNIX） | `accept.rs`, `unix_sock.rs`, `socket_block.rs` | 真阻塞 + `EINTR`；去除 tick 上限假 `EAGAIN` |
| IO-P1-01/02 | `read`/`write` socket | `read.rs`, `write.rs` | 同上 |
| SIG-P1-01 | `accept` | `accept.rs` | 同上 |
