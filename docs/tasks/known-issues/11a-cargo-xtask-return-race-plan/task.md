# K-53 修复计划：`cargo xtask` 返回竞态

## 问题

BuildStorm 在 `[axbuild] ... done` 后偶发不返回，导致 shell 管道不能读取
`/work/.build.rc`，最终不打印 `BUILDSTORM_COMPILE`。

## 根因假设

`exit_group` 已发布 `ProcessState::Exiting`，但当前实现只在 **trap 进入时且即将返回
用户态** 检查一次该状态：

```rust
if cx.returns_to_user() {
    if let Some(process) = task::current_process_snapshot() {
        if let task::ProcessState::Exiting(exit_code) = process.state {
            task::exit_group_current(exit_code);
        }
    }
}
```

若 sibling 线程在 `exit_group` 发布后仍阻塞在内核 syscall（futex、read、wait 等），
它被唤醒后直接在 `dispatch_syscall_from_trap` 的返回路径上回到用户态，不会再次经过
上面的入口检查。该线程可继续运行，进程无法全部标记 `Exited`，`cargo xtask` 不退出。

## 修复思路

在 trap 的统一返回用户态路径上，再做一次 `ProcessState::Exiting` 检查：

```rust
if cx.returns_to_user() {
    if let Some(process) = task::current_process_snapshot() {
        if let task::ProcessState::Exiting(exit_code) = process.state {
            task::exit_group_current(exit_code);
        }
    }
    return_to_user_signal_delivery(authoritative, trap_cause, cx, restart);
}
```

这样覆盖：

- 阻塞 syscall 被唤醒后返回用户态；
- IPI/timer 中断后返回用户态；
- 普通 syscall 执行期间进程被标记 Exiting；
- 用户态线程即使不主动 syscall，也会在下一个 trap 返回点退出。

该检查不改变普通进程状态，开销是每个用户 trap 返回前一次进程状态快照查询。

若 sibling 已运行在长内核路径，仅靠返回用户态检查仍可能来不及退出。因此在 timer
中断且 `returns_to_user=false` 时，若当前进程已是 `Exiting`，也调用
`exit_group_current`。这覆盖内核态长时间运行、不主动返回用户态的 sibling。

## 实施文件

- `os/src/trap_handler.rs`

## 验证

1. `make rv_check && make la_check`。
2. `make kernel-rv-final && make kernel-rv-pre`。
3. RISC-V Final 连续多轮输出 `BUILDSTORM_COMPILE ok=true`。
4. LoongArch Final 复测。

## 验收标准

- [ ] 不再出现 axbuild `done` 后 5 分钟仍无 `BUILDSTORM_COMPILE`。
- [ ] 双架构 Final 至少各通过一轮。
- [ ] Pre smoke 无 panic。
