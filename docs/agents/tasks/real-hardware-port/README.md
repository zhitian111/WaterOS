# 真实硬件移植任务手册（Loongson 2K1000 + VisionFive 2）

本目录是 WaterOS 向两块物理板移植的**顺序任务清单**。每一个任务（`NN-*.md`）
对应一次**可回归、可验收的 commit**。任务按依赖顺序编号，前一个通过验收后才进入
下一个；禁止跨任务混改无关模块。

## 目标平台

| 板子 | SoC | 架构 | 启动固件 | 主要存储 | 主要串口 |
|------|-----|------|----------|----------|----------|
| Loongson 2K1000 | LS2K1000LA | LoongArch64 | PMON（uImage） | SATA/AHCI | NS16550 @ 0x1fe2_0000 |
| StarFive VisionFive 2 | JH7110 | RISC-V64 | OpenSBI + U-Boot | eMMC/SD（DW MMC） | DW APB UART |

## 已定的关键决策（约束后续所有任务）

1. **许可证**：WaterOS 保持 **MIT**。禁止把 GPL 文件原样拷入。
   - 第三方宽松许可 crate 可直接复用：`isomorphic_drivers`（AHCI）、`pci`（robigalia）、
     以及 WaterOS 已有的 `impl-uart-16550`。
   - 参考实现 NPUcore-IMPACT（GPL-3.0）只当作**规格来源**，按硬件事实（地址、寄存器、
     DMW 配置、CPUCFG 时钟算法）在 WaterOS 内**重新实现表达**，不逐字搬运其源文件。
2. **第一轮范围**：先「能启动 + 串口打印 + 定时器 + 块设备读写」，外部中断控制器
   （LIOINTC/PLIC 之外的高阶 IRQ）、网络、USB、显示后置。
3. **架构层不重写**：WaterOS 已有 `impl-riscv64`/`impl-loongarch64`（QEMU），只补板级
   缺口，不复刻参考实现的页表/trap/寄存器层。
4. **参考仓库**：`Fediory/NPUcore-IMPACT`（分支 `ext4`），本地审查副本在
   `/tmp/npucore-sparse`（若丢失可重新稀疏克隆）。
5. **旧工作树仅作资产**：`feat/real-hardware-common`、`feat/visionfive2-port`、
   `feat/loongson2k1000-port` 只 cherry-pick/移植，不直接 merge（main 领先 220 提交）。
6. **`user/` 已是 rootfs 生成器，不是从零建**：当前已有 `base-layout` 骨架、
   busybox、`make image ARCH=rv|la` 产出 raw EXT4。上板要补的是「分区整盘镜像 +
   userspace-init + 动态库/设备节点收尾」，见阶段 D 的任务 12/13。

## 任务顺序总览

### 阶段 A：共享平台基础（两块板都依赖，先做）

| 编号 | 文档 | 可 QEMU/宿主机验收 |
|------|------|--------------------|
| 00 | `00-platform-memory-api.md` | ✅ 高（QEMU virt + host） |
| 01 | `01-machine-driver-external-irq.md` | ✅ 高（QEMU virt） |
| 02 | `02-driver-common-dummy-profile.md` | ✅ 高（host 单测 + QEMU virt） |
| 03 | `03-root-image-devfs-partition.md` | ✅ 高（host 工具 + QEMU 镜像） |
| 04 | `04-board-feature-build-wiring.md` | ✅ 高（cargo check 两架构） |

### 阶段 B：VisionFive 2 / JH7110（第一块板）

| 编号 | 文档 | 可 QEMU/宿主机验收 |
|------|------|--------------------|
| 05 | `05-jh7110-platform-profile.md` | 🟡 中（host 单测 + QEMU virt 逻辑对照） |
| 06 | `06-jh7110-driver-profile.md` | 🟡 中（host 单测 + QEMU virt PLIC 对照） |
| 07 | `07-dw-mmc-block.md` | 🟠 低（host 单测为主，真机读 SD） |
| 08 | `08-jh7110-sd-ext4-rw.md` | 🔴 真机为主（ext4 逻辑可 QEMU 回归） |

