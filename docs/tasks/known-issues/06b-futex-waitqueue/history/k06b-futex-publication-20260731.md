# K-06B Futex 等待发布竞态修复报告

```text
task: K-06B futex wait publication
date: 2026-07-31
kernel_commit: 26355818939d39f538cce6e8af9440dc0620d0a9 + 本报告对应未提交修复
user_submodule_commit: 2f470f95fa6bf0401c4b1b7ef3bb8fc7a10b870b
architecture: RISC-V64, 8 CPU
qemu_and_firmware: QEMU 11.0.2, OpenSBI 1.7
image_sha256: 69cd55ecd4118cf24f9dbdd145c35734693ae0021077f4d4dd27f0ee965c6870
overlay: none；测试 ELF 临时写入 os/sdcard-rv.img，测试后删除
commands: make rv_check; make la_check; timeout 30s env WOS_KERNEL=... rv_pre_run.sh
result_markers: FUTEX_SMOKE_OK transitions=16000; FUTEX_SMOKE_DONE
first_failure: none
raw_log_path: 未保留；本轮按白天短测约束直接筛选终端输出
raw_log_sha256: unavailable
```

## 结论

已修复 `FutexHub::wait_while()` 中仍可吞掉并发 wake 的窗口。修改仅位于
`wateros-ipc-futex-impl-task`，没有改变 task API、scheduler 锁序、任务状态机或
`WaitQueue` 结构。

## 问题

旧代码在 registry 锁内取得队列并登记 waiter，但在解锁后才读取
`wake_sequence`：

```rust
let (wait_queue, wake_sequence) = with_registry(...);
let observed_wake = wake_sequence.load(Ordering::Acquire);
```

若 waker 在两行之间取得队列、递增 sequence 并执行一次空 scheduler wake，waiter
会把递增后的值当成基线。用户 futex 字发生 ABA 或 wake 本身不改值时，后续条件复查
仍可成立，waiter 随后入队且不再收到这次 wake。

## 修改

现在 waiter 在持有 registry 锁、完成队列使用权和 task 登记后立即读取 sequence，
并把 `(queue, sequence, observed)` 一起带出临界区。waker 获取同一队列也必须经过
该锁，因此两者形成明确的线性化顺序：

- 先发生的 wake 位于 waiter 发布之前，不负责唤醒该 waiter；
- 发布之后发生的 wake 必然改变 waiter 已记录的 sequence，或在 waiter 入队后由
  scheduler 正常唤醒。

原有用户字二次复查、Mesa condition 语义、timeout/signal 返回和 sequence 的
Release/Acquire 配对均保留。

## 验证

- `make rv_check`：通过。
- `make la_check`：通过。
- RISC-V64/OpenSBI、8 核 QEMU 定向 pthread 测试：通过。
- 测试使用 8 个线程、`pthread_mutex` 与 `pthread_cond_broadcast`，按固定顺序完成
  16,000 次交接，输出 `FUTEX_SMOKE_OK transitions=16000`，内核记录耗时约 3.204 秒。
- 临时测试 ELF 和源码已删除，bringup 命令已还原。

本次按白天短测约束没有运行完整 LTP、pre 或 final 测试。BuildStorm 长测和 futex
LTP/robust/clear-child-tid 全量回归仍属于 K-06B 总体验收，需在夜间测试窗口执行。
