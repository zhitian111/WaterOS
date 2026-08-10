# fork COW 消除重复本地全量 TLB 失效（2026-08-11）

## 为什么选择这里

刚验证的 COW 单页 TLB 优化把完整 BuildStorm 从约 `874.46s` 降到约 `818.7s`。
同一调用链里还剩下一个同类重复：

```text
fork_user_aspace
  -> with_user_aspace_mut_and_flush
       -> parent.fork_cow()
            -> flush_address_space_translations()   // 本地全量 sfence
       -> flush_tlb_local(All)                      // 又做一次本地全量 sfence
       -> request_tlb_shootdown(handle)
```

BuildStorm 会创建大量进程/线程，`fork` 是高频路径。`fork_cow` 修改的是父地址空间
整棵页表树，外层聚合接口本来就会执行一次本地全量失效和远端 shootdown，因此
`fork_cow` 内部的本地全量 flush 是重复的。

## 选择的方案

删除 `Sv39AddressSpace::fork_cow` / `LoongArch64AddressSpace::fork_cow` 末尾的
`flush_address_space_translations()`。

- `kernel_mm_impl::fork_user_aspace` 仍通过
  `with_user_aspace_mut_and_flush` 完成唯一一次本地全量 TLB 失效和远端 shootdown。
- `user_access.rs` 的 fork/COW 测试不依赖 TLB flush，只直接检查物理页内容。
- 不改变 `fork_cow` 的页表复制、COW 标记、frame refcount 或错误路径。

## 为什么这么做

1. 与刚验证的 COW 优化同属 MM-02：把 TLB 失效收敛到实际拥有地址空间句柄的聚合层。
2. 不重复此前已回退的 mprotect/munmap/brk 实验，只处理 `fork_user_aspace` 明确
   由 `with_user_aspace_mut_and_flush` 包裹的路径。
3. 改动极小，双架构对称，风险集中在“是否仍有调用方绕过外层 flush”。

## 接下来的工作

1. 在 `perf/fork-single-tlb-flush` 分支删除两架构 `fork_cow` 内的重复 flush。
2. 双架构 Final `make check`。
3. 180 秒 smoke，重点确认 fork/exec 无回归。
4. RISC-V 完整 BuildStorm A/B；相对当前 `main` 有 ≥ 1.5% 净改善才合并。
5. 完成后跑 pc-hot/wait-hot 并归档。

## 验收标准

- 双架构 Final check 通过。
- fork/COW/exec 定向路径无回归。
- 完整 BuildStorm 无 panic/SIGSEGV，相对同宿主 main 有可复现收益。

## 实测结果（2026-08-11）

```text
fork-single-flush-full-a1: BUILDSTORM_COMPILE ok=true elapsed_s=815.50
main-cow-full-b1:          BUILDSTORM_COMPILE ok=true elapsed_s=817.27
```

双架构 Final check 与 180 秒 smoke 通过；完整 BuildStorm 成功，无 panic/SIGSEGV。
相对当前 main（已含 COW 优化）只快约 `1.77s`（0.22%），落在运行噪声内，未达到
1.5% 合并门槛。代码已全部回退，仅保留本记录。
