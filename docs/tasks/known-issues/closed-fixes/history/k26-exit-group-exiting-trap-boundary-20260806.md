# K-26 `exit_group` 远端线程退出边界修复（2026-08-06）

## 问题

`exit_group` 会把进程发布为 `ProcessState::Exiting`，代码注释约定仍在远端 CPU
运行的 sibling 会在下一次 trap 边界观察到该状态并自行退出。但 trap 返回路径没有
检查 `Exiting`，因此多核并发下可能留下仍处于用户态/内核边界的 sibling；父进程
`waitpid` 一直等待进程变成 `Exited`，BuildStorm 脚本在 `cargo xtask` 已输出 `done`
后停在 `| tee` 管道，最终结果标记无法打印。

## 修复

在 `os/src/trap_handler.rs` 的用户 trap 处理入口增加 `ProcessState::Exiting` 检查：

```rust
if let Some(process) = task::current_process_snapshot() {
    if let task::ProcessState::Exiting(exit_code) = process.state {
        task::exit_group_current(exit_code);
    }
}
```

该路径只影响已经进入 `exit_group` 的进程，不改变普通进程生命周期、task API 或
调度队列结构。

## 验证

```text
make rv_check
make la_check
make kernel-rv-final
```

完整 RISC-V Final：

```text
CAgent 10/10
compile_count=136
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1567.28 cores=8 bytes=1681000 arch=riscv64
#### OS COMP TEST GROUP END buildstorm-glibc ####
```

原始日志：`/tmp/k25-full-exiting-rv-1785995120.log`。

初赛可行性：使用 `os/sdcard-rv.img` 和 `kernel-rv-pre` 运行 60 秒，系统正常挂载并
进入 `hackbench`/`cyclictest`，无 panic、deadlock 或 ext4 读块错误。日志：
`/tmp/k25-pre2-rv-*.log`。
