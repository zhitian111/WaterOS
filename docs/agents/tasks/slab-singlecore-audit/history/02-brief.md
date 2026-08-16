# Task 02 简报：slab 路由与 fallback guard 实验回退

## 完成情况

实现并审计了以下候选改动：在 `GlobalAlloc` 入口只解析一次 `SizeClass`，将解析结果传入
slab；非 slab 且 large-frame 关闭时直接进入 boot backend；slab miss 在已持有的 allocator
interrupt guard 内进入 TLSF，退出 guard 后再执行高水位诊断。TLSF 与 linked-list 两个后端
均实现了相同的“guard 已持有”入口。

候选实现通过静态、构建、机器码和实际 workload 功能验收，但单轮 RISC-V BuildStorm 相对
Task 01 退化 2.74%，超过任务规定的 2% 回退线。因此候选源码已全部回退，本提交只保留
实验事实，不改变 Task 01 后的 allocator 行为。

## 静态与构建验证

```text
make rv_check HEAP_ALLOCATOR_FEATURE=heap-slab              PASS
make la_check HEAP_ALLOCATOR_FEATURE=heap-slab              PASS
make rv_check HEAP_ALLOCATOR_FEATURE=heap-linked-list       PASS
make la_check HEAP_ALLOCATOR_FEATURE=heap-linked-list       PASS
make kernel-rv-final HEAP_ALLOCATOR_FEATURE=heap-slab       PASS
make kernel-la-final HEAP_ALLOCATOR_FEATURE=heap-slab       PASS
rustc --edition 2024 --test .../slab/size_class.rs          PASS (5/5)
git diff --check                                             PASS
```

候选产物：

```text
8f0120f044646fc93fb628b1714873c723321d0272da5ff7c42d00136676d2f2  kernel-rv-final
e728bf30bcdcef947424e3a381dfb69f0a61cc7b2a0f013f499fe0e84a1a57f4  kernel-la-final
```

RISC-V 反汇编中，`__rust_alloc` 对 slab layout 只有一次 class 表查询；非 slab 分支直接
调用 TLSF 的单层 guard；slab 分支只调用外层 guard，fallback 在闭包内直接取得 TLSF 锁。
dealloc 同样只在 slab/large 路径需要时进入前置 guard。说明实现达到了机器码层面的预期，
但这不足以抵消或稳定改善完整 BuildStorm 链路。

## RISC-V 功能与性能

固定 QEMU 9.2.1、16 GiB、8 vCPU、`-snapshot`。raw 镜像从 pub gzip 重新解压，恢复脚本
SHA-256 为 `84d631012532e6817565cba02d35d8a2721c5ec7787a1e0519d6d0ae0a4274bb`，
mode 为 `0755`。

```text
main/TLSF baseline: BUILDSTORM_RESULT elapsed_s=549.68 status=OK run=OK
Task 01 slab:       BUILDSTORM_RESULT elapsed_s=542.97 status=OK run=OK
Task 02 candidate:  BUILDSTORM_RESULT elapsed_s=557.84 status=OK run=OK

Task 02 vs main:    +8.16s (+1.48%)
Task 02 vs Task 01: +14.87s (+2.74%)
full host elapsed:  595s, exit=0
```

候选日志为 `/tmp/wateros-task02-slab-rv-buildstorm.log`。`TOOLCHAIN_RESULT`、
`MINIBUILD_RESULT` 和 `all commands finished` 均存在，无 panic、OOM、ENOMEM、SIGSEGV、
错误退出或产物缺失。一次性 raw 镜像已删除。

## 结论

本任务按预设门禁判定为性能失败并回退，不把单轮波动解释成优化成功。后续任务从 Task 01
的 `c2a07cd3` allocator 状态继续；Task 03 的诊断默认关闭，不允许把新的生产热路径计数
重新引入。
