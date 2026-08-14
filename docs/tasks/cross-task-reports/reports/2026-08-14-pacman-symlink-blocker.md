# Arch RISC-V 软件包安装被符号链接阻断：问题汇报与修复建议

日期：2026-08-14
状态：默认 RW ext4 后端的 fast symlink 已完成基础持久化回归，extent symlink 已实现；
长链接与 pacman/neovim 端到端验收待执行。

## 摘要

WaterOS 当前可以运行静态构建的 `pacman`，可以通过 HTTPS 下载 Arch RISC-V 软件包，
也能创建普通文件、目录和硬链接。但是根文件系统不支持创建符号链接，导致任何含有
共享库别名、命令别名或 manpage 链接的软件包只能部分解压。动态程序因此通常不可运行。

这不是 pacman、镜像站或 Arch 软件包的问题；是 WaterOS 默认读写 ext4 后端
`impl-another-ext4` 缺少 `ReadWriteFs::symlink` 实现。VFS 将其默认的
`FsError::Unsupported` 映射成了错误的 `EINVAL`，掩盖了真实语义。

## 可复现证据

在 WaterOS 中执行：

```sh
mkdir -p /opt/archriscv/link-test
touch /opt/archriscv/link-test/target
ln -s target /opt/archriscv/link-test/symlink
# ln: .../symlink: Invalid argument

ln /opt/archriscv/link-test/target /opt/archriscv/link-test/hardlink
# 成功；ls -l 显示 target 和 hardlink 的链接计数均为 2
```

使用隔离 root 安装 neovim 时，pacman 继续完成事务，但反复报告：

```text
warning given when extracting .../usr/lib/libevent-2.1.so (Can't create ...)
warning given when extracting .../usr/lib/libluv.so.1 (Can't create ...)
```

随后：

```sh
archriscv-run /usr/bin/nvim
# /opt/archriscv/usr/bin/nvim: error while loading shared libraries:
# libluv.so.1: cannot open shared object file: No such file or directory
```

`libluv.so.1` 是应由包内符号链接提供的 SONAME 路径。该错误与安装警告完全一致。

## 已定位调用链

```text
musl ln -s
  -> symlinkat(2)
  -> syscall-impl/.../sys/fs/dir.rs: sys_symlinkat
  -> wateros-vfs/src/lib.rs: symlink_absolute
  -> vfs-impl/impl-fs-bridge/src/path_ops.rs: symlink_path
  -> ReadWriteFs::symlink
  -> impl-another-ext4
```

`sys_symlinkat` 的参数复制和 `target/linkpath` 顺序正确；VFS 路由也正确地调用
`symlink_path(link_path, target)`。问题在于 `AnotherExt4Fs` 没有覆盖
`ReadWriteFs::symlink`，因而调用 trait 的默认实现并返回 `FsError::Unsupported`。

当前 `vfs_error_to_errno` 中：

```rust
VfsError::InvalidPath | VfsError::Unsupported => ErrNo::EINVAL
```

所以用户态观察到 `EINVAL`。对于文件系统不支持的操作，Linux 兼容语义应为
`EOPNOTSUPP`；但本任务的目标是实现支持，而不是只修正错误码。

## 影响范围

- `pacman -S`：含符号链接的包被部分安装，动态库和命令别名可能缺失。
- 动态 ELF：SONAME 通常指向符号链接，例如 `libluv.so.1`。
- 现有 mGBA/Nano-X 和将来的 GUI/终端移植：只要使用动态库，都会受影响。
- 常规 POSIX 兼容性：`ln -s`、`readlink`、`lstat/statx`、路径跟随语义需要作为整体回归。

硬链接成功说明这不是目录权限、父目录解析、磁盘写入或 ext4 基础目录操作的普遍故障。

## 建议拆分任务

### 任务 A：another_ext4 创建 fast/extent symlink（P0）

责任范围：

- `os/vendor/another_ext4/src/ext4/low_level.rs`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/operations.rs`

实现要求：

1. 在 `another_ext4::Ext4` 提供窄的 `symlink(parent, name, target)` 操作。
2. 目标长度不超过 ext4 inode `i_block` 的 60 字节时使用 fast symlink；
   超过 60 字节时使用 extent 数据块。
3. 新 inode mode 必须为 `S_IFLNK | 0777`（`InodeMode::SOFTLINK | ALL_RWX`）。
4. fast 表示将原始字节写入 inode inline block 并保持 block count 为零；
   extent 表示分配数据块。两者都设置 inode size/checksum，再链接到 parent。
5. 任何目标都不得静默截断；分配失败应保留底层错误语义。
6. `AnotherExt4Fs::symlink` 负责路径存在性、父目录检查、缓存更新、`flush_all` 与
   `check_backend`。不应把实现散落在 syscall 层。

为什么允许修改 vendor：这里是默认 RW ext4 实现所依赖的第三方库缺少底层创建原语；
VFS 适配层无法在不破坏 ext4 inode 格式的前提下替代它。补丁应保持小而独立，并附带测试。

### 任务 B：VFS/ABI 错误码语义（P1，可与 A 并行）

责任范围：

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/vfs_util.rs`
- ABI errno 定义及已存在的其他 `Unsupported` 调用点。

