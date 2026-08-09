# pc-hot：按 PC 统计指令执行的 QEMU 插件 + 符号归并工具

用于找内核热路径：运行期间 QEMU 插件在内存里按“PC → 指令数”计数（每核一张哈希表，退出时合并），**运行过程零输出**，退出时把全量不同 PC 一行一条写进文件；再用 `nm` 符号表把 PC 归并回内核符号，得到“符号 → 指令数”表。配合 `-icount` 可以把指令数换算成 QEMU 虚拟时钟 tick，得到“每个核 / 每个符号占用的时钟 tick 数”。

RISC-V 和 LoongArch 是两套独立入口，互不影响，也不改动仓库里其他任何文件。

## 文件

```
scripts/pc-hot/
  pc-hot.c        插件源码（架构无关，两套共用）
  pc-hot.sh       主脚本（build / run / analyze / all）
  pc-hot-rv.sh    RISC-V 入口
  pc-hot-la.sh    LoongArch 入口
  build/          编译产物（已 gitignore，rv/la 分开）
```

## 用法

```bash
# 编译插件
./scripts/pc-hot/pc-hot-rv.sh build
./scripts/pc-hot/pc-hot-la.sh build

# 跑一次负载（把 -plugin 追加到你的 qemu 命令上；运行中不打印 PC，退出后写文件）
./scripts/pc-hot/pc-hot-rv.sh run /tmp/pcs-rv.txt -- \
    timeout 300 qemu-system-riscv64 -machine virt -kernel ./kernel-rv-final \
    -m 8G -nographic -smp 8 -bios default -no-reboot \
    -plugin file=/path/pc-hot-rv.so,out=/tmp/pcs-rv.txt,fast=1

# 反推回符号（pcs.txt + 内核 ELF，默认取 kernel-rv-final / kernel-la-final）
./scripts/pc-hot/pc-hot-rv.sh analyze /tmp/pcs-rv.txt kernel-rv-final 50

# 统计虚拟时钟 tick：qemu 加 -icount shift=0,sleep=off（每条指令推进 2^0=1ns），
# analyze 加 -t 0 对应换算，输出每核 ms 和每个符号的 ms
./scripts/pc-hot/pc-hot-rv.sh analyze -t 0 /tmp/pcs-rv.txt kernel-rv-final 50

# 一步到位：build + run + analyze
./scripts/pc-hot/pc-hot-la.sh all /tmp/pcs-la.txt 50 -- \
    timeout 300 qemu-system-loongarch64 -kernel ./kernel-la-final \
    -m 8G -nographic -smp 8 -no-reboot
```

LoongArch 参考启动参数（与 `scripts/la_final_run.sh` 一致，另加 `-snapshot` 可避免写盘）：

```bash
qemu-system-loongarch64 -kernel ./kernel-la-final -m 8G -nographic -smp 8 \
    -drive file=./sdcard-la-pub.img,if=none,format=raw,id=x0 \
    -device virtio-blk-pci,drive=x0 -no-reboot \
    -device virtio-net-pci,netdev=net0 -netdev user,id=net0 -rtc base=utc
```

## wait-hot：每核 idle/WFI 时间

`wait-hot-rv.sh` / `wait-hot-la.sh` 是独立的 QEMU 插件入口，不改内核。它统计每个
vCPU 进入/离开 WFI 的墙钟时间，并把 idle 时间归到 WFI PC。可和 pc-hot 同时挂载：

```bash
./scripts/pc-hot/wait-hot-rv.sh build
./scripts/pc-hot/wait-hot-rv.sh run /tmp/wait.txt -- \
    timeout 3600 qemu-system-riscv64 ... \
    -plugin file=./scripts/pc-hot/build/rv/pc-hot-rv.so,out=/tmp/pcs.txt \
    -plugin file=./scripts/pc-hot/build/rv/wait-hot-rv.so,out=/tmp/wait.txt
```

输出中的 `wfi_pc` 可用 `addr2line` 归到内核符号。当前 BuildStorm 中所有核的 WFI PC
均归到 `__wateros_idle_task_runtime_main`。

