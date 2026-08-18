# Console、TTY、klog、debug 与 GUI 边界手册

这些模块都与“输入输出/可观测性”有关，但拥有完全不同的状态。混用它们会制造递归日志、锁反转、控制字符失效，或图形设备双重所有权。

## 1. 五条互不替代的路径

| 路径 | 目的 | 状态所有者 | 用户可见入口 |
| --- | --- | --- | --- |
| runtime console | 极早期与即时文本输出 | platform console 锁/设备 | 串口/QEMU console |
| runtime logging | `log` facade 的带级别即时输出 | 静态 logger，无留存环 | console |
| TTY/PTY | 终端输入、行规程、会话和 job control | wateros-tty | stdin/stdout、`/dev/tty`、PTY |
| klog | 固定容量的结构化日志留存 | klog ring | `sys_syslog`/dmesg 类工具 |
| debug ABI | GDB 停机快照、事件与关键锁诊断 | 固定原子诊断区 | 主机 debug 工具 |

GUI 是另一条显示/输入消费路径：它通过 display/input driver，而不是 console/TTY。

## 2. 输出调用链与禁区

即时日志：

```text
log::{trace,debug,info,warn,error}!
  -> WaterOSLogger::log
  -> console::println!（一次完整 fmt::Arguments）
  -> platform_console_write_fmt
  -> platform 跨 CPU console 锁
  -> early UART/console writer
```

klog 留存：

```text
klog_*!
  -> 512-byte 栈格式缓冲
  -> 关本 CPU IRQ
  -> KLOG TrackedMutex
  -> 固定槽 ring append
  -> 恢复 IRQ
```

两者目前不自动互相转发。看到串口日志不代表 `dmesg` 一定有记录，klog 中有记录也不代表即时打印。

禁止在 allocator、console、klog、scheduler 或关键 registry 锁内调用可能分配的日志路径。需要在敏感路径记录时，优先使用固定原子 debug event，或先复制最小字段、解锁后再日志。

## 3. console 与 TTY 的区别

console 是输出设备通道；TTY 是用户终端语义。TTY 负责 termios、canonical/raw、echo、VMIN/VTIME、前台进程组和控制字符，但不直接拥有 UART。

```text
UART/host input
  -> VFS console input worker
  -> tty::feed_input
       -> ICRNL / canonical edit / raw enqueue
       -> ECHO bytes
       -> 可选 TtyControlEvent(pgid, signal)
  -> 锁外写 echo
  -> 锁外向进程组投递 signal
  -> wake input waitqueue
```

`Ctrl-C` 失效应分别检查：原始字节是否到达、TTY 是否启用 `ISIG`、`VINTR` 是否匹配、foreground pgid 是否正确、事件是否被 operator input worker 投递、signal 是否在 trap 返回时送达。直接往 UART 写 `^C` 文本不能替代这条链。

TTY read 同样使用 reserve-copy-commit。canonical 数据在整行/VEOF 后就绪；用户 copy 失败必须把未复制字节按原序放回，不能把输入吞掉。

## 4. PTY 生命周期

PTY pair 由共享 terminal 对象连接 master/slave：

- `/dev/ptmx` 创建 pair；unlock 后 `/dev/pts/N` 可打开。
- master 写入进入 slave 行规程，slave 输出/echo 进入 master 队列。
- 两侧各有 read reservation 和数据/空间 waitqueue。
- 端点 clone/dup 增加共享引用，不复制 terminal 状态。
- 最后一个 master/slave descriptor 关闭产生 hangup 并唤醒对端。
- 最后一个 `Arc` 释放才从 registry 删除 pair 和会话映射。

新增 PTY ioctl 时先判断它修改 terminal 共享状态、端点 OFD 状态还是 fd slot flag；放错层会在 dup/fork 后不一致。

## 5. klog 环语义

当前环包含固定 256 个槽，每槽正文最多 1024 字节；格式化入口的栈缓冲为 512 字节。环满覆盖最旧记录并累计 dropped 统计，不阻塞写者。

`KlogRecordView` 的正文借用只在持 ring 锁的闭包内有效，不得保存到解锁后。syslog 读取必须在锁内完成选择/格式化/游标推进，再在锁外复制给用户。

全局 read cursor 意味着多个读取者会互相消费，不是每 fd 独立 cursor。增加 reader 模式或 procfs 节点时必须明确是否沿用该语义。

常见诊断：

- console 有日志、syslog 空：调用者使用 runtime logger 而非 klog 宏。
- dropped 增长：生产速度超过固定环容量，不一定是消费者 bug。
- klog 路径死锁：检查是否在 `KLOG` 闭包内递归记录或调度。
- 时间/task id 为 0：记录发生在对应平台/task 状态可用之前。

