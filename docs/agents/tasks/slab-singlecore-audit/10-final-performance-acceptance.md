# Task 10：main 与最终候选的 RISC-V 性能终验

## 任务目标

用同一宿主、同一 QEMU 9.2.1、同一恢复脚本、同一内存和 vCPU 参数，证明最终候选优于
保存的 main RISC-V 基线；双架构功能通过是前置条件。

## 实施方案

1. 任务 00 保存的 main kernel 与最终候选 kernel 不得混用；分别记录 SHA-256。
2. 为 RISC-V 准备至少三份从 `~/Downloads/sdcard-rv-pub.img.gz` 重新解压并覆写脚本的 raw 镜像。
3. 采用交错 `A/B/B/A` 顺序，轮间确认无 QEMU、runner、异常 swap 或宿主内存压力；异常轮
   只记录，不擅自删除，重新准备新镜像后补测。
4. 以 guest `BUILDSTORM_RESULT ... elapsed_s` 为主指标，取三轮中位数；同时记录
   toolchain/minibuild、产物启动、内核错误和宿主状态。

## 性能门禁

- RV 最终候选中位数必须严格小于 main RV 中位数。
- 三轮均必须功能成功；任何功能失败即使耗时更快也判定不合格。
- 若差异小于运行噪声，必须增加交错轮次，不能把单轮优势当作结论。
- 诊断 feature、统计符号和调试日志不得出现在最终 kernel。

## 验收命令

```bash
sha256sum os/.perf-baseline-slab-singlecore-main/kernel-rv-final os/kernel-rv-final
ps -eo pid,ppid,stat,%cpu,etime,args | rg '[q]emu-system|[b]uildstorm' || true
git diff --check
```

QEMU 启动命令和镜像覆写命令严格复用任务 00；禁止使用默认 PATH 中的 QEMU，必须检查：

```bash
~/qemu_9_2_1/qemu-9.2.1/build/qemu-system-riscv64 --version
```

## 完成后

新增 `history/10-brief.md`，记录 main/candidate kernel SHA、各轮结果、三轮中位数、
功能标记、宿主状态、统计方法和最终是否满足 RISC-V 性能门禁。README 的任务状态同步为
完成或明确剩余风险。
