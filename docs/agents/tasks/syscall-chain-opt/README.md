# BuildStorm syscall 链路优化任务

本目录用于在独立分支 `perf/syscall-chain-opt` 上按可回归、可验收的 commit 推进
路径解析、FD I/O、ELF 装载、TLB、文件系统写回和后续并发优化。每个编号文档只描述
一个实现任务；任务成功后形成一个实现 commit，并新增一份对应简报。

## 工作区

- 工作树：`/home/zhitian/project/WaterOS_refactor/.worktrees/syscall-chain-opt`
- 分支：`perf/syscall-chain-opt`
- 起点：`main` 的 `e54000d9fce5f97ca1e24175945b641bb0c92680`
- CodeGraph：已初始化，执行任务前先 `codegraph sync .`
- 所有构建、运行命令除特别说明外均从工作树的 `os/` 目录执行。

## 统一性能验收基线

线上平台使用 QEMU 9.2.1，本任务禁止使用 `PATH` 中的默认 QEMU：

```bash
/home/zhitian/qemu_9_2_1/qemu-9.2.1/build/qemu-system-riscv64 --version
```

版本输出必须是 `QEMU emulator version 9.2.1`。RISC-V 8 核性能验收命令为：

```bash
/home/zhitian/qemu_9_2_1/qemu-9.2.1/build/qemu-system-riscv64 \
  -machine virt \
  -kernel kernel-rv \
  -m 16G \
  -nographic \
  -smp 8 \
  -bios default \
  -drive file=/home/zhitian/Downloads/sdcard-rv-pub.img,if=none,format=raw,id=x0 \
  -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
  -no-reboot \
  -device virtio-net-device,netdev=net \
  -netdev user,id=net \
  -rtc base=utc \
  -snapshot
```

`-snapshot` 是硬性要求，任何任务文档中的简写命令都不得移除它。每轮记录：commit、
QEMU 版本、kernel SHA-256、镜像 SHA-256、SMP、开始/结束时间、BuildStorm 各阶段结果和
完整日志路径。性能任务至少使用同一宿主条件做交错 A/B；候选不得出现 panic、SIGSEGV、
OOM、文件系统错误或 workload 缺项。低于宿主噪声的差异只记为“无可确认收益”。

## 每个任务的共同门禁

```bash
cd /home/zhitian/project/WaterOS_refactor/.worktrees/syscall-chain-opt
codegraph sync .
git status --short
cd os
make rv_check
make la_check
cd ..
git diff --check
```

架构专有任务可以在文档中缩小运行矩阵，但不能省略另一架构静态检查。涉及内核热路径、
MM、VFS、FS 或 task 生命周期的任务还必须构建对应 final kernel；涉及持久化的任务必须
使用 `-snapshot` QEMU 回归，并另外在可丢弃镜像副本上做重挂载与 `e2fsck -fn`。

## 提交与简报

- 一个编号任务对应一个实现 commit，不把下一任务的准备性重构混入当前 commit。
- 提交信息使用 `[perf] ...`、`[fix] ...`、`[refactor] ...` 或 `[docs] ...` 格式。
- 每次任务完成后新增 `history/<task-id>-brief.md`，与实现放在同一 commit。
- 简报必须记录：完成状态、提交 hash、关键文件、行为变化、实际命令及结果、A/B 数据、
  未验证项、回退条件、下一任务前置条件和文档同步情况。
- 失败实验也要写简报；代码回退后保留证据，不把失败结果描述为已完成优化。

## 任务顺序

| 顺序 | 文档 | 独立验收目标 |
|---:|---|---|
| 00 | `00-reproducible-performance-runner.md` | 固化 QEMU 9.2.1 与 A/B 证据格式 |
| 01 | `01-low-overhead-hotpath-counters.md` | 建立本任务所需低开销计数 |
| 02 | `02-mount-namespace-arc-cow.md` | mount 路由不再深拷贝 namespace |
| 03 | `03-positive-dentry-clock-eviction.md` | 4096 项缓存不再整表清空 |
| 04 | `04-process-io-atomic-counters.md` | read/write 不再锁 cwd registry 计数 |
| 05 | `05-fd-slot-resource-classification.md` | FD slot 持有稳定资源分类和 flags 快照 |
| 06 | `06-unified-read-io-lease.md` | read/pread 家族单次 FD registry 查询 |
| 07 | `07-close-dup-resource-lifecycle.md` | close/dup 去除 PTY/Unix/epoll 负向探测 |
| 08 | `08-fs-lookup-token-api.md` | 建立一次 lookup 可复用的 FS/VFS 契约 |
| 09 | `09-single-lstat-symlink-walker.md` | 普通分量一次 lstat，symlink 才 readlink |
| 10 | `10-resolved-path-openat.md` | openat 消费稳定节点、metadata 和 mount identity |
| 11 | `11-resolved-path-stat-family.md` | newfstatat/statx 复用解析结果 |
| 12 | `12-exec-file-stable-handle.md` | exec 前缀、ELF 头和 PT_LOAD 共用句柄 |
| 13 | `13-riscv-lazy-elf-ab.md` | RISC-V 显式启用 lazy ELF 并完成 A/B |
| 14 | `14-loongarch-lazy-elf.md` | 解决装载后 patch 等 LA 特有约束后启用 |
| 15 | `15-mmap-pte-change-summary.md` | lazy mmap 无 PTE 变化时不 flush |
| 16 | `16-munmap-range-tlb-summary.md` | munmap 仅按实际移除 PTE 同步且不重复 fence |
| 17 | `17-file-writeback-batch-boundary.md` | 一个文件 writeback 周期只做一次后端提交 |
| 18 | `18-filesystem-persistence-boundaries.md` | flush 边界收口到 fsync/sync/O_SYNC |
| 19 | `19-root-fs-read-concurrency-contract.md` | 建立不依赖全局可变 FS 锁的只读契约 |
| 20 | `20-vfs-concurrent-stable-read-path.md` | metadata/read_range 切到稳定只读通道 |
| 21 | `21-read-staging-buffer-reuse.md` | 降低普通 read staging 分配与清零成本 |
| 22 | `22-sharded-futex-registry.md` | futex registry 分片且保持 lost-wake 防护 |
| 23 | `23-final-regression-and-handoff.md` | 完整功能、性能、文档与交接验收 |

任务必须按顺序推进；13、14、17、18、19、20、22 是高风险门禁任务，前一项未提交简报
或未满足回退标准时不得继续扩大改动。
