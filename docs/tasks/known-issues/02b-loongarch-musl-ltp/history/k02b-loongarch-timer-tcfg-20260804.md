# K-02B LoongArch 定时器 TCFG 修复报告（2026-08-04）

## 现象

LoongArch64/QEMU 8 核 musl LTP 中，`alarm05` 在一次 `sleep(1)` 后报告剩余 6--7 秒，
而不是 9 秒；`alarm06` 的 2 秒闹钟在一次 `sleep(1)` 内已经触发。修复前定向结果为
`alarm05` 2 PASS/1 FAIL、`alarm06` 0 PASS/2 FAIL，两用例单调耗时约 15.2 秒。

## 根因

`rdtime.d` 和 CSR 定时器使用同一 100 MHz StableCounter 刻度。LoongArch 架构手册规定
`TCFG[1:0]` 是控制位且计时值按 4 tick 对齐；QEMU 的
`target/loongarch/tcg/constant_timer.c` 则直接以 `TCFG & ~3` 作为倒计时 tick。

平台实现把 delta 写成 `(delta << 2) | ENABLE`，因此每个调度 tick 实际延长为 4 倍。
单调时钟仍按原始 StableCounter 前进，导致基于调度 tick 的 `sleep` 与基于单调时钟的
`alarm` 产生 4 倍偏差。

参考：

- <https://loongson.github.io/LoongArch-Documentation/LoongArch-Vol1-EN.html#timer-configuration-tcfg>
- <https://gitlab.com/qemu-project/qemu/-/blob/master/target/loongarch/tcg/constant_timer.c>

## 修改

`os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/timer.rs`
现在将 delta 向上对齐到 4 tick 后直接写入 `TCFG`，不再左移两位；加法溢出继续返回
`InvalidDeadline`。该修改仅属于 LoongArch QEMU 平台实现，不改变 task、syscall 或
通用 platform API。

## 验证

- `make la_check` 和 `make kernel-la-ltp-musl` 通过，仅有既存 unused/dead-code 警告。
- 修复后 `alarm05` 3/3 PASS、`alarm06` 2/2 PASS，定向耗时约 5.3 秒。
- LoongArch release 8 核连续 3 轮运行 `alarm02/03/05/06/07`：每轮 15 个断言全部
  PASS，runner 正常结束，耗时约 8.4--8.5 秒。
- RISC-V/OpenSBI 8 核同组对照 15/15 PASS。两架构镜像均不含 `alarm01`，因此该项
  为命令不存在（127），未计入断言。
- 使用 `wateros-debug` 的 LoongArch GDB 构建复验同组，runner 正常结束。一次 release
  扩展运行曾在全部断言通过后停于 runner 清理，后续 GDB 运行和 3 轮 release 并发
  回归均未复现；该时序风险不归因于本次 TCFG 修复。

关键日志 SHA-256：修复前 `c4c69767...bcc77e`，修复后定向
`c68a1230...a2af7d`，三轮 release 分别为 `44d54a14...b36cbc`、
`fe6abdd7...e60c07`、`bf4afcb7...f4af79`。
