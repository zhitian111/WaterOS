# Console TTY 与 PTY 实现手册

[TTY 总览](../../README.md) · [IPC WaitQueue](../../../wateros-ipc/ipc-waitqueue/waitqueue-impl/impl-task/README.md)

系统控制台用一个 `Mutex<TtyState>` 保存 termios、会话、readable/editing、EOF 与唯一 read reservation；输入 waitqueue 单独存放，任何 wake/wait 都在释放 TTY 锁后执行。

输入链：ICRNL → ISIG 控制字符 → canonical 编辑/EOF/换行或 raw 入队 → 生成短 echo 与控制事件 → 锁外输出/投递/wake。prepared read 暂时移走字节，finish 只提交 copied 前缀并按原顺序归还后缀。VMIN/VTIME 等待用 scheduler 临界区条件复查封住丢唤醒。

PTY registry 只保存 number→Weak terminal 和 session→TerminalId。每对 terminal 有 master/slave read queue、space queue和 64 KiB 有界数据；master 写入经过 slave 行规程，slave 输出/echo 进入 master 队列。最后 master close 产生 hangup及 SIGHUP/SIGCONT 意图，最后 Arc Drop 才移除 registry 并释放 waitqueue ID。

锁内禁止用户复制、调度、设备输出和信号投递。修改行规程时同时更新 console 与 PTY slave，覆盖 erase/kill/EOF/newline、OPOST/ONLCR、EFAULT 回滚、非阻塞、队列满、master/slave 不同关闭顺序和 session/job control。

## 1. 源码边界

| 文件 | 所有状态与职责 |
| --- | --- |
| `src/lib.rs` | 唯一系统 console 的 `TTY`、输入行规程、读取 reservation、VMIN/VTIME 等待辅助和输出转换 |
| `src/pty.rs` | UNIX98 PTY registry、terminal pair、master/slave 端点、队列、hangup 与控制事件 |

稳定常量和 ABI 中立类型在 [TTY API](../../tty-api/api-v0/README.md)，完整跨组件链见
[TTY 总览](../../README.md)。本实现不管理 `/dev/ptmx`、`/dev/pts/N` 路径、fd 表和 Linux ioctl
结构，也不直接 user-copy 或投递信号；这些由 VFS/syscall 完成。

## 2. Console `TtyState`

```text
TTY: Mutex<TtyState>
├─ mode: Interactive | Closed | Fixture
├─ termios, winsize
├─ foreground_pgid, controlling_sid
├─ readable: VecDeque<u8>       已完成行规程，可交付
├─ editing: Vec<u8>             canonical 未提交行
├─ eof_pending                  空行 VEOF 的一次性 EOF
├─ active_read: Option<u64>     最多一个锁外 user-copy reservation
└─ next_read_id

INPUT_WAIT: Mutex<Option<WaitQueue>>
```

`INPUT_WAIT` 懒创建且与 TTY 锁分开。`configure` 重置 termios、会话、缓冲和 active reservation；
Closed 设置一次 EOF 状态，Fixture 预填 `password\n`，并在队列耗尽且无 reservation 时自动补充。
Fixture 因而不是“一次性密码文件”，重复读取可以再次得到 fixture。

`set_termios(flush_input=true)` 清 readable/editing/EOF；不 flush 且 canonical→raw 时把 editing
立即并入 readable。当前函数修改后没有显式 wake input waiters，这是调用侧需要留意的限制：
若新配置让已有字节立刻满足读取条件，阻塞 waiter 可能仍需其它事件唤醒。修改时应补锁外 wake
并增加竞态测试。

## 3. Console 输入状态机

`feed_input` 只在 Interactive 接受字节：

```text
raw UART byte
  -> ICRNL: '\r' 转 '\n'
  -> ISIG: VINTR/VQUIT/VSUSP?
     -> 清 editing，生成最多 4 字节 echo
     -> foreground_pgid 非零才返回 TtyControlEvent
  -> ICANON?
     -> VERASE/backspace: editing pop，echo "BS space BS"
     -> VKILL: 清 editing，echo ^U
     -> VEOF: 空 editing -> eof_pending；非空 -> 提交该行但不加入 VEOF
     -> newline: editing -> readable，再加入 newline
     -> 其它: append editing
  -> raw: append readable
  -> 释放 TTY 锁
  -> wake INPUT_WAIT
  -> 调用者锁外写 echo、投递 control event
```

