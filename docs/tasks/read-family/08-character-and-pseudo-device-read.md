# RIO-08：字符设备与伪设备读取

## 任务目标

审计并适配所有经 `VfsIoHandle::read()` 进入的字符/伪设备，保证访问模式准确，并对会
消费状态的 UART、随机设备等实现可提交读取。纯生成或 EOF 设备使用轻量 lease。

## 前置条件

- RIO-01、RIO-02、RIO-03、RIO-04 已合入。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-driver.md`
- `docs/exports/public-api/wateros-driver.md`
- `docs/exports/impl-guide/wateros-driver.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/exports/public-api/wateros-vfs.md`
- `docs/exports/impl-guide/wateros-vfs.md`
- `docs/tasks/read-family/README.md`
- `docs/tasks/read-family/04-vfs-read-lease-and-files.md`

## 涉及文件

- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/handles.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/char_dev_handle.rs`
- `os/components/wateros-driver/driver-character/character-api/api-v0/src/lib.rs`
- `os/components/wateros-driver/driver-character/character-impl/`

当前可读对象包括：

| 对象 | 当前语义 | 是否有破坏性状态 |
|---|---|---|
| ConsoleIn | bring-up 占位 EOF | 否 |
| ConsoleOut | 仅写 | 不应允许 read |
| `/dev/null` | EOF | 否 |
| `/dev/zero` | 生成 0 | 否 |
| `/dev/urandom` | 推进全局 PRNG state | 是 |
| `/dev/cpu_dma_latency` | 当前读 EOF | 否 |
| RTC stub/device | driver snapshot | 通常否，需确认实现 |
| UART/serial tty | driver read 消费 RX | 是 |

## 已知信息与代码证据

`CharDevHandle::read()` 直接调用 driver：

```rust
let mut guard = self.device.lock();
guard.read(buf)
```

syscall 随后才执行 user-copy，因此 UART 字节可能在 `EFAULT` 时丢失。`/dev/urandom`
也会在 user-copy 前修改全局状态。ConsoleOut 等仅写对象如果不覆盖访问模式，会继承
错误的默认只读能力。

## 任务内容

### Stateless 设备

`/dev/null`、`/dev/zero`、EOF stub 和 RTC snapshot 可创建拥有 staging 的轻量 lease：

- `/dev/null`/EOF 返回空 lease；
- `/dev/zero` 按内部 cap 生成零 buffer；
- RTC 在 begin 时生成快照；
- finish 不需要回写设备状态。

### UART/serial

字符驱动 API 当前没有 peek/commit。新增可选读取事务能力，不要让 VFS downcast 具体
UART 实现。建议在 character `api-v0` 定义 driver read lease，或提供
`begin_read/commit_read/cancel_read`。

要求：

- 短锁把 RX 字节移入 reservation，其他 reader 不得越过；
- 未提交字节仍计入 RX capacity；
- user-copy 在 driver 锁外；
- cancel 把未提交前缀恢复到逻辑队首；
- 不支持事务的真实消费型 driver 必须明确返回 `Unsupported`，不能静默退回破坏性
  read。

### `/dev/urandom`

不要在 begin 阶段不可逆地 `fetch_add/store` 全局 PRNG state。可选方案：

- reservation 捕获 old/new state，commit 用 generation/CAS 发布；
- 或将随机流状态放入可串行化的共享 state，active lease 阻止另一个 reader 越过。

首字节 fault 时 state 不变；partial stream copy 时只提交对应前缀后的 state。若决定
随机设备允许不同 Linux 行为，必须先做差分测试并在文档记录，不能凭直觉跳过。

## 访问模式

按 RIO-01 准确报告：

- ConsoleIn `O_RDONLY`；
- ConsoleOut `O_WRONLY`；
- null/zero/urandom 通常 `O_RDWR`；
- 实际 `/dev/*` open 应保留用户请求的 accmode，不由设备类型硬编码覆盖。

## 如何验收

- ConsoleOut read 返回 `EBADF`，包括 count=0。
- zero 跨页 fault 只报告实际复制进度，不访问非法页之后的地址。
- UART 首字节 fault 后，下一次 valid read 仍得到相同字节。
- UART partial fault 保留未提交后缀且顺序不变。
- UART nonblocking/TTY poll 无回归。
- urandom 首字节 fault 不推进内部 generation。
- RTC 和 EOF 节点不被错误地当 `EISDIR` 或 `EINVAL`。
- 所有设备 lease Drop 都不遗留 reservation。

执行：

```bash
cd os
make rv_check
make la_check
```

串口输入不易自动化时，至少提供 character impl 单元测试的 fake FIFO device，并在
RIO-10 记录 QEMU 可执行的真实路径。

## 搜索范围、并行与交付

用 `rg "impl VfsIoHandle|impl CharacterDevice|fn read\\("` 审核 fd-session 的全部伪
设备、character API、每个 active impl 和平台设备注册。将对象按 stateless snapshot、
generated stream、consuming FIFO 分类后逐项确认。

本任务可与 RIO-05、RIO-06、RIO-07 并行。driver 契约放 character `api-v0`，设备算法
放 impl，VFS 只做适配。fake FIFO 测试留在 impl，QEMU 日志放 `/tmp`。完成后在索引
勾选 RIO-08，记录设备矩阵和未具备真实输入环境的限制。

## 禁止做法

- 不在 VFS 中硬编码具体 UART 类型。
- 不持 driver spin lock 跨 user-copy。
- 不把所有设备都宣称 `O_RDWR`。
- 不因当前 QEMU stdin 常为 EOF 就跳过消费型设备协议。
