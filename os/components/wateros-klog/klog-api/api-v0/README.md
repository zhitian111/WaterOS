# wateros-klog-api-v0 离线开发手册

本 crate 只定义 klog 的版本化内核契约：记录元数据、标志、`syslog(2)` action、环存储
trait 和统计结构。它不创建全局环、不关中断、不取得锁，也不接触用户地址。实际存储和
并发策略在 [`impl-kernel`](../../klog-impl/impl-kernel/README.md)，用户态 ABI 接线在
[`sys/misc/syslog`](../../../wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/README.md)。
模块总览与完整数据流见 [wateros-klog](../../README.md)。

## 代码地图与边界

| 文件 | 公开内容 | 修改时重点 |
| --- | --- | --- |
| `src/action.rs` | `SYSLOG_ACTION_*`、`decode_action`、`is_write_priority` | action 0..=10 与 WRITE priority 的判别不能重叠 |
| `src/meta.rs` | facility/level 常量、`KlogRecordMeta` | 这是内核内部结构，不可当作用户态裸 ABI |
| `src/flags.rs` | `KlogFlags::{CONT,TRUNC,USER}` | 未知位应保留，组合使用 `with` |
| `src/store.rs` | `KlogStore`、借用视图、统计快照 | trait 不暴露锁类型，但调用者必须维持视图生命周期 |
| `src/error.rs` | `KlogError`、`AppendResult` | errno 映射属于 syscall/实现层 |
| `src/lib.rs` | 统一重导出 | 新公共类型必须从此处可见 |

依赖方向应保持为：

```text
base-config（容量常量）
        ↑
klog-api-v0（纯契约）
        ↑
klog-impl-kernel（状态、锁、平台上下文）
        ↑
wateros-klog 聚合门面 ← syscall 实现
```

不要把 `platform`、`task`、`arch`、用户复制或某个 mutex 类型放进本 API；否则 API
版本会反向绑定具体内核实现，架构移植和单元测试都会变难。

## 记录元数据

`KlogRecordMeta` 使用 `#[repr(C)]` 固定字段顺序，但目前只用于内核内部观测，并没有被
`copy_to_user` 原样导出：

| 字段 | 谁填写 | 含义与不变量 |
| --- | --- | --- |
| `seq: u64` | `KlogStore::append` | 提交序号；调用者用 `new` 构造时为 0 |
| `ts_nsec: u64` | 全局实现 | 单调时钟纳秒；时间服务未就绪时允许为 0 |
| `text_len: u16` | `append` 覆盖 | 实际保存的正文字节数，不是原输入长度 |
| `facility: u8` | 调用者/WRITE priority | 当前常用 `LOG_KERN=0`、`LOG_USER=1` |
| `flags: u8` | 调用者与 `append` | `KlogFlags` 的位域；截断时实现追加 `TRUNC` |
| `level: u8` | 调用者 | syslog 级别 0..7；传统输出只取低 3 位 |
| `caller_id: u32` | 全局实现 | 写入时任务 ID；启动早期无任务时为 0 |

`KlogRecordMeta::traditional_level_char()` 返回 `b'0' + (level & 7)`。传统输出当前是
`<level>text\n`，只编码 level，不编码 facility、时间戳或 caller ID。若要向用户态导出
完整元数据，应定义新的显式 ABI 结构和版本，不能直接复制本结构：字段填充、对齐、端序和
未来兼容性都需要独立约定。

## 标志位

- `CONT`：续行标志，目前存储层只保存，不负责合并续行。
- `TRUNC`：存储层收到的正文超过单槽上限时设置。
- `USER`：消息来自 `sys_syslog` WRITE 路径。

一个容易误判的限制是：`klog_info!` 等宏先经过实现层 512 字节格式化缓冲，超出部分在那里
静默丢弃；环只看到不超过 512 字节的切片，因此这条路径目前不会设置 `TRUNC`。直接调用
`record(..., text)` 且 `text` 超过单槽上限时，环层才会设置该位。

## action 与 WRITE priority 判别

已知 action 为 0..=10：

| 值 | 常量 | 当前实现意图 |
| ---: | --- | --- |
| 0/1 | `CLOSE` / `OPEN` | 兼容占位 |
| 2 | `READ` | 读下一条但当前不推进全局游标 |
| 3 | `READ_ALL` | 循环读取并逐条推进游标 |
| 4 | `READ_CLEAR` | 读一条并推进游标 |
| 5 | `CLEAR` | 游标跳到最新之后，记录仍留在环中 |
| 6/7/8 | `CONSOLE_OFF/ON/LEVEL` | 当前为 no-op |
| 9 | `SIZE_UNREAD` | 游标之后的正文总字节数近似值 |
| 10 | `SIZE_BUFFER` | 配置声明的 text 容量 |

`decode_action(raw)` 只识别上表；`is_write_priority(raw)` 对“未知且非 0”的值返回 true。
因此新增 action 时必须先把值加入 `decode_action`，否则它会被误送进 WRITE 路径。当前 WRITE
priority 解码约定由实现层解释为：