console 的 readable/editing 当前没有容量上限，持续输入可增长内核堆；PTY 对应队列有 64 KiB
上限。若给 console 加限额，必须定义 canonical 行超长时是丢弃、截断、响铃还是阻塞输入，并
确保 UART 中断/轮询路径不能睡眠。

echo 返回固定 `[u8;8] + len`，避免锁内 heap allocation。控制字符只生成意图；VFS 的 UART
输入适配在拿到返回值后执行输出/信号，禁止把 signal 调用移进 `feed_input` 锁区。

## 4. Console 读取 reservation

`prepare_read(max_len)` 返回：Pending、Eof 或 `Data(TtyReadReservation)`。Data 会把最多 max_len
字节从 readable 临时 pop 到 Vec 并设置唯一 active id。user-copy 在无 TTY 锁时进行，随后：

```text
finish_read(reservation, copied, complete)
  -> id 必须等于 active_read，copied <= bytes.len
  -> [copied..] 逆序 push_front，恢复原顺序
  -> active_read=None
  -> copied=0 && !complete: Err（上层映射 EFAULT）
  -> 否则 Ok(copied)
```

console reservation 没有 Drop 自动回滚。调用者取得 Data 后必须恰好调用一次 finish，且必须保证
`copied <= bytes.len()`；非法参数会在恢复/清 active 前返回 Err，reservation 随值销毁，造成字节
丢失和 active_read 永久占用。VFS lease 负责维持此契约，新增调用者不能直接丢弃 reservation。

`try_reserve_exact` 失败目前退化成 Pending，无法区分“暂无数据”和 OOM，阻塞读取可能反复等待。
若完善内存错误，应给 `TtyPreparedRead` 增加明确错误而不是静默 Pending。

`prepare_partial_read` 忽略 VMIN 阈值，只允许 VFS 在非 canonical 字节间 timer 到期时提交已有
字节；普通 read 不应直接调用它。

## 5. VMIN/VTIME 四种组合

TTY impl 提供条件与 wait primitive，完整计时状态机在 VFS `char_dev_handle`：

| VMIN | VTIME | 期望行为 |
| --- | --- | --- |
| 0 | 0 | 立即返回现有字节，没有则返回 0 |
| >0 | 0 | 无限等到 `min(VMIN, read_len)` 个字节 |
| 0 | >0 | 从调用开始计时，首字节到达即返回；超时无字节返回 0 |
| >0 | >0 | 首字节前无限等待，之后每个新字节重启字节间 timer；达 VMIN 或 timer 到期返回部分字节 |

`wait_for_input(max_len)` 用 scheduler 临界区复查 `readable_for(max_len)`；
`wait_for_input_change(previous_len)` 等首个/下个字节；带 ticks 版本实现字节间超时。VTIME 单位是
十分之一秒，VFS 用 `SCHED_TIMER_PERIOD_MS` 向上取整成至少 1 tick。

poll 没有 read buffer 长度，`poll_readable` 使用 raw VMIN。read_len 小于 VMIN 时 read 仍只等
min 值，否则会永久等待用户缓冲容不下的字节数。

## 6. PTY registry 与对象图

```text
REGISTRY: Mutex<PtyRegistry>
├─ pairs: number(0..63) -> Weak<SharedTerminal>
└─ sessions: sid -> TerminalId

Arc<SharedTerminal>
├─ id（2 起单调 AtomicU64）/ number
├─ Mutex<PtyState>
├─ master_wait / slave_wait
└─ master_space_wait / slave_space_wait
```

registry 只存 Weak，真实所有权在打开的 endpoint/lease/reservation。`allocate_pty` 清理 dead Weak，
选择第一个空 number，创建初始 locked pair，master open-description 数为 1。`TIOCSPTLCK` 解锁后
`open_pty_slave` 才成功。最多 64 对；TerminalId 不随 PTY number 复用。

`SharedTerminal::Drop` 证明所有 Arc 已消失后移除 pair/session，并尝试释放四个空 waitqueue ID。
任何新增后台引用都必须定义何时释放，否则 PTY number、session 和 waitqueue 会永久保留。

