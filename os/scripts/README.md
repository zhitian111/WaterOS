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
- ./scripts/resolve_pc_symbol.py
    给定地址，解析其所属内核符号区间与源码位置（`--arch rv|la`）。
- ./scripts/wateros_debug.py
    双架构统一 GDB 入口：`doctor` 检查依赖，`run` 一键构建/启动/监测，
    `snapshot` 手动抓取，`watch` 自动判断停滞，`gdb` 打开带 `wos-*` 命令的交互
    调试器。默认使用磁盘 snapshot，报告写入 `debug-reports/`。

### Makefile 调试目标（在 `os/` 目录下）

| 目标 | 用途 |
|------|------|
| `make rv_pc_watch` | 编译并监视 riscv64 PC 变动（终端逐行输出） |
| `make la_pc_watch` | 编译并监视 loongarch64 PC 变动 |
| `make rv_symbol_at ADDR=0x80201234` | 查询 riscv64 内核地址所属符号 |
| `make la_symbol_at ADDR=0x90001234` | 查询 loongarch64 内核地址所属符号 |
| `make rv_final_run_log` | 启用 `stall-debug`，运行并保存 `output.log` |
| `make rv_pre_run-gdb` | 构建独立 `kernel-rv-pre-gdb`，开放 GDB 端口并暂停 |
| `make rv_final_run_log-gdb` | `stall-debug` 内核开放 GDB 端口并保存串口日志 |
| `make la_pre_run-gdb` | LoongArch 初赛内核开放 GDB 端口并暂停等待连接 |
| `make la_final_run-gdb` | LoongArch 决赛内核开放 GDB 端口并暂停等待连接 |
| `make la_gdb_snapshot` | 统一 snapshot 命令的兼容别名，使用 GDB 抓取 LA 完整报告 |

所有真实运行目标都支持 `-gdb` 后缀。默认 `GDB_WAIT=1`，QEMU 使用 `-S`；若要
让系统先运行到卡死位置，再连接调试器，使用
`make rv_final_run_log-gdb GDB_WAIT=0`。端口可通过 `GDB_PORT=1235` 修改。
`WOS_SMP=1..8` 控制 vCPU 数量；GDB 模式默认传入 `WOS_QEMU_SNAPSHOT=1`，普通
运行目标仍保持原磁盘语义。

GDB/LLDB 的完整操作流程见
[`docs/debugging/GDB_STALL_DEBUG.md`](../../docs/debugging/GDB_STALL_DEBUG.md)。