### 阶段 C：Loongson 2K1000（第二块板）

| 编号 | 文档 | 可 QEMU/宿主机验收 |
|------|------|--------------------|
| 09 | `09-loongson2k1000-platform-profile.md` | 🟡 中（若拿到 2K1000 QEMU fork 可仿真，否则真机） |
| 10 | `10-loongson2k1000-sata-ahci.md` | 🟠 低（host 单测 + 真机 SATA） |
| 11 | `11-loongson2k1000-external-irq.md` | 🔴 真机 |

### 阶段 D：rootfs 正经化（与阶段 B/C 并行开发，板级挂载前完成）

| 编号 | 文档 | 可 QEMU/宿主机验收 |
|------|------|--------------------|
| 12 | `12-rootfs-partitioned-image.md` | ✅ 高（host + QEMU 整盘镜像） |
| 13 | `13-userspace-init.md` | ✅ 高（QEMU virt） |

### 阶段 E：后续功能（到达时再拆成单 commit 文档）

- 网络：2K1000 GMAC / JH7110 GMAC（stmmac/DW MAC）
- 2K1000 MMC/SD（如需第二存储）
- USB、显示/GPU、GPIO/pinmux 细化
- rootfs 动态库/共享库收尾（OpenJDK/Nano-X 的 loader 与 `/glibc`、`/musl`）

> 阶段 E 在进入前，会按与上述相同的模板补写 `14-*.md` 起的新文档。

## 每个任务文档的固定字段

每个 `NN-*.md` 必须包含：

1. **任务内容**：做什么、为什么、完成边界。
2. **实施方案**：分层、步骤、关键决策、依赖与风险。
3. **涉及文件 / CodeGraph 查询**：预计改动的文件 + 用于定位的
   `codegraph explore "<符号或问题>"` 命令。
4. **验收方式**：可勾选的客观验收点。
5. **验收命令**：在 `os/` 下执行的精确命令。
6. **验证环境**：宿主机 / QEMU / 真机的覆盖比例与做不到的部分。
7. **任务简报**：完成后必须追加一节《任务简报》，简要记录实际改动、验收命令结果、
   未验证项。

## 任务简报约定（每个 commit 完成后）

每个任务通过验收并提交后，在该任务的 `.md` 文档末尾追加：

```markdown
## 任务简报

- 完成日期：YYYY-MM-DD
- commit：<短哈希>
- 实际改动：<一句话 + 关键文件列表>
- 验收结果：<命令 + 结果摘要>
- 未验证/风险：<真机未跑的部分、遗留 TODO>
```

## 验证环境分级（回答“多少能上 QEMU”）

- **L0 宿主机**：`cargo check`、`cargo test`、`git diff --check`、镜像/工具脚本。覆盖
  全部任务的**编译与静态正确性**，约 100%。
- **L1 QEMU virt（现有）**：WaterOS 已有的 `qemu-riscv64-virt` / `qemu-loongarch64-virt`
  可回归公共层、VFS/ext4、syscall、中断路由、driver facade 等。**阶段 A 与大部分公共
  逻辑可在此完整验收**。
- **L2 板级 QEMU fork（可选）**：NPUcore 带过一个改版 `util/qemu/2k1000`；JH7110 有
  社区/厂商 QEMU fork。若接入，2K1000 的 PMON/uImage、UART、SATA 与 JH7110 的 UART/MMC
  可在仿真里先过一遍。
- **L3 真机**：真实 DRAM 训练、时钟/PLL、pinmux、IRQ 时序、DMA、cache 一致性、SD/SATA
  实际读写与烧写稳定性，**只能在物理板验证**。

**总体估算**：约 **50–60%** 的提交可完全在宿主机 + QEMU 上验收；若引入板级 QEMU fork，
可提高到 **70–80%**（到“能启动打印 + 基本块设备读写”）；剩下 20–30% 必须真机。
板级外设任务的宿主机/QEMU 可验证部分，主要体现在驱动状态机单测、DTB/内存解析单测、
以及用 QEMU virt 对照验证“同一契约的通用路径”。
