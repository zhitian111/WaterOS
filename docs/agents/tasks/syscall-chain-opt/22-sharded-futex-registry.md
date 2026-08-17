# 任务 22：分片 futex registry 并保持 lost-wake 防护

## 任务内容与目标

在锁等待数据证明 futex 全局 registry 成为瓶颈后，按稳定 FutexKey hash 分片 registry，提升
多核并行 wait/wake。条件复查、wake_sequence、waiter_sequence 和 scheduler 入队原子性不得
删除或弱化。

## 实施方案

1. 固定 2 的幂分片数，private/shared key 的同一逻辑地址始终映射同一 shard。
2. 单 key wait/wake 只锁一个 shard；requeue 两 key 按 shard index 排序加锁，同 shard 只锁一次。
3. 保持当前“预检 -> publish waiter -> 复检 -> sequence guard -> sleep”流程。
4. robust cleanup、task cancel 和队列 active_users 生命周期覆盖所有 shard。
5. 增加并发 wait/wake/requeue/timeout/signal/exit、反向双 key requeue 死锁测试。

## 涉及文件

- `os/components/wateros-ipc/ipc-futex/futex-impl/impl-task/src/{global,registry}.rs`
- futex key/hash API 与 waitqueue 测试
- 必要的 diagnostics 汇总

## CodeGraph 查询

```bash
codegraph explore "FutexRegistry wait_while wake requeue wake_sequence finish_waiting_task"
codegraph impact "FutexRegistry"
codegraph callers "with_registry"
```

## 验收方式

```bash
cd os
cargo test --offline --manifest-path components/wateros-ipc/ipc-futex/futex-impl/impl-task/Cargo.toml
make rv_check && make la_check && make kernel-rv-final
# SMP futex wait/wake/requeue/robust/timeout/signal 压力测试
cd .. && git diff --check
```

不得出现 lost wake、重复 wake、泄漏或 shard 反序死锁。只有基线锁等待显著且任务 00 A/B 有
可复现改善才保留；否则全局短临界区可能更优，应回退并记录结论。

## Commit 与简报

提交建议：`[perf] futex registry 按 key 分片`。新增 `history/22-brief.md`，附锁等待和压力结果。
