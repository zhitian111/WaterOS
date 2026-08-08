# `socketpair` 未对齐 `sv` 指针校验（2026-08-08）

## 问题

LTP `socketpair01` 的 `bad unaligned pointer` 用例把 `sv` 设为
`(int *)7`，期望 `EFAULT`。内核此前只检查 `sv == 0`，随后通过软件
`copy_to_user` 写入未对齐地址，在地址可写时错误地返回成功并泄漏一对 socket fd。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/socketpair.rs`：

```rust
if sv_ptr == 0 || sv_ptr % core::mem::align_of::<i32>() != 0 {
    return UserRet::from_error(ErrNo::EFAULT);
}
```

校验发生在创建 fd 之前，避免失败路径需要回滚已注册 socket。

## 验证

LTP 定向日志 `/tmp/socketpair-fixed.log`：

```text
socketpair01: invalid domain / invalid type / UNIX dgram / raw /
bad aligned pointer / bad unaligned pointer / UDP / TCP dgram /
TCP socket / ICMP stream 全部 TPASS
FAIL LTP CASE socketpair01 : 0
```

同一轮还复验了 `rmdir01`，当前通过。

## 后续

`rmdir02` 仍存在非空目录返回 `EEXIST` 而不是 `ENOTEMPTY`、符号链接循环和
只读/挂载占用错误映射问题，单独归入 FS 错误映射任务。
