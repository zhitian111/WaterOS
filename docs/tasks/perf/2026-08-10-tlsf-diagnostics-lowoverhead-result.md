# TLSF 低开销诊断结果（2026-08-10）

## 目标

把上一版全局 `AtomicU64` 计数改为 per-CPU 普通计数，使 300 秒诊断不再因计数开销
拖慢，并拿到可用的 TLSF size/lock 数据。

## 验证

```text
make check ARCH=rv PROFILE=final EXTRA_FEATURES=tlsf-diagnostics 通过
make check ARCH=la PROFILE=final EXTRA_FEATURES=tlsf-diagnostics 通过
make check ARCH=rv PROFILE=final 通过
make check ARCH=la PROFILE=final 通过
```

普通 Final 内核 `strings` 未出现 `BUILDSTORM_PERF_COUNTERS` / `tlsf_diagnostics`。

## 300 秒运行

```text
runner: perf/tlsf-slab 分支 buildstorm_runner.py（临时 worktree）
arch: rv
kernel: os/tem/perf/buildstorm/kernels/kernel-rv-tlsf-diagnostics-lowoverhead
image: os/sdcard-rv-pub.img
timeout: 300s
snapshot: yes
result: os/tem/perf/buildstorm/tlsf-diag-lowoverhead-300/result.json
```

运行状态：

- `BUILDSTORM_TOOLCHAIN ok`：通过
- `BUILDSTORM_MINIBUILD ok`：通过
- 进入正式 cargo build 并开始编译依赖
- 无 panic / SIGSEGV / stall

## 早期计数

计数在 `cagent` 命令结束时输出，覆盖 buildstorm 正式编译前的启动/Cargo toolchain
阶段：

| bucket | alloc | free | realloc | bytes |
|---:|---:|---:|---:|---:|
| 16 | 88009 | 77464 | 6936 | 696401 |
| 32 | 14773 | 14706 | 9817 | 359653 |
| 64 | 40721 | 50100 | 9783 | 2138112 |
| 128 | 31749 | 31663 | 94 | 2984296 |
| 256 | 4285 | 2051 | 118 | 1008332 |
| 512 | 1291 | 1251 | 452 | 402569 |
| 1024 | 504 | 764 | 362 | 372298 |
| 2048 | 111 | 79 | 13 | 175997 |
| >2048 | 2857 | 3951 | 1711 | 109129589 |

其它：

```text
tlsf_align_gt16=0
tlsf_lock_acquire=395615
tlsf_lock_contended=52357
tlsf_oom=0
```

## 结论

1. 诊断本身已可运行，300 秒进度与纯 main 早期窗口接近，不再因计数阻塞。
2. 早期阶段分配次数最多的是 16/64/128 字节；按字节则是 >2048 占绝对主导。
3. 锁竞争率约 13.2%，不能忽略，但还不能据此确定是算法成本还是真实等待。
4. 当前数据只到正式编译前，后续完整 BuildStorm 计数仍需一次更长运行或周期性
   dump，才能定位编译期 size class。

## 周期输出改进（2026-08-10 第二轮）

为覆盖 BuildStorm 正式编译期，增加：

- alloc 路径每 `1 << 18` 次分配设置一次 `SAMPLE_PENDING`。
- `dispatch_syscall_from_trap` 返回后检查并输出一次累计快照。
- 普通 Final 通过 `cfg(feature = "tlsf-diagnostics")` 排除。

300 秒诊断 `tlsf-diag-periodic-300b` 共输出 53 组快照，进入正式编译并编译到
`panic_abort` 附近；无 panic/SIGSEGV。

最后一组快照：

| bucket | alloc | free | realloc | bytes |
|---:|---:|---:|---:|---:|
| 16 | 5580621 | 4753953 | 367338 | 38899109 |
| 32 | 1833905 | 1917062 | 623451 | 45625182 |
| 64 | 2382653 | 2889038 | 614837 | 121112798 |
| 128 | 3689930 | 3628017 | 221264 | 332043397 |
| 256 | 237971 | 373009 | 278516 | 51604586 |
| 512 | 74081 | 99089 | 35637 | 21112106 |
| 1024 | 49339 | 49492 | 5460 | 35707743 |
| 2048 | 3957 | 3579 | 3920 | 4869106 |
| >2048 | 576286 | 686407 | 115769 | 2968522223 |

```text
tlsf_lock_acquire=31094581
tlsf_lock_contended=718213
```

结论：

- 正式编译阶段分配次数集中在 16/128/64/32 字节。
- `>2048` 的字节数仍占绝对主导，但次数占比不高。
- 锁竞争率约 2.3%，TLSF 热点主要来自分配/释放调用次数和算法成本，而不是真实锁等待。
