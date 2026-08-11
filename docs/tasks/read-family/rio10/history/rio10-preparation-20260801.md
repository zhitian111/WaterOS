# RIO-10 准备记录：LTP 读取回归入口

## 结果

新增 `os/scripts/guest_read_family_regression.sh`，统一编排初赛镜像已有的 LTP
read-family 二进制。该入口替代重复编写一套 C 语义测试，预期行为直接以仓库中的
LTP 20240524 源码为依据。

runner 默认覆盖 `open/read/readv/pread/preadv`、pipe、Unix socket、inet socket 和
eventfd 共 25 个用例。每个用例独立限时并输出稳定的开始、结果和汇总标记；缺失二进制、
超时及非零退出都计为失败。可通过以下环境变量复用：

```text
LTP_BIN_DIR
READ_FAMILY_CASES
READ_FAMILY_CASE_TIMEOUT
READ_FAMILY_BUSYBOX
```

## 验证

- `sh -n os/scripts/guest_read_family_regression.sh`：通过。
- host runner 冒烟：`/usr/bin/true` 汇总为 1 passed、0 failed。
- RISC-V QEMU 短回归：`read02`、`pread02`、`eventfd01` 均退出 0，共 10 个 TPASS，
  runner 汇总为 3 passed、0 failed、0 missing。日志：
  `/tmp/wateros-rio10-runner-smoke-2.log`。
- 镜像测试入口已恢复，注入的 runner 已删除，并通过 `cmp` 确认入口内容不变。

本记录不代表 RIO-10 完成。完整 runner、8 核竞态、LoongArch、iozone、CAgent、
BuildStorm、镜像 hash 和 `e2fsck` 门禁仍按约定等待夜间执行。
