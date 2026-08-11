# BuildStorm VirtIO 块 I/O 完成路径诊断

## 已确认的问题

WaterOS 的 `VirtioBlkDevice::{read_blocks,write_blocks}` 直接调用 `virtio-drivers 0.12.0` 的同步
接口。该接口最终进入 `VirtQueue::add_notify_wait_pop()`，提交 descriptor 后执行：

```text
while !self.can_pop() {
    spin_loop();
}
```

同时所有文件系统 I/O 经 `SharedBlockDevice = Arc<spin::Mutex<Box<dyn BlockDevice>>>` 串行化。
因此一个任务等待 VirtIO 完成时会占用 vCPU；其他 vCPU 上进入块设备的任务还可能在同一把
spin mutex 上继续忙等。在 QEMU TCG 的 8 vCPU BuildStorm 中，这些 vCPU 与 cargo/rustc 的
计算线程争用宿主 CPU，机制上符合“串行读取吞掉 CPU”的现象。

## Linux 对照

Linux blk-mq 将请求提交与完成分开，以 tag 关联异步完成，并使用每 CPU 软件队列和硬件 dispatch
queue，避免 SMP 上单队列/单锁扩展瓶颈：

- <https://docs.kernel.org/block/blk-mq.html>
- <https://docs.kernel.org/scheduler/completion.html>

WaterOS 不适合一步照搬完整 blk-mq。本阶段先量化忙等是否仍是 current-best 内核的热点，再选
最小可落地层次：

1. 300s `pc-hot` 诊断确认 `add_notify_wait_pop` / virtqueue used-ring polling及 spin mutex 占比。
2. 若不热，停止架构改造，回到文件系统/分配器热点。
3. 若显著热，设计“非阻塞提交 + task wait queue + IRQ completion”的单硬件队列原型；请求等待
   期间不得持有会让同 CPU 新任务无限自旋的 `spin::Mutex`。
4. 原型必须保留同步 `BlockDevice` 上层语义，但内部允许多个 in-flight request；先 RV，LA 仍须
   能构建并保留轮询 fallback，避免线上兼容性回退。

## 验证约束

- profiling 只作诊断，不作为墙钟成绩。
- 架构原型先做短启动/文件 I/O 回归，再做一次 candidate/main BuildStorm A/B。
- 首次明确有效即停止；不明确最多补一次 matched 对照。
- 只有 RV/LA `make all` 均通过、脚本正文打印仍在、BuildStorm marker 完整且性能明确改善才合入 main。

