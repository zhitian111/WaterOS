# 09 Loongson 2K1000 平台 profile（PMON/uImage 启动 + DMW + 内存 + 时钟 + 串口 + reset）

## 任务内容

为 Loongson 2K1000LA 建立平台 profile `impl-loongson2k1000la`。**必须保持 MIT**：
只参考 NPUcore-IMPACT（GPL-3.0）与硬件手册/Linux DT binding 中的**事实**，在 WaterOS 内
重新实现表达，不逐字搬运其源文件。

关键事实（已核对，可写入实现）：

- 启动固件为 PMON，内核打 **uImage**（`mkimage -A loongarch -O linux -T kernel`），
  链接基址物理 `0x9000_0000`，入口 `_start` 自行配置 DMW0/DMW1 段窗口；
- 物理 RAM `0x9000_0000`，256 MiB（`0x9000_0000..0xA000_0000`）；
- NS16550 UART 基址 `0x1fe2_0000`（复用 `impl-uart-16550` 的 `Byte16550`）；
- ACPI/PM1（S5/reset）基址 `0x1fe2_7000`；
- PCI 配置空间基址 `0xfe00_0000`（供任务 10 使用）；
- 时钟频率用 `cpucfg` index 4/5 动态推导：`base * mul / div`；
- 定时中断走 LoongArch CSR `TCfg`/`TIClr` + `ECfg` 的 `LineBasedInterrupt::TIMER`，
  timebase 用 `rdtime.d`。

## 实施方案

1. 新增 `impl-loongson2k1000la`：`entry.S`（DMW 段保留 + 跳转屏障）、`boot.rs`、
   `memory.rs`（实现任务 00 契约）、`console.rs`（NS16550 注册）、`time.rs`（CPUCFG 频率）、
   `timer.rs`（TCfg/TIClr）、`reset.rs`（PM1 S5）、`smp.rs`（先 BSP-only/unsupported）。
2. 新增 `linker.ld`（`BASE_ADDRESS=0x9000_0000`）与 Makefile 的 uImage 生成目标。
3. 复用 `impl-uart-16550`，不新写串口寄存器驱动。
4. 从旧 `feat/loongson2k1000-port` 只提取「地址/寄存器常量与 UEFI→PMON 的差异说明」作为
   审计对照，不作为代码来源。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/**`（新增）
- `os/components/wateros-platform/linker/**`
- `os/Makefile`、`os/Cargo.toml`
- `os/components/wateros-driver/driver-character/character-impl/impl-uart-16550/**`

CodeGraph：

```bash
codegraph explore "device_tree_phys_addr"
codegraph explore "boot"
codegraph explore "console"
codegraph explore "register_character_device"
```

## 验收方式

- [ ] `--features loongson2k1000la,pre` 能 `cargo check`（`loongarch64-unknown-none`）通过。
- [ ] 生成 uImage 的 Makefile 目标可运行，段地址/入口正确（`readelf -h` 核对）。
- [ ] UART 能打印内核 banner（真机项；若接入 2K1000 QEMU fork 可先仿真）。
- [ ] 未引入 GPL 文件（diff 中无 NPUcore 源文件逐字拷贝）。

## 验收命令

```bash
cd os
make configure
make la_check
cargo check --no-default-features --features loongson2k1000la,pre --target loongarch64-unknown-none
git diff --check
# 有 uImage 目标后：readelf -h ./target/.../wateros 核对 entry/段
```

## 验证环境

- L0 宿主机：`cargo check`、`readelf`。✅
- L2 板级 QEMU fork：NPUcore 带过改版 `util/qemu/2k1000`，接入后可仿真 PMON/uImage 启动。🟡
- L3 真机：真实 PMON 加载、DRAM、UART 时序、时钟。🔴（关键里程碑）

## 任务简报

（完成后追加，格式见目录 README。）
