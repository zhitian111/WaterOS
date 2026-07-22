# 决赛任务三人分工

## 目标和分工原则

本文把 [`final-test-readiness.md`](./final-test-readiness.md) 中的决赛任务分配给三名成员。分工依据现有模块所有权：

- 成员 B 已负责 task 模块，继续主责 task、scheduler 和线程同步。
- 成员 C 已负责 driver/network 模块，继续主责网络设备、网络协议栈和网络验证。C 不负责 block driver 或其它 driver 子系统。
- 成员 A（项目主要维护者）负责其余模块、跨模块接口和最终集成。

任务不按数量平均分配，而是尽量减少多人同时修改同一文件。每个跨模块功能只指定一名集成人，其他成员提供稳定接口和测试结果。

## 总览

| 成员 | 主责范围 | 决赛主要交付 |
|---|---|---|
| A | platform、MM、VFS、syscall、procfs、集成 | 双架构启动、CPU-local、TLB、glibc/文件系统兼容、最终测试 |
| B | task、scheduler、task 相关 IPC | SMP 调度、per-CPU task、clone/futex/exit、多核任务正确性 |
| C | `driver/network` 和平台中的网卡注册代码 | virtio-net 收发、smoltcp 外网链路、网络并发、TCP 状态快照 |

## 共同约定

### 所有权

- A 负责 `wateros-platform`、`wateros-mm`、`wateros-vfs`、`wateros-syscall`、`wateros-fs` 和 `os/src/main.rs`。
- B 负责 `wateros-task`、两个 scheduler impl，以及 task 语义直接依赖的 futex/waitqueue 路径。
- C 负责 `wateros-driver/driver-network`，以及两个平台 driver impl 中只与 virtio-net 探测和注册有关的代码。
- block、character、通用 driver 框架和 BuildStorm 文件 IO 不属于 C；这些任务由 A 负责或另行安排。
- 需要修改非本人主责模块时，先约定 API，由模块负责人完成内部修改。紧急联调改动需要在提交说明中点明。

### 提交

- 每个提交只完成一个可描述的任务，不混入格式化或无关重构。
- 提交信息带任务编号，例如 `smp(B1): add per-cpu current task state`。
- 合入前至少运行本人任务列出的静态检查和定向测试。
- 跨模块 API 分成两个提交：先合 API/桩，再合调用方，避免长期维护一个大冲突分支。

### 运行记录

每个验证结果至少记录：

- commit
- 架构与 QEMU 参数
- 使用的镜像及哈希
- 测试命令
- 串口日志路径
- 成功标记或第一个失败点
- 修改前后耗时（性能任务）

## 成员 A 任务

### A0：恢复双架构静态构建

- [ ] **A0.1** 修复 LoongArch64 `PlatformTime` trait 接口漂移。
- [ ] **A0.2** 保证 `make rv_check` 和 `make la_check` 同时通过。
- [ ] **A0.3** 记录现存 warning，不在本任务中顺手清理无关 warning。

验收：两个 `make *_check` 命令退出码均为 0。

### A1：CPU 和双架构启动基础

- [ ] **A1.1** 为 RISC-V64 和 LoongArch64 提供 8 份独立 boot stack。
- [ ] **A1.2** 拆分 BSP 与 AP 入口，确保全局初始化只执行一次。
- [ ] **A1.3** 实现 RISC-V OpenSBI HSM 启动流程。
- [ ] **A1.4** 确认并实现 LoongArch QEMU virt 的 AP 启动协议。
- [ ] **A1.5** 提供 `current_cpu_id()`、最大 CPU 数和 online CPU mask。
- [ ] **A1.6** 建立不依赖堆分配的 CPU-local 基础设施。
- [ ] **A1.7** 每核初始化 trap、timer、内核地址空间和中断状态。
- [ ] **A1.8** 提供 reschedule、TLB shootdown 和 stop/panic 所需的 IPI 基础接口。

交付给 B 的接口：

```rust
pub fn current_cpu_id() -> usize;
pub fn online_cpu_mask() -> CpuMask;
pub fn online_cpu_count() -> usize;
pub fn send_reschedule_ipi(cpu_id: usize);
```

