# ELF 系统调用静态审计

[工具总览](./README.md) · [脚本清单](../../os/scripts/README.md)

`os/scripts/analysis/elf_syscalls.py` 用于移植 Linux 用户软件前，静态盘点一个 ELF
可执行文件或动态库可能触发的 Linux syscall。工具会递归解析 ELF interpreter、
`DT_NEEDED`、`RPATH`/`RUNPATH` 和 `$ORIGIN`，并按每个 ELF 文件保留证据来源。

运行环境需要 `llvm-readelf` 与 `llvm-objdump`；脚本会在找不到 LLVM 工具时回退到同名的
GNU `readelf`/`objdump`。目标文件不会被执行，也不会修改 rootfs。

## 快速使用

从 `os/` 目录运行：

```bash
./scripts/analysis/elf_syscalls.py /path/to/program

./scripts/analysis/elf_syscalls.py \
  --root ../user/build/staging/rv/rootfs \
  ../user/build/staging/rv/rootfs/usr/bin/program

./scripts/analysis/elf_syscalls.py \
  --root /path/to/rootfs \
  -L /opt/application/lib:/usr/local/lib \
  --format json \
  /path/to/rootfs/opt/application/bin/program > /tmp/program-syscalls.json
```

交叉架构 ELF 应指定 `--root`，否则脚本不会从目标 rootfs 中找到动态加载器和依赖库。
`-L` 可重复使用；绝对目录在指定 `--root` 后按 guest 路径解释。目标 rootfs 内的绝对
symlink 也按 chroot 语义解析，不会意外跳到宿主的 `/lib`。
musl rootfs 中即使没有单独的 `libc.so` symlink，工具也会按 musl 语义把该依赖解析到
对应的 `ld-musl-*.so.1`。

## 输出含义

工具组合两种静态证据：

- `instruction`：反汇编中发现 `ecall`、`syscall` 或 `svc`，且能恢复 syscall 编号寄存器
  中的常量；
- `wrapper-symbol`：ELF 的未定义动态符号与 Linux syscall/libc wrapper 同名。这是用于
  弥补尚未找到动态库时的保守候选，可信度低于直接指令。

RISC-V64、LoongArch64 和 AArch64 使用 Linux asm-generic64 编号。对这些架构，文本和
JSON 输出还会把编号与 WaterOS 当前 syscall 分发表比较，标记为 `implemented` 或
`missing`。x86-64 可用于分析宿主工具，但其编号不能与 WaterOS 直接比较，状态显示为
`n/a`。

`Indirect syscall sites` 表示发现了 syscall 指令，但编号由参数、内存或控制流动态提供，
典型例子是 libc 的 `syscall(2)`。`--strict` 会在存在此类位置或未解析动态库时返回非零
状态，适合自动化检查。文本默认显示紧凑清单，`--show-evidence` 可展开每个编号的来源；
需要机器读取时使用 `--format json`，JSON 始终保留完整证据且 stdout 不混入日志。

## 静态分析边界

结果是保守的静态上界，不是某次执行的精确集合。动态库中从未被目标程序调用的条件路径
也可能进入结果；`dlopen` 的文件名、函数指针、JIT 代码以及传给 `syscall(2)` 的运行时
编号无法完整恢复。移植工作的推荐流程是先用本工具建立候选清单，再用
[`syscall-profile`](../../os/scripts/syscall-profile/README.md) 在代表性 workload 下采集
运行时画像，两者取并集后核对 WaterOS 的实现和 errno 语义。

完整选项以帮助文本为准：

```bash
./scripts/analysis/elf_syscalls.py --help
```
