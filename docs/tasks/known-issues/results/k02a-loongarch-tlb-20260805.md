# K-02A LoongArch PTE 与远端 TLB 验证（2026-08-05）

## 根因与修复

LoongArch 页表把 PTE `W` 位作为软件权限记录，硬件 store 是否允许由 `D` 位执行。
原 `from_perm()` 对所有映射无条件设置 `D`，因此 `mprotect(PROT_READ)` 虽清除了软件
`W`，用户写入仍被硬件允许。

修复后仅可写映射设置 `W | D`，只读映射保持 `D=0`。定向诊断确认 CPU1 对只读页
写入触发 `Exception(StorePageFault)`，而不是静默写穿。

## 远端 TLB 测试

测试进程固定 controller 到 CPU0、worker 到 CPU1。worker 先读取并缓存映射，CPU0
随后释放该 VA，让 guard 映射占用旧物理页，再以新物理页重建原 VA；worker 必须读到
新页内容。该流程同时覆盖 `munmap`、`MAP_FIXED` 和 LoongArch IOCSR IPI shootdown，
不依赖当前仍有兼容问题的用户 signal return。

三次独立 8 CPU QEMU 运行均通过：

```text
SMP_K02A_REMAP_PASS iterations=10000 remote_cpu=1 controller_cpu=0
```

累计 30,000 次远端同 VA 物理页替换，无旧映射或错页。`make la_check`、pre GDB kernel
构建通过；`getcpu` 同批修改还完成 `make rv_check`。

日志备份于 `os/debug-reports/archive/k02a-loongarch-tlb-20260805/`。三轮 SHA-256
依次为 `1cf3c28f...2a04626`、`33878bc3...3824c1`、`61488b8a...a8511`，完整值见
`SHA256SUMS`。

## 剩余边界

本阶段结束时仍需覆盖 fork/exec/跨进程地址空间、显式 IPI pending/clear 计数和逐核
timer/idle，后续报告已补齐。初始 raw handler 使用 `longjmp` 绕过 `rt_sigreturn`，并非
trampoline 缺陷；标准 libc handler-return 路径的双架构回归见
`k02a-signal-return-20260805.md`。
