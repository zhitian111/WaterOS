# BS-SC-04A 任务简报

## 状态

接受。RISC-V64 正式平台的编译期日志上限改为 `Error`，此前使用 Info ELF 得到的两次 RV
BuildStorm 结果因热路径日志污染而作废。commit 为本简报所在提交，完整 hash 由 BS-SC-04
补记。

## 修改与影响面

- `os/Cargo.toml`：`qemu-riscv64-opensbi` 从 `runtime/impl-info` 改为唯一启用
  `runtime/impl-error`；LoongArch64 保持 Error。
- 根 `README.md`、`os/README.md`、`docs/tools/makefile.md`：同步正式 profile 的日志契约，
  明确 Info/Debug 诊断构建不得用于性能验收。
- 任务 README 与 `04a-riscv-release-error-log-level.md`：记录本次新增前置修正及验收边界。

CodeGraph 查询：

```text
codegraph explore "runtime logging compile-time maximum log level qemu-riscv64-opensbi"
```

查询确认日志 feature 经 `wateros-runtime` 转发到 `log/max_level_*`，编译期会连同高于上限的
格式化参数求值一起裁掉；本次不修改 logger、console 或具体 VFS 日志调用。

## 实际验收

```text
make rv_check                         PASS（仅有既有 warning）
make la_check                         PASS（仅有既有 warning）
make kernel-rv-final                  PASS
make kernel-la-final                  PASS
strings kernel-rv-final | rg
  '\[paged_handle\] seek path='       无匹配
git diff --check                      PASS
```

ELF SHA-256：

```text
旧 RV Info ELF  8ffe99ea1ff20c4b8e13f9d77d492fccb0c10defef7b048d1ed40bfee1e84947
新 RV Error ELF 0e5cdf5fc75f59ffa9ab6e9e3e109ef2c243fc69a73b43048ef9e0df9b2083af
LA Error ELF     974d12834ce283be50f2395b4aa5c134674415682f7315b1e686eb1abcc3d9c8
```

新 RV ELF 使用 QEMU 9.2.1 运行低风险批次 probe，日志：

```text
os/tem/perf/buildstorm-singlecore/functional/logs/bs-sc-04a-rv-error.log
sha256=f443828b6d16428eb5682634d2d95023699437ed09e05895a6724737b99d05f5
LOW_RISK_PROBE step=fpu-syscall-timer status=OK alarms=69 loops=10000
LOW_RISK_PROBE step=stop-continue-kill status=OK
LOW_RISK_PROBE step=eintr-rt-sigreturn status=OK count=1
LOW_RISK_PROBE step=interval-posix-timers status=OK virtual=1 prof=1 posix=1
LOW_RISK_BATCH_RESULT status=OK
```

日志中没有 `[INFO]`、`[paged_handle]`、panic、SIGSEGV 或 timeout。功能 raw 已删除，日志保留；
压缩输入未修改。

## 作废的性能结果

以下两次均使用旧 Info ELF，只作为错误配置审计记录，不参与 `520s` 基线判定：

```text
bs-sc-04-rv-low-risk-batch-1/result.json
sha256=961f447176ecd27b8e83ee96c8308aa12cd26cec0d8fb48e3f1772f55d5755ac
elapsed_s=362.05 status=FAIL（rustc 用户页错/SIGSEGV）

bs-sc-04-rv-low-risk-batch-2/result.json
sha256=6b3fb746dd25e5c7e3bc60374e893a8cd090d2fcf33943be7887ca88a55a3a7f
elapsed_s=566.44 status=OK（INFO 热路径日志污染）
```

第一次运行暴露的 rustc 用户页错仍需由后续有效配置运行观察是否复现，不能因日志配置错误而
宣称缺陷已关闭。

## 决定与下一步

保留日志级别修正。BS-SC-04 必须使用新 Error ELF 和重新准备的干净 RV 性能镜像重跑；本
任务没有产生可采信的 BuildStorm 成绩。双架构平台、赛事 feature 和 QEMU 参数未变化。
