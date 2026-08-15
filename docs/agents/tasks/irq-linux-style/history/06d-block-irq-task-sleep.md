# T06-d 任务简报：块 IRQ 等待改任务睡眠（boot 阶段保留自旋回退）

- 完成日期：2026-08-15
- commit：`703c3abe`（`[feat] 块 IRQ 等待改任务睡眠（boot 阶段保留自旋回退，默认关闭）`）
- 前置：`02fd912f`（T07-c SharedRwFs 读写锁）、`c5b006c1`（T07-b ext4 缓存锁拆分）

## 目标与结论

把 T06 的「自旋 + 定期 drain」等待升级为 Linux 风格的任务睡眠：IRQ top-half ack
后调度 bottom-half 回收 used ring，等待任务在 waitqueue 上睡眠，被唤醒后取回
结果。**该路径基础设施已入库但默认关闭**（`BLOCK_IRQ_MODE_ENABLED=false`）：
在用户态全量负载（cagent）下 IRQ 模式仍会冻结（证据见下），根因未定位前保持
同步路径正确性——本分支同步路径已通过 cagent 10/10 + buildstorm `exit_code=0`
的完整回归。

## 实际改动摘要

`impl-virtio-mmio`：

- `VirtioBlkDevice` 新增每槽位完成标志 `slots_done: [AtomicBool; ASYNC_SLOTS]`：
  `wait_current_while_*` 在调度器临界区内复查条件，不能再抢 `async_state`
  Mutex（否则与持设备锁的 bottom-half 形成 async_state ↔ scheduler 成环死锁）。
- 等待队列 `wait_queue: Once<WaitQueue>` **惰性分配**：驱动在 `task::init()` 之前
  的 boot 阶段实例化（`from_mmio`），构造期分配 scheduler waitqueue 会写未初始化
  的调度器状态，早期表现为 `runqueue.rs:97` 的垃圾 cpu_id OOB panic。
- `wait_irq_completion` 双路径：boot 阶段（`task::in_boot_context()`）沿用 T06
  已验证的「仅外部中断 + 自旋 + 定期 drain」；真实任务上下文走
  `wait_current_while_for_ticks(2, …)` 睡眠，超时后直接 drain 自愈。
- `irq_bottom_half` 在锁外 `wake_all`（避免 async_state → scheduler 锁序）；
  队列未分配时跳过 wake——`wait_current_while` 会在调度器临界区内复查完成条件，
  不丢完成事件。

`task` 门面：

- `scheduler-impl` 新增 `current_in_boot_context()`、`wateros-task` 新增
  `in_boot_context()`：引导上下文只是调度器的 idle 占位，运行在启动栈上，
  `schedule_wait` 会把它当普通任务入队并切走启动栈（这正是最初 panic 的根因
  之一），需要睡眠的调用方应退化为自旋/轮询。

平台接线（RISC-V `register.rs`）：

- 修复 `blk_irq_top` 把 `dev_id` 当 MMIO 基址读 `0x60` 的隐患：新增
  `BLK_IRQ_BASE` 原子表按块设备索引取恒等映射基址做无锁 ack。
- 注册改为 `request_irq_with_bottom_half`：top-half ack 后由 bh 内核任务回收
  used ring 并唤醒等待队列。
- `BLOCK_IRQ_MODE_ENABLED` 默认 `false`。

## 调试过程与冻结证据

1. **首个 panic**（开启 IRQ + 睡眠等待）：`runqueue.rs:97 index out of bounds:
   len=8 index=2173265168`。根因有两个，均已修复：
   - `WaitQueue::new_named` 在 `from_mmio`（早于 `task::init()`）分配 → 惰性化；
   - boot 上下文经 waitqueue 阻塞 → `in_boot_context()` 时退化为自旋路径。
2. **用户态冻结**（boot 读盘正常、cagent 卡死、无 testcase 输出）。加埋点后
   `[virtio-blk-irq]` 日志显示：**IRQ 链路在用户态工作正常**——submit → top
   fired（`isr=0x1`）→ bh complete → `wait ret=Woken` 反复成功数千次，随后某个
   请求 `submit + wait begin` 后再无 top/bh/ret 日志。
3. **GDB 抓冻结现场**（`qemu -s` + gdb-multiarch）：8 个 hart 中 7 个（0,1,2,3,4,6,7）
   停在 `__wateros_idle_task_runtime_main` 的 WFI，`scause=5`（timer），
   `sip=0`；dashboard 显示这些 CPU 的 `Timer` 计数停在 ~182..203 不再增长，
   仅 BSP/timekeeper（CPU 2）继续 tick；`SYSCALL total` 冻结、所有用户任务无进展。
   即：block IRQ 投递后各 AP 的**定时器停止投递、全部落入 idle WFI**，全局冻结。
4. 该冻结与 T06b 观察（用户态 IRQ 模式偶发卡死、`isr=1/pending=0`）同属一类：
   问题在 block IRQ 与 trap/timer/idle 的交互，而非等待策略本身。自旋路径与
   睡眠路径在用户态都会触发，故不作为本轮修复目标，另开专项任务定位
   （候选方向：event_idx 通知抑制、PLIC claim/complete 与 timer/soft 中断的
   投递互扰、idle WFI 在 IRQ 模式下的恢复路径）。

## 验证结果

- 静态：`make rv_check && make la_check` 通过。
- 单测：`wateros-irq` 2 passed、`impl-block-cache` 11 passed、
  `another_ext4 --features block_cache` 4 passed。
- QEMU 功能（同步路径，`BLOCK_IRQ_MODE_ENABLED=false`）：
  - cagent-glibc：10/10 `testcase cagent … pass`，
    `command succeeded program=/glibc/cagent_testcode.sh exit_code=0`；
  - buildstorm-glibc：`BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=635.41
    cores=8`，`command succeeded … exit_code=0`。
- IRQ 模式（开启）仅限调试：boot 读盘/根卷挂载正常；用户态冻结如上。

## 未验证项 / 剩余风险

- IRQ 模式用户态冻结根因未定位（专项任务）。
- LoongArch 本任务未做 QEMU 验证（无 IRQ 模式接线变更之外的影响；仅 check）。
- 开启 IRQ 开关的任何后续验证必须确认 cagent 全量通过后才能提交合并。

## 验收命令

```bash
cd /home/zhitian/project/WaterOS_irq_linux_style/os
make rv_check && make la_check
make kernel-rv
# QEMU 9.2.1 + -snapshot + pub 镜像跑 cagent/buildstorm，断言
# "command succeeded … exit_code=0" 与全部 testcase pass
```
