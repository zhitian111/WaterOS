# wateros-task 功能快照

## 事实来源

- `os/components/wateros-task/Cargo.toml`、`src/lib.rs`
- `task-api/api-v0`、`task-impl/impl-core`、`task-scheduler/`
- `os/src/main.rs`（`task::init`、`init_kernel_trap_satp`、`run_first_task`）
- `os/src/self_tests/task.rs`（固定 hello world ELF 启动与 pipe IPC 自检）

## 当前状态

当前已具备单核内核态任务切换、timer 驱动 round-robin 调度，以及 Stage3A 第一轮边界收紧后的任务/runtime/scheduler 分层。

当前已落地的能力包括：

- 任务对象由 `task-impl/impl-core` 统一承载
- `TaskSnapshot` 已收敛为稳定公共快照，不再暴露栈顶地址和启动协议细节
- 任务状态已从单纯 `Ready/Running` 扩展为 `Ready`、`Running`、`Blocking`、`Sleeping`、`Exited`
- 调度器已可区分 `yield`、timer tick、阻塞、睡眠与退出等调度原因
- 调度器已开始收敛为“任务注册表 + TaskId 队列”，并具备最小的阻塞队列、睡眠队列、退出队列和显式唤醒入口
- 已具备最小 `WaitQueue` 能力，可显式 `wait_current`、`wait_current_for_ticks`、`wake_one`、`wake_all`
- 已具备条件等待能力：`wait_on_while` / `wait_on_while_for_ticks` 与 `WaitQueue::wait_current_while*` 会在调度临界区内复查条件，服务 pipe 等 IPC 对象的无丢唤醒等待
- 已具备最小的 timed wait 与退出回收入口，可显式 `reap_exited_task`、`reap_one_exited_task`
- 已引入通用 `TaskWaitHandle` / `TaskWaitTarget`，`waitqueue`、“等待任务退出”与“等待任意子任务退出”已共用同一条等待与 timeout 路径
- spawn 会记录最小 `parent_id`，`TaskSnapshot` / `ExitedTask` 暴露该关系，供 syscall `waitpid` 判断与回收子任务
- 退出任务现在会保留为可回收 zombie，并在退出时自动唤醒等待其退出的 waiter 或父任务的 child-exit waiter
- task 根 crate 已收紧为 facade，trap/tick/task-entry hook 已迁入内部 runtime
- trap 路径已开始把完整 trap frame 快照复制进当前任务对象，并在返回前回写到 trap 栈帧
- trap 读写路径已显式区分“是否返回用户态”的语义，完整 trap frame 留在 `platform-arch`/task impl 机制层，task 公共 API 通过 `TaskTrapSnapshot` 暴露架构无关语义快照
- 已具备最小 `spawn_user_task` 骨架：用户任务可预分配用户栈，并准备首次 `sret` 进入所需的 trap frame
- 已接入 `wateros-mm-api-v0::kernel_bringup::LoadedElf`：task 根 crate 可将 MM loader 返回的 `entry_pc`、`satp`、镜像范围与外部栈区间转换为 `UserTaskSpec`，并直接生成用户态任务
- RISC-V 自检已以根卷默认 ELF 作为唯一用户态回归路径；ELF observer 会等待、reap 并校验退出码、trap frame、地址空间、image 与外部栈元数据
- LoongArch64 路径已用独立 `.text.user_smoke` 段创建 PLV3 用户态 syscall smoke，并通过 `UserTaskSpec` / observer 校验 entry、image、栈与 trap frame 快照；它目前不声明地址空间句柄，真实 ELF 任务仍依赖后续 LoongArch MM/FS/loader 接入
- `current_task_snapshot` 可提供不含任务切换上下文、但包含最近一次 trap 语义快照的轻量任务状态快照与统计信息

## 后续关注点

- 继续把当前“复制 + 回写”模式推进为完整 trap frame 归属与恢复模型
- 继续把当前 wait handle 与条件等待模型推进为更完整的通用阻塞对象 / block object 层
- 继续补更明确的 task handle / generation 语义，并在 fork/exec 完成后收敛完整父子进程生命周期
- 继续扩展真实用户态镜像覆盖面，包括更多 syscall 与进程/地址空间场景
- 持续补齐注释与公共 API 文档

