# K-35 页缓存 read/write 复用 FileCacheKey（2026-08-07）

## 问题

`pc-hot` 显示 `file_key` 约 11.5M 指令，且 TLSF allocate/deallocate 是内核侧最大
热点之一。`GlobalFilePageCache::read`/`write` 每页会多次调用 `file_key(path)`，
每次 `Arc::from(path)` 都会在内核堆上分配一份路径字符串。

## 修改

- 新增 `file_key_from_arc`、`get_file_entry_for_key`。
- `install_page`/`install_zero_page` 改为接收 `&FileCacheKey`。
- `read`/`write` 只创建一次 key，循环内复用同一个 key 做 install 和 index lookup。
- 保留原有基于 `&str` 的公开 API，外部调用方式不变。

涉及文件：

- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs`

## pc-hot A/B

同一 180 秒 Final 早期阶段，基线 K-33：

| 符号 | 基线 | 当前 |
|---|---:|---:|
| 总指令 | 17.25B | 17.10B |
| `file_key` | 11.51M | 3.53M |
| TLSF `allocate` | 508.97M | 466.59M |
| TLSF `deallocate` | 365.99M | 336.60M |
| `purge_closed_file` | 350.89M | 350.08M |

`file_key` 下降约 69%，TLSF allocate/deallocate 各下降约 8%。

## 验证

```text
cargo test --manifest-path os/components/wateros-vfs/vfs-impl/impl-page-cache/Cargo.toml
make rv_check
make la_check
make kernel-rv-final
make kernel-rv-pre
```

完整 Final：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1896.21 cores=8 bytes=1681000 arch=riscv64
#### OS COMP TEST GROUP END buildstorm-glibc ####
```

完整墙钟为 `1896.21s`，与 K-33 的 `1873.87s` 处于噪声范围；热路径指令下降未稳定
转化为整轮时间收益，仍需继续压缩 TLSF/VirtIO/用户态热点。

Pre 60 秒 smoke：root RW 挂载成功，cyclictest、hackbench 与 LTP 早期用例进入执行，
无 panic 和 ext4 读块错误。

`qemu-img check`：`No errors were found on the image.`

## 可复核材料

```text
task: K-35 page-cache FileCacheKey reuse
date: 2026-08-07
kernel_commit: 5587cb76 + working-tree K-35
user_submodule_commit: 2f470f95fa6bf0401c4b1b7ef3bb8fc7a10b870b
architecture: riscv64
qemu_and_firmware: qemu-system-riscv64 virt, OpenSBI 1.7
image: os/sdcard-rv-pub.img (qcow2 overlay)
raw_log_path: /tmp/k35-full-rv-20260807.log
raw_log_sha256: 2741e8cb6f8938d86e4a1c7a0b0f64d1a0fdea93e8651138542a8e1a6e28ef21
pre_log_path: /tmp/k35-pre-rv-20260807.log
pre_log_sha256: e481669e59c7f62e9ed35484d7da041fcd64b6ccacb7601f572c2ee3c6c49045
pcs_current_sha256: 863c56eb9e68a2444d7775804d77d94dc76be68e7122b533cfcd376c55311fa6
overlay_qemu_img_check: ok
```
