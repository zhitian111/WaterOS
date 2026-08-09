# WaterOS 真机移植任务简报

本文件按批次追加 VisionFive 2（JH7110）与龙芯 2K1000LA 真机移植的任务、设计、完成情况、验证证据和待测项目。不要覆盖既有记录。

## 总体分支与验证策略

- 公共工作分支：`feat/real-hardware-common`，承载远程调试、平台接口、磁盘/分区、动态设备与通用驱动。
- VisionFive 2 分支：待公共启动和内存接口稳定后，从公共分支创建 `feat/visionfive2-port`。
- 2K1000LA 分支：待公共启动和内存接口稳定后，从公共分支创建 `feat/loongson2k1000-port`。
- 无真机阶段使用三层证据：host 单元测试、QEMU 集成测试、文档驱动的寄存器/DTB 测试。只有前两类通过的功能才标为已验证。
- 必须真机确认的启动 ABI、时钟、中断路由、DMA/cache 和电气行为统一标为“待真机测试”。
- 为节省空间，优先复用现有稀疏镜像并使用 QEMU `-snapshot`；新建测试镜像原则上控制在 64 MiB 以内。

## 总体待办

- [ ] 平台化 RAM/MMIO/链接地址、启动参数和逻辑 CPU 映射。
- [x] 新增 VisionFive 2 与 2K1000LA 编译型 platform profile（driver profile 待后续批次）。
- [ ] 实现并验证 PLIC、ICU、外部中断分发和 SMP 映射。
- [ ] 构建持久根文件系统和小型分区磁盘/SD 卡镜像生成器。
- [ ] 块设备增加分区扫描与分区子设备，不再默认整盘即文件系统。
- [ ] 改进 devfs 的设备增删、稳定命名和热插拔刷新。
- [ ] 依次推进 MMC/AHCI、DWMAC/PHY、USB/HID、显示与触摸。
- [ ] 调研可复用的上游驱动；引入时记录来源、版本、修改和许可证。
- [ ] 在 PTY、用户态 socket 和登录基础具备后，将开发监视器升级或替换为用户态远程登录服务。

## 2026-08-10：批次 1——开发用 TCP 远程调试监视器

### 任务与设计

1. 审计现有 TCP、用户态 shell、TTY/PTY 和 BusyBox 路径。
2. 在 SSH/PTY 基础不足时提供可由 QEMU 验证的低成本替代。
3. 默认关闭，通过显式 feature 启用；不得被误认为安全登录服务。
4. QEMU host forwarding 只绑定主机 loopback。
5. 不创建新磁盘镜像，复用现有根盘并使用 snapshot。

审计结论：内核已有 TCP `bind/listen/accept` 和 socket 生命周期管理，但仓库内的极简用户程序尚无 socket、`dup`、PTY 或认证支持。直接引入 SSH 服务会一次耦合密码学、PTY、用户态网络包装和根文件系统部署。本批次实现内核态诊断监视器作为过渡方案，不执行任意用户命令。

### 完成内容

- [x] 新增 opt-in feature `remote-debug-monitor`，默认构建不启用。
- [x] 新增 TCP 监视器，监听 guest `2323`，支持 `help`、`ping`、`status`、`version`、`quit`。
- [x] `status` 返回 scheduler tick、online CPU mask 与内核堆快照。
- [x] 明确标注无认证、无加密、仅限开发网络。
- [x] QEMU launcher 支持 `WOS_REMOTE_DEBUG_PORT`，转发固定绑定 `127.0.0.1`。
- [x] RV/LA launcher 都覆盖端口校验和转发参数测试。
- [x] 公共分支重命名为 `feat/real-hardware-common`。

### 验证证据

- `python3 -m unittest scripts.tests.test_qemu_run`：10 项通过。
- RISC-V cross `cargo check`，启用 `remote-debug-monitor`：通过。
- LoongArch64 cross `cargo check`，启用 `remote-debug-monitor`：通过。
- `make kernel-rv EXTRA_FEATURES=remote-debug-monitor,operator-shell`：release 构建通过。
- QEMU RISC-V + virtio-net + SLIRP + host forwarding 实测通过：
  - banner 正常；
  - `ping` 返回 `pong`；
  - `status` 返回 tick、CPU mask 和 heap 数据；
  - `version`、`help`、`quit` 正常；
  - 串口记录服务监听和客户端连接。
- 测试复用已有稀疏根盘并启用 snapshot，没有生成新镜像；测试后清理约 1.2 GiB 可再生成构建产物。

### 使用方法

```bash
cd os
make kernel-rv EXTRA_FEATURES=remote-debug-monitor,operator-shell
WOS_KERNEL=./kernel-rv \
WOS_SMP=1 \
WOS_QEMU_MEM=1G \
WOS_QEMU_SNAPSHOT=1 \
WOS_SDCARD=/path/to/sdcard-rv.img \
WOS_REMOTE_DEBUG_PORT=22323 \
python3 ./scripts/qemu_run.py --arch rv --profile final

nc 127.0.0.1 22323
```

