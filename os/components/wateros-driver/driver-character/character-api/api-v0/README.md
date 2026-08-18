# Character Device API v0 开发手册

[Character Driver 总览](../../README.md) · [VFS FD Session](../../../../wateros-vfs/vfs-impl/impl-fd-session/README.md)

该 API 是 UART、RTC、null 等字节设备与 devfs/VFS 之间的稳定边界。它不实现终端行规程；TTY 在字符设备之上处理 canonical input、echo、控制字符和作业控制。

## 类型与注册表

- `CharacterDeviceKind` 当前有 `Serial`、`Rtc`、`Null`；默认是 Serial，非串口实现必须覆盖。
- `SharedCharacterDevice = Arc<Mutex<Box<dyn CharacterDevice>>>`：Arc 管寿命，mutex 串行所有 `&mut self` 方法。
- `CHARACTER_DEVICES` 是只增不减的全局 `Vec`，注册返回从 0 开始的稳定索引。
- `character_device_at`/`first_character_device` 克隆 Arc；`with_character_device` 在闭包整个执行期持设备锁。

当前没有注销、复用槽位或设备 generation。devfs 若把 index 持久化，新增 hot-unplug 时不能直接从 Vec 删除导致后续 index 移位；应使用 tombstone/generation 或稳定 ID。

## CharacterDevice 契约

`read`/`write` 返回实际字节数，必须允许 partial I/O。空 buffer 通常返回 0；read 的 0 是 EOF，不等同于“暂时无数据”。暂时无数据应由非阻塞层映射为 WouldBlock/EAGAIN，但通用 `DriverError` 当前没有专门枚举，一些实现暂用 `Unsupported`，上层必须知道这个兼容约定。

`poll_revents(events)` 只返回请求且确实 ready 的位。trait 默认会无条件回报所请求的 POLLIN/POLLOUT，只适合 `/dev/null` 这类永远就绪对象；任何会阻塞或暂时无数据的设备都必须覆盖。poll 观察到 readable 后，紧随其后的非阻塞 read 仍可能因并发消费者失败，这是正常竞争，不能把 poll 当 reservation。

`ioctl` 默认 `Unsupported`。实现必须验证 request、arg 指针对齐/长度和方向；用户指针复制属于 syscall 层，不应在 driver 中直接解引用。

## 两阶段 consuming read

`CharacterReadReservation { id, bytes }` 解决“硬件 FIFO 已消费，但 copy_to_user 中途 EFAULT”导致数据丢失：

```text
sys_read
  -> 锁设备，prepare_read(max_len)
  -> 解锁，逐段 copy_to_user
  -> 再锁设备，finish_read(reservation, copied, complete)
  -> 提交 copied 前缀，将剩余后缀按原顺序放回
```

`Ok(None)` 表示事务型设备现在无数据；默认 `Unsupported` 表示实现不支持该协议。`finish_read` 返回 `Bytes(n)` 或“第一个字节都未复制”的 `Fault`。reservation 是线性 owner：必须且只能 finish 一次，不能 clone、遗失或交给另一个设备。

## SerialPortCharacterDevice 状态机

结构包含底层 `port`、回滚队列 `pending`、`active_read: Option<u64>` 和 wrapping `next_read_id`。

- `prepare_read(0)` 返回 id=0 的空 reservation，不占 active；
- 非零读取最大只预留 256 字节，先取 pending，再轮询 UART；
- 已有 active reservation 时返回 None；
- 成功预留记录递增 id；空输入返回 None；
- finish 校验 id 和 copied；未复制后缀逆序 `push_front`，从而恢复原字节顺序；
- copied=0 且 complete=false 返回 Fault，其余返回 Bytes(copied)。

`read` 是全在设备锁内的兼容路径：prepare、复制到内核 buffer、finish。无数据被映射为 `Unsupported`。`write` 逐字节同步写 UART；中途失败返回 `IoError`，但已发送前缀无法回滚，当前也没有返回 partial count。

serial `poll_revents` 会从 UART 预取一个字节进 pending，这是一项有状态操作。active reservation 存在时不报告 readable；POLLOUT 始终 ready。轮询必须先消费 pending，read 也必须先读 pending，否则会乱序/丢字节。

## 锁与故障规则

不得持设备 spin mutex 等待 scheduler、用户内存 fault、TTY 队列空间或另一个锁。两阶段 API 的目的正是让 user-copy 在锁外发生。`with_character_device` 适合短查询，不适合把可能阻塞的任意闭包传入。

当前 reservation bytes 使用 `Vec::try_reserve_exact`，失败映射 `IoError`；全局 registry 的 `Vec::push` 则仍可能触发不可恢复 heap allocation error。设备注册应只在 bring-up 早期、内存充足时发生，或改用 fallible reserve。

## 新设备实例：RTC

实现 RTC 时：kind 返回 Rtc；read 定义固定结构/文本格式并明确 EOF/重复读；若始终可读就使用默认 POLLIN，否则实现真实 readiness；ioctl 的用户结构在 syscall 层 copy in/out，再把验证过的内核值交驱动。构造成功后包装 `Arc<Mutex<Box<_>>>` 注册，再由 devfs 按 kind 创建节点。失败对象不得注册。

## 回归清单

- 空 read/write、partial write 失败、EOF 与暂时无数据区分；
- poll 预取后 read 不丢字符，POLLIN/POLLOUT 掩码正确；
- reservation 全复制、部分 EFAULT、首字节 EFAULT、错误 id、copied 越界、重复 finish；
- pending 后缀恢复顺序，`next_read_id` wrap 时不与 active id 混淆；
- 两个读者竞争时同一字节只交付一次；
- registry index、kind、越界查询和并发注册；
- 长时间串口输入及大量 EFAULT 后 pending/heap 不增长。