要求：将常规文件系统 `Unsupported` 映射为 `EOPNOTSUPP`，不要与
`InvalidPath -> EINVAL` 合并。审查所有调用者，确认不会改变那些按 Linux 应返回
`EINVAL` 的 syscall 专用非法参数情形。

### 任务 C：符号链接解析/元数据回归（P0）

责任范围：VFS bridge、another-ext4 测试以及 QEMU 测试脚本。

验证项目：

```sh
ln -s target link
readlink link                 # target
test -L link
cat link                      # 跟随并读取 target
stat link                     # 跟随目标
stat -L link                  # 按现有工具语义核对
ln -s missing dangling
test -L dangling              # 成功
test -e dangling              # 失败
```

还须覆盖：已有目标返回 `EEXIST`、非目录 parent 返回 `ENOTDIR`、只读挂载返回
`EROFS`、循环链接返回 `ELOOP`，以及重启后仍可 `readlink`（持久化验证）。

### 任务 D：pacman 端到端验收（P0，依赖 A/C）

在新镜像上重新创建 `/opt/archriscv`，不要复用此前部分安装的根目录：

```sh
archriscv-pacman -S neovim
archriscv-run /usr/bin/nvim --version
```

验收标准：安装日志不再出现 `Can't create` 的符号链接警告；`libluv.so.1` 存在且为
正确 symlink；`nvim --version` 至少能完成动态加载。随后再评估 PTY/终端能力，不能把
交互式 neovim 无法使用误判为动态加载失败。

## `clear_child_tid` 警告的处置

```text
[exit] clear_child_tid write failed ... ErrNo(14)
```

该警告与 `libluv.so.1` 缺失没有直接因果关系：动态加载器已经报告缺库并退出，随后触发
线程退出清理。它应另立诊断任务：检查退出路径是否在销毁用户地址空间后才写
`clear_child_tid`。Linux 对该写入失败通常不应将退出变成失败；至少应避免把常见的
`EFAULT` 作为高优先级告警刷屏。此项不要与 symlink 修复合并，以免扩大风险面。

## 推荐合入顺序

```text
A（底层创建） + C（文件系统回归）
             -> B（错误码独立审查）
             -> D（pacman/neovim 端到端验证）
```

每项均应在镜像副本或 qcow2 overlay 上验证，并在写入验证后执行宿主侧
`e2fsck -fn`。不得通过 pacman 的 `--overwrite` 或在解包工具中把 symlink 改成普通文件来绕过内核问题。

## 2026-08-14 修复结果

- `another_ext4::Ext4::symlink` 现在创建 `S_IFLNK | 0777` 符号链接：不超过 60 字节时
  将目标原始字节写入 inode 的 `i_block`，清除 extent 标志并保持 block count 为零；
  较长目标通过 extent 数据块持久化。两种表示均写回 inode checksum，且不会截断目标。
- `AnotherExt4Fs::symlink` 完成存在性和父目录检查，成功后刷新后端、检查块设备错误状态，
  并发布 lookup cache。
- inode 回收会识别未设置 extents flag 的 fast symlink，不再把 `i_block` 内的目标
  字节误当 extent header 遍历，因而覆盖安装时可以正常删除旧链接。
- 通用 `VfsError::Unsupported` 改为映射到 `EOPNOTSUPP`；`pread`/`pwrite`、`flock`、
  `fcntl` 等已有 syscall 专用映射保持不变。
- RISC-V 镜像副本中已验证 `ln -s`、`readlink`、`test -L`、跟随读取、dangling link、
  `EEXIST` 和循环链接的 `ELOOP`；fast symlink 重启后再次读取通过。extent symlink 的运行
  与重启回归留给本轮集成测试。
  宿主 `debugfs` 确认链接 inode 为 mode `0777`、block count `0`，`e2fsck -fn` 五阶段通过。
- 尚未执行新 Arch RISC-V root 上的 `pacman -S neovim` 与 `nvim --version`，因此动态包
  端到端验收仍保留为后续项。