### 未验证与后续测试

- [ ] LoongArch64 QEMU 运行时连接尚未执行；本批次只有 cross check 和 launcher 测试。
- [ ] 真机网卡驱动尚不存在，因此监视器尚不能在两块目标板运行。
- [ ] 多客户端并发未实现；当前一次服务一个连接，适合 bring-up 调试。
- [ ] 当前不是登录 shell，没有认证、加密、PTY、用户进程 stdio 转接或权限隔离。
- [ ] 真机正式部署前必须保持默认关闭，或由用户态认证服务替代。

### 提交

- `[feat] add opt-in TCP debug monitor`

## 2026-08-10：批次 2——平台化内核内存布局

### 任务与设计

1. 定义与板卡无关的 RAM、MMIO 和页表探针布局契约。
2. 让架构 MM 实现消费平台布局，清除 RISC-V/LoongArch64 MM 中的 QEMU 地址常量。
3. 将现有 QEMU 地址收敛到各自 platform implementation，保持原有启动行为。
4. 用 host 单测拒绝空、未对齐或重叠的布局。

当前契约有意只描述一段主连续 RAM；这足以承载现有 QEMU 和两块目标板的早期启动，避免在尚未确认真机保留区和 DMA 约束时过早引入多内存域分配器。

### 完成内容

- [x] 新增 `PhysicalRange` 和 `KernelMemoryLayout` 平台 API。
- [x] 布局验证覆盖页对齐、RAM/MMIO 重叠、MMIO 互相重叠和探针 VA 冲突。
- [x] QEMU RISC-V 平台提供 DTB RAM 上界、virt MMIO、RTC 和 Sv39 探针 VA。
- [x] QEMU LoongArch64 平台提供高段 RAM、低端 MMIO 和 PCI MMIO 窗口。
- [x] Sv39 和 LoongArch64 内核页表初始化不再内嵌板卡地址。
- [x] 保留 DTB 位于 RAM 时截断早期帧池的保护行为。

### 验证证据

- `cargo test --manifest-path os/components/wateros-platform/platform-api/api-v0/Cargo.toml`：3 项单测和 doc-test 通过。
- RISC-V `cargo check --target riscv64gc-unknown-none-elf --no-default-features --features qemu-riscv64-opensbi,pre,heap-tlsf`：通过。
- LoongArch64 `cargo check --target loongarch64-unknown-none --no-default-features --features qemu-loongarch64-virt,pre,heap-tlsf`：通过。
- `make kernel-rv`：release 构建通过。
- QEMU RISC-V 1 GiB/8 hart/snapshot 回归通过：所有 AP 进入 Rust，virtio-net/blk 发现正常，devfs 刷新、ext4 根挂载与内核自测正常。
- QEMU 复用现有根盘并使用 snapshot，没有修改或新建镜像。

### 未验证与后续测试

- [ ] LoongArch64 QEMU 运行时回归未执行，当前有交叉编译证据。
- [ ] VisionFive 2 和 2K1000LA 的精确 RAM/MMIO/保留区需在各自 profile 引入后依据厂商 DTB 和手册填充。
- [ ] 多段 RAM、DMA 可达性和 cache 一致性策略待驱动 bring-up 阶段扩展。
- [ ] 实机上的启动器保留区、DTB 位置和内核镜像边界必须再验证。

### 提交

- `[ref] make kernel memory layout platform-owned`

## 2026-08-10：批次 3——目标板编译型平台骨架

### 任务与设计

1. 建立 VisionFive 2 与 2K1000LA 独立 platform crate，不将真机 profile 别名到 QEMU 实现。
2. 抽取与板卡无关的 OpenSBI timer/reset/HSM/IPI/remote-fence 运输层。
3. 接入顶层 feature、架构启动路径、入口汇编和链接脚本。
4. 无真机证据的 2K1000LA SMP/reset 不写猜测性寄存器操作，显式降级为单核/`Unsupported`。

### 完成内容

- [x] 新增 `wateros-platform-impl-opensbi-common`，不包含 UART、内存或链接地址等板卡假设。
- [x] 新增 `wateros-platform-impl-jh7110-visionfive2`。
- [x] VF2 链接入口为 `0x4020_0000`，RAM 从 `0x4000_0000` 开始并可从 DTB 取上界。
- [x] VF2 early console 使用 DW APB UART0 `0x1000_0000`、`reg-shift=2`、32 位 MMIO 和 LSR `+0x14`。
- [x] VF2 时钟回退为 OpenSBI 报告的 4 MHz，仍允许 DTB 覆盖。
- [x] 新增 `wateros-platform-impl-loongson2k1000la`。
- [x] 2K1000LA 链接入口为 `0x9000_0000`，early console 使用 BSP `serial0` 的 `0x1fe2_0000` ns16550a。
- [x] 2K1000LA 暂用 1 GiB 板型的保守高段 RAM 窗口，启动参数解析完成前不声称适配其它容量。
- [x] 顶层新增 `visionfive2` 和 `loongson2k1000la` profile，当前驱动层显式使用 dummy machine。
- [x] 通用内核启动模块按 ISA/firmware 分组，不再以 QEMU 名称作为模块语义。

