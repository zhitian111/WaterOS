# `pwrite64` O_APPEND 追加语义（2026-08-08）

## 问题

LTP `pwrite04` 以 `O_APPEND` 打开文件后调用 `pwrite(fd, buf, len, 0)`，Linux 应
追加到文件末尾，而不是写到 offset 0。内核此前完全忽略 O_APPEND，导致文件大小
错误。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`：

- `sys_pwrite64` 检查 fd 的 `O_APPEND` 状态。
- O_APPEND 时使用当前 metadata size 作为实际写偏移。
- 保持 pwrite 不改变 fd 的文件位置语义。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/pwrite04-oappend-fixed.log`：

```text
pwrite04: O_APPEND test passed
pwrite04_64: O_APPEND test passed
```
