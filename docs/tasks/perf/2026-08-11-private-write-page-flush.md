# private write 单页 TLB 失效方案（2026-08-11）

## 为什么选择这里

当前 main 已合入 COW 单页 TLB 优化，完整 BuildStorm 约 `815-818s`。继续检查
MM 路径时发现 `ensure_private_for_write` 仍在复制共享页后执行本地全量 TLB 失效：

```text
copy_to_user / mprotect
  -> ensure_private_for_write
       -> handle_cow_page / 复制共享帧
       -> flush_address_space_translations()   // 本地全量 sfence
```

该函数只修改一个用户 PTE。mprotect 外层还有
`with_user_aspace_mut_and_flush` 的本地全量 + 远端 shootdown，因此内部全量失效
至少是重复的；用户拷贝路径即使没有外层 flush，单页失效也已满足当前“仅本地失效”
的一致性范围。

## 选择的方案

把 `ensure_private_for_write` 中复制共享帧后的全量
`flush_address_space_translations()` 改为：

```rust
platform::arch::paging::flush_tlb_local(
    platform::arch::paging::TlbFlushRange::Page { addr: vpn.start_addr().0 },
);
```

- RISC-V 真正缩小到单页。
- LoongArch 后端如果仍保守实现为全量，语义不变，只影响后续架构优化。
- 不改变 `handle_cow_page` 的现有本地全量路径；本项只处理非 COW 的 shared-refcount
  private copy。

## 为什么这么做

1. 这是 MM-02 “按实际修改范围失效”的同一方向，且改动局限在一个函数。
2. 不触碰 mprotect 语义，不改变远端 shootdown 策略。
3. 与已合并的 COW 优化同模式，便于单独 A/B 验证。

## 接下来的工作

1. 在 `perf/private-write-page-flush` 分支修改 RISC-V/LoongArch 两处。
2. 双架构 Final check 与 180 秒 smoke。
3. RISC-V 完整 BuildStorm A/B，相对当前 main 有 ≥ 1.5% 净改善才合并。
4. 完成后补 pc-hot/wait-hot 分析并归档。

## 验收标准

- 双架构 Final check 通过。
- 用户写、mprotect、COW 定向路径无回归。
- 完整 BuildStorm 无 panic/SIGSEGV，相对同宿主 main 有可复现收益。

## 实测结果（2026-08-11）

```text
private-write-page-flush-full-a1:
  axbuild done (804.81s)，但 1200s 超时前未打印 BUILDSTORM_COMPILE
```

双架构 Final check 与 180 秒 smoke 通过，但完整 BuildStorm 在编译完成后长时间
停滞，未达到可验收结果。改动已全部回退，仅保留本记录。
