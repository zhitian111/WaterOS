# BuildStorm 双架构验收

权威命令和输入哈希见
[`docs/agents/tasks/buildstorm-singlecore-runtime/README.md`](../agents/tasks/buildstorm-singlecore-runtime/README.md)。
host 工具位于 `os/scripts/perf/`，负责：

1. 从只读 organizer `.img.gz` 解压 raw 母盘；
2. 覆写并校验 `/glibc/buildstorm_testcode.sh`，记录最终 raw SHA-256；
3. 对每轮创建独立运行副本；
4. 使用指定 QEMU 9.2.1 参数启动双架构 Final 内核；
5. 把完整功能协议与性能数据写入 `result.json`。

```bash
cd os
python3 scripts/perf/prepare_buildstorm_image.py --arch rv
python3 scripts/perf/prepare_buildstorm_image.py --arch la
python3 scripts/perf/buildstorm_runner.py --arch rv --kernel ./kernel-rv-final \
  --image tem/perf/buildstorm-singlecore/images/sdcard-rv-pub-prepared.img \
  --run-id rv-a1 --timeout 1800
```

不要直接启动 `/home/zhitian/Downloads` 中的压缩输入，不要手改母盘，也不要复用 run-id。
QEMU plugin 运行不是墙钟性能证据。磁盘不足以同时保存两架构 raw 母盘时，允许按架构依次
准备和运行，再删除 `os/tem/perf/buildstorm-singlecore/images/` 中可重建的母盘；固定压缩
输入和每轮结构化结果必须保留。
