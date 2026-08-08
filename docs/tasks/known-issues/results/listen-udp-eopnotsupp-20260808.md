# `listen(2)` UDP 错误码修复（2026-08-08）

## 问题

LTP `listen01` 对 UDP socket 调用 `listen(2)` 期望 `EOPNOTSUPP`，内核此前把所有
网络错误都映射为 `EINVAL`。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/listen.rs`：

- `WrongSocketType` / `Unsupported` 映射为 `EOPNOTSUPP`。
- 其余网络错误仍保持 `EINVAL`。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/listen-fixed.log`：

```text
listen01: bad file descriptor / not a socket / UDP listen 全部 TPASS
FAIL LTP CASE listen01 : 0
```
