# 资源分配与生命周期审计

## 任务目标

审计内核内**所有可分配资源**的分配与回收链路，梳理每种资源在特定分配路径下的**完整生命周期**，评估系统持有时能否保证**资源账本稳定**（不泄漏、不重复释放、不悬空引用）；并检查在**资源不足时强行分配**是否有明确、可预期的失败处理。

**背景假设**：跑测试时频繁出现意外卡死或后期行为异常，部分原因可能是资源泄漏、回收路径缺失、错误路径未回滚、或耗尽时静默继续分配导致内核状态不一致。

**本任务交付**：

1. 审计文档（问题清单 + 生命周期说明）
2. 《修复任务队列》（按优先级排列的可执行修复项）
3. （可选）收敛/修复代码修改

不要求一次补全所有缺失的回收或限额逻辑；当前目标是**摸清账本 + 标注风险 + 收敛不可信路径**。

## 执行前必须参考的 prompt

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/architecture.md`
- `docs/prompts/coding.md`

## 执行前必须参考的导出文档

- `docs/exports/README.md`
- `docs/exports/snapshot/current.md`
- `docs/exports/architecture/components.md`
- `docs/exports/architecture/module-relations.md`
- 按资源所在子系统按需阅读：
  - `docs/exports/features/wateros-mm.md`、`docs/exports/impl-guide/wateros-mm.md`
  - `docs/exports/features/wateros-task.md`、`docs/exports/impl-guide/wateros-task.md`
  - `docs/exports/features/wateros-vfs.md`、`docs/exports/impl-guide/wateros-vfs.md`
  - `docs/exports/features/wateros-fs.md`
  - `docs/exports/features/wateros-ipc.md`
  - `docs/exports/features/wateros-runtime.md`
  - `docs/exports/features/wateros-driver.md`
  - `docs/exports/features/wateros-syscall.md`
- 若已有相关审计产出，对照避免重复劳动：
  - `docs/audits/syscall-issues.md`、`docs/audits/syscall-coverage.md`
  - `docs/audits/lock-issues.md`、`docs/audits/lock-coverage.md`、`docs/audits/lock-inventory.md`

## 需要优先查看的源文件

| 资源类别 | 典型位置（以代码搜索为准） |
|----------|---------------------------|
| 物理页帧 / 页表 | `wateros-mm/**`（`StackFrameAllocator`、映射/unmap） |
| 内核堆 | `wateros-runtime/runtime-heap-allocator/**` |
| 任务 / 线程槽位 | `wateros-task/**`（`ProcessRegistry`、调度器、栈分配） |
| 文件描述符 | `wateros-vfs/**`（`PerTaskFdRegistry`、`fd.rs`） |
| VFS 句柄 / inode 引用 | `wateros-vfs/**`、`wateros-fs/**` |
| 页缓存 / 块缓存 | `wateros-vfs/vfs-impl/impl-page-cache/**`、`wateros-driver/**/block-cache/**` |
| pipe / socket / shm | `wateros-ipc/**`、`wateros-syscall/**/socket_fd.rs`、`unix_sock.rs` |
| 挂载 / 文件系统实例 | `wateros-vfs/**/mount_table.rs`、`wateros-fs/fs-rootfs/**` |
| futex / 信号表项 | `wateros-ipc/ipc-futex/**`、`wateros-ipc/ipc-signal/**` |
| 设备注册表 | `wateros-driver/**` |

搜索关键字建议：`alloc`、`free`、`dealloc`、`drop`、`release`、`close`、`destroy`、`unregister`、`unmap`、`remove`、`take`、`leak`、`ENOMEM`、`OutOfMemory`、`capacity`、`limit`、`refcount`、`Arc`、`Rc`。

## 搜索范围

- `os/components/**` 全组件
- syscall 入口到子系统实现的跨模块调用链（分配发生在哪、释放在哪）
- 错误路径与早期 `return`/`?` 分支上的回滚是否完整

## 输出目录

主 agent 汇总后写入（若目录不存在则创建）：

- `docs/audits/resource-issues.md` — **文档 A**：《资源生命周期潜在问题清单》
- `docs/audits/resource-lifecycle.md` — **文档 B**：《资源类型与生命周期说明》
- `docs/audits/resource-fix-queue.md` — **文档 C**：《资源审计修复任务队列》
- `docs/audits/resources/` — 各 subagent 单资源文档（`docs/audits/resources/<resource-kind>.md`）

可选：

- `docs/audits/resource-inventory.md` — 《可分配资源清单》（拆分子任务前产出）

## 审计维度（每种资源均需覆盖）

对每一种可分配资源，subagent 须回答：

1. **分配入口**：哪些函数/syscall/初始化路径会分配该资源；分配条件与前置依赖。
2. **回收入口**：正常释放、异常回滚、`Drop`、任务退出、文件 `close`、进程 `exit` 等所有释放入口是否齐全。
3. **生命周期状态机**：从「未分配 → 已分配 → 使用中 → 已释放」各阶段的持有者与转移条件；是否存在「半初始化」状态。
4. **账本稳定性**：
   - 分配与释放是否成对（含错误路径上的 partial alloc 回滚）
   - 引用计数 / 所有权转移是否正确（`Arc`、fd 表、open_refs 等）
   - 是否存在 double-free、use-after-free、泄漏、野指针复用风险
5. **耗尽处理**：容量上限是多少（若存在）；资源不足时返回什么（`ENOMEM`、`EBADF`、panic、无限重试、静默截断）；是否应拒绝分配却继续执行。
6. **跨资源耦合**：例如 fork 时 fd 表复制、exit 时批量回收、unmap 与帧回收顺序等。

## 并行拆分策略

**主 agent 职责**：盘点 → 并行派发 subagent → 收集 → 去重合并 → 产出文档 A/B/C。

1. **盘点**：枚举所有可分配资源类型（名称、所属组件、分配 API、回收 API、是否有硬上限、初步复杂度）。
2. **拆分单位**：默认**每种资源类型（或强相关的一组）一个 subagent**。建议分组示例：
   - `physical-frames` — 物理页帧与映射
   - `kernel-heap` — 运行时堆
   - `task-slots` — 任务/进程槽位与用户栈
   - `file-descriptors` — per-task fd 表与 dup/fork/exit
   - `vfs-handles` — 打开文件、目录项、挂载引用
   - `page-cache` / `block-cache` — 缓存页与淘汰
   - `pipe-buffers` — 内核 pipe 环缓冲
   - `ipc-shm-futex-signal` — 共享内存、futex 表项、信号注册
   - `sockets` — 网络/unix socket 句柄与全局注册表
   - `fs-instances` — 挂载点、SharedFs、rootfs 句柄
   - `driver-slots` — 块设备、字符设备、网络设备注册
3. **每个 subagent 交付**（单资源文档），至少包含：
   - 资源名称、所属组件、主要类型/结构体
   - 分配链路与回收链路（函数级，附文件路径）
   - 生命周期状态图或文字描述
   - 账本稳定性结论（稳定 / 部分稳定 / 不可靠）
   - 耗尽与失败处理现状（及与 Linux/预期语义差距）
   - **潜在问题列表**（按严重程度：泄漏 / UAF / 卡死 / 静默耗尽 / 错误码不符）
   - **收敛建议**：不可靠路径应如何 warn、应返回何错误、是否需加硬上限
   - **修复任务草案**：1～N 条可独立执行的修复项（标题、文件、验收标准）
4. **主 agent 汇总**：
   - 文档 A：去重合并所有问题，按严重程度排序
   - 文档 B：按资源类型分节的统一生命周期手册
   - 文档 C：从各 subagent 修复草案合并为优先级队列（P0 泄漏/UAF/卡死 → P1 错误路径回滚 → P2 限额与错误码）

## 修复与收敛策略

对确认**生命周期不完整或耗尽处理不可靠**的路径，不要按「看似能分配」的方式强行继续：

1. 在分配入口或危险分支加入判断（上限检查、状态校验）
2. 日志打印 **warn**，内容包含：**资源类型名称、分配/释放操作、调用位置（函数/文件）、相关参数或当前用量**（如 `used/capacity`）
3. 返回明确错误（如 `-ENOMEM`、`-EMFILE`、`-ENOSPC` 或内部 `Err`），或走安全失败路径；**禁止**静默截断、无限重试或 panic（除非文档明确标注为不可恢复且已有约定）
4. 错误路径上已 partial 分配的资源必须回滚释放
5. 将路径记入文档 A（「已收敛 / 待实现」），对应修复项写入文档 C

具体修改由 subagent 在单资源文档中给出建议；主 agent 汇总时统一错误码与日志风格。

## 约束与假设

- 当前阶段为**单核多线程**；审计以该模型为 baseline，多核路径合理则不视为错误，但须标注额外并发风险
- 以 **Linux 常见资源语义**为对照（fd 上限、`ENOMEM`、`EMFILE`、`EBADF` 等），内核内部资源可无用户态 errno，但须有一致失败契约
- 面向**所有**已识别的可分配资源，不遗漏
- 优先标注会导致**泄漏、UAF、卡死、全局耗尽后雪崩**的路径
- 与用户沟通使用**简体中文**

## 完成后的回填要求

- 将 `docs/audits/resource-issues.md`、`docs/audits/resource-lifecycle.md`、`docs/audits/resource-fix-queue.md` 纳入版本管理
- 高优先级修复项回填 `docs/roadmap/todolist.md` 或相关 work package
- 若本轮已落地收敛/修复，在对应 `docs/exports/features/<component>.md` 中简要注明资源限额或已知泄漏修复（或指向审计文档，避免双源冲突）
- 与 syscall/锁审计交叉项（如同一 fd 路径）在文档 A 中交叉引用，避免重复开修

---

## 主 Agent 执行顺序

1. 扫描代码库，生成《可分配资源清单》
2. 按清单并行启动 subagent
3. 收集各 subagent 单文档
4. 去重、合并，产出文档 A、B、C
5. （可选）按文档 C 的 P0/P1 执行修复或收敛代码
6. 完成回填

## 与其他审计任务的对应关系

| 维度 | 系统调用审计 | 锁机制审计 | 本任务 |
|------|-------------|-----------|--------|
| Baseline | Linux syscall 语义 | 单核多线程持锁闭环 | 分配/回收成对 + 账本稳定 + 耗尽可预期失败 |
| 拆分单位 | 每个 syscall | 每个带锁数据结构 | 每种可分配资源 |
| 文档 A | syscall 潜在问题 | 锁机制潜在问题 | 资源生命周期潜在问题 |
| 文档 B | Linux vs 当前覆盖 | 预期语义 vs 当前实现 | 资源类型与完整生命周期 |
| 额外产出 | 高优先级收敛列表 | 高优先级修复列表 | **修复任务队列（文档 C）** |
| 不可靠路径 | warn + 返回错误 | warn + 安全失败 | warn + 失败返回 + **partial alloc 回滚** |
