# RIO-01：读取访问模式与错误顺序

## 任务目标

让 `read(2)` 在不触碰底层数据的情况下先完成 fd、`O_PATH`、访问模式和对象类型校验，
修正零长度请求及多个错误同时存在时的 errno。此任务不实现读取租约，也不改 MM。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-syscall.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/exports/public-api/wateros-syscall.md`
- `docs/exports/public-api/wateros-vfs.md`
- `docs/exports/impl-guide/wateros-vfs.md`
- `docs/tasks/read-family/README.md`

## 已知信息与代码证据

`sys_read()` 当前先处理长度和指针：

```rust
if len == 0 {
    return UserRet::from_success(0);
}
if ptr == 0 {
    return UserRet::from_error(ErrNo::EFAULT);
}
```

因此当前会得到：

```text
read(-1, buf, 0)             -> 0，Linux 为 EBADF
read(write_only, buf, 0)     -> 0，Linux 为 EBADF
read(directory, buf, 0)      -> 0，Linux 为 EISDIR
read(-1, NULL, 1)            -> EFAULT，Linux 为 EBADF
```

普通文件 `read()` 没有检查访问模式。`PagedFileHandle` 已保存 `accmode`，但
`BufferedFileHandle` 只有 `writable: bool`，并把所有可写句柄报告成 `O_RDWR`：

```rust
fn open_accmode(&self) -> u32 {
    if self.writable { 2 } else { 0 }
}
```

pipe 读/写端没有覆盖 `open_accmode()`，继承的默认值是只读，导致写端方向错误无法在
syscall 前置校验中识别。

## 涉及文件

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/vfs_util.rs`
- `os/components/wateros-vfs/vfs-api/api-v0/src/handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/file_handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/dir_handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/handles.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/char_dev_handle.rs`
- `os/components/wateros-driver/driver-network/src/socket_handles.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/unix_sock.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/eventfd.rs`

## 任务内容

1. 在 VFS API 中建立不会执行 I/O 的读取能力校验。优先复用 `open_accmode()`，但必须
   让所有句柄准确实现；也可增加语义更清楚的 `validate_read_access()`。
2. `BufferedFileHandle` 保存真实 `accmode`，不能用 `writable` 推断
   `O_WRONLY/O_RDWR`。
3. pipe read end 为 `O_RDONLY`，write end 为 `O_WRONLY`；socket 和 eventfd 为
   `O_RDWR`；console stdin/stdout 分别为只读/只写。
4. 目录读取校验返回 `VfsError::NotAFile`，syscall 映射为 `EISDIR`。
5. `O_PATH` 继续由 fd slot 标志优先拒绝，返回 `EBADF`。
6. fd/type/access 校验完成后，`count == 0` 才返回 0；此时允许 `buf == NULL`。
7. `count > 0` 时才检查用户指针。无效 fd 与无效指针同时出现时优先 `EBADF`。

建议形态，命名可按现有风格调整：

```rust
fn validate_read_fd(fd: usize) -> Result<(), ErrNo> {
    if vfs::fd::is_path_only_fd(fd).map_err(vfs_error_to_errno)? {
        return Err(ErrNo::EBADF);
    }
    vfs::fd::with_current_io(fd, |handle| handle.validate_read_access())
        .map_err(vfs_error_to_errno)
}

validate_read_fd(fd)?;
if len == 0 {
    return success(0);
}
if ptr == 0 {
    return error(EFAULT);
}
```

不要通过调用 `read(&mut [])` 做能力探测；一些对象的空读实现会直接成功，另一些会进入
对象逻辑，无法保证“无副作用校验”。

## 并行与边界

本任务可与 RIO-02、RIO-03 并行。若增加 VFS API 方法，先单独提交 API 和所有默认/
dummy 实现，再提交 syscall 调用方，避免其它分支长期编译失败。

本任务不修改 `copy_to_user`，不实现 pipe/socket 事务，也不顺手修改 write 调用族。

## 如何验收

至少覆盖以下矩阵：

```text
invalid fd + count 0          -> EBADF
invalid fd + NULL + count 1   -> EBADF
valid readable fd + NULL + 0  -> 0
O_WRONLY regular file         -> EBADF
pipe write end                -> EBADF
directory fd                  -> EISDIR
O_PATH fd                     -> EBADF
socket/eventfd readable       -> 不被前置校验误拒绝
```

执行：

```bash
cd os
make rv_check
make la_check
```

运行验收纳入 RIO-10；可先用仓库 LTP
`test_case/ltp-full-20240524/testcases/kernel/syscalls/open/open09.c` 和
`test_case/ltp-full-20240524/testcases/kernel/syscalls/pipe/pipe03.c` 做定向验证。

## 搜索范围与交付

实现前用 `rg "impl VfsIoHandle|open_accmode|is_path_only_fd"` 覆盖
`os/components/wateros-vfs`、`wateros-driver` 和 `wateros-syscall`，不能只修改本文
列出的两个文件句柄。

代码和测试写入对应组件；临时日志放 `/tmp`。完成后在
`docs/tasks/read-family/README.md` 勾选 RIO-01，并在提交说明中记录 errno 矩阵及
`make rv_check/make la_check` 结果。不要提交 QEMU 日志或内核二进制。

## 禁止做法

- 不把所有 `Unsupported` 全局改成 `EBADF`。
- 不让 `open_accmode()` 的默认只读值掩盖遗漏实现。
- 不为通过零长度测试而跳过目录或 `O_PATH` 校验。
