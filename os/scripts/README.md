# WaterOS 脚本工具

[项目首页](../../README.md) · [内核工程](../README.md) · [工具文档](../../docs/tools/README.md)

`os/scripts/` 保存 WaterOS 的构建配置、QEMU 运行、调试、测试和维护工具。日常构建与
运行应优先使用 [`../Makefile`](../Makefile) 提供的统一目标；直接运行脚本主要用于专项
测试、性能分析或维护底层工具。

除特别说明外，下文命令均从 `os/` 目录执行。

## 目录结构

```text
scripts/
├── competition/       # 比赛平台认证与提交环境辅助工具
├── config/            # Cargo feature 树的导出、转换和应用
├── debug/             # QEMU/GDB 调试、停滞检测与符号解析
├── gdb/               # 在 GDB 内加载的 WaterOS 扩展
├── maintenance/       # 清理、统计、导出和仓库维护
├── pc-hot/            # 基于 QEMU TCG plugin 的 PC 与等待热点分析
├── run/               # 统一 QEMU 启动器、兼容入口与并行运行
├── root_image/        # 物理板根镜像构建与校验（MBR/GPT、loopback 无特权）
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
| `testing/operator_smoke.py` | `--arch {rv,la}` | profile、SMP、模式、Guest 脚本、超时和日志路径 |
| `testing/ltp_prune_sdcard_before.sh` | 无 | 镜像、起始用例、libc、dry-run 和重置源镜像 |
| `root_image/root_image.py` | `build` 或 `verify` | `--output`、`--manifest`、`--size-mib`、`--partition-table {mbr,gpt}`、`--source-root` |

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
