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
- ./scripts/rv_qemu_run_trace_pc.sh
    与 `rv_qemu_run.sh` 相同，追加 `-d exec,nochain` 将 guest PC 打印到 stderr；由 PC 监控 TUI 解析，通常通过 `make rv_qemu_trace_pc` 启动。
- ./scripts/la_qemu_run_trace_pc.sh
    LoongArch 版 PC 追踪 QEMU 启动脚本；通常通过 `make la_qemu_trace_pc` 启动。
- ./scripts/debug/qemu_pc_monitor.py
    Textual TUI：实时解析 QEMU trace，显示串口日志、PC+符号、折线图与循环检测。
- ./scripts/resolve_pc_symbol.py
    给定地址，解析其所属内核符号区间与源码位置（`--arch rv|la`）。
- ./scripts/requirements-debug.txt
    PC 监控 TUI 依赖（`textual`）。**请自行安装**，例如 `pip install -r scripts/requirements-debug.txt` 或 Arch 上 `pacman -S python-textual`。

### Makefile 调试目标（在 `os/` 目录下）

| 目标 | 用途 |
|------|------|
| `make rv_qemu_trace_pc` | 编译并启动 riscv64 PC 追踪 TUI |
| `make la_qemu_trace_pc` | 编译并启动 loongarch64 PC 追踪 TUI |
| `make rv_symbol_at ADDR=0x80201234` | 查询 riscv64 内核地址所属符号 |
| `make la_symbol_at ADDR=0x90001234` | 查询 loongarch64 内核地址所属符号 |
