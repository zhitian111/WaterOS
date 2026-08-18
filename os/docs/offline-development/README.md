# WaterOS 线下开发手册

本文档集面向无法使用 Agent、只能阅读源码和手工调试的比赛现场。目标不是替代各组件
README，而是回答三个更实际的问题：

1. 故障属于哪个组件，第一处断点或日志应放在哪里；
2. 修改一个功能必须同时维护哪些状态、生命周期钩子和错误边界；
3. 怎样用最小测试证明局部正确，再扩大到双架构和完整 workload。

所有描述以当前源码为准。组件 README 解释内部机制；本手册解释跨组件修改方法。

## 阅读顺序

| 场景 | 首先阅读 | 然后阅读 |
| --- | --- | --- |
| 第一次接触内核 | [架构与调用链](architecture-and-call-chains.md) | 对应组件 README |
| 启动、SMP、shell 或评测队列异常 | [启动与 bring-up](boot-and-bringup.md) | platform、task、FS README |
| 驱动、根挂载、写回或网络异常 | [设备/存储/网络/runtime](device-storage-network-runtime.md) | driver、FS、network、runtime README |
| console、TTY、日志、GDB 或 GUI 异常 | [可观测性与交互边界](console-tty-klog-debug-gui.md) | tty、klog、debug、gui README |
| 新增或补全 syscall | [添加系统调用](adding-a-syscall.md) | `wateros-syscall/syscall-impl/impl-kernel/src/sys/<domain>/README.md` |
| 增加 fd、procfs、socket option 或 task 状态 | [功能补充实例](feature-cookbook.md) | 对应实现源码与生命周期手册 |
| panic、卡死、OOM、错误码不符 | [调试与回归](debugging-and-regression.md) | 状态所有者组件 README |
| 执行 LTP、压力或 benchmark | [测试执行与结果口径](testing-playbook.md) | [现有 PPT 数据底稿](../reports/2026-08-18-ltp-bench-ppt-report.md) |
| 修改 fork/exec/exit | 架构文档的“进程生命周期” | task、MM、VFS、IPC、cred README |
| 查资源泄漏或过早释放 | [跨组件数据结构与生命周期](data-structure-lifetimes.md) | task syscall、MM、VFS README |
| 修改文件映射或页缓存 | MM 与 VFS README | syscall mem/fs README |
| 修改任意一级组件 | [组件修改检查表](component-change-checklists.md) | 对应组件 README |
| 不知道源码入口或搜索词 | [全组件源码导航](source-navigation-index.md) | 再回到对应专题手册 |

## 代码分层：先找状态所有者

WaterOS 的组件通常分为三层：

```text
wateros-foo/
├── foo-api/api-v0/       跨组件契约、值类型、trait；不能依赖具体实现
├── foo-impl/impl-*/      状态、锁、队列、硬件或算法实现
└── src/lib.rs            feature 选择和稳定再导出
```

修改时遵守以下判断：

- ABI 参数、Linux flag、errno：属于 `wateros-syscall`。
- 地址空间、PTE、VMA、物理帧、TLB：属于 `wateros-mm`。
- fd 表、打开文件描述、路径解析、页缓存：属于 `wateros-vfs`。
- inode、目录项、磁盘格式和文件系统同步：属于 `wateros-fs`。
- 线程、进程、调度实体、等待/回收：属于 `wateros-task`。
- pipe/futex/signal/SHM 等机制状态：属于 `wateros-ipc`。
- IRQ、定时器、上下文切换、ISA trap 帧：属于 `wateros-platform`。
- VirtIO 队列和设备发现：属于 `wateros-driver`。

不要因为 syscall 是入口，就把长期状态放进 syscall handler；也不要为了绕过后端缺口，
在 handler 中返回假成功。查询类兼容接口可以有有据可查的退化值，改变系统状态的操作必须
由真正的状态所有者完成。

## 组件导航

| 组件 | 状态与职责 | 常见修改入口 | 详细文档 |
| --- | --- | --- | --- |
| base | CPU ID/mask、once cell、集中容量配置 | `wateros-base/base-config` | [README](../../components/wateros-base/README.md) |
| platform | trap、SMP、IPI、时钟、上下文、板级初始化 | `platform-arch/arch-impl/*` | [README](../../components/wateros-platform/README.md) |
| runtime | 堆、panic、console、logging、serial | `runtime-*/` | [README](../../components/wateros-runtime/README.md) |
| task | TCB、进程组、调度器、睡眠与回收 | `task-impl/impl-core` | [README](../../components/wateros-task/README.md) |
| mm | 帧、页表、VMA、COW、用户拷贝、TLB | `mm-impl/impl-{sv39,loongarch64}` | [README](../../components/wateros-mm/README.md) |
| vfs | fd/cwd、路径、打开句柄、挂载路由、页缓存 | `vfs-impl/*` | [README](../../components/wateros-vfs/README.md) |
| fs | rootfs、devfs/procfs、ext4/ramfs 后端 | `fs-impl/*` | [README](../../components/wateros-fs/README.md) |
| ipc | pipe、futex、signal、waitqueue、SHM、eventfd | `ipc-*/` | [README](../../components/wateros-ipc/README.md) |
| cred | 每任务 credential 与 fork/clone 生命周期 | `cred-impl/impl-root` | [README](../../components/wateros-cred/README.md) |
| syscall | generic64 ABI、分发表、用户拷贝、errno | `syscall-impl/impl-kernel/src/sys` | [README](../../components/wateros-syscall/README.md) |
| driver | DTB/PCI 探测、VirtIO、设备注册 | `driver-impl/*`、各类 driver | [README](../../components/wateros-driver/README.md) |
| network | socket 状态与 smoltcp 协议栈 | `network-impl/impl-smoltcp` | [README](../../components/wateros-network/README.md) |
| tty | console tty、行规程、前台进程组 | `tty-impl/impl-console` | [README](../../components/wateros-tty/README.md) |
| klog | 并发内核日志环和读取游标 | `klog-impl/impl-kernel` | [README](../../components/wateros-klog/README.md) |
| debug | GDB 可读快照、事件和锁记录 | `wateros-debug/src` | [README](../../components/wateros-debug/README.md) |
| gui | 内核软件合成器和输入事件 | `gui-impl/impl-software` | [README](../../components/wateros-gui/README.md) |
| utils | 无状态格式化工具 | `table-format` | [README](../../components/wateros-utils/README.md) |

