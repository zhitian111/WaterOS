# RISC-V QEMU SMP 多核运行实现说明

本文档描述 WaterOS 首版多核运行方案。目标是在 `qemu-riscv64-opensbi` 下支持最多 8 个 QEMU hart，让 BSP 完成全局初始化后经 SBI HSM 启动 AP hart，并让多个 hart 共同参与任务调度。

## 当前状态

已落地的基础能力：

- `CpuId` 在 RISC-V QEMU 首期等同 hart id，所有入口都在堆分配前校验其小于 `MAX_CPUS = 8` 并初始化 CPU-local/trampoline return frame。
- trampoline 有独立的 8 份 return frame；调度器维护每 CPU 的 online/current/idle/runqueue 状态。
- BSP 使用 OpenSBI 指定的 boot hart，完成全局初始化后调用 SBI HSM `hart_start`；AP 激活内核页表、安装 trap vector、打开 SSIE/STIE/timer 后发布 online。
- SBI IPI 与 software-interrupt trap 已接通；IPI 只请求本 CPU 重调度，绝不推进全局 timeout/tick。
- process registry、frame allocator、cred、FD/CWD、mount namespace 和静态回调槽已迁移到多核安全容器。

仍未完成的部分不能视为多核用户态验收通过：远端 runqueue 投递与定向 IPI、`ready_cpu/last_cpu` 状态、BSP-only timeout timekeeper，以及地址空间 active-CPU mask 和带 ack 的 TLB shootdown 尚待实现。因此当前 AP bring-up 仅用于内核级验证，不应作为并发 `mmap`/`fork`/用户态工作负载的最终 SMP 支持。

首版 SMP 不追求高扩展性，只要求正确地启动多个 hart，并避免同一个 task 被多个 hart 同时运行。

## 设计决策

本文件描述的首版只支持 RISC-V QEMU/OpenSBI；LoongArch 后续已通过 IOCSR
mailbox + boot IPI 实现独立的 AP 启动路径：

- hart 上限固定为 `MAX_CPUS = 8`，暂不解析 DTB CPU 节点。
- 调度器采用全局 ready queue + 全局自旋锁。
- AP hart 参与普通任务调度，不只是启动后空转。
- 不实现 CPU affinity、work stealing、热插拔或运行中迁移。
- TLB shootdown 暂不完整实现；在其完成前不把用户地址空间并发修改作为 SMP 支持范围。

## 运行前提

多核运行必须提供包含 SBI HSM 扩展的 OpenSBI 固件；QEMU 的 `-bios default` 不是多核验收环境。脚本会在 `SMP_CORES > 1` 且未设置固件时失败，并始终使用 QEMU `-snapshot`，避免写入 `sdcard-rv.img`：

```bash
cd os
SMP_CORES=4 WATEROS_OPENSBI_FW=/path/to/opensbi-hsm-fw.bin ./scripts/run/rv_qemu_run.sh
```

可使用 `SMP_CORES=1` 做 RISC-V 单核回归。LoongArch QEMU 脚本固定使用
`-smp 8`，BSP 会通过 mailbox 逐个启动其余 7 个 vCPU。

## 启动流程

BSP/AP 入口需要拆分：

1. `_start.S` 从 OpenSBI 获取 `a0 = hart_id`、`a1 = dtb_pa`。
2. 将 `hart_id` 写入 `tp`，作为 `current_hart_id()` 的早期来源。
3. 按 `hart_id` 选择独立 boot stack，避免多个 hart 共用同一栈。
4. `hart_id == 0` 进入现有 BSP `kernel_main`。
5. `hart_id != 0` 进入 `secondary_main(hart_id)`。

BSP 负责：

- 初始化 console/logging/heap/platform arch。
- 初始化 task、trap handler、MM、driver、fs、用户 bring-up。
- 初始化 SMP 元数据和 AP boot stack 表。
- 调用 OpenSBI HSM `hart_start(hart_id, secondary_start, opaque)` 启动 AP。
- 发布 `SMP_READY = true`。
- 开启本 hart timer interrupt，进入调度。

AP 负责：

- 等待 `SMP_READY`。
- 设置 `tp = hart_id`。
- 执行本 hart 的 `platform::arch::init()`。
- 激活内核地址空间 token。
- 注册/确认 trap vector。
- 开启本 hart timer interrupt 和 global interrupt。
- 调用 task 的 per-hart 调度入口进入 idle/普通任务切换。

## 平台与固件接口

需要在 platform/firmware 层补齐最小 SBI HSM 封装：

- `hart_start(hart_id, start_addr, opaque) -> Result<()>`
- `hart_get_status(hart_id) -> Result<HartStatus>`
- 可选：`hart_stop() -> !`

在 platform/base 层补齐：

- `current_hart_id() -> usize`，RISC-V 首版读取 `tp`。
- `SMP_MAX_HARTS` 配置常量。
- `HartLocal<T>` 或最小数组封装，用于 per-hart current task、idle task、boot stack。

QEMU 脚本增加：

- `SMP_CORES=${SMP_CORES:-4}`
- `qemu-system-riscv64 ... -smp ${SMP_CORES}`

## 调度器改造

`impl-round-robin` 从单 current task 改为 per-hart current task：

- 全局 `RoundRobinScheduler` 改由 `spin::Mutex` 保护。
- 内部维护 `current_task_by_hart: [Option<TaskId>; SMP_MAX_HARTS]`。
- 每个 hart 创建独立 idle task。
- 所有调度入口通过 `current_hart_id()` 找到本 hart current task。
- ready/sleep/wait/exited 队列仍保持全局唯一。
- 上下文切换前必须释放 scheduler lock，避免切换后锁遗留在旧任务栈上。

调度规则：

- 一个 task 只能处于一个状态：Running(hart)、Ready、Blocked、Sleeping、Exited。
- 从 ready queue 取出 task 后，必须先标记为 Running(hart)，再返回 switch pair。
- tick 只为本 hart 当前任务计时。
- idle task 不进入普通 ready queue。
- 当前 hart 无普通任务时切到本 hart idle task。

## 同步改造

所有可能被多个 hart 同时访问的全局状态不能继续使用 `UniprocessorSafeCell`：

- task scheduler 全局实例改为 `spin::Mutex`。
- process registry 改为 `spin::Mutex`。
- frame allocator 改为自旋锁保护。
- VFS fd/cwd registry、cred registry 等后续同步替换。
- 已经使用 `spin::Mutex` 的 block/network/fs 对象可先保留。

临界区规则：

- scheduler lock 内不调用可能阻塞、等待、执行 syscall 或触发调度的逻辑。
- 持 scheduler lock 时可短暂关本 hart 中断，避免本 hart timer 重入。
- 关本 hart 中断不能替代跨 hart 锁。

## Trap 与 Timer

timer interrupt 是 per-hart 行为：

- 每个 hart 在进入调度前都要开启 timer interrupt。
- timer trap 中重新设置本 hart 下一次 deadline。
- `task::schedule_tick()` 根据 `current_hart_id()` 调度本 hart 当前任务。
- `sscratch` 是 per-hart CSR，现有汇编注释方向正确，但 Rust 侧 current task 必须改为 per-hart。

用户态 trap 返回路径保持原语义：

- 进入内核时切到 kernel satp。
- syscall/page fault 等逻辑仍操作当前 hart 的 current task trap frame。
- 返回用户态前从当前 hart current task 恢复 trap frame。

## 验证计划

静态检查：

```bash
cd os && make rv_check
