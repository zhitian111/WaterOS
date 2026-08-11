# K-05C 块缓存 O(1) LRU 优化报告

```text
task: K-05C block-cache O(1) LRU subtask
date: 2026-08-02
kernel_commit: 6bb3e66c + 本报告对应未提交修复
user_submodule_commit: 2f470f95fa6bf0401c4b1b7ef3bb8fc7a10b870b
architecture: RISC-V64, 8 CPU
qemu_and_firmware: QEMU 11.0.2, OpenSBI 1.7
image_sha256: dd9bbc442f990b228087f15c8da14776981eb38ee393a84a89daf39e46c119d0
overlay: 每轮使用基于干净 os/sdcard-rv-pub.img 的新 qcow2 overlay
first_failure: none in guest; host raw conversion failed with Disk quota exceeded
```

## 结论

已把驱动块缓存命中和淘汰的 LRU 维护从 O(n) 降为 O(1)。修改不改变
`BlockDevice` API、缓存容量、写穿语义或 task 模块架构。相同最小 Cargo 编译探针中，
编译时间从 39.15 秒降至 33.16 秒，整体命令从 45.51 秒降至 38.13 秒。

## 问题证据

块缓存容量为 1024 个 512 字节块。旧实现使用 `VecDeque`，每次缓存命中执行
`iter().position()` 后删除并移至队尾，因此命中更新最坏需要扫描 1024 个槽位。

临时计数器显示，最小 `cargo build` 在同一统计节点累计：

```text
logical_requests=199816
logical_blocks=1598521
cache_hit_blocks=1336393
device_read_ops=32768
device_read_blocks=262128
```

高命中率导致约 134 万次线性 LRU 搜索，成为确定的 CPU 热点。临时计数器已删除。

## 修改

- 每个固定缓存槽增加 `prev`、`next` 索引。
- 缓存对象维护 `lru_head`、`lru_tail`，命中移动、插入和头部淘汰均为 O(1)。
- 保留 `BTreeMap<Lba, slot>` 的定位方式、固定容量和连续未命中合并逻辑。
- 新增回归测试，验证命中刷新后淘汰的是实际最久未使用块。

## 验证

| 指标 | 修改前 | 修改后 | 变化 |
| --- | ---: | ---: | ---: |
| Cargo 编译 | 39.15 s | 33.16 s | -15.3% |
| bringup 整体 | 45.51 s | 38.13 s | -16.2% |
| 用户页故障 | 66,379 | 66,415 | 基本不变 |

- 块缓存单元测试：6/6 通过。
- `make rv_check`：通过。
- `make la_check`：通过。
- `git diff --check`：通过。组件 `cargo fmt --check` 会按仓库当前 rustfmt 配置重排该
  crate 的大量既有代码，本次为避免无关格式化改动没有应用该结果。
- 修改后最小工程成功生成，`BLOCK_STATS_MINIBUILD_END rc=0`。
- 未出现 panic、BadFd、SIGSEGV、OOM 或 NoSpace。
- 基线日志：`/tmp/wateros-block-stats-build.log`，SHA-256
  `8a9b1fbdab6e34618ecccd55411976f56e5795a5380d550c3f454ad76eb87513`。
- 修改后日志：`/tmp/wateros-block-lru-build.log`，SHA-256
  `751b3ca84083d8ca6b2e951cdfaa27954b856b35f531413def5b9103c642ebec`。

将 qcow2 展开为 raw 以运行 `e2fsck -fn` 时，宿主机在约 1.5 GiB 处报告磁盘配额
不足；未完成的 raw 文件已删除。该探针只在 `/tmp` ramfs 创建工程，没有执行 ext4
数据写压力，不能代替夜间镜像完整性验证。

## 剩余验收

本报告只完成 K-05C 的块缓存算法子任务。三轮 iozone、随机 I/O 退化检查、FS LTP、
完整 CAgent/BuildStorm 和最终 `e2fsck -fn` 仍需在夜间全量窗口执行。
