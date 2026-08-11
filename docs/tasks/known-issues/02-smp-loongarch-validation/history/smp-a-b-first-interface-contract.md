# SMP A 与 B 的接口和数据结构契约

## 目标

本文规定 SMP 第一轮协作中：

- 成员 A 为成员 B 提供哪些 CPU/platform 能力。
- 成员 B 如何改造 task/scheduler 数据结构。
- 成员 B 最后向 A 的 trap、MM、procfs 和启动代码暴露哪些接口。

第一轮不要求一次完成 8 核启动。目标是先冻结依赖方向和公共类型，让 A、B 可以并行开发：

1. A 的接口在单核状态下也能工作，CPU 0 是唯一 online CPU。
2. B 不再在 scheduler 内假设全局唯一 current、idle、bootstrap context 和时间片计数。
3. A 后续接入真实 AP 启动和 IPI 时，不需要再次重写 task 公共 API。

## 当前代码约束

### 依赖方向

当前 `wateros-task` 和 scheduler 依赖 `wateros-platform-arch`，不依赖顶层 `wateros-platform`。应保持以下方向：

```text
wateros-base
  -> platform-arch
  -> task-api / scheduler

platform 聚合层
  -> 组合 CPU online 状态和板级 IPI

os-kernel
  -> 把 platform IPI 适配成 task 回调
```

不要让 `platform` 依赖 `task`。scheduler 也不要直接依赖具体 OpenSBI 或 LoongArch platform impl。

### 当前单核假设

需要由 B 改造的主要假设：

- `TaskState::Running` 不记录运行 CPU。
- `TaskRegistry` 只有一个 `bootstrap_task_cx` 和一个 `current_task_id`。
- 全系统只有一个 `IDLE_TASK_ID` 和一个 idle TCB。
- `MultiClassScheduler` 和 `RoundRobinScheduler` 各只有一个 `current_task_ticks`。
- scheduler 全局对象使用 `UniprocessorSafeCell`，只靠关闭本核中断保护。
- `current_task_id()`、trap frame、kernel stack 和地址空间查询都默认只有一个 current task。

## 第一轮边界

### A 负责

- 公共 CPU 标识和 CPU mask 类型。
- 当前 CPU id 的架构接口和双架构实现。
- configured/online CPU 信息。
- reschedule IPI 的平台接口和单核 fallback。
- 在 `os-kernel` 中把 platform 能力传给 task。

### B 负责

- task 公共状态增加 CPU 所有权语义。
- scheduler 和 registry 的 per-CPU 数据。
- 全局 scheduler 的 SMP 互斥。
- 当前 CPU 上的 current/idle/tick/bootstrap context。
- task、MM、trap 和 procfs 所需的稳定快照。
- BSP/AP 进入 task runtime 的 task 侧入口。

### 第一轮明确不做

- A 在本轮不必完成真实 AP 唤醒和跨核 IPI，只需保证接口和单核 fallback 可用。
- B 在本轮不实现 per-CPU run queue 或 work stealing，ready/wait/sleep queue 仍可全局共享。
- B 不修改 MM 页表或实现 TLB shootdown，只提供地址空间在哪些 CPU 运行的快照。
- 不在第一轮实现 CPU affinity 策略。affinity mask 先表示所有 online CPU 都可运行。

## A 交付一：公共 CPU 类型

### 修改位置

- `os/components/wateros-base/src/cpu.rs`
- `os/components/wateros-base/base-config/src/task.rs`

### 配置常量

在 `wateros-base-config::task` 增加：

```rust
/// 当前内核静态支持的最大逻辑 CPU 数。
pub const MAX_CPUS: usize = 8;
```

约束：

- 决赛固定使用 8 CPU，第一版直接使用 8 个静态槽位。
- `MAX_CPUS` 是内核容量，不等于 online CPU 数。
- configured 和 online CPU 数不能超过 `MAX_CPUS`。

### `CpuId`