```rust
let level = ((priority >> 3) & 7) as u8;
let facility = (priority & 7) as u8;
```

这与常见 syslog priority 的 facility/level 位排布并不相同；修改前必须同步核对已有用户态
程序和比赛测试，不能只依据常见 libc 习惯替换。

## `KlogStore` 契约

`KlogStore` 把存储算法和全局锁解耦。各方法的约束如下：

| 方法 | 是否修改状态 | 关键语义 |
| --- | --- | --- |
| `append` | 是 | 覆盖 `meta.seq/text_len/flags`，返回序号与截断状态 |
| `stats` | 否 | 返回某一锁内时刻的复制快照 |
| `unread_bytes` | 否 | 仅正文近似总量，不包含 `<n>` 和换行格式开销 |
| `buffer_bytes` | 否 | `SIZE_BUFFER` 报告值，不保证等于实际对象占用 |
| `peek_next_unread` | 否 | 返回不小于读游标的最小有效 sequence |
| `advance_read_cursor` | 是 | 把下一读位置设为 `after_seq + 1`，并夹到现存最旧序号 |
| `clear_read_cursor` | 是 | 指向当前 `next_seq`，只标记已读而不擦除槽 |

`KlogRecordView<'a>` 的 `meta` 是副本，`text` 却直接借用 store 内存。它只能在实现所提供的
锁闭包内即时消费：不可返回到闭包外、保存到全局、跨调度使用，也不可在获得视图后执行可能
追加 klog 的代码。需要跨边界时先复制正文，再释放锁。

全局读游标是当前 API 的组成部分。它不是 fd/进程私有状态，多个 `dmesg`/`syslog` 消费者
可能彼此影响。要实现独立 reader，不能简单给 `peek_next_unread` 增加一个布尔参数；应把
cursor 从 store 全局状态拆成调用者持有的 token/reader 对象，同时明确覆盖后的夹紧规则。

## 添加 action 的完整实例

假设要新增只读 action `SYSLOG_ACTION_SIZE_RECORDS=11`：

1. 在 `action.rs` 定义常量，并加入 `decode_action` 的 match；这是防止它被当成 WRITE 的关键。
2. 若需要新存储能力，在 `KlogStore` 添加最小的语义方法，例如 `record_count()`；不要暴露
   `Slot`、`head` 或锁实现。
3. 在 impl 的 `dispatch_kernel` 增加分支并返回 `isize`。
4. 在 syscall 层决定是否需要缓冲区、指针检查、权限检查以及错误到 `ErrNo` 的映射。
5. 添加 API 判别测试：新 action 的 `decode_action` 为 `Some` 且 `is_write_priority` 为 false。
6. 添加实现测试与 RV/LA 顶层 `make check`；若是用户 ABI，还要用用户态程序检查返回值。

若新增 action 会复制结构体到用户态，还需额外定义 `#[repr(C)]` 用户 ABI、固定整数宽度、
长度协商、向后兼容策略和坏指针测试。

## 常见错误与排查

- **新增 action 后写入了一条怪日志**：先查是否忘记更新 `decode_action`。
- **连续 `READ` 总是同一条**：这是当前语义；普通 READ 的 `advance=false`。消费式读取使用
  `READ_CLEAR` 或 `READ_ALL`。
- **`SIZE_UNREAD` 大于实际复制量**：它只按正文求和，而输出还会增加 `<n>` 与换行，并受
  用户缓冲、2048 字节 syscall 栈缓冲限制。
- **拿到 view 后出现正文变化或越界**：检查是否让借用跨越锁边界或在锁内发生追加。
- **`TRUNC` 未出现但消息明显被截短**：检查是否先被 512 字节宏格式化缓冲截断。
- **想把 `KlogError` 直接返回用户态**：先在 syscall 边界映射为稳定 `ErrNo`；API 错误不是
  Linux errno ABI。

## 修改检查清单

- [ ] API 中没有用户指针、平台时钟、当前任务或具体锁类型。
- [ ] 新 action 已加入 `decode_action`，不会落入 WRITE priority。
- [ ] 新字段没有被误认为现有用户态 ABI；布局变更有版本策略。
- [ ] `KlogRecordView` 没有逃逸实现锁的生命周期。
- [ ] 游标推进、覆盖夹紧和 `NoUnread` 的语义有测试。
- [ ] 文档与实现的 READ/READ_CLEAR/READ_ALL 差异保持一致。
- [ ] 通过目标架构顶层构建，而不是只检查这个无架构 API crate。

## 验证入口

```bash
cd os
cargo test --manifest-path components/wateros-klog/klog-impl/impl-kernel/Cargo.toml
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

实现 crate 依赖平台、任务和架构选择，若单独 `cargo test` 因 feature 组合不完整失败，应以
顶层 WaterOS 的 RV/LA feature 图为准，并保留 API/状态机的最小单元测试。

