# 08 JH7110 SD 分区 + ext4 只读→读写 + 持久化

## 任务内容

在 VisionFive 2 上打通「SD 分区挂载 → ext4 只读 → 读写 → 卸载/同步后持久化」闭环，
用任务 03 的根镜像/分区工具生成测试镜像，并用宿主机 `e2fsck -fn` 做只读一致性校验。

这是 VisionFive 2 第一块板的**真机里程碑**。

## 实施方案

1. 用 `root_image.py` 生成带 GPT/MBR 分区的 SD 测试镜像。
2. 真机从 DW MMC 枚举 SD，挂载分区，走 `impl-another-ext4` RW。
3. 依次验证：open/read → write → close/fsync/sync/unmount → 重新打开读取一致。
4. 镜像离线后用宿主机 `e2fsck -fn` 校验，确认无脏页/损坏。

## 涉及文件 / CodeGraph 查询

- `os/scripts/root_image/root_image.py`
- `os/components/wateros-fs/**`（fs bridge / page cache / ext4 适配）
- `os/components/wateros-driver/driver-block/**`

CodeGraph：

```bash
codegraph explore "mount"
codegraph explore "fsync"
codegraph explore "sync"
codegraph explore "write_block"
```

## 验收方式

- [ ] SD 分区被识别并挂载。
- [ ] ext4 只读→读写→持久化四步闭环通过，重开读取一致。
- [ ] 离线 `e2fsck -fn` 无错。

## 验收命令

```bash
cd os
# 真机烧写后执行对应 user workload / 最小 fs 读写
make configure && make rv_check
git diff --check
# 宿主机对 SD 镜像只读校验：
#   e2fsck -fn <image>
```

## 验证环境

- L0 宿主机：`e2fsck -fn`、`git diff --check`。✅
- L1 QEMU virt：ext4 RW/持久化逻辑可在 QEMU virt（VirtIO block）先回归。✅
- L3 真机：SD 真实枚举/读写/持久化。🔴（必须）

## 任务简报

（完成后追加，格式见目录 README。）
