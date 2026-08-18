# NS16550 / DW APB UART 实现手册

本 crate 提供 WaterOS 共用的轮询 MMIO UART 实现。字符设备总览见
[driver-character](../../README.md)，稳定接口见
[character-api](../../character-api/api-v0/README.md)，平台注册实例见
[RISC-V virt driver](../../../driver-impl/impl-qemu-riscv64-virt/README.md)。

## 1. 适用范围与限制

当前覆盖两种寄存器布局：

| `RegisterLayout` | 寄存器步长/访问宽度 | 已知用途 |
| --- | --- | --- |
| `Byte16550` | index 即字节偏移，8-bit volatile | QEMU RV/LA virt、龙芯 2K1000 |
| `DwApb32` | `index << 2`，32-bit volatile 后取低 8 位 | JH7110 / DW APB UART |

驱动假定固件已完成时钟、pinmux、baud divisor、word length 等线参数配置；`init_minimal`
只把 IER 写 0，关闭 UART 中断。它没有 IRQ/RX ring buffer、DMA、错误状态统计或电源管理，
收发都由调用 CPU 轮询。

`DwApb32` 当前只保证布局和编译正确，真机仍需验证 MMIO endian、busy/USR 约束及具体 IP
变体。不能仅凭 compatible 名称假定所有 16550 clone 都能用这两种布局。

## 2. 数据结构

```rust
pub struct Ns16550Port {
    base: usize,
    layout: RegisterLayout,
}
```

`base` 必须是内核可直接 volatile 访问的 MMIO 地址，可能是恒等映射，也可能是平台建立的
kernel VA；本 crate 不执行 ioremap、不验证范围，也不拥有该映射。`Ns16550Port` 是 Copy
寄存器句柄，但同一硬件不应被多个未协调的实例并发访问。

寄存器序号固定为 THR/RBR=0、IER=1、LSR=5。LSR bit0 表示 data ready，bit5 表示 THR
empty。DLAB 由固件保持清零；若其它代码把 LCR.DLAB 置 1，index 0/1 会变成 divisor latch，
本驱动将读写错误寄存器。

## 3. I/O 调用链

引导注册：

```text
平台枚举 DTB / 固定板级设备
  -> 判断 ns16550a/ns8250/snps,dw-apb-uart
  -> 选择 base + RegisterLayout
  -> register_uart_character_device(base, layout)
     -> Ns16550Port::new
     -> init_minimal: IER=0
     -> SerialPortCharacterDevice::new(port)
     -> Arc<Mutex<Box<dyn CharacterDevice>>>
     -> 全局 register_character_device，返回 index
  -> devfs 按 index 建立字符设备节点
```

写路径：VFS/user-copy 在锁外准备内核字节 → 设备 mutex → wrapper `write_all` → 每字节读取
LSR，等待 THRE → 写 THR。每个字节最多自旋 1,000,000 次，仍不可写返回
`TransmitterStuck`，wrapper 映射为 `DriverError::IoError`。

读路径：wrapper 的 `prepare_read` 持设备 mutex 调用 `try_read_byte`，LSR.DR 未置位即停止；
取得的字节进入 reservation，释放设备锁后才 user-copy；`finish_read` 提交成功前缀并按原顺序
恢复未复制后缀。裸 `read_byte_blocking` 会无限自旋，不能在持其它 spin lock、关中断或必须
响应调度的路径使用。

## 4. 锁、并发和内存顺序

volatile 保证编译器执行 MMIO 访问，但不是跨 CPU 的设备所有权协议。正常路径通过全局注册表
返回的 `Arc<Mutex<Box<dyn CharacterDevice>>>` 串行化同一 UART。平台 early-console 若绕开该
mutex 与正式字符设备并发输出，字节可能交错。

注册表锁只在插入/取句柄时持有，设备 I/O 不应持注册表锁。设备 mutex 内不能睡眠或 user-copy；
当前 TX busy wait 可能持续较久，这是轮询实现的既有限制。若改 IRQ 驱动，应把硬件锁、RX/TX
队列锁与 task waitqueue 分层，并在解锁后唤醒。

## 5. 新增平台支持实例

为新板增加 UART：

1. 从 DTB `reg/reg-shift/reg-io-width/compatible/clock-frequency/current-speed` 核对布局；
2. 由平台 MM 子系统建立可访问 MMIO 映射，确认 base 的地址属性；
3. 只有完全匹配现有布局才复用 enum，否则新增明确 layout/quirk，不要在平台代码散落偏移；
4. 在平台枚举中调用 `register_uart_character_device(base, layout)`；
5. 确保同一 DT 节点不会被 runtime console 与 character probe 重复注册；
6. devfs 检查 `/dev/ttyS*`/console alias 指向预期 index；
7. 验证 TX、RX、空读 poll、EFAULT 回滚、大量输出及 SMP 并发。

若要设置 baud，应增加显式配置结构与“等待 TX idle → DLAB/divisor → LCR 恢复”的事务，
并处理 DW APB busy 检测；不要悄悄把它塞进 `new()`。

## 6. 常见故障

- 一写即卡住：base 未映射、布局/访问宽度错误、时钟未开或 LSR 偏移错误；
- 输出乱码：固件 baud/clock/word length 不匹配，不是字节队列问题；
- 输入偶发丢失：纯轮询无硬件 FIFO drain/IRQ，CPU 未及时读取；
- poll 后 read 无数据：并发消费者可能先取走，调用层仍必须重试；
- 启动日志重复/交错：early console 与注册字符设备同时访问同一 UART；
- DW APB 真机 data abort：检查 MMIO 32-bit 对齐、映射属性和 IP compatible。

## 7. 回归清单

运行 host 单元测试确认两种布局偏移；平台 check 覆盖 RV/LA。真机/QEMU 还应检查空输入不会
伪报 `POLLIN`、事务读 fault 不丢字节、TX stuck 能返回错误、串口 index/devfs alias 稳定，
并在高频 klog 与用户写并发时观察是否死锁。

