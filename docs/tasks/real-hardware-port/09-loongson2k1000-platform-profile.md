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

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `impl-loongson2k1000la` 从占位换成真实实现（全部 MIT，参考 NPUcore 硬件事实
    与旧分支 BSP 代码，未逐字搬运 GPL 文件）：
    - `boot.rs`：PMON/uImage 语义（旧分支的 UEFI ABI 已弃用）；`BootArgs` 为空，
      `device_tree_phys_addr()` 返回显式保存的 DTB（PMON 通常无 DTB → 0）。
    - `memory.rs`：DTB 包含内核链接地址的最大连续 RAM + 保守回退
      `0x9000_0000..0xA000_0000`（256 MiB，同参考实现）；MMIO
      `[0x1000_0000..0x3000_0000, 0x4000_0000..0x8000_0000]`。
    - `console.rs`：NS16550A 字节 MMIO @ `0x1FE2_0000`（窗口访问待真机验证）。
    - `time.rs`：CPUCFG 4/5 动态推导频率（`base*mul/div`），失败回退 100 MHz。
    - `timer.rs`：TCFG/TICLR 倒计时（rdtime.d 差值转换）。
    - `reset.rs`：PM 控制器 DTB 发现 + PMON 无 DTB 时回退 `0x1FE2_7000`；
      PM1_STS 0x0c / PM1_CNT 0x14 / RST_CNT 0x30。
    - `smp.rs`：BSP-only（补 `flush_icache_remote` 以满足 main 的 `PlatformSmp`）。
    - `asm/_start.S`（arch boot ABI：a0=CPUNUM）、`linker/link.ld`
      （`KERNEL_ENTRY_ADDRESS=0x90000000`）。
  - `src/main.rs` 新增 `loongson2k1000la` 板级模块（BSP-only bring-up、PM 发现、
    SMP 后置），使内核可链接。
  - `build.rs` 增加 `loongson2k1000la` 链接脚本分支。
  - `os/Makefile`：新增 `kernel-la2k` / `la2k_uimage` 目标；uImage 优先 mkimage，
    本机 mkimage（2026.07）缺 loongarch 架构，回退新增的
    `scripts/root_image/mk_uimage.py`（legacy 头格式与 mkimage 一致）。
  - 安装 `uboot-tools`（Arch）；`os/.gitignore` 忽略 `kernel-la2k*` 产物。
- 验收结果：
  - `cargo check --no-default-features --features loongson2k1000la,pre
    --target loongarch64-unknown-none`：通过。
  - `cargo test -p wateros-platform-impl-loongson2k1000la`：7 passed（host）。
  - `cargo build`（同 feature）：**链接成功**；`readelf -h`：LoongArch EXEC，
    entry `0x90000000`，`.text @ 0x90000000`。
  - `make kernel-la2k` / `make la2k_uimage`：uImage 生成成功并自校验
    （magic `0x27051956`、load/entry `0x90000000`、arch=24、payload CRC 匹配）。
  - `make la_check`、`make rv_check`：无回归；`git diff --check`：clean。
- 未验证/风险：
  - 真机未验证（PMON 实际加载、DRAM、UART 时序、时钟、PM 序列）；`_start.S`
    的 DMW/加载地址窗口（uImage 头用 32 位物理 `0x90000000`，64 位窗口视图
    `0x9000000090000000` 由 PMON 侧解释）保留 TODO。
  - uImage 由 Python 兜底生成（本机 mkimage 不支持 loongarch）；若后续获得支持
    loongarch 的 mkimage，目标会优先使用它。
