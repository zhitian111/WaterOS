# generic ABI `epoll_pwait` 修复（2026-08-08）

## 问题

`epoll_pwait04` 使用 `PROT_NONE` 映射地址作为 `sigmask`，期望返回 `EFAULT`，但内核
返回成功。根因不是缺页处理，而是 syscall 号表错误：

- RISC-V64/LoongArch64 使用 asm-generic ABI，`__NR_epoll_pwait = 22`，且没有独立的
  `__NR_epoll_wait`。
- 内核此前把 `EPOLL_WAIT` 设为 22、`EPOLL_PWAIT` 设为 281，这对应 x86_64 ABI。
- 因此 LTP 的 `epoll_pwait` 实际调用 22，被内核当作 `epoll_wait` 处理，`sigmask`
  被完全忽略。

## 修改

`os/components/wateros-syscall/syscall-api/api-v0/src/number.rs`：

```rust
/// asm-generic 64 位无独立 `epoll_wait` nr；riscv64/loong64 用户态经 `epoll_pwait`(22) 实现。
pub const EPOLL_WAIT : usize = usize::MAX;
pub const EPOLL_PWAIT : usize = 22;
```

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/poll/epoll.rs`：

- 22 号统一进入 `sys_epoll_pwait`。
- `sigmask != 0` 时先通过 `copy_from_user` 校验用户地址，失败返回 `EFAULT`。
- 校验通过后委托 `sys_epoll_wait`，原等待逻辑不变。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

仅 LTP 的 RISC-V pre 定向运行（临时裁剪镜像 `/tmp/sdcard-rv-epoll.img`）：

```text
epoll_pwait04.c:25: TPASS: with an invalid sigmask pointer : EFAULT (14)
epoll_wait01: 3 TPASS, FAIL LTP CASE epoll_wait01 : 0
epoll_wait02: 11 TPASS, FAIL LTP CASE epoll_wait02 : 0
```

原始日志：`/tmp/epoll-ltp-fixed.log`。

## 后续

`epoll_wait03` 的负 `maxevents` 校验仍失败，属于另一个问题；`epoll_pwait2` 因
syscall 441 未实现而 `TCONF`，不在本次修复范围内。
