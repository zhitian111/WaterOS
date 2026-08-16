# BS-SC-00 任务简报

## 状态

接受。建立了固定输入校验、镜像覆写、QEMU 9.2.1 启动和结果判定工具。本提交只涉及 host
工具、测试与文档，不改变内核行为。commit 为本简报所在提交，完整 hash 由下一任务补记。

## 修改与影响面

- 新增 `os/scripts/perf/prepare_buildstorm_image.py` 与 `buildstorm_runner.py`；
- 新增 14 个 host 单元测试，以及 `os/scripts/perf/README.md`；
- 新增 `docs/tools/buildstorm-runner.md`，同步脚本与工具索引；
- CodeGraph 查询了 `buildstorm_testcode.sh`、`run_stage_busybox`、QEMU launch 和
  `run_stage_busybox` impact；未发现需要修改的内核调用链。

## 实际验收

```text
python3 -m py_compile os/scripts/perf/*.py os/scripts/tests/test_*buildstorm*.py  PASS
python3 os/scripts/tests/test_prepare_buildstorm_image.py                     5/5 PASS
python3 os/scripts/tests/test_buildstorm_runner.py                            9/9 PASS
git diff --check                                                              PASS
qemu-system-riscv64 --version                                                 9.2.1
qemu-system-loongarch64 --version                                             9.2.1
```

两架构都完成一次真实解压、脚本覆写、`0755` inode 检查、dump-back 哈希和
`e2fsck -fn`。raw 逻辑大小均为 15,032,385,536 字节：

| 架构 | 压缩输入 SHA-256 | 准备后 raw SHA-256 | dump-back | e2fsck |
|---|---|---|---|---|
| RV | `cba87f43...1df6f1` | `82404d8483a77ca8419c22f36c9295dc2b26a6c034b4e2ad9be36cdd51c70ccb` | 脚本 SHA 匹配 | exit 0 |
| LA | `2c411447...d90d2` | `6e59292d914d0f915269f50387898ad326b19892bdf36cb6011048735c8c9aeb` | 脚本 SHA 匹配 | exit 0 |

两份发布镜像的 `e2fsck -fn` 都报告 free blocks/free inodes 计数与扫描值不一致，但返回 0；
原始输出完整保存在各自 manifest/dry-run `result.json`，不将其表述为“无提示”。RV 与 LA 的
dry-run 分别验证了用户指定的 8 vCPU/16 GiB virtio-mmio 和 12 vCPU/36 GiB virtio-pci
完整 argv，均未使用 `-snapshot`。

baseline ELF 保持不变：RV SHA-256 为 `2f85662a...80d`，LA 为 `d4a3a7bb...f47`。

## 决定与剩余风险

本任务不运行 clean-main 性能 baseline，固定基线仍为两架构 `520s`。宿主仅余约 7.6 GB，
RV/LA raw 母盘实占约 7.5/7.9 GB，后续必须按架构轮换；删除的母盘仅是可由固定压缩输入
重建的生成物。完整 guest 功能与性能验收留给 BS-SC-04。