## 7. fd clone 与 open-file-description

`PtyEndpointHandle` 保存 pair、endpoint、`Arc<EndpointLease>`、共享 `AtomicBool nonblocking` 和
accmode。普通 Rust Clone/dup/fork 只 clone lease，不增加 `master/slave_open_descriptions`：多个 fd
仍属于同一个 open file description，并共享 O_NONBLOCK。`open_pty_slave` 或 `terminal_by_id`
创建新的 lease，才增加 open-description 计数。

最后一个 lease Drop 时相应计数减一：

- master 归零：`master_hung_up=true`，为前台组排队 SIGHUP 和 SIGCONT；
- slave 归零：`slave_hung_up=true`；
- 解锁后唤醒相关 read 与 space waiter。

sys_close 先让 VFS 释放 endpoint，再 `take_control_events(id)`，最后在 TTY/VFS 锁外投递信号。
close_range、exec/exit fd 清理也必须收集 terminal id 并走相同事件派发，否则只有显式 close 有
hangup signal。

## 8. 两个数据方向

```text
master.write(key bytes)
  -> slave input line discipline
  -> slave_editing/slave_readable
  -> echo -> master_readable
  -> wake slave readers + master readers（echo）

slave.write(program output)
  -> OPOST|ONLCR 将 '\n' 扩成 "\r\n"
  -> master_readable
  -> wake master readers
```

两端目标队列上限都是 64 KiB。master write 可先于 slave 首次 open 暂存；只有 slave 曾打开后
最后关闭导致 `slave_hung_up`，master 才不可写。slave write 要求 master 未 hung up 且 master
open-description 非零。

master 输入经过行规程时，每个源字节可能不进入 slave 队列（erase/signal），或因 echo 额外占
master queue。write 返回值统计消耗的源字节，不等于目标队列增加字节数。slave newline 扩张需
两个空间；不足时在字符边界停止并返回部分进度。

当前 `push_master_bytes` 在 echo 队列满时静默截断 echo；输入仍可被 slave 接收。这优先保证输入
语义，但终端显示可能缺字符。

## 9. PTY read、EOF 与 EIO

master/slave 各自最多一个 active read id。PTY reservation 自带 Drop：未 finish 时把全部字节
按原顺序恢复并清相应 active id；finish 提交 copied 前缀、恢复后缀、解锁后唤醒空间 waiter。

hangup 差异：

- slave 读：master 关闭且队列空 → Eof；
- master 读：slave 关闭后先排空 master_readable，空后 → HungUp，VFS 映射 EIO；
- canonical 空行 VEOF：slave 得一次 Eof，随后可继续读取新输入。

`partial=true` 只用于 VMIN/VTIME timeout；它只要队列非空就发 reservation，不以 hangup/VMIN
作为 ready 条件。

PTY reservation 的 Drop 清 active id 后当前不显式 wake read waiter；数据被恢复但已有其它
reader 可能仍睡眠，直到下一次输入/状态变化。若调整 EFAULT/cancel 语义，应补 read wake 并做
多 reader lost-wake 测试。

## 10. poll 与等待队列映射

| 操作 | 条件 | waitqueue |
| --- | --- | --- |
| master read | master_readable 非空或 slave hung up | `master_wait` |
| slave read | 行规程/VMIN 就绪、EOF 或 master hung up | `slave_wait` |
| master write | slave 输入/编辑总量低于 64 KiB且未 hung up | `slave_space_wait` |
| slave write | master 输出队列有空间且 master 存活 | `master_space_wait` |

所有 wait 使用 `wait_current_while` 在 scheduler 临界区重查持短状态锁的条件。所有 wake 都在
释放 PtyState 后进行。poll_writable 与实际 write 必须使用相同容量/hangup条件，否则会出现
POLLOUT 后立即 EAGAIN 的非竞争性错误。

## 11. 会话和 job control

`attach_session` 只接受 slave 和非零 sid，锁序是 REGISTRY→PtyState；拒绝一个 sid 指向不同
terminal，或 terminal 已属于不同 sid。成功后设置 controlling_sid、foreground_pgid 并插入
sessions。`detach_session` 使用同一锁序清两处状态。

