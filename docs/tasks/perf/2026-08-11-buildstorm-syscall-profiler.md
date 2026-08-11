# BuildStorm syscall 与参数画像工具方案

## 为什么选择这里

此前优化多由单个 hot symbol 推导。最新 another-ext4 direct-map 实验把 `memcmp` 从
1.519B 指令降至 0.376B，完整 BuildStorm 却没有稳定达到 1.5% 收益，说明局部指令下降
不足以证明端到端优化方向正确。

BuildStorm 是 `cargo`、`rustc`、linker 和大量短生命周期进程共同形成的混合负载。工具链
ELF、sysroot 与 registry 可能高频复用，源码和中间产物则可能接近一次性流量。只有先知道
syscall 类型、参数分布、路径复用和 I/O 粒度，才能判断应推进共享 file-backed page、
pathname/metadata 合并、异步块 I/O，还是进程与 futex 链路。这些方向都有产生两位数收益
的可能，优先级高于继续替换单个基础符号。

## 选择的方案

在 `os/scripts/syscall-profile/` 新增 QEMU TCG 插件和架构入口，支持三种采集后端：

- `backend=qemu`：使用 QEMU native syscall entry/return callback，适用于 linux-user；
- `backend=ecall`：在 system emulation 翻译到 RISC-V 用户地址的 `ecall` 时注册执行回调，
  从 `a7`、`a0..a5` 读取 Linux ABI syscall 编号和参数；
- `backend=auto`：根据 `qemu_info_t.system_emulation` 自动选择。WaterOS full-system 默认
  使用 `ecall`，Linux user-mode reference 默认使用 `qemu`。

第一版聚合以下信息，退出时一次写文件，不逐条打印：

1. syscall 总次数与 per-vCPU 次数；
2. 六个参数的 `0`、小整数、2 的幂级数量级分桶；
3. read/write/pread/pwrite 请求长度分桶；
4. mmap/munmap/mprotect 的长度、flags/prot 原值热点；
5. futex op、clone flags、open flags 等原值热点；
6. 已知 pathname syscall 的有界字符串、hash、出现次数、唯一/重复比例与近似复用距离；
7. native QEMU backend 下的返回值成功/errno 分布。

路径内容最多读取 512 字节，按 guest 页边界分段；读取失败只记计数。每个 vCPU 使用独立
计数与路径表，运行期不共享锁；退出时合并。路径复用距离是 per-vCPU 近似值，唯一/重复
次数在合并后精确计算。

## 为什么这样做

- 插件不改内核热路径，避免统计代码改变被测实现。
- 只在 `ecall` 执行时回调，不像 pc-hot 那样覆盖每条指令。
- Linux ABI 解码在两种后端共用，可直接比较 WaterOS 与 Linux reference 的调用形态。
- 参数与路径在 host 侧聚合，不通过慢速串口，也不占 guest 内存。
- 先证明工作负载结构，再选择共享 ELF/page、VFS、I/O 或调度的架构级优化。

## 实现步骤

1. 复用 `pc-hot` 的 build/run shell 结构，新增 `syscall-profile.c` 与 RV/LA wrapper；第一版
   先完成 RISC-V ecall 和 QEMU native 后端。
2. 添加 host 侧插件自检：参数解析、syscall 名称表、分桶、路径合并和输出格式。
3. 用短 WaterOS smoke 验证 `backend=qemu` 在 full-system 是否产生事件，并验证
   `backend=auto/ecall` 能捕获预期的 `write/exit/openat` 等调用。
4. 测量无插件与插件到同一 guest marker 的宿主开销；若超过 10%，增加采样或关闭完整
   参数读取，只保留计数。
5. 运行 120 秒和 300 秒 BuildStorm 画像，输出 syscall、参数与路径复用报告。
6. 若 Linux reference 可用，运行同一 workload 的 native callback 画像进行对照。

## 验收标准

- full-system WaterOS 能稳定采集 syscall 编号和六个参数；QEMU native 后端的触发情况有
  实测结论，不靠假设。
