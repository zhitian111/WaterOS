# generic syscall 号与 `siginfo_t` 布局修复（2026-08-08）

## 问题

继续审计 `wateros-syscall-api` 的 generic ABI 号表时发现：

- `__NR_waitid` 在 riscv64/loongarch64 是 95，内核此前使用 247（x86_64 编号），
  LTP `waitid04+` 无法进入已实现的 `sys_waitid`。
- asm-generic 没有独立 `__NR_poll`，内核把 271（实际是 `process_vm_writev`）
  当作 `poll` 分发；用户态 `poll(2)` 经 `ppoll`(73) 进入即可。
- RISC-V `siginfo_t` 在 `si_signo/si_errno/si_code` 后有 4 字节填充，
  `si_pid` 从偏移 16、`si_status` 从偏移 24 开始。内核 `UserSigInfo` 缺 pad，
  导致 `waitid` 返回成功但 `si_pid/si_status` 未写到用户期望的位置。

## 修改

`os/components/wateros-syscall/syscall-api/api-v0/src/number.rs`：

```rust
pub const WAITID : usize = 95;
/// asm-generic 64 位无独立 `poll` nr；riscv64/loong64 用户态经 `ppoll`(73) 实现。
pub const POLL : usize = usize::MAX;
```

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/wait.rs` 与
`.../sys/ipc/signal.rs`：

```rust
struct UserSigInfo {
    signo : i32,
    errno : i32,
    code : i32,
    pad : i32,
    payload : [u8; 112],
}
```

`wait.rs` 的 `UserSigInfo` 增加 128 字节编译期断言；`signal.rs` 的 rt_sigqueueinfo
payload 偏移同步改为 `payload[0..4]`（pid）与 `payload[4..8]`（uid）。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/waitid-layout-fixed.log`：

```text
waitid05: si_pid/si_status/si_signo/si_code 全部 TPASS
waitid06: si_pid/si_status/si_signo/si_code 全部 TPASS
```

`waitid04` 也通过；`waitid07+` 仍在内核排除名单中，未在本轮复验。

## 后续

271 现在不再被误路由到 `poll`；如后续实现 `process_vm_writev` 或 `execveat`(281)，
应分别按 generic ABI 注册到对应 handler。
