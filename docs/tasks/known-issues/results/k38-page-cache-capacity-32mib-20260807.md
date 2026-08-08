# K-38 页缓存扩容到 32MiB（2026-08-07）

## 问题

BuildStorm 会反复读取大量 crate/header 文件，16MiB 页缓存容易被冷文件挤出热集。

## 修改

`os/components/wateros-base/base-config/src/fs.rs` 将
`FILE_PAGE_CACHE_CAPACITY` 从 `4096` 调到 `8192`：

- 16MiB → 32MiB
- 内核堆仍为 128MiB，页缓存占用约 25%

## 对比

P-core、8 vCPU、8 GiB：

| 配置 | `elapsed_s` |
|---|---:|
| K-36 16MiB 页缓存 | 1348.86 |
| K-38 32MiB 页缓存 | 1282.12 |

完整 Final：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1282.12 cores=8 bytes=1681000 arch=riscv64
#### OS COMP TEST GROUP END buildstorm-glibc ####
```

Pre 60s smoke（P-core）：root RW 挂载成功，cyclictest、hackbench 与 LTP 用例进入
执行，无 panic 和 ext4 读块错误。

`qemu-img check`：`No errors were found on the image.`

## 可复核材料

```text
task: K-38 page-cache capacity 32MiB
date: 2026-08-07
kernel_commit: f720138b + working-tree K-38
architecture: riscv64
qemu_and_firmware: qemu-system-riscv64 virt, OpenSBI 1.7
image: os/sdcard-rv-pub.img (qcow2 overlay)
raw_log_path: /tmp/k38-full-pcore-rv-20260807.log
raw_log_sha256: 9a494babd8f38108d8fb0c86e50948754420c268737ecc9315404c9369f9c3f9
pre_log_path: /tmp/k38-pre-pcore-rv-20260807.log
pre_log_sha256: 8941b73f81598b7789437beb396ea9798a1dde6e66154d85e4591e594e4edddb
overlay_qemu_img_check: ok
```
