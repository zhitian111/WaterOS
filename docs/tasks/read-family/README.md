# `read` 调用族修复任务索引

[项目首页](../../../README.md) · [文档总览](../../README.md) · [任务总览](../README.md)

## 范围

本目录覆盖当前已经确认的 `read(2)`、`readv(2)`、`pread64(2)`、`preadv(2)` 问题，
以及为了正确修复它们必须调整的 MM 用户拷贝、VFS open-file-description、pipe、
Unix socket、inet socket、eventfd 和字符设备接口。

已修复的 BuildStorm 阻断是：大于 4 MiB 的 `read` 不再直接返回 `EINVAL`，而是返回
合法短读。该修复不是本任务集的终点。问题证据见
[`buildstorm-cargo-index-filesystem-report.md`](../cross-task-reports/reports/buildstorm-cargo-index-filesystem-report.md)。

## 已确认问题

| 编号 | 问题 | 直接后果 |
|---|---|---|
| R1 | `count == 0` 在 fd/type 校验前返回 0 | 无效 fd、写端、目录 errno 错误 |
| R2 | `ptr == NULL` 在 fd 校验前返回 `EFAULT` | `read(-1, NULL, 1)` 错误优先级不符 Linux |
| R3 | 普通文件和特殊句柄未统一检查访问模式 | `O_WRONLY` 文件可读，pipe 写端返回错误 errno |
| R4 | 先消费底层数据，再 `copy_to_user` | `EFAULT` 后文件偏移前进，pipe/socket 数据丢失 |
| R5 | user-copy 丢失跨页部分复制进度 | 无法只提交已复制字节 |
| R6 | `dup/fork` 复制句柄状态 | 文件偏移和 `O_NONBLOCK` 等 OFD 状态不共享 |
| R7 | pipe/Unix/inet/eventfd 只有破坏性 read | 无法实现 copy 成功后再提交 |
| R8 | `readv` 单段超过 4 MiB 返回 `EINVAL` | 内核缓冲上限泄漏为用户 ABI |
| R9 | iovec 地址运算、后段 fault 和部分成功处理不统一 | 溢出风险及错误返回不符合 Linux |
| R10 | `pread64` 按用户长度直接分配 `Vec` | 巨额分配、OOM 风险，与 `read` 上限策略不一致 |
| R11 | 热路径遗留逐 iovec/read 的 trace/info | 压力测试日志开销和噪声 |
| R12 | 大请求在 fd/access 校验前分配 staging | 无效请求也制造最高 4 MiB 堆压力 |
| R13 | `with_current_io` 持 spin mutex 执行阻塞 read | 同一 fd 并发操作可能长期自旋，锁序难以证明 |

## 架构决定

不要用“先 probe 用户地址，再破坏性读取”作为最终修复。probe 后另一线程仍可
`munmap/mprotect/fork`，存在 SMP TOCTOU。

也不要直接把帧引用计数当写 pin。当前 fork/COW 不区分映射引用与内核写 pin，pin 后
并发 fork 再直接写物理页可能绕过 COW。

本任务集采用以下方向：

1. MM 用户拷贝返回“已复制字节数 + fault”。
2. VFS 读取源创建短锁保护的读取租约，锁外执行 user-copy。
3. 租约按实际复制字节提交；未提交部分回滚且保持原顺序。
4. 固定宽度或报文型对象可要求全量提交。
5. `dup/fork` 共享 OFD 状态，fd descriptor flags 仍保持每 fd 独立。
6. fd 短锁内只生成拥有 `Arc` 稳定状态的 prepared read；等待、ext4/network I/O 和
   user-copy 全部在 fd/OFD spin lock 外执行。

## 依赖与并行关系

```text
RIO-01 访问模式与 errno ───────────────┐
RIO-02 user-copy 部分进度 ────────┐    │
RIO-03 OFD 共享状态 ──────────────┴─> RIO-04 统一读取租约+文件
                                       ├─> RIO-05 pipe/Unix socket
                                       ├─> RIO-06 inet socket
                                       ├─> RIO-07 eventfd
                                       └─> RIO-08 字符与伪设备

RIO-01 + RIO-02 + RIO-04 ────────────> RIO-09 readv/pread 收敛
RIO-01..09 ───────────────────────────> RIO-10 集成验收
```

`RIO-01`、`RIO-02`、`RIO-03` 可并行。`RIO-05` 至 `RIO-08` 在 `RIO-04` 的 API
提交合入后可并行。`RIO-09` 可先完成 iovec 解析部分，但最终接入必须等待所有读取源。

## 任务文件

| 状态 | 文件 | 交付 |
|---|---|---|
| [ ] | [`rio10/task.md`](./rio10/task.md) | Linux 对照、LTP、双架构和 BuildStorm |

RIO-01 至 RIO-09 的实现已进入当前代码；其任务定义已退役，但每项记录保留在对应
`rioXX/history/`。后续修改以 RIO-10 的集成回归为准。

## 共同完成要求

- 先读各任务列出的 prompt、导出文档和源码，不直接照抄建议接口名。
- API 放 `api-v0`，算法放 `impl-*`，最终能力从聚合 crate 导出。
- 不能在持有 pipe、network stack、OFD 或地址空间自旋锁时进行可能 fault、分配或睡眠的
  user-copy。
- 不允许用定时轮询、吞掉 errno、复制回已消费数据的临时 hack 代替提交协议。
- 每项单独提交，至少运行 `cd os && make rv_check && make la_check`。
- 测试使用独立 qcow2 overlay，不修改原始评测镜像。
