# K-02A Signal Return ABI 核验（2026-08-05）

## 初始误判

LoongArch TLB 探针最初出现两类现象：标准 `sigaction()` 返回 `EINVAL`，raw handler
进入后再次发生同一写 fault。代码与 UAPI 核对后确认：

- RISC-V/LoongArch 使用 asm-generic kernel sigaction：`handler + flags + 64-bit mask`，
  共 24 字节，不含 `sa_restorer`；当前内核布局正确。
- loader 在固定 `...B000` 地址映射了架构专用 `rt_sigreturn(139)` trampoline，和
  signal delivery 使用的地址一致。
- 注入的旧 LoongArch glibc 使用 128 signals，向 syscall 传 `sigset_size=16`；当前
  competition generic ABI 使用 64 signals/8 字节，因此按契约返回 `EINVAL`。
- raw 探针从 handler 执行 `longjmp`，跳过 trampoline/`rt_sigreturn`，SIGSEGV 保持
  自动屏蔽；第二次 fault 无可投递信号是预期结果，不证明 trampoline 损坏。

## 标准路径验证

使用当前比赛镜像对应 libc 的 `sigaction()` 安装 handler。每轮把匿名页改为只读，
store 触发 SIGSEGV；handler 调用 `mprotect(RW)` 后正常 return，经 trampoline 执行
`rt_sigreturn`，原 store 重试并成功。RISC-V 与 LoongArch 各完成：

```text
SIGNAL_RETURN_PASS iterations=1000 handled=1000
```

两架构均 exit code 0，无二次 fault、mask 残留或 signal frame 错误。因此无需修改内核
signal ABI。旧 128-signal LoongArch glibc 不属于当前 generic ABI，不能通过忽略高 64
位的方式伪兼容。

日志备份于 `os/debug-reports/archive/signal-return-20260805/`。SHA-256：RISC-V
`fe4aad1b...20628`，LoongArch `5cfff9d5...df0bf`；完整值见 `SHA256SUMS`。
