# K-10 `acct02` Helper 过滤修复报告（2026-08-04）

## 现象

LoongArch-musl LTP 基线中 `acct02` 返回 `acct file is empty`。初看像进程退出未写
accounting record，但当前 `acct(2)` 实现和旧审计均表明该用例曾通过。

## 根因

LTP 20240524 的 `acct02` 会在启用 accounting 后执行 `acct02_helper`，helper 退出才会
产生待检查记录。启动期排除表却包含 `acct02_helper`，会同时从 musl/glibc 的
`testcases/bin` 删除 helper，但保留 `acct02` 主程序。

旧裁剪镜像的四个 libc/架构位置均缺少 helper；未裁剪且通过 `e2fsck -fn` 的
`os/tem/test-sdcard.img` 与 `os/tem/sdcard-la.resume.img` 均包含对应 ELF。诊断运行
确认启用和关闭 accounting 之间没有 helper 进程退出，因此空文件是过滤制造的假失败。

## 修改

从 `os/src/user_bringup_ltp_exclusions.rs` 删除 `acct02_helper`，清单计数由 2353 改为
2352。过滤仍只删除明确跳过的顶层用例，不再删除被保留用例的运行依赖。期间尝试的
accounting 生命周期改动和诊断日志已全部撤销，syscall/task 架构未改变。

## 验证

- LoongArch64/QEMU、8 CPU、musl LTP：1 PASS、0 FAIL，正确解析 1 条 record。
- RISC-V64/OpenSBI、8 CPU、musl LTP：1 PASS、0 FAIL，正确解析 1 条 record。
- 两份测试副本注回原镜像同架构 helper 后通过 `e2fsck -fn`，QEMU 使用 snapshot。
- `make rv_check`、`make la_check` 及两架构 LTP-musl 内核构建通过，仅有既存 unused
  警告。

日志：`/tmp/wateros-acct02-la-filter-fix.log`（SHA-256
`11d7325b114054f1f7d12947b3d76815d864413d2a292fea3cdbb15a89abbce9`）与
`/tmp/wateros-acct02-rv-filter-fix.log`（SHA-256
`79a6da0ea8a84ad7047ce81a10c261e049708044eca866c02b0b91b510e465b3`）。
