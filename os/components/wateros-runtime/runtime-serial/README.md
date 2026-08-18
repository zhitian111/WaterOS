# runtime-serial

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-runtime](../README.md)

这个 crate 是一个很薄的运行期串口再导出层，供伪 shell 或驱动 bring-up 在 UART 字符
设备注册后读写串口。它不实现 UART、不管理全局实例，也不是内核日志通道。

## 1. 边界与依赖

```text
runtime::serial（启用 serial-uart-virt）
  -> runtime-serial
     -> driver::uart 的 QEMU RISC-V virt 适配
     -> driver::character 的 Ns16550Port / RegisterLayout
     -> character-api-v0 的 CharacterDevice / SerialPort / 错误类型
```

当前 `runtime-serial/Cargo.toml` 显式启用 driver 的 `impl-qemu-riscv64-virt`，导出的常量
也是 `QEMU_VIRT_UART0_BASE`。因此它目前是 RISC-V QEMU virt 辅助层，不是自动随顶层
`ARCH` 切换的跨架构门面。LoongArch64 有自己的 driver UART 实现，但没有通过本 crate
对称再导出；新增 LA 使用点前应先重构 feature，而不是误用 RISC-V 基址。

顶层 `wateros-runtime` 仅在 `serial-uart-virt` feature 开启时暴露 `runtime::serial`。
正常日志不需要也不应开启这个 feature。

## 2. 再导出 API

| 名称 | 来源/语义 |
|---|---|
| `CharacterDevice` | 字符设备统一 `read`/`write`/`ioctl` 契约 |
| `SerialPort`、`SerialError`、`SerialResult` | UART 端口级 API 与错误 |
| `Ns16550Port` | 16550 MMIO 句柄；当前假定固件已设置基本线参数 |
| `RegisterLayout` | 寄存器布局，QEMU virt 使用 `Byte16550` |
| `QEMU_VIRT_UART0_BASE` | 来自 base config 的 RISC-V QEMU virt UART0 基址 |
| `qemu_virt_default_port()` | 构造句柄，不完成注册 |
| `register_uart_character_device(base)` | 将指定基址的 Byte16550 注册到字符设备表，返回索引 |
| `with_default_uart(f)` | 短借字符设备表索引 0，未注册返回 `None` |
| `init_default_virt_uart()` | RISC-V 兼容空函数，注册后即视为 ready |
| `read_byte_blocking(dev)` | 轮询 `CharacterDevice::read` 直到得到 1 字节 |

`read_byte_blocking` 对 `Ok(0)`、所有 `Err` 和异常的 `Ok(n>1)` 都只执行 `spin_loop` 后
重试：没有 timeout、取消点、错误返回或调度器让出。它适合极简伪 shell bring-up，不适合
生产 TTY、可中断 read 或无人输入的内核任务。

## 3. 与 console/TTY 的区别

| 路径 | 时期 | 功能 | 同步 |
|---|---|---|---|
| `platform::console` | early boot 起 | best-effort 内核输出 | platform 跨 CPU console 锁 |
| `runtime-console` | 全程 | 日志/原始输出统一门面 | 转到 platform console |
| `runtime-serial` | 驱动注册后 | 直接操作字符设备，含输入 | 字符设备注册表及驱动锁 |
| TTY/PTY | task/VFS 就绪后 | 行规程、会话、阻塞/唤醒、termios | TTY 状态锁与 waitqueue |

early console 和字符设备可能指向同一物理 UART。不得一边持字符设备锁一边调用 logger，
也不得绕过两条既有路径直接 MMIO，否则可能发生字节交错或锁反转。正式用户态 stdin/
stdout 应走 TTY/VFS，而不是让 syscall 直接调用 `read_byte_blocking`。

## 4. 初始化和使用实例

RISC-V QEMU virt 的最小顺序：

```text
platform early UART 已可用于日志
  -> driver registry 初始化
  -> register_uart_character_device(dtb_uart_base)
  -> with_default_uart(|dev| ...)
```

伪 shell 单字节读取示例：

```rust
runtime::serial::with_default_uart(|dev| {
    let byte = runtime::serial::read_byte_blocking(dev);
    // 在闭包内处理；不要保存借用到注册表之外。
});
```

先处理 `Option::None`；不要 `unwrap` 假定设备编号 0 存在。真实 DTB 扫描若先注册了别的
字符设备，“索引 0 即 UART”也可能失效，应按设备 kind/注册句柄查找后再扩展 API。

## 5. 扩展为跨架构门面

若比赛题要求 LA 也通过 runtime serial 使用，建议：

1. 给 `runtime-serial` 增加互斥的 `impl-riscv64`/`impl-loongarch64` feature；
2. 两个 feature 分别映射到 driver 的对应 board 实现；
3. 统一函数签名，注意当前 RISC-V `register_uart_character_device(base)` 与 LA 无参数版本
   不一致；
4. 用架构中立名称再导出默认基址/布局，或明确只暴露查询函数；
5. 顶层 runtime 的架构 feature 透传，禁止同时选中两个 board；
6. 两个架构分别做 `make check` 和 QEMU 输入输出测试。

## 6. 自回归

- 未注册时 `with_default_uart` 返回 `None`；
- 注册一次后能读写，索引/设备 kind 正确；
- 连续输入、无输入、UART 错误分别验证当前阻塞语义；
- console 日志与 serial 输入并发时无死锁，输出按既定锁边界串行；
- 不启用 `serial-uart-virt` 时顶层 runtime 不拉入 driver board；
- RV/LA 编译检查确保没有错误基址意外进入另一架构。

```sh
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```