验收：两个架构在 `-smp 8 -m 8G` 下启动，8 个 CPU 使用不同栈并到达 AP ready 点。

### A2：内存与跨核 TLB

- [ ] **A2.1** 把 frame allocator 从 `UniprocessorSafeCell` 改为 SMP 安全容器。
- [ ] **A2.2** 保护用户地址空间并发修改和并发 page fault。
- [ ] **A2.3** 实现跨核 TLB shootdown 与完成确认。
- [ ] **A2.4** 处理 `mmap`、`munmap`、`mprotect`、`brk`、exec 和地址空间销毁的一致性。
- [ ] **A2.5** 核对 LoongArch 4 GiB frame 上限与 8 GiB QEMU 配置。
- [ ] **A2.6** 修复 heap recursion depth 的跨核误判，保留同核递归检测。

与 B 的接口约定：B 提供“某地址空间当前运行在哪些 CPU”的查询或回调；A 负责发 shootdown 并等待确认。

验收：8 核并发 page fault 和映射修改无 stale mapping、UAF 或 frame 泄漏。

### A3：VFS、syscall 和 procfs

- [ ] **A3.1** 改造 fd、cwd、mount namespace 和 credential registry 的 SMP 同步。
- [ ] **A3.2** 实现单调、真实的 `/proc/uptime`。
- [ ] **A3.3** 按 online CPU 信息生成 `/proc/cpuinfo` 和 affinity mask。
- [ ] **A3.4** 接入 C 提供的 TCP 快照，生成 `/proc/net/tcp`。
- [ ] **A3.5** 验证 proc、sysfs 和 devtmpfs mount 兼容性。
- [ ] **A3.6** 完成 `/tmp` 的 fd/cwd/page-cache 联调。
- [ ] **A3.7** 验证 ext4/page cache 的并发 create、rename、truncate、unlink、fsync 和 mmap。
- [ ] **A3.8** 按 rustc/cargo 第一个失败点补齐 syscall 或 ABI 语义。

验收：`nproc` 返回 8，`/proc/uptime` 单调，CAgent 能读取 TCP 状态，cargo minibuild 的文件操作完整通过。

### A4：测试集成和材料

- [ ] **A4.1** 准备双架构决赛 QEMU 启动目标，参数为 `-smp 8 -m 8G`。
- [ ] **A4.2** 管理官方镜像哈希、Linux 基线和测试日志目录。
- [ ] **A4.3** 集成 CAgent 十项测试。
- [ ] **A4.4** 运行 toolchain、minibuild 和 BuildStorm。
- [ ] **A4.5** 汇总三人的修改前后数据，完成内核设计与优化文档。
- [ ] **A4.6** 汇总 AI 使用说明和一键复现步骤。

## 成员 B 任务

### B1：SMP 调度器

- [ ] **B1.1** 用 SMP 锁替换两个 scheduler impl 的 `UniprocessorSafeCell`。
- [ ] **B1.2** 实现 per-CPU current task 和 idle task。
- [ ] **B1.3** 把任务运行状态扩展为可表达 `Running(cpu_id)`。
- [ ] **B1.4** 保证 task 只属于一个状态和一个 CPU。
- [ ] **B1.5** 统一 tick、yield、sleep、block、wake、exit 的状态转换。
- [ ] **B1.6** 确保 scheduler lock 在 `__switch` 前释放。
- [ ] **B1.7** 接入 A 提供的 CPU id、online mask 和 reschedule IPI。
- [ ] **B1.8** 为任务双跑、重复入队和非法状态转换增加 debug 断言。

验收：8 CPU 都能运行普通任务；同一 task 不会双跑；idle CPU 能被新任务及时唤醒。

### B2：process registry 和任务生命周期

- [ ] **B2.1** 把 process registry 改为 SMP 安全容器。
- [ ] **B2.2** 检查 clone、fork、exec、exit、wait 和 reap 的跨核竞争。
- [ ] **B2.3** 防止 TCB、内核栈和用户地址空间在另一 CPU 使用时被释放。
- [ ] **B2.4** 把大型 drop 和地址空间销毁移出 scheduler/process registry 临界区。
- [ ] **B2.5** 提供 procfs 所需的短锁 task/process 快照 API。
- [ ] **B2.6** 提供 A2 所需的地址空间运行 CPU 集合。

