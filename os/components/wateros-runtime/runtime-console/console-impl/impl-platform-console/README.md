# Platform Console 桥接实现离线手册

[runtime-console 总览](../../README.md) · [Console API v0](../../console-api/api-v0/README.md) · [Platform API](../../../../wateros-platform/platform-api/api-v0/README.md)

本 crate 是很薄但关键的依赖反转层：runtime 不直接选择 RISC-V UART、LoongArch UART或
运行期字符设备，而是把输出交给 `wateros-platform::console`。所有函数采用 best-effort，
当前都丢弃 platform error。

## 1. 入口对照

| 函数 | 下游 | 锁粒度/用途 |
| --- | --- | --- |
| `platform_console_write_a_byte` | `console_write_a_byte` | 单字节一次锁；只用于兼容/低层路径 |
| `platform_console_write_a_buffer` | `console_write_a_buffer` | 整个 UTF-8/普通 buffer；后端可做换行规范化 |
| `platform_console_write_raw_buffer` | `console_write_raw_buffer` | wire bytes 原样输出；stdout/TTY syscall |
| `platform_console_write_fmt` | `console_write_fmt` | 整次 `fmt::Arguments` 在同一 platform 锁内 |

日志必须优先用 fmt 入口；循环调用 byte 会把一行拆成大量锁和 MMIO轮询。raw 与 buffer
不能互换：raw 中的 `\n` 不应被自动扩成 CRLF，任意 0xff 也不要求 UTF-8。

## 2. `PlatformConsoleHandle`

```rust
#[derive(Default)]
pub struct PlatformConsoleHandle;
```

它是零大小、无本地状态的 `fmt::Write + api_v0::Console` 句柄。`write_str` 把完整 `&str`
转为 bytes 调 buffer 入口，然后无条件返回 `Ok(())`。

因此：

- UART timeout/Unavailable 不会成为 `fmt::Error`；上层只能看到“格式化成功”；
- `write!(&mut handle, ...)` 的默认 formatter 可能多次调用 `write_str`，每次单独加 platform
  锁，整条记录可能交错；runtime 根的 `write_fmt(format_args!(...))` 才保证一次锁；
- 构造 handle 不初始化设备，实际 backend 由 platform boot 状态决定；
- handle Drop 没有副作用，不能注销 writer。

## 3. 下游状态机

```text
调用桥接函数
  -> platform::console 获取/递归检测输出锁
  -> runtime writer 已注册？
       否：board early console
       是：已注册字符设备 writer
  -> 同 CPU递归：绕开 runtime writer，回退 early console
  -> platform 返回 Result
  -> 本桥接丢弃 Result
```

跨 CPU锁、关中断、owner 检测和 writer 注册的完整规则见 runtime-console 总览与
`wateros-platform/src/console.rs`。本 crate 不应再加第二把输出锁，否则递归路径更容易
死锁。

## 4. 错误、panic 与锁序

丢弃错误是为了避免“记录日志失败又触发 panic/日志”的递归灾难，不表示输出可靠。
需要统计错误时应在 platform 层用原子计数，恢复后再读取；不能在本函数的 Err 分支再次
调用 console。

调用方不得持 scheduler、heap、VFS、地址空间或设备高层锁进入 console。运行期 writer
可能反向进入字符设备/TTY锁。panic 时另一 CPU若永久持有 platform console lock，当前
CPU仍可能自旋；本桥接没有强制抢锁机制。

## 5. 新增功能实例：显式可观测写

若某诊断命令需要知道写失败，不要改变现有 `Console` trait 行为；可在此 crate 添加一个
清晰命名的低层入口并原样返回 platform result：

```rust
pub fn try_platform_console_write_raw(bytes: &[u8])
    -> platform::console::PlatformConsoleResult<()> {
    platform::console::console_write_raw_buffer(bytes)
}
```

再由受控诊断调用者使用。不要让普通 logger 在错误时无限重试，也不要把
`fmt::Arguments` 存入异步队列：它只在当前调用期间有效。

## 6. 自回归

- 注册 runtime writer 前/后的 buffer、raw、fmt 路径；
- buffer 换行和 raw 不转换；
- Unicode、多段 formatter、空 buffer；
- 模拟 platform error，确认 handle 仍返回 `fmt::Ok`；
- 多 CPU完整 fmt 不交错，直接 `write!(&mut handle)` 的边界符合文档；
- runtime writer 内递归输出回退 early backend；
- panic、关中断、设备未 ready 三种场景。

```bash
cd os
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```
