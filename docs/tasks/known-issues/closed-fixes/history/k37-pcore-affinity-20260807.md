# K-37 本机 Final 默认绑定 P-core（2026-08-07）

## 问题

本机 CPU 为 Intel i9-13980HX，`lscpu -e` 显示逻辑 CPU 0-15 是 P-core
（最高 5.4-5.6GHz），16-31 是 E-core（4GHz）。此前完整 Final 测试常绑定到 24-31，
实际跑在 E-core 上。

## 修改

- `os/scripts/rv_final_run.sh` 和 `os/scripts/la_final_run.sh` 默认设置
  `WOS_TASKSET_CPUS=0,2,4,6,8,10,12,14`，每个物理 P-core 只选一个逻辑线程。
- 保留 `WOS_TASKSET_CPUS` 环境变量覆盖，方便其它机器和并行测试调整。

## 对比

| 运行 | CPU 集 | 结果 |
|---|---|---|
| K-36 E-core 完整 | `24-31` | `elapsed_s=1881.13` |
| K-36 P-core 完整 | `0,2,4,6,8,10,12,14` | `elapsed_s=1348.86` |

P-core 完整轮比 E-core 快约 28%。

## 验证

完整 Final：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1348.86 cores=8 bytes=1681000 arch=riscv64
#### OS COMP TEST GROUP END buildstorm-glibc ####
```

Pre 60s smoke（P-core）：root RW 挂载成功，cyclictest、hackbench 与 LTP 用例进入
执行，无 panic 和 ext4 读块错误。

`qemu-img check`：`No errors were found on the image.`

## 可复核材料

```text
task: K-37 P-core affinity default
date: 2026-08-07
kernel_commit: 70e628dd (K-36)
architecture: riscv64
qemu_and_firmware: qemu-system-riscv64 virt, OpenSBI 1.7
image: os/sdcard-rv-pub.img (qcow2 overlay)
raw_log_path: /tmp/k36-full-pcore-rv-20260807.log
raw_log_sha256: 5c0432a6344f197a3f0580f157a436bb1ce48f9a00e3c92187dbac05190fe916
pre_log_path: /tmp/k37-pre-pcore-rv-20260807.log
pre_log_sha256: ff6171a6bea995c3a0657c37b55b8ca15a84653a0b66446105cfc80344d1bdc7
overlay_qemu_img_check: ok
```