## 6. debug ABI 适用场景

debug 组件用于“目标已卡住，普通日志不再前进”的情况：

- 每 CPU 双槽状态先写非活动槽，再用 Release 发布槽号。
- 事件环最后发布 sequence；主机忽略 sequence 不匹配的半写记录。
- `TrackedMutex` 记录锁类别、地址和 owner，不改变普通调用点 `lock()` 形状。
- 关闭 feature 时记录入口编译为空操作，release 不承担热路径成本。

新增 `DebugEventKind` 只能追加稳定编号，不能复用/重排，否则主机脚本会错误解码。记录函数必须固定大小、无分配、无日志、无阻塞。

现场顺序：

```text
make debug-server ...
  -> 校验本地 ELF build ID / frame pointers
  -> GDB 停住目标
  -> 读取每 CPU active state slot
  -> 检查 current task / wait target / held locks
  -> 按 sequence 重建最近事件
  -> 对照 dropped_events 判断时间线是否完整
```

不要把 debug state 当业务真相源；它是可能略滞后的诊断快照，业务修改仍必须操作原组件状态。

## 7. GUI 的所有权与锁顺序

GUI feature 下：

```text
display/input driver registry
  -> gui::initialize
  -> GuiRuntime（全局 Mutex<Option<_>>）
       -> ShadowSurface(BGRA8888 Vec)
       -> Desktop/windows/widgets/input queues/dirty regions
  -> gui_refresh_task
       -> poll input
       -> scene state transition
       -> render shadow
       -> GUI lock -> display device lock
       -> copy dirty regions + flush
```

允许的嵌套顺序是 GUI runtime 锁后短暂获取 display/input device 锁。设备操作必须非阻塞；不能在 GUI 锁内睡眠或反向调用会获取 GUI 锁的回调。

`gui` 与 `user-graphics` 互斥：前者让内核 GUI 拥有 framebuffer/input，后者通过 VFS 把设备交给用户态。不能为了让两个 feature 同时编译就删除互斥检查，否则会出现双消费者和 framebuffer 写竞争。

dirty region 只限制 shadow 到 framebuffer 的复制/flush；当前 desktop 仍重画完整 shadow surface。性能分析要区分 CPU 绘制时间、内存复制量和设备 flush 三部分。

## 8. GUI 失败恢复

显示提交失败时，已取出的 dirty regions 必须重新加入，以便下一帧重试。输入队列满会丢新事件并增加计数，输出语义队列满则丢最旧事件；二者不阻塞生产者。

新增图元/控件应验证：

- 所有坐标采用半开区间并先裁剪；
- stride、width*4、height 乘法 checked；
- BGRA/RGBA 转换只在明确边界发生；
- 删除窗口同步清 focus/capture/drag；
- 精确标脏包含旧位置与新位置，移动时通常全屏标脏更安全；
- 非 ASCII 文本当前字体能力有限，模型能保存 UTF-8 不代表能渲染。

## 9. utils 的准入规则

`wateros-utils` 只放无平台、无策略、无全局状态的确定性工具。允许依赖 `core` 和调用方提供的 `fmt::Write`；不允许为了“复用”把 UART、CSR、scheduler、MM 或 VFS 逻辑放进 utils。

新增 utility 前回答：

1. 是否能在 host 单元测试中运行；
2. 是否不需要全局初始化；
3. 是否没有架构/设备假设；
4. 错误是否通过返回值表达；
5. 是否能由调用方决定分配和输出策略。

任一答案为否，应放回拥有该策略/状态的组件。

## 10. 修改后的回归矩阵

| 修改 | 必测 |
| --- | --- |
| console/logger | SMP 多 CPU 整行不交错；初始化前后输出；panic 不递归 |
| TTY canonical/raw | erase/kill/EOF、VMIN/VTIME、坏用户指针回滚、signal interrupt |
| PTY | ptmx unlock、双向数据、dup/fork、队列满、master/slave 最后关闭 hangup |
| klog | append/read/clear、截断、环覆盖、多个 reader、syslog bad pointer |
| debug ABI | feature on/off、build ID、停在发布中途的槽一致性、事件环回卷 |
| GUI | 无设备退化、两架构显示、输入/焦点/拖动、提交失败重试、队列溢出 |
| utils | host 单测、边界输入、无架构 feature 依赖 |

相关详细文档：[`wateros-tty`](../../components/wateros-tty/README.md)、[`wateros-klog`](../../components/wateros-klog/README.md)、[`wateros-debug`](../../components/wateros-debug/README.md)、[`wateros-gui`](../../components/wateros-gui/README.md)、[`wateros-utils`](../../components/wateros-utils/README.md)。
