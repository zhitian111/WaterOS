# BuildStorm 性能验收工具

本目录提供双架构 BuildStorm 镜像准备和 QEMU 9.2.1 运行工具。输入固定为
`/home/zhitian/Downloads/` 中的 organizer `pub` 压缩镜像和恢复后的测试脚本；工具只写
`os/tem/perf/buildstorm-singlecore/`，不修改原始输入。

## 准备母盘

```bash
python3 scripts/perf/prepare_buildstorm_image.py --arch rv
python3 scripts/perf/prepare_buildstorm_image.py --arch la
```

工具验证输入哈希、解压后容量、`/glibc/buildstorm_testcode.sh` dump-back 哈希和
`e2fsck -fn`，并把最终 raw 镜像 SHA-256 写入 manifest。已经通过相同 manifest 验证的
母盘会在重新计算 raw 哈希后复用；不一致的已存在输出需要显式 `--force` 才能替换。

两份 raw 镜像的逻辑容量各约 15 GB，实际稀疏占用也可能超过 7 GB。磁盘无法同时容纳时，
按架构依次准备、运行并删除 `os/tem/perf/buildstorm-singlecore/images/` 中可再生的母盘；不得
删除 `/home/zhitian/Downloads/` 中的固定输入。每轮运行仍由 runner 创建独立副本。

## 运行

```bash
python3 scripts/perf/buildstorm_runner.py \
  --arch rv --kernel ./kernel-rv-final \
  --image tem/perf/buildstorm-singlecore/images/sdcard-rv-pub-prepared.img \
  --run-id example-rv-a1 --timeout 1800
```

runner 强制使用 `/home/zhitian/qemu_9_2_1/qemu-9.2.1/build/` 下的 QEMU 9.2.1。
每个 run-id 创建独立目录和 raw 运行副本，不使用 `-snapshot`，不会覆盖已有结果。输出包括
`serial.log` 和 `result.json`；默认在结束后删除大体积运行副本，排查文件系统时可加
`--keep-run-image`。runner 会重新计算母盘 SHA-256，并在 manifest 提供预期哈希时拒绝不一致
的镜像。

性能判定：小于 500 秒可直接保留；500 到 520 秒需复跑；大于等于 520 秒不优于固定基线。
这些阈值不豁免 cagent、toolchain、minibuild、正式编译、产物启动和无 panic/SIGSEGV 的
功能要求。

`--plugin pc-hot` 或 `--plugin wait-hot` 运行会在 `result.json` 中标记
`wall_clock_eligible=false`，只能用于诊断。
