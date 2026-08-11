# non-user 信号帧建立失败不再 fatal 停机

## 现象

BuildStorm 完整轮在 `UserEnvCall` 返回用户态时偶发：

```text
fatal kernel trap (attempted to terminate a non-user task)
cause=Exception(UserEnvCall) returns_to_user=true
```

排查发现除 `rt_sigreturn` 外，`return_to_user_signal_delivery()` 在
`deliver_pending_signal()` 返回错误时也会调用 `kill_current_user_task()`。若当前
任务已不是用户任务、进程快照也已消失，就会走 fatal 停机。

## 修改

`os/src/trap_handler.rs`：

- 信号帧建立失败时先检查当前是否仍存在可终止的用户任务/进程上下文。
- 如果已经变成非用户上下文，不再 fatal，而是 warn 并跳过本次信号交付，继续 trap
  返回。

## 验证

- `make check ARCH=rv PROFILE=final` 通过。
- `make check ARCH=la PROFILE=final` 通过。

完整 BuildStorm 仍被 allocator 递归 panic 阻断，需要单独继续定位。