在 `wateros-base::cpu` 增加新类型，保留现有 `CPUHartID` 作为兼容别名或逐步迁移：

```rust
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CpuId(usize);

impl CpuId {
    pub const BOOT: Self = Self(0);

    pub const fn from_raw(raw: usize) -> Self;
    pub const fn raw(self) -> usize;
    pub const fn index(self) -> usize;
    pub const fn fits_capacity(self, capacity: usize) -> bool;
}
```

语义：

- `wateros-base` 只定义无策略的标识类型，不依赖 `wateros-base-config`。
- platform 从固件/DTB 建立拓扑、task 将 id 用作数组索引时，必须先验证 `cpu.fits_capacity(MAX_CPUS)`。
- `index()` 只完成类型到索引的转换，不替调用方做容量检查。
- 固件 hart id 与逻辑 CPU id 第一版可以一一对应，但接口名称使用 `CpuId`。以后若两者不一致，在 platform 层建立映射。

### `CpuMask`

```rust
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuMask(u64);

impl CpuMask {
    pub const EMPTY: Self = Self(0);

    pub const fn from_bits(bits: u64) -> Self;
    pub const fn bits(self) -> u64;
    pub const fn contains(self, cpu: CpuId) -> bool;
    pub fn insert(&mut self, cpu: CpuId);
    pub fn remove(&mut self, cpu: CpuId);
    pub const fn count(self) -> usize;
    pub const fn is_empty(self) -> bool;
}
```

`CpuMask` 同样不依赖 `MAX_CPUS`。platform 和 task 接收外部 mask 时，必须验证 `bits >> MAX_CPUS == 0`；非法 mask 返回错误，不能静默截断。

### 暴露位置

最终由以下路径公开：

```rust
wateros_base::cpu::{CpuId, CpuMask}
wateros_base_config::task::MAX_CPUS
```

task-api、platform-arch、platform 和 os-kernel 都使用同一类型，禁止各模块重新定义 `CpuId = usize` 或另一份 mask。

依赖方向必须保持为：`wateros-base-config` 和各使用方可以依赖 `wateros-base`，`wateros-base` 不反向依赖配置 crate。

## A 交付二：当前 CPU id

### API 层

新增：

```text
os/components/wateros-platform/platform-arch/arch-api/api-v0/src/cpu.rs
```

并在 `arch-api/api-v0/src/lib.rs` 增加 `pub mod cpu;`。

建议 trait：

```rust
use wateros_base::cpu::CpuId;

pub trait ArchCpu {
    /// 返回当前正在执行本段内核代码的逻辑 CPU。
    fn current_cpu_id() -> CpuId;
}
```

### 架构实现

新增或修改：

```text
platform-arch/arch-impl/impl-riscv64/src/cpu.rs
platform-arch/arch-impl/impl-loongarch64/src/cpu.rs
platform-arch/arch-impl/*/src/lib.rs
platform-arch/src/lib.rs
```

聚合层最终暴露：

```rust
pub mod cpu {
    pub use wateros_base::cpu::{CpuId, CpuMask};

    pub fn current_cpu_id() -> CpuId;
}
```

调用路径：

```rust
arch::cpu::current_cpu_id()
```

### 实现注意

- RISC-V 不能在用户态 trap 后无条件把 `tp` 当作 hart id。用户 `tp` 用于 glibc TLS，trap 汇编必须保存用户 `tp` 并恢复内核 CPU-local 值，或者选择不与用户 TLS 冲突的 CPU-local 机制。
- LoongArch 需要确认 QEMU virt 上可稳定读取的 CPU id 来源，并在 AP 早期入口建立逻辑 CPU 映射。
- BSP-only 第一提交允许两个实现暂时返回 `CpuId::BOOT`，但必须在注释和测试中标为单核 fallback。AP 上线前必须替换为真实实现。
- scheduler 不得直接读取 `tp`、CSR 或固件参数，只调用 `arch::cpu::current_cpu_id()`。

