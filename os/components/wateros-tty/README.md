# wateros-tty

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-tty` 管理 WaterOS 唯一系统控制台的 TTY 行规程。它不负责 VFS 路径、
文件描述符表，也不包含 Linux syscall 请求号，从而可以独立于具体文件系统和
系统调用实现复用。

## 目录结构

- `tty-api/api-v0`：稳定的版本化接口，定义终端工作模式、`termios`、窗口大小、
  控制字符索引以及终端产生的信号事件。
- `tty-impl/impl-console`：控制台 TTY 实现，维护受锁保护的输入和编辑状态，处理
  canonical/raw 读取、`VMIN/VTIME`、回显、输出换行转换、前台进程组和控制会话。
- `src/lib.rs`：聚合 facade，选择并重新导出当前 TTY 实现。

## 数据流

```text
UART 字符设备
    │ 原始字节
    ▼
VFS 字符设备适配层 ──feed_input──▶ wateros-tty
    │                                  │
    │                                  ├─ 输入缓冲与行编辑
    │                                  ├─ echo 字节
    │                                  └─ TtyControlEvent
    ▼
stdin/stdout/stderr                 signal 调度层
```

VFS 字符设备适配层从 UART 读取原始字节，再调用 `feed_input`。TTY 返回需要回显的
字节和可选的 `TtyControlEvent`；调用方必须在释放 TTY 锁后输出回显或投递信号。
syscall 层只负责在 Linux 用户 ABI 的 `termios`/`winsize` 内存布局与本组件类型之间
转换。

## 主要状态

- `ConsoleTtyMode::Interactive`：从真实 UART 接收输入。
- `ConsoleTtyMode::Closed`：读取立即返回 EOF，用于无人值守评测。
- `ConsoleTtyMode::Fixture`：只提供兼容旧测试的固定 `password\n` 输入。
- `TtyTermios`：输入、输出、本地模式和控制字符配置。
- `TtyWinSize`：通过 `TIOCGWINSZ/TIOCSWINSZ` 访问的终端尺寸。
- 前台进程组和控制会话：供 job control、Ctrl-C/Ctrl-Z 以及后台读写检查使用。
- processed input：已经完成 CR 转换、行编辑或 raw 处理，可交付给 `read(2)` 的字节。

## 锁与调度约束

- 持有 TTY 锁时禁止访问 UART、复制用户内存、执行调度或投递信号。
- UART 设备锁只保护一次短读取，不得与 TTY 锁嵌套持有。
- `prepare_read` 先预约输入字节；用户复制成功后由 `finish_read` 提交，失败时将未复制
  字节放回队首，避免并发读取重复消费或丢失输入。
- 阻塞读取通过 waitqueue 等待输入或超时，不允许忙等占满 CPU。

## 各层职责

- `wateros-tty`：终端机制和状态。
- `wateros-vfs`：UART/字符设备发现以及 TTY 文件描述符适配。
- `wateros-syscall`：TTY ioctl、用户内存复制、后台进程组检查和信号返回值。
- `os/src/user_operator.rs`：按 `pre`/`final_online`/`operator-shell` 等编译期
  feature 选择 interactive/closed/fixture，并启动唯一控制台输入任务。
