# K-10 LTP 过滤策略边界验证（2026-08-03）

## 目标

保留初赛 bringup 在运行前删除不适配 LTP 用例的机制，同时移除通用 syscall 对
`ltp_cgroup_helper`、程序名和 argv 的依赖。过滤名单现在由
`os/src/user_bringup_ltp_exclusions.rs` 持有，仅在 `pre` 构建中使用。

## 结果

- RISC-V64 与 LoongArch64 的 `cargo check` 均通过。
- `make kernel-rv-ltp-musl` 构建通过。
- 180 秒上限的 RISC-V musl LTP 短测中，主动取得证据后于约 80 秒终止。
- prune 日志：`2353 basenames x 2 libc, removed=4686 absent=20 failed=0`。
- runner 随后到达 `OS COMP TEST GROUP START ltp-musl` 并连续执行原生 LTP 用例。
- 日志中未发现 panic、deadlock 或 prune 失败。
- 停机后 `e2fsck -fn os/sdcard-rv.img` 五阶段检查通过。

这证明过滤行为不再依赖 syscall helper，且删除机制仍在测试启动前生效。本轮不是全量
LTP 兼容性结论；全量回归继续使用夜间测试窗口。

## 复现信息

```text
kernel_base_commit: ce2f829836e472f0b7ee7b9ff914d88619804aab
user_submodule_commit: 2f470f95fa6bf0401c4b1b7ef3bb8fc7a10b870b
architecture: riscv64, OpenSBI, 8 CPU
qemu: 11.0.2
commands: make check; make la_check; make kernel-rv-ltp-musl
smoke_command: timeout 180s env WOS_KERNEL=./kernel-rv-ltp-musl bash ./scripts/rv_pre_run.sh
raw_log_path: /tmp/wateros-ltp-prune-smoke-20260803.log
raw_log_sha256: 34fec3fb5ab7c21161482eabd11622aa4366505f74dcc5e8e96de75c7694e2dd
image_sha256_after: eed7f895f54a0a606d8bf05e2558650dd51f3b02b74b9703f6ad6fb1e8f03516
```
