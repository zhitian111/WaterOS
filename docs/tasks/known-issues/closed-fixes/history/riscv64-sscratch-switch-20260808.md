# RISC-V 切换到内核/idle 上下文时清理 sscratch

## 现象

完整 RISC-V Final BuildStorm 偶发出现：

```text
[trap] ignoring signal frame setup failure on non-user context
task=Some(7) kind=Some(Kernel) state=Some(Running) process_task=None returns_to_user=true
[PANIC] restore_current_trap_frame failed before sret to user
```

即当前任务是内核/idle 任务，但 trap 帧仍被判断为来自用户态。

## 根因

`__wateros_riscv_restore_user_from_frame` 在每次 `sret` 到用户态前会把 `sscratch`
设为当前 CPU 的 user return frame。协作式 `__switch` 切到内核/idle 任务时没有清理
`sscratch`，导致下一个内核 trap 被入口汇编误当作“来自用户态”，并继续使用旧用户
return frame。随后 trap handler 发现当前任务不是用户任务，restore TCB trap 帧失败。

## 修改

`os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/asm/switch.S`：

- `__switch` 加载新任务栈后、`ret` 前执行 `csrw sscratch, x0`。
- 用户任务恢复时会再次由 `__wateros_riscv_restore_user_from_frame` 设置 `sscratch`，
  因此该清理不会破坏用户 trap 返回路径。

## 验证

- `make check ARCH=rv PROFILE=final` 通过。
- `make check ARCH=la PROFILE=final` 通过。
- 完整 RISC-V Final 多轮运行不再出现“内核任务却返回用户态”的 restore 失败；
  BuildStorm 仍会继续暴露后续 heap allocator 递归问题，单独跟踪。

原始日志示例：

```text
/tmp/final-rv-ra10.log
/tmp/final-rv-ra11.log
```
