# Task 01 简报：移除 slab 热路径全局统计原子操作

## 完成情况

删除 `SLAB_ALLOC_COUNT`、`SLAB_DEALLOC_COUNT` 及无人调用的 `heap_slab_stats()` 导出。
生产 slab 的每次成功 alloc/free 不再对同一全局缓存线执行 `fetch_add`；后续低扰动统计由
Task 03 的独立诊断 feature 提供。同步更新 runtime heap allocator README，不再承诺生产
热路径计数接口。

涉及文件：

- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/mod.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/README.md`

## 静态与构建验证

```text
make rv_check HEAP_ALLOCATOR_FEATURE=heap-slab       PASS
make la_check HEAP_ALLOCATOR_FEATURE=heap-slab       PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final PASS
git diff --check                                      PASS
```

产物：

```text
1174123f971c58395a8bd3593c20780c3636b3496c0ee3fd637d4c92eacd0b4d  kernel-rv-final
035ced77a61ad48aca4c1d8852c85b13acae31faabcded363b3eb3fb4b570982  kernel-la-final
```

两个内核的 `llvm-nm` 均不含 `SLAB_ALLOC_COUNT` 或 `SLAB_DEALLOC_COUNT`。RISC-V
`slab::alloc_on`/`slab::dealloc_on` 反汇编不含 `amoadd`；仍可看到 remote-free 队列自身
正确性所需的 `amoswap.d.aqrl`，不属于本次删除的统计 RMW。

局部 host `cargo test` 未进入用例：`impl-riscv64` 依赖的 `sbi-rt` 在 x86_64 host 编译时
因 `a0`...`a7` 寄存器不可用而失败。两架构目标 check/build 与下述实际 QEMU workload
覆盖了本次生产路径。

## RISC-V 功能与性能

固定 QEMU 9.2.1、16 GiB、8 vCPU、`-snapshot`。raw 镜像从原始 pub gzip 重新解压，
覆写脚本后 guest 文件 SHA-256 与
`buildstorm_testcode.recovered.sh` 的
`84d631012532e6817565cba02d35d8a2721c5ec7787a1e0519d6d0ae0a4274bb` 一致，mode 为
`0755`。

```text
main/TLSF baseline: BUILDSTORM_RESULT elapsed_s=549.68 status=OK run=OK
Task 01 slab:       BUILDSTORM_RESULT elapsed_s=542.97 status=OK run=OK
delta:              -6.71s (-1.221%)

Task 01 TOOLCHAIN_RESULT status=OK
Task 01 MINIBUILD_RESULT status=OK
Task 01 full script elapsed=577.672s, all commands finished
```

候选日志为 `/tmp/wateros-task01-slab-rv-buildstorm.log`。没有 panic、OOM、ENOMEM、
SIGSEGV、recursive allocation、double free 或错误产物；QEMU 正常退出 0。实验 raw 镜像
已删除，候选内核保存在本地
`os/.perf-task01-remove-slab-counters/kernel-rv-final`。

性能数据是用户要求的单轮 RISC-V 结果，且 main/TLSF 与任务分支 slab 还包含 allocator 和
既有分支优化差异，因此只能证明当前候选方向优于当前 main，不能把全部 `1.221%` 独立归因
于计数器删除。Task 01 已满足进入 Task 02 的功能门禁与“无稳定回退”门槛。
