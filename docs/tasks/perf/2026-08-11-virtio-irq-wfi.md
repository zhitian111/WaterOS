# VirtIO block IRQ + WFI 等待方案（2026-08-11）

## 为什么选择这里

300 秒 pc-hot 中 RISC-V block 侧
`VirtQueue::add_notify_wait_pop` 约 `1.44B` 条指令，其中约 97% 集中在：

```text
while !self.can_pop() {
    spin_loop();
}
```

这是 QEMU VirtIO 请求完成前的同步忙等。BuildStorm 大量文件读写的等待时间被这
段 poll 放大。Linux 使用 IRQ 完成路径；WaterOS 当前把 RISC-V `SupervisiorExternel`
当作未处理 trap。

## 选择的方案

第一版只替换“忙轮询等待完成”为“IRQ 辅助的 WFI 等待”，不引入完整异步请求表：

1. 新增 QEMU PLIC 的 S-mode enable/claim/complete 最小封装。
2. RISC-V arch 增加 `sie.SEIE` 开关，启动时 BSP/AP 打开外部中断。
3. trap handler 处理 `SupervisiorExternel`：从 PLIC claim IRQ 后立即 complete，
   不在中断上下文中触碰块设备。
4. `VirtioBlkDevice` 新增 `read_blocks_irq` / `write_blocks_irq`，使用
   `read_blocks_nb` / `write_blocks_nb` 提交请求，然后循环 `peek_used()`；
   未完成时临时打开全局中断并执行 WFI。
5. 仍持有 `SharedBlockDevice` 锁，因此 queue depth 保持 1；WFI 只消除同核忙等，
   不并发提交请求。

## 为什么这么做

1. 这是向 IRQ 完成路径迁移的最小第一步，不改变块设备 API、文件系统调用链或锁模型。
2. 不需要一次性实现异步请求表、DMA buffer pin 和任务等待队列，降低正确性风险。
3. 若完整 BuildStorm 证明 IRQ/WFI 有收益，再继续做真正的多请求在途和任务睡眠。

## 接下来的工作

1. 在 `perf/virtio-irq-wfi` 分支实现 PLIC、`sie.SEIE`、trap 外部中断分支和
   VirtIO IRQ 等待。
2. 双架构 Final check；LoongArch 保持同步 API 不变，只保证 check 通过。
3. 180 秒 smoke，重点确认块 I/O、PLIC claim/complete 和 WFI 不 hang。
4. RISC-V 完整 BuildStorm A/B，相对当前 main 有 ≥ 1.5% 净改善才合并。
5. 完成后跑 pc-hot/wait-hot 并归档。

## 验收标准

- 双架构 Final check 通过。
- 普通启动、根文件系统挂载、BuildStorm 无 panic/SIGSEGV/停滞。
- 完整 BuildStorm 相对当前 main 有可复现收益。

## 实测结果（2026-08-11）

双架构 Final check 通过；PLIC MMIO 映射、`sie.SEIE`、外部中断 trap 分支和
`read_blocks_nb`/`write_blocks_nb` + WFI 等待均已实现，但 180 秒 smoke 在第一次
块读取 `lba=2` 后停滞：

```text
[WARN] [virtio-irq] read start lba=2
（之后无 PLIC claim、无后续 FS 初始化输出，QEMU 一直停在 WFI）
```

设备没有进入外部中断处理，说明 VirtIO MMIO 的 IRQ 通知还需要与 transport
interrupt status/ack、队列 used-buffer notification 和 PLIC 路由做完整生命周期
接线，不能仅靠 `set_dev_notify(true)` 加 WFI 完成。所有实现已回退，仅保留本记录。

后续若继续该方向，应先确认 VirtIO MMIO ISR/ack 在提交后确实置位，再实现真正的
设备 IRQ handler 和请求等待队列，而不是在同步路径上依赖 WFI。
