# K-18 QEMU 并发运行与核绑定修复（2026-08-05）

## 任务目标
- 用 32 核机器并行加速竞赛测试（重点是 final 阶段）
- 支持 run_qemu_parallel.sh 在不破坏既有架构的前提下按主机核分片
- 验证并行起停与初赛/决赛 qemu 启动链路

## 本次提交内容
1. os/scripts/run_qemu_parallel.sh
  - 补充 WOS_AUTO_SMP：命令未显式 WOS_SMP 时自动按 WOS_CORES_PER_JOB 注入。
  - 补充 WOS_AUTO_UNLOCK_DRIVE：自动为每个命令可选注入 WOS_QEMU_IMAGE_DRIVE_OPTIONS=locking=off。
  - 新增异常退出清理 trap，并发结束时自动 kill 子进程。
  - 增加镜像路径解析与兼容性判断：locking=off 仅对 .qcow2/.qcow 注入。
  - 未匹配 qcow2 时输出 skip locking=off 提示，避免 raw 启动失败。

2. os/README.md
  - 增补 32 核并行示例（含 buildstorm 场景）。
  - 补充 qcow2 与 raw 的 locking=off 使用边界说明。

3. os/scripts/README.md
  - 修订并行 unlock 用法与示例：示例显式使用 qcow2 镜像。
  - 添加 qcow2 转换命令示例。

## 验证
- bash -n os/scripts/run_qemu_parallel.sh
- cd os && make rv_check
- cd os && make la_check
- WOS_QEMU_SNAPSHOT=1 WOS_SMP=2 make rv_pre_run：初赛启动、根卷挂载、busybox runner 到位。
- WOS_QEMU_SNAPSHOT=1 WOS_SMP=4 make rv_final_run：决赛启动、cagent 通过并进入 buildstorm 构建阶段。

## 说明
该轮聚焦基础设施与可启动性；需继续补齐 buildstorm 全量稳定性与并行稳定性。

