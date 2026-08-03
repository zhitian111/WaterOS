# K10 Futex Requeue 计数校验结果

## 问题

LoongArch64 8 核全量 musl LTP 的 `futex_cmp_requeue02` 中，传入负的
`nr_wake` 或 `nr_requeue` 时系统调用错误地返回成功。该用例同时是
CVE-2018-6927 的回归测试。

## 根因与修复

`sys_futex()` 将系统调用参数直接转换为 `u32`。用户态传入 `-1` 后会成为
`u32::MAX`，并被继续传给 futex 队列层；入口处没有执行 Linux 要求的
`INT_MAX` 上界检查。

在
`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/futex.rs`
增加 `validate_requeue_count()`，并在生成 futex key、读取用户地址以及取得调度器锁
之前同时校验唤醒数和迁移数。超过 `i32::MAX` 的值返回 `EINVAL`。修复保持
`wateros-ipc` 和 task 调度接口不变。

## 验证

- `make check`：通过。
- `make kernel-la-ltp-musl`：通过。
- LoongArch64/QEMU、8 核、musl LTP 定向运行 `futex_cmp_requeue02`：
  3 个子项全部通过，退出码为 0。
  - 负 `nr_requeue`：`EINVAL`；
  - 负 `nr_wake`：`EINVAL`；
  - 比较值不匹配：`EAGAIN`。
- 验证日志：`/tmp/wateros-futex-cmp-requeue-after.log`。

全量 LTP 运行使用修复前已启动的内核，因此其历史失败记录不用于判定本修复结果；
下一轮全量镜像验证会包含该修复。