### 验证证据

- VF2 platform host 单测：1 项 RAM/MMIO/探针布局测试通过。
- 2K1000LA platform host 单测：1 项保守内存布局测试通过。
- `visionfive2,pre,heap-tlsf` RISC-V cross check 通过。
- `loongson2k1000la,pre,heap-tlsf` LoongArch64 cross check 通过。
- 两个新 profile 的 release 内核都完成链接。
- `llvm-readelf` 确认 VF2 ELF 为 RISC-V/entry `0x40200000`，2K1000LA ELF 为 LoongArch/entry `0x90000000`。
- `cargo tree` 确认新 profile 不依赖任何 QEMU platform implementation。
- 现有 QEMU RISC-V 和 QEMU LoongArch64 profile 交叉检查回归通过。

### 未验证与后续测试

- [ ] VF2 `0x4020_0000` 需与实际 U-Boot `kernel_addr_r` 核对；DW APB UART 32 位访问需真机串口验证。
- [ ] VF2 MMIO 暂为早期宽窗口，待 PLIC/clock/reset/driver 引入时按 DTB 缩窄。
- [ ] 2K1000LA 需解析 a0/a1/a2 启动参数及内存表，取代固定 RAM 上界。
- [ ] 2K1000LA StableCounter 的 100 MHz 回退仅用于编译 bring-up，需经 CPUCFG/固件或真机测量确认。
- [ ] 2K1000LA 当前只启动 BSP；mailbox/IPI、ICU、双核启动和远端 TLB flush 全部待实现。
- [ ] 2K1000LA reset/shutdown 当前返回 `Unsupported`，待核对 PM/reset-controller 序列。
- [ ] 两个新 profile 当前都使用 dummy machine driver，只能证明内核可编译/可链接，不能证明外设可用。

### 提交

- `[feat] add real-hardware platform profiles`

## 2026-08-10：批次 4——有边界的 MBR 分区块设备

### 任务与设计

1. 在公共块设备层读取 MBR，而不是让 ext4 或 devfs 猜测分区偏移。
2. 将每个主分区包装成独立块设备，所有读写先做分区内边界检查，再平移到父设备 LBA。
3. 为注册项记录“整盘/分区”角色；devfs 只为真实发现的分区创建 `/dev/vdXn`。
4. 保留整盘文件系统兼容路径：没有 MBR 签名时只创建 `/dev/vdX`，根挂载自动回退整盘。

本批次只接受传统 MBR 的四个主分区。GPT protective MBR 和扩展/逻辑分区会被明确识别并拒绝，避免尚未实现的格式被错误暴露为普通分区。

### 完成内容

- [x] 新增 MBR 签名、主分区项、磁盘范围和分区重叠校验。
- [x] 新增 `PartitionBlockDevice`，读写均校验长度、溢出和分区末端。
- [x] 整盘注册时自动发现并登记有效主分区。
- [x] 注册表增加整盘/分区角色及父设备、分区号元数据。
- [x] 两套 devfs 实现删除伪造的 `vda1`/`vda2`，按元数据生成节点。
- [x] 默认根设备优先真实 `/dev/vda1`，不存在时回退 `/dev/vda`。
- [x] GPT、扩展分区和损坏表会记录跳过原因；无 MBR 签名的整盘布局不告警。

### 验证证据

- block API host 单测 3 项通过，覆盖两个主分区、LBA 写平移、越界读、坏签名、超磁盘范围、GPT、扩展分区和分区重叠。
- 两套 devfs crate 的 host `cargo check` 通过。
- 现有 QEMU RISC-V 与 LoongArch64 profile 交叉检查通过。
- `make kernel-rv` release 构建通过。
- 使用临时 32 MiB 稀疏 MBR 镜像验证 QEMU RISC-V：只含一个从 LBA 2048 开始的 Linux 分区，分区内 4 KiB block ext4 成功通过 `/dev/vda1` 读写挂载；镜像已清理。
- 使用已有无分区 ext4 镜像和 QEMU snapshot 回归：块设备数为 1，根文件系统成功从 `/dev/vda` 读写挂载，QEMU 正常退出且原镜像未修改。

### 未验证与后续测试

- [ ] GPT 只检测 protective MBR，尚未解析 GPT header、entry array 和 CRC。
- [ ] MBR 扩展/逻辑分区尚不支持。
- [ ] 当前注册表只支持启动期追加，不处理设备热移除及其子分区失效。
- [ ] 真实 SD/eMMC/AHCI 驱动尚未接入，因此分区扫描尚未在目标板存储控制器上运行。
- [ ] 真机仍需验证设备容量、扇区大小、缓存一致性、写屏障和掉电恢复行为。

### 提交

- `[feat] add bounded MBR partition devices`
