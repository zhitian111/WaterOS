# T10：input/display/GPU 中断化（第二阶段）

## 任务内容

把 input（键盘/鼠标/平板）、display/GPU 的轮询改为 IRQ 唤醒，减少空转与延迟。
这些设备不在 BuildStorm 吞吐热路径，重点是可用性与延迟，而非编译性能。

## 实施方案

1. `VirtioInput*Device` / `VirtioGpu*Device` 接入 `ack_interrupt/enable_interrupts`，
   事件读取改为「IRQ/bottom-half 唤醒 + 非阻塞 drain」。
2. `os/src/main.rs` 的 GUI/输入 poll 任务与 `os/src/user_operator.rs` 的
   `poll_console_input_once` 等轮询点改为等待事件唤醒。
3. 保持无事件时睡眠；UART 控制台读写仍保留同步（启动日志安全）。
4. RISC-V MMIO 与 LoongArch PCI 都接入，先功能后优化。

## 涉及文件

- `os/components/wateros-driver/driver-input/input-impl/impl-virtio-{mmio,pci}/src/lib.rs`
- `os/components/wateros-driver/driver-display/display-impl/impl-virtio-{mmio,pci}/src/lib.rs`
- `os/src/main.rs`
- `os/src/user_operator.rs`

## CodeGraph 查询命令

```bash
codegraph explore "VirtioInputMmioDevice VirtioInputPciDevice"
codegraph explore "VirtioGpuMmioDevice VirtioGpuPciDevice ack_interrupt"
codegraph explore "poll_event poll_console_input_once"
```

## 验收方式

- 静态：双架构 check。
- 功能：输入事件、显示刷新、GUI/console 交互不回归；无事件时 CPU 空转下降。
- 回归：不破坏 tty/console 输入路径。

## 验收命令

```bash
cd /home/zhitian/project/WaterOS_irq_linux_style/os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
```

## 任务简报

完成后写 `history/10-input-display-irq.md`。
