# wateros-klog-impl-kernel 离线开发手册

本 crate 是 WaterOS klog 的当前内核实现：固定槽环、全局锁和中断保护、写入上下文采集、
无堆格式化，以及 `syslog` action 的内核缓冲语义。公共类型契约见
[`api-v0`](../../klog-api/api-v0/README.md)，整体边界见 [wateros-klog](../../README.md)，
用户指针处理见 [`sys/misc`](../../../wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/README.md)。

## 模块地图

| 文件 | 职责 | 不应承担的职责 |
| --- | --- | --- |
| `src/state.rs` | `Slot`、`KlogRingbufInner`、覆盖与游标算法 | IRQ、全局实例、用户 copy |
| `src/global.rs` | `KLOG`、IRQ guard、锁闭包、时间/task 上下文 | traditional 行格式、用户地址验证 |
| `src/format.rs` | 512 字节宏格式化、`<level>text\n` 输出 | 堆分配、console 输出 |
| `src/syslog.rs` | action 分派和内核缓冲读写 | `copy_{to,from}_user`、syscall 参数寄存器解释 |
| `src/lib.rs` | 对外重导出和最小 self-test | 保存第二份状态 |

外部公开入口只有 `init`、`record`、`record_fmt`、`stats`、`dispatch_kernel`。内部调用者
若绕开这些入口直接操作状态，就会绕过 IRQ/锁不变量。

## 环结构与容量

`KlogRingbufInner` 的核心布局是：

```text
slots: [Slot; KLOG_DESC_SLOTS]
head: 下一次覆盖/写入的槽下标
count: 当前有效槽数，最大 KLOG_DESC_SLOTS
next_seq: 下一条记录的 sequence，初值 1
oldest_seq: 当前环内最小有效 sequence，空环为 0
read_cursor_seq: 全局消费者下一目标，初值 1
records_committed / records_dropped: 饱和统计计数
```

每个 `Slot` 内联 `valid`、一份 `KlogRecordMeta` 和
`[u8; KLOG_MAX_RECORD_BYTES]`。当前配置为 256 槽、每槽 1024 字节；这意味着状态对象本身
至少占约 256 KiB，实际并不存在独立的 32 KiB text ring。`KLOG_TEXT_RING_BYTES=32 KiB`
目前只由 `buffer_bytes()` 报给 `SIZE_BUFFER`，改容量时不要把报告值和真实内存占用混为一谈。

### 追加状态机

```text
copy_len = min(text.len, 1024)
若超长：meta.flags |= TRUNC
index = head
若已满：统计 dropped；被覆盖的 seq 正好是 cursor 时先推进 cursor
否则：count += 1
head = (head + 1) % slots
meta.seq = next_seq；next_seq 饱和 +1
整槽替换并复制正文
重新计算 oldest_seq；把落后的 cursor 夹到 oldest_seq
records_committed 饱和 +1
```

环满时写入永不阻塞，代价是覆盖最旧记录。`records_dropped` 只统计发生过多少次覆盖，不能
恢复具体丢失区间。`next_seq` 使用饱和加法；到达 `u64::MAX` 后会重复最大序号，当前实现没有
处理这一理论上的极长期运行边界。

### 读取与游标

`peek_next_unread` 扫描有效槽，选择 `seq >= read_cursor_seq` 的最小 sequence，并返回正文
借用视图。`advance_read_cursor(seq)` 把游标设为 `seq+1`，再夹到 `oldest_seq`；`CLEAR` 调用
`clear_read_cursor()`，直接设为 `next_seq`，不会清零槽或统计。

当前只有一个全局 `read_cursor_seq`。普通 `READ` 使用 `advance=false`，会反复返回同一条；
`READ_CLEAR` 读一条并推进；`READ_ALL` 每取一条就推进。这不是每 fd reader 模型。

还有一个重要的失败语义：`READ_ALL` 在锁内先推进游标，之后 syscall 层才 `copy_to_user`。
如果最终用户复制失败，已经推进的记录不会回滚；`READ_CLEAR` 也一样。若比赛要求“坏指针不
消费日志”，需要设计两阶段读取（复制/预留 token/成功后 commit），不能只在 syscall 层
调换两行代码，因为正文借用可能在解锁后被覆盖。

