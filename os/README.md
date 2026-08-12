<div align="center">
  <a href="../README.md">
    <img src="../docs/assert/cover.jpg" height="72" alt="山东大学" />
  </a>
  <h1>WaterOS Kernel</h1>
  <p>双架构内核工程、构建入口与开发导航</p>
  <p>
    <a href="../README.md">项目首页</a> ·
    <a href="../docs/tools/makefile.md">Makefile</a> ·
    <a href="./scripts/README.md">脚本工具</a> ·
    <a href="../docs/README.md">项目文档</a>
  </p>
</div>

---

`os/` 是 WaterOS 内核工程根目录，包含顶层 `wateros` crate、内核组件、构建配置、
平台链接脚本和开发工具。RISC-V64 与 LoongArch64 共用同一套内核机制，通过 Cargo
features 在编译期选择架构、平台、赛事阶段与组件实现。

## 快速开始

下面的命令均在 `os/` 目录执行：

```bash
make help
make show-config ARCH=rv PROFILE=pre

make build ARCH=rv PROFILE=pre
make run ARCH=rv PROFILE=pre

make build ARCH=la PROFILE=pre
make run ARCH=la PROFILE=pre
```

根文件系统镜像可以使用 Makefile 的默认路径，也可以通过 `SDCARD` 覆盖：

```bash
make run ARCH=rv PROFILE=final SDCARD=/path/to/rootfs.img
```