### Cargo 依赖

需要在 arch-api 和两个 arch impl 的 `Cargo.toml` 中加入 `wateros-base`。如果 impl 只通过 api-v0 使用类型，可以由 api-v0 再导出，避免重复依赖。

## A 交付三：CPU topology 和 online mask

### API 层

新增：

```text
os/components/wateros-platform/platform-api/api-v0/src/smp.rs
```

同时修改：

- `os/components/wateros-platform/platform-api/api-v0/Cargo.toml`：增加 `wateros-base` 依赖。
- `os/components/wateros-platform/Cargo.toml`：增加 `wateros-base` 依赖，供聚合层保存和校验拓扑。
- 两个具体 platform impl 的 `Cargo.toml`：若实现代码直接引用 `CpuId`/`CpuMask`，增加 `wateros-base` 依赖；仅使用 api-v0 再导出类型时可以不重复添加。

建议公共数据：

```rust
use wateros_base::cpu::{CpuId, CpuMask};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuTopology {
    pub boot_cpu: CpuId,
    pub configured: CpuMask,
    pub online: CpuMask,
}
```

`configured` 表示 QEMU/DTB 声明并准备启动的 CPU，`online` 表示已经完成本核初始化并可进入调度的 CPU。两者不能混用。

### platform 聚合层状态

在 `wateros-platform/src/lib.rs` 增加 `smp` 模块，内部可先使用两个 `AtomicU64` 保存 configured/online mask：

```rust
pub fn init_topology(boot_cpu: CpuId, configured: CpuMask) -> Result<(), SmpError>;
pub fn mark_cpu_online(cpu: CpuId) -> Result<(), SmpError>;
pub fn cpu_topology() -> CpuTopology;
pub fn online_cpu_mask() -> CpuMask;
pub fn online_cpu_count() -> usize;
```

规则：

- `init_topology` 只由 BSP 调用一次。
- BSP 初始化后先将 boot CPU 置 online。
- AP 完成 trap、timer、CPU-local 和内核地址空间初始化后，才调用 `mark_cpu_online`。
- 对外查询使用 Acquire，发布 online 使用 Release。
- 第一轮单核 fallback 为 configured=`{CPU0}`、online=`{CPU0}`。

### 暴露位置

```rust
platform::smp::{cpu_topology, online_cpu_count, online_cpu_mask}
```

B 的 scheduler 不直接依赖该模块。A 在调用 `task::init_smp()` 时把拓扑快照传入 task。

## A 交付四：reschedule IPI 抽象

### platform-api 类型

在 `platform-api/api-v0/src/smp.rs` 增加：

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmpError {
    InvalidCpu,
    CpuOffline,
    Unsupported,
    FirmwareFailure,
}

pub type SmpResult<T> = core::result::Result<T, SmpError>;

