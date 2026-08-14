# T07-b 任务简报：another_ext4 块缓存 miss 读释放缓存锁

- 完成日期：2026-08-15
- commit：`c5b006c1`（`[fix] another_ext4 块缓存 miss 读释放缓存锁并回查装填（T07-b）`）
- 前置：`02fd912f`（T07-c SharedRwFs 读写锁）

## 实际改动摘要

`os/vendor/another_ext4/src/ext4_defs/cache.rs`（vendor 补丁，根因明确位于 vendor
缓存锁跨 backend I/O）：

- `BlockCache::read_block` 改为三段：锁内命中检查 + 被逐脏块写回（持锁保序）→
  **锁外 backend 读** → 重取锁回查（其它线程已安装则保留现有条目）后装填。
- 写回（eviction/flush）与 `write_block` 保持持锁：写路径暂不引入异步睡眠，
  T06 异步路径后续只对读启用。
- 新增并发 miss 一致性测试（`concurrent_miss_reads_are_consistent`）：
  两线程并发 miss 同一块，断言数据一致且后续读取命中缓存、backend 读次数不再
  增长。

## 验证

- `cargo test --offline --features block_cache`（another_ext4）：4 passed
  （含新增并发测试）。
- `make rv_check` / `make la_check`：通过。
- `git diff --check`：干净。

## 未验证项 / 风险 / 下一步

- QEMU 级回归未跑（后台用户 QEMU 运行中）：T07-a/b/c 三处锁拆分叠加后首次跑
  cagent/BuildStorm 需确认无数据竞争/回归，窗口空闲时执行。
- T07 锁拆分结构已完成（块缓存 + vendor 缓存 + SharedRwFs）。下一步：T06 升级为
  「任务睡眠」等待（提交后释放全部锁、唤醒后回查），并对读启用 IRQ 模式重新
  评估。
