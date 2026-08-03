# K-10 BuildStorm 并行退出探针（2026-08-03）

## 目标

在不运行完整 446 单元 BuildStorm 的白天窗口，验证多线程 exec 清理修复后的 Cargo、
rustc、futex、链接、线程 join、进程回收和文件系统写入链。测试使用 qcow2 overlay，
没有修改主办方 raw 基线镜像。

## 结果

- CAgent 仍为 10/10，约 3.7 秒完成。
- guest 同时编译 8 个独立 crate，Cargo 使用 `-j8`。
- 8 个 hart 的 timer 持续增长，QEMU 峰值使用约 6.9 个宿主核。
- `BUILDSTORM_PROBE_END rc=0 built=8 elapsed_s=229.45`。
- bringup 成功回收脚本，内核正常关机，宿主 QEMU 返回 0。
- 未出现 panic、SIGSEGV、signal frame 错误或无 syscall 进展告警。
- overlay 的 ext4 五阶段 `e2fsck -fn` 通过；`qemu-img check` 无错误。

本轮证明修复后的并行 rustc 可以完成并退出，但不替代正式 BuildStorm 门禁。下一夜间
窗口必须使用 fresh overlay 运行官方脚本，直到输出
`BUILDSTORM_COMPILE mode=multi ok=true`，随后再次离线检查 overlay。

## 复现信息

```text
kernel_base_commit: d3e9d3a4be8db65342d4f34e39724d10fdc00cc0
user_submodule_commit: 2f470f95fa6bf0401c4b1b7ef3bb8fc7a10b870b
kernel_sha256: 6a74798bd7e8cb23b266c526efe56a2d991c0c695d03cc0373a64fb61e1ec8e5
base_image_sha256: e4912bf0084dd53bb7eae99a1d2e61311a8fcf823b6ec1a761c7317c33d84fe2
overlay: /tmp/wateros-buildstorm-probe-20260803.qcow2
overlay_sha256_after: 2bd24c49e21f3c4a43abc51ad60f249f6361a72b4f5d812e9694b126e31da442
qemu: 11.0.2, riscv64 virt, OpenSBI, 8 CPU, 8 GiB
guest_probe: os/scripts/guest_buildstorm_parallel_probe.sh
raw_log_path: /tmp/wateros-buildstorm-parallel-probe-20260803.log
raw_log_sha256: 8d5eee562696b0b0028ab780a03ee4392dc0975c7c23c12b0b11fb9e465b58fe
```