pub trait PlatformSmp {
    fn send_reschedule_ipi(cpu: CpuId) -> SmpResult<()>;
}
```

两个 platform impl 提供 `PlatformSmpImpl`。第一轮可以返回 `Unsupported`，但 API 和错误语义必须固定。

### platform 聚合层

最终暴露：

```rust
platform::smp::send_reschedule_ipi(cpu: CpuId) -> SmpResult<()>;
```

### os-kernel 适配

B 不直接依赖 platform 聚合层。A 在 `os/src/main.rs` 或一个新的 `os/src/smp_hooks.rs` 中提供适配函数：

```rust
fn request_task_reschedule(cpu: CpuId) -> bool {
    platform::smp::send_reschedule_ipi(cpu).is_ok()
}
```

然后装入 B 提供的 `TaskSmpHooks`。第一轮 IPI unsupported 时返回 false，scheduler 允许目标 CPU 等下一次 timer tick，不得 panic。

## A 第一轮完成定义

A 应交付以下内容：

- [ ] `CpuId`、`CpuMask` 和 `MAX_CPUS` 只有一份定义。
- [ ] `arch::cpu::current_cpu_id()` 在双架构可编译，单核返回 CPU 0。
- [ ] `platform::smp::cpu_topology()` 单核报告 configured/online CPU 0。
- [ ] `platform::smp::send_reschedule_ipi()` 有固定错误接口，允许暂时 Unsupported。
- [ ] os-kernel 有 task reschedule hook 适配函数。
- [ ] `make rv_check` 和 `make la_check` 通过。

建议提交拆分：

1. `smp(A1): add shared cpu id and mask types`
2. `smp(A1): expose current cpu id through arch`
3. `smp(A1): add platform topology and reschedule IPI API`

## B 交付一：task 公共 SMP 类型

### 修改位置

- `os/components/wateros-task/task-api/api-v0/src/task.rs`
- `os/components/wateros-task/task-api/api-v0/src/snapshot.rs`
- 新增 `os/components/wateros-task/task-api/api-v0/src/smp.rs`
- `os/components/wateros-task/task-api/api-v0/src/lib.rs`
- task-api `Cargo.toml` 增加 `wateros-base` 依赖

### `TaskState`

把：

```rust
TaskState::Running
```

改为：

```rust
TaskState::Running { cpu: CpuId }
```

所有状态匹配同步修改。核心不变量：

- `Running { cpu }` 的 task 必须等于该 CPU 的 `current_task_id`。
- 一个 task 最多处于一个 CPU 的 current 槽位。
- Ready/Blocking/Sleeping/Exited task 不得出现在任何 CPU 的 current 槽位。
- 切入前在 scheduler lock 内标记 `Running { cpu }`。
- 切出时在同一锁内先移除 current 所有权，再转入下一个状态。

### `TaskSmpConfig` 和 hooks

在 `task-api/api-v0/src/smp.rs` 增加：

```rust
use wateros_base::cpu::{CpuId, CpuMask};

#[derive(Clone, Copy)]
pub struct TaskSmpHooks {
    /// 请求目标 CPU 尽快重新调度。false 表示当前平台暂不支持 IPI。
    pub request_reschedule: fn(CpuId) -> bool,
}

#[derive(Clone, Copy)]
pub struct TaskSmpConfig {
    pub boot_cpu: CpuId,
    pub configured: CpuMask,
    pub online: CpuMask,
    pub hooks: TaskSmpHooks,
}
```

校验规则：

- boot CPU 必须同时属于 configured 和 online。
- online 必须是 configured 的子集。
- 空 configured/online mask 返回初始化错误，不 panic。
- 第一轮允许 online 只有 CPU 0。

### `CpuTaskSnapshot`

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuTaskSnapshot {
    pub cpu: CpuId,
    pub online: bool,
    pub current_task_id: Option<TaskId>,
    pub idle_task_id: Option<TaskId>,
    pub current_address_space: Option<AddressSpaceHandle>,
    pub tick_count: u64,
}
```

这是 B 提供给 A 的主要稳定快照。它不暴露 TCB 指针、内核栈指针、`TaskContext` 指针或 scheduler lock guard。

## B 交付二：per-CPU scheduler 数据

### 数据结构

在 scheduler-api 或各 scheduler 共用位置增加：

```rust
use arch::task::ActiveArchTaskContext as TaskContext;
use wateros_base::cpu::CpuId;

pub struct CpuSchedulerState {
    pub cpu: CpuId,
    pub bootstrap_task_cx: TaskContext,
    pub current_task_id: Option<TaskId>,
    pub idle_task_id: Option<TaskId>,
    pub current_task_ticks: u64,
}
```

字段语义：

- `bootstrap_task_cx`：每个 CPU 第一次进入 scheduler 时独立保存其启动上下文，绝不能共享。
- `current_task_id`：该 CPU 当前任务；CPU 未进入 scheduler 时为 None。
- `idle_task_id`：该 CPU 独立 idle TCB。不要再假定所有 CPU 共用 `IDLE_TASK_ID=0`。
- `current_task_ticks`：只累计该 CPU 当前任务的时间片。

