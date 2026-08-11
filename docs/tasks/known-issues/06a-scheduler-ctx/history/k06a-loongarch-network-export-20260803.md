# LoongArch 网络类型导出修复记录

## 问题

合并 `github/main` 后，`make la_check` 在 `wateros-driver-network` 编译阶段失败。聚合 crate 导出了不存在的 `VirtioPciNetProbeInfo`，而 PCI 实现和 LoongArch 驱动使用的实际类型名均为 `VirtioNetPciProbeInfo`。

## 修复

- 修正 `os/components/wateros-driver/driver-network/src/lib.rs` 的类型再导出。
- 不改变 `api-v0`、设备探测流程或运行时行为。

## 验收

- `make check`：通过（合并后验证）。
- `make la_check`：通过；仅保留既有未使用代码警告。