- 无逐条串口输出，QEMU 退出后生成完整、可解析的结果文件。
- 结果能回答至少三个决策问题：工具链路径复用率、主要 I/O 大小、主要 syscall 家族。
- 给出插件自身开销；画像数据不直接作为 BuildStorm 墙钟验收成绩。
- 基于画像选出下一项具有架构级收益潜力的优化，并在独立分支开始完整 A/B。

## 实现与验证结果

### 工具

- 新增 `os/scripts/syscall-profile/syscall-profile.c`，支持
  `backend=auto|qemu|ecall`、路径读取上限和 top-path 数量配置。
- 新增 RV/LA build/run wrapper；当前 `ecall` 后端明确只支持 RISC-V，LoongArch 可使用
  QEMU native backend，后续再补 `syscall 0` 寄存器后端。
- 新增 `analyze.py`，将 TSV 聚合结果转换为 syscall、路径复用、大小分布、flags 和热点
  路径 Markdown 报告。
- 插件以 `-O2 -Wall -Wextra` 构建通过；分析器合成结果测试通过。

### 双后端实测

同一 WaterOS main kernel、同一 Final 镜像、8 vCPU、`-snapshot`：

- 强制 `backend=qemu` 运行 40 秒：`total=0`。QEMU native syscall callback 在
  full-system 中不会观察由 WaterOS guest 内核处理的 syscall。
- 强制 `backend=ecall` 运行 60 秒：捕获 `127,900` 次 syscall；8 个 vCPU 的
  `register_failures` 全为 0，证明 `a7/a0..a5` 与 guest 虚拟内存读取有效。
- `backend=auto` 在 system emulation 正确选择 `ecall`。

### 300 秒 BuildStorm 画像

画像进入正式 `arceos-helloworld` 编译阶段，共捕获 `340,830` 次 syscall：

| syscall | 次数 | 占比 |
|---|---:|---:|
| `mprotect` | 100,154 | 29.39% |
| `statx` | 58,881 | 17.28% |
| `futex` | 32,858 | 9.64% |
| `read` | 25,360 | 7.44% |
| `openat` | 24,025 | 7.05% |
| `clock_gettime` | 17,352 | 5.09% |

路径复用并非接近零：`statx` 读取成功路径中 81.99% 为重复，`openat` 为 69.90%，
`readlinkat` 为 92.14%，`execve` 为 62.23%。热点包含 Rust sysroot、Cargo registry、
`/work/tgoskits` 层级、libc 和 target JSON，证明工具链/元数据确实被反复访问。

最强的新信号是 `mprotect`：85,544 次长度位于 4 KiB 桶，98,643 次目标权限为
`PROT_READ|PROT_WRITE`；同时有 302 次约 128 MiB 匿名 mmap。该形态符合 allocator
“大范围保留、逐页 commit”的模式。WaterOS private anonymous mmap 已是 lazy VMA，但
mprotect 即使只修改未驻留 lazy VMA，当前仍执行实现层 fence，并经
`with_user_aspace_mut_and_flush` 再做本地全 flush 与远端 shootdown。

### 开销与限制

- 画像窗口只用于诊断，不纳入墙钟成绩。
- 300 秒画像比此前 300 秒 pc-hot 样本推进得更深，未观察到明显超过 10% 的插件开销；
  但宿主已证明存在约 20 秒级漂移，因此不能给出精确开销百分比。
- `ecall` 后端目前没有 syscall return callback，read/write 记录的是请求长度而非实际返回
  长度；阻塞时间也不能仅由入口插件可靠配对。

## 下一步决策

优先验证“未驻留 lazy VMA 的 mprotect 不做 TLB flush/shootdown”以及已驻留页的精确
flush summary。该方向覆盖约 10 万次 syscall，属于架构级同步削减；历史 MM-02A 的旧
结果受当前已确认的宿主漂移影响，应重新标为存疑而非永久否决。之后再根据路径复用数据
设计工具链/sysroot 热点保护和 lookup/metadata 合并。