存储方式第一版建议使用固定数组：

```rust
cpu_states: [CpuSchedulerState; MAX_CPUS]
```

不要为 CPU 数组使用启动后才分配的 `Vec`，避免 AP 入口依赖额外分配和扩容。

### `TaskRegistry` 改动

位置：

```text
task-scheduler/scheduler-api/api-v0/src/registry.rs
```

需要改动：

- 从 `TaskRegistry` 删除全局 `bootstrap_task_cx`。
- 从 `TaskRegistry` 删除全局 `current_task_id`。
- `spawn/fork/clone/exec` 中的“当前任务”由调用方传入 `CpuId`，registry 再查对应 CPU current。
- `first_switch_to`、`take_current_switch_out`、`mark_running_and_set_current` 增加 `cpu: CpuId` 参数，操作对应 `CpuSchedulerState`。
- 初始化时为每个 configured CPU 创建独立 idle TCB，保存到对应 `idle_task_id`。
- `is_idle(task_id)` 继续由 TCB kind 判断，不再依赖固定 task id。
- `TaskTable::remove` 不再特殊判断单一 `IDLE_TASK_ID`；所有 idle TCB 都不进入普通 reap 路径。

建议函数形状：

```rust
fn current_task_id(&self, cpu: CpuId) -> Option<TaskId>;
fn first_switch_to(&mut self, cpu: CpuId, next: TaskId) -> SwitchPair;
fn take_current_switch_out(&mut self, cpu: CpuId) -> Option<(TaskId, *mut TaskContext)>;
fn mark_running_and_set_current(
    &mut self,
    cpu: CpuId,
    task_id: TaskId,
) -> *const TaskContext;
```

如果 `CpuSchedulerState` 放在 scheduler 而不是 registry 中，这些方法可由 scheduler 组合调用，但所有权规则必须保持一致，不能同时保留 registry 和 scheduler 两份 current 真相。

### scheduler impl 改动

位置：

```text
task-scheduler/scheduler-impl/impl-multi-class/src/lib.rs
task-scheduler/scheduler-impl/impl-multi-class/src/scheduler.rs
task-scheduler/scheduler-impl/impl-round-robin/src/lib.rs
task-scheduler/scheduler-impl/impl-round-robin/src/scheduler.rs
```

需要改动：

- 全局 scheduler storage 从 `UniprocessorSafeCell` 改为真正的 spin mutex 或等价 SMP lock。
- `with_scheduler` 先保存并关闭本核中断，再获取跨核 scheduler lock。
- 所有 current 操作先读取 `arch::cpu::current_cpu_id()`。
- `current_task_ticks` 移入 `CpuSchedulerState`。
- `prepare_first_switch`、`schedule`、`schedule_wait` 明确当前 CPU 参数，或在最外层读取一次后传入，避免同次决策中多次读取不同来源。
- ready、wait、sleep、exited queue 第一版保持全局，由 scheduler lock 统一保护。
- 在释放 scheduler lock 前确定 current/next 所有权和状态；释放锁后再调用 `__switch`。

### 上下文切换边界

禁止：

```text
持 scheduler lock -> __switch -> 期望另一任务替当前栈解锁
```

正确顺序：

```text
关本核中断
  -> 获取 scheduler lock
  -> 选择 next
  -> current 状态转移
  -> next 标为 Running(cpu)
  -> 取得两个 TaskContext 指针
  -> 释放 scheduler lock
  -> 按路径决定何时恢复本核中断
  -> __switch
```

`SwitchPair` 中的 TCB/TaskContext 生命周期必须在 switch 完成前稳定。reap 不能并发销毁 current 或 next。

## B 交付三：task 初始化和 CPU 上线入口

### 新入口

