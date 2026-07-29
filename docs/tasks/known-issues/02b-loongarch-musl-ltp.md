# K-02B：LoongArch-musl LTP 0 分复验

## 任务目标

确认旧评分中 LoongArch-musl LTP 整套 0 分是否仍存在；若存在，从最小 ELF 到单个
LTP 用例逐级定位第一个根因。该任务可与 K-02A 并行。

## 执行前必读

- `docs/tasks/known-issues/02-smp-loongarch-validation.md`
- `docs/prompts/general.md`
- `docs/prompts/debug_workflow.md`
- `docs/exports/features/wateros-mm.md`
- `docs/exports/features/wateros-syscall.md`
- `docs/tasks/run_testsuits_qemu.md`
- `docs/todo/perf-baseline-gap-report.md`

## 已知信息与代码证据

旧结果是 glibc-la 和 musl-rv 有效，只有 musl-la 整套为 `-`/0。当前 bringup 已提供
musl-only 入口：

```rust
#[cfg(all(feature = "pre", feature = "bringup-ltp-musl-only"))]
const BRINGUP_COMMANDS: &[BringupCommand] = &[/* /musl/ltp_testcode.sh */];
```

动态解释器、lazy ELF、ASID 和 bringup 后续均有改动，旧结果必须先复验。

## 涉及文件

- `os/src/user_bringup_{busybox,common}.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/kernel_elf.rs`
- `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/`
- `os/components/wateros-syscall/`
- `test_case/` 中 LoongArch musl 镜像与 LTP 脚本

## 任务内容

1. 保存 LoongArch 镜像 hash、QEMU 命令和 feature tree。
2. 依次运行最小静态 ELF、musl busybox、一个已知通过的 LTP case、一个历史失败 case
   和完整 LTP。
3. 记录最早的 mount/exec/ELF/trap/signal/timeout 错误；禁止从最终 0 分猜 syscall。
4. 与 musl-rv 及 glibc-la 使用同一用例和环境对照。
5. 若当前已通过，只更新历史报告；若失败，只修第一个根因并重跑全链。

## 如何验收

- [ ] musl-la LTP 产生有效统计，不再是整套未运行。
- [ ] 最小层级和完整脚本均有退出码、标记与原始日志。
- [ ] 修复不以架构/libc 路径硬编码 Linux syscall 语义。
- [ ] `make la_check`、glibc-la、musl-rv 抽样无回归。

交付 `docs/tasks/known-issues/results/k02b-YYYYMMDD.md`；缺镜像时记录为外部阻断。
