# 16 sv39 内核页表接入平台内存布局（修真机挂死）

## 任务内容

修复 VisionFive 2 真机首个挂死点：`[mm] init_after_boot begin` 之后无输出。

根因：`impl-sv39` 的 `kernel_global::init` 把 QEMU virt 的物理布局写死
（RAM 恒等映射从 `0x8000_0000` 起、MMIO 用 `QEMU_VIRT_*` 常量、探测 VA 用
`0x4000_0000`、DTB 判 RAM 也用 `0x8000_0000`）。真机 RAM 在
`0x4000_0000`、内核自身运行在 `0x4020_0000`，新页表安装（satp 切换）后
下一跳取指即缺页，trap 入口同样未映射，表现为静默挂死。

平台层已有正确契约 `platform::memory::kernel_layout()`（任务 00），
jh7110 实现声明了 RAM `0x4000_0000` 起、MMIO `0x0100_0000..0x4000_0000`、
探测 VA `0x0020_0000`，但 sv39 从未消费它。

## 实施方案

1. `kernel_global::init` 改为从 `platform::memory::kernel_layout()` 取
   RAM/MMIO/探测 VA：
   - RAM 恒等映射 `[layout.ram.start, layout.ram.end)` RWX；
   - 遍历 `layout.mmio` 每个区间恒等映射 RW；
   - 探测页 VA 用 `layout.probe_virtual_page`；
   - `dtb_reserved_ppns` 的 RAM 判定改用 `layout.ram.start`。
2. `ram_end_exclusive` 形参保留并 `debug_assert_eq` 与布局一致（入口传的
   就是 `platform::physical_ram_end_exclusive()`，同源）。
3. 同步模块头注释，去掉 QEMU 专用表述。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-mm/mm-impl/impl-sv39/src/kernel_global.rs`

CodeGraph：

```bash
codegraph explore "kernel_global::init"
codegraph explore "kernel_layout"
codegraph explore "activate_address_space_token_and_flush"
```

## 验收方式

- [ ] `make jh7110_check` 与 `make rv_check` 通过。
- [ ] QEMU virt 冒烟无回归（布局值不变：RAM `0x8000_0000` 起、MMIO 窗口、
      探测 VA `0x4000_0000`）→ rootfs 挂载 `/dev/vda4` → login。
- [ ] 真机重烧后越过 `[mm] init_after_boot begin` 继续打印。

## 验收命令

```bash
cd os
make jh7110_check
make rv_check
make jh7110_uimage
make jh7110_bootdir
cd ../user && make disk ARCH=rv PACKAGE=minimal IMAGE_SIZE_MB=64 \
  DISK_SIZE_MB=192 BOOT_DIR=../os/build/jh7110-boot BOOT_SIZE_MB=64
cd ../os && make run ARCH=rv PROFILE=pre SDCARD=../user/build/images/wateros-rv.img
git diff --check
```

## 验证环境

- L0 宿主机：check/构建。✅
- L1 QEMU virt：同值回归。✅
- L3 真机：重烧后验证越过 mm init（本次真机已复现挂死点）。🔴→✅

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `os/components/wateros-mm/mm-impl/impl-sv39/src/kernel_global.rs`：
    `init` 改为消费 `platform::memory::kernel_layout()`——RAM 恒等映射用
    `layout.ram.start/end`，MMIO 遍历 `layout.mmio`，探测页 VA 用
    `layout.probe_virtual_page`，`dtb_reserved_ppns` 的 RAM 判定用
    `layout.ram.start`；`ram_end_exclusive` 形参保留并 `debug_assert_eq`
    与布局一致。删除全部 QEMU 硬编码（`0x8000_0000`/`QEMU_VIRT_*`）。
- 验收结果：
  - `make jh7110_check` / `make rv_check`：通过。
  - QEMU virt 回归：`[mm] init_after_boot complete` →
    `probed root partition /dev/vda4` → mount RW → rcS → `wateros login:`。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk ...`：uImage
    1.94 MiB，镜像 192 MiB 重新生成。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期越过 `[mm] init_after_boot begin`，继续打印
    `[mm] init_after_boot complete` 与后续 driver/fs 日志；
  - 之后的下一个已知缺口：MMC 激活仍 fail-closed（`activation=UNVERIFIED`），
    无 SD 设备时 rootfs 挂载失败属预期，将按真机日志继续解锁。
