# `operator-run` 脚本结束后自动关机

## 动机

性能优化需要可靠的 pc-hot A/B，但 `operator-run` 之前执行完脚本后会继续进入救援
shell，QEMU 不会自然退出，pc-hot 只能在超时或手动 kill 时落盘。偶发的 shell/idle
trap 会让两轮采样时长不一致，难以验收。

## 修改

`os/src/user_operator.rs` 中 `OperatorMode::Run` 的 `on_exit` 从 `Shell` 改为
`Shutdown`：

```rust
OperatorMode::Run => {
    plan.mode = OperatorMode::Run;
    plan.on_exit = ExitPolicy::Shutdown;
    ...
}
```

`operator-run` 构建的自动化内核会在 `SCRIPT` 执行完成后直接调用平台 shutdown。
`operator-shell` 和 `auto` 的行为不变。

## 验证

- `make check ARCH=rv PROFILE=pre`
- `make check ARCH=la PROFILE=pre`
- `make check ARCH=rv PROFILE=final`
- `make check ARCH=la PROFILE=final`
- RISC-V QEMU 使用 `MODE=run SCRIPT=/glibc/pcbench-auto.sh`：
  `mmap01`、`epoll_wait01` 通过，脚本打印 `PCDONE` 后 QEMU 自动退出。
- pc-hot 在自然退出时写盘：
  `/tmp/pcs-auto-shutdown.txt`，共 `326216437` 条指令。

## 后续

这个自然退出点可以作为后续所有 pc-hot A/B 的稳定 workload 终止条件。
