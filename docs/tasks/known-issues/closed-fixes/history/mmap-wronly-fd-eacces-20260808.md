# `mmap` O_WRONLY fd 返回 `EACCES`（2026-08-08）

## 问题

LTP `mmap06` 用 `O_WRONLY` 打开的 fd 映射文件，期望 `EACCES`。内核此前只做
`len==0` 与 flags 校验，未检查 fd 打开访问模式，错误地允许映射。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/mmap.rs`：

- 文件映射前通过 `vfs::fd::with_current_io` 读取 `open_accmode`。
- fd 访问模式为 `O_WRONLY` 时返回 `EACCES`。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/mmap06-eacces-fixed.log`：

```text
mmap06: EACCES x6 / EINVAL x2 全部 TPASS
```
