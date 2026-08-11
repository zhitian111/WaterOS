# PC-hot 性能热点分析流程

适用于定位长负载中的内核执行热点、CPU 负载失衡和 idle/WFI 等待。工具选项、输出格式和
开销说明见 [`../tools/pc-hot.md`](../tools/pc-hot.md)。

## 1. 建立可比基线

记录架构、内核 ELF、镜像、QEMU 参数、SMP 数、workload、超时与宿主负载。先在 snapshot 或
overlay 上跑一次无插件基线；不要把不同 workload、不同镜像或不同 CPU 亲和性结果混在同一张表。

## 2. 采样

从 `os/` 运行。日常使用低开销的 `fast=1`；仅在需要把指令数换算为可复现虚拟时间时才加入
`-icount shift=0,sleep=off` 与 `analyze -t 0`。

```bash
./scripts/pc-hot/pc-hot-rv.sh build
./scripts/pc-hot/pc-hot-rv.sh run /tmp/pcs-rv.txt -- \
  timeout 300 qemu-system-riscv64 ... \
  -plugin file=/path/pc-hot-rv.so,out=/tmp/pcs-rv.txt,fast=1
./scripts/pc-hot/pc-hot-rv.sh analyze /tmp/pcs-rv.txt kernel-rv-final 50
```

若要区分“忙于执行”与“CPU 在等待”，同时采集 wait-hot：

```bash
./scripts/pc-hot/wait-hot-rv.sh build
./scripts/pc-hot/wait-hot-rv.sh run /tmp/wait-rv.txt -- \
  timeout 300 qemu-system-riscv64 ...
```

LoongArch 使用同名的 `-la.sh` 入口。

## 3. 解读与决策

1. 保存 stdout Top-N、`build/<arch>/fn-agg.txt`、原始 PC 文件和完整 QEMU 命令。
2. 先排除 OpenSBI/固件与 idle 符号；PC-hot 统计的是执行指令，不等同于 wall-clock 时间。
3. 对比每核计数和 wait-hot：高指令数是执行热点，WFI 时间高则优先检查负载分配或 I/O 等待。
4. 用第二轮相同条件采样确认热点稳定，再决定优化；被拒绝的 A/B 实验也保存结论和数据。
5. 修复后同时跑功能回归与同条件 A/B，报告绝对量、占比和可能的测量误差，不只报告“变快”。

原始采样输出和插件构建产物均为生成物，不提交；可把摘要、命令和结论记录到对应任务的
`history/` 或 `reports/`。
