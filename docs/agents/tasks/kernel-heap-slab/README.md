# 内核堆 slab/SLUB 优化任务

本目录是 `perf/kernel-heap-slab` 分支的专用任务拆解，目标是把 WaterOS 当前
“单全局 TLSF 堆”逐步演进为 Linux SLUB 风格的“统一 frame allocator + 每核 slab
快路径”，以降低多核下内核堆分配路径的全局锁竞争。

## 工作区与基线

- 分支：`perf/kernel-heap-slab`
- 工作树：`/tmp/wateros-kernel-heap-slab`
- 基线提交：`59f50c44`
- CodeGraph：已在 `/tmp/wateros-kernel-heap-slab/.codegraph` 初始化，任务内优先使用
  `codegraph explore` / `codegraph impact` 定位调用链。
- 当前 main 基线构建：
  - RV：`/tmp/wateros-kernel-heap-slab-baseline/kernel-rv-final`
  - LA：`/tmp/wateros-kernel-heap-slab-baseline/kernel-la-final`

基线 SHA-256（当前 HEAD `59f50c44` 的 `make kernel-*-final` 产物）：

```text
0ce76fba044687f3f59dba4dd0c833b9a7c302135500405b78b62148ef7d96bb  kernel-rv-final
a663db04ddb7bd0121df9682dcb9e0f292fbf2b9787ce294d05c000121d41b38  kernel-la-final
```

## 外部输入审计值

```text
2c411447274fbd83505d2fac505a5d9e8ed8ff3bdfc3d2d6cbdb8f61ff7d90d2  ~/Downloads/sdcard-la-pub.img.gz
cba87f43ae569bcf2b8e4614f75cec1bf51bedb2804626fe466fcce3861df6f1  ~/Downloads/sdcard-rv-pub.img.gz
84d631012532e6817565cba02d35d8a2721c5ec7787a1e0519d6d0ae0a4274bb  ~/Downloads/buildstorm_testcode.recovered.sh
```

## 两条验收主线

### 功能/ bug 验收

最终功能验收标准：

1. 两个架构都必须以 QEMU 9.2.1、线上等价参数连续 3 轮全新镜像通过完整 buildstorm；
2. 每轮同时满足：
   - `TOOLCHAIN_RESULT status=OK`
   - `MINIBUILD_RESULT status=OK`
   - `BUILDSTORM_RESULT mode=multi status=OK rc=0 ... run=OK`
   - WaterOS 输出 `all commands finished`
   - 无内核 panic、无 `[heap] OOM`、无 `ENOMEM`、无 `SIGSEGV`/fault 异常
3. `make rv_check`、`make la_check`、`git diff --check` 必须通过。

单任务的回归验收在各自文档中给出；纯文档/抽象任务以 build + check 为准，运行时
语义任务至少完成受影响架构的一次完整 buildstorm 或等效冒烟。

### 性能验收

性能指标：镜像内 `BUILDSTORM_RESULT ... elapsed_s=<秒>`。

- **性能测试互斥规则**：性能测试开始前必须检查系统中是否已有 QEMU 进程；若有，
  必须等待所有 `qemu-system-riscv64` / `qemu-system-loongarch64` 退出后再开始。
  功能测试不受此限制。
- 基线：task 00 在 `59f50c44` 上分别对 LA、RV 跑 3 轮新鲜镜像，记录 `elapsed_s`
  并取中位数。
- 最终：task 08 在最终分支提交上以同样脚本和命令跑 3 轮新鲜镜像，取中位数。
- 最终要求：LA 和 RV 的最终 `elapsed_s` 中位数都小于 baseline 对应中位数；过程中
  中间任务允许性能回退，但不能留下功能 bug。

性能测试前执行：

```bash
pgrep -af 'qemu-system-(riscv64|loongarch64)' && { echo 'wait for existing QEMU'; sleep 5; }
```

## 镜像准备通用命令

以下命令假设从 `os/` 执行，且磁盘只剩一个 15 GB raw 镜像的空间时必须先删除上一轮
临时镜像。不要覆盖 `~/Downloads` 下唯一压缩包。

