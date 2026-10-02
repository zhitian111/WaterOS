# WaterOS 脚本工具

[项目首页](../../README.md) · [内核工程](../README.md) · [工具文档](../../docs/tools/README.md)

`os/scripts/` 保存 WaterOS 的构建配置、QEMU 运行、调试、测试和维护工具。日常构建与
运行应优先使用 [`../Makefile`](../Makefile) 提供的统一目标；直接运行脚本主要用于专项
测试、性能分析或维护底层工具。

除特别说明外，下文命令均从 `os/` 目录执行。

## 目录结构

```text
scripts/
├── analysis/          # ELF 依赖与 Linux syscall 静态审计
├── competition/       # 比赛平台认证与提交环境辅助工具
├── config/            # Cargo feature 树的导出、转换和应用
├── debug/             # QEMU/GDB 调试、停滞检测与符号解析
├── gdb/               # 在 GDB 内加载的 WaterOS 扩展
├── la2k/              # 2K1000 整盘镜像分片与 TFTP 烧录准备
├── maintenance/       # 清理、统计、导出和仓库维护
├── pc-hot/            # 基于 QEMU TCG plugin 的 PC 与等待热点分析
├── real-hardware/     # 物理板块设备镜像写入
├── run/               # 统一 QEMU 启动器、兼容入口与并行运行
├── root_image/        # 物理板内核 uImage 与启动模板（rootfs 整盘镜像见 user/tools）
├── setup/             # Rust、链接工具链和官方测试环境初始化
├── source/            # Shell 与 Python 脚本共用模块
├── syscall-profile/   # 系统调用频次和开销画像
├── testing/           # 功能、性能、LTP 与 guest 侧专项测试
└── tests/             # 脚本自身的 Python 单元测试
```

## 推荐入口

| 场景 | 命令 |
|:--|:--|
| 查看有效配置 | `make show-config ARCH=rv PROFILE=pre` |
| 构建内核 | `make build ARCH=rv PROFILE=pre` |
| 启动内核 | `make run ARCH=rv PROFILE=pre` |
| 进入交互终端 | `make shell ARCH=la PROFILE=pre` |
| 静态检查 | `make check ARCH=rv PROFILE=final` |
| 检查调试环境 | `make doctor` |
| 自动调试与停滞监测 | `make debug ARCH=rv PROFILE=final` |
| 两终端 GDB 调试 | `make debug-server ...`，另一终端执行 `make gdb` |

