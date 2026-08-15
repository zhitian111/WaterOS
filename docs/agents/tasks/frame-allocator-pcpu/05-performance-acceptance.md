# Task 05：最终性能验收与收尾

## 任务目标

证明最终分支在 RV 和 LA 上均优于 slab 基线，并清理临时文件、确认交付物。

## 验收机制

1. 使用与基线相同的镜像准备、脚本覆写、QEMU 9.2.1 命令和 guest 内存配置。
2. 每架构跑 3 轮，取 `BUILDSTORM_RESULT.elapsed_s` 中位数。
3. 比较：

   ```text
   final_median_la < slab_baseline_median_la
   final_median_rv < slab_baseline_median_rv
   ```

4. 性能测试前必须确认无其他 QEMU 进程；所有运行加 `-snapshot`。

## 运行命令

沿用本目录 README 和 slab 任务的镜像/QEMU 命令，日志命名：

```text
/tmp/wateros-frame-pcpu-la-final-perf-N.log
/tmp/wateros-frame-pcpu-rv-final-perf-N.log
```

## 收尾

- 删除临时 raw 镜像；
- 确认 `~/Downloads/*.img.gz` SHA 不变；
- 工作树不残留 `kernel-*`、`*.img`、`target/`、日志等生成物；
- 确认分支可推送/合并，写清最终 commit 和性能表。

## 完成后

新增 `history/05-brief.md`。