在 `wateros-task/src/lib.rs` 最终暴露：

```rust
pub fn init_smp(config: TaskSmpConfig) -> Result<(), TaskSmpInitError>;

/// 当前 CPU 首次进入调度器。BSP 和 AP 各调用一次，不返回。
pub fn run_first_task_on_current_cpu() -> !;

/// platform 将 AP 标为 online 后，同步 task 的可调度 CPU 集合。
pub fn set_cpu_online(cpu: CpuId) -> Result<(), TaskSmpError>;
```

兼容入口：

```rust
pub fn init();
pub fn run_first_task() -> !;
```

第一轮可以保留，内部构造 CPU 0 单核配置并转调新接口。这样现有启动代码和测试不会立即全部中断。

### 初始化顺序

BSP：

```text
A: platform::smp::init_topology
  -> A: 获取 CpuTopology
  -> A: 组装 TaskSmpConfig + hooks
  -> B: task::init_smp
  -> 全局 task/process 初始化
  -> B: task::run_first_task_on_current_cpu
```

AP 后续接入：

```text
A: AP arch/trap/timer/MM 初始化
  -> A: platform::smp::mark_cpu_online
  -> B: task::set_cpu_online
  -> B: task::run_first_task_on_current_cpu
```

`set_cpu_online` 不负责硬件初始化，也不调用 platform。

## B 交付四：向 A 暴露查询接口

### 当前 CPU 查询

保留现有接口名称，但语义改为当前 CPU：

```rust
pub fn current_task_id() -> Option<TaskId>;
pub fn current_task_snapshot() -> Option<TaskSnapshot>;
pub fn current_task_user_aspace_ptr() -> usize;
pub fn current_task_user_address_space_token() -> usize;
pub fn current_task_trap_return_address_space_token() -> usize;
```

这些函数必须先通过 `arch::cpu::current_cpu_id()` 找到本 CPU current，不能读取全局 current。

### 指定 CPU 快照

在 `wateros-task` 根 crate 暴露：

```rust
pub fn cpu_task_snapshot(cpu: CpuId) -> Option<CpuTaskSnapshot>;
pub fn all_cpu_task_snapshots() -> alloc::vec::Vec<CpuTaskSnapshot>;
pub fn running_cpu(task_id: TaskId) -> Option<CpuId>;
```

用途：

- A 的启动日志确认每个 CPU 是否进入 task runtime。
- procfs 生成 CPU/task 调试信息。
- debug 断言检查 task 是否双跑。

### 地址空间运行 CPU mask

在 `wateros-task` 根 crate 暴露：

```rust
pub fn address_space_active_cpu_mask(address_space: AddressSpaceHandle) -> CpuMask;
```

实现语义：

- 遍历 per-CPU current 快照。
- 当前 task 的用户地址空间 token 与参数相同，则设置对应 CPU 位。
- 内核任务和 idle 不计入用户地址空间 mask。
- 返回的是调用瞬间的快照，不提供长期 pin 保证。

A 的 MM 在第一版 TLB shootdown 中使用该 mask。后续若需要“读取 mask 到 shootdown 完成期间不允许迁移”的强保证，应单独增加 generation/ack 协议，不能把普通快照误当成同步屏障。

### 暴露层级

公共类型：

```rust
wateros_task::api_v0::{TaskSmpConfig, TaskSmpHooks, CpuTaskSnapshot}
```

聚合层函数：

```rust
wateros_task::{
    init_smp,
    set_cpu_online,
    run_first_task_on_current_cpu,
    current_task_id,
    cpu_task_snapshot,
    all_cpu_task_snapshots,
    running_cpu,
    address_space_active_cpu_mask,
}
```

不要从公共 API 暴露：

- `CpuSchedulerState` 的可变引用
- scheduler mutex guard
- TCB 指针
- `TaskContext` 指针
- ready/wait queue 内部结构

## A 与 B 的最终接口表

