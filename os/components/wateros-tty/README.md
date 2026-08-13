# wateros-tty

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-tty` 管理 WaterOS 唯一系统控制台的 TTY 行规程。它不负责 VFS 路径、文件描述符表，
也不包含 Linux syscall 请求号，从而可以独立于具体文件系统和系统调用实现复用。

## 模块分层


| 层         | 路径                     | 职责                                                                                                      |
| ------------ | -------------------------- | ----------------------------------------------------------------------------------------------------------- |
| 聚合门面   | `src/lib.rs`             | 按 feature 选择并重导出当前 TTY 实现，作为调用方访问终端策略和处理后输入的唯一入口。                      |
| TTY API    | `tty-api/api-v0/`        | 版本化终端数据契约：`ConsoleTtyMode`、`TtyTermios`、`TtyWinSize`、`TtyControlEvent` 及控制字符/信号常量。 |
| 控制台实现 | `tty-impl/impl-console/` | 控制台 TTY 行规程：输入缓冲、行编辑、canonical/raw、`VMIN/VTIME`、回显、前台进程组与控制会话。            |

## 实现说明

- `wateros-tty` 只负责终端机制和状态，不实现 VFS 路径、fd 表或 Linux syscall 请求号。
- 数据流：UART 字符设备 → VFS 字符设备适配层 → `feed_input` → TTY（输入缓冲与行编辑、echo
  字节、`TtyControlEvent`）→ stdin/stdout/stderr 与 signal 调度层。
- 调用方必须在释放 TTY 锁后输出回显或投递信号；syscall 层只负责在 Linux 用户 ABI 的
  `termios`/`winsize` 内存布局与本组件类型之间转换。
- 主要状态：
  - `ConsoleTtyMode::Interactive`（真实 UART）、`Closed`（读立即 EOF，无人值守评测）、
    `Fixture`（固定 `password\n`，兼容旧测试）。
  - `TtyTermios`（输入/输出/本地模式与控制字符）、`TtyWinSize`（`TIOCGWINSZ/SWINSZ`）。
  - 前台进程组与控制会话：供 job control、Ctrl-C/Ctrl-Z 及后台读写检查使用。
  - processed input：已完成 CR 转换、行编辑或 raw 处理、可交付 `read(2)` 的字节。
- 锁与调度约束：
  - 持有 TTY 锁时禁止访问 UART、复制用户内存、执行调度或投递信号。
  - UART 设备锁只保护一次短读取，不得与 TTY 锁嵌套持有。
  - `prepare_read` 先预约输入字节，用户复制成功后由 `finish_read` 提交，失败时把未复制字节
    放回队首，避免并发读取重复消费或丢失输入。
  - 阻塞读取通过 waitqueue 等待输入或超时，不允许忙等占满 CPU。
- `os/src/user_operator.rs` 按 `pre`/`final_online`/`operator-shell` 等编译期 feature 选择
  interactive/closed/fixture，并启动唯一控制台输入任务。

## 调用链路

输入数据流：

```text
UART 字符设备
  -> VFS 字符设备适配层
  -> feed_input(raw bytes)
  -> 输入缓冲与行编辑（canonical / raw）
  -> echo 字节 / TtyControlEvent
  -> 可交付字节 → read(2)；信号 → signal 调度层
```

读取路径：

```text
read(2)
  -> syscall 层（Linux ABI 转换）
  -> TTY prepare_read -> 预约输入字节
  -> 用户复制成功后 finish_read 提交；失败回滚到队首
```

ioctl 路径：

```text
TIOCGWINSZ / TIOCSWINSZ / tcsetattr 等
  -> syscall 层解析 Linux termios / winsize 布局
  -> 与 TtyTermios / TtyWinSize 互相转换