交付给 A 的接口应只返回快照或稳定 ID，不返回跨越锁生命周期的内部可变引用。

### B3：pthread、futex 和线程退出

- [ ] **B3.1** 验证 `CLONE_VM | CLONE_THREAD` 和 glibc TLS。
- [ ] **B3.2** 验证 futex wait、wake、requeue 和 timeout。
- [ ] **B3.3** 验证 robust list 和 `clear_child_tid`。
- [ ] **B3.4** 处理 exec 与进程退出时的其它线程。
- [ ] **B3.5** 检查 waitqueue/futex handler 的锁序和丢失唤醒。
- [ ] **B3.6** 运行 8 核 pthread、futex、clone/exit 长时间压测。

验收：rustc 使用多线程时不死锁、不丢唤醒，线程退出后没有遗留 TCB 或 futex waiter。

### B4：task 性能

此阶段只在 BuildStorm 首次成功后开始。

- [ ] **B4.1** 测量 scheduler lock 等待、context switch 和 idle 比例。
- [ ] **B4.2** 根据数据评估 per-CPU run queue、work stealing 或批量唤醒。
- [ ] **B4.3** 优化 fork/exec/exit 和 PID/TID 查找退化。
- [ ] **B4.4** 给每项优化提供 BuildStorm 或定向基准的前后对比。

## 成员 C 任务

### C 的范围

C 负责的数据路径是：

```text
QEMU virtio-net
  -> RISC-V virtio-mmio / LoongArch virtio-pci
  -> NetworkDevice::send/receive
  -> SmoltcpAdapter
  -> NETWORK_STACK
  -> TCP/UDP 网络能力
```

C 负责的主要目录：

```text
os/components/wateros-driver/driver-network/
os/components/wateros-driver/driver-impl/impl-qemu-riscv64-opensbi/  # 仅网卡探测/注册部分
os/components/wateros-driver/driver-impl/impl-qemu-loongarch64-virt/ # 仅网卡探测/注册部分
```

C 明确不负责：

- `driver-block`、块缓存和文件系统 IO
- QEMU SMP 启动、CPU-local、IPI 和调度器
- socket syscall、VFS fd、poll/epoll 的 syscall 实现
- `/proc/net/tcp` 的 procfs 节点和文本格式
- CAgent 其余九项和 BuildStorm 文件系统问题

当网络能力需要上层改动时，C 提供错误现象、日志和所需接口，A 在 syscall、VFS 或 procfs 中完成接入。

### C0：先证明真实网卡和外网链路

这一阶段不涉及 SMP，先回答当前网络是否真的能离开系统内部。

- [ ] **C0.1 网卡注册。** 在两个架构的启动日志中确认找到并注册 virtio-net，而不是退回 `loopback_only()`。
- [ ] **C0.2 二层收发。** 记录 virtio 网卡 TX/RX 计数，确认能向 QEMU user network 发帧并收到 ARP/DNS/TCP 回复。
- [ ] **C0.3 网关连通。** 使用当前静态地址 `10.0.2.15/24` 和网关 `10.0.2.2`，验证向网关或一个固定外部 IPv4 地址建立 TCP 连接。
- [ ] **C0.4 DNS。** 验证 UDP 访问 QEMU DNS `10.0.2.3:53`。C 负责确认 UDP 报文能正常收发；A 负责 `/etc/resolv.conf` 和用户态 DNS 配置。
- [ ] **C0.5 外网 TCP。** 用固定公网 IPv4 的 TCP 端口验证外网路径，再用域名做第二步验证，避免把 DNS 失败误判为网卡失败。
- [ ] **C0.6 双架构记录。** RISC-V virtio-mmio 和 LoongArch virtio-pci 各保留一份日志，说明成功到哪一层以及第一个失败点。

验收结果必须能回答：真实 virtio 网卡是否注册、是否有 TX/RX、是否能到 QEMU 网关、是否能访问外部 IPv4、DNS 是否有回复。

