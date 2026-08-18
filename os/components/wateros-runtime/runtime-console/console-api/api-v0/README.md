# Console API v0 离线开发手册

[runtime-console 总览](../../README.md) · [平台桥接实现](../../console-impl/impl-platform-console/README.md)

本 crate 只有一个稳定边界：

```rust
pub trait Console: core::fmt::Write + Default {}
```

它是类型标记，不拥有全局控制台、不选择 UART，也不提供输入、raw bytes、flush、错误
分类或注册机制。真正的跨 CPU 序列化在 `wateros-platform::console`，runtime 根 crate 的
`write_fmt/write_str/write_raw_bytes` 才是普通调用方应使用的门面。

## 1. trait 契约

实现类型必须：

- `Default`：无需外部资源参数即可构造一个写句柄；不等于构造时硬件已 ready；
- `fmt::Write`：实现 `write_str(&str) -> fmt::Result`，输入必为合法 UTF-8；
- 显式 `impl Console for Type`：表明它可被 runtime 选作 console handle。

API没有要求 `Send/Sync/Copy/Clone`。若后端句柄含可变状态，调用者不能自行跨 CPU共享；
当前 `PlatformConsoleHandle` 是无状态 ZST，同步由更下层完成。

`fmt::Write` 只允许返回 `fmt::Error`，无法携带 UART timeout、设备下线等细节。因此该
trait 适合 best-effort 日志，不适合需要确认写入/持久化的协议。

## 2. 调用链

```text
format/write!(&mut ConsoleImpl, ...)
  -> fmt::Write::write_fmt 默认实现
     -> formatter 可能多次调用 write_str
        -> 具体 Console impl

通常的 WaterOS 路径：
console::println!
  -> runtime-console::write_fmt（一次完整 Arguments）
  -> impl-platform-console
  -> platform::console（一次跨 CPU 锁临界区）
```

直接对 `ConsoleHandle` 调用标准 `write!` 时，formatter 可能拆成多个 `write_str`，每段各
进入一次 platform 锁，整条记录不保证不被其他 CPU插入。要求整条日志原子时使用 runtime
根 crate 的 `write_fmt`，不要依赖此 marker trait。

## 3. 当前没有的能力

- 输入字符/阻塞 read：属于 TTY/字符设备；
- 任意非 UTF-8 输出：使用 `runtime_console::write_raw_bytes`；
- flush/drain：使用 platform/board 专用接口；
- 写错误详情或重试：直接依赖 platform API；
- 动态后端注册：由 platform console registry 实现；
- 日志 level/filter/timestamp：属于 runtime-logging。

不要为了某个 UART 在 v0 trait 中加入架构参数。需要稳定新增能力时创建兼容的扩展 trait
或 API v1，并让根聚合层选择版本，避免破坏所有实现。

## 4. 新增实现实例

```rust
#[derive(Default)]
pub struct RingConsole;

impl core::fmt::Write for RingConsole {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        ring_try_write(text.as_bytes()).map_err(|_| core::fmt::Error)
    }
}

impl api_v0::Console for RingConsole {}
```

实现前必须回答：ring 是否预分配、满时覆盖还是失败、谁加 SMP 锁、panic 时能否重入、
formatter 的多次 `write_str` 是否允许交错。禁止在 console 锁内扩容、睡眠或再次日志。

## 5. 生命周期与失败

`Default` 可以被反复调用，因此实现不能把“构造句柄”当成唯一硬件初始化。硬件 init 和
后端发布应由 board/platform boot 流程完成。句柄 Drop 也不能注销全局 console。

若实现把设备错误折叠为 `fmt::Error`，formatter 会停止本次格式化；若像当前 platform
handle 一样总返回 `Ok(())`，上层无法知道字节已丢失。这两种策略都允许，但必须在实现
手册中写明。

## 6. 自回归

- `Default` 在 heap/scheduler 之前可构造；
- ASCII、Unicode、空字符串、长字符串；
- formatter 多段 `write_str` 的交错边界；
- 后端 unavailable/timeout 时 `fmt::Result` 语义；
- 同 CPU递归输出与多 CPU并发；
- 确认 raw bytes、flush、input 没有被错误塞进 UTF-8 trait。

```bash
cd os
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```
