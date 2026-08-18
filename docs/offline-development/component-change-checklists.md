# 组件修改检查表

本页是 17 个一级组件的线下修改速查。详细数据结构和运行链见各组件 README；这里强调“改一个点时
还必须检查什么”。先定位状态所有者，再沿创建、共享、销毁和失败回滚审计。

## wateros-base

核心状态：`CpuId/CpuMask/CpuLocal`、`MultiprocessorSafeCell`、`BootOnceCell/RuntimeOnceCell` 和
`base-config` 的编译期容量。

修改检查：

- 改 CPU 上限时同步 mask 位宽、per-CPU 数组、runtime discovered CPU 数和所有边界断言。
- 改 once-cell 时写清 bootstrap 阶段是否允许无锁访问，以及运行期 Release/Acquire 发布规则。
- base 不得依赖 task/MM/platform/syscall；否则会形成依赖环和早期初始化不可用。
- 配置常量注明单位（byte/page/tick/ns）和“容量上限”还是“运行时实际值”。

最小验证：base crate test、SMP=1 与 SMP=8 启动、越界 CPU/mask 自检。

## wateros-platform

核心状态：架构 trap frame、中断控制器、timer/IPI、CPU bringup、地址空间激活 token、板级设备描述。

修改检查：

- trap frame 字段顺序必须与汇编保存/恢复完全一致；同步检查信号 `SignalFrameCodec`。
- 中断 handler 区分中断上下文与 task 上下文，禁止睡眠和持普通任务锁。
- timer 重武装只推进本 CPU 调度；全局 timeout timekeeper 不能被每个 AP 重复推进。
- IPI 新消息要定义发布内存序、目标 CPU 集合、确认和离线 CPU 行为。
- 通用改动同时检查 RISC-V/LoongArch；板级差异放 `platform-impl`。

最小验证：两架构 build/check、SMP 启动、timer tick、软件 IPI、用户 syscall/page fault 往返。

## wateros-runtime

核心状态：固定内核堆、allocator metadata、console/serial writer、日志级别与 panic path。

修改检查：

- allocator API 必须允许大对象分配失败可观测，不能递归记录日志或再次分配。
- panic/早期 console 必须在 heap、scheduler 或正常锁不可用时仍能输出。
- 日志热路径避免大临时 `String/Vec`；编译期 max-level 会裁掉更低级别参数求值。
- 新后端实现需保持 `no_std`，明确初始化前后的可调用范围。

最小验证：allocator 自检、OOM 诊断、早期 boot log、并发 console 输出和目标 profile 编译。

## wateros-task

核心状态：TCB/PCB registry、run queue、task state、process/thread group、wait/zombie、CPU affinity 和统计。

修改检查：

- 状态转换列出 `Ready/Running/Blocked/Exited` 的合法边，wake 必须幂等或有序号保护。
- 创建任务时先完成外围资源，再发布 Ready；失败撤销未发布 child。
- exit 先发布可等待状态，外围资源锁外清理；区分 exit 和 reap。
- scheduler/process registry 锁内不做用户复制、VFS、设备 I/O 或普通睡眠。
- 调度字段变化同步 procfs、rusage、affinity 和 fork/clone 继承。

最小验证：线程创建/退出、wait 状态、SMP 迁移、affinity、forkheavy 和资源基线。

## wateros-mm

核心状态：frame allocator、架构页表、VMA 集合、COW/refcount、ASID、TLB CPU mask 和用户地址空间句柄。

修改检查：

- 新映射类型实现 fault/fork/protect/remap/unmap/destroy/snapshot 全链。
- 明确页面所有者：普通帧、共享引用帧、只读缓存页或外部设备/SHM 页。
- 页表修改只通过带 flush 的包装；同时实现两套架构。
- 用户 copy 跨页、部分进度和 COW fault 不能在调用方锁内发生。
- 地址区间统一使用页对齐的半开区间，所有加法检查溢出。

最小验证：COW、共享映射、部分 munmap、坏地址、双架构 check、连续两轮 mmap 压测。

## wateros-vfs

核心状态：每任务 fd/cwd/mount namespace、共享打开描述、路径 resolver、stable node、页缓存与脏页。

