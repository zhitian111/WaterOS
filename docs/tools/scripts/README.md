# 脚本使用指南

[项目首页](../../../README.md) · [工具总览](../README.md) · [脚本清单](../../../os/scripts/README.md)

WaterOS 的脚本已经按使用场景整理到 `os/scripts/`。本页帮助开发者判断应该使用
Makefile 还是直接调用脚本；完整文件清单、参数示例和副作用说明统一维护在
[`os/scripts/README.md`](../../../os/scripts/README.md)，这里不再复制。

## 先选择入口

日常开发优先从 `os/` 执行 Makefile 目标：

```bash
cd os
make show-config ARCH=rv PROFILE=pre
make build ARCH=rv PROFILE=pre
make run ARCH=rv PROFILE=pre
```

Makefile 负责校验参数、选择 Cargo features、确定内核和镜像，并调用底层 QEMU 或调试
脚本。只有以下情况适合直接进入 `scripts/`：

- 分析 PC、等待时间或 syscall 热点；
- 执行与某项优化绑定的最小验收；
- 分阶段运行比赛 workload；
- 维护 feature 配置树；
- 调试 Makefile 调用的底层工具；
- 安装工具链或执行仓库维护操作。

Makefile 的目标、参数传播和扩展方式见 [`../makefile.md`](../makefile.md)。面向使用者的
完整参数表见根目录 [`README.md`](../../../README.md#构建配置)。

## 按场景查找

| 场景 | 目录 | 进一步说明 |
|:--|:--|:--|
| QEMU 启动、CPU 绑定与并行运行 | `os/scripts/run/` | [`os/scripts/README.md#run运行与-qemu-编排`](../../../os/scripts/README.md#run运行与-qemu-编排) |
| Cargo feature 配置 | `os/scripts/config/` | [`os/scripts/README.md#configfeature-配置`](../../../os/scripts/README.md#configfeature-配置) |
| GDB、停滞检测与符号解析 | `os/scripts/debug/`、`gdb/` | [`../debugging.md`](../debugging.md) |
| 功能、性能与 LTP 测试 | `os/scripts/testing/` | [`os/scripts/README.md#testing功能与性能测试`](../../../os/scripts/README.md#testing功能与性能测试) |
| PC 与等待热点 | `os/scripts/pc-hot/` | [`../pc-hot.md`](../pc-hot.md) |
| syscall 画像 | `os/scripts/syscall-profile/` | [`syscall-profile/README.md`](../../../os/scripts/syscall-profile/README.md) |
| 工具链和测试环境安装 | `os/scripts/setup/` | [`os/scripts/README.md#setupmaintenance-与-competition`](../../../os/scripts/README.md#setupmaintenance-与-competition) |
| 清理、统计和仓库导出 | `os/scripts/maintenance/` | 同上 |
| 比赛平台辅助 | `os/scripts/competition/` | 同上 |

## 状态影响

脚本目录不是一组只读工具。直接运行前应根据文件头和脚本总览确认状态影响：

- `testing/` 中部分脚本会临时改写 `BRINGUP_COMMANDS`；
- LTP 裁剪和迭代脚本可能直接修改目标磁盘镜像；
- `config/apply-config-as-default-features.bash` 会改写多个 `Cargo.toml`；
- `config/rust-analyzer-apply-config.bash` 会写入编辑器配置；
- `maintenance/update*.sh` 会执行 `git add --all`；
- `setup/` 中部分脚本会调用 `sudo`、访问网络或启动 Docker；
- `competition/` 的本地配置可能包含认证信息，不得提交。

运行会写盘的测试时，应使用可恢复的镜像副本或 qcow2 overlay。运行会改源码的脚本前，
先确认 Git 工作区状态；脚本异常退出后也要检查临时备份是否已经恢复。

## 路径约定

稳定入口应通过 Makefile 调用，不应依赖脚本移动前位于 `scripts/` 根目录的旧路径。直接
引用时使用新的分类路径，例如：

```bash
python3 ./scripts/debug/wateros_debug.py --help
python3 ./scripts/run/qemu_run.py --help
./scripts/testing/run_phase_tests.sh
```

历史实验报告中的旧命令用于记录当时环境，不代表当前推荐入口。当前路径始终以
[`os/scripts/README.md`](../../../os/scripts/README.md) 和 `os/Makefile` 为准。
