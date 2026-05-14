# test_case 全通过路线图

**事实来源**：`test_case/README.md`、`test_case/sdcard/**/**_testcode.sh`、`test_case/scripts/**/_testcode.sh`、`docs/roadmap/todolist.md`、`docs/architecture/snapshot.md`、`docs/exports/features/` 与 `docs/exports/release-overview/current.md`、`docs/prompts/`（规划类任务请配合 `general.md`、`structure.md`、`architecture.md`）。

**范围说明**：「全通过」指赛题磁盘镜像中的各组 `*_testcode.sh` 在 **RISC-V 与 LoongArch**、**glibc 与 musl** 变体下均能按评测要求跑完并得到预期输出。工作量极大，下文按**依赖顺序**拆分阶段，便于迭代验证。

---

## 零、基于导出文档的当前进度（与源码细读无关的共识）

以下归纳自 **`docs/architecture/snapshot.md`** 与各组件 **`docs/exports/features/*.md`**，用于把「全通过路线图」锚定到仓库**已有进度**，避免从零假设。

### 已具备或较成熟的底座（RISC-V 主线为主）

- **启动与平台**：QEMU riscv64 + OpenSBI 下 `kernel_main` 流程完整；控制台、日志、堆、panic；定时器与中断已接入调度主线（见架构快照）。
- **驱动**：DTB 扫描、**virtio-mmio 块设备**注册、与 **devfs** 刷新协作；**virtio-net 仅识别类 device_id，无驱动与栈**（`wateros-driver` 功能快照）。
- **文件系统**：**ext4** RO + **RW（ext4plus beta）**；devfs **`/dev/vblkN`**；根卷挂载与启动期 **`fs::test`** 树遍历/自检（`wateros-fs` 功能快照）。
- **VFS**：**`vfs-impl-fs-bridge`** 烟囱能力（单根 RO 委托、RW 会话、与 `fs` 自检对齐）；**尚未**与 per-task fd、syscall 打通（`wateros-vfs` 功能快照）。
- **内存管理**：**Sv39**、内核 ELF 装载、全局内核页表、**栈式物理帧分配器**；**`UserMemoryOps` / `mmap` / `brk` 在用户态路径上仍属未完整落地**（`wateros-mm` 功能快照）。
- **任务与调度**：轮转、**阻塞/睡眠队列**、zombie 与回收、`WaitQueue`、trap 帧与任务对象协作；**`spawn_user_task` 骨架已存在**，用户态主线与完整恢复模型仍在推进（`wateros-task` 功能快照）。
- **系统调用**：**仅** `yield` / `exit` / `exit_group` / **`write` 仅 fd 1、2 走控制台** / **`brk` 桩**；其余 **`ENOSYS`**（`wateros-syscall` 功能快照）。
- **IPC**：聚合层默认 **dummy** + **`waitqueue` 薄封装**；**pipe/signal/shm/futex 等子 crate 未进入默认依赖图**（`wateros-ipc` 功能快照）。

### LoongArch 与评测脚手架

- **LoongArch64**：UART、trap、timer、**演示性 kernel task 轮转**；架构快照明确：**未接入真实 MM、driver、fs/vfs 与用户态地址空间**。
- **赛题对齐**：多 virtio-blk、**virtio-net**、RTC、根目录串行跑 `*_testcode.sh`、**关机** 等仍属路线图基础设施（见下文第一节），与当前「内核自检 + 演示任务」主线不同。

### 规划含义（在已有代码上的「下一步」）

你们**不是空仓库**：**块设备 + ext4 + VFS 桥 + MM 契约 + 任务/调度骨架**已减轻后续工作量；当前瓶颈集中在 **（1）syscall ↔ fd ↔ vfs/fs 闭环**、**（2）用户地址空间与 mmap/brk 真语义**、**（3）fork/exec/wait 与 IPC 最小集**、**（4）virtio-net 与协议栈**、**（5）LoongArch 能力对齐 RISC-V**。下文阶段划分在保持赛题顺序的前提下，应优先把 **RISC-V 上「第一个磁盘 ELF + basic 子集」** 做成可回归里程碑，再并行推进 LoongArch 分页/驱动。

---

## 一、赛题侧硬性要求（基础设施）

来自 `test_case/README.md` 的评测约定，与具体测例脚本无关但必须先满足：

1. **产物**：项目根 `Makefile` 的 `all` 能构建 **`kernel-rv`**、**`kernel-la`**（ELF）；可选 **`disk.img`**。
2. **QEMU 环境**：`virtio-blk` 挂载含测试点的 **EXT4 无分区表** 磁盘；评测命令还包含 **`virtio-net`** 与 **RTC**；可选第二块盘 `disk.img`。
3. **自举测例**：内核启动后需能发现并**串行**执行各 `*_testcode.sh`，输出形如 `#### OS COMP TEST GROUP START … ####` / `END` 的标记。
4. **收尾**：全部测试点后主动**关机/退出 QEMU**。