## 全局锁、中断与生命周期

全局状态是：

```rust
TrackedMutex<Option<KlogRingbufInner>>
```

`Option` 支持首次访问惰性构造；`init()` 通过同一入口 reset。每次 `KlogRingbuf::with` 的
严格顺序为：

```text
读取本 CPU 全局中断状态
→ 关闭本 CPU 全局中断
→ 获取 KLOG TrackedMutex
→ 在闭包中访问 ring
→ 释放 mutex
→ 恢复进入前的中断状态
```

guard 恢复的是原状态，而不是无条件开中断。关闭本 CPU IRQ 防止中断处理路径在同一 CPU
重入 klog 自旋死锁；mutex 负责 SMP 互斥。二者并不允许锁内做任意工作。

`with` 闭包内禁止：

- 再次调用任何 klog 宏或 `record`；
- `log::*`/console 输出（可能引入未知锁序或重入）；
- 用户内存 copy、页故障处理、调度、等待队列和阻塞；
- 获取可能在其它路径先持有再写 klog 的锁；
- 把 `KlogRecordView.text` 保存到闭包外。

外部时间和 caller ID 都在取得 ring 锁前采集。新增上下文元数据也应沿用这一模式：先获取
可能失败/阻塞的数据，再进入极短临界区提交纯内存状态。

## 写入调用链

宏路径：

```text
klog_info!(...)
→ wateros-klog 门面生成 format_args!
→ impl_kernel::record_fmt(level, Arguments)
→ KlogFmtBuffer[512] 在当前栈格式化
→ global::record(level, LOG_KERN, bytes)
→ ts_nsec_now + caller_id_now
→ record_with_meta
→ KlogRingbuf::with（关 IRQ + KLOG.lock）
→ KlogRingbufInner::append
```

直接路径：

```text
record(level, facility, &[u8])
→ 构造 meta
→ 同一 append 链
```

两条路径都不会打印 console，也不使用堆。runtime 的 `log::info!` 走即时 console logger，
不会自动进入 klog；需要双写时必须显式设计桥接，并警惕 console 锁与 KLOG 锁的顺序。

`KlogFmtBuffer` 容量只有 512 字节，`fmt::Write::write_str` 在空间耗尽后仍返回 `Ok(())`，所以
宏调用方看不到截断，环也不会设置 `TRUNC`。若修复这一点，可让 buffer 记录
`truncated: bool`，再构造带 `TRUNC` 的 meta；不要在日志路径改用 `String`。

## `sys_syslog` 完整链路

```text
用户 syscall(type, buf, len)
→ syscall_nr_dispatch::SYSLOG
→ sys::misc::sys_syslog
   ├─ WRITE：copy_from_user 到 [u8; 2048]
   └─ READ 类：先准备 [u8; 2048]
→ klog::syscall::dispatch_kernel
→ impl_kernel::syslog::dispatch_kernel
→ ring 操作/传统行格式化
→ syscall 层 copy_to_user（READ 类）
→ UserRet
```

实现层收到的 `kernel_buf` 必须是真正的内核切片。它不验证空指针、可读写权限或跨页映射，
也不返回 `ErrNo`。这些工作只能在 syscall 层完成。

### action 当前语义

| action | 实现行为 | 游标行为 |
| --- | --- | --- |
| `OPEN/CLOSE` | 返回 0 | 不变 |
| `READ` | 格式化下一条到 `<n>text\n` | 不推进 |
| `READ_CLEAR` | 格式化下一条 | 在内核 copy 前推进一条 |
| `READ_ALL` | 循环格式化，最多填满传入长度 | 每条在锁内推进 |
| `CLEAR` | 返回 0 | 跳到 `next_seq`，记录保留 |
| `CONSOLE_*` | no-op 返回 0 | 不变，也不控制 runtime console |
| `SIZE_UNREAD` | 正文长度求和 | 不变 |
| `SIZE_BUFFER` | 返回 32 KiB 配置报告值 | 不变 |
| WRITE priority | 写入 `USER` 记录并返回消息长度 | 不变 |

未知非零整数会先被 `is_write_priority` 判为 WRITE；真正落入 match 的未知值目前会 panic，
并没有 `EINVAL` 通路。READ 内部除 `NoUnread` 外的错误也 panic。要提高 ABI 健壮性，应让
`dispatch_kernel` 返回结构化错误，再由 syscall 层映射 errno，而不是在本层直接构造负数。