### C1：网络驱动的多核安全

这一阶段在 A/B 提供 8 核运行基础后开始。

- [ ] **C1.1 单次初始化。** 网卡探测、virtio queue 创建和 `NETWORK_STACK` 初始化只由 BSP 执行一次。
- [ ] **C1.2 virtio-net 互斥。** 多个 CPU 发送或轮询接收时，descriptor 和 DMA buffer 不会被重复使用。
- [ ] **C1.3 poller 唯一性。** 明确只有一个 network poller，或者证明多个 poller 的同步正确；默认建议只运行一个。
- [ ] **C1.4 网络栈锁。** 检查 `NETWORK_STACK` 全局锁的持有范围，网络等待期间不能持锁进入 sleep 或等待另一任务。
- [ ] **C1.5 并发连接。** 多个 TCP/UDP socket 同时 connect、send、receive 和 close 时不死锁、不串数据。
- [ ] **C1.6 压测。** 在两个架构运行多连接 loopback 和 virtio 外网测试，记录错误、超时和锁竞争。

验收：8 核并发网络请求不出现 virtio descriptor 损坏、丢失唤醒、全局锁死锁或连接间数据混淆。

### C2：向 CAgent 和 procfs 提供网络状态

C 不实现 `/proc/net/tcp`。C 只提供一份不泄漏内部锁和对象的 TCP 状态快照，由 A 在 procfs 中格式化。

- [ ] **C2.1** 明确 `SocketMeta` 中可直接提供的 local address、remote address 和连接状态。uid、VFS inode 等上层字段不要求 C 生成。
- [ ] **C2.2** 实现 `tcp_connection_snapshots()`，调用结束前释放 `NETWORK_STACK` 锁。
- [ ] **C2.3** 为快照增加至少三种状态测试：LISTEN、CONNECTING/ESTABLISHED、CLOSED。
- [ ] **C2.4** 配合 A 验证 `/proc/net/tcp` 能看到 `simple_llm_server` 与 agent 之间的 ESTABLISHED 连接。
- [ ] **C2.5** CAgent 并发运行时，确认网络驱动和协议栈没有连接失败或超时；测试脚本、procfs 和 judge 由 A 负责。

交付给 A 的建议接口：

```rust
pub struct TcpConnectionSnapshot {
    pub local_addr: IpEndpoint,
    pub remote_addr: IpEndpoint,
    pub state: TcpConnectionState,
}

pub fn tcp_connection_snapshots() -> Vec<TcpConnectionSnapshot>;
```

快照函数只复制元数据，不应在 procfs 格式化期间持有网络栈锁。

### C3：网络性能

这一阶段只在功能正确且 BuildStorm 首次成功后开始。

- [ ] **C3.1** 测量 `NETWORK_STACK` 锁等待、poll 次数、TX/RX 包数和丢包。
- [ ] **C3.2** 定位 syscall 路径与 network poller 的重复 full poll；C 优化 network 内部，所需 syscall 改动交由 A 完成。
- [ ] **C3.3** 用 iperf/netperf 或等价测试记录修改前后吞吐与 CPU 占用。
- [ ] **C3.4** 性能修改不能改变 C0 外网连通性或 CAgent 稳定性。

## 跨成员接口清单

A 与 B 第一次协作的具体类型、文件和签名见 [`smp-a-b-first-interface-contract.md`](./smp-a-b-first-interface-contract.md)。

| 接口 | 提供者 | 使用者 | 合入时机 |
|---|---|---|---|
| `current_cpu_id`、online mask | A | B、C、procfs | 第一阶段最先合入 |
| reschedule IPI | A | B | AP 能启动后 |
| TLB shootdown API | A | B/task 生命周期 | 地址空间并发前 |
| current task/per-CPU task API | B | A 的 trap、MM、procfs | scheduler SMP 化时 |
| task/process snapshot | B | A 的 procfs/syscall | process registry SMP 化时 |
| 地址空间运行 CPU 集合 | B | A 的 MM | shootdown 联调前 |
| TCP connection snapshot | C | A 的 procfs | CAgent network 前 |
| 网络状态和 TX/RX 统计 | C | A 的网络测试记录 | 网络联调阶段 |

