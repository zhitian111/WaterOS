# TTY API v0 开发手册

[TTY 总览](../../README.md) · [VFS FD Session](../../../wateros-vfs/vfs-impl/impl-fd-session/README.md) · [Syscall FS](../../../wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/README.md)

本 crate 只定义终端模块间共享的值类型和 Linux 语义常量，不拥有输入队列、不实现 line discipline，也不直接解码 ioctl 用户结构。比赛中修改 TTY 时，应先判断变化属于这里的跨模块 ABI，还是 tty-impl 的状态机。

## 标识和端点

`TerminalId(u64)` 是内核路由 ID，支持排序/hash；`CONSOLE` 固定为 1，PTY 从注册表分配其它值。它不是 Linux `dev_t`、fd 或进程组号，不能直接暴露给用户态。分配器必须防 wrap 和重复，终端销毁后若复用 raw ID，需要 generation 避免旧 fd 指向新终端。

`TerminalEndpoint` 区分 `Console`、`PtyMaster`、`PtySlave`。master/slave 共享一套 PTY 状态但方向和 ioctl 权限不同：master 写入成为 slave 输入，slave 输出可供 master 读取；控制终端与作业控制通常绑定 slave。不能仅凭 TerminalId 推断 endpoint。

`PtyError` 是 TTY 内部错误：NotFound、Locked、NoSpace、Invalid、HungUp、WouldBlock、Interrupted、Busy。VFS/syscall 层负责稳定映射 errno，例如 WouldBlock→EAGAIN、Interrupted→EINTR；不要在 TTY 实现里直接返回裸负 errno。

## termios 布局与位

`TtyTermios` 使用 `repr(C)`：四个 `u32` flag、`u8 line`、`cc: [u8; 19]`。这是架构无关内核布局，不保证与用户 libc 的 `struct termios` 二进制相同；TCGETS/TCSETS 必须在 syscall 层逐字段转换和 copy_to/from_user。

本 API 公开的关键位：输入 `ICRNL`；输出 `OPOST|ONLCR`；local `ISIG|ICANON|ECHO|TOSTOP`。控制字符 index 包括 VINTR、VQUIT、VERASE、VKILL、VEOF、VTIME、VMIN、VSUSP。未公开的默认 flag 位仍存在于 `TtyTermios::DEFAULT`，修改常量时必须同时核对默认十六进制值和实现使用处。

canonical 模式按行交付：erase/kill 修改编辑缓冲，VEOF 触发当前缓冲可读但通常不把 VEOF 字节交给应用。noncanonical 模式的 VMIN/VTIME 是四种组合状态机，timeout 与首字节到达时点有关，不能简化成一次 poll。

输入 `ICRNL` 在入队前做 CR→NL；输出只有同时启用 OPOST 与 ONLCR 才做 NL→CRNL。echo 输出也要遵循一致的输出处理策略，避免重复 CR。

## 信号与锁顺序

ISIG 开启时，VINTR/VQUIT/VSUSP 分别产生 SIGINT(2)、SIGQUIT(3)、SIGTSTP(20)。hangup、继续和窗口变化使用 SIGHUP(1)、SIGCONT(18)、SIGWINCH(28)。

`TtyControlEvent { process_group, signal }` 是延迟投递请求：line discipline 在 TTY 锁内完成缓冲/前台组状态变化，只记录 event；释放 TTY 锁后由 syscall/signal 层投递。禁止在 TTY 锁内获取 task/signal 锁，否则 signal 路径反向查询 TTY 时死锁。

`TOSTOP` 涉及后台进程组写控制终端，实际 SIGTTOU 等策略需由实现/作业控制层补齐；本 API 当前没有公开 SIGTTOU 常量，不能把 TOSTOP 视作已完整支持。

## 窗口尺寸与控制台模式

`TtyWinSize` 也是 `repr(C)`，默认 25x80、pixel 为零。TIOCSWINSZ 应先比较旧值，锁内更新，锁外向前台进程组发 SIGWINCH；同值设置不应制造信号风暴。

`ConsoleTtyMode`：Interactive 消费物理控制台，Closed 立即 EOF，Fixture 提供 pre/LTP 固定密码输入。final profile 不得误开 Fixture，否则测试可能通过但真实交互输入被伪造；模式选择应在 bring-up 日志中可见。

## 新 ioctl 实例：TIOCGPGRP

读取前台进程组时，在 TTY state 锁内复制一个 `usize pgid` 后立即解锁；syscall 层检查用户指针并 copy_to_user。设置版本需要验证目标进程组存在、调用者 session/controlling-terminal 权限，再短暂加锁提交。任何 signal 投递都放到解锁之后。

## 生命周期

终端 state 应由 master/slave fd、controlling-terminal 引用和 registry 共同持有。关闭最后一个 master 通常让 slave hung up，唤醒所有阻塞 read/poll 并产生必要信号；不能只删路径而留下永久 waiter。等待者必须注册在可被 close/hangup 唤醒的 waitqueue 上。

## 回归清单

- TerminalId 唯一性、console 固定值、master/slave 路由；
- TCGETS/TCSETS round-trip、未知 flag 保留策略、错误用户指针 EFAULT；
- canonical 行、erase/kill/EOF、ICRNL、echo 与 ONLCR；
- VMIN/VTIME 四组合、O_NONBLOCK、信号中断和 timeout 边界；
- VINTR/VQUIT/VSUSP 只投递前台组且无锁反转；
- TIOCG/SETWINSZ 与 SIGWINCH 去重；
- master/slave close、hangup、阻塞 waiter 唤醒和资源基线；
- Interactive/Closed/Fixture 三模式，final 明确拒绝测试 fixture 泄漏。
