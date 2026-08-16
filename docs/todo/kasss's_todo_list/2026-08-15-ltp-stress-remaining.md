# 内核待完善清单（LTP + stress-ng）

> 日期：2026-08-15。当前主要验证环境为 RISC-V final、SMP=8、
> stress-ng 0.19.02；RISC-V/LoongArch final `cargo check` 均通过。
> 本文只把“有客观证据的问题”列为缺陷；不能由当前内核真实提供的数据，
> 不以固定零值或伪造节点冒充 Linux 能力。

---

## 一、当前已验证基线

### 1. procfs / sysfs

- `/proc` 已提供系统级 CPU、内存、负载、mount、网络、pressure、SysV IPC、
  `sysctl` 只读视图，以及 PID 的 `stat/status/statm/maps/smaps/limits/mountinfo`
  等常用节点。
- `/proc/<pid>/fd`、`fdinfo`、`task`、`ns` 可枚举；namespace fd 支持
  `NS_GET_USERNS`、`NS_GET_PARENT`、`NS_GET_NSTYPE`、`NS_GET_OWNER_UID`。
- `/proc/<pid>/task/<tid>` 提供线程自己的 `comm/stat/status/wchan`；线程级 stat
  使用目标 TID、状态、tick、CPU、调度策略和 affinity，不复制 leader 数据。
- `/sys` 已挂载并提供 CPU/NUMA、网络接口、virtio 块设备的只读视图。
- guest 已验证：`ps`、`pgrep`、`lscpu`、`lsblk`、`lsns` 均可正常运行。
- `/proc/cgroups` 只输出表头，`/proc/<pid>/cgroup` 为空：WaterOS 尚未实现
  cgroup 层级，因此不再虚报 memory/cpu 等控制器。
- `/proc/<pid>/mounts` 会校验目标 PID；不存在 PID 的路径正确返回不存在。

### 2. stress-ng

- 2026-08-15 最新三轮连续验证：
  `stress-ng --procfs 2 --sysfs 2 --timeout 15s --verify --metrics-brief`，
  每轮 `passed: 4, failed: 0`，最终状态 0。
- 随后提高到 `--procfs 4 --sysfs 4 --timeout 30s` 连续三轮，均为
  `passed: 8, failed: 0`；压测前后可见进程数均为 4。
- fork/clone/vfork/forkheavy/mmapfork 混合 30 秒验证通过 8 个 worker、失败 0，
  压测前后可见进程数均为 4；exec stressor 因 stress-ng 拒绝 root 身份而跳过。
- 更早的 CPU、fork、VM、malloc、pipe、signal、futex、SysV msg/sem/shm
  组合压力均有通过记录。
- 修复跨 CPU COW 后，`--sysfs` 不再因本 CPU 保留只读 TLB 项而误触
  `StorePageFault`；3 轮单独 sysfs 和 1 轮 4+4 proc/sys 组合均已通过。

### 3. 已解决的高优先级问题

- ext4 空路径 remove/rename 不再下溢 panic，返回 `ENOENT`。
- `renameat2(old == new, RENAME_NOREPLACE)` 返回 `EEXIST`。
- heap brk 与 mmap 使用分离地址区间，不再互相侵占。
- PIE 可执行文件装载到非零基址，保留 null page。
- capability 目标按 Linux PID/TID 正确解析。
- SysV SHM 标记删除、attach/detach、fork/exit 生命周期和 `/proc/sysvipc/shm`
  已补齐。
- SysV 控制命令采用 Linux ABI：MSG_STAT=11、MSG_INFO=12、
  MSG_STAT_ANY=13；SEM_STAT=18、SEM_INFO=19、SEM_STAT_ANY=20。
- 任务退出时 credential 保留到退出收尾完成；连续 proc/sys 压测不再出现
  `[cred] no cred for tid=... (current)` panic。

---

## 二、仍需完善的问题（按现场价值排序）

### 🔴 P0：资源耗尽与退出回收

#### P0-1 内核堆高水位 / fork-heavy 长时稳定性

- 历史上 `forkheavy` 曾把 256 MiB 内核堆推到 OOM；当前堆已扩大到
  512 MiB，但扩容只能降低触发概率，不能证明没有泄漏。
- 需要在 final 8 GiB 配置下持续运行 fork/mmap/reap 压力，并采集每轮结束后的
  heap、frame、task、fd、address-space 数量；稳定回落才算解决。
