# Makefile 使用与维护

[项目首页](../../README.md) · [工具总览](./README.md) · [脚本指南](./scripts/README.md)

`os/Makefile` 是 WaterOS 面向开发者的统一操作入口。它负责把架构、赛事阶段、运行模式、
镜像和诊断选项转换为一致的 Cargo features 与 QEMU 参数。正常情况下，不需要手工拼接
`cargo build --features ...`，也不需要直接复制完整 QEMU 命令。

## 快速入口

所有命令从 `os/` 目录执行：

```bash
make help
make show-config ARCH=rv PROFILE=pre
make build ARCH=rv PROFILE=pre
make run ARCH=rv PROFILE=pre
```

使用者参数、默认镜像、调试选项和目标清单已经集中写在根目录
[`README.md`](../../README.md#构建配置)，本页不再复制整张参数表。

## Makefile 负责什么

一次标准调用可以分成四步：

1. `validate-config` 检查 `ARCH`、`PROFILE`、`MODE`、`SMP`、磁盘和调试参数；
2. 根据 `ARCH` 与 `PROFILE` 选择 Cargo target、平台 feature、比赛阶段 feature 和唯一的
   编译期日志上限；
3. 组合堆分配器、operator 模式、额外能力和 GDB feature；
4. 构建命名明确的内核产物，并在需要时交给统一 QEMU 或调试入口。

可以通过 `make show-config` 查看第一至第三步最终解析出的结果。排查“用了哪个镜像”、
“为什么启用了某个模式”或“当前会生成哪个内核”时，应先运行它。

## 参数如何传播

Makefile 只暴露少量稳定参数，内部再转换为脚本环境变量和 Cargo features：

| 输入 | 主要下游 |
|:--|:--|
| `ARCH`、`PROFILE` | Cargo target、平台 feature、阶段 feature、产物名和默认镜像 |
| `MODE`、`SCRIPT`、`GUEST_SHELL` | `operator-*` feature 与构建期环境变量 |
| `SMP`、`SDCARD`、`SNAPSHOT`、`WRITE_DISK` | `scripts/run/qemu_run.py` 的 `WOS_*` 环境变量 |
| `EXTRA_FEATURES`、`HEAP_ALLOCATOR_FEATURE` | 顶层 Cargo feature 列表 |
| `GRAPHICS`、`GRAPHICS_BACKEND` | QEMU 显示设备和显示后端 |
| `PORT`、`START_PAUSED`、`FAULTS` | `scripts/debug/wateros_debug.py` |

底层环境变量是脚本之间的接口，不是日常命令的首选接口。除非正在调试脚本本身，否则
优先传 Make 参数。

日志上限由平台 feature 固定选择：RISC-V64 当前转发 `runtime/impl-warn`，LoongArch64
转发 `runtime/impl-error`。两者最终启用对应的 `log/max_level_*` 编译期过滤，不能同时选择
多个级别，也不能在 operator 启动后动态覆盖。

## 目标分层

Makefile 中的目标按职责分为以下几组：

- 稳定入口：`help`、`show-config`、`build`、`check`、`run`、`shell`；
- 调试入口：`doctor`、`debug`、`debug-server`、`gdb`、`snapshot`、`watch`；
- 产物入口：`kernel-rv-*`、`kernel-la-*`、`all`；
- 真机入口：`la2k_check`、`la2k_uimage`、`la2k_bootscr`、`la2k_tftp`、
  `jh7110_check`、`jh7110_uimage`；
- 地址与 trace 工具：`rv_pc_watch`、`la_pc_watch`、`*_symbol_at`、`*_elf_info`；
- 配置维护：`configure`、`apply_features*`、`revert_features`；
- 仓库维护：`fmt`、`clean`、`stat`、`export`。

其中 `rv_pre_run`、`rv_final_run`、`la_pre_run`、`la_final_run` 等目标属于历史兼容入口。
新的命令、文档和自动化流程应使用 `make run ARCH=... PROFILE=...`，避免继续扩大兼容层。

## 构建产物

`make build` 生成与参数一一对应的文件：

```text
kernel-rv-pre       kernel-rv-final
kernel-la-pre       kernel-la-final
```

`make all` 构建两个 `final` 目标，并额外生成 `kernel-rv` 与 `kernel-la` 兼容副本。Cargo
中间产物仍位于 `target/<target-triple>/<profile>/`，不应直接作为比赛提交文件。

### Loongson 2K1000LA 真机

2K1000LA 的 LA264 核关闭非对齐访问能力，并使用 large code model；`la2k_check` 和
`kernel-la2k` 会带 `-C target-feature=-ual`，同时通过 `-Z build-std=core,alloc` 重建
与该能力一致的核心库。常用流程为：

```bash
make la2k_check
make la2k_uimage
make la2k_tftp TFTP_LISTEN=192.168.1.2 TFTP_ROOT=/srv/tftp
```

`la2k_uimage` 生成 `kernel-la2k`、`kernel-la2k.bin` 和 `kernel-la2k.ui`；这些都是本地
构建产物并已由 `os/.gitignore` 排除。`la2k_tftp` 还生成 U-Boot script，随后调用
`scripts/real-hardware/tftp_serve.sh` 同步文件并启动前台 `dnsmasq`。它会使用 `sudo` 写入
TFTP 根目录，执行前必须确认监听地址和目标目录。

## 镜像与写入策略

Makefile 根据架构与阶段选择四个默认镜像变量，也允许用 `SDCARD` 覆盖单次运行。默认
`SNAPSHOT=1`，QEMU 不向基础镜像写回。只有明确设置 `WRITE_DISK=1` 时，`SNAPSHOT` 才
默认切换为 `0`。

这两个参数表达不同意图：

- `SNAPSHOT` 控制本次 QEMU 是否丢弃磁盘写入；
- `WRITE_DISK` 明确表示调用者允许持久化修改镜像。

验证 fsync、卸载或重启后持久性时，先复制镜像，再开启写盘。不要把唯一赛事镜像作为
可写实验盘。

## Feature 配置工具

标准构建直接由 Makefile 组合 features。`make configure` 生成的 `config.conf` 和
`feature-tree.txt` 用于观察与维护组件配置，不会自动改变一次标准构建。

`make apply_features` 和 `make apply_features_la` 会把配置写入多个 Cargo manifest 的
默认 features，并创建 `.wosbak` 备份；它们不是普通构建步骤。详细说明见
[`os/scripts/README.md#configfeature-配置`](../../os/scripts/README.md#configfeature-配置)。

## 扩展 Makefile

新增用户可见能力时，优先遵循以下边界：

- 新的架构或阶段选择进入统一参数校验，不复制一套独立运行命令；
- QEMU 参数继续由 `scripts/run/qemu_run.py` 统一组装；
- GDB 与停滞诊断继续由 `scripts/debug/wateros_debug.py` 管理；
- 专项测试脚本放入 `scripts/testing/`，只有稳定入口才接入 Makefile；
- 新目标同步更新 `make help`、根 README 和相应工具文档；
- 兼容目标只做窄转发，不维护第二套默认值。

修改后至少执行：

```bash
make show-config ARCH=rv PROFILE=pre
make show-config ARCH=la PROFILE=final
make -n build ARCH=rv PROFILE=pre
make -n run ARCH=la PROFILE=final
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
```

内核构建或运行行为发生变化时，还应按改动范围执行对应架构的 `make check`、构建和 QEMU
workload。dry-run 只能证明命令展开正确，不能代替实际构建与运行验证。
