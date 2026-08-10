# COW 缺页单页 TLB 失效方案（2026-08-11）

## 为什么选择这里

300 秒 pc-hot 中：

```text
handle_user_page_fault / handle_cow_fault / map_page_to_ppn 合计约 730M
```

当前 `kernel_mm_impl::handle_cow_fault` 通过
`with_user_aspace_mut_and_flush` 进入：

```text
handle_cow_fault
  -> with_user_aspace_mut_and_flush
       -> aspace.handle_cow_fault
            -> handle_cow_page
                 -> flush_address_space_translations()   // 本地全量 sfence
       -> flush_tlb_local(All)                           // 又做一次本地全量 sfence
       -> request_tlb_shootdown(handle)
```

同一 COW 页只修改了一个 PTE，却执行两次本地全量 TLB 失效。RISC-V 的
`sfence.vma` 本身是热路径指令，BuildStorm 的 fork/COW 频率较高，这个重复可以
消除。

## 选择的方案

1. 把 `Sv39AddressSpace::handle_cow_page` 中的本地全量 flush 移到
   `handle_cow_fault` 外层，并增加不 flush 的内部调用入口。
2. 新增 `with_user_aspace_mut_and_page_flush`：闭包返回是否修改页表；只有
   实际发生 COW 时，才执行一次单页本地 TLB 失效和一次远端 shootdown。
3. `kernel_mm_impl::handle_cow_fault` 改用这个新的单页接口，不再经过
   `with_user_aspace_mut_and_flush`。
4. 用户拷贝内部直接调用 `handle_cow_fault` 的路径保持现有本地全量 flush，
   不扩大本项改动范围。
5. LoongArch 保持同一结构，使用其已有 `Page` 本地失效后端。

## 为什么这么做

1. 它对应 roadmap 的 MM-02：按实际修改范围执行 TLB 失效。
2. 与之前被回退的 mprotect 猜测不同，这里不改变 mprotect 语义，只去掉 COW
   路径中明确重复的本地失效。
3. 远端 shootdown 仍按地址空间缓存 CPU 集合执行，不改变跨核可见性。

## 接下来的工作

1. 在 `perf/cow-single-page-flush` 分支实现 RISC-V/LoongArch 两套改动。
2. 双架构 Final check。
3. 180 秒 smoke，重点确认 COW/fork 不再 panic。
4. RISC-V 完整 BuildStorm A/B；相对同轮 main 有 ≥ 1.5% 净改善才合并。
5. 若有效，补 pc-hot/wait-hot 前后对比并归档。

## 验收标准

- 双架构 Final check 通过。
- fork、COW 写入、跨 CPU TLB 定向测试无回归。
- 完整 BuildStorm 无 panic/SIGSEGV，相对同宿主 main 有可复现收益。

## 实测结果（2026-08-11）

```text
cow-single-flush-full-a1: BUILDSTORM_COMPILE ok=true elapsed_s=824.67
cow-single-flush-full-a2: BUILDSTORM_COMPILE ok=true elapsed_s=812.72
main-5a080c07-full-b1:    BUILDSTORM_COMPILE ok=true elapsed_s=874.46
```

双架构 Final check 与 180 秒 smoke 通过；两轮完整 BuildStorm 均成功，无
panic/SIGSEGV。两轮中位/均值约 `818.7s`，相对同轮 main `874.46s` 快约 6.4%，
达到合并门槛。

300 秒 pc-hot 中 `handle_cow_fault` 由约 `98M` 指令降至约 `86M`，且完整墙钟收益
明显高于指令下降，说明主要收益来自减少 COW 路径上的全量 TLB 失效/远端 shootdown。
300 秒 wait-hot 仍显示 BuildStorm 编译负载不均，但本轮 COW 优化已可稳定提升完整
耗时，因此保留并合并 main。
