# BuildStorm 块缓存准入实验

## 问题

首轮缓存画像中，LBA 块缓存处理 16,544,129 个读块，但仅命中 1,081,310 个（6.54%）。
当时 16,177,711 次索引冲突掩盖了真实容量行为；main 已通过降低索引装载因子修复该问题，
并取得 1.79% 的 matched A/B 改善。因此不能直接拿修复前的 ghost/淘汰数据设计下一步策略。

文件页缓存同时表明 57.41% 的被淘汰页装入后从未再次访问。普通文件数据既进入 32 MiB
page cache，又进入独立 8 MiB LBA cache；对 cargo/rustc 的顺序扫描和新产物写入，这可能造成
重复拷贝、索引/LRU 维护和热点污染。

## Linux 对照与候选假设

Linux 的普通文件内容由 page cache 管理，buffer head / iomap 主要描述块映射和文件系统元数据，
而不是再建立一份覆盖所有文件数据的固定容量 LBA 数据缓存。WaterOS 当前块设备接口无法区分
ext4 元数据和普通文件数据，因此本阶段不做不可靠的路径特判。

参考：

- <https://docs.kernel.org/filesystems/iomap/operations.html>：buffered read/readahead 直接填充
  page cache，iomap 以更轻量的逐块状态描述替代 buffer head。
- <https://docs.kernel.org/filesystems/buffer.html>：buffer head 维护文件系统块状态，且该接口已弃用，
  新文件系统应使用 iomap。
- <https://docs.kernel.org/filesystems/iomap/design.html>：`IOMAP_DONTCACHE` 明确支持完成 buffered I/O
  后丢弃未被其他线程使用的 page-cache 数据，说明一次性 I/O 的 bypass/admission 是正常策略。

候选假设是采用有界 ghost history 做二次命中准入：首次读 miss 直接从设备返回，只在近期再次
访问同一 LBA 时才安装缓存。这样可让反复读取的 ext4 元数据和真实热点进入缓存，并让一次性
文件数据绕过 8 MiB 数据池。写入语义单独处理，避免破坏 write-through 后立即读取的正确性。

## 先诊断后实现

1. 在 current main 上仅启用 `cache-layer-diagnostics`，重新跑一轮 BuildStorm 画像。
2. 只读取 `result.json` 与最后一组 `[block-cache-diag]`、`[page-cache-diag]`、
   `[elf-cache-diag]` 汇总行，不展开正常编译日志。
3. 若修复后块缓存仍是低命中、低 ghost-refault、高容量淘汰，实施二次命中准入。
4. 若 ghost-refault 已明显升高，说明容量内存在有价值工作集，停止准入实验并转向 CLOCK/分代回收。

## 验证口径

- 诊断轮不作为墙钟成绩；普通 Final 不带任何诊断计数。
- 候选先跑定向单测、`make check`、`make all`，确保 main 的 RV/LA Final 产物约束不退化。
- 性能只跑同镜像、同 CPU 的 candidate/main A/B；第一次已明确有效便停止，首次不明确只允许
  一次补充对照，不用重复运行制造结论。
- 只有明确改善且所有功能 marker 通过的候选才合入 main。
