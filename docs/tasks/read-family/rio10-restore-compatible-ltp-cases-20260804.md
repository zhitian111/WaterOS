# RIO-10 恢复兼容 LTP 用例

## 问题

初赛启动阶段会按 `user_bringup_ltp_exclusions.rs` 删除不适配用例。清单中仍包含 12 个
当前内核已经支持的 read-family 用例，导致标准回归只能报告 34 个通过，并将其余用例
记为 `missing`，而不是验证真实 syscall 行为。

本次从通用清单移除以下条目，并保留原有“仅删除明确不适配用例”的机制：

```text
open06 open09 rename09 renameat201 renameat202 read03
pipe03 pipe04 pipe05 pipe07 pipe09 pipe2_04
```

通用排除项由 2352 条降为 2340 条。没有按程序名、argv 或测试用例修改 syscall 语义。

## 验证方法

使用干净的 RISC-V 初赛镜像和修复后通过 `e2fsck` 的 LoongArch 初赛镜像，以 qcow2
overlay 隔离写入，并使用 8 CPU 启动 `bringup-ltp-glibc-only` 内核。

1. 临时禁用裁剪，只运行上述 12 项，确认它们在双架构均真实存在并通过。
2. 恢复正常裁剪，运行上述 12 项，确认清单不会再删除它们。
3. 恢复仓库标准 `guest_read_family_regression.sh`，运行全部 46 项组合回归。
4. 合并运行后的 overlay，执行 `e2fsck -fn` 检查 ext4 一致性。

## 结果

| 验证 | RISC-V64 | LoongArch64 |
|---|---:|---:|
| 不裁剪的恢复用例 | 12/12 | 12/12 |
| 正常裁剪的恢复用例 | 12/12 | 12/12 |
| 完整 read-family 回归 | 46/46 | 46/46 |
| 缺失用例 | 0 | 0 |
| 运行后 `e2fsck -fn` | 通过 | 通过 |

RISC-V 正常裁剪日志显示 `2340 common basenames`、`removed=4661`、`failed=0`，说明
裁剪机制仍生效。双架构完整回归均输出：

```text
READ_FAMILY_RESULT passed=46 failed=0 missing=0
```

`make rv_check && make la_check` 通过。基础 raw 镜像未直接作为 QEMU 写盘目标。

## 证据与调试边界

```text
/tmp/wateros-rio10-full-rv.log
/tmp/wateros-rio10-full-la.log
/tmp/wateros-rio10-full-rv-fsck.log
/tmp/wateros-rio10-full-la-fsck.log
```

pre 专用 bringup 在输出 `all commands finished` 后不一定主动关机，不能仅凭外层
`timeout` 判断为内核死锁。若 runner 结果未完成且输出停止，应使用 `wateros-debug` 的
QEMU/GDB 脚本抓取各核栈、当前任务和锁等待现场，再区分测试超时与内核停滞。

本报告关闭此前双架构门禁中“12 个用例因清单被删除”的限制；RIO-10 的 Linux 差分、
连续压力与性能资源基线仍按集成任务文档执行。
