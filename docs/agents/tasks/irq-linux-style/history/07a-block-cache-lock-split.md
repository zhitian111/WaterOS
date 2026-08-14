# T07-a 任务简报：块缓存 miss 读取移出缓存锁（锁拆分第一片）

- 完成日期：2026-08-15
- commit：`0860b957`（`[refactor] 块缓存 miss 读取移出缓存锁并回查装填`）
- 前置：`640145d6`（T06 IRQ 完成路径基础设施）

## 实际改动摘要

`impl-block-cache` 的 `CachingBlockDevice::read_blocks` 拆为三段：

1. 锁内扫描命中/未命中区间（命中段直接服务缓存）；
2. **锁外**调用 backend `read_blocks`（期间其它任务可命中缓存或提交新请求）；
3. 重取锁，对每个块**回查** `map`：已被其它线程安装则跳过，否则
   `admit_read_miss` 装填。

目的：消除「跨 backend I/O 持缓存锁」，为 T06 的任务睡眠（提交后释放全部
锁、IRQ/bottom-half 唤醒后回查）铺路。当前 FS 层 `SharedRwFs` 仍是互斥串行，
因此本片在现状下行为等价（10 个既有单测全部通过）；并发回查逻辑新增宿主测试
验证。

## 验证

- `cargo test --offline -p wateros-driver-block-impl-block-cache`：11 passed
  （含新增 `concurrent_miss_reads_install_consistently`：两线程并发 miss，断言
  数据一致、后续读取命中缓存、backend 不重复读）。
- `make rv_check` / `make la_check`：通过。
- `git diff --check`：干净。

## 下一步（T07-b/c）

- b：`another_ext4` 的 block-cache 锁在 backend read 前释放并回查（vendor 补丁，
  需谨慎）。
- c：`SharedRwFs` 由 `Mutex` 改为读写锁、FS API 读侧 `&self` 并发——这是让
  T06 任务睡眠与多请求在途成立的最后一块，也是历史实验里最易踩坑的部分。
- 期间遵守「后台有 QEMU 不开新 QEMU」：块缓存锁拆分用宿主测试验证，QEMU 级
  验证等用户测试窗口空闲时补跑。