完整参数、默认镜像和目标表见项目首页的
[`构建配置`](../README.md#构建配置)。Makefile 的参数传播、目标分层和扩展约定见
[`docs/tools/makefile.md`](../docs/tools/makefile.md)。

## 常见开发场景

### 交互终端

```bash
make shell ARCH=rv PROFILE=pre
make shell ARCH=la PROFILE=final SMP=4
```

需要覆盖 guest shell 时，通过 `GUEST_SHELL` 传入镜像内的绝对路径：

```bash
make shell ARCH=rv PROFILE=final GUEST_SHELL=/glibc/busybox
```

TTY、Ctrl-C、raw mode 与救援终端的实现说明见
[`wateros-tty`](./components/wateros-tty/README.md)。

### 执行 guest 脚本

`MODE=run` 在编译期启用 `operator-run`，并将 `SCRIPT` 指定的 guest 绝对路径嵌入
内核：

```bash
make run ARCH=rv PROFILE=final \
  MODE=run SCRIPT=/glibc/iperf_testcode.sh
```

脚本结束后 supervisor 会关闭系统，适合自动化验证和性能采样。需要保留现场时使用
`make shell` 或调试入口。

### 图形界面

GUI 默认不进入比赛构建。显式启用后，Makefile 会挂载 VirtIO GPU、键盘和平板设备：

```bash
make run ARCH=rv PROFILE=pre EXTRA_FEATURES=gui
make run ARCH=la PROFILE=pre EXTRA_FEATURES=gui
```

无桌面环境可使用 `GRAPHICS_BACKEND=none` 验证设备初始化。GUI 的结构与扩展方式见
[`wateros-gui`](./components/wateros-gui/README.md)。

### 调试与停滞分析

```bash
make doctor
make debug ARCH=rv PROFILE=final
```

两终端手动调试：

```bash
# 终端一
make debug-server ARCH=la PROFILE=pre PORT=1234

# 终端二
make gdb
```

活动会话建立后，可以使用 `make snapshot`、`make watch` 和 `make gdb` 继续附加。完整
GDB 命令、报告结构与故障注入说明见
[`docs/tools/debugging.md`](../docs/tools/debugging.md)。

### 磁盘写入

普通运行默认 `SNAPSHOT=1`，不会写回基础镜像。只有验证持久化语义时才应使用镜像副本
并开启写盘：

```bash
cp /path/to/baseline.img /tmp/wateros-write-test.img
make run ARCH=rv PROFILE=final \
  SDCARD=/tmp/wateros-write-test.img WRITE_DISK=1
```

## 工程结构

```text
os/
├── Cargo.toml          # 顶层 crate 与 feature 组合
├── Cargo.lock          # Rust 依赖锁定
├── Makefile            # 构建、运行、检查与调试入口
├── build.rs            # 内核构建脚本
├── src/                # 内核入口、Trap 与 workload bring-up
├── components/         # 按子系统拆分的 wateros-* 组件
├── scripts/            # 配置、运行、调试、测试和维护工具
└── vendor/             # 项目使用的第三方本地 fork
```

顶层 `src/` 负责启动顺序和子系统接线，机制与状态应放在对应组件中。组件通常继续分为：

```text
wateros-example/
├── example-api/api-v0/       # 跨实现稳定契约
├── example-impl/impl-*/      # 具体机制和平台实现
└── src/lib.rs                # Feature 选择、再导出与组合逻辑
```

完整组件目录树和一级依赖关系见项目首页的
[`项目结构`](../README.md#项目结构)与[`系统架构`](../README.md#系统架构)。

## 子系统入口

| 子系统 | 主要职责 | 文档 |
|:--|:--|:--|
| `wateros-base` | 基础类型、CPU 标识、同步与集中配置 | [`README`](./components/wateros-base/README.md) |
| `wateros-platform` | ISA、Trap、上下文切换、板级平台与 SMP | [`README`](./components/wateros-platform/README.md) |
| `wateros-runtime` | 控制台、日志、堆、串口与 panic | [`README`](./components/wateros-runtime/README.md) |
| `wateros-task` | 进程线程生命周期、调度与等待 | [`README`](./components/wateros-task/readme.md) |
| `wateros-mm` | 地址空间、页表、物理帧与映射 | 源码目录 |
| `wateros-vfs` | 路径、FD、FS bridge 与页缓存 | 源码目录 |
| `wateros-fs` | 根文件系统、伪文件系统与 ext4 后端 | 源码目录 |
| `wateros-ipc` | signal、futex、pipe、SHM 与 waitqueue | 各子模块 README |
| `wateros-driver` | 块、网络、显示、输入与板级设备发现 | 源码目录 |
| `wateros-network` | Socket 接口与 smoltcp 协议栈 | 源码目录 |
| `wateros-syscall` | Linux generic64 syscall 分发与实现 | [`README`](./components/wateros-syscall/README.md) |
| `wateros-tty` | 终端会话、行规程与字符交互 | [`README`](./components/wateros-tty/README.md) |
| `wateros-gui` | 软件桌面、显示与输入事件 | [`README`](./components/wateros-gui/README.md) |

## 工具与验证

- 脚本分类、参数和安全边界：[`scripts/README.md`](./scripts/README.md)
- 工具文档总览：[`docs/tools/README.md`](../docs/tools/README.md)
- 标准操作流程：[`docs/workflows/README.md`](../docs/workflows/README.md)
- 功能测试与日志分析：
  [`run_testsuits_qemu.md`](../docs/agents/tasks/run_testsuits_qemu.md)、
  [`analyze_kernel_log.md`](../docs/agents/tasks/analyze_kernel_log.md)
- PC 与等待热点：[`docs/tools/pc-hot.md`](../docs/tools/pc-hot.md)

改动内核路径后，应根据影响范围执行对应架构的 `make check`、内核构建和 QEMU
workload。仅通过 Cargo check 不能证明运行时行为正确。

## 开发约定

- 保持 `api-v0`、聚合 crate 和 `impl-*` 的职责边界；
- 通用逻辑同时考虑 RISC-V64 与 LoongArch64；
- 修改初始化顺序时检查堆、页表、调度、中断、驱动和文件系统依赖；
- 不在热路径默认开启高频日志或诊断 feature；
- 文件系统写入测试使用镜像副本或 overlay；
- 保留工作区中与当前任务无关的已有修改。

更完整的代码导航和验证矩阵见项目的 `AGENTS.md`。
