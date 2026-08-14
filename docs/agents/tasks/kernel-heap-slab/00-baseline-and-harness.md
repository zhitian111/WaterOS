# Task 00：建立 current-main 构建与性能基线

## 任务目标

在开始任何代码改动前，固定当前 `main` 快照的功能与性能基线，并建立可重复的
镜像覆写、运行、采集流程。本任务不修改内核语义，只提交任务文档和必要的本地
辅助脚本/说明。

## 当前状态

- 分支：`perf/kernel-heap-slab`
- 基线提交：`59f50c44`
- 基线内核已保留：
  - `/tmp/wateros-kernel-heap-slab-baseline/kernel-rv-final`
  - `/tmp/wateros-kernel-heap-slab-baseline/kernel-la-final`

## 实施方案

1. 在 worktree 中确认 `git status`，不要带回主仓库已有的 untracked 文件。
2. 从 `os/` 构建 current main 的两个 final 内核：

   ```bash
   cd /tmp/wateros-kernel-heap-slab/os
   make kernel-rv-final
   make kernel-la-final
   sha256sum kernel-rv-final kernel-la-final
   ```

3. 把产物复制到 `/tmp/wateros-kernel-heap-slab-baseline/`，命名保持不变。
4. 对每个架构执行一次“镜像准备 → 完整 buildstorm → 保存日志”的流程，先验证
   功能通过；基线性能采样建议每个架构 3 轮。
5. 将每轮日志、镜像 SHA、`elapsed_s` 记录到
   `docs/agents/tasks/kernel-heap-slab/history/00-brief.md`。

性能测试前必须先确认无其他 QEMU 进程占用：

```bash
pgrep -af 'qemu-system-(riscv64|loongarch64)' || true
```

若存在进程，则等待其退出后再开始性能测试。

## 涉及文件/目录

- `os/kernel-rv-final`、`os/kernel-la-final`：构建产物，不提交。
- `/tmp/wateros-kernel-heap-slab-baseline/`：基线产物保存目录。
- `docs/agents/tasks/kernel-heap-slab/README.md`：本任务目录总览。
- `docs/agents/tasks/kernel-heap-slab/history/00-brief.md`：完成后简报。

## CodeGraph 查询

本任务不查代码实现，但建议用以下命令确认基线代码位置：

```bash
cd /tmp/wateros-kernel-heap-slab
codegraph status .
codegraph explore "runtime::heap_allocator::init"
codegraph impact "HEAP_ALLOCATOR"
```

## 镜像覆写与运行

以下命令从 `os/` 执行。若同时存在两个 15 GB raw 镜像会占满空间，必须一次只保留
一个临时镜像，测完删除再准备另一架构。

### LoongArch64

```bash
cd /tmp/wateros-kernel-heap-slab/os
gzip -dc /home/zhitian/Downloads/sdcard-la-pub.img.gz > /tmp/wateros-slab-la.img
debugfs -w -R 'rm /glibc/buildstorm_testcode.sh' /tmp/wateros-slab-la.img
debugfs -w -R 'write /home/zhitian/Downloads/buildstorm_testcode.recovered.sh /glibc/buildstorm_testcode.sh' /tmp/wateros-slab-la.img
debugfs -w -R 'set_inode_field /glibc/buildstorm_testcode.sh mode 0100755' /tmp/wateros-slab-la.img

/home/zhitian/qemu_9_2_1/qemu-9.2.1/build/qemu-system-loongarch64 \
  -kernel kernel-la-final -m 36G -nographic -smp 12 \
  -drive file=/tmp/wateros-slab-la.img,if=none,format=raw,id=x0 \
  -device virtio-blk-pci,drive=x0 -no-reboot \
  -device virtio-net-pci,netdev=net0 -netdev user,id=net0 -rtc base=utc -snapshot \
  2>&1 | tee /tmp/wateros-slab-la-baseline-N.log
```

### RISC-V64

```bash
cd /tmp/wateros-kernel-heap-slab/os
gzip -dc /home/zhitian/Downloads/sdcard-rv-pub.img.gz > /tmp/wateros-slab-rv.img
debugfs -w -R 'rm /glibc/buildstorm_testcode.sh' /tmp/wateros-slab-rv.img
debugfs -w -R 'write /home/zhitian/Downloads/buildstorm_testcode.recovered.sh /glibc/buildstorm_testcode.sh' /tmp/wateros-slab-rv.img
debugfs -w -R 'set_inode_field /glibc/buildstorm_testcode.sh mode 0100755' /tmp/wateros-slab-rv.img

/home/zhitian/qemu_9_2_1/qemu-9.2.1/build/qemu-system-riscv64 \
  -machine virt -kernel kernel-rv-final -m 16G -nographic -smp 8 -bios default \
  -drive file=/tmp/wateros-slab-rv.img,if=none,format=raw,id=x0 \
  -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 -no-reboot \
  -device virtio-net-device,netdev=net -netdev user,id=net -rtc base=utc -snapshot \
  2>&1 | tee /tmp/wateros-slab-rv-baseline-N.log
```

## 性能数据采集

从日志中提取：

```bash
grep -E 'TOOLCHAIN_RESULT|MINIBUILD_RESULT|BUILDSTORM_RESULT|all commands finished' /tmp/wateros-slab-*.log
```

记录字段：

```text
arch=la|rv round=N status=OK|FAIL rc=<rc> cores=<n> elapsed_s=<s> run=OK|FAIL
```

## 验收方式

1. 两个 baseline 内核均可构建且 SHA 与 README 记录一致。
2. 至少每架构 1 轮完整 buildstorm 功能通过；性能基线最终记录每架构 3 轮中位数。
3. 临时镜像用后删除，`~/Downloads` 下压缩包 SHA 保持不变。
4. `git diff --check` 通过；本任务只提交文档/脚本，不提交内核或镜像。

## 完成后

按 README 要求新增 `history/00-brief.md`，写明基线数值、日志路径、未验证项。
