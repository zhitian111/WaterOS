# runtime-panic

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-runtime](../README.md)

这个 crate 提供 WaterOS 的不可恢复 panic 终点。它只做三件事：尽力打印现场、尽力
flush early console、反复请求平台关机。它不展开栈、不杀单个任务、不恢复调度，也不做
文件系统写回。

## 1. 精确调用链

```text
Rust #[panic_handler]
  -> runtime::panic::panic_handler(PanicInfo)
     -> PanicInfo::location()
     -> console::println!(颜色、PANIC、文件:行、message)
     -> platform::console::console_flush()
     -> while shutdown(SystemFailure) 返回 Err：再次调用 shutdown
     -> 若某次 shutdown 反常返回 Ok：进入 loop {}
```

有源码位置时格式为 `Panicked at file:line. message`；无位置时只输出 panic 与 message。
使用 `format_args!`/`PanicInfo` 借用，不需要 heap。控制台和 flush 的错误均被忽略，确保
输出失败不会触发第二次 panic 掩盖原始故障。

## 2. 依赖和 feature

crate 默认 feature 为空。`impl-platform-console` 同时打开 runtime console 的 platform
实现和 platform API；顶层 `wateros-runtime` 的同名 feature 会向下透传。若未启用控制台
实现，`console::println!` 是 no-op，但 platform reset 仍必须由最终板级配置提供。

panic handler 可在 heap、task、VFS 之前使用，因此新增代码严禁：

- `Box`、`Vec`、`String` 或任何可能分配的格式化辅助；
- 睡眠、等待队列、文件 fd、文件系统写回或网络发送；
- 获取 scheduler、MM、allocator、VFS 等业务锁；
- 用普通 logger 报告 panic 输出失败；
- 从 handler 返回或尝试继续执行发生 panic 的上下文。

## 3. 控制台锁的真实边界

“不主动获取业务锁”不等于完全无阻塞。`console::println!` 最终进入 platform 的跨 CPU
自旋锁：

- 同一 CPU 已持有 console 锁时，owner 检测绕过重入加锁并走 early board UART；
- 另一 CPU 正持锁时，本 CPU 会自旋等待；若持锁 CPU 已永久卡死，panic 文本可能永远
  无法完成；
- RISC-V early UART 有有限轮询，LoongArch64 early UART/flush 当前可能无限轮询；
- `console_flush()` 直接走 board 后端，不经过已注册 runtime writer，也不持聚合 console
  锁。

因此 panic 文本是 best-effort，终止语义不能依赖它成功。若要增强极端故障可见性，应
设计独立的 `panic_write_raw`：try-lock 失败就绕过运行期 writer、使用有上限的 early
MMIO 轮询；不能在现有普通输出 API 中静默改变同步语义。

## 4. reset 行为

`shutdown(PlatformResetReason::SystemFailure)` 的平台约定是成功时机器停止、不会返回。
目前 RISC-V OpenSBI 路径在 SBI 调用返回后统一给 `Err(Failed)`；LoongArch64 MMIO reset
写入后也返回错误。因此 `while let Err(...)` 会持续重试，直到 QEMU/固件真正终止虚机。

若未来某实现返回 `Ok(())` 却没有停机，后面的 `loop {}` 保证 `-> !` 且不会误回到损坏
上下文。该空循环目前没有显式 `wfi`，会占用一个 vCPU；如改成架构 halt 指令，必须保证
中断不会让 panic CPU 恢复正常执行。

## 5. 生命周期与二次 panic

panic 可能发生在以下任意阶段：early MMIO 尚不可用、allocator 锁内、console formatter
内部、SMP 已启动或关机驱动本身故障。handler 不知道原先持有哪些锁，也不会释放它们。
任何新增的 `Display` 实现都应只读取 `PanicInfo`/静态数据，不能加锁和分配。

Rust 内核构建应保持 `panic=abort`/单一 handler。不要在 panic handler 内再次 `panic!`、
`unwrap` 或索引未经验证的数据；二次 panic 通常只会丢失第一现场或直接 abort。

## 6. 新增 panic 原因码实例

若需要让 QEMU 测试区分普通关机与内核失败，应扩展 platform 的 reset reason 映射，而
不是把状态写入 VFS：

1. 在 platform API 增加/复用静态枚举值；
2. 两个 board 映射到各自 firmware/MMIO 能表达的 reason；
3. handler 仍只传栈上枚举，不分配；
4. QEMU 自动化同时验证输出可能缺失时，退出状态仍能判定失败；
5. firmware 不支持细分 reason 时保留 `SystemFailure` 降级。

## 7. 自回归矩阵

- heap 初始化前主动 panic，能关机且不发生链接/分配依赖；
- 带/不带 `location` 的格式路径（后者可用单元级辅助 formatter 覆盖）；
- console 未启用、UART 不 ready、flush 失败时仍进入 shutdown；
- 同 CPU console 重入不自锁；另一 CPU 持锁是当前已知风险，应有超时测试或明确标记；
- shutdown 返回 `Err` 时重试，反常返回 `Ok` 时不返回调用方；
- RV OpenSBI 和 LA MMIO reset 都由 QEMU 观测到退出；
- panic 信息中的长文件名/Unicode 不触发堆分配。

```sh
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```
