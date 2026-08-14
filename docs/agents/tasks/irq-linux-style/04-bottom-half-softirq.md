# T04：bottom-half / softirq + irqaction 线程化

## 任务内容

引入 Linux softirq/threaded_irq 语义的 bottom-half：ISR 只 claim/ack/标记/记录，
真正回收 virtqueue 与唤醒等待任务的工作在可调度的上下文执行。禁止在 ISR 内 sleep、
拿阻塞锁或直接调度。

## 实施方案

1. 在 `components/wateros-irq` 增加 bottom-half 原语：
   - per-hart 或 per-device pending 位 + 待处理列表；
   - `schedule_bottom_half(dev_id)` 只做标记 + 唤醒；真正处理在 kernel task 或现有
     `TaskNotify`/调度路径中执行。
2. `IrqAction` 增加 top-half（ISR）与 bottom-half（可调度）两个回调；top-half 默认
   只 ack + 记录 generation + 标记 bottom-half。
3. 复用现有 `task::WakeQueue`/`task::wake_task` 与 `IpiKind::TaskNotify` 完成跨核唤醒。
4. 保持 timer/IPI 路径不变，确保回归。

## 涉及文件

- `os/components/wateros-irq/src/bottom_half.rs`（新增，或并入 `lib.rs`）
- `os/components/wateros-irq/src/lib.rs`
- `os/src/trap_handler.rs`（外部中断分支改为调度 bottom-half）
- `os/components/wateros-task/src/schedule.rs`（必要时暴露唤醒入口）

## CodeGraph 查询命令

```bash
codegraph explore "wake_task TaskNotify schedule_reschedule IpiKind"
codegraph callers wake_task
codegraph explore "SupervisiorExternel"
```

## 验收方式

- 静态：双架构 check。
- 单测：用一个虚拟 irq 自测「ISR 只标记 → bottom-half 被调度并完成」。
- QEMU smoke：timer/IPI/外部中断路径不回归，无死锁、无丢失唤醒。

## 验收命令

```bash
cd /home/zhitian/project/WaterOS_irq_linux_style/os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
```

## 任务简报

完成后写 `history/04-bottom-half-softirq.md`。
