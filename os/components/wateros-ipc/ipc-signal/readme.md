下面以 UnixBench 中一次 `SIGALRM` 为例，标出从初始化、产生信号、进入 handler 到恢复现场的完整链路。

**完整链路**

```text
execve 创建地址空间
  → 映射 signal trampoline
用户 rt_sigaction 注册 handler
用户 setitimer 设置定时器
  → timer IRQ
  → 检查 interval timer
  → 产生 SIGALRM
  → 写入 process pending
  → 唤醒阻塞线程
返回用户态前检查 pending
  → 保存原用户现场
  → 构造 signal frame
  → 修改 PC/SP/RA/参数寄存器
  → sret 进入 handler
handler 执行 ret
  → 跳转 signal trampoline
  → rt_sigreturn syscall
  → 读取 signal frame
  → 恢复寄存器/PC/SP/FP/mask
  → sret 回原程序
```

**1. 映射 Trampoline**

加载 ELF、建立用户栈时，为每个用户地址空间映射 trampoline：

[RISC-V `map_signal_trampoline` line 442](../../wateros-mm/mm-impl/impl-sv39/src/kernel_elf.rs#L442)

其中的机器码：

[RISC-V trampoline CODE line 446](../../wateros-mm/mm-impl/impl-sv39/src/kernel_elf.rs#L446)

等价于：

```asm
addi a7, zero, 139   # rt_sigreturn
ecall
```

LoongArch 对应实现：

[LoongArch `map_signal_trampoline` line 526](../../wateros-mm/mm-impl/impl-loongarch64/src/kernel_elf.rs#L526)

该页面权限为用户态 `R | X`，不可写。

**2. 注册 Handler**

用户执行：

```c
signal(SIGALRM, handler);
```

glibc 最终调用 `rt_sigaction`：

[`sys_rt_sigaction` line 492](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/task.rs#L492)

内核把 handler、flags、mask 写入进程共享 disposition：

[`SignalRegistry::set_action` line 245](src/lib.rs#L245)

进程共享信号状态定义：

[`ProcessSignalState` line 58](src/lib.rs#L58)

线程私有 mask/pending 定义：

[`ThreadSignalState` line 110](src/lib.rs#L110)

**3. 设置 Interval Timer**

用户执行：

```c
setitimer(ITIMER_REAL, &timer, NULL);
```

syscall dispatch：

[`dispatch_setitimer` line 466](../../wateros-syscall/syscall-impl/impl-kernel/src/lib.rs#L466)

参数检查与用户数据复制：

[`sys_setitimer` line 582](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/task.rs#L582)

写入进程 timer 状态：

[`SignalRegistry::set_timer` line 462](src/lib.rs#L462)

**4. Timer IRQ 产生 SIGALRM**

CPU timer IRQ 进入统一 trap handler：

[`wateros_kernel_trap_handler` line 85](../../../src/trap_handler.rs#L85)

timer 分支调用 signal 计时：

[`syscall::timer_tick` line 187](../../../src/trap_handler.rs#L187)

计算 elapsed、推进 CPU timer 和 REAL timer：

[`timer_tick` line 153](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs#L153)

检查已到期 REAL timer：

[`SignalRegistry::expire_realtime` line 524](src/lib.rs#L524)

产生进程级 `SIGALRM`：

[`SignalRegistry::send_process` line 361](src/lib.rs#L361)

此时信号只是进入 `process.pending`，尚未运行 handler。

**5. 唤醒阻塞线程**

根据 disposition 处理信号结果：

[`apply_signal_dispatch` line 99](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs#L99)

如果目标线程正在 sleep、pipe、futex 或 wait4：

[`task::interrupt_task` line 193](../../wateros-task/src/lib.rs#L193)

调度器把线程从等待队列和 timeout 队列移出，并返回 `Interrupted`。

**6. 返回用户态前交付信号**

所有 trap 的公共返回路径都会调用：

[`return_to_user_signal_delivery` line 254](../../../src/trap_handler.rs#L254)

进入真正的交付函数：

[`deliver_pending_signal` line 221](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs#L221)

选择：

```text
(thread pending | process pending) - thread mask
```

对应：

[`SignalRegistry::take_deliverable` line 424](src/lib.rs#L424)

这里也会处理：

- handler 执行期间的 mask。
- `SA_NODEFER`。
- `SA_RESETHAND`。
- `sigsuspend` 原 mask 保存。

**7. 保存原用户现场**

架构无关的寄存器现场格式：

[`SignalMachineContext` line 154](../../wateros-platform/platform-arch/arch-api/api-v0/src/trap.rs#L154)

包括：

```text
GPR[32]
PC
status
FP registers[32]
FCSR
```

统一架构接口：

[`SignalFrameCodec` line 177](../../wateros-platform/platform-arch/arch-api/api-v0/src/trap.rs#L177)

捕获当前现场：

[`capture_signal_context` call line 237](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs#L237)

RISC-V 实现：

[RISC-V `capture_signal_context` line 280](../../wateros-platform/platform-arch/arch-impl/impl-riscv64/src/trap.rs#L280)

LoongArch 实现：

[LoongArch `capture_signal_context` line 287](../../wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/trap.rs#L287)

**8. 构造 Signal Frame**

用户态 `ucontext`：

[`UserUContext` line 47](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs#L47)

完整 signal frame：

[`UserRtSignalFrame` line 58](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs#L58)

内核从原 SP 向下分配 frame，并进行 16 字节对齐：

[`frame_sp` calculation line 247](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs#L247)

然后写入用户栈：

[`copy_to_user_struct` line 267](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs#L267)

**9. 修改寄存器进入 Handler**

公共调用点：

[`prepare_signal_handler` call line 279](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs#L279)

RISC-V 设置：

[RISC-V `prepare_signal_handler` line 306](../../wateros-platform/platform-arch/arch-impl/impl-riscv64/src/trap.rs#L306)

```text
ra   = signal trampoline
sp   = signal frame
a0   = signal number
a1   = siginfo*
a2   = ucontext*
sepc = handler
```

LoongArch 设置：

[LoongArch `prepare_signal_handler` line 313](../../wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/trap.rs#L313)

之后 trap 返回，CPU 从 handler 地址继续运行。

**10. Handler 返回到 Trampoline**

handler 执行：

```c
void handler(int sig) {
    expired = 1;
}
```

handler 的 `ret` 使用内核预先设置的 `ra`，因此跳到 trampoline。

Trampoline 发起 syscall 139：

```text
rt_sigreturn()
```

**11. rt_sigreturn 恢复现场**

trap 层识别特殊 syscall：

[`RT_SIGRETURN` branch line 119](../../../src/trap_handler.rs#L119)

调用 frame 恢复：

[`restore_signal_frame` call line 121](../../../src/trap_handler.rs#L121)

读取并验证用户 signal frame：

[`restore_signal_frame` line 288](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs#L288)

恢复架构现场：

[`restore_signal_context` call line 296](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs#L296)

RISC-V 实现：

[RISC-V `restore_signal_context` line 292](../../wateros-platform/platform-arch/arch-impl/impl-riscv64/src/trap.rs#L292)

LoongArch 实现：

[LoongArch `restore_signal_context` line 299](../../wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/trap.rs#L299)

最后恢复 signal mask，`sret` 回到信号到来前的 PC。

**12. SA_RESTART 支线**

阻塞 syscall 返回 `EINTR` 时，trap 层记录原 syscall：

[`restartable_syscall` decision line 142](../../../src/trap_handler.rs#L142)

支持自动重启的 syscall 列表：

[`restartable_syscall` line 265](../../../src/trap_handler.rs#L265)

如果 handler 设置了 `SA_RESTART`：

[`prepare_syscall_restart` call line 240](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs#L240)

架构契约：

[`SignalFrameCodec::prepare_syscall_restart` line 195](../../wateros-platform/platform-arch/arch-api/api-v0/src/trap.rs#L195)

RISC-V 实现：

[RISC-V `prepare_syscall_restart` line 324](../../wateros-platform/platform-arch/arch-impl/impl-riscv64/src/trap.rs#L324)

它会：

```text
保存 PC -= 4
恢复 syscall 参数
恢复 syscall number
```

因此 handler 执行完 `rt_sigreturn` 后，会重新执行原来的 `ecall`。
