# 工作包：wateros-platform + driver — 赛题对齐与可靠性（可与用户 ABI 并行）

**所属**：`os/components/wateros-platform`、`wateros-driver`；根目录 `Makefile`、QEMU 脚本。  
**并行度**：与 **mm/vfs/syscall 纵轴** 高度并行；本地「单盘 BusyBox」可不等待本包全部完成，但 **赛题 busybox 组正式评测** 依赖其中多项。

## 要做什么

1. **多 virtio-blk 实例**：DTB 扫描与 devfs 节点命名稳定；第二盘 `disk.img` 与赛题 `-drive ... id=x1 ... bus=virtio-mmio-bus.1` 一致（参考 `test_case/README.md` 与 `os/scripts/test_in_qemu_riscv.sh` 差距）。
2. **RTC**：`-rtc base=utc` 下时间 syscall 与 **只读时钟硬件** 或固件约定对齐（与 `wp-syscall-mem-time.md` 协调）。
3. **关机**：用户态或内核在完成 bring-up 总线后调用 **SBI system reset shutdown**（或已有 `FirmwareReset` 路径），使 QEMU 退出；供赛题「跑完测例关机」使用。
4. **virtio-net**：BusyBox 组若不要求网络可降优先级；**iperf/netperf** 前必须完成，本文件仅列 **占位任务** 与验收条目供排期。
5. **根 Makefile `all`**：产出 `kernel-rv`（及赛题若仍要 `kernel-la` 可标为后续），与评测 Docker 行为一致。

## 验收要求

- [ ] 使用与赛题一致的 QEMU 命令行（riscv64）可启动内核，**两块盘**均在 guest 内可见（日志打印块设备路径或 minor 号）。
- [ ] `shutdown` 路径：从 bring-up 总线末尾或用户 `poweroff` 调用后 **QEMU 进程退出**，退出码符合脚本约定（若无可约定为 0）。
- [ ] RTC：连续两次 `gettimeofday` 间隔与真实流逝 **同量级**（允许 QEMU 误差，文档写阈值）。

## 验证方式

1. 在 `os/scripts/` 增加或修订 **`test_in_qemu_riscv_contest.sh`**（名称可自定），与 `test_case/README.md` 中 riscv 命令行对齐；CI 或本地手动运行。
2. bring-up 总线增加可选阶段 `[bringup][scaffold] multi-blk OK`，读取第二盘上已知魔术文件（需事先制作小镜像）。
3. **不依赖** `self_tests`：关机验证可在 **空用户测例** 下由内核直接调用固件关机 API 测通。

## 依赖

- **上游**：现有 DTB 与 virtio-mmio 扫描框架。
- **下游**：赛题全量 `*_testcode.sh` 调度（另文或大路线图）；本包为基础设施。

## 可并行对象

几乎所有用户态 syscall 工作包；注意 **合并冲突** 多发生在 `main.rs` 与 `Makefile`，需与 `wp-init-test-bus.md` 负责人协调。