想要“时间”严格正比于指令数，qemu 命令加 `-icount shift=0,sleep=off`（每条指令推进 2^0=1ns 虚拟时间；`sleep=off` 防止 QEMU 按墙钟放慢模拟），analyze 加 `-t 0` 做换算。

## 两种计数模式与性能开销

- 默认模式（不带 `fast=1`）：每条指令回调 + glib 哈希查找，结果精确到每条指令、每核。实测（本机 RV，跑到同一标记点）比无插件慢约 **3.7 倍**（~85 MIPS vs 基线 ~270 MIPS）。
- `fast=1` 模式：用 scoreboard 把 `+1` 内联进翻译后的代码，执行期无回调、无哈希；输出格式与默认模式完全一致。实测开销**接近 0**（14.7s vs 基线 13.4s，~246 MIPS）。代价是每个不同的指令占 8B×核数个内存（几十万~上百万条指令约几十 MB），且翻译阶段稍慢。
- 两者之上再叠 `-icount` 会再慢 2~3 倍（实测慢模式 + icount 约 39 MIPS），只在需要确定性虚拟时间时使用。

日常做热点分析建议直接用 `fast=1`；需要确认个别指令的精确计数时再用默认模式。

## 时钟 tick 统计的语义

- 插件 API 只能推进虚拟时钟（`qemu_plugin_update_ns`），没有读当前时钟的接口；因此“每个符号/每核占用的 tick”用 `-icount` 下“指令数 × 每条指令 2^shift ns”换算，结果确定、可复现。
- `analyze -t N` 会额外输出一行 `# per-core virtual time (ms): v0=... total=...`，并在 Top-N 表里加 `ms` 列；不加 `-t` 则行为与之前完全一致（纯指令数）。
- 注意：`wfi`/idle 期间 QEMU 会推进虚拟时钟让定时器到期，但这部分 tick 不属于任何指令，也不会归到任何符号/核头上——它统计的是“各核运行（执行指令）时占用的 tick”，不是墙钟总时长。若要看睡眠/等待的真实时长，需要锁代码内显式计时或 guest 侧 tick 采样。
- 不加 `-icount` 时，guest 时钟跟随宿主墙钟，指令数占比 ≠ 时间占比；加了 `-icount` 后指令数占比才是虚拟时间占比。

## 输出

`pcs.txt`（`out=` 指定的文件）：

```
# pc-hot: 1521982159 insns, 260035 distinct pcs
  52777335 0x0000000080282696 7243326 7344810 4796566 6426588 8076070 5237851 7464131 6187993
```

每行：`计数 PC v0 v1 ... vN-1`（各核指令数，可看负载是否均衡）。行数只取决于不同 PC 数量，与运行时长/总指令数无关。

`analyze` 输出：

- stdout：Top-N 符号表 `rank 计数 函数名`（Rust 名已用 addr2line 反混淆）；
- `build/<arch>/fn-agg.txt`：全量 `计数 sample_pc 符号 v0 ... vN-1`；
- `build/<arch>/nm.txt`：本次使用的符号表。

## 注意

- 归并时跳过 `.L*`/`$x`/`$d` 汇编标签，PC 归到最近的函数符号；映射不到的显示 `??`（如 OpenSBI/固件地址，RV 为 0x80000000 附近，LA 类似）。
- 0x8033xxxx 附近（RV）是编译器内置库/字符串处理代码，符号边界偶尔模糊，精确归属以反汇编为准；0x80200000–0x80300000 的 WaterOS 内核符号可靠。
- 插件用 `qemu_plugin.h`（QEMU 11 的新 API）和 glib 编译；若头文件/版本与你的 QEMU 不一致，编译或加载会报错，换用与 QEMU 版本匹配的头文件即可。
- 如需自定义 nm/addr2line（例如交叉工具链），设 `PC_HOT_NM` / `PC_HOT_ADDR2LINE`。
- 仓库里已有的 `scripts/debug/pc_trace_watch.py` 是逐条 trace 的思路（PC 变化即打印），适合短窗口调试；本工具是聚合统计，适合长负载热点分析，两者互补。
