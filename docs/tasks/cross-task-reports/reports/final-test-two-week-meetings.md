# 决赛任务两周会议安排

## 使用范围

本安排覆盖 2026-07-20 至 2026-07-31，两周内每周一、三、五在固定时间同步。具体会议时间和地点由团队补充。

会议日期：

| 次数 | 日期 | 主要目的 |
|---|---|---|
| 第 1 次 | 7 月 20 日，周一 | 确认目标、所有权、接口和第一批任务 |
| 第 2 次 | 7 月 22 日，周三 | 检查构建与设计结果，冻结第一版接口 |
| 第 3 次 | 7 月 24 日，周五 | 第一周集成，确认 8 核启动前置条件 |
| 第 4 次 | 7 月 27 日，周一 | 处理集成问题，安排 8 核正确性联调 |
| 第 5 次 | 7 月 29 日，周三 | 检查 8 核调度、设备和内存测试 |
| 第 6 次 | 7 月 31 日，周五 | 两周验收、风险复盘和下一阶段安排 |

任务来源：

- 总体缺口：[`final-test-readiness.md`](./final-test-readiness.md)
- 三人分工：[`final-test-team-allocation.md`](./final-test-team-allocation.md)
- 多核位置：[`../单核设计转多核设计需要更改的位置的预记录.md`](../单核设计转多核设计需要更改的位置的预记录.md)

## 固定会议规则

### 时间控制

每次会议建议控制在 40 分钟：

1. 5 分钟：确认上次行动项是否完成。
2. 15 分钟：每人 5 分钟报告进度。
3. 10 分钟：处理跨模块阻塞和接口变更。
4. 8 分钟：安排下一次会议前的任务。
5. 2 分钟：复述负责人、交付物和截止时间。

技术问题如果 10 分钟内无法形成决定，应指定一到两人会后调查，并规定返回时间，不在会议中继续展开实现细节。

### 进度报告格式

每位成员会前准备以下内容，会议中只报告事实：

```text
已完成：任务编号、commit、验证命令和结果
正在做：当前任务和预期完成时间
阻塞：需要谁提供什么接口、日志或决定
风险：可能影响本周目标的问题
下一步：下次会议前可验收的交付
```

“完成”必须带 commit 或文档链接以及验证结果。“做了一部分”应说明还缺什么。“测试失败”应说明第一个可靠失败点。

### 任务大小

- 周一安排到周三的任务，目标工期不超过两个工作日。
- 周三安排到周五的任务，优先形成可集成提交。
- 周五安排下周一的任务，以修复、验证和准备联调为主。
- 每人同一时间只保留一个主任务和一个不阻塞主线的备用任务。
- 新发现的问题先进入待办，不应自动替换当前任务。只有 P0 阻断或团队共同决定后才调整优先级。

### 当场必须记录

每次会议结束前记录：

- 已确认的技术决定
- 新增或变化的接口
- 每项行动的唯一负责人
- 截止到哪次会议
- 可检查的交付物
- 尚未解决的阻塞和负责调查的人

## 第一次会议

### 会议信息

```text
日期：2026-07-20
时间：待补充
地点/会议链接：待补充
主持：成员 A
记录：建议由 B、C 轮流记录，第一次可由 A 记录
时长：45 分钟以内
```

### 第一次会议目标

本次会议只需要得到五个结果：

1. 三人确认模块所有权和两周共同目标。
2. 确认 A、B、C 之间第一批 API，不再各自发明重复接口。
3. 确认当前基线：RISC-V64 可静态构建，LoongArch64 有明确构建错误，当前内核不是 8 核实现。
4. 给每人安排一个在周三前可以验收的主任务。
5. 确认代码分支、提交、测试日志和会议记录的统一方式。

### 会前材料

三人至少提前阅读：

- [`final-test-team-allocation.md`](./final-test-team-allocation.md) 的“总览”“共同约定”和本人任务部分
- [`final-test-readiness.md`](./final-test-readiness.md) 的 P0 部分
- [`../单核设计转多核设计需要更改的位置的预记录.md`](../单核设计转多核设计需要更改的位置的预记录.md) 中与本人模块有关的章节

会前不要求写代码，但每人应准备一个问题列表，标出自己需要其他成员提供的接口。

### 会议议程

#### 1. 开场和目标，3 分钟

主持人可以直接说明：

> 决赛任务的第一目标是两个架构在 8 核下正确运行，再处理工具链兼容和性能。未来两周先消除构建和 SMP 基础阻断。今天要确定模块边界、接口和周三前的交付，不讨论没有数据支撑的性能重构。

共同目标暂定为：

- 第一周：双架构构建恢复；CPU/task/driver 的 SMP 方案和第一版接口落地；具备开始 8 核联调的条件。
- 第二周：争取两个架构至少完成 8 CPU 上线和基础调度；并发 task、内存和设备测试有可定位结果。

