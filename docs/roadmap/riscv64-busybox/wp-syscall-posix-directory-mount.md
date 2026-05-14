# 工作包：wateros-syscall + vfs/fs — 目录遍历、元数据与挂载语义（POSIX 子集）

**所属**：`wateros-syscall`、`wateros-vfs`、`wateros-fs`（ext4 目录项与挂载协议）。  
**并行度**：在 **`open/read/write`** 雏形稳定后可与 **内存/时间 syscall** 并行；**`mount`/`umount`** 与内核根卷策略强相关，需与 `fs-rootfs` 维护者对齐。

## 要做什么

1. **目录**：`getdents64` 或等价、`mkdir`、`rmdir`、`unlink`、`rename`（可先限制为 **同一目录内 rename**）。
2. **元数据**：`fstat`、`stat`/`lstat`/`newfstatat` 中与路径/ fd 相关的实现，返回 **BusyBox `ls -l`** 所需最小字段（mode、size、mtime 可简化）。
3. **挂载**：`mount`、`umount2` 的最小子集——至少支持 **已格式化的 ext4 块设备** 挂到指定挂载点（或仅赛题约定的 **一次根挂载 + bind 占位**）；与当前启动期 `mount_default_root` 不冲突，行为写清。
4. **`chdir`/`fchdir`**：若已在进程包实现 cwd，本包只接 **路径与错误码**；否则在本包实现并与进程包合并文档避免重复。

## 验收要求

- [ ] 用户程序：创建目录、创建文件、`getdents` 列出含 `.` / `..` 或项目约定行为，与 Linux 文档一致处须注明差异。
- [ ] `unlink` 后 `open` 返回 `ENOENT`。
- [ ] `busybox ls /` 或等价 C 测例列出根目录 **至少 N 个** 预定条目（N 与镜像版本绑定，写在测例常量中）。

## 验证方式

1. bring-up 总线：`[bringup][posix-fs-meta] PASS`，由静态 ELF 或极小程序执行，日志打印目录名列表摘要（避免刷屏可只打印 hash 或 count）。
2. 与 **`fs::test`** 顺序：若测例写根目录文件，须遵守 `main.rs` 关于 **RW 写盘前后** 与 ELF 视图一致性的注释。
3. **不**使用 `self_tests`。

## 依赖

- **上游**：`wp-syscall-file-io.md`、`wp-vfs-fd-session.md`。
- **下游**：`wp-syscall-process-exec.md`（脚本 cwd）、`wp-ash-job-control.md`（shell 内建依赖 `stat`/`test` 等）。

## 可并行对象

`wp-syscall-mem-time.md`；`wp-platform-driver-scaffold.md`（多盘为 mount 测例提供设备节点）。
