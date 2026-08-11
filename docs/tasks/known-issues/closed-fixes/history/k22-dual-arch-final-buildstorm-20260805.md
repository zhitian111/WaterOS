# K-22 双架构 Final BuildStorm 并行验证（2026-08-05）

## 结论

提交 `3cb63a7e` 在 RISC-V64/OpenSBI 与 LoongArch64/QEMU 上均完成 CAgent 和
BuildStorm。两架构 CAgent 均为 10/10；BuildStorm 的 toolchain、minibuild、
tg-xtask 预构建及 libc 完整 release 编译均通过，未出现挂起、死锁、内核 panic
或 another-ext4 后端 I/O 错误。

本轮证明当前候选的 CAgent + BuildStorm 双架构门禁通过，不代表所有 final 测试组
均已执行。基础镜像使用 QEMU `-snapshot`，未被测试写入修改；本轮没有保留可供
离线 `e2fsck` 的写后 overlay。

## 并行配置

宿主为 32 逻辑 CPU、24 物理核心的 Intel Core i9-13980HX。两实例同时运行：

```text
RISC-V64:    host cpuset=0-7,   guest vCPU=8, memory=8G
LoongArch64: host cpuset=16-23, guest vCPU=8, memory=8G
```

逻辑 CPU 0-7 包含四组 SMT 线程，16-23 对应八个独立物理核心。该分配保证实例间
CPU 集不重叠，但两组算力并不完全对称；后续性能对比应使用物理核心拓扑一致的集合。

## 结果

| 架构 | CAgent | BuildStorm | 编译耗时 | 产物 | 结果 |
|---|---:|---|---:|---:|---|
| RISC-V64 | 10/10 | `mode=multi ok=true` | 1816.79s | 1,681,000 B | exit 0 |
| LoongArch64 | 10/10 | `mode=multi ok=true` | 1939.31s | 1,714,568 B | exit 0 |

LoongArch Cargo 输出了 `failed to save last-use data` 和整数时间转换警告。客户机
时间从 1970 年开始，缓存记账失败，但后续完整编译成功，因此不属于编译失败。

## 命令与证据

```bash
timeout 2400s env WOS_TASKSET_CPUS=0-7 WOS_SMP=8 \
  WOS_QEMU_SNAPSHOT=1 WOS_QEMU_MEM=8G \
  WOS_KERNEL=./kernel-rv-final bash ./scripts/rv_final_run.sh

timeout 2400s env WOS_TASKSET_CPUS=16-23 WOS_SMP=8 \
  WOS_QEMU_SNAPSHOT=1 WOS_QEMU_MEM=8G \
  WOS_KERNEL=./kernel-la-final WOS_SDCARD=./sdcard-la-pub.img \
  bash ./scripts/la_final_run.sh
```

```text
rv_kernel_sha256=2cca4b2ae1bfad47643cdf6354297a393c114bb5f0446ae3b8d21e8d5b80a373
la_kernel_sha256=c3ec1d5734817d451177ff8195451f8457a7b931dbe10381e12077f04e685abc
rv_log_sha256=46b64261cb4877e65b6846f8804cbc0c9adbe487f19dfe9f4b9a3d59b4622fe2
la_log_sha256=6dbef559baaebaf3087e7c9e3f9a02a7a7333680c27e966ec8cb441923f6097f
```

原始日志归档在 `os/debug-reports/archive/final-3cb63a7e-20260805/`，该目录不进入
Git。运行结束后两个基础镜像的大小和修改时间未改变。
