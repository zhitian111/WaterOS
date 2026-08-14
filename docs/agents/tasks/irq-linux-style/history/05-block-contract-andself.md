# T05 任务简报：BlockDevice 契约改 `&self` + 同步回退

- 完成日期：2026-08-15
- commit：`7bb844ec`（`[refactor] BlockDevice 契约改为 &self 并下沉内部锁`）
- 前置：`a83d717a`（T04 bottom-half）

## 实际改动摘要

`block-api/api-v0`：

- `BlockDevice: Send + Sync`；`read_blocks/write_blocks/flush/read_bytes/read_prefix`
  全部改为 `&self`。
- `SharedBlockDevice = Arc<dyn BlockDevice>`（移除外层 `Mutex<Box<...>>`）。
- 去掉不再使用的 `boxed::Box` 导入；`test()` 同步更新。

驱动实现：

- `impl-virtio-mmio` / `impl-virtio-pci`：`inner` 改为 `Mutex<VirtIOBlk<...>>`，
  方法经 `self.inner.lock()` 串行化；PCI crate 补 `spin` 依赖。
- `impl-block-cache`：拆分 `CacheState`（原全部缓存字段）与
  `CachingBlockDevice { inner, state: Mutex<CacheState> }`；`read_blocks` 保持
  持锁调用 backend（T05 语义与旧外层 Mutex 等价，T07 再拆锁）。
  `manager::wrap` 直接返回 `Arc<dyn BlockDevice>`，`flush_all` 去 `.lock()`。
- 两个 `driver-impl` 的 `register.rs` 非缓存路径改为 `Arc::new(dev)`；
  LoongArch `test.rs` 去掉 `.lock()`。

FS/VFS 适配：

- `impl-another-ext4/block_io.rs`、`operations.rs`、`impl-ext4-rs/core.rs`、
  `impl-ext4/lib.rs`、`user_graphics.rs` 全部去掉 `device.lock()`，改直接调用
  `&self` 方法（ext4-rs `write_partial_block` 参数改 `&dyn BlockDevice`）。

## 验证命令与结果

- `cargo test --offline -p wateros-driver-block-impl-block-cache`：10 passed
  （调试中发现并修复测试侧同一 spin Mutex 单表达式二次 lock 的自死锁）。
- `cd os && make rv_check`：通过。
- `cd os && make la_check`：通过。
- RISC-V QEMU smoke（QEMU 9.2.1 + `-snapshot`）：
  - virtio-blk 注册、`probe matched impl=another-ext4 kind=Ext4`、`[fs] init end`、
    `ext4 root mounted (RW)`、cagent_testcode.sh `exit_code=0`；
  - 无 panic / fatal / `[irq] unhandled`。
- `git diff --check`：干净。

## 未验证项 / 风险 / 下一步

- 锁粒度仍是整设备串行（与旧外层 Mutex 等价），并发 miss 不会并行 backend I/O；
  这是 T07 锁拆分的目标。
- 未做写路径专项 QEMU 测试（cagent 覆盖了读取与部分写入）；完整 iozone/BuildStorm
  在 T12 性能验收统一跑。
- 下一步 T06：RISC-V virtio-MMIO 块 request_queue + submit/complete + 多请求在途
  （首次真正使用 PLIC + bottom-half）。