`detach_session_by_sid` 为 leader exit 路径：先在 registry 移除并取得 pair Arc，释放 registry
锁后再清 PtyState，避免长时间双锁。它不关闭任何 fd。

TIOCSPGRP 的组存在性/同 session 权限、TIOCSCTTY leader 权限、TIOCSWINSZ 后 SIGWINCH 由 syscall
层检查并执行。本 crate setter 不做 Linux credential/process-tree 验证，不能直接暴露给用户。

## 12. 锁序和禁止事项

已存在的双锁顺序是 `REGISTRY -> PtyState`；反向路径必须先释放 PtyState 再进 registry。
SharedTerminal Drop 只有在最后 Arc 消失时拿 registry，理论上已无并发 state 使用者，但新增
self-reference/回调要重新审查。

TTY/PtyState 锁内禁止：

- user-copy 或触发缺页；
- waitqueue wait/wake、scheduler、IPI；
- UART/console 输出；
- signal registry 与 task process registry 操作；
- VFS fd close/open 回调。

锁内只计算 `TtyControlEvent` 或 echo bytes，解锁后由调用者执行副作用。

## 13. 新增 ioctl 实例：`TCFLSH`

1. syscall 层 copy/校验 selector（TCIFLUSH/TCOFLUSH/TCIOFLUSH）和 fd terminal endpoint；
2. console/PTY 提供领域方法，锁内只清对应 input editing/readable 或 output queue；
3. 若 active reservation 存在，必须定义 flush 是否取消它；推荐拒绝/保留 reservation，不能清
   队列后让旧 reservation 再把字节恢复；
4. 清队列后释放状态锁，唤醒 read waiter 重新判断 EOF/VMIN，并唤醒 space waiter；
5. PTY input 指 slave_readable+slave_editing，output 指 master_readable；master/slave fd 的 ioctl
   是否作用同一 terminal 状态要保持 Linux 语义；
6. 不在 TTY 层解析用户 pointer 或返回 errno；
7. 测试 canonical 未提交行、active EFAULT reservation、并发 writer、hangup 和 poll readiness。

## 14. 常见故障定位

| 现象 | 首查 |
| --- | --- |
| console read 永久 Pending | mode、VMIN/read_len、active_read、termios 改变后是否 wake |
| EFAULT 后永远不可读 | reservation 是否 finish/Drop，active id 是否清理、字节是否恢复 |
| Ctrl-C 有 echo 但进程不停 | foreground_pgid、control event 是否锁外 drain/apply |
| PTY master 写 EAGAIN | slave input+editing 是否满、旧 reservation 是否未提交 |
| slave close 后 master EOF 而非 EIO | HungUp 在 VFS 是否正确映射 |
| dup 一个 fd 后关闭产生过早 SIGHUP | 错把 fd clone 当新 open-description 增计/减计 |
| exec/exit 无 SIGHUP | 批量 fd 清理未收集 terminal events |
| `/dev/pts/N` 永久占用 | Weak 清理、Arc reservation/handle 泄漏、SharedTerminal Drop |
| ABBA | 是否出现 PtyState→REGISTRY，而 attach/detach 是 REGISTRY→PtyState |

## 15. 回归矩阵

- console Interactive/Closed/Fixture 三模式及重复 fixture 读取；
- canonical erase/kill/EOF/newline，raw 四种 VMIN/VTIME；
- ICRNL、ISIG、ECHO、OPOST/ONLCR、TOSTOP；
- read reservation 全提交、部分提交、0 字节 EFAULT、非法 token/copy count；
- PTY lock/unlock、0..63 分配、NoSpace、number 复用而 TerminalId 不复用；
- master-before-slave 写、双向 64 KiB 边界、newline 扩张部分写；
- nonblock、poll/select/epoll 与条件一致；
- dup/fork/new open-description 的 O_NONBLOCK 和 close 计数；
- master/slave 两种关闭顺序、数据排空、EOF/EIO、SIGHUP/SIGCONT；
- controlling session、前台 pgid、leader exit、SIGWINCH；
- 多 reader/writer、EFAULT rollback 与 hangup 同时发生的 SMP 压力；
- 独立 host tests、RV/LA check、operator smoke 的 shell/Ctrl-C/raw TTY cases。
