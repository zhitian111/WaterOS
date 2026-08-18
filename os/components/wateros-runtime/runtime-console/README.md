# runtime-console

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-runtime](../README.md)

这是内核从 early boot 到完整驱动阶段共用的输出门面。它不负责日志级别、输入、TTY 行
规程或字符设备 fd；核心目标是在不能依赖堆和调度器时仍能格式化输出，并使一整次调用在
SMP 上尽量不交错。

## 1. 分层与 feature

```text
调用方 print!/println!/write_*
  -> runtime-console 根 crate
     -> impl-platform-console（feature 可选）
        -> platform::console 聚合层
           -> 未注册 runtime writer：board early UART
           -> 已注册 runtime writer：字符设备 writer
```

- `console-api/api-v0` 只定义 `Console: fmt::Write + Default` 标记 trait；
- `console-impl/impl-platform-console` 将 runtime API 接到 platform；
- 根 crate 定义统一函数、宏、ANSI 色彩及 `ConsoleHandle` 再导出；
- 关闭 `impl-platform-console` 后，`write_fmt`/`write_str`/`write_raw_bytes` 静默丢弃输出，
  这是最小编译配置，不代表运行时成功写到了设备。

## 2. 公共入口的精确区别

| 入口 | 输入 | 原子性边界 | 换行语义 |
|---|---|---|---|
| `write_fmt(args)` | `fmt::Arguments`，只在调用期间有效 | 整次格式化持 platform console 锁 | board/运行期 writer 决定 |
| `write_str(text)` | UTF-8 `&str` | 整个 slice | early board 常把 `\n` 转 CRLF |
| `write_raw_bytes(bytes)` | 任意字节 | 整个 slice | 不做 CR/LF 转换 |
| `print!` / `println!` | literal 格式串 | 转到一次 `write_fmt` | `println!` 追加 `\n` |

`macro_rules!` 只接受 literal 格式参数。非 UTF-8 stdout/TTY 输出必须走 raw 接口；日志应
走格式化接口。不要循环调用单字节写函数，否则每字节都产生锁竞争和 MMIO 轮询。

`AnsiColor` 仅输出 SGR 转义序列，接收端不支持 ANSI 时会看到原始控制字符。panic 或
早期 bring-up 若追求最小失败面，可使用无颜色的 `write_str`。

## 3. 锁、重入和切换

真实同步在 `wateros-platform/src/console.rs`：

1. 保存并关闭本 CPU 全局中断；
2. 获取跨 CPU `CONSOLE_WRITE_LOCK`；
3. 记录 `CONSOLE_WRITE_OWNER=cpu_id`；
4. 在锁内完成整个 buffer 或整个 `fmt::Arguments`；
5. 清 owner、解锁并恢复原中断状态。

同一 CPU 在 formatter/错误路径中递归输出时，owner 检测会跳过再次加锁并强制走 early
board 后端，避免本核自锁和 runtime writer 递归。不同 CPU 若遇到持锁者，会自旋等待；
所以控制台不是严格意义的 NMI/panic-safe 通道：若另一 CPU 永久死在锁内，当前 panic
输出也可能卡住。

`register_runtime_writer(fn(&[u8]) -> PlatformConsoleResult<()>)` 由 platform 层提供，
只能在字符设备及其锁全部初始化后、且启动序列保证无并发切换时调用。接口没有注销或
热替换同步。注册前自动使用 early UART，注册后正常输出走 runtime writer；递归输出仍
回退 early UART。

锁顺序规则：不要持有 scheduler、VFS、地址空间、frame allocator、heap allocator 或
设备内部高层锁再输出。串口很慢，即使不会死锁也会放大关键区延迟。正确做法是在短锁内
复制诊断快照，释放业务锁后一次性输出。

## 4. 生命周期和分配

console 路径自身不要求 heap。`format_args!` 借用参数而不分配，platform writer 直接逐段
写出；但参数的 `Display`/`Debug` 实现可能自行分配或加锁，调用者必须审核。禁止缓存
`fmt::Arguments` 或把它发送给异步任务，因为其中借用只保证当前调用有效。

`PlatformConsoleHandle` 是无状态零大小句柄，实现 `Default + fmt::Write + Console`。
它的 `write_str` 把完整 UTF-8 slice 传给 platform buffer 路径并忽略平台错误，最终总是
返回 `fmt::Result::Ok`；需要观察设备错误的代码应直接使用 platform API。

## 5. 故障语义

- runtime 根入口返回 `()`，当前采用 best-effort，写失败不会反馈给调用者；
- platform 层可返回 `Unsupported`、`Unavailable`、`WriteFailure`、`BufferFailure`；
- RISC-V early UART 每字节轮询最多 1,000,000 次，超时返回失败；
- LoongArch64 early UART 当前无轮询上限，设备永久不 ready 会永久自旋；
- `console_flush()` 直接调用 board early backend，不经过 runtime writer，也不获取聚合输出
  锁；它只等待 UART 发送完成，不等价于日志持久化或全局内存屏障。

输出失败时不能再用同一 logger 报错，否则可能递归进入相同故障路径。需要故障计数时用
原子变量记录，待系统恢复后读取。

## 6. 启动和 panic 调用链

推荐启动顺序：

```text
arch/platform MMIO 可访问
  -> runtime::init_console()（空 raw write，仅触发链接/路径）
  -> early 启动日志 / show_logo
  -> heap、driver registry 初始化
  -> 安装 runtime console writer
  -> logging、task、VFS 正常运行
```

panic handler 使用 `console::println!` 后调用 `platform::console::console_flush()`。它不分配，
但仍可能受跨 CPU console 锁或 UART ready 状态影响；修复 panic 路径时不能假定所有 panic
文本一定可见，关机兜底必须独立于输出成功。

## 7. 新增后端/镜像输出实例

新增 board console 时应实现 platform 层的 buffer、raw buffer 与 flush，而不是在 runtime
里匹配架构。buffer 路径可以做 `\n -> \r\n`，raw 路径必须保持 wire bytes 不变。

若要增加内存 ring buffer 镜像，建议放在 platform 聚合锁内：先把本次调用写入有界、
预分配 ring，再写 UART。必须定义满时覆盖/丢弃策略，不能在 console 锁内分配或睡眠，
也不能用 console 自身报告 ring 错误。

## 8. 自回归矩阵

- heap 初始化前输出 ASCII、Unicode 和换行；
- SMP 多 CPU 同时各写一整行，验证行内不交错；
- formatter 内递归日志，验证不自锁且走 early fallback；
- runtime writer 注册前/后输出路径切换；
- raw bytes 包含无效 UTF-8 和 `\n` 时不被转换；
- 模拟 UART 不 ready，验证 RISC-V 超时及 LoongArch64 已知自旋行为；
- panic 位于普通路径、本 CPU console 临界区、另一 CPU 持锁三种情形；
- 关闭 `impl-platform-console` 时可编译且输出确实为 no-op。

从 `os/` 运行集成检查：

```sh
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```
