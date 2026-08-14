# Task 09：最终性能验收与收尾

## 任务目标

对比最终分支与 task 00 baseline，证明两架构最终 `BUILDSTORM_RESULT.elapsed_s`
都优于基线，并清理临时镜像、确认交付物和分支状态。

## 验收机制

1. 使用 task 00 相同的镜像准备、脚本覆写和 QEMU 9.2.1 命令。
2. 每架构跑 3 轮全新镜像，提取：

   ```text
   BUILDSTORM_RESULT mode=multi status=OK rc=0 cores=<n> elapsed_s=<s> ... run=OK
   ```

3. 取 3 轮 `elapsed_s` 中位数，与 baseline 中位数比较：

   ```text
   final_median_la < baseline_median_la
   final_median_rv < baseline_median_rv
   ```

4. 任一架构不满足，性能验收不通过。中间任务可以回退，最终必须满足。
5. 性能比较只比较同一 guest 配置和同一脚本；不允许篡改时钟、跳过 workload 或
   改变镜像内测试内容。

性能测试开始前必须执行：

```bash
pgrep -af 'qemu-system-(riscv64|loongarch64)' || true
```

如果存在其他 QEMU 进程，先等待其自然退出，再进行性能测试。

## 运行命令

同 README 和 task 00，日志命名：

```text
/tmp/wateros-slab-la-final-perf-N.log
/tmp/wateros-slab-rv-final-perf-N.log
```

## 收尾

- 删除 `/tmp/wateros-slab-*.img` 等临时 raw 镜像；
- 确认 `~/Downloads/*.img.gz` SHA 不变；
- 确认工作树中不残留 `kernel-*`、`*.img`、`target/`、日志等生成物；
- 最终只保留源码、任务文档、历史简报和必要脚本；
- 确认分支可推送/合并，并在简报中写清最终 commit、性能表和未验证项。

## 完成后

新增 `history/09-brief.md`，包含 baseline/final 中位数、6 轮结果表、日志 SHA、
清理结果和交付说明。