## 分阶段并行计划

### 第一阶段：建立基础，三人并行

成员 A：

- A0 双架构构建
- A1.1 至 A1.6，完成每核栈、BSP/AP 和 CPU 基础接口
- A3.2 `/proc/uptime`

成员 B：

- B1.1 至 B1.6，在临时 CPU id 抽象上完成调度器内部改造
- B2.1 process registry SMP 容器

成员 C：

- C0，先验证真实 virtio 网卡、QEMU 网关、外部 IPv4 和 DNS
- 同时整理 C1 的多核风险，不在 8 核基础完成前改并发模型

阶段出口：两个架构可编译；跨成员 API 已确定；没有人再新增直接依赖单核容器的代码。

### 第二阶段：8 核正确性联调

成员 A：完成 AP trap/timer/IPI、frame allocator 和基础 TLB shootdown。

成员 B：接入真实 CPU id 和 IPI，完成 per-CPU 调度、生命周期和 task 压测。

成员 C：保证 network 初始化一次，完成 virtio-net、poller 和网络栈并发测试。

共同出口：两架构 8 CPU 在线并参与调度，pthread/futex、并发 page fault 和并发 IO 稳定。

### 第三阶段：功能测例

成员 A：procfs、VFS、syscall、glibc、minibuild 和测试集成。

成员 B：clone、TLS、futex、exec/exit，处理 rustc 首个 task 相关失败。

成员 C：处理 CAgent 中由网卡、TCP/UDP 或网络栈导致的失败，并向 A 提供 TCP 状态快照。

阶段出口：CAgent 十项通过；toolchain 和 minibuild 通过；BuildStorm 首次完整成功。

### 第四阶段：性能和提交材料

- A 负责 MM、page cache、ext4 和整体计时。
- B 负责 scheduler、进程和 futex。
- C 负责 virtio-net、smoltcp 和网络吞吐。
- A 汇总实验数据、AI 使用说明和复现步骤。

只有带数据的优化才进入最终设计文档。

## 集成顺序

每轮集成按以下顺序，避免基础接口反复漂移：

1. A 的 platform/CPU 基础 API。
2. B 的 scheduler 和 process registry。
3. A 的 trap、MM、TLB 与 procfs 接入。
4. C 的 network SMP 改造和快照 API。
5. A 的 VFS/syscall 与完整测试入口。

## 冲突高风险文件

下列文件由指定成员主改，其他成员尽量只通过接口接入：

| 文件或目录 | 主改成员 | 原因 |
|---|---|---|
| `os/src/main.rs` | A | BSP/AP 与全局初始化顺序 |
| `wateros-platform/**` | A | CPU、IPI、trap、timer |
| `wateros-task/**` | B | 调度和任务状态机 |
| `wateros-mm/**` | A | 页表、frame、shootdown |
| `wateros-driver/driver-network/**` | C | virtio-net、smoltcp、TCP/UDP 状态 |
| 平台 driver impl 的网卡探测段 | C | 双架构 virtio-net 注册；其它平台启动代码由 A 负责 |
| `fs-procfs/**` | A | A 组合 B/C 的快照接口 |
| `wateros-vfs/**` | A | fd、cwd、mount、page cache |
| `wateros-syscall/**` | A | glibc ABI 集成；task 语义修改与 B 评审 |

## 每周同步模板

每人同步以下五项即可：

```text
已完成：任务编号和 commit
正在做：当前任务编号
阻塞：需要谁提供什么接口或日志
验证：运行命令和结果
下一步：下一个可验收交付
```

发生失败时报告第一个可靠失败点，不用“整体还不能跑”代替可定位的信息。

## 完成定义

团队任务完成需要同时满足：

- RISC-V64 和 LoongArch64 静态构建通过。
- 两个平台以 `-smp 8 -m 8G` 稳定运行。
- CAgent 十项在两个平台通过。
- toolchain 和 minibuild 在两个平台通过。
- BuildStorm 产物存在且不少于 500 KiB。
- BuildStorm 至少连续成功 3 次，并保留时间数据。
- 设计优化文档含根因、实现、实验、AI 使用和复现步骤。
