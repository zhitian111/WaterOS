# 性能任务：启用并扩容块缓存（RV + LA）

## 任务目标

在 **RISC-V 与 LoongArch** 评测构建中真正启用写穿块缓存（`CachingBlockDevice`），并适度扩容容量；可选实现 write-allocate，使 iozone 读/写项有机会 **score > 1.0**。

**本任务只做块设备层与 Cargo feature 接线**，不改 ext4/VFS 逻辑（dcache 见 `wave2-fs-read-path.md`）。

## 背景（必读）

- `docs/todo/perf-baseline-gap-report.md` §G2.1
- 现状：RV `os/Cargo.toml` 的 `qemu-riscv64-opensbi` **未**包含 `driver/impl-block-cache`；LA `impl-qemu-loongarch64-virt` **无** `BlockCacheManager::wrap`；`BLOCK_CACHE_CAPACITY_BLOCKS=64`（仅 32KiB）。

## 执行前必须参考的 prompt

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`

## 执行前必须参考的文档

- `docs/todo/perf-fs-vfs.md`（F-9、F-8）
- `docs/todo/perf-risk-assessment.md`（F-9 风险：低）
- `docs/exports/features/wateros-driver.md`（若存在）

## 需要优先查看的源文件

| 文件 | 用途 |
|------|------|
| `os/Cargo.toml` | `qemu-riscv64-opensbi` / `qemu-loongarch64-virt` feature 列表 |
| `os/components/wateros-driver/Cargo.toml` | `impl-block-cache` feature 接线 |
| `os/components/wateros-driver/driver-impl/impl-qemu-riscv64-opensbi/src/lib.rs:268-275` | RV 已有 `#[cfg(feature="block-cache")]` wrap |
| `os/components/wateros-driver/driver-impl/impl-qemu-loongarch64-virt/src/lib.rs:129-134` | LA 当前裸 `Arc<Mutex<dev>>` |
| `os/components/wateros-driver/driver-impl/impl-qemu-loongarch64-virt/Cargo.toml:27-29` | LA 已有 `block-cache` feature 未用 |
| `os/components/wateros-driver/driver-block/block-impl/impl-block-cache/src/lib.rs` | 写穿、连续 miss 合并读 |
| `os/components/wateros-base/base-config/src/fs.rs:34` | `BLOCK_CACHE_CAPACITY_BLOCKS` |

## 实施要点

1. RV：在 `os/Cargo.toml` 对应 platform feature 加入 `"driver/impl-block-cache"`。
2. LA：在 probe virtio-blk 处对齐 RV 的 `BlockCacheManager::wrap`；在 `driver/Cargo.toml` 的 `impl-block-cache` 中追加 LA 子 feature。
3. 将 `BLOCK_CACHE_CAPACITY_BLOCKS` 提到 **256~1024**（按 RAM 预算，注释说明评测镜像假设）。
4. （可选）`write_blocks` 后对写入 LBA **write-allocate** 入缓存（`impl-block-cache/src/lib.rs:210-227`），利于 re-reader/rewriter。
5. 保持写穿语义，不引入 write-back 脏数据风险。

## 验收标准

- [ ] `make rv_check && make la_check` 通过
- [ ] 启动日志可见块设备经缓存包装（或单元测试 `impl-block-cache` 仍绿）
- [ ] 两架构 virtio blk 探测/读写不回归（LTP 文件类抽样或 bringup）
- [ ] 改动范围限于 driver + base-config 常量，无无关重构

## 完成后的回填

- 在 `docs/todo/perf-baseline-gap-report.md` 或 PR 描述中注明「块缓存已默认启用」
- 若改容量常量，在 `base-config` 旁注评测依据

## 任务完成自检清单

- [ ] RV、LA 构建均走 `CachingBlockDevice` 路径（grep 确认 feature 链）
- [ ] 未破坏 LA PCI / RV MMIO 各自 probe 逻辑
- [ ] 未默认开启 write-back
- [ ] 已跑静态检查

## 示例：交给 Agent 的一次性用户 prompt

```
@docs/tasks/perf/wave1-enable-block-cache.md

请按任务文件启用 RV/LA 块缓存并扩容 BLOCK_CACHE_CAPACITY_BLOCKS。
改完后 make rv_check && make la_check。不要改 ext4/VFS。
```