```bash
cd /tmp/wateros-kernel-heap-slab/os

# LA 临时镜像 + 覆写 buildstorm 脚本
gzip -dc /home/zhitian/Downloads/sdcard-la-pub.img.gz > /tmp/wateros-slab-la.img
debugfs -w -R 'rm /glibc/buildstorm_testcode.sh' /tmp/wateros-slab-la.img
debugfs -w -R 'write /home/zhitian/Downloads/buildstorm_testcode.recovered.sh /glibc/buildstorm_testcode.sh' /tmp/wateros-slab-la.img
debugfs -w -R 'set_inode_field /glibc/buildstorm_testcode.sh mode 0100755' /tmp/wateros-slab-la.img

# RV 临时镜像 + 覆写 buildstorm 脚本
gzip -dc /home/zhitian/Downloads/sdcard-rv-pub.img.gz > /tmp/wateros-slab-rv.img
debugfs -w -R 'rm /glibc/buildstorm_testcode.sh' /tmp/wateros-slab-rv.img
debugfs -w -R 'write /home/zhitian/Downloads/buildstorm_testcode.recovered.sh /glibc/buildstorm_testcode.sh' /tmp/wateros-slab-rv.img
debugfs -w -R 'set_inode_field /glibc/buildstorm_testcode.sh mode 0100755' /tmp/wateros-slab-rv.img
```

若 `debugfs set_inode_field` 被本机版本拒绝，先 `debugfs -R 'stat /glibc/buildstorm_testcode.sh'`，
再使用等价 `debugfs` 写入命令；脚本正文不得改动。

## QEMU 9.2.1 运行命令

```bash
QEMU=/home/zhitian/qemu_9_2_1/qemu-9.2.1/build

# LoongArch64
"$QEMU/qemu-system-loongarch64" -kernel kernel-la-final -m 36G -nographic -smp 12 \
  -drive file=/tmp/wateros-slab-la.img,if=none,format=raw,id=x0 \
  -device virtio-blk-pci,drive=x0 -no-reboot \
  -device virtio-net-pci,netdev=net0 -netdev user,id=net0 -rtc base=utc

# RISC-V64
"$QEMU/qemu-system-riscv64" -machine virt -kernel kernel-rv-final -m 16G -nographic -smp 8 \
  -bios default \
  -drive file=/tmp/wateros-slab-rv.img,if=none,format=raw,id=x0 \
  -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 -no-reboot \
  -device virtio-net-device,netdev=net -netdev user,id=net -rtc base=utc
```

## 任务顺序

| 任务 | 目标 |
|---|---|
| `00-baseline-and-harness.md` | 建立 current-main 构建、镜像覆写与性能基线 |
| `01-heap-backend-abstraction.md` | 抽象 GlobalAlloc 后端选择，保持 TLSF 行为不变 |
| `02-slab-core.md` | 实现未接线的 slab size-class/per-CPU 核心与单测 |
| `03-slab-boot-switch.md` | 接入 boot TLSF + frame-backed slab 切换，小对象走 slab |
| `04-cross-cpu-free.md` | 跨核 free、partial/empty 管理与空 slab 归还 |
| `05-large-object-fallback-and-realloc.md` | 大对象回退与 realloc 正确性 |
| `06-contiguous-frame-path.md` | 大对象迁入 frame allocator 的连续帧路径 |
| `07-telemetry-tuning.md` | 统计、诊断、参数调优与文档同步 |
| `08-final-functional-acceptance.md` | 双架构完整功能/bug 回归 |
| `09-final-performance-acceptance.md` | 双架构最终性能对比与收尾 |

## 每个任务完成后的任务简报

每个任务文档完成后，必须新增：

```text
docs/agents/tasks/kernel-heap-slab/history/<task-id>-brief.md
```

简报至少包含：

1. 当次任务完成情况（目标是否全部完成，部分完成要列缺口）；
2. 实际改动文件与关键 diff 摘要；
3. 实际执行的验收命令和结果；
4. 未验证项、已知风险和下一任务前置条件；
5. 是否触发 `docs/`、README、Makefile、feature 传播等文档同步。

## 提交约定

- 一次任务一个 commit；
- 提交信息格式：`[heap-slab] <task-id> <一句话说明>`；
- 提交前执行 `git diff --check`，不要混入无关文件或生成物；
- 生成物 `kernel-*`、`*.img`、`target/`、日志、`.codegraph/` 不得提交。
