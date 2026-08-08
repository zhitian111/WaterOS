# 脚本执行须知
## 权限修改
当你把脚本拉取下来之后，需要先进行权限修改，使得脚本具有执行权限。
假设你的终端模拟器的工作路径为项目根目录，则执行以下命令：
```bash
chmod +x ./scripts/script_name.sh
```
其中，script_name.sh 是你要执行的脚本文件名。
如果你不想麻烦，可以直接使用通配符为所有脚本文件添加执行权限：
```bash
chmod +x ./scripts/*.sh
```
具体命令请根据终端模拟器实际工作目录修改。
## 执行脚本
因为技术力原因，有的脚本的执行是不可逆的，因此在执行脚本之前，请务必清楚你在做什么。
假设你的终端模拟器的工作路径为项目根目录，则执行以下命令：
```bash
./scripts/script_name.sh
```
其中，script_name.sh 是你要执行的脚本文件名。
具体命令请根据终端模拟器实际工作目录修改。
## 脚本列表和效果说明
(顺序不分先后)
- ./scripts/rustc_target_for_oscmp.sh
    用于查看当前rust编译器支持的本次比赛需要的目标架构
- ./scripts/rustc_target_tools_install.sh
    用于安装构建对应平台的程序的rust工具链
- ./scripts/rust-install.sh
    用于安装rust语言环境
    ！！！注意：这个脚本请在本项目之外运行！！！
- ./scripts/test_in_qemu_riscv.sh
    用于在qemu-riscv上测试rust程序
    ！！！注意：运行这个脚本之前，请先配置好qemu环境！！！
- ./scripts/check_current_elf_file.sh
    用于检查可以运行的elf文件
- ./scripts/rustc_target_tools_install.sh
    用于安装构建对应平台的程序的rust工具链
- ./scripts/update.sh
    用于更新远程仓库代码
- ./scripts/feature-args.sh
    读取 os/feature-config.toml，输出 cargo --features 所需的逗号分隔参数字符串。需在 os 目录下使用，例如：`cargo build -p os --features "$(./scripts/feature-args.sh)"`
- ./scripts/pc_trace_watch.py
    启动 QEMU，**仅在 guest PC 变化时**向 stdout 打印一行（含符号）。无 TUI、无额外依赖。`make rv_pc_watch` / `make la_pc_watch`。
- ./scripts/debug/qemu_launcher.py
    构建 QEMU 命令（trace 走独立 fd，串口 stdout 后台排空）。
- ./scripts/qemu_run.py
    普通运行、GDB 和四个兼容 shell 脚本共用的 QEMU 参数组装器。
- ./scripts/resolve_pc_symbol.py
    给定地址，解析其所属内核符号区间与源码位置（`--arch rv|la`）。
- ./scripts/wateros_debug.py
    双架构统一 GDB 入口：`doctor` 检查依赖，`run` 一键构建/启动/监测，
    `snapshot` 手动抓取，`watch` 自动判断停滞，`gdb` 打开带 `wos-*` 命令的交互
    调试器。默认使用磁盘 snapshot，报告写入 `debug-reports/`。
- ./scripts/operator_smoke.py
    仅用 Python 标准库通过 PTY 驱动 operator 串口，验证管道/重定向、后台
    `wait`、Ctrl-C、raw termios 和救援 shell；`--mode run --script /...` 验证
    脚本模式，`--vim` 另要求镜像包含 Vim 并验证其 raw mode 保存。

### 启动行为由编译期 feature 控制

QEMU 启动命令不再携带 `-append` 或 `wos.*` bootargs。`make MODE=...` 只在构建期
选择根 crate feature：`auto` 不加 operator feature，`shell` 启用
`operator-shell`，`run` 启用 `operator-run`。shell 路径和 run 脚本分别通过
`GUEST_SHELL`、`SCRIPT` 在编译时嵌入内核。

推荐通过 `make run`、`make shell` 或 `make debug-server` 启动；Makefile 会按
`ARCH/PROFILE` 从 `RV_PRE_IMAGE`、`RV_FINAL_IMAGE`、`LA_PRE_IMAGE`、
`LA_FINAL_IMAGE` 中选择镜像。只有绕过 Make 直接执行上述兼容脚本时，才需要
显式设置 `WOS_SDCARD`。