## 从故障现象反查入口

| 现象 | 第一检查点 | 第二检查点 | 不应首先修改 |
| --- | --- | --- | --- |
| 未知 syscall 返回 `ENOSYS` | `syscall_nr_dispatch.rs` 是否登记 | `number.rs` 调用号 | trap 汇编 |
| 参数稍大就 panic/OOM | `fallible_buf.rs` 与容器 `try_reserve` | base-config 容量、资源生命周期 | 单纯扩大 QEMU RAM |
| 用户指针导致 kernel fault | `user_copy.rs` | MM `UserMemoryOps` 和 fault 路径 | 裸指针 `unsafe` 解引用 |
| fork 压力下内存持续上涨 | task 子进程是否 reap | fd/VMA/signal/cred 的退出钩子 | 增大固定内核堆 |
| `munmap`/exit 后物理内存下降 | VMA 对页面的所有权分类 | fork 引用计数、destroy 回收 | `/proc/meminfo` 统计公式 |
| 文件写入成功但 fsync `EIO` | VFS writeback 与 flush 边界 | FS `sync`、块设备 flush | 吞掉所有 VFS 错误 |
| pipe/futex 永久睡眠 | 注册 waiter 与条件复查顺序 | wake、signal、timeout 清理 | 调度器随机 yield |
| SMP 偶发旧映射 | MM TLB CPU 集合与 shootdown | platform IPI | 只做本地 fence |
| LoongArch 能过、RISC-V 失败 | 两套 arch/MM 实现差异 | trap 帧 ABI、页表 flag | 通用 syscall handler 分叉 |
| QEMU 启动时报 hostfwd 失败 | 宿主 `127.0.0.1:2222` | `WOS_QEMU_HOSTFWD` | 内核网络代码 |

## 修改前的五项记录

动手前在纸上写清楚以下内容，能显著减少“修一处坏三处”：

1. **状态所有者**：哪个结构保存真相，是否存在重复 side table。
2. **创建和销毁**：boot/open/fork/clone/exec/exit/reap 中哪些事件会触及它。
3. **并发规则**：保护锁、原子顺序、能否睡眠、锁内是否允许用户拷贝或设备 I/O。
4. **错误边界**：底层错误在哪一层转成 `ErrNo`，失败后状态是否可重试。
5. **验证证据**：成功值、失败 errno、资源回到基线、双架构编译分别由什么测试证明。

## 每次修改的最小闭环

```mermaid
flowchart LR
    A[复现并保存最小输入] --> B[找到状态所有者]
    B --> C[列出创建/共享/销毁路径]
    C --> D[修改最小正确层]
    D --> E[组件 check 或单测]
    E --> F[rv + la 编译检查]
    F --> G[QEMU 定向测试]
    G --> H[压力测试并比较资源基线]
    H -->|失败| B
    H -->|通过| I[记录限制与结果]
```

“命令退出码为 0”不是资源测试的充分证据。涉及 fd、页、pipe、任务或 socket 时，必须至少
重复两轮并比较前后计数；首轮可能包含动态链接器、缓存或一次性初始化成本。

## 文档维护规则

- 新增长期状态时，在所属组件 README 的“核心状态与数据结构”补 owner、锁和销毁点。
- 新增跨组件调用时，在本手册的架构调用链或相应专题文档补链路。
- 新增 syscall 时，按 [添加系统调用](adding-a-syscall.md) 的清单逐项确认。
- 修改命令、feature 或默认值时，同步修改这里和 `os/README.md`。
- 文档只描述已实现行为；未实现项明确写“当前不支持”，不要写成计划已经落地。

提交前运行文档结构自检：

```sh
python3 scripts/maintenance/check_offline_docs.py
python3 scripts/maintenance/check_offline_docs.py --content-audit
```

它检查 12 份必需专题手册、所有 `components/**/Cargo.toml` 所在 crate 的本地 README、相对
Markdown 链接和代码围栏。第二条命令还保守检查每个 crate 文档是否覆盖调用流程、并发/生命
周期、失败边界和回归验证；它适合在离线文档补全阶段发现短占位 README。两种模式都不能证明
语义与源码一致；修改状态机、锁或生命周期后仍必须由开发者逐段对照实现。