修改检查：

- 区分 descriptor flag 和 OFD status flag；dup/fork/exec 行为不同。
- 新 handle 实现 read reservation、poll、duplicate、close 和错误类型。
- 路径必须尊重 process root、dirfd、符号链接上限、final-symlink policy 和最长挂载前缀。
- 页缓存遵守 `files -> entry -> state -> FS` 锁序；后端 I/O 在 cache state 锁外。
- writeback 与 fsync/FS flush 分开；unlink/rename 后打开句柄仍指向原 stable node。

最小验证：dup offset/CLOEXEC、符号链接、rename/unlink-open、并发页缓存写回、fork/exit fd 清理。

## wateros-fs

核心状态：root implementation selection、mount generation、root device、devfs/procfs、各 ext4/ramfs 后端。

修改检查：

- 区分 FS mount（后端读取设备）与 VFS namespace mount（路径路由）。
- 新后端实现 metadata/read/write/truncate/sync/stable-node 能力矩阵，unsupported 不假成功。
- inode/lookup cache 的 key 在 rename/unlink 后保持身份正确；mount generation 隔离旧缓存。
- block I/O 错误保留原始语义到 FS/VFS 转换边界。
- vendor 修改与 WaterOS adapter 修改分开记录，避免更新 vendor 时丢补丁。

最小验证：镜像副本读写、目录/链接、rename/unlink、fsync、重新挂载读回以及两种根后端 feature。

## wateros-ipc

核心状态：pipe buffer/endpoints、waitqueue waiter、futex registry、signal registry、SHM segment/frame。

修改检查：

- 阻塞前在状态锁内登记 waiter，释放锁后睡眠，醒来循环复查。
- pipe capacity 是流控上限，不等于创建时的物理分配；写入用可失败扩容。
- futex shared key 来自稳定物理/映射身份；COW/private key 不能混用。
- signal/futex/SHM 新状态接入 fork/clone/exec/exit/reap。
- SHM/设备外部页不能由普通地址空间 destroy 回收。

最小验证：pipe 大小/非阻塞/关闭、futex timeout/requeue/robust、signal frame、SHM fork/remove。

## wateros-cred

核心状态：`ProcessCredentials`、owner/refcount、uid/gid 三元组、supplementary groups。

修改检查：

- 线程共享进程 credential，fork 复制，reap 清 owner；不要按每线程无条件深复制。
- real/effective/saved ID 一次规划后提交，避免部分更新。
- capability 当前跨 cred/task 两处；修改时同步 setid、prctl、exec 和权限消费者。
- VFS/signal/mount 等消费者应检查具体能力而非新增 `uid==0` 特判。

最小验证：set*id 组合、groups 边界、KEEP_CAPS、fork/exec 继承和文件/信号真实权限。

## wateros-syscall

核心状态：调用号表、ABI 结构、用户复制、errno 映射，以及少数组合层 registry。

修改检查：

- 依次更新 number、领域 handler/export、稠密 dispatch table、restartable 属性。
- 用户长度先上限/溢出检查，再可失败分配；用户指针只能经 user_copy。
- handler 只做 ABI 和编排，长期状态放真正组件。
- 消费型读取 reserve/copy/commit；坏指针不改变可重试状态。
- 未知 flag 报错，状态修改类未实现不能无操作成功。

最小验证见 [添加系统调用](adding-a-syscall.md)。

## wateros-driver

核心状态：board probe 结果、共享设备 handle、VirtIO queue/descriptor、DMA buffer、IRQ completion。

修改检查：

- 设备注册在消费者启动前完成；探测失败不发布半初始化 handle。
- descriptor/DMA buffer 从提交到 completion 期间保持所有权，回收恰好一次。
- MMIO 与 PCI transport、RISC-V 与 LoongArch board 选择分别检查。
- IRQ handler 只确认/记录/唤醒，耗时工作移到 task/poll 上下文。
- raw block 与 block cache 分层，flush 能力和错误不能伪造。

最小验证：设备枚举、基本 I/O、队列满、IRQ/poll completion、重复读写和两架构启动。

## wateros-network