磁盘策略只由 `WOS_QEMU_SNAPSHOT` / Makefile 的 `SNAPSHOT`、`WRITE_DISK`
控制，与启动行为无关。完整的现场命令表见 `os/README.md`。

### Makefile 调试目标（在 `os/` 目录下）

| 目标 | 用途 |
|------|------|
| `make rv_pc_watch` | 编译并监视 riscv64 PC 变动（终端逐行输出） |
| `make la_pc_watch` | 编译并监视 loongarch64 PC 变动 |
| `make rv_symbol_at ADDR=0x80201234` | 查询 riscv64 内核地址所属符号 |
| `make la_symbol_at ADDR=0x90001234` | 查询 loongarch64 内核地址所属符号 |
| `make debug ARCH=rv PROFILE=pre` | 自动构建、启动和 watch |
| `make debug-server ARCH=la PROFILE=final` | 终端一启动手动 GDB server |
| `make gdb` | 终端二从活动会话恢复 ELF/端口并连接 |
| `make snapshot` | 对活动会话立即生成一次完整报告 |
| `make watch` | 对活动会话启动停滞监测 |

`debug-server` 默认 `START_PAUSED=1`；运行期手动附加使用
`START_PAUSED=0`。活动会话保存在 `debug-reports/active/session.json`，
所以 `make gdb/snapshot/watch` 不需要重复架构、ELF 和端口。旧 `*-gdb`
目标仅作弃用兼容转发。

#### 并行跑 QEMU（32 核可直接按核分片）

所有 RISC-V / LoongArch 的 run 脚本都支持 `WOS_TASKSET_CPUS`，用逗号/横杠指定主机
CPU 集合，例如：

```bash
WOS_TASKSET_CPUS=0-7 ./scripts/rv_final_run.sh
```

也可以用总控脚本同时启动多个测试（按主机核心自动分配）：

```bash
cd os
WOS_CORES_PER_JOB=8 \
WOS_MAX_PARALLEL_JOBS=4 \
./scripts/run_qemu_parallel.sh \
  "WOS_SMP=8 make rv_final_run" \
  "WOS_SMP=8 make rv_final_run" \
  "WOS_SMP=8 make rv_final_run" \
  "WOS_SMP=8 make rv_final_run"
```

如果你机器是 32 核（`nproc`=32），上面会自动分配核区间 `0-7 / 8-15 / 16-23 / 24-31`，
4 个实例可并行执行。也可在命令中省略 `WOS_SMP`，通过自动注入让其等于
`WOS_CORES_PER_JOB`：

```bash
WOS_CORES_PER_JOB=4 WOS_AUTO_SMP=1 \
./scripts/run_qemu_parallel.sh \
  "make rv_final_run" \
  "make rv_final_run" \
  "make rv_final_run" \
  "make rv_final_run"
```

在 32 核机器上，上面会使用 `0-7 / 8-15 / 16-23 / 24-31` 四组核。

如果测试要写盘，使用 snapshot 时要给每实例分配不同的 `WOS_SNAPSHOT_ID`，
避免 overlay 文件互相覆盖。

并行运行同一镜像多个实例时，可通过总控脚本关闭 qemu 磁盘锁验证（仅推荐 qcow2 镜像）:

```bash
cd os
WOS_CORES_PER_JOB=4 WOS_AUTO_SMP=1 WOS_AUTO_UNLOCK_DRIVE=1 \
./scripts/run_qemu_parallel.sh \
  "WOS_QEMU_SNAPSHOT=1 WOS_SDCARD=./sdcard-rv-pub.qcow2 make rv_pre_run" \
  "WOS_QEMU_SNAPSHOT=1 WOS_SDCARD=./sdcard-rv-pub.qcow2 make rv_pre_run"
```

`WOS_AUTO_UNLOCK_DRIVE=1` 会为每个任务注入
`WOS_QEMU_IMAGE_DRIVE_OPTIONS=locking=off`（会自动忽略非 qcow2 镜像）。若要避免读共享冲突请改用独立镜像文件。

若你现在只有 raw 镜像，可先克隆为 qcow2 供并行测试：
`qemu-img convert -f raw -O qcow2 sdcard-rv-pub.img sdcard-rv-pub.qcow2`

GDB/LLDB 的完整操作流程见
[`docs/debugging/GDB_STALL_DEBUG.md`](../../docs/debugging/GDB_STALL_DEBUG.md)。