`read_all` 若剩余空间不足一整行，会复制行前缀并推进该条游标，然后退出，因此被部分复制的
记录也视为已消费。traditional formatter 保证非空输出缓冲被截断时最后一个字节尽量是
换行；长度为 0 时返回 0。

## 安全添加新日志入口实例

例如增加保存设备号的内核记录 helper，而不改变用户 ABI：

```rust
pub fn record_device(level: u8, device_id: u32, text: &[u8]) -> AppendResult {
    // 在锁外把 device_id 编入固定栈缓冲或新增版本化 meta 字段。
    // 不可在 KlogRingbuf::with 闭包中查询设备、分配或打印。
    record(level, api_v0::LOG_KERN, text)
}
```

若 `device_id` 必须可查询，不应偷偷挪用 `caller_id` 或 flags 未知位。应先升级 API 元数据或
设计独立 payload 版本，再同步 formatter、统计/导出路径和兼容测试。

## 实现每 reader 游标的改造路线

1. 在 API 定义调用者持有的 `KlogCursor { next_seq }`，而不是继续扩大全局状态。
2. 把 `peek_next_unread` 改为接收 cursor；覆盖后把传入 cursor 夹到 `oldest_seq`。
3. 明确 `READ`、`READ_CLEAR` 与 `READ_ALL` 是否共享 Linux 全局游标；若要 fd 私有 reader，
   reader 的生命周期应挂到 fd/session，而非 syscall 的一次栈帧。
4. 解决用户复制失败事务：可在锁内复制到内核缓冲但不 commit，用户复制成功后用 sequence
   条件提交；提交时必须处理期间发生的覆盖和其它 reader 竞争。
5. 增加双 reader、覆盖、短缓冲、坏指针、并发 writer 的测试。

不要将 `KlogRecordView` 直接保存进 reader，因为它借用的槽随下一次覆盖失效。

## 故障定位表

| 现象 | 优先检查 |
| --- | --- |
| 记录写入后 `dmesg` 看不到 | 是否用了 runtime `log!` 而非 klog 宏；游标是否已被 CLEAR/其它 reader 推进 |
| 多次 READ 返回相同内容 | 普通 READ 本来不推进；确认调用 action |
| 压力下日志跳号 | `stats().records_dropped`、槽数和 reader 速度 |
| CPU 卡在 klog 锁 | 是否在 `with` 内递归记录、console 输出、等待或违反锁序 |
| 中断状态异常 | 是否绕过 `KlogInterruptGuard`；恢复是否使用保存的原状态 |
| `SIZE_BUFFER` 与内存占用不符 | 当前报告 32 KiB，但真实正文为 256×1024 内联槽 |
| 长宏日志无 `TRUNC` | 先被 512 字节 formatter 截断 |
| EFAULT 后日志被消费 | 当前游标先推进、用户 copy 后发生；需要两阶段提交 |
| 未知 action 导致奇怪 USER 记录 | `is_write_priority` 将未知非零数视为 WRITE；检查 API 判别表 |

## 回归与检查清单

- [ ] 所有 ring 状态访问都经过 `KlogRingbuf::with`。
- [ ] 外部上下文在取得 KLOG 锁前准备，闭包中无日志、调度、用户 copy 或阻塞。
- [ ] append 覆盖后 `oldest_seq`、cursor 和 dropped 统计仍一致。
- [ ] READ 三种 action 的推进差异与测试、文档一致。
- [ ] 短缓冲/部分行的“已消费”语义经过显式决定。
- [ ] 新 action 已在 API `decode_action` 注册，且 syscall 层做了相应指针/errno 处理。
- [ ] 没有为日志格式化引入堆分配。
- [ ] RV 与 LA 顶层 feature 组合均能构建。

推荐验证：

```bash
cd os
cargo test --manifest-path components/wateros-klog/klog-impl/impl-kernel/Cargo.toml
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

单 crate 测试可能缺少顶层选择的 `platform/task/arch/debug` feature；遇到这种情况先确认是
feature 图缺失还是代码错误，并始终以两个架构的顶层 `make check` 作为集成回归依据。