核心状态：smoltcp interface/socket set、TCP/UDP socket state、端口/地址、网卡收发队列和轮询时钟。

修改检查：

- socket handle 生命周期与 VFS fd 对齐；close/shutdown/peer close 状态可被 poll 观察。
- stack lock 内不睡眠和不做用户复制；阻塞 syscall 释放锁后等待并重新 poll。
- TCP connect/listen/accept 和 UDP datagram 的 readiness 条件分别定义。
- 驱动 RX/TX buffer 所有权在 stack 和 VirtIO completion 间清晰交接。
- 新协议族在 syscall ABI、network API 和后端三层都有明确 unsupported/实现状态。

最小验证：loopback TCP/UDP、非阻塞 connect、backpressure、half-close、poll、长时间收发和 packet loss。

## wateros-tty

核心状态：terminal/session、line discipline、输入编辑缓冲、PTY pair、foreground process group 和 termios。

修改检查：

- canonical/raw、echo、特殊字符和 EOF 的输入消费顺序一致。
- Ctrl-C 等字符向前台进程组发信号，不直接终止任意当前 task。
- background read/write 按 job-control 规则返回或发信号。
- PTY master/slave close、hangup、session detach 和 poll readiness 同步。
- ioctl 结构布局在 syscall 层，终端机制放 tty。

最小验证：shell 交互、Ctrl-C/Ctrl-D、raw mode、PTY、前后台进程组、窗口大小和关闭 hangup。

## wateros-klog

核心状态：消息/文本环、sequence、读游标、覆盖计数和 record metadata。

修改检查：

- writer 在中断/panic 相关路径不可依赖会递归日志的锁或分配。
- 发布 record 时 reader 不得看到半写 payload；遵循 sequence/内存序协议。
- 覆盖旧记录时 reader 能检测 overrun 并恢复到最老可用 sequence。
- syslog/proc/debug consumer 的过滤、clear 和游标各自独立。

最小验证：并发 writer、wraparound、长消息截断、慢 reader overrun、syslog bad-pointer 回滚。

## wateros-debug

核心状态：per-CPU snapshot、active slot、event ring、sequence/counter 和 `TrackedMutex` 记录。

修改检查：

- GDB reader 无法取得 Rust 锁，数据格式必须靠序号和 Release/Acquire 验证一致性。
- debug 组件保持极低依赖，不能为记录锁而引入新的内核锁环。
- feature 关闭时热路径应接近 no-op；fault injection 与观测 feature 分开。
- 导出结构变更同步 host-side GDB 脚本和 build ID/version。

最小验证：feature on/off 编译、并发事件、GDB 读取一致快照、锁记录和 fault injection 隔离。

## wateros-gui

核心状态：`GuiRuntime`、窗口 z-order/focus、shadow surface、dirty regions、输入/输出队列。

修改检查：

- 像素格式、stride、surface bounds 在任何绘制前校验。
- dirty region 队列有界；溢出退化为全屏重绘而非丢失更新。
- 固定 GUI runtime -> display 的锁序，输入回调不反向持锁。
- focus/drag/close 事件先更新状态，再计算损坏区域和合成。
- 内核 GUI 与 user-graphics 是互斥 framebuffer owner。

最小验证：窗口遮挡/移动、focus、键鼠输入、dirty overflow、双缓冲 flush 和两类 display transport。

## wateros-utils

核心状态：当前主要是无状态 table-format 构造器和渲染参数。

修改检查：

- 保持无平台/任务依赖和 `no_std`；不要把内核全局状态放入工具 crate。
- 宽度、Unicode/ASCII、对齐和空表必须有确定输出。
- 渲染避免不受限临时分配；错误由调用者决定日志/显示策略。

最小验证：纯 host unit test，包括空输入、长单元格、对齐、边框风格和输出快照。

## 完成一个组件修改前

无论改哪个组件，都要留下四类证据：

1. 状态结构和 owner 的源码位置；
2. 创建、共享/复制、销毁、失败回滚的调用点；
3. 最小功能与错误测试；
4. 目标 profile 的双架构编译，以及涉及并发/资源时的重复 runtime 测试。