- QEMU 退出 247 不能一律归因于 guest：至少有一次由宿主 WSL OOM killer
  终止。报告必须同时保留 guest 串口尾部和宿主退出原因。

#### P0-2 多线程退出资源生命周期

- 本轮已修复 credential 过早删除，但 fd、cwd、mount namespace、signal、robust
  futex、SHM attachment 的退出与 reap 仍分散在多条路径。
- 需要覆盖 `exit`、`exit_group`、exec 清除 sibling、fork/clone 回滚、父进程 wait、
  operator 强制清理，验证所有侧表最终归零且每项只产生一次有语义副作用的清理。

### 🔴 P0：内存管理

#### P0-3 MAP_GROWSDOWN

- `mmap18` 的 grow-stack 场景仍可能因未完整实现 `MAP_GROWSDOWN` 而 SIGSEGV。
- 需要给 VMA 保存 grow-down 属性，在合法 guard 范围内由 page fault 向下扩展，
  同时受 `RLIMIT_STACK`、相邻 VMA 和地址下限约束。

#### P0-4 `/proc/<pid>/maps` 仍是近似视图

- 当前 maps/smaps 使用主 ELF 聚合范围和用户栈快照，足以支持现场工具和基本
  stress，但还不是逐 VMA、逐 ELF segment、文件 offset 精确输出。
- 应从 MM 导出只读 VMA snapshot，包含起止、权限、共享/私有、offset、设备号、
  inode、path，并让 procfs 只负责格式化。

### 🟠 P1：procfs / sysfs 真实性与覆盖面

- `/proc/<pid>/environ`、`auxv`、`io` 尚未实现；它们需要 process/exec/IO
  侧真实快照，不应先添加全零占位。
- 线程级 proc 仍未提供 `sched`、`stack`、`syscall` 等调试节点；这些需要
  scheduler/trap 导出可靠快照后再补。
- `/proc/status` 的 Vm/RSS 当前为 image + 初始 stack 的估计，不是驻留页精确记账。
- `/proc/interrupts` 尚无真实 per-CPU IPI 计数来源。
- sysfs 尚缺 `/sys/class/block/vda`、网络 statistics、设备/驱动层级；只有内核
  能提供真实统计后再发布计数器。
- `/proc/sys/kernel/random/uuid` 若仍为固定字符串，会违反每次读取生成新 UUID 的
  语义，需要接入已有随机源。

### 🟠 P1：信号、futex 与时间

- 历史 LTP 记录包含 `sigwait` 的 `kill(...)=EINVAL` 和 futex requeue 场景中
  `waitpid=EINTR`；当前镜像未包含对应 helper，尚未用最新内核复测，不能直接标为
  已修或仍失败。
- 10 ms scheduler tick 会让 `clock_nanosleep`/futex timeout 有可见向上取整；若线下
  测试要求更高精度，应改为 deadline timer，而不是简单缩短所有 CPU 的固定 tick。

### 🟡 P2：功能型缺口

- process accounting 记录仍不完整，`acct02` 可能得到空记录。
- setuid/setgid 可执行文件、完整 capability 集合与 securebits 仍是简化模型。
- cgroup、完整 device model、热插拔和动态 sysfs 写接口尚未实现；当前只读视图
  会对不支持的节点明确返回 `ENOENT`/只读错误。

---

## 三、下一轮验证顺序

1. fork/clone/exec/exit_group 长时压力后对比 heap/frame/task/fd 计数，定位是否泄漏。
2. 注入包含最新 LTP helper 的镜像，复测 signal、futex、MAP_GROWSDOWN 和 acct。
3. 为 MM 增加 VMA snapshot，再精确化 maps/smaps。
4. 只在底层存在真实数据源后扩充 `/proc/<pid>/io`、网络 statistics 和 interrupts。

---

## 四、环境注意事项

- `SNAPSHOT=1` 只把磁盘写入放在 QEMU 临时 overlay 中；重启后 guest 安装的软件
  会消失。需要持久化时显式使用写盘模式，并保留基础镜像备份。
- LTP helper 缺失导致的 `ENOENT` 属镜像问题，不应通过伪造 syscall 成功掩盖。
- glibc 可能在用户态拦截部分 SysV 命令；判断内核能力时同时使用 raw syscall
  或 musl 测试，确认请求是否真正进入内核。
