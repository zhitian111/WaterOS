# 工作包：wateros-task + syscall — fork、execve、wait、exit 与进程树

**所属**：`os/components/wateros-task`、`wateros-syscall`、`wateros-mm`（exec 映像替换）。  
**并行度**：设计与 **pipe** 可部分重叠；**编码**应在 **open/read + mmap 最小集** 可用后进行。

## 要做什么

1. **`fork` 或 `clone` 最小子集**：创建子进程，复制 fd 表（**写时复制可选**，首版允许全量复制以降低风险）、复制地址空间或共享只读段策略须在 rustdoc 说明。
2. **`execve`**：从 **VFS 打开的可执行文件** 装载 ELF，替换用户地址空间；正确处理 `#!` 与脚本解释器可 **二期**（首版可先只支持 ELF）。
3. **`wait4`/`waitpid`**：回收僵尸、传递 exit code；与现有 task 的 **zombie/reap** 路径统一，避免两套语义。
4. **`exit`/`exit_group`**：与现实现一致或扩展为多线程预留。
5. **`chdir`/`getcwd`**：至少支持 **单根 ext4** 下的 cwd 串（可限制最大长度），供脚本 `cd` 使用。

## 验收要求

- [ ] bring-up：`fork` → 子进程 `execve("/bin/静态测例")` → 父进程 `waitpid` 得到 **约定 exit code**。
- [ ] 父子 **PID 不同**（若已实现 `getpid`）；若未实现，至少在日志打印内核分配的 task id。
- [ ] 子进程继承父进程 **已打开 fd** 的行为与 Linux 常见语义一致（dup 测例可放在 `wp-ipc` 或本包）。

## 验证方式

1. 根卷放置 **`/bringup/exec_child`**（或约定路径）静态 ELF；总线阶段 shell 式字符串日志 `[bringup][process] child exited=42`。
2. **不依赖** `self_tests`：全部由 `user_bringup_bus` 调度。
3. 失败分类：装载错误 `ENOEXEC`、文件不存在 `ENOENT`，与 ABI 表一致。

## 依赖

- **上游**：`wp-syscall-file-io.md`、`wp-syscall-mem-time.md`（exec 映射）、`wp-vfs-fd-session.md`。
- **下游**：`wp-ipc-pipe-signal.md`、`wp-ash-job-control.md`。

## 可并行对象

`wp-ipc-pipe-signal.md` 的数据结构设计与单元测试（内核态无用户 fork 的 pipe 自测）。