**对应内核工作**：多 `virtio-blk` 实例、块设备与 EXT4 用户态可见路径、**virtio-net + 用户态协议栈或兼容层**、时钟源与 wall-clock、进程内执行脚本（或等价解释器）、**poweroff/reboot** 路径。当前仓库 `os/scripts/test_in_qemu_riscv.sh` 仍缺网卡与第二磁盘，与赛题完整命令不一致，需对齐。

---

## 二、测例分组与能力依赖（12 组）

| 组别 | 典型入口脚本 | 主要依赖 |
|------|----------------|----------|
| **basic** | `basic_testcode.sh` → `basic/run-all.sh` | `brk/chdir/clone/close/dup/dup2/execve/exit/fork/fstat/getcwd/getdents/getpid/getppid/gettimeofday/mkdir/mmap/mount/munmap/open/openat/pipe/read/sleep/times/umount/uname/unlink/wait/waitpid/write/yield` 等 POSIX 子集；可执行文件加载；根文件系统挂载语义 |
| **busybox** | `busybox_testcode.sh` + `busybox_cmd.txt` | 上述 syscall 子集 + **ash/sh**、大量文件与管道命令、**后台作业 `&`、kill、sleep、进程表** |
| **lua** | `lua_testcode.sh` | `busybox` + 解释执行多个 `.lua`；动态链接与文件 IO |
| **libctest** | `libctest_testcode.sh` | **静态与动态**链接测例、`dlopen` 相关 so（`sdcard/.../lib/`） |
| **iozone** | `iozone_testcode.sh` | 多线程/多进程 IO（`-t 4`）、多种读写模式、`pread/pwrite/preadv/pwritev`、**fsync**、临时文件 |
| **unixbench** | `unixbench_testcode.sh` | 多进程/管道/算术等综合负载（与调度、pipe、fork 等相关） |
| **lmbench** | `lmbench_testcode.sh` | **null/read/write/stat/fstat/open** 延迟、`select`、**signal 安装/捕获/保护**、**pipe**、**fork/exec/shell**、**mmap/page fault**、**上下文切换**、目录 `/var/tmp`、`/tmp` |
| **iperf** | `iperf_testcode.sh` | **TCP/UDP**、本机 `127.0.0.1`、多流、`-R` 反向；后台 **iperf3 server** |
| **netperf** | `netperf_testcode.sh` | **netserver** 后台 + TCP/UDP STREAM/RR/CRR |
| **libcbench** | `libcbench_testcode.sh` | libc 密集场景（与 VDSO、锁、内存分配等相关，依具体脚本与二进制） |
| **cyclictest** | `cyclictest_testcode.sh` | **高精度定时器/时钟**、`pthread`、**SCHED_FIFO 等**、`cyclictest` + **hackbench** 负载、**SIGINT（kill -2）** |
| **LTP** | `ltp_testcode.sh` | 遍历 `ltp/testcases/bin` 下全部用例，**POSIX 覆盖面最大**，应置最后 |

`sdcard` 下同时存在 **riscv/loongarch × glibc/musl** 四套用户态二进制，内核需在两条架构上达到相近的 Linux 兼容度。

---

## 三、与当前内核的差距（摘要）

在 **第二节零** 所述「已有底座」之上，与 **basic～LTP** 测例之间的主要缺口仍是：

- **系统调用面**：与 fd、文件、进程、IPC、网络相关的绝大多数号码仍为 **`ENOSYS`**（导出文档与 `wateros-syscall` 快照一致）。
- **纵向打通**：**`wateros-vfs` 桥接层**尚未接到 per-task fd 与 syscall；**`wateros-mm`** 的用户 `mmap`/`brk` 语义未与 syscall/task 联调闭环；**`wateros-ipc`** 的 pipe/signal 未进默认构建。
- **驱动与平台**：**virtio-net**、DTB 对齐的计时频率、**IrqLine 端到端** 等仍在驱动/平台快照的「后续关注点」中。
- **LoongArch**：与 RISC-V 主线差距大，需单独里程碑，避免与「赛题全通过」混为单线排期。

**结论**：全通过仍是 **完整用户态 OS 能力栈** 的长期建设；但你们已越过「只有骨架」阶段，**应优先利用已有 fs/vfs-bridge/mm 契约/task 用户骨架做纵向切片**，而不是平行铺开所有 syscall。

---

## 四、推荐实施顺序（分阶段，已按当前仓库进度收紧）