| 接口/类型 | 提供者 | 定义位置 | 使用者 |
|---|---|---|---|
| `CpuId`、`CpuMask` | A | `wateros-base::cpu` | platform、arch、task、MM、procfs |
| `MAX_CPUS` | A | `wateros-base-config::task` | platform、scheduler |
| `arch::cpu::current_cpu_id()` | A | `wateros-platform-arch` | B 的所有 current 路径 |
| `CpuTopology` | A | `platform-api::smp` | os-kernel、task 初始化 |
| `platform::smp::online_cpu_mask()` | A | `wateros-platform` | os-kernel、procfs |
| `platform::smp::send_reschedule_ipi()` | A | `wateros-platform` | os-kernel hook |
| `TaskSmpConfig`、`TaskSmpHooks` | B | `task-api::smp` | A 组装并传入 task |
| `TaskState::Running { cpu }` | B | `task-api::task` | scheduler、procfs/debug |
| `CpuTaskSnapshot` | B | `task-api::smp` | A 的启动、MM、procfs |
| `task::init_smp()` | B | `wateros-task` | A 的 BSP 启动 |
| `task::set_cpu_online()` | B | `wateros-task` | A 的 AP 启动 |
| `task::run_first_task_on_current_cpu()` | B | `wateros-task` | A 的 BSP/AP 入口 |
| `task::cpu_task_snapshot()` | B | `wateros-task` | A 的启动和诊断 |
| `task::running_cpu()` | B | `wateros-task` | debug、诊断 |
| `task::address_space_active_cpu_mask()` | B | `wateros-task` | A 的 MM/TLB shootdown |

## 合入顺序

为了避免双方互相等待，按以下顺序合入：

1. A：`CpuId`、`CpuMask`、`MAX_CPUS`。
2. A：`arch::cpu::current_cpu_id()`，先提供 CPU 0 fallback。
3. B：task-api 的 `TaskSmpConfig`、`CpuTaskSnapshot`、`Running { cpu }`。
4. B：per-CPU scheduler 数据和 CPU 0 兼容路径。
5. A：platform topology、IPI API 和 os-kernel hook。
6. B：`init_smp`、`set_cpu_online`、指定 CPU 快照和地址空间 mask。
7. A/B：接入真实 AP 启动和 reschedule IPI。

步骤 1 至 4 完成后，现有单核 QEMU 行为应保持不变，B 已可独立完成大部分 scheduler 改造。

## 验收测试

### 静态检查

```bash
cd os
make rv_check
make la_check
```

### CPU 0 兼容回归

- `task::init()` 和 `task::run_first_task()` 旧入口仍可工作。
- `arch::cpu::current_cpu_id()` 返回 CPU 0。
- topology 的 configured/online mask 都只含 CPU 0。
- `current_task_id()` 与改造前单核语义一致。
- `cpu_task_snapshot(CpuId::BOOT)` 返回当前/idle/tick 信息。
- `address_space_active_cpu_mask()` 对当前用户任务返回 bit 0。

### scheduler 不变量测试

B 至少增加以下测试或 debug 断言：

- 同一 task 不能被两个 `CpuSchedulerState.current_task_id` 同时引用。
- `Running { cpu }` 与 CPU current 槽位互相一致。
- 每个 configured CPU 有不同 idle task 和 bootstrap context。
- idle task 不进入普通 ready queue，也不能被 reap。
- 非 online CPU 不能进入 `run_first_task_on_current_cpu()`。
- scheduler lock 在 `__switch` 前已经释放。

### 未来 8 核接入验收

- 8 个 CPU 的 `cpu_task_snapshot()` 都显示 online。
- 8 个 CPU 的 idle task id 不同。
- 普通 task 能在不同 CPU 上运行，但同一时刻只属于一个 CPU。
- reschedule IPI unsupported 时系统仍正确，只是唤醒延迟较高。
- IPI 实现后，idle CPU 能被新任务及时唤醒。
