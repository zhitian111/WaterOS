# Task 05 简报：remote-free 有界分流与 owner 维护

## 完成情况

Task 03 显示 BuildStorm 有约 71.6 万次 remote free，旧实现把一个 owner CPU 的所有 size
class 混入同一个 Treiber 栈，并在任意一次本地分配时完整 drain；单次最大批量达到 9,174 个
对象。当前实现改为每个 `owner CPU + size class` 一条原子栈，同 class 分配只处理对应队列，
单次最多 pop 256 个对象。每个 owner 维护 pending class bitmap 和轮转 cursor，timer tick 每次
最多推进一个 pending class，因此 owner 不再分配该 class 时也能继续回收。

remote push 不分配内存、不记录日志、不取得阻塞锁。队列头是对象所有权的事实来源，pending
bit 仅作维护提示；清位后重新检查队列头，覆盖与 remote push 的并发窗口。只有 owner CPU
执行 pop，本地 allocator 临界区关闭中断；已入队对象在 drain 前不会重新分配，因此不会形成
Treiber ABA。完全空闲页沿 Task 04 的回收路径归还共享 frame pool，随后可由其它 CPU 取得；
仍含活对象的页继续由原 owner 管理。

## 功能与构建验证

三种 host 配置均为 `14 passed, 0 failed`：

```text
cargo test --lib --no-default-features --features impl-tlsf,impl-slab
cargo test --lib --no-default-features --features impl-tlsf,impl-slab,slab-diagnostics
cargo test --lib --no-default-features --features impl-linked-list-allocator,impl-slab
```

新增测试覆盖：两个 class 隔离、队列长度超过 256、分配触发有界 drain、timer 清空 owner
不再使用的 class、四个 remote CPU 并发 push、对象恰好一次复用、页回收和 frame 再利用。

与 Task 05a 最终候选一起完成的双架构门禁：

```text
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check          PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check          PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final   PASS
git diff --check                                        PASS
```

```text
3e77275c53d5478e1ca63499c7f70fc4acbb703f0f665caabb3fdfef9260a356  kernel-rv-final
707776fb2f72459312e9163a2b56fa71c0cf34ba4247f98631d952d232811498  kernel-la-final
```

## 诊断结果

RISC-V diagnostics Final 在 QEMU 9.2.1、16 GiB、8 vCPU、全新 raw 镜像上完整成功：

```text
BUILDSTORM_RESULT elapsed_s=554.55 status=OK run=OK
remote push CAS miss:       44
drain events:          175,160
drained objects:       695,767
maximum drain:             256
pop CAS miss:              142
drain-limit hits:          107
frame refills:          13,937
frame reclaims:          8,416
retained pages:          5,521
empty pages:                80
OOM:                         0
```

日志：`/tmp/wateros-task05-diag-rv-full.log`。最大 drain 从 Task 03 的 9,174 限制为 256，
只有 107 次触顶；约 69.6 万 remote 对象全部由 owner 分 class 推进。普通 Final 不包含
`[heap][slab-diag]`、诊断计数更新或汇总符号。

## RISC-V 性能门禁

固定 QEMU 9.2.1、16 GiB、8 vCPU 和 `-snapshot`。有效轮从
`sdcard-rv-pub.img.gz` 重新解压 raw，覆写后的官方脚本 mode 为 `0755`，回读 SHA-256 为
`84d631012532e6817565cba02d35d8a2721c5ec7787a1e0519d6d0ae0a4274bb`。

```text
main TLSF baseline:       549.68s
Task 04 slab:             553.55s
Task 05 slab:             551.32s

Task 05 vs Task 04:        -2.23s  (-0.403%)
Task 05 vs main:           +1.64s  (+0.298%)
```

有效轮同时包含：

```text
TOOLCHAIN_RESULT status=OK
MINIBUILD_RESULT status=OK
[axbuild] ... done (534.59s)
BUILDSTORM_RESULT mode=multi status=OK rc=0 cores=8 elapsed_s=551.32 ... run=OK
#### OS COMP TEST GROUP END buildstorm-glibc ####
[busybox-bringup] all commands finished
```

日志：`/tmp/wateros-task05-slab-rv-retry-after-exit-fix.log`。没有 panic、OOM、ENOMEM、
SIGSEGV、double free、递归 heap、超时或残留 QEMU。此前普通轮在 `axbuild ... done (541.31s)`
后未退出，已由独立 Task 05a 修复；该失败轮不计为性能样本。

Task 05 相对 Task 04 未退化且略有改善，满足“不退化超过 2%”的阶段门槛。当前仍比 main
慢 0.298%，尚未满足最终性能要求；下一步进入 Task 06 的单核/SMP allocator 归因。
