# Task 03 简报：增加低扰动 slab 诊断

## 完成情况

新增独立 `slab-diagnostics` feature。启用时，每个 CPU 拥有 cache-line 对齐的统计槽，
记录 16 个 size class 的 alloc、local hit/free、remote free、frame refill、页数和空页数，
以及 fallback、OOM、remote CAS miss、drain 次数/对象数/最大长度。owner CPU 在 allocator
interrupt guard 内使用原子 load/store 更新自己的槽位，不使用全局 `fetch_add`；采样率为
1/1。bring-up 自动队列结束时才一次性输出 per-CPU 与总计汇总。

普通 Final 不编译诊断模块、字段、更新调用、日志字符串或汇总符号。feature 从顶层
`wateros` 经 `wateros-runtime` 传播到 `runtime-heap-allocator`，并同步更新项目、内核、
Makefile 工具和 allocator README。

## 静态与构建验证

以下配置均通过：

```text
make rv_check HEAP_ALLOCATOR_FEATURE=heap-slab
make la_check HEAP_ALLOCATOR_FEATURE=heap-slab
make rv_check HEAP_ALLOCATOR_FEATURE=heap-slab EXTRA_FEATURES=slab-diagnostics
make la_check HEAP_ALLOCATOR_FEATURE=heap-slab EXTRA_FEATURES=slab-diagnostics
make rv_check HEAP_ALLOCATOR_FEATURE=heap-linked-list EXTRA_FEATURES=slab-diagnostics
make kernel-rv-final HEAP_ALLOCATOR_FEATURE=heap-slab
make kernel-la-final HEAP_ALLOCATOR_FEATURE=heap-slab
make kernel-rv-final HEAP_ALLOCATOR_FEATURE=heap-slab EXTRA_FEATURES=slab-diagnostics
make kernel-la-final HEAP_ALLOCATOR_FEATURE=heap-slab EXTRA_FEATURES=slab-diagnostics
git diff --check
```

产物：

```text
c34dae02fc5fa604c7c01cd6e0acab5566f6d099534715935f451a47c6601725  RV default
2a05b8b11f8131fdbcc9f929f532b70181698cd565d36826a1e2293f1a4b968a  LA default
fae6e86ab9365c19c8e262eb466d5ad97745ca1e8520860916bcb53d74fa4989  RV diagnostics
0e796428537df251a7ca4fde4d3384ef45f65a7bd20525f9b88ee2636ce4f126  LA diagnostics
```

`llvm-nm` 和 `strings` 确认 RV default 不含 `SlabDiagnostics`、`log_diagnostics` 或
`[heap][slab-diag]`；diagnostics Final 包含对应符号和日志。RISC-V 反汇编确认诊断计数
使用 `ld`/`sd`，没有为计数引入 `amoadd`。LoongArch64 按既定策略只做 check/build，不跑
长时间性能 workload。

## RISC-V 运行验证

固定 QEMU 9.2.1、16 GiB、8 vCPU、`-snapshot`。每轮 raw 镜像均从 pub gzip 重新解压，
覆写脚本 SHA-256 为
`84d631012532e6817565cba02d35d8a2721c5ec7787a1e0519d6d0ae0a4274bb`，mode 为
`0755`。

120 秒观察轮到达 `TOOLCHAIN_RESULT status=OK`、`MINIBUILD_RESULT status=OK` 和
`BUILDSTORM_BEGIN mode=multi`，持续正常编译，之后由宿主按计划以退出码 124 终止。完整轮：

```text
BUILDSTORM_RESULT elapsed_s=569.89 status=OK run=OK
HOST_ELAPSED_S=610 HOST_EXIT=0
all commands finished
```

没有 panic、OOM、ENOMEM、SIGSEGV、递归分配或错误产物。完整日志为
`/tmp/wateros-task03-diag-rv-full.log`；120 秒日志为
`/tmp/wateros-task03-diag-rv-120s.log`。一次性 raw 镜像均已删除。诊断耗时不作为普通 Final
性能结论。

## 诊断结果

总计：

```text
slab allocs:       42,747,281
local hits:        42,738,772 (99.9801%)
frame refills:          8,509
pages retained:         8,509
empty pages:            2,847 (33.46%)
fallbacks:          1,938,721
OOM:                        0
remote frees:         715,538
drained objects:      715,442
drain events:         150,580
max drain length:       9,174
remote CAS misses:          76
```

最明显的空页集中在：

| size class | pages | empty pages | empty ratio |
|:--|--:|--:|--:|
| 64 B | 632 | 397 | 62.82% |
| 384 B | 1,917 | 679 | 35.42% |
| 768 B | 1,531 | 1,176 | 76.81% |
| 1,024 B | 385 | 312 | 81.04% |

1,536 B 的 2,393 页仅有 13 页为空，主要对应仍存活的大对象，不能靠空页回收解决。remote
CAS miss 极少，说明当前主要瓶颈不是 push 竞争；但最大 drain 长度达到 9,174，Task 05 仍需
检查批量 drain 尾延迟。

## 结论

数据满足 Task 04 的结构改造门禁：优先实现“每 class 保留少量热空页，其余归还 frame
allocator”，目标是回收 64/384/768/1024 B class 的大量空页。Task 05 应关注 drain 批量
上限和队列滞留，而不是增加更复杂的 CAS 结构。
