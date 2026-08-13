# wateros-driver-character

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [系统架构](../../../../README.md#系统架构)

`wateros-driver-character` 是 WaterOS 的字符设备子系统。它抽象“字节流 I/O + 可选
poll/ioctl”，覆盖串口、实时钟和 null 设备。驱动只负责寄存器级读写与字节流语义，不做行
规整、转义或终端处理。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | 按 feature 再导出字符 API 与具体实现，提供 `supported_devices()`、`character_subsystem_claims_device()`、`is_uart_compatible()` 与 `register_builtin_character_devices()`。 |
| 字符设备 API | `character-api/api-v0/` | `CharacterDevice`、`SerialPort`、`CharacterDeviceKind` 与全局注册表。 |
| NS16550 串口实现 | `character-impl/impl-uart-16550/` | NS16550 家族 MMIO 串口（QEMU RV/LA、龙芯 2K1000、JH7110 共用）。 |
| 实时钟实现 | `character-impl/impl-rtc-stub/` | `RtcCharacterDevice` / `RtcTime`。 |
| null 实现 | `character-impl/impl-null-stub/` | `NullCharacterDevice`。 |
| 占位实现 | `character-impl/impl-dummy/` | 无硬件占位。 |

## 实现说明

- `CharacterDevice` 只负责字节流语义；`poll_revents` 按请求掩码返回就绪事件；syscall 层负责
  Linux ABI 与用户指针复制，驱动不转换 errno。
- 可选的事务式读取：`prepare_read` 预留最多 `max_len` 字节，`finish_read` 提交复制前缀并
  恢复未消费后缀；非消费型设备保留默认 `Unsupported`。
- `SerialPort` 是串口最小 I/O 契约（写单字节、批量写、阻塞读、非阻塞读），由
  `SerialPortCharacterDevice` 包装为 `CharacterDevice`；`SerialError::TransmitterStuck`
  表示发送侧自旋后仍不可写。
- `impl-uart-16550` 统一处理两种寄存器布局：`Byte16550`（标准 16550A 字节布局）与
  `DwApb32`（DesignWare APB `reg-shift=2`、`reg-io-width=4`）。平台层只传基址与
  `RegisterLayout` 并注册，本 crate 不做波特率除数编程（假定固件已配置线参数）。
- DTB 声明支持 `ns16550a` / `ns8250`；`is_uart_compatible` 还识别 `snps,dw-apb-uart`。
- 内置 RTC / null 字符设备由 `register_builtin_character_devices` 注册。

## 调用链路

引导期注册（RISC-V 为例）：

```text
init_after_boot()
  -> probe_character_devices()
       -> character_subsystem_claims_device(compatibles, DeviceType::Character)
       -> uart::register_uart_character_device(base)
       -> register_character_device(SharedCharacterDevice)
  -> register_builtin_character_devices()   // RTC / null
```

上层访问：

```text
syscall / VFS
  -> with_character_device(...) / first_character_device()
  -> CharacterDevice::read / write / poll_revents
  -> 底层 UART 寄存器读写（Byte16550 / DwApb32）
```

## 各实现功能

### character-api / 字符设备 API

主要实现在 `character-api/api-v0/src/lib.rs`：

- `CharacterDevice` trait：`read` / `write` / `poll_revents`，以及可选
  `prepare_read` / `finish_read`（`CharacterReadReservation` / `CharacterReadFinish`）。
- `SerialPort` trait：`write_byte` / `write_all` / `read_byte_blocking` / `try_read_byte`。
- `CharacterDeviceKind`：`Serial` / `Rtc` / `Null`，供 devfs 路径别名与 syscall 分发。
- 注册表：`register_character_device`、`first_character_device`、`with_character_device`、
  `character_device_count`、`character_device_at`。

### impl-uart-16550 / NS16550 串口

- `RegisterLayout`：`Byte16550` 与 `DwApb32` 两种布局。
- `Ns16550Port`：按布局换算寄存器偏移并执行 volatile 读写。
- `register_uart_character_device(base)`：平台层调用，注册为 `CharacterDevice`。

### impl-rtc-stub / 实时钟

- `RtcCharacterDevice` / `RtcTime`：读取实时钟时间；由 `register_rtc_stub` 注册。

### impl-null-stub / null 设备

- `NullCharacterDevice`：丢弃写入、无数据的占位设备；由 `register_null_stub` 注册。

### impl-dummy / 占位实现

- 无硬件场景的占位字符设备，配合 `impl-dummy` feature 使用。
