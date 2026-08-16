# Task 04 简报：回收空 slab 页并修正内存统计

## 完成情况

实现了小对象 slab 页的完整回收状态机。页只有在所有对象已经归还、remote-free 已由
owner CPU drain，且已经从 `current` / `partial` 引用中摘除后，才会失效 header 并归还
frame source。RISC-V 通过 per-CPU frame batch 回收，LoongArch64 直接归还全局 frame pool。

每个 CPU/class 最多保留一个 empty current 和一个 detached warm reserve。第一次实现只
保留一个空页，完整 BuildStorm 为 `571.95s`，相对 Task 01 的 `542.97s` 退化 `5.337%`，
触发任务门禁。将迟滞调整为上述两页上限，并把生产页统计限制到 refill/reclaim 和 detached
reserve 状态变化后，有效重试为 `553.55s`，相对 Task 01 退化 `1.949%`，满足本任务允许的
5% 阈值。

`heap_mem_stats()` 现在分别报告 boot heap、slab retained/reclaimable 和 frame pool，避免
把 boot TLSF 容量误当作全部内核内存。`slab-diagnostics` 增加累计 frame reclaim 计数。

## 主要文件

- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/{cpu_slab,slab_cache,slab_page,page_stats}.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/{lib,backend_tlsf,backend_linked_list,interrupt_guard}.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/{Cargo.toml,README.md}`
- `os/components/wateros-runtime/README.md`
- `os/src/main.rs`

## 功能验证

host 单测覆盖多页回收、partial 页保留、remote drain 前不回收、frame 复用、重复归还检测，
以及每个 size class 10,000 次分配/释放：

```text
cargo test --lib --no-default-features --features impl-tlsf,impl-slab
cargo test --lib --no-default-features --features impl-tlsf,impl-slab,slab-diagnostics
cargo test --lib --no-default-features --features impl-linked-list-allocator,impl-slab
以上三组均为 12 passed, 0 failed
```

两架构静态与构建验证：

```text
make rv_check HEAP_ALLOCATOR_FEATURE=heap-slab                         PASS
make la_check HEAP_ALLOCATOR_FEATURE=heap-slab                         PASS
make rv_check HEAP_ALLOCATOR_FEATURE=heap-slab EXTRA_FEATURES=slab-diagnostics PASS
make la_check HEAP_ALLOCATOR_FEATURE=heap-slab EXTRA_FEATURES=slab-diagnostics PASS
make kernel-rv-final HEAP_ALLOCATOR_FEATURE=heap-slab                  PASS
make kernel-la-final HEAP_ALLOCATOR_FEATURE=heap-slab                  PASS
git diff --check                                                       PASS
```

产物 SHA-256：

```text
90cd0e7d41db8de3c65f408117655155f8e65e9fc35c4012ca8e9bb48a12b239  kernel-rv-final
08d6a7e3b7d3d90b6899b564a8c14299dea2a2f51de5d5bfc879e950455169d4  kernel-la-final
```

## RISC-V BuildStorm

固定 QEMU 9.2.1、16 GiB、8 vCPU、`-snapshot`。每轮均从原 gzip 生成新 raw 镜像，镜像内
脚本 mode 为 `0755`，SHA-256 为
`84d631012532e6817565cba02d35d8a2721c5ec7787a1e0519d6d0ae0a4274bb`。

```text
main/TLSF baseline:       549.68s
Task 01 slab:             542.97s
Task 04 one-page policy:  571.95s  (+5.337% vs Task 01, rejected)
Task 04 tuned retry:      553.55s  (+1.949% vs Task 01, +0.704% vs main)

TOOLCHAIN_RESULT status=OK
MINIBUILD_RESULT status=OK
BUILDSTORM_RESULT status=OK rc=0 cores=8 elapsed_s=553.55 run=OK
HOST_ELAPSED_S=594 HOST_EXIT=0
all commands finished
```

调优候选的首次运行在 guest `cargo xtask` 已打印最终 artifact 路径后未退出，最终触发
`HOST_EXIT=124`，没有输出 `BUILDSTORM_RESULT`，也没有 panic/OOM/SIGSEGV。相同内核和全新
镜像的唯一重试完整成功，因此该轮作为偶发收尾停滞保留记录，不计为性能样本；后续功能验收
仍需留意进程退出/等待链路。

有效日志为 `/tmp/wateros-task04-slab-rv-buildstorm-retry.log`，超时日志为
`/tmp/wateros-task04-slab-rv-buildstorm-tuned.log`。实验 raw 镜像均已删除，原始 gzip 未修改。

## 结论与后续风险

Task 04 达到功能门禁和阶段性能阈值，可以进入 Task 05。当前候选仍比 main 慢 `0.704%`，
不满足最终性能验收；Task 05 及后续链路优化必须消除这一差距。偶发的 guest 子进程收尾停滞
尚未稳定复现，最终功能验收不得忽略该风险。
