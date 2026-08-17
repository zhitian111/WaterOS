# RISC-V 用户 trap 单次 FPU 恢复实验

## 为什么选择这里

RISC-V trap 入口把 `f0..f31` 和 `fcsr` 保存进栈上 `TrapContext`。Rust handler 返回后，
公共汇编路径原本先恢复一次完整 FPU 状态；若目标是用户态，随后
`__wateros_riscv_restore_user_from_frame` 又在切换用户 `satp` 前从同一份
`TrapContext` 恢复一次。因此每个用户 syscall、page fault 和 timer interrupt 都重复执行
32 次 `fld` 和一次 `fcsr` 写入。

用户态返回必须经过后一个 helper，因为首次启动、exec 和 fork 也共用它；公共路径中的恢复
只对内核态返回有必要。

## 方案

- 删除 Rust handler 返回后的公共 FPU 恢复块；
- 把同一恢复块移动到 `.Ltrap_return_kernel`；
- 用户返回继续只由 `__wateros_riscv_restore_user_from_frame` 恢复；
- 不修改 `TrapContext` 布局、保存路径、`sstatus`、`sepc`、GPR 或 `satp` 顺序。

该修改不实现 lazy FPU，也不改变任务切换时 FPU 状态的所有权，只消除已经由下一跳完整完成的
重复工作。LoongArch 使用独立的 trap 汇编，不在本实验范围内。

## 验证

1. `make rv_check`；
2. `make kernel-rv-final`；
3. RISC-V 启动与用户程序 smoke，覆盖 syscall、timer 抢占和 page fault；
4. 浮点定向测试，至少覆盖两个用户线程被定时器抢占后结果不串线；
5. 完整 BuildStorm candidate/main A/B；
6. `git diff --check`。

只有完整 workload 功能通过且墙钟无回归时才接受性能候选。

## 当前验证结果

- `make rv_check`：通过；仅有仓库既有 warning。
- `make kernel-rv-final`：通过，候选内核 SHA-256 为
  `2dcd979b417f395c404c94ee850b2636d0dedd1894dfa855495687a8a660bb06`。
- QEMU 9.2.1、16 GiB、8 vCPU、指定 Final 镜像、`-snapshot` 运行 120 秒：
  - OpenSBI 识别 8 hart，WaterOS 正常进入 Final workload；
  - cagent 十项测试全部通过；
  - `TOOLCHAIN_RESULT status=OK`；
  - `MINIBUILD_RESULT status=OK`；
  - BuildStorm 正常进入正式 cargo/rustc 编译并推进至 `ax-posix-api`；
  - 120 秒到期后由 host `timeout` 发送 SIGTERM，未见 panic、SIGSEGV、非法指令或 stall。
- `git diff --check`：通过。

尚未运行完整 BuildStorm candidate/main 墙钟 A/B，因此当前只证明功能短测通过，不宣称已有
端到端性能收益。
