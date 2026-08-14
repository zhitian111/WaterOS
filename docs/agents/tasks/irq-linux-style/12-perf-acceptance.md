# T12：性能验收与归档

## 任务内容

对 `feat/irq-linux-style` 做最终验收：先功能全绿、无 bug，再做双架构 BuildStorm
性能 A/B（main 基线 vs 本分支），记录数据并决定是否合并进 main。

## 实施方案

1. 重建 main 基线内核（或校验 `os/.perf-baseline-main/` 与 clean HEAD 一致），双架构。
2. 镜像统一解压到 `~/Downloads/`，用 `debugfs -w` 覆写 BuildStorm 脚本为
   `~/Downloads/buildstorm_testcode.recovered.sh`。
3. 用 QEMU 9.2.1（`~/qemu_9_2_1/qemu-9.2.1/build/`）跑双架构 BuildStorm，按
   `README.md` 的启动命令执行。
4. 每次性能测试前确认无正在运行的 QEMU；功能测试可随时做。
5. 记录 `BUILDSTORM_COMPILE ... elapsed_s=...`，做多轮取稳定值（如环境允许）。
6. 结论三选一：
   - 功能全绿且最终性能优于基线 → 可合并；
   - 功能全绿但性能未优于基线 → 保留分支、写清原因，不合并；
   - 有 bug → 回到对应任务修复。

## 涉及文件

- 无源码变更，产出 `docs/agents/tasks/irq-linux-style/history/12-perf-acceptance.md` 与日志摘要。

## CodeGraph 查询命令

本任务以运行与测量为主，不依赖 CodeGraph。

## 验收方式

- 功能：双架构 BuildStorm marker 全部通过，无 panic/SIGSEGV/卡死/超时。
- 性能：最终 BuildStorm 耗时优于 main 基线；过程中允许临时劣化。

## 验收命令

```bash
cd /home/zhitian/project/WaterOS_irq_linux_style/os
# 确认无运行中的 QEMU
pgrep -af qemu-system || true
# 解压镜像到 ~/Downloads（gunzip -k）
gunzip -k ~/Downloads/sdcard-rv-pub.img.gz
gunzip -k ~/Downloads/sdcard-la-pub.img.gz
# 按 README.md 的 QEMU 命令跑双架构 BuildStorm
```

## 任务简报

完成后写 `history/12-perf-acceptance.md`。
