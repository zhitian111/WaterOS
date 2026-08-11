# BuildStorm non-user rt_sigreturn 永久停机修复

## 现象

完整 RISC-V Final BuildStorm 编译早期会偶发：

```text
[trap] fatal kernel trap (attempted to terminate a non-user task)
       cause=Exception(UserEnvCall) raw_cause=0x8 returns_to_user=true
```

此前该路径直接进入 `fatal_kernel_trap()`，CPU 永久停在 WFI，BuildStorm 日志不再
推进，成为完整轮阻断。

## 根因

用户态执行 `rt_sigreturn` 时 `restore_signal_frame()` 失败。trap 帧仍标记
`returns_to_user=true`，但当前任务快照已不是 `TaskKind::User`，且没有可终止的用户
进程快照。旧代码无条件尝试终止进程，发现“non-user task”后直接停机。

## 修改

`os/src/trap_handler.rs`：

- `kill_current_user_task()` 只在“无用户态返回”或“无当前用户进程快照”时才 fatal。
- `rt_sigreturn` 失败且当前上下文不是用户进程时，不再停机，而是把该用户 ecall
  当作 `-EINVAL` 返回，并正常走 trap 返回路径。

## 验证

- `make check ARCH=rv PROFILE=final` 通过。
- `make check ARCH=la PROFILE=final` 通过。
- 完整 RISC-V Final 复跑：不再出现该 non-user fatal；日志推进到 BuildStorm 编译，
  随后命中另一个 `recursive heap allocation` panic，已单独记录为后续任务。

## 后续

allocator 递归 panic 是当前完整轮的新阻断。诊断显示递归发生在 TLSF `dealloc`
路径的 guard 内，需要继续定位是哪段代码在释放过程中再次进入分配器。