```

## 实现功能

### tty-api / 终端数据契约

主要实现在 `tty-api/api-v0/src/lib.rs`：

- `ConsoleTtyMode`：`Interactive` / `Closed` / `Fixture`。
- `TtyTermios`：`NCCS = 19` 的 Linux 兼容行规程状态，含 `TtyTermios::DEFAULT`。
- `TtyWinSize`：`row` / `col` / `xpixel` / `ypixel`，含 `DEFAULT`（25×80）。
- `TtyControlEvent { process_group, signal }`：终端控制字符产生的信号请求；实际投递须在释放
  TTY 锁后由 syscall/signal 层完成。
- 常量：控制字符索引（`VINTR`/`VERASE`/`VEOF`/`VMIN`/`VTIME`/`VSUSP` 等）、信号（`SIGINT`/
  `SIGQUIT`/`SIGTSTP`）与模式位（`ICANON`/`ECHO`/`ISIG`/`ICRNL`/`OPOST`/`ONLCR` 等）。

### impl-console / 控制台 TTY

主要实现在 `tty-impl/impl-console/src/lib.rs`，核心是两个模块级全局变量。

#### `static TTY: Mutex<TtyState>` —— 系统控制台的共享行规程状态

`TtyState` 字段（由该自旋锁保护）：

- `mode: ConsoleTtyMode`：stdin 来源策略（`Interactive` / `Closed` / `Fixture`）。
- `termios: TtyTermios`：当前行规程配置。
- `winsize: TtyWinSize`：对用户态报告的窗口尺寸。
- `foreground_pgid: usize`：可接收终端控制信号的前台进程组。
- `controlling_sid: usize`：当前拥有该控制终端的会话。
- `readable: VecDeque<u8>`：已完成行规程处理、可交付给读取者的字节。
- `editing: Vec<u8>`：canonical 模式下尚未提交的一行。
- `eof_pending: bool`：空行收到 VEOF 后待交付的一次 EOF。
- `active_read: Option<u64>` / `next_read_id: u64`：当前独占读取预约与下一次预约序列号。

对 `TTY` 的操作入口（公开函数）：

- `configure(mode)` / `mode()`：配置与查询 stdin 来源策略。
- `termios()` / `set_termios(t, flush_input)`：读取/设置行规程。
- `winsize()` / `set_winsize()`：读取/设置窗口尺寸。
- `foreground_pgid()` / `set_foreground_pgid()`、`controlling_sid()` /
  `set_controlling_sid()` / `detach_controlling_terminal()`：前台进程组与控制会话。
- `output_stops_background()`：`TOSTOP` 后台写检查。
- `feed_input(byte) -> (Option<TtyControlEvent>, [u8; 8], usize)`：喂入一个输入字节，返回
  信号请求与回显字节。
- `prepare_read(max_len)` / `prepare_partial_read(max_len)` / `finish_read(rsv, copied, complete)`：事务式读取预约、提交与回滚。
- `poll_readable()` / `readable_len()` / `read_settings()` / `transform_output()`：读取就绪、
  可读长度、设置与输出转换。
- `wait_for_input(max_len)` / `wait_for_input_change()` /
  `wait_for_input_change_for_ticks()`：阻塞等待输入或变化（配合 `INPUT_WAIT`）。

#### `static INPUT_WAIT: Mutex<Option<WaitQueue>>` —— 输入等待队列

- 阻塞读取在无输入时登记到该 waitqueue；`feed_input` 投递输入后唤醒等待者。
- 锁顺序：持 `TTY` 锁时只准备等待条件，真正的阻塞/唤醒在释放锁后进行。

### 聚合门面 / src/lib.rs

- 按 feature 导出 `api` 与 `impl-console`，调用方通过本 crate 访问终端策略和处理后的输入。
- 各层职责：
  - `wateros-tty`：终端机制和状态。
  - `wateros-vfs`：UART/字符设备发现以及 TTY 文件描述符适配。
  - `wateros-syscall`：TTY ioctl、用户内存复制、后台进程组检查和信号返回值。
  - `os/src/user_operator.rs`：按 feature 选择 interactive/closed/fixture 并启动输入任务。
