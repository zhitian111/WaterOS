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