第二周目标受 LoongArch AP 启动协议和现有代码问题影响，会议上应明确它是目标，不提前宣称一定完成。

#### 2. 确认当前事实，5 分钟

由 A 简要说明静态检查结论：

- `make rv_check` 通过，有现存 warning。
- `make la_check` 失败，`PlatformTime` trait 需要 `get_time_frequency_hz`，实现仍为 `time_frequency_hz`。
- 两个架构的启动汇编都使用一份 boot stack。
- RISC-V 次核进入 Rust 后永久 WFI，LoongArch 没有 AP 路径。
- scheduler、process registry、frame allocator、fd/cwd/cred 等仍有 `UniprocessorSafeCell`。
- QEMU 脚本固定为 `-smp 1 -m 1G`。
- `/proc/uptime` 和 `/proc/net/tcp` 缺失，`/proc/cpuinfo` 固定报告 CPU 0。

这一段只用于建立共同事实。对某条有异议时，指定会后用代码或运行日志核实。

#### 3. 确认所有权，5 分钟

逐项确认：

| 范围 | 负责人 | 评审人 |
|---|---|---|
| platform、启动、IPI、MM、VFS、syscall、procfs、集成 | A | 涉及 task 时 B 评审，涉及 driver 时 C 评审 |
| task、scheduler、process lifecycle、task 相关 futex/waitqueue | B | A |
| `driver/network`、virtio-net、smoltcp | C | A |

需要当场问清：

- B 是否同时负责 futex/waitqueue 中与 task 调度强相关的改造？
- 确认 C 只负责 network，不负责 block driver、block cache、socket syscall 或 procfs。
- `os/src/main.rs` 是否统一由 A 集成？建议统一由 A 修改，B/C 通过 API 接入。

如成员实际时间不足，应在这里缩小范围，不要默认会后自行协调。

#### 4. 冻结第一批接口，10 分钟

第一次会议只冻结名称和职责，具体类型允许周三前由实现者调整。

A 与 B 的详细接口和数据结构以 [`smp-a-b-first-interface-contract.md`](./smp-a-b-first-interface-contract.md) 为评审底稿。

A 提供给 B/C：

```rust
current_cpu_id() -> usize
online_cpu_count() -> usize
online_cpu_mask() -> CpuMask
send_reschedule_ipi(cpu_id)
```

B 提供给 A：

```rust
current_task_id()
cpu_current_task(cpu_id)
task/process snapshot
address_space_active_cpu_mask(aspace_id)
```

C 提供给 A：

```rust
tcp_connection_snapshots()
network_tx_rx_statistics()
```

当场确认以下规则：

- snapshot 不返回内部可变引用。
- snapshot 调用结束后不继续持有 registry 或 network lock。
- platform API 不依赖 task crate，避免循环依赖。
- task 可以依赖 platform 的 CPU/IPI API。
- procfs 只消费 B/C 的快照，不读取其私有全局对象。

#### 5. 分配周三前任务，12 分钟

每人只有一个主任务。备用任务只在主任务提前完成后开始。

##### 成员 A

主任务：**A0.1/A0.2 恢复双架构构建基线。**

交付物：

- 修复 LoongArch `PlatformTime` trait 接口漂移
- `make rv_check` 和 `make la_check` 的结果
- 一个只包含相关修复的 commit

完成后备用任务：整理 A1 的 CPU API 最小草案，标出放置 crate 和依赖方向，不要求周三前完成 AP 启动。

##### 成员 B

主任务：**B1 调度器 SMP 改造设计和最小骨架。**

交付物：

- 列出两个 scheduler impl 中 current task、ready queue、wait queue 和 `__switch` 的关键入口
- 给出 per-CPU current/idle 与 `Running(cpu_id)` 的数据结构草案
- 明确 scheduler lock 在哪里获取和释放
- 确认是否可以先只改当前默认启用的 multi-class，再同步 round-robin
- 如接口已确定，提交不改变单核运行语义的 CPU id 抽象或数据结构骨架

周三验收重点是设计是否覆盖所有调度入口，不要求两天内完成整个 SMP scheduler。

##### 成员 C

主任务：**C0 真实 virtio 网卡和外网链路验证。**

交付物：

- 两个架构是否成功注册真实 virtio-net，是否退回 loopback-only
- virtio-net TX/RX 计数或等价日志
- 是否能连接 QEMU 网关、固定外部 IPv4，以及 UDP DNS `10.0.2.3:53`
- 每一层的第一个失败点和复现命令
- 简要列出 `NETWORK_STACK`、virtio-net 和 poller 的多核风险
- 给出 `tcp_connection_snapshots()` 的数据结构草案，不要求周三前实现 procfs

周三验收重点是回答“当前网络能否通过 QEMU 访问外部 IPv4和 DNS”，并把失败定位到网卡注册、帧收发、路由、TCP/UDP 或用户态配置中的一层。

#### 6. 确认工作流，5 分钟

当场决定并记录：

