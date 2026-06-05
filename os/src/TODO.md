# WaterOS Bring-up TODO

本文档记录 `os/src/user_bringup_busybox.rs` 当前启用脚本暴露出的内核侧缺口。
来源：`os/run.log` 中 `busybox-glibc` 组。

## busybox-glibc

### P0：用户态页错误

- [ ] 排查 `busybox grep hello busybox_cmd.txt` 触发的 `LoadPageFault`。
  - 现象：用户任务在 `sepc=0x1201d0f00`、`stval=0x7fffff4ed8` 被 kill。
  - 方向：优先确认 `grep` 的用户栈/argv/envp/iovec 或 mmap 区域是否被提前释放或未映射；这不是 `unknown syscall`，而是用户内存访问路径问题。

### P1：文件/目录语义

- [x] 完善 `renameat2` 的目录 rename 支持（同父目录、`flags=0`）。
  - 已实现：VFS/FS `rename` + ext4 link/unlink；`sys/renameat2` 经 `vfs::rename_absolute`。
  - 剩余限制：跨目录 rename、覆盖已有目标、`RENAME_*` flags、journal 原子语义。

- [ ] 处理 busybox 文件测试残留目录。
  - 现象：`mkdir test_dir` 报 `File exists`。
  - 方向：优先确认是否由上次运行残留导致；必要时在测试前刷新镜像或在脚本/bring-up 中清理 `test_dir`。

### P1：标准输入/管道兼容

- [ ] 评估非交互 stdin 的 EOF 语义。
  - 现象：`busybox od test.txt` 打印文件内容后仍报 `standard input: Bad file descriptor`。
  - 当前限制：`ConsoleInHandle::read` 返回 `BadFd`。
  - 方向：对于无真实输入的 bring-up 场景，可考虑 stdin read 返回 `Ok(0)` 表示 EOF，提升 busybox 过滤器类 applet 兼容性。

### P2：伪文件系统与设备 stub

- [ ] 补最小 `/proc`。
  - 现象：`ps` 无法打开 `/proc`，`free` 无法打开 `/proc/meminfo`。
  - 方向：先提供 `/proc` 目录、`/proc/meminfo`、必要的进程目录或空目录行为，让 busybox 探测类命令可降级通过。

- [ ] 补最小 RTC 设备节点。
  - 现象：`hwclock` 无法打开 `/dev/misc/rtc`。
  - 方向：可先提供只读/返回固定时间或 `ENOSYS` 语义明确的 devfs stub。

### P2：环境与路径

- [ ] 排查 `busybox which ls` 失败。
  - 方向：检查脚本中的 `PATH`、busybox applet 安装方式，以及 `access/openat` 对相对路径和可执行权限的处理。
