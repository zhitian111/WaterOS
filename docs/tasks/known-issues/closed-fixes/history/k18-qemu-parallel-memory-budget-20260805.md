# K-18 QEMU 并行内存预算（2026-08-05）

## 问题

仓库已经支持用 `WOS_TASKSET_CPUS` 将多个 QEMU 实例绑定到独立宿主 CPU 集，但各
启动脚本仍固定使用 1 GiB 或 8 GiB 内存。32 线程、30 GiB 宿主并行运行时，固定
8 GiB 会限制并发数，并可能因换页反而降低 BuildStorm 调试效率。

## 修改

- RV/LA 的 pre、final 启动脚本支持 `WOS_QEMU_MEM`。
- 保持原有默认值：pre/兼容启动为 1 GiB，final 为 8 GiB。
- README 记录变量含义及 32 线程机器的并行示例；轻量测试可使用 2 GiB，完整
  BuildStorm 仍使用默认 8 GiB。
- legacy 和 snapshot 的 RISC-V 脚本采用同一变量，避免不同入口行为分叉。

## 验证

```text
bash -n os/scripts/{rv,la}_{pre,final}_run.sh
bash -n os/scripts/rv_qemu_run.sh os/scripts/rv_qemu_run_snapshot.sh

run_qemu_parallel.sh "echo job1" "echo job2"
job1: cpuset=0-7
job2: cpuset=8-15
```

RISC-V pre 使用 `WOS_SMP=2 WOS_QEMU_MEM=2G WOS_QEMU_SNAPSHOT=1` 启动成功，
完成 glibc LTP 后进入 libcbench；30 分钟外层超时终止，不是内核停滞。

## 使用建议

- 完整 final BuildStorm：每实例 8 个宿主线程、8 个 guest vCPU、8 GiB 内存。
- pre 冒烟或聚焦回归：每实例 4 至 8 个宿主线程、2 GiB 内存。
- 并行实例必须使用 snapshot 或独立 overlay，不允许并发写同一 raw 镜像。
