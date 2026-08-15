# T06-e 任务简报：block 读去 FS 持锁 + virtio 完成健壮性

- 完成日期：2026-08-15
- commit：`7ff1185b`（`[feat] 块 IRQ 改 Linux 式单请求等待并修复 virtio 通知抑制（默认关闭）`）

## 结论

block IRQ 链路已从「IRQ 触发/死锁」推进到「IRQ 完成路径正常工作」，剩余问题在
页面缓存/VFS 与 IRQ 异步读的交互，而非 block 驱动本身。

## 实际改动

- `impl-virtio-mmio`：等待改为 Linux 式**单请求在途**——原子 `in_flight` 门闩 +
  `wait_current_while` 睡眠，等待期间不持设备自旋锁；IRQ top-half 只 ack，
  bottom-half 在可调度上下文 `notify_irq` 唤醒。
- `vendor/virtio-drivers/src/device/blk.rs`：`read_blocks_nb/write_blocks_nb`
  改为无条件 `transport.notify`（强制 kick），修复 event_idx 通知抑制导致设备
  不处理新请求的根因。
- `register.rs`：IRQ 开关默认 `false`；top-half ack + bh notify 接线。
- `main.rs`：全局中断就绪后 `enable_runtime_dispatch`。

## 验证

- `make rv_check` / `make la_check` 通过。
- IRQ 模式：top ack → bh notify → waiter wake 链路完整；`avail/used/last_used`
  同步推进；读回 `buf` 头部字节为真实内容（ext4 元数据、RISC-V 机器码、文本）。
- 同步路径（开关关闭）cagent 全绿（rebase 后回归）。

## 剩余问题与下一步

IRQ 模式开启时 cagent 卡在 `paged_handle seek` 循环（bash 反复 seek/重读脚本），
但 block 数据正确。下一步专项审计 `impl-page-cache` 的 `install_page` 锁外读 +
回查装填在 IRQ 异步完成下的空/旧页竞态，或做「同步 vs IRQ 同页字节」对比。

## 追加结论（2026-08-15 深挖后）

- 短读诊断未触发：`install_page` 的 `read_range` 每次返回完整长度，页面无空页。
- cagent 从「seek 卡死」推进到 `Simple LLM Server listening`（脚本解析完成），
  说明 block IRQ 完成链路**功能正确、不是死锁**，只是**非常慢**。
- 慢因：当前单请求在途（`in_flight` 原子门闩）把大量块读完全串行，且每次块读
  走 IRQ + waitqueue 睡眠/唤醒往返；同步路径是锁内紧自旋，微秒级完成。
- 结论：性能收益必须回到**多请求在途（async slots）**，而它此前被 virtio
  描述符回收损坏（重复 token / used 条目未消费）挡住，是下一个主攻点。

## 多请求在途实验（槽位自旋版）

- 恢复 8 槽位 async 结构 + 强制 kick + bh 回收 + 无超时 waitqueue，并把槽位满
  改为自旋等待空闲槽。结果 cagent 很快 `failed to read block ... IoError`（
  非槽位耗尽，而是完成路径失败），子进程 `exit_code=-11`。
- 定性为 vendor `virtio-drivers` 在「nb 提交 + 按 used 顺序回收 + 多请求在途 +
  乱序完成」下描述符 free-list 不变量被破坏，导致重复 token，drain 错槽，
  `complete_*` 返回 WrongToken。下一步在 vendor 描述符生命周期层面修复。
