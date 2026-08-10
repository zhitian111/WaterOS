# page cache 多页命中批量复制方案（2026-08-11）

## 为什么选择这里

当前 300 秒 pc-hot 中：

```text
GlobalFilePageCache::read_key      ~80M
GlobalFilePageCache::install_page  ~190M
FileCacheKey::cmp                  ~200M
```

`read_key` 对一次跨页用户读会逐页执行：

```text
install_page -> lock cache -> BTreeMap get -> copy 4 KiB -> unlock
```

如果这些页已经在 cache 中，完全没有必要逐页重复 lock、BTree lookup 和 LRU touch。
BuildStorm 编译读取大量连续文件数据，多页命中路径值得单独优化。

## 选择的方案

在 `read_key` 开头增加一个批量命中快速路径：

1. 一次锁定 `GlobalCacheState`。
2. 对请求涉及的页逐个查 `index`；只要连续页都在，就在锁内直接复制到用户缓冲。
3. 如果中途遇到 missing 页，立即释放锁并回退到现有逐页 `install_page` 路径。
4. 快速路径不调用 `touch_lru`；批量命中不改变 LRU 顺序，避免热路径更新侵入式链表。
5. 保持 `last_read_end_page` 顺序读检测和 read-ahead 逻辑不变。

## 为什么这么做

1. 这是“减少重复锁与 BTree 查找”，不是新增缓存或复制。
2. 与之前失败的 path bulk copy 不同，这里没有过量预读，只复制请求内已缓存页。
3. 不改变缺页、eviction、rename、truncate 或 dirty 语义。

## 接下来的工作

1. 在 `perf/page-cache-bulk-hit` 分支实现快速路径。
2. 双架构 Final check 与 180 秒 smoke。
3. RISC-V 完整 BuildStorm A/B；相对当前 main 有 ≥ 1.5% 净改善才合并。
4. 完成后补 pc-hot/wait-hot 并归档。

## 验收标准

- 双架构 Final check 通过。
- 普通 read、跨页 read、cache miss fallback、read-ahead 无回归。
- 完整 BuildStorm 无 panic/SIGSEGV，相对同宿主 main 有可复现收益。

## 实测结果（2026-08-11）

```text
page-cache-bulk-hit-full-a1: BUILDSTORM_COMPILE ok=true elapsed_s=809.37
page-cache-bulk-hit-full-a2: BUILDSTORM_COMPILE ok=true elapsed_s=820.19
main-cow-full-b1:            BUILDSTORM_COMPILE ok=true elapsed_s=817.27
```

两轮完整 BuildStorm 均成功，无 panic/SIGSEGV，但相对 main 没有稳定净收益。
实现已全部回退，仅保留本记录。