## fork/clone 实现说明

### 当前实现（2026-05）

`fork`（即 `clone` 系统调用中 `child_stack=0` 的情形）的处理途径 `task::fork_current` → `fork_user_task`。

#### 用户栈分配策略

子任务的用户栈分配取决于父任务的栈类型和 `child_stack` 参数：

| 父栈类型 | child_stack != 0（clone） | child_stack == 0（fork） |
|---------|--------------------------|------------------------|
| `UserStackBacking::Kernel` | 分配新 `UserStack` | 分配新 `UserStack` |
| `UserStackBacking::External` | 共享父栈区间 | **共享物理页，但 SP 放在栈底+4KB** |

**fork + External 栈**（当前 oscomp 测例路径）的实现在 `fork_user_task`（`task-impl/impl-core/src/tcb.rs`）中：

```
External 栈 [bottom, top) 例如 [0x7fff6000, 0x7fffa000)
    bottom + 4096  ← 子进程 SP（CHILD_STACK_GUARD）
      ...
    top - ~0x500   ← 父进程 SP（调用 fork 时的栈顶附近）
```

父子进程使用同一片物理栈页上的不同区域，子进程的栈操作不会覆盖父进程的栈帧。

#### 地址空间与文件描述符

- 地址空间（`AddressSpaceHandle` / `satp`）**完全共享**——父子进程使用相同的页表。
- 文件描述符表（`vfs::cwd`）在 `sys_clone` 中通过 `copy_cwd_from_parent` 继承。

#### 已知限制

1. **地址空间共享**：由于页表完全共享，子进程对任何可写页面的修改（包括 libc 的 `FILE` 结构、全局变量等）都会影响父进程。当前 oscomp 测例均使用裸 `write` 系统调用而非 `printf`，因此不会触发此问题；但运行使用 `printf`/`fprintf` 等缓冲 IO 的程序时会导致父进程输出异常或崩溃。

2. **栈顶安全区域依赖**：当前策略假设子进程仅使用栈底部 `CHILD_STACK_GUARD`（4KB）空间，父进程使用栈顶剩余区域。若子进程栈使用超过 4KB，仍会侵入父进程区域。当前 oscomp fork 测例子进程仅调用 `write` + `exit`，远低于此阈值。

3. **COW 缺位**：没有写时复制（Copy-on-Write）机制，`fork` 不会复制物理页，所有可写页共享。

### 长期改造方向

当前的 External 栈分区方案是**临时性**的，长期必须用真正的地址空间隔离替代：

**方案一：独立地址空间 + 逐页复制（中期）**

为子进程创建独立 `Sv39AddressSpace`，遍历父进程页表并复制每一项，为可写页分配新帧并拷贝内容。这是最简单的正确实现，但 fork 开销与父进程工作集大小成正比。

**方案二：写时复制（长期推荐）**

在方案一的基础上，将可写页标记为只读，父子进程任一者写入时触发 page fault 再复制。这是 Linux 等成熟内核的经典做法，延迟了物理页的复制，大幅降低 fork 开销。

**方案三：`UserStackBacking::Kernel` 独立栈 + MM 页表映射（备选）**

此前尝试过为子进程分配内核堆栈（`UserStackBacking::Kernel`）并在用户页表中添加 `U` 权限。该方案在技术上可行，但需要解决：
- 内核堆页面在用户页表中的 `U` 位标记（已在 `setup_forked_child_stack` 中实现过 `protect_page`）
- 父子进程共享 libc 可写数据区的问题依然存在
- 需要 TLB 刷新的正确性保证

此方案不能解决地址空间共享的根本问题，**不再优先考虑**。

### 优先级建议

1. **短期**：保持当前栈分区方案，确保 oscomp 基础测例通过。
2. **中期**：实现方案一（独立地址空间 + 逐页复制），在 `fork_user_task` 或 `sys_clone` 中调用新的 MM API `copy_address_space`（待实现）。
3. **长期**：实现方案二（COW），由 MM 层提供 `fork_address_space` 原语。