阶段划分原则：**先纵向打通「用户任务 + fd + vfs/fs + 最小 open/read/write/close + exit」**，再扩展 **basic 全表**，再 busybox/lua，再 benchmark/网络，最后 LTP；LoongArch 在 RISC-V 用户路径稳定后按平台组件并行。

1. **阶段 A — 评测脚手架对齐**  
   Makefile 产物；QEMU 参数与赛题一致（含 **virtio-net**、RTC、可选第二盘）；内核能枚举测试脚本并输出 **START/END** 标记；**关机**。

2. **阶段 B0 — 用户态纵向切片（利用现有桥，不必等「完整 VFS」）**  
   在 **`spawn_user_task` 骨架**上跑通 **从 ext4 加载的一个静态 ELF**；建立 **per-task fd 表**；`open/read/write/close`、`exit`（及必要 `write` 到文件）走 **`vfs`↔`fs` 已有桥与 ext4**；替换或收紧当前 **`brk` 桩**与 **`wateros-mm`** 用户语义的第一版对齐。  
   **验收**：不依赖赛题 shell，即可在 QEMU 下用户程序读写根目录文件并退出。

3. **阶段 B1 — basic 闭环**  
   按 `basic/run-all.sh` 补齐其余 syscall（`fork/execve/wait*`、`pipe`、`mmap/munmap`、`getdents`、`mount/umount`、`chdir/getcwd`、`unlink/mkdir`、`times/gettimeofday/sleep`、`uname/fstat`、`clone` 等），与 **`wateros-abi`** 号表及 trap 参数约定一致。

4. **阶段 C — busybox + lua**  
   **dup/dup2**、作业与 **`kill`** 所需的最小信号/进程组语义；`busybox_cmd.txt` 覆盖的命令路径。

5. **阶段 D — libctest（静态/动态）**  
   动态链接器、`mmap` 可执行映射、**TLS**、与 `sdcard/.../lib/` 协同。

6. **阶段 E — lmbench / unixbench / libcbench**  
   `select`/`poll`、**signal** 完整化、pipe 带宽、抢占/多核（若赛题要求）。

7. **阶段 F — iozone**  
   多线程/多进程 IO、`preadv/pwritev`、**fsync**、临时目录语义。

8. **阶段 G — 网络（iperf、netperf）**  
   **virtio-net** + 协议栈（loopback + TCP/UDP）+ 后台 server 进程模型。

9. **阶段 H — cyclictest**  
   高精度计时、实时调度、**SIGINT**、hackbench 联调。

10. **阶段 I — LTP**  
    分桶收敛；**riscv/loongarch × glibc/musl** 交叉验证置于此后的持续集成策略中。

**并行建议**：LoongArch 侧优先 **分页 facade 真实化 + 块设备/fs 最小挂载**，再复用 RISC-V 上已验证的 syscall/VFS 分层；业务层避免硬编码 Sv39 细节（见 `docs/prompts/architecture.md`）。

---

## 五、Markdown 勾选清单（维护用）

- [ ] 阶段 A：Makefile、`kernel-rv`/`kernel-la`、QEMU 赛题参数、测试脚本调度、START/END 输出、关机  
- [ ] 阶段 B0：用户任务 + fd 表 + `open/read/write/close` 走 vfs↔fs 桥与 ext4；首用户 ELF；brk/mmap 与 mm 第一版对齐  
- [ ] 阶段 B1：`basic/run-all.sh` 全 syscall + 进程/fork/exec/wait/pipe/mount 等  
- [ ] 阶段 C：busybox + lua  
- [ ] 阶段 D：libctest 静态/动态 + TLS/dlopen  
- [ ] 阶段 E：lmbench、unixbench、libcbench  
- [ ] 阶段 F：iozone  
- [ ] 阶段 G：iperf、netperf（含 loopback 与后台服务）  
- [ ] 阶段 H：cyclictest（含 hackbench、SIGINT）  
- [ ] 阶段 I：LTP 全量遍历与分桶修复  
- [ ] 交叉验证：riscv/loongarch × glibc/musl 四套 sdcard 镜像抽样与 CI 策略  

---

## 六、与 `docs/prompts` 的协作方式

- 编码与 feature 切换：遵循 `structure.md` 的同步文件列表与 `architecture.md` 的 API/impl 分层。  
- 扩展 syscall 与 ABI：对齐 `wateros-abi` 与 `docs/exports/`，并回写 `docs/roadmap/todolist.md` / `docs/architecture/snapshot.md`。  
- 大规模规划或冲突策略：可先走 `general.md` 中的规划类交付结构（目标、依赖、顺序、风险、同步文档）。

本文档应随 `test_case` 或内核能力变更**增量修订**，避免与 `todolist.md` 长期矛盾。
