# T06 任务简报：块设备 IRQ 完成路径基础设施

- 完成日期：2026-08-15
- commit：`640145d6`（`[feat] 块设备 IRQ 完成路径基础设施（实验开关默认关闭）`）
- 前置：`7bb844ec`（T05 BlockDevice `&self`）

## 实际改动摘要

`wateros-irq`：

- 新增 `wait.rs`：`with_external_only_interrupts` 等待窗口（保存状态 → 屏蔽
  timer/soft → 开全局中断 → 等待 → 恢复），避免持 FS 自旋锁等待时被定时器抢占。
- `bottom_half.rs`：bh 任务等待改为 1 tick 超时守卫等待（偶发唤醒丢失兜底）。

`block-api`：

- `BlockDevice` 增加 `enable_irq` / `irq_bottom_half` 默认方法；块缓存透传两者。

`impl-virtio-mmio`：

- `VirtioBlkDevice` 增加异步槽位（8 个，请求/响应/数据缓冲自持）与 `irq_mode`：
  - `submit_async`：`read_blocks_nb/write_blocks_nb` 提交，锁序 state→inner；
  - `irq_bottom_half`：ack + 按 used-ring 顺序 `complete_*`，置 Done；
  - 等待：`with_external_only_interrupts` 窗口内自旋，并每 ~1M spins 定期 drain
    used ring 作为自愈兜底（IRQ 是快速路径，drain 保证不丢事件）。
- `read_blocks/write_blocks` 按 `irq_mode` 选择 IRQ 路径或同步回退。

平台接线（RISC-V）：

- `plic.rs`：每 CPU 一个 `PlicChip`（`PLIC_CHIPS`），`register_device_line` 按当前
  CPU 绑定线路并使能（修复 boot hart 不固定的问题）。
- `external_irq.rs`：`register_device_line` 门面。
- `register.rs`：IRQ top-half 用无锁原始 MMIO ack 设备 ISR（`dev_id`=MMIO 基址），
  不 mask、不依赖 bh 任务；注册动作与 `enable_irq` 置于 `BLOCK_IRQ_MODE_ENABLED`
  开关（默认 `false`）。

## 验证结果

- `make rv_check` / `make la_check`：通过。
- `wateros-irq` 2 tests、block-cache 10 tests：通过。
- `git diff --check`：干净。
- 开启开关的调试构建（QEMU 9.2.1 + `-snapshot`）：
  - IRQ 注册成功（`hwirq=1 virq=Virq(0)`）；
  - **boot 阶段（FS 探测、ext4 根卷挂载）IRQ 读盘可工作**：claim → top-half ack →
    等待者 drain 完成，多轮验证通过；
  - **用户态全量负载（cagent 模型加载）下偶发卡死**：root 定位到 PLIC enable 位
    被 top-half mask 后 bh 任务唤醒丢失、掩码不复位的竞态；改为无 mask + 等待者
    自愈 drain 后仍存在用户态卡死（疑似 syscall 上下文与等待窗口/PLIC 投递交互，
    诊断日志会显著改变时序，呈 Heisenbug）。

## 结论与下一步

- 因「不能有任何 bug」的验收门槛，IRQ 模式默认关闭（`BLOCK_IRQ_MODE_ENABLED=false`），
  内核保持 T05 同步路径行为；基础设施全部入库，可翻转开关继续调试。
- 下一步（T06b / 并入 T07 前）：专项调试用户态卡死——重点排查 syscall 上下文
  中 `with_external_only_interrupts` 窗口与 PLIC 投递、used_event（event_idx）通知
  在长时间大量读后的失效；完成后按计划进入 T07（ext4/block-cache/VFS 锁拆分 +
  任务睡眠）。
- 回归 smoke（同步路径 cagent）待后台用户 QEMU 结束后补跑。

## T06b 追加：stall-debug 定位（2026-08-15）

用 `stall-debug` feature + IRQ 开关打开跑了复现，关键证据：

- 停滞时 `[stall-debug] no syscall progress for 1500 ticks`；卡住的是 cagent 子
  任务（id=14/15，User/Running）的 read() syscall，`ticks=0`（等待窗口屏蔽了
  timer，任务无法被抢占/计 tick），父任务阻塞在 ChildExit 等待它们；
- 其它 CPU 计时器正常（3425..5326），bh 任务正常阻塞在 waitqueue；
- 结论：不是锁结构死锁，而是**设备侧不再完成请求**（used ring 为空 → 等待者
  自旋永不完成）；结合此前 `isr=1 / pending=0` 的观测，方向指向 virtio
  `event_idx` 的 avail/used 通知在长时间大量提交后失效（`should_notify` 判定或
  `used_event` 阈值）。

下一轮修复候选（按优先级）：

1. IRQ 路径禁用 `VIRTIO_F_RING_EVENT_IDX`（走 avail flags 通知 + `set_dev_notify`
   控制），验证长跑不再丢通知；
2. 若保留 event_idx：每次提交强制 `transport.notify`（跳过 `should_notify`
   抑制）作为自愈；
3. 在等待自旋里加「设备 ISR/used ring 长时间无进展」的探针日志，确认丢通知
   的确切位置。
