# VirtIO IRQ/WFI + 提前 SEIE 方案（2026-08-11）

## 为什么再试一次

上一版 `2026-08-11-virtio-irq-wfi-ack.md` 已确认 PLIC claim、设备 ISR ack 和
VirtIO used-buffer 通知本身都能工作，但 smoke 仍在第一次根卷读取后停滞。定位结果
是 BSP 在 `bringup_driver_and_user()` 返回后才打开 `sie.SEIE`，而第一次块 I/O 发生
在 FS 探测/挂载阶段，因此 IRQ/WFI 等待永远等不到外部中断。

本轮把 `enable_external_interrupt()` 提前到驱动和 FS 初始化之前，并补上正确的
WFI 等待顺序。

## 实现

1. 增加 QEMU RISC-V PLIC 的 S-mode enable/claim/complete 最小封装，并恒等映射
   PLIC MMIO。
2. RISC-V arch 增加 `sie.SEIE` 开关，BSP 在 `bringup_driver_and_user()` 前开启；
   trap 增加 `SupervisiorExternel` 分发。
3. VirtIO-MMIO 块驱动使用 `read_blocks_nb` / `write_blocks_nb` 提交请求，中断
   handler 只 ack ISR 并发布原子完成计数。
4. 同步等待循环先关全局中断执行 WFI，醒来后再短暂开/关 SIE，让 pending 外部
   中断正常投递，避免中断在 WFI 指令前被取走导致 `sret` 后重新执行 WFI。
5. 只有 PLIC 实际使能的 BSP 使用 WFI；其他 CPU 保持 spin_loop，避免跨核 PLIC
   路由不确定导致 WFI 睡眠后无法被唤醒。

## 验证

- RISC-V / LoongArch 双架构 `make check` 通过。
- 180 秒 smoke 通过：根文件系统挂载、VFS 自检、cagent 全通过，并进入
  BuildStorm。
- 完整 BuildStorm：
  - `BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=851.58`
  - 内核 runner 总耗时约 `884.67s`
  - 当前 main 基线约 `817.27s`

## 结论

本轮实现功能上可跑通，但完整 BuildStorm 比 main 基线慢约 34s（约 4.2%），未达到
1.5% 合并门槛。所有代码改动已回退，只保留本记录。

后续若继续 IRQ 方向，需要先解决请求与 PLIC 路由的动态绑定：让当前执行块 I/O 的
CPU 成为该请求的完成目标，或引入真正的异步请求表和任务睡眠，而不是在同步
`SharedBlockDevice` 锁内做 WFI。