- 三人使用独立分支还是独立 worktree
- commit 命名规则
- 谁负责合入主分支
- 测试日志放置目录和命名规则
- API 变更通过什么方式通知
- 周一、三、五会议的固定时间和记录负责人轮换顺序

建议：

```text
分支：final/a-*、final/b-*、final/c-*
提交：smp(A1): ... / task(B1): ... / driver(C1): ...
日志：不提交大日志；会议记录只链接本地路径或摘要
集成：A 负责，模块负责人负责解决本人模块冲突
```

#### 7. 复述决定，2 分钟

主持人逐项复述：

- A、B、C 各自周三前的交付物
- 需要谁先提供哪个接口
- 当前未解决的问题由谁调查
- 周三会议时间和记录人

三人确认后结束会议，不在最后两分钟新增大型任务。

### 第一次会议必须形成的决定

会议结束时，下表不得留空：

| 决定 | 结果 |
|---|---|
| 两周第一优先级 | 双架构构建和 8 核正确性基础 |
| A 主责范围 | 待会议确认 |
| B 主责范围 | 待会议确认 |
| C 主责范围 | 待会议确认 |
| `os/src/main.rs` 集成人 | 建议 A |
| CPU/IPI API 提供者 | 建议 A |
| task snapshot 提供者 | 建议 B |
| network snapshot 提供者 | 建议 C |
| 主分支集成人 | 建议 A |
| 周三会议时间 | 待补充 |
| 周三记录人 | 待补充 |

### 第一次会议纪要模板

```markdown
# 决赛任务同步 01

日期：2026-07-20
参加：A、B、C
主持：A
记录：

## 当前基线

- RISC-V64：
- LoongArch64：
- 当前可运行配置：

## 已确认决定

- 模块所有权：
- 第一批接口：
- 分支与集成方式：
- 日志与验证记录方式：

## 行动项

| 任务 | 负责人 | 截止 | 交付物 | 验证方法 |
|---|---|---|---|---|
| A0.1/A0.2 | A | 7 月 22 日会议前 | commit + 双架构 check 结果 | make rv_check / make la_check |
| B1 设计和骨架 | B | 7 月 22 日会议前 | 设计说明或 commit | 入口覆盖检查 + 单核 check |
| C0 外网链路验证 | C | 7 月 22 日会议前 | 双架构分层结果 + 日志 + API 草案 | virtio 注册、TX/RX、网关、外部 IPv4、DNS |

## 阻塞和风险

| 问题 | 负责人 | 返回时间 | 需要的决定或证据 |
|---|---|---|---|
|  |  |  |  |

## 下次会议

时间：2026-07-22，具体时间待补充
记录人：
重点：检查三个交付物，冻结 CPU/task/network 第一版接口
```

## 后续五次会议重点

### 第二次：7 月 22 日

- 验收 A 的双架构构建结果。
- 评审 B 的 scheduler 状态机和锁边界。
- 评审 C 的 driver/network 锁清单和 TCP snapshot API。
- 冻结第一版 CPU/task/network 接口。
- 安排周五前可合入的最小提交。

### 第三次：7 月 24 日

- 合并第一周提交并运行双架构静态检查。
- 确认每核 boot stack、BSP/AP、scheduler 和 driver 的集成顺序。
- 列出所有阻塞 8 核首次启动的问题。
- 安排周末/下周一前的首次 8 核启动尝试。

### 第四次：7 月 27 日

- 查看首次 `-smp 8` 日志，每个问题只追第一个可靠失败点。
- 确认 CPU 上线、trap/timer、调度和设备初始化状态。
- 安排 task、MM、driver 三类并发定向测试。

### 第五次：7 月 29 日

- 验收 8 核 task owner、pthread/futex、page fault 和 IO 测试。
- 处理跨核 TLB、丢唤醒、设备锁或死锁问题。
- 判断周五可以完成哪些两周目标，主动延后不成熟的性能工作。

### 第六次：7 月 31 日

- 对照两周完成定义逐项验收。
- 汇总已完成、未完成、阻塞及证据。
- 确认下一阶段是继续 SMP 正确性，还是进入 toolchain/minibuild。
- 只有 BuildStorm 已首次成功时才安排系统性性能优化。

## 两周验收目标

最低目标：

- 双架构静态构建通过。
- CPU、task、network 三组跨模块接口稳定。
- 两个架构的共享 boot stack 问题有修复或已验证方案。
- scheduler、process registry、frame allocator 和 driver 的 SMP 风险有负责人和提交。
- 至少一个架构完成 8 CPU 上线或得到可复现的第一个阻塞点。

争取目标：

- 两个架构都能看到 8 个 CPU 上线。
- 8 CPU 参与基础调度，无 task 双跑。
- 基础 pthread/futex、并发内存和并发设备测试可以运行。

两周内不应把“BuildStorm 性能达标”设为硬目标。当前先建立正确的 SMP 和可重复测试基线，否则性能数据没有可信度。