Makefile 会统一校验 `ARCH`、`PROFILE`、`SMP`、`MODE`、镜像和调试参数。完整参数说明见
仓库根目录的 [`README.md`](../../README.md#构建配置)。

## 参数查询

面向开发者直接调用的脚本均应支持 `-h` 或 `--help`。Python 子命令还可以继续查询下一级
帮助，例如：

```bash
python3 ./scripts/debug/wateros_debug.py --help
python3 ./scripts/debug/wateros_debug.py run --help
./scripts/config/config-to-features.bash --help
./scripts/pc-hot/pc-hot-rv.sh --help
```

参数分为三类：位置参数决定输入文件或操作对象，选项控制本次行为，环境变量负责向
Makefile 调用的底层脚本传递运行环境。常用直接入口如下：

| 脚本 | 必需参数 | 可选参数或主要环境变量 |
|:--|:--|:--|
| `run/qemu_run.py` | `--arch {rv,la}`、`--profile {pre,final}` | `WOS_SDCARD`、`WOS_KERNEL`、`WOS_SMP`、`WOS_QEMU_MEM`、GDB 与图形变量 |
| `run/run_qemu_parallel.sh` | 至少一条完整命令 | `WOS_CORES_PER_JOB`、`WOS_MAX_PARALLEL_JOBS`、日志目录与工作目录变量 |
| `debug/wateros_debug.py` | 子命令；`run` 和 `server` 还需要 profile | SMP、连接地址、端口、采样间隔、确认次数和超时参数 |
| `config/config-to-features.bash` | 无 | 配置文件和根 package，均可使用默认值 |
| `config/features-conf-to-cargo.bash` | 配置文件、package | `WATEROS_SCRIPTS_QUIET=1` 可关闭操作日志 |
| `pc-hot/pc-hot-{rv,la}.sh` | `build`、`run`、`analyze` 或 `all` | 输出文件、ELF、Top N、icount shift 和完整 QEMU 命令 |
| `pc-hot/wait-hot-{rv,la}.sh` | `build` 或 `run` | 输出文件和完整 QEMU 命令 |
| `syscall-profile/syscall-profile-{rv,la}.sh` | `build` 或 `run` | 输出文件、plugin `key=value` 选项和完整 QEMU 命令 |
| `analysis/elf_syscalls.py` | ELF 可执行文件或动态库 | rootfs、动态库搜索目录、文本/JSON 输出和严格模式 |
| `testing/operator_smoke.py` | `--arch {rv,la}` | profile、SMP、模式、Guest 脚本、超时和日志路径 |
| `testing/ltp_prune_sdcard_before.sh` | 无 | 镜像、起始用例、libc、dry-run 和重置源镜像 |
| `user/tools/root_image.py` | `build` 或 `verify` | `--output`、`--manifest`、`--copy-tree`、`--size-mib`、`--partition-table {mbr,gpt}`、`--source-root`、`--extra-image`/`--extra-partition-type`（P2 起追加无分区文件系统）、`--root-size-mib`、`--boot-dir`/`--boot-size-mib`（VisionFive 2：P3 FAT boot + P4 rootfs） |
| `real-hardware/dd_image.sh` | `<image> <device>` | 交互确认（输入 `y`）；防呆：整盘/非系统盘/未挂载/容量校验；由 `make dd_img_vf2`、`make dd_img_2k1000` 包装 |
| `la2k/prepare_tftp.sh` | `--image IMAGE` | 分片 2K1000 GPT 镜像、生成 U-Boot SATA 烧录脚本；默认以前台 `dnsmasq` 提供 TFTP，`--prepare-only` 仅准备文件；由 `make la2k_tftp{,_prepare}` 包装 |

表格只用于入口导航，脚本的 `--help` 是参数名称、默认值和副作用的权威说明。新增或修改
参数时必须同时更新帮助文本；供 Makefile 调用的兼容包装脚本可以将帮助直接转发给实际
实现。

## `run/`：运行与 QEMU 编排

| 脚本 | 作用 |
|:--|:--|
| `qemu_run.py` | RISC-V64 与 LoongArch64 QEMU 参数的唯一组装实现，由 `make run` 调用 |
| `qemu_exec_with_taskset.sh` | 执行 QEMU；设置 `WOS_TASKSET_CPUS` 时绑定宿主 CPU |
| `run_qemu_parallel.sh` | 按宿主 CPU 预算并发运行多条独立 QEMU 命令并保存日志 |
| `{rv,la}_{pre,final}_run.sh` | 固定架构与阶段的兼容入口，最终调用 `qemu_run.py` |
| `{rv,la}_qemu_run_snapshot.sh` | 为历史性能流程创建临时 qcow2 overlay 后启动单核 QEMU |
| `rv_qemu_run.sh` | 可指定 OpenSBI 固件的旧版 RISC-V SMP 启动入口 |
| `rv_qemu_run_with_log.sh` | 生成高容量 `qemu.log` 的短窗口诊断入口 |

直接调用 `qemu_run.py` 时，必须提供架构和阶段；镜像等参数通过 `WOS_*` 环境变量传入：

```bash
WOS_SDCARD=./sdcard-rv.img WOS_SMP=4 \
  python3 ./scripts/run/qemu_run.py --arch rv --profile pre
```

并行运行器把每个带引号的参数视为一条完整命令：

```bash
WOS_CORES_PER_JOB=4 WOS_AUTO_SMP=1 \
  ./scripts/run/run_qemu_parallel.sh \
  "make run ARCH=rv PROFILE=final SDCARD=/tmp/rv-a.img" \
  "make run ARCH=la PROFILE=final SDCARD=/tmp/la-a.img"
```

## `config/`：Feature 配置

| 脚本 | 作用 | 是否写文件 |
|:--|:--|:--|
| `configure.bash` | 生成 `config.conf` 与 `feature-tree.txt` | 是 |
| `export-feature-tree.bash` | 扫描 Cargo 清单并导出完整 feature 树 | 是 |
| `print-config.bash` | 按 crate 打印当前配置 | 否 |
| `config-to-features.bash` | 将配置树转换为顶层 Cargo feature 字符串 | 否 |
| `config-to-features-make.bash` | 面向 Make/编辑器的安静输出适配层 | 否 |
| `features-conf-to-cargo.bash` | 提取单个 package 的直接 feature 选择 | 否 |
| `rust-analyzer-apply-config.bash` | 将选择写入 `.cursor/settings.json` | 是 |
| `apply-config-as-default-features.bash` | 备份并改写各 Cargo.toml 默认 features，或恢复备份 | **是** |

常规构建不依赖 `apply-config-as-default-features.bash`。需要检查配置树时运行：

```bash
make configure
./scripts/config/print-config.bash
```

## `debug/` 与 `gdb/`：调试

`debug/wateros_debug.py` 是统一调试入口，提供 `doctor`、`run`、`server`、`snapshot`、
`watch` 和 `gdb` 子命令。Makefile 已封装常用参数，优先使用 `make debug` 等目标。

其余文件按职责拆分：

- `debug_abi.py`：解析内核导出的稳定诊断 ABI；
- `gdb_remote_snapshot.py`：最小 GDB Remote 协议客户端；
- `loop_detector.py`：识别重复 PC 模式；
- `pc_trace_parser.py`、`pc_trace_watch.py`：解析并观察 QEMU PC trace；
- `qemu_launcher.py`：为 PC trace 调试组装 QEMU 参数；
- `symbol_index.py`、`resolve_pc_symbol.py`：ELF 符号与源码位置解析；
- `gdb/wateros.py`：在 GDB 会话中注册 WaterOS 命令。

```bash
make doctor
make debug ARCH=rv PROFILE=final
make rv_symbol_at ADDR=0x80200000
```

## `testing/`：功能与性能测试

| 脚本 | 场景 | 状态影响 |
|:--|:--|:--|
| `operator_smoke.py` | 驱动串口 operator shell 完成冒烟测试 | 启动 QEMU |
| `parse_qemu_test_log.py` | 汇总 bring-up 日志中的测试组结果 | 只读 |
| `run_phase_tests.sh` | 分 P1 至 P6 运行 RISC-V 测试 | 临时改写并恢复 bring-up 源码 |
| `run_perf_bringup_phases{,_la}.sh` | 分功能、benchmark、LTP 三组运行性能负载 | 临时改源码并创建 overlay |
| `run_iozone_minimal.sh` | 只运行 glibc iozone | 临时改写并恢复 bring-up 源码 |
| `min_accept_execve_lazy.sh` | 双架构 execve lazy-map 最小验收 | 临时改写并恢复 bring-up 源码 |
| `ltp_hang_iterate.sh` | 自动定位 LTP 卡死并迭代 skip/checkpoint | **修改源码和镜像** |
| `ltp_prune_sdcard_before.sh` | 用 debugfs 裁剪指定用例之前的 LTP 文件 | **修改目标镜像** |
| `ltp_sum_passed.py` | 汇总 LTP Summary 中的 passed 数量 | 只读 |
| `guest_buildstorm_parallel_probe.sh` | guest 内构造无网络 Cargo 并发负载 | 修改 guest `/tmp` |
| `guest_cgroup_capability_regression.sh` | guest 内确认未实现的 cgroup 控制器不会被误报 | guest 内执行测试 |
| `guest_read_family_regression.sh` | guest 内运行 read/pipe/socket 等 LTP 用例 | guest 内执行测试 |
| `regress_ext4_dir_tail.sh`（scripts 根目录） | QEMU 内验证 ext4 目录块 tail 边界；可选 apt/dpkg 模式 | 创建 overlay、注入 guest 脚本 |

这类脚本通常绑定具体性能任务和镜像布局。运行前应阅读文件头与参数解析部分，并确保
Git 工作区可恢复。会写镜像的脚本只能针对副本或可丢弃 overlay 使用。

## 性能分析

- [`pc-hot/`](./pc-hot/)：以 QEMU TCG plugin 统计 guest PC、符号和等待时间，使用方法见
  [`docs/tools/pc-hot.md`](../../docs/tools/pc-hot.md)。
- [`syscall-profile/`](./syscall-profile/)：采集 syscall 画像并生成 Markdown 报告，详见
  [`syscall-profile/README.md`](./syscall-profile/README.md)。

性能测量应固定 QEMU 版本、镜像、SMP、宿主 CPU 绑定与本地 baseline。诊断 feature 和
QEMU trace 会改变热路径开销，不能与正式成绩直接比较。

## `analysis/`：软件移植审计

`analysis/elf_syscalls.py` 接受一个 ELF 可执行文件或动态库，递归解析 interpreter、
`DT_NEEDED`、`RPATH`/`RUNPATH` 和 `$ORIGIN`，汇总反汇编 syscall 指令及动态 wrapper
符号给出的 Linux syscall 候选。分析目标 rootfs 中的软件时必须用 `--root` 指定其根目录：

```bash
./scripts/analysis/elf_syscalls.py \
  --root ../user/build/staging/rv/rootfs \
  ../user/build/staging/rv/rootfs/bin/busybox
```

该工具只读 ELF 和 rootfs，不执行目标程序。结果是静态上界，不替代实际 workload 的
syscall trace；JSON 输出、严格模式和分析边界见
[`docs/tools/elf-syscalls.md`](../../docs/tools/elf-syscalls.md)。

## `setup/`、`maintenance/` 与 `competition/`

`setup/` 中的安装脚本具有平台假设，其中部分脚本仅适用于 Debian/Ubuntu，并会调用
`sudo apt`、访问网络或启动 Docker。Arch Linux 等环境应按 README 的依赖清单手动准备。

`maintenance/` 包含 Cargo workspace 清理、项目统计、比赛仓库导出和历史 Git 辅助
脚本。其中 `update*.sh` 会执行 `git add --all`，不建议在存在未确认改动时使用；
`export-to-gitlab.bash` 的目标目录也是本机固定路径，执行前必须核对源码。

`competition/educg_update_cookie.py` 用于更新比赛服务器上的会话 cookie。配置文件可能
包含敏感信息，不得提交真实 cookie；仓库只保留 `.example` 模板。

## 脚本测试

脚本自身的单元测试位于 `tests/`，不启动内核即可运行：

```bash
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
python3 scripts/syscall-profile/test_analyze.py
```

提交路径调整前还应执行：

```bash
find scripts -type f \( -name '*.sh' -o -name '*.bash' \) -print0 \
  | xargs -0 -n1 bash -n
python3 -m compileall -q scripts
make show-config
make -n build ARCH=rv PROFILE=pre
make -n run ARCH=la PROFILE=final
```

## 编写约定

- 新脚本放入最接近其使用场景的目录，不再堆放到 `scripts/` 根目录；
- 文件头使用中文说明用途、主要参数、输出位置和破坏性行为；
- 从脚本自身路径推导 `os/` 根目录，不依赖调用者当前目录；
- 公共逻辑放入 `source/` 或 Python 模块，避免复制 QEMU 和 feature 选择策略；
- 新的稳定入口接入 Makefile，专项工具则在本 README 中登记；
- 不静默覆盖唯一测试镜像，不用无限重试或无界忙等掩盖失败。

操作日志统一使用 `[COMPONENT][LEVEL] message key=value` 格式。Shell 使用
`source/console.bash`，Python 使用 `source/logging_utils.py`；帮助文本、分析表格和机器
可读结果保持原始格式，不混入日志前缀。完整约定见
[`docs/tools/scripts/README.md#日志规范`](../../docs/tools/scripts/README.md#日志规范)。

## 历史改动统计

```bash
make stat                       # 默认整个仓库
make stat DIR=.                 # 只统计 os/
make stat DIR=components         # 相对于执行 make 的 os/ 目录
make stat DIR=..                 # 显式指定整个仓库
make stat DIR=.. DETAILS=1       # 新增/删除/总改动及领域明细
python3 scripts/maintenance/stat_contribute.py --help
python3 scripts/tests/test_stat_contribute.py
```

脚本统计本地开发分支、远程跟踪分支和标签可达的历史，包含备份/实验分支；
排除 refs/remotes/gitlab/ 导出引用，并无条件排除 OuterSystems 提交。不包含
stash、reflog、悬空提交或网络上尚未获取的提交，不会自动 fetch。启动时固定引用对象，
明细模式报告引用数量和快照摘要。浅克隆会报错。计入以下作者邮箱（可由 `.mailmap` 归并）：
`2367651943@qq.com`（zhitian111）、`1592858973@qq.com`（kasss233）、
`2076567173@qq.com`（cesllill）、`lixianli@example.com`（lixianlilili）。
原始作者名 OuterSystems（不区分大小写）排除，即使导出提交从其他引用可达也不计入。
GitLab 的按周重写历史不是独立工作，不能靠提交级 patch-id 完整去重。
其他作者不参与百分比分母。

指定目录必须位于仓库内，可以指定已删除的历史目录。范围按各提交中的路径筛选，
不会追溯文件迁入该目录之前的改动。脚本和 make stat 默认统计整个仓库；
指定目录相对于调用者当前目录解析。

采用功能文件白名单：源码/测试、脚本、配置和移植补丁，分别输出分类明细；未知文件不计入。
同时按 `os/`（内核及内核工具）、`user/`（用户态及移植）、其余路径（仓库工具/配置）
输出领域汇总，所有领域等权计入所选目录的总表。默认范围为整个仓库，DIR=. 可只统计 os/。
具体扩展名和特殊文件名位于脚本的 `CODE_SUFFIXES`、`SCRIPT_SUFFIXES`、
`CONFIG_SUFFIXES`、`CONFIG_NAMES`。JSON/YAML 等配置还须通过目录排除规则。
用户态纳入 build.py、package.toml、C/Java 程序与测试、patches/ 下的 .patch/.diff、
scripts/bin/sbin/init.d 下的无后缀启动脚本、包 config/ 下的维护配置、
rootfs 的 profile/hosts/inittab/passwd/group/wateros-release，以及 pacman 包的 mirrorlist
和 archriscv 启动入口。硬件启动纳入 DTS/DTSI、U-Boot .cmd 和已知 uEnv.txt，此外
纳入 .cnf 和 Java .MF 清单。预编译 .class/.jar 的 Base64、ROM、资源和成绩计算脚本排除。
用户态还必须通过 `user_owned_path` 所有权规则：顶层只计 tools/tests/configs/rootfs 和
Makefile/.gitignore；packages 内仅计 build.py/package.toml、patches/scripts/tools/config，
以及已核对的 mGBA/WaterFM 前端、operator-tools 冒烟源码、OpenJDK 自有探针和 pacman
运行配置。packages 内任意 src/upstream/assets 等目录不会因源码或配置后缀自动入选。
完整自动生成的 BusyBox wateros_defconfig 排除；其构建适配逻辑 build.py 保留。
新增自有源码路径须补充 `PACKAGE_OWN_SOURCES` 或已有明确职责目录，不能泛化为
“包内全部源码”。本规则按已核对的工程职责筛选，不能自动证明每一行的原始来源。

补丁文件只累计维护差异中以 +/- 开头的载荷字符，去掉该标记；上游上下文、补丁头、
hunk 定位信息不计入。新增/删除仍表示补丁文件本身的维护操作，不是对上游应用补丁
之后的源码新增/删除统计。不同时统计 vendor 内的已应用版本，因此不会与它重复累计。
`EXCLUDED_DIRS` 排除 vendor、third_party、third-party、thirdparty、external、
文档、导出、审计、日志、agent 数据、编辑器配置及构建目录；`ltp_log*` 全部排除。
`EXCLUDED_FILES` 排除锁文件及当前配置生成物。**vendor 的新增、修改和删除均不计入**。
不在这些路径中的第三方复制代码、未声明的生成文件不能自动辨认，需补充明确排除规则。

默认输出仅显示“贡献度”、作者百分比和条形图。贡献度定义为去重功能补丁的有效
新增字符数占比，以纳入作者的新增字符总量为分母，按新增量排序；不使用固定比例。
极小的非零占比保留最小可见条形，准确数值以百分比为准。统计过程在 stderr 显示进度条；
终端内原位刷新，重定向时每约 10% 输出一行，不污染 stdout 结果。
`DETAILS=1`（直接脚本使用 `--details`）显示新增/删除行数、字符数、各自占比、总占比
及领域/文件类别明细。总字符量为新增加删除；修改一行同时统计旧行和新行的完整字符。
历史新增即使后来被删除仍保留，删除归属于删除提交作者。源文件注释仍计入。
百分比衡量历史文本改动量，不代表工作价值或实际工时。

同一哈希跨分支只遍历一次；仅对所选目录中纳入统计的功能文件差异计算稳定 Git
`patch-id`，相同补丁（如 cherry-pick/rebase 的重复副本）只计一次。归属采用逆拓扑
遍历中第一个符合条件的作者。OuterSystems 不参与统计。发生冲突或修改过的补丁可能
获得不同 patch-id，因而仍会分别统计；备份分支中有差异的旧补丁也可能被计入。

忽略空白差异和空白行，Git 识别到的纯重命名不计改动；换行重排、跨文件复制和拆分
仍可能累计。合并提交本身排除，冲突解决改动不计入。报告显示遍历提交、排除作者、
合并提交、重复补丁和实际计入补丁数；这些计数有交集，不可直接相加。
Git 二进制差异以及包含 NUL 或无法解码为 UTF-8 的变更行不计字符。

`make stat` 不再调用当前工作树文本规模扫描；`stat_texts.bash` 保留供单独使用。
