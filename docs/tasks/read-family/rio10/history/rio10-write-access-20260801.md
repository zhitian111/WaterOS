# RIO-10 问题修复：write 访问模式与 iovec 导入

## 问题

RIO-10 定向 LTP 首次运行发现：

- `write(pipe_read_end, ...)` 返回 `EINVAL`，Linux/LTP 要求 `EBADF`；
- `pwritev` 的 `iov_len=-1` 在复制用户数据时返回 `EFAULT`，应在 iovec 导入阶段返回
  `EINVAL`。

根因分别是 write 调用族缺少前置 `open_accmode` 校验，以及 writev/pwritev 在完整验证
descriptor 前直接 gather 用户数据。

## 修复

- 增加 syscall 内部 `validate_write_fd`，在用户指针和 zero-length 判断前检查 fd、
  `O_PATH` 和访问模式。
- `write`、`writev`、`pwrite64`、`pwritev` 统一使用该校验；pipe 读端和只读文件返回
  `EBADF`，可写 pipe 的 positional write 仍由 VFS 返回 `ESPIPE`。
- writev/pwritev 复用 RIO-09 的 `import_iovecs`，先完成 descriptor 地址、数量、NULL、
  checked total 和 `SSIZE_MAX` 校验，再构造最多 4 MiB 的 staging。

修改文件：

```text
os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs
```

## 验证

- `make rv_check`、`make la_check`、`make kernel-rv-ltp-glibc`：通过。
- `pipe03`：2 TPASS，确认 pipe 两端反向访问均为 `EBADF`。
- `write02`：2 TPASS；`pwrite02`：5 TPASS。
- `writev02`：1 TPASS；`pwritev02`：7 TPASS。
- 日志：`/tmp/wateros-write-access.log`、`/tmp/wateros-writev-import.log`。
- 临时镜像入口和 runner 已恢复/删除并通过 `cmp`。

未修改 task、scheduler、pipe 数据结构或 VFS API。
