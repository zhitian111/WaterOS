# Platform API v0 开发手册

[Platform 总览](../../README.md) · [Arch API](../../platform-arch/arch-api/api-v0/README.md)

本 crate 描述“板级机器或固件怎样提供能力”，不描述 ISA CSR、页表和 trap frame。具体 profile 实现这些类型，`wateros-platform` 聚合层选择后端并保存运行期探测值。

## 模块边界

| 模块 | 稳定契约 | 不属于这里 |
|---|---|---|
| `boot` | 固件入口的最多三个原始参数槽 | `_start` ABI、栈和 BSS 初始化 |
| `time` | 单调 tick 的频率 Hz | 读取 ISA counter 的指令 |
| `timer` | 编程绝对 tick deadline | 本地 timer pending/enable CSR |
| `smp` | 启 CPU、IPI transport、远端 TLB/I-cache fence | 本地中断清除、scheduler 策略 |
| `console` | early console 错误分类 | UART 字符设备/TTY |
| `reset` | shutdown/reboot 类型与原因 | 用户权限与 reboot magic 校验 |

判断新增能力归属时：CSR/指令进入 platform-arch，独立设备寄存器和枚举进入 driver，task 选择/负载均衡进入 scheduler，板级地址/固件调用才进入 platform。

## Boot 参数

`PlatformBootArgs: Debug + Clone + Copy` 提供 `arg0/arg1/arg2 -> Option<usize>`，默认全为 None。每个 profile 自己解释位置，例如 RISC-V OpenSBI 常见 arg0=hart id、arg1=DTB PA；不能把该含义写进通用调用者。

这些值是未经验证的固件输入。DTB 指针使用前要检查非零、对齐、header/长度与物理 RAM/保留区；CPU id 使用前要检查编译容量。trait 的 Copy 不赋予指针目标任何生命周期，分页切换后还必须保证对应映射存在。

入口汇编、链接地址、BSS 清零、BSP/AP 分流不由 trait 表达。新增 profile 时必须把 `_start.S` 的寄存器约定与 BootArgs 构造写在同一份 profile 手册。

## 时间和 deadline

`PlatformTime::get_time_frequency_hz()` 返回 profile 默认 tick/秒，0 必须是 `InvalidFrequency`；聚合层可用 DTB 探测值覆盖。这个频率必须与 arch `read_time_tick` 的计数源完全相同，否则所有 sleep、timeslice 和 wall-clock 换算都会按比例漂移。

`PlatformTimerDeadline(u64)` 是绝对 tick，不是 duration，也不是纳秒：

```text
arch read_time_tick() -> now
duration * frequency  -> delta ticks（checked/saturating）
now + delta           -> absolute deadline
platform backend      -> 若硬件只收 relative，则在此处再读同源 now 并换算
```

过期 deadline 应尽快触发，通常至少编程 1 tick；不能 unsigned underflow 成极远未来。每 CPU timer 的初始化、pending 清除和 enable 属于 arch/profile 的联合责任。换频后旧 deadline 如何处理必须明确，当前设计更适合启动期固定频率。

## SMP 状态机

`CpuId` 是平台逻辑 ID；RISC-V profile 目前直接使用 hart id，但通用代码不能假设。`configured_cpu_mask()` 表示机器容量，不表示 OS online。向 CPU 发任务/IPI 前至少与 scheduler online mask 求交。

`start_cpu(cpu,start_addr,opaque)` 只表示固件/控制器接受请求。完整状态链是：

```text
configured/stopped -> start request -> StartPending
 -> AP assembly entry -> CPU-local/trap/MM/timer init
 -> scheduler publish online -> 才可投递普通任务
```

`AlreadyAvailable` 也不能跳过 OS online 等待。`HartStatus::Started` 是固件视图，不等价 scheduler ready；`Unknown(raw)` 要保留原始值便于诊断。

`IpiKind` 是聚合层 pending bitmap 的软件 reason：Reschedule、TlbShootdown、TaskNotify。硬件/SBI IPI 通常只运送“有中断”，发送顺序必须先 Release 发布 reason，再发送通知；接收端先清本地硬件 pending，再 AcqRel 取 reason。多个 kind 可合并，不能用枚举变量覆盖前一个 bit。

`send_ipi(mask)` 只负责运输。`flush_tlb_remote` 和 `flush_icache_remote` 则是同步完成契约：返回 Ok 后目标 CPU 的 fence 必须已经完成。MM 只有据此才能释放页表/物理页或复用 ASID；缺失实现必须返回 Unsupported，绝不能伪成功。

## Console 与 reset

平台 console 是 heap、driver 和 TTY 之前的 early output。错误区分 Unsupported、Unavailable、WriteFailure、BufferFailure；panic 路径应 best-effort，避免打印失败递归 panic。正常 runtime console 可在更高层提供格式化和锁。

reset 类型有 Shutdown、ColdReboot、WarmReboot，reason 有 NoReason/SystemFailure。用户 syscall 的 capability、magic 值、sync 和杀任务策略不在后端；平台只执行最终不可返回动作。若后端调用意外返回，必须报告 Failed/Unsupported，调用者不能继续假装系统已经关机。

## 新 profile 实施顺序

1. 固定链接脚本、入口寄存器、BSP/AP 和栈。
2. 验证 BootArgs/DTB，得到 RAM、reserved-memory、CPU mask、timebase。
3. 建 early console，保证失败不会依赖 heap。
4. 实现同源 tick frequency 与绝对 deadline。
5. 实现单 AP start/online，再实现定向 IPI reason。
6. 实现有 ack 的远端 TLB/I-cache fence。
7. 最后接 reset，并验证所有错误码不伪成功。

## 回归清单

- 缺失/畸形 boot args、DTB 越界、CPU id 超容量；
- DTB frequency、fallback frequency、零频率和长时间漂移；
- 过去/当前/未来/接近 `u64::MAX` deadline，每 CPU 独立触发；
- 重复/并发 start，firmware Started 与 OS online 的时间窗；
- 单 kind、多 kind、空 mask、offline target 的定向 IPI；
- remote fence 成功、Unsupported、firmware error、超时，MM 不提前回收；
- early console 并发/panic 重入；Shutdown/Cold/Warm 和失败返回；
- 至少双 CPU 真实运行；仅 cargo check 不能证明平台时序正确。
