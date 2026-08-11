# K-07 mremap/VMA 搬迁一致性报告（2026-08-02）

## 记录信息

- task: K-07，BuildStorm `rustc` 用户态 SIGSEGV / `mremap(2)`
- kernel_commit: 本报告所在提交，基于 `e783263c2a40bfac72d67fc266b7bdd488e97500`
- user_submodule_commit: `2f470f95fa6bf0401c4b1b7ef3bb8fc7a10b870b`
- architecture: RISC-V64/OpenSBI，8 CPU；LoongArch64 完成编译检查
- qemu_and_firmware: `qemu-system-riscv64 -machine virt -smp 8`，OpenSBI v1.7
- image_sha256: `sdcard-rv.img` =
  `7deebc7a558e9d24567d13bc54c581913a5ff05d5ae5788097e02756a0424c15`
- overlay: 每轮新建 qcow2，原始镜像不写回

## 现象与根因

BuildStorm 编译 `libc 0.2.186` 时，`rustc` 在用户态发生 load page fault：

```text
old=0x1ff0c000 old_size=0x55000 new_size=0xa9000 result=0x2cdc7000
stval=0x2cdcd358 sepc=0x17416cc2
error: rustc interrupted by SIGSEGV
```

故障地址位于刚搬迁的目标区间。原实现只用已驻留 PTE 搜索搬迁地址，没有避开尚未
fault 的 lazy VMA；同时搬迁后没有同步 lazy VMA 元数据。后续页面被丢弃时，新地址
没有 loader 可以处理缺页，最终向 `rustc` 投递 SIGSEGV。原实现还把目标权限固定为
RWU，固定搬迁没有完整验证重叠，缩小时复制长度可能超过目标范围。

## 修复

- 架构层使用同时检查 PTE、lazy VMA 和 shared-anon VMA 的地址搜索器。
- 原位扩容即使只撞到未驻留 VMA，也强制走 `MREMAP_MAYMOVE`。
- 完整覆盖源区的 lazy VMA 随映射移动/缩放，保留 loader、偏移和权限；
  `MREMAP_DONTUNMAP` 保留旧 VMA 并登记新 VMA。
- 公共 mover 使用源映射权限，复制长度限制为源/目标长度较小值。
- `MREMAP_FIXED` 必须同时提供 `MREMAP_MAYMOVE`，拒绝源/目标重叠。
- shared-anon 源搬迁暂返回 `Unsupported`，避免复制物理页后破坏共享身份。
- RISC-V Sv39 与 LoongArch64 保持同一行为；未修改 task 模块或其架构。

## 验证

执行命令：

```text
cd os && make rv_check
cd os && make la_check
make kernel-rv-ltp-glibc
qemu-system-riscv64 ... -smp 8 ... <fresh qcow2 overlay>
e2fsck -fn /tmp/wateros-mremap-ltp-report.raw
```

结果：

- `make rv_check`、`make la_check` 通过。
- 初赛镜像原生 glibc LTP `mremap01..06` 全部退出 0。
- `mremap05` 七项断言全部 `TPASS`，覆盖 flag、非对齐、重叠、目标覆盖和数据保持。
- QEMU 顶层命令退出 0，正常关机；未出现 kernel panic 或用户页故障。
- overlay 转 raw 后 `e2fsck -fn` 五阶段通过，退出码 0。

原始记录：

- `/tmp/wateros-mremap-ltp-report.log`，SHA-256
  `89fa5b8e206053f4e56a5e1afce7975ef35b74ad6da93e0fd98d872d60b2b1ab`
- `/tmp/wateros-mremap-ltp-e2fsck.log`，SHA-256
  `3fd5f8be1fa2c8f439330b1bcbb2294a62db9228ba83e7c2f7cbbcb9b7d4772a`
- 修复前 fault probe：`/tmp/wateros-buildstorm-libc-probe3.log`，SHA-256
  `45514cee5836d5bffed55fbbfc246400d7c88854f0e1b46ee596a52fd80bec96`

## 未关闭门禁

同机制的早期修复版本曾在定向 `tg-xtask` 编译中越过原故障位置，从 `22/446`
推进到 `24/446` 且未再次 fault，但不等同于最终代码的完整 BuildStorm 通过。最终代码
的白天 10 分钟尝试在 Cargo 输出前异常终止，日志无 panic/SIGSEGV，证据不足。

因此本提交关闭已定位的 `mremap` 一致性缺陷和初赛 syscall 回归，不关闭 K-10。
夜间仍须用最终代码和全新 final overlay 完成 CAgent、BuildStorm，并在正常关机后执行
`e2fsck -fn`。此外，部分 lazy VMA、非驻留源区和 shared-anon 的完整 Linux
`mremap` 语义仍是后续 MM 兼容性任务。
