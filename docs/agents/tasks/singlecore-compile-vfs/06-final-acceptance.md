# Task 06：最终功能与性能验收

## 任务目标

证明本分支在 RV/LA 上均优于 frame-allocator 基线，并完成收尾。

## 功能验收

- 双架构各 3 轮全新镜像 BuildStorm；
- 每轮 TOOLCHAIN/MINIBUILD/BUILDSTORM OK；
- 无 panic/OOM/ENOMEM/SIGSEGV/fault/缓存失效异常。

## 性能验收

- 每架构 3 轮取中位数；
- 要求 final_median_la < baseline_median_la；
- 要求 final_median_rv < baseline_median_rv；
- 性能测试前等待系统中无 QEMU。

## 镜像与 QEMU

沿用 frame-allocator 分支和本仓库 QEMU 9.2.1 命令，所有运行加 `-snapshot`。

## 收尾

- 删除临时 raw 镜像；
- 确认 `~/Downloads/*.img.gz` SHA 不变；
- 工作树不残留内核、镜像、日志；
- 补齐历史简报并确认分支可合并。

## 完成后

新增 `history/06-brief.md`。

