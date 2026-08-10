# WaterOS 真机移植任务简报

本文件按批次追加 VisionFive 2（JH7110）与龙芯 2K1000LA 真机移植的任务、设计、完成情况、验证证据和待测项目。不要覆盖既有记录。

## 总体分支与验证策略

- 公共工作分支：`feat/real-hardware-common`，承载远程调试、平台接口、磁盘/分区、动态设备与通用驱动。
- VisionFive 2 分支：`feat/visionfive2-port`，独立工作树 `WaterOS_visionfive2_port`。
- 2K1000LA 分支：`feat/loongson2k1000-port`，独立工作树 `WaterOS_loongson2k1000_port`。
- 无真机阶段使用三层证据：host 单元测试、QEMU 集成测试、文档驱动的寄存器/DTB 测试。只有前两类通过的功能才标为已验证。
- 必须真机确认的启动 ABI、时钟、中断路由、DMA/cache 和电气行为统一标为“待真机测试”。
- 为节省空间，优先复用现有稀疏镜像并使用 QEMU `-snapshot`；新建测试镜像原则上控制在 64 MiB 以内。

## 总体待办

- [ ] 平台化 RAM/MMIO/链接地址、启动参数和逻辑 CPU 映射。
- [x] 新增 VisionFive 2 与 2K1000LA 编译型 platform profile（driver profile 待后续批次）。
- [ ] 实现并验证 PLIC、ICU、外部中断分发和 SMP 映射。
- [x] 构建持久根文件系统和小型分区磁盘/SD 卡镜像生成器。
- [x] 块设备增加分区扫描与分区子设备，不再默认整盘即文件系统。
- [x] 改进 devfs 的设备增删、稳定命名和热插拔刷新。
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

## 2026-08-10：批次 5——小型物理根盘镜像工具

### 任务与设计

1. 用无特权宿主工具生成可供 QEMU 与真机共用的原始磁盘/SD 卡镜像。
2. 用声明式 JSON 清单描述根卷目录、权限、内联文件和外部二进制来源。
3. 固定 MBR、1 MiB 分区对齐、ext4 UUID/卷标/block size 与兼容 feature。
4. 构建完成后先独立校验，再原子替换输出；失败不得破坏已有镜像。
5. 用 QEMU snapshot 验证内核自动发现分区并从 `/dev/vda1` 读写挂载。

默认镜像逻辑容量 32 MiB，采用稀疏文件；空骨架在本机只占约 104 KiB。默认清单只提供物理根卷骨架和 `/etc/wateros-release`，不把数 GiB 的比赛测试树隐式复制进去。架构相关 BusyBox、动态链接器和应用可通过单独清单的 `source` 项加入。

### 完成内容

- [x] 新增 `os/scripts/root_image/root_image.py`，构建过程不 mount、不使用 `sudo`。
- [x] 新增默认 `rootfs-manifest.json`，预建 `/bin`、`/sbin`、`/etc`、`/proc`、`/dev`、`/tmp`、`/root`、`/usr` 和 `/var` 骨架。
- [x] 固定 DOS/MBR disk id `0x574f5301`、2048 起始扇区、单个 `0x83` 分区。
- [x] 固定 4 KiB ext4 block、UUID、卷标，关闭 journal 并保留 `64bit` descriptor feature。
- [x] 输出使用同目录临时文件；MBR、ext4 和清单验证全部通过后才原子替换旧镜像。
- [x] 独立 verifier 检查 MBR 签名、分区范围/重叠/类型/对齐、`e2fsck -fn`、ext4 feature、必需路径和声明文件逐字节内容。
- [x] Makefile 新增 `physical-root-image` 与 `verify-physical-root-image`。
- [x] 新增工具说明，记录宿主依赖、清单格式和真机风险。

### 验证证据

- Python 单测 4 项通过：正常 MBR、越界/重叠分区、清单路径逃逸和权限、失败的强制重建不破坏旧镜像。
- `py_compile` 通过。
- Makefile 实际构建和独立校验 32 MiB 镜像通过；布局为 start=2048、sectors=63488，实际分配约 104 KiB。
- QEMU 对 ext4 feature 做了四组最小变量实验：默认空卷成功、默认预填充卷成功、仅关闭 journal 成功、关闭 `64bit` 稳定以 `InvalidPath` 失败。由此将 `64bit` 纳入 verifier 强制约束。
- RISC-V release 内核构建通过。
- 新镜像 QEMU snapshot 回归通过：virtio-blk 注册后 `block=2`，devfs 刷新成功，根卷从 `/dev/vda1` 读写挂载，QEMU 正常退出。
- LoongArch64 release 内核构建及同镜像 QEMU snapshot 回归通过；启动进入只会在根卷挂载成功后发布的 BusyBox 队列并正常退出。当前 LoongArch early serial 的 ANSI 光标输出覆盖了前段日志，后续需改善日志捕获以保留逐行 `/dev/vda1` 证据。
- snapshot 运行后再次执行独立 verifier 仍通过，基准镜像未被写回。

### 已知限制、未验证与后续测试

- [ ] 默认最小清单没有 BusyBox/动态链接器；自动比赛队列找不到 `/glibc/cagent_testcode.sh` 是预期行为，不代表用户环境已经完成。
- [ ] 需要为 RISC-V64 与 LoongArch64 分别提供发布清单，并验证 ELF 架构、解释器和共享库闭包。
- [ ] `another_ext4` 对非 `64bit` descriptor 小卷返回 `EINVAL` 的根因仍在 vendor 内部；当前通过明确镜像契约规避。
- [ ] journal 为控制体积而关闭，当前不承诺写入时掉电恢复；生产策略需评估只读根、独立数据分区或完善 journal/write barrier。
- [ ] e2fsprogs 跨版本不保证输出逐字节一致；正式发布若要求 bit-reproducible，需要固定构建容器和工具版本。
- [ ] VisionFive 2 SD/eMMC 与 2K1000LA AHCI 上的容量、flush、cache coherency、DMA 和掉电行为均待真机测试。

### 提交

- `[feat] add reproducible physical root image tooling`

## 2026-08-10：批次 6——动态设备拓扑与 devfs 自动同步

### 任务与设计

1. 为块、字符、输入和显示注册表提供稳定 slot、快照与受控注销。
2. 用全局单调 topology generation 通知上层设备成员变化。
3. devfs 在列举、查找和选择根盘前按 generation 自动同步，不再依赖启动调用者手工刷新。
4. 消费者不能再用 `0..active_count` 猜测 slot；改用带稳定 ID 的 snapshot。
5. 注销整盘时同步移除所有分区节点，已有 `Arc` 句柄继续存活到持有者释放。

slot 只追加、不移动、不复用，避免旧索引在设备移除后指向另一设备。`*_device_count()` 继续表示活动设备数，仅用于统计；带空洞的实际枚举必须使用 snapshot。generation 只是缓存失效提示，不替代设备事件队列。

### 完成内容

- [x] `driver-api` 新增全局 topology generation，使用 Acquire/Release 原子语义。
- [x] block registry 改为稳定可空 slot，并新增带角色 snapshot 与注销 API。
- [x] 整盘及其 MBR 分区在一次注册表事务中同时发布，generation 只递增一次。
- [x] 注销整盘会清除所有引用该父 slot 的分区；重复注销返回 `false`。
- [x] character/input/display registry 改为稳定可空 slot，新增 snapshot/注销入口。
- [x] 字符 snapshot 在释放注册表锁后才锁具体设备读取 kind，避免锁嵌套。
- [x] kernel devfs 与简化 devfs 都会在 generation 变化后自动重建缓存。
- [x] devfs 不再把 RTC 和 null 错误暴露为 `ttyS*`；`/dev/console`、`/dev/tty` 绑定当前首个 Serial。
- [x] GUI `InputBridge` 按稳定 ID 添加和删除输入设备状态。
- [x] VFS 默认控制台改用字符设备 snapshot，不会因中间 slot 空洞漏掉后续串口。
- [x] 注销 API 注释明确要求真机驱动先停止中断/DMA；该硬件顺序尚未验证。

### 验证证据

- block API host 单测 4 项通过，其中动态注册测试覆盖 generation、分区发现、父盘级联注销、重复注销和 slot 不复用。
- character API host 单测通过，覆盖 kind snapshot、注销、重复注销和 slot 不复用。
- 简化 devfs host 单测通过：无需显式 `refresh()`，注册后节点自动出现，注销后 lookup 与节点列表自动消失。
- input、display 和 kernel devfs host `cargo check` 通过。
- GUI host 单测 9 项通过；新增用例验证 `InputBridge` 无需重建即可发现注册设备，并在注销后丢弃对应跨事件状态。
- fd-session 单 crate 无架构 feature 检查触发既有 `Arch*Impl` 未定义；RISC-V64 与 LoongArch64 顶层 profile 交叉检查均通过，证明其实际配置下 snapshot 改造可编译。
- RISC-V release 内核构建通过。
- 使用上一批 32 MiB 分区镜像做 QEMU snapshot 回归：`block=2`、`character=3`，devfs 节点由 21 个收敛为 19 个（删除 RTC/null 的伪 `ttyS*`），根卷仍从 `/dev/vda1` 读写挂载并正常退出。

### 已知限制、未验证与后续测试

- [ ] 注册表注销只改变新枚举可见性；已挂载根卷或已打开 fd 持有的共享句柄仍存活，尚无统一的 device-gone I/O 错误状态。
- [ ] 未实现根卷卸载/阻止拔盘策略；物理根盘热拔属于高风险操作，驱动接入前不得宣称支持。
- [ ] QEMU 本批没有执行 monitor `device_del`，因为当前 virtio 驱动没有中断/DMA quiesce 与 PCI/MMIO remove 回调；host 状态机测试不能替代该硬件流程。
- [ ] 输入设备已能从 GUI 动态移除，但尚未暴露 Linux `/dev/input/eventN` 字符设备语义。
- [ ] display 注销要求 GUI 先 shutdown；尚无自动通知正在呈现的 GUI runtime。
- [ ] network registry 尚未迁移到同一套 topology/注销模型，网卡热插拔仍待后续批次。
- [ ] slot 永不复用会随极端长期热插拔增长；当前嵌入式启动生命周期可接受，未来可引入带 generation 的复合 `DeviceId` 后安全复用。
- [ ] VisionFive 2/2K1000LA 上屏蔽中断、停止 DMA、cache flush 和设备断电顺序均待真机验证。

### 提交

- `[feat] add dynamic device topology tracking`

## 2026-08-10：批次 7——evdev 输入字符设备与多消费者扇出

### 任务与设计

1. 将输入设备按稳定 slot 暴露为 `/dev/input/eventN`。
2. 采用 RISC-V64/LoongArch64 共用的 Linux 64 位 `input_event` 小端布局（24 字节）。
3. 在原始硬件队列与消费者之间增加有界扇出，避免 GUI 与用户态读取互相抢事件。
4. 支持 VFS 的 `prepare_read/finish_read` 事务，用户复制失败时恢复尚未提交的完整事件。
5. 用内存状态机、devfs、两架构构建和带 VirtIO 键鼠的 QEMU 分层验证。

每个订阅者有独立的 256 事件队列；队列满时丢弃该订阅者最旧事件并累计 dropped 计数，不拖慢其他消费者。当前记录的 `timeval` 两字段明确置零，待平台单调时钟接口稳定后接入。

### 完成内容

- [x] 输入注册表增加独立订阅 API，硬件事件一次取出后复制给所有活动订阅者。
- [x] GUI `InputBridge` 改用订阅，不再直接消费硬件队列。
- [x] 新增只读 evdev 字符适配器，输出 `sec:i64/usec:i64/type:u16/code:u16/value:i32` 小端记录。
- [x] 小于一个完整记录的 read 返回参数错误；无事件保持非阻塞语义；write 不支持。
- [x] evdev 支持字符设备读预留、提交和完整事件后缀回滚；部分暴露的记录按已消费处理，避免重复事件。
- [x] devfs 按输入稳定 slot 自动生成 `/dev/input/eventN`，注销后 generation 刷新会移除节点。
- [x] 打开的旧句柄在设备注销后停止取得事件，不会重新绑定到其他 slot。

### 验证证据

- input API host 单测 2 项通过：双订阅者收到完全相同事件；24 字节布局、零时间戳及事务后缀回滚正确。
- kernel devfs host 单测通过：event 节点和字符绑定随注册出现、随注销消失。
- GUI host 单测 9 项通过，包括设备动态发现/移除和原有键盘、指针解释测试。
- 仓库规定的 RISC-V `make check` 通过；RISC-V GUI release 内核实际构建通过。
- LoongArch64 `make kernel-la` 通过。
- RISC-V QEMU 以 VirtIO GPU、keyboard、tablet 启动：成功注册 Keyboard #0 与 Pointer #1；devfs 报告 `input=2`、`total_nodes=18`，对应 `/dev/input/event0` 与 `event1`；GUI 初始化、`/dev/vda1` 根卷挂载及 VFS 自测继续通过。
- QEMU 使用上一批 32 MiB 稀疏根盘的 snapshot 模式，未写回基准镜像。

### 已知限制、未验证与后续测试

- [ ] `input_event` 时间戳当前为零；需接入平台单调时钟，并验证 Linux 用户程序对时钟域的预期。
- [ ] 当前 QEMU 图形运行关闭 monitor，已验证键盘/平板枚举与节点生成，但未自动注入按键/坐标后从用户态读取字节；host 扇出与编码测试不能替代这一端到端测试。
- [ ] 尚未实现 evdev `EVIOCG*` ioctl、设备能力 bitmap、grab、FF/LED；当前只保证基础 read/poll 事件流。
- [ ] 慢消费者溢出只提供内核 dropped 计数，尚无用户态查询 ABI；需要决定是否通过 ioctl 或 `SYN_DROPPED` 报告。
- [ ] 注销后的旧 fd 当前得到通用驱动参数错误，尚未统一映射为 Linux `ENODEV`。
- [ ] GUI 和用户态订阅都采用轮询泵送；中断唤醒/等待队列接入后才能给阻塞 read/poll 提供低延迟、低空转语义。
- [ ] VisionFive 2 与 2K1000LA 的物理 USB HID/input 驱动、IRQ/DMA/cache 一致性及热拔顺序仍待真机验证。

### 提交

- `[feat] expose fanout input event devices`

## 2026-08-10：批次 8——网络设备租约与自动远程调试验收

### 任务与设计

1. 审计首批 TCP monitor 是否具备完整可脚本化协议，而不重复实现 SSH。
2. 将 network registry 迁移到与其它设备一致的稳定 slot、snapshot、generation 和注销模型。
3. 协议栈持有可失效 lease；网卡注销后不得继续通过旧 `Arc` 访问硬件。
4. 提供不依赖 `nc`、第三方包和用户根盘内容的远程 monitor 客户端。
5. 自动启动 QEMU、等待 guest readiness、执行命令并清理进程，覆盖两种 ISA。

网卡 slot 只追加且不复用。注册项包含独立 `AtomicBool present`；注销以 Release 标记 lease 失效，RX、TX token 消费和能力查询均以 Acquire 检查。真机驱动仍必须先屏蔽 IRQ、停止 DMA 并完成 cache 同步，再调用 registry 注销；lease 解决的是上层旧引用，不替代硬件 quiesce。

### 完成内容

- [x] network registry 改为可空稳定 slot，活动计数不再等同于 `Vec::len()`。
- [x] 新增 `network_devices_snapshot()`、按 slot/首设备 lease 获取和 `unregister_network_device()`。
- [x] 网络注册/注销纳入全局 topology generation。
- [x] `NetworkDeviceLease` 在注销时原子失效，已安装 smoltcp adapter 随即停止物理 RX/TX，loopback 路径仍可工作。
- [x] smoltcp 初始化改为只接受受 registry 管理的 lease，不再把裸设备 `Arc` 当作永久在线。
- [x] 新增 `remote_debug_client.py`，校验 banner、prompt、响应上限及 `ping/status/version/quit` 结果。
- [x] 客户端 readiness 同时要求 TCP 建连和有效 WaterOS banner；QEMU hostfwd 早于 guest listener 接受连接时会安全重连。
- [x] 新增 `remote_debug_qemu_smoke.py`，强制 snapshot，失败时输出临时串口尾部，并确保 QEMU terminate/kill 清理。

### 验证证据

- network API host 单测通过：generation 递增、lease 失效、重复注销、活动 snapshot 和 slot 不复用。
- Python 单测共 13 项通过：完整 monitor 会话、错误 banner、多行命令拒绝、hostfwd 提前接受连接后的 readiness 重连，以及 RV/LA launcher 参数。
- 两个 Python 工具均通过 `py_compile`。
- RISC-V `make check` 通过，证明实际 smoltcp/virtio feature 组合可编译。
- LoongArch64 启用 `remote-debug-monitor,operator-shell` 的 release 内核构建通过。
- 自动 RISC-V QEMU smoke 通过：banner、`pong`、tick/CPU/heap status、版本和 `bye` 全部校验成功。
- 自动 LoongArch64 QEMU smoke 通过同一组协议断言，补齐批次 1 留下的 LA runtime 连接缺口。
- 两次 QEMU 都复用批次 5 的 32 MiB 稀疏分区根盘并强制 snapshot，没有写回或新建大镜像。

### 已知限制、未验证与后续测试

- [ ] monitor 仍是无认证、无加密的内核诊断接口，不是 SSH/登录 shell；生产构建继续默认关闭。
- [ ] 用户态认证登录仍依赖 PTY、stdio 转接、用户 socket 包装和可部署 BusyBox/dropbear 等根盘内容。
- [ ] 当前协议栈只在初始化时选择第一块网卡；注销后阻止旧设备 I/O，但不会自动迁移现有 socket 到新网卡。
- [ ] QEMU 未执行 `device_del`：VirtIO 驱动还没有 IRQ/DMA quiesce/remove 回调，直接热删会违反注销 API 的前置条件。
- [ ] TX token 已在消费时再次检查 lease；设备驱动自身的并发 send/receive 与 quiesce 屏障仍必须由具体驱动保证。
- [ ] VisionFive 2 DWMAC/PHY 和 2K1000LA GMAC/PCIe 网卡驱动尚未接入，真机远程调试仍不可用。
- [ ] 真机需验证链路状态、PHY reset/时钟、DMA cache 一致性、中断亲和性及断链恢复。

### 使用方法

```bash
cd os
make kernel-rv EXTRA_FEATURES=remote-debug-monitor,operator-shell
python3 scripts/remote_debug_qemu_smoke.py \
  --arch rv --profile final --kernel ./kernel-rv \
  --sdcard /path/to/wateros-root.img --port 22323
```

### 提交

- `[feat] harden remote debug network lifecycle`

## 2026-08-10：批次 9——VisionFive 2 DTB 拓扑与 PLIC 基础

### 任务与设计

1. 从公共基线创建两个真机平台的独立分支和工作树，本批只修改 VisionFive 2。
2. 用 CodeGraph 审计 machine driver、DTB、UART 与中断路径，确认 VisionFive 2 仍使用 dummy 且无 PLIC 实现。
3. 新增 JH7110 machine profile，以 DTB `/chosen/stdout-path` 选择控制台，不重复写死设备枚举。
4. 解析 PLIC MMIO、`riscv,ndev` 与 `interrupts-extended`，但在 S-mode context 未确认前不触碰寄存器。
5. 用纯算术测试与现场编译的最小 DTS fixture 验证无板阶段可验证部分。

### 完成内容

- [x] 创建 `feat/visionfive2-port`、`feat/loongson2k1000-port` 及独立工作树。
- [x] `visionfive2` 顶层 feature 从 dummy machine driver 切换到独立 JH7110 profile。
- [x] 支持 `starfive,jh7110-uart` + `snps,dw-apb-uart` 的 32 位/4 字节步长 UART 描述和字符设备注册。
- [x] 正确处理常见的 `stdout-path = "serial0:115200n8"` alias 与串口参数后缀。
- [x] 发现 `riscv,plic0`/`sifive,plic-1.0.0` PLIC，保存 MMIO、源数量与上下文中断对。
- [x] 实现标准 PLIC enable、claim/complete 偏移校验和显式 unsafe MMIO claim/complete 边界。
- [x] 初始化日志明确输出 `activation deferred`，防止把 DTB 解析成功误报成真机 IRQ 可用。

### 验证证据

- JH7110 profile host 单测 3 项通过：上下文对解析、畸形长度拒绝、PLIC 偏移/源边界、UART layout 与 compatible。
- `dtc` 将 `visionfive2-minimal.dts` 编译为临时 DTB，`inspect_dtb` 端到端解析 chosen UART、两组 PLIC context、136 个源及 MMIO 地址；临时文件自动删除。
- `cargo check --no-default-features --features visionfive2,heap-tlsf,pre --target riscv64gc-unknown-none-elf` 通过。
- 测试只产生可删除的编译缓存与临时 DTB，没有创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] JH7110 PLIC 的实际 S-mode context 索引、各设备 IRQ 路由、优先级和 claim/complete 行为待官方 DTB 对照及真机验证。
- [ ] 当前 PLIC core 只提供安全构造/偏移计算和显式 unsafe claim/complete，尚未接入 RISC-V 外部 trap 分发，也未主动 enable source。
- [ ] UART DTB 绑定和字符读写已编译并通过 fixture；实际 UART0 时钟、reset、pinmux、FIFO 和中断行为待真机。
- [ ] 尚未从发行版/固件采集多版本 VisionFive 2 DTB 做兼容性 corpus；fixture 只覆盖目标属性形态。
- [ ] 本批没有实现 MMC、DWMAC 或 USB；下一批优先完成 PLIC CPU context 映射与外部中断分发，再推进可中断驱动。

### 提交

- `[feat] add VisionFive 2 DTB machine profile`

## 2026-08-10：批次 10——VisionFive 2 PLIC 上下文与外部中断分发

### 任务与设计

1. 审计 RISC-V `scause=9` 路径、machine driver 契约、CPU/hart 编号和第 9 批 PLIC 描述。
2. 通过 CPU 子节点的 interrupt-controller phandle 反查 hart ID，不假设 context 下标等于 hart ID。
3. 补齐 PLIC priority、enable/disable、threshold、claim/complete 的范围和溢出校验。
4. 在 machine driver 契约增加平台中立的 external-interrupt 入口，由内核 trap 组合层调用。
5. 设备源只有显式注册 handler 后才启用；未知源先屏蔽再 complete，避免中断风暴。

### 完成内容

- [x] DTB 扫描 `/cpus/cpu@N/interrupt-controller` 的 `phandle`/`linux,phandle`，把 PLIC context 映射到明确 hart ID。
- [x] 只把 RISC-V 中断号 9 认作 supervisor external context；缺失映射的 context 保持未解析状态。
- [x] PLIC MMIO 构造验证 context、source、窗口大小与地址算术。
- [x] 新增 source priority、按 context enable/disable、threshold、claim 和受检 complete 操作。
- [x] 新增 IRQ handler 注册表；重复 source 和越界 source 会被拒绝。
- [x] 新增 `MachineDriver::handle_external_interrupt` 默认契约，QEMU/dummy profile 无需伪造 PLIC。
- [x] RISC-V `SupervisorExternal` trap 接入当前 machine driver；控制器不可用时按启动契约错误处理。
- [x] JH7110 仅在 boot hart 找到有效 S-mode context 且 claim/complete 路径就绪后打开 `sie.SEIE`。
- [x] 未注册但由固件遗留为 enabled 的 source 会先在当前 context 屏蔽，再 complete。

### 验证证据

- JH7110 profile host 单测 5 项通过，包括 PLIC context 解析、寄存器偏移、纯内存 volatile MMIO、hart 分发、handler 调用和 complete。
- 最小 DTS fixture 端到端解析通过：M/S context 交错时得到 `phandle 1 → hart 0 → S context 1`、`phandle 2 → hart 1 → S context 3`，并继续验证 chosen DW APB UART。
- VisionFive 2 完整交叉检查通过：`cargo check --no-default-features --features visionfive2,heap-tlsf,pre --target riscv64gc-unknown-none-elf`。
- 公共 QEMU RISC-V `make check` 通过，证明 machine trait 和 trap 改动未破坏既有 profile 编译。
- 平台 memory profile 的恒等 MMIO 窗口覆盖 DTB fixture 的 PLIC 地址；未创建任何磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] PLIC 寄存器布局遵循标准 RISC-V PLIC；JH7110 实际 claim/complete、priority 位宽及固件初始状态仍待真机测试。
- [ ] 当前只在执行 machine driver 初始化的 boot hart 打开 SEIE；AP per-hart 初始化钩子尚未接入，因此多核外部 IRQ 亲和性未完成。
- [ ] IRQ handler 是同步函数指针，尚无共享 IRQ、threaded IRQ、注销/quiesce 和设备生命周期屏障。
- [ ] 尚无真实设备 source 注册；MMC/DWMAC/USB 驱动接入时必须由其 DTB IRQ 属性注册，不能写死 source 编号。
- [ ] 合成内存可证明寄存器地址和软件分发次序，不能证明 PLIC 的硬件 claim 副作用、电气路由或中断丢失行为。

### 提交

- `[feat] add VisionFive 2 external interrupt routing`

## 2026-08-10：批次 11——PLIC 多 hart 初始化与 IRQ 注销生命周期

### 任务与设计

1. 审计 BSP/AP 启动顺序，确认 AP 上线早于全局 machine driver 发现，不能直接访问尚不存在的 PLIC topology。
2. 在 machine driver 契约增加调用 CPU 的局部初始化，并用 driver-ready 屏障协调 AP。
3. IRQ 注册改为 append-only 稳定 slot 和可失效 lease，支持同一 source 注销后重新注册但不复用旧 slot。
4. 注销顺序定义为：设备驱动先 quiesce，PLIC 各 S context 屏蔽 source，lease 失效，等待 in-flight handler 排空。
5. 用多 context 内存 MMIO、并发线程和 4-hart QEMU smoke 验证软件与启动路径。

### 完成内容

- [x] 新增 `MachineDriver::init_current_cpu(cpu_raw)` 默认契约，现有无 per-CPU 设备的 profile 保持兼容。
- [x] RISC-V AP 在页表、timer/IPI 和 online 标记完成后等待 driver-ready；不会阻塞 BSP 的 online 等待。
- [x] BSP 完成全局设备发现后初始化自己的 machine-local 状态，再 Release driver-ready；AP Acquire 后初始化各自 PLIC context。
- [x] 每个已解析 S-mode context 独立清 threshold 并在当前 hart 打开 `sie.SEIE`。
- [x] IRQ 注册返回 `IrqLease`，包含稳定 slot、source 和 presence；活动 source 不允许重复注册。
- [x] 新增 `unregister_irq_handler`，重复注销幂等，旧 lease 永久失效，source 重注册获得更大的新 slot。
- [x] handler 分发使用 in-flight 引用计数；注销会等待已复制 handler 返回，不在执行 handler 时持有注册表锁。
- [x] PLIC source enable/disable 遍历原始 context 表，只操作已解析的 supervisor context，不误用交错的 M-mode context。
- [x] QEMU remote-debug smoke 新增 `--smp 1..8` 参数，可重复验收多核启动而不写回根盘。

### 验证证据

- JH7110 profile host 单测 7 项通过，并以 4 个并发 test thread 运行。
- 纯内存 PLIC 测试验证 context 1/3 的 threshold，以及 source 33 在两个 S context 的 enable/disable 位；M context 未被写入。
- 生命周期测试验证注销幂等、slot 不复用、旧 lease 失效，以及并发注销确实等待正在执行的 handler 退出。
- 合成 DTB fixture 继续通过 M/S context 交错、hart phandle 映射和 chosen UART 解析。
- VisionFive 2 RISC-V64 完整交叉检查通过；公共 QEMU RISC-V `make check` 通过。
- 4-hart QEMU runtime smoke 通过，远程 `status` 报告 `online_cpus=0xf`，并完成 ping/status/version/quit。
- QEMU 复用 32 MiB 稀疏根盘并使用 snapshot，没有写回镜像。

### 已知限制、未验证与后续测试

- [ ] JH7110 真机各 hart 的 OpenSBI HSM 可用性、PLIC context 可写性、SEIE 行为和中断亲和性仍待真机测试。
- [ ] 注销调用者必须先屏蔽设备侧 IRQ、停止 DMA并完成 cache/order 同步；PLIC lease 无法替代设备 quiesce。
- [ ] 禁止 handler 同步注销自身，否则会等待自己的 in-flight 引用；后续 threaded IRQ 可提供延迟注销路径。
- [ ] 仍未支持共享 IRQ、IRQ affinity 配置、优先级策略和 storm 计数/自动隔离阈值。
- [ ] QEMU 验证的是通用 SMP driver-ready 屏障，不是 JH7110 PLIC 硬件；后者只有 DTB和内存 MMIO 证据。
- [ ] 下一批开始解析 VisionFive 2 MMC/SDIO DTB，优先评估可复用的 DesignWare MMC/SDHCI 实现及许可证。

### 提交

- `[feat] add per-hart PLIC and IRQ leases`

## 2026-08-10：批次 12——VisionFive 2 MMC 拓扑与 PIO 控制器核心

### 任务与设计

1. 以 Linux 主线 DT binding 和 JH7110/VisionFive 2 DTS 为权威来源，确认两路 MMC 的地址、IRQ、总线宽度和板级角色。
2. 审计 DesignWare MMC 与 StarFive 扩展实现的许可证，只复用硬件接口事实，不复制或翻译 GPL 驱动代码。
3. 将 MMC host 加入 DTB 拓扑；禁用节点不得注册，非法总线宽度必须拒绝。
4. 实现可注入寄存器后端、版本化 FIFO 地址、控制器复位和有界 CMD17 PIO 单块读取核心。
5. 用合成寄存器和现场编译 DTB 验证无板可验证部分，明确隔离时钟、复位、调谐和卡枚举等真机工作。

### 完成内容

- [x] 解析 `starfive,jh7110-mmc` 节点的 MMIO、IRQ、`bus-width`、`max-frequency`、`fifo-depth` 与 `non-removable`。
- [x] DTB fixture 描述 mmc0/eMMC（`0x16010000`、IRQ 74、8-bit）和 mmc1/SD（`0x16020000`、IRQ 75、4-bit）。
- [x] 新增有边界检查的 volatile MMIO 后端和可替换 `RegisterIo`，测试不访问宿主机物理地址。
- [x] 根据控制器版本选择旧版 `0x100` 或 2.40a 及以后 `0x200` FIFO 窗口。
- [x] 实现有界软复位、CMD17 编码、响应/数据 CRC 与超时分类、FIFO 计数驱动的 512-byte PIO 读取。
- [x] machine driver 只报告 host 拓扑，并明确打印 `clock/reset/card bring-up status=UNVERIFIED`，尚不注册块设备。
- [x] 新增独立来源与许可证审计，记录 GPL-only StarFive 扩展只用于识别待办边界。

### 验证证据

- JH7110 profile host 单测 10 项通过，其中 3 项直接覆盖 MMC：成功块读取、复位、版本化 FIFO、错误块长、CRC 错误和有界超时。
- 合成 FIFO 使用 128 个不同的 32-bit word，断言首尾字节、CMD17 编号、参数及块长度，避免只验证全零缓冲区。
- DTS fixture 经 `dtc` 现场编译并端到端解析两路 host；断言地址、IRQ、总线宽度和 eMMC `non-removable`。
- VisionFive 2 RISC-V64 交叉检查与公共 RISC-V `make check` 作为本批提交前验收项。

### 已知限制、未验证与后续测试

- [ ] **没有真机，因此 SD/eMMC 可读写状态仍为 UNVERIFIED；宿主机 mock 只能证明软件状态机和边界。**
- [ ] 尚未实现 JH7110 clock/reset/syscon/pinmux、电源 regulator、card-detect、卡初始化和容量识别，当前不会向 block registry 暴露设备。
- [ ] 尚未实现 CMD0/CMD8/ACMD41/CMD2/CMD3/CMD7、SDHC 块寻址、写入、多块传输和停止命令。
- [ ] StarFive sample-phase tuning、DDR 50/52 MHz 时钟倍频、1.8V 切换及 HS200 只能结合硬件和示波/日志验证。
- [ ] 当前只提供 polling PIO 核心；DMA、cache coherency、IRQ 模式和注销 quiesce 必须在后续独立批次实现。
- [ ] 真机首测应从 mmc1 可移除 SD 的低速 1-bit/400 kHz 初始化开始，保持 eMMC 根盘只读，确认容量与随机块校验后再允许写入。

### 提交

- `[feat] add VisionFive 2 MMC PIO foundation`

## 2026-08-10：批次 13——SD 初始化协议与只读块设备适配

### 任务与设计

1. 审计 block API、设备注册与分区扫描契约，保持现有 512-byte LBA 接口不变。
2. 将 SD native-mode 协议与 DesignWare MMIO 分层，以可注入 transport 驱动同一初始化状态机。
3. 实现 CMD0、CMD8、CMD55/ACMD41、CMD2、CMD3、CMD7 和 SDSC 的 CMD16 流程。
4. 根据 CMD8 与 OCR CCS 区分 SDSC 字节寻址和 SDHC/SDXC 块寻址，所有轮询与算术必须有界。
5. 提供只读 block adapter；在板级供电/时钟未验证前不自动注册设备。

### 完成内容

- [x] 新增 `SdTransport`、响应类型和脚本化 transport，可在无 MMIO 环境验证完整命令序列。
- [x] CMD0 自动设置 DesignWare `SEND_INITIALIZATION`，非数据命令支持短响应、无 CRC OCR 响应和 136-bit 长响应。
- [x] CMD8 正确回显 `0x1aa` 时请求 HCS；CMD8 response timeout 作为旧版卡兼容路径，其它错误不被吞掉。
- [x] ACMD41 每次轮询前重新发送 CMD55，验证 APP_CMD、power-up、目标电压范围和 CCS，尝试次数由调用方限制。
- [x] 获取并校验非零 RCA、选择卡；SDSC 设置 512-byte block length，SDHC/SDXC 保持块寻址。
- [x] `SdCard<T>` 实现现有 `BlockDevice`：连续读拆为单块 CMD17，拒绝非整块缓冲、寻址溢出和全部写请求。
- [x] DesignWare host 直接实现 `SdTransport`，协议测试通过后可复用到真实 MMIO 后端，不是 mock-only API。
- [x] 来源审计补充 SD Association 官方简化规范；仅实现协议事实，没有复制规范文字或第三方驱动代码。

### 验证证据

- JH7110 profile host 单测增至 15 项；新增测试覆盖 DesignWare CMD0 初始化位、长响应 flags 和四类 SD 场景。
- 脚本化 SDHC 卡严格断言命令、参数和 response kind，并读取连续两个非零模式块，确认使用 LBA 7/8。
- 脚本化 SDSC 卡以 CMD8 timeout 进入旧卡路径，断言 LBA 3 转换为 byte address 1536，并拒绝 32-bit 地址溢出。
- 异常测试覆盖 ACMD41 有界超时、错误 CMD8 回显、非整块缓冲、SDHC 地址溢出和写入拒绝。
- DTB fixture、VisionFive 2 RISC-V64 交叉检查与公共 RISC-V `make check` 作为提交前验收项。

### 已知限制、未验证与后续测试

- [ ] **状态机和 DesignWare 编码已测试，但真实 SD 卡初始化仍为 UNVERIFIED；尚无物理 VF2 证明 400 kHz 时钟或线路工作。**
- [ ] 尚未解析 CMD9/CSD，`total_blocks` 保持 `None`；注册为整盘前必须补容量解析和越界测试。
- [ ] 尚未读取 SCR、切换 4-bit bus、查询写保护或处理 card-detect；首轮真机仍应使用 1-bit 低速只读模式。
- [ ] 当前不自动调用 `register_block_device`，因此不会被根文件系统或 MBR 扫描误选为可用磁盘。
- [ ] DesignWare clock divider/update-clock 命令、JH7110 reset/syscon/pinmux/regulator 尚未实现，协议层调用前置条件尚不成立。
- [ ] eMMC 使用不同于 SD 的 CMD1 初始化流程，本批只覆盖 mmc1 可移除 SD，不得用于 mmc0 eMMC。
- [ ] 后续先实现并纯函数测试 CSD v1/v2 容量解析，再建立显式、失败可回滚的只读注册入口。

### 提交

- `[feat] add SD card initialization state machine`

## 2026-08-10：批次 14——SD CSD 容量与整盘边界

### 任务与设计

1. 统一协议层的 136-bit 响应为 MSW-first 128-bit payload，在 DesignWare transport 边界转换 RESP0..3。
2. 在获得 RCA 后、选卡前发送 CMD9，解析 CSD v1/v2 容量。
3. 用 checked arithmetic 计算 512-byte 总块数，拒绝保留 CSD structure、非法 block length 和溢出。
4. 在发出任何 CMD17 前验证整个连续请求没有越过盘尾，避免部分读取。
5. 提供显式共享块设备句柄构造，但保持 registry 插入由后续真机激活流程控制。

### 完成内容

- [x] `CommandResponse::Long` 明确定义为 MSW-first；DesignWare 的 RESP3..RESP0 在 transport 边界规范化。
- [x] SD 初始化新增 CMD9，参数使用 RCA，CSD 必须解析成功后才返回 `SdCard`。
- [x] CSD v2 以 22-bit `C_SIZE` 计算 `(C_SIZE + 1) * 1024` 个逻辑块，覆盖最大 2 TiB/2^32 blocks。
- [x] CSD v1 组合 `READ_BL_LEN`、`C_SIZE`、`C_SIZE_MULT` 并换算 512-byte blocks，只接受规范允许的 9..11 block-length exponent。
- [x] `SdCardInfo.total_blocks` 从 `None` 改为可信 `Some`，公共 `BlockDevice::total_blocks()` 可供 MBR 分区边界校验使用。
- [x] 多块读取先验证 `[start, end)` 完整范围；跨盘尾请求在 transport I/O 前失败。
- [x] 新增 `into_shared()` 显式构造公共块设备句柄，不自动注册、不触发 MBR 扫描。

### 验证证据

- JH7110 profile host 单测增至 17 项。
- 合成 CSD v2 最大字段解析为 `2^32` blocks；合成 CSD v1 解析为 32768 blocks。
- 保留 CSD structure 和非法 256-byte read block length 均被拒绝。
- 两块请求从 LBA 1023 跨越 1024-block 盘尾时返回 `InvalidParam`，并断言 transport 没有收到任何读命令。
- 既有 SDHC/SDSC 严格命令脚本加入 CMD9/RCA 参数断言，连续读和地址换算测试继续通过。

### 已知限制、未验证与后续测试

- [ ] **DesignWare RESP3..RESP0 到 CSD MSW/LSW 的转换依据控制器接口定义和上游行为，仍须用真机已知卡的原始 CSD/容量交叉确认。**
- [ ] CSD 可证明容量算法，不证明卡实际返回稳定响应；真实 CMD9 CRC、timeout 和线路行为仍为 UNVERIFIED。
- [ ] 尚未解析 erase geometry、write-protect、transfer speed 等 CSD 字段；当前只读路径不依赖这些字段。
- [ ] 尚未自动注册设备；必须先完成 JH7110 时钟、reset、syscon、pinmux 和低速初始化前置条件。
- [ ] 首次显式注册前应至少重复读取 MBR/LBA0 和随机末尾块，并与离线镜像哈希对照，失败时不得进入 registry。

### 提交

- `[feat] validate SD card capacity from CSD`

## 2026-08-10：批次 15——DesignWare MMC identification 时钟

### 任务与设计

1. 实现 DesignWare 8-bit divider 的受检计算，向上取整以保证实际卡时钟不高于目标。
2. 按控制器协议执行 disable/update、divider/update、enable/update 三阶段时钟切换。
3. update-clock 使用 `CMD.START` 自清除而非普通 `CMD_DONE`，并设置独立有界轮询。
4. 新增保守 polling/PIO 初始化：复位、供电、timeout、1-bit bus、IRQ mask 和 FIFO watermark。
5. 保持 JH7110 上游 AHB/CIU clock、reset、syscon 和 pinmux 为板级前置条件。

### 完成内容

- [x] `clock_divider()` 支持 bypass 和 1..255 divider，拒绝零频率及无法达到的过低目标。
- [x] 50 MHz 输入、400 kHz 目标得到 divider 63、实际 396825 Hz，不超过 identification 上限。
- [x] `update_clock()` 清 pending status，发出 UPDATE_CLOCK command，并有界等待 START 清零。
- [x] `configure_card_clock()` 只有前三阶段全部成功才返回实际频率；第一阶段失败时不会写 divider 或 enable。
- [x] `initialize_polling()` 保持 DMA 和控制器 IRQ mask 关闭，设置 1-bit CTYPE、全 timeout 和按 FIFO depth 计算的 RX/TX watermark。
- [x] 板级依赖在 API 文档中明确标为 UNVERIFIED，machine init 不调用该入口。

### 验证证据

- JH7110 profile host 单测增至 19 项。
- 寄存器模型断言三次 update-clock、最终 divider 63、CLKENA 1、CTYPE 0、INTMASK 0，以及 32-word FIFO 的 RX/TX watermark 15/16。
- divider 测试覆盖 bypass、400 kHz 向下逼近、零目标和超出 8-bit 能力的目标。
- hardware-lock 注入在第一次 update 时返回专用错误，并断言 CLKDIV/CLKENA 均未推进。
- START 永不自清除的模型在固定 poll limit 后超时，并保持卡时钟禁用。

### 已知限制、未验证与后续测试

- [ ] **控制器内部 divider 序列已用模型验证，但 JH7110 CIU 输入是否确为 50 MHz、update-clock 副作用仍须真机确认。**
- [ ] `PWREN=1` 只是 DesignWare host 端位；VF2 SD slot regulator、GPIO card-detect 和电气供电仍未控制。
- [ ] JH7110 clock/reset provider 与 syscon sample phase 尚未实现；调用 `initialize_polling()` 前置条件目前无法在 machine driver 中满足。
- [ ] FIFO watermark 只服务 polling PIO；实际 RXDR 触发阈值和 FIFO depth 应用 HCON/DTB 与真机日志交叉确认。
- [ ] 下一批应从 DTB 解析 clock/reset phandle specifier，先建立只描述、不写寄存器的资源拓扑与可测试解析器。

### 提交

- `[feat] add DesignWare MMC identification clock setup`

## 2026-08-10：批次 16——MMC clock/reset/syscon DTB 资源拓扑

### 任务与设计

1. 依据 JH7110 binding/DTS 解析 `clocks`、`clock-names`、`resets` 和 `starfive,sysreg`。
2. phandle specifier 必须读取 provider 的 `#clock-cells`/`#reset-cells`，不得写死每项两个 cell。
3. 对 MMC binding 要求唯一 `biu`、`ciu` 名称和单个 reset；资源只描述、不执行硬件副作用。
4. sysreg 保存 provider、offset、shift、mask，并校验对齐、位宽和 mask/shift 一致性。
5. 用有效、禁用和必须拒绝的畸形 DTB fixture 做端到端测试。

### 完成内容

- [x] 新增可变参数 `ResourceSpecifier { provider, args }`，支持 provider 声明 0..8 个参数 cell。
- [x] specifier parser 逐项解析 phandle、查询 provider cell count，并拒绝未知 provider、截断参数和非 cell 对齐属性。
- [x] string-list parser 要求 NUL 终止、合法 UTF-8、非空名称；命名资源要求名称数与 specifier 数一致且目标名唯一。
- [x] `MmcHostDescription` 新增 BIU clock、CIU clock、reset 和可选 sysreg 位域。
- [x] fixture 增加合成 syscrg/syscon provider；mmc0 解析 ID 91/92/reset 90 和 `<0x14,26,0x7c000000>`。
- [x] mmc1 解析 ID 93/94/reset 95 和 `<0x9c,1,0x3e>`。
- [x] `status = "disabled"` 且资源引用故意损坏的第三节点被忽略，不污染可用 topology。
- [x] fixture runner 动态生成重复 clock-name、未知 provider、截断 specifier 和非法 sysreg mask 四类畸形 DTB，discover 必须返回 `InvalidDtb`。

### 验证证据

- JH7110 profile 19 项 host 单测继续通过。
- 有效 DTS 经 `dtc` 编译后，example 端到端断言两路 host 的 provider phandle、clock/reset ID 和 sysreg 位域。
- 畸形 DTS corpus 覆盖 `biu,biu`、不存在的 `0xffffffff` phandle、缺少 provider argument 和 shift 以下含位的 mask；同一 discover 路径全部明确拒绝，临时文件由 trap 清理。
- 禁用节点没有 `interrupts`、clock provider 也无效，但解析仍成功且 host 数保持 2，证明 status 检查早于资源解引用。
- VisionFive 2 RISC-V64 交叉检查与公共 RISC-V `make check` 作为提交前验收项。

### 已知限制、未验证与后续测试

- [ ] **资源引用解析已验证，但 syscrg/syscon provider 尚无 WaterOS 驱动，本批不会 enable clock、deassert reset 或写 sysreg。**
- [ ] fixture 的 clock/reset ID 取自目标属性形态用于解析测试；实际 ID 数值和固件 DTB 版本仍须与真机导出的 DTB 对照。
- [ ] `starfive,sysreg` 的 sample phase 位域只保存不解释；高速调谐仍为 UNVERIFIED。
- [ ] 目前 generic specifier 上限为 8 args，足够目标 provider；未来遇到更宽 binding 时应显式提高并增加内存边界测试。
- [ ] 下一批应实现独立 JH7110 syscrg 描述与受检 MMIO backend，先覆盖 clock/reset 寄存器算术，不在 machine init 自动激活。

### 提交

- `[feat] discover VisionFive 2 MMC resources`

## 2026-08-10：批次 17——2K1000LA UEFI 启动契约与 FDT 发现

### 任务与设计

1. 依据龙芯《CPU 统一系统架构规范（LA 架构嵌入式系列）》纠正入口参数语义，不沿用未经证实的 `argc/argv/envp` 假设。
2. LoongArch 内核入口将三个原始固件参数显式传给 machine profile；QEMU profile 保持固定 DTB 地址语义。
3. 2K1000LA 只接受 `a0 == 1` 的 UEFI-compatible 启动，并从 `a2` 指向的 EFI System Table Configuration Table 查找 `DEVICE_TREE_GUID`。
4. 对 system-table 对齐、签名、configuration-table 空指针和条目数设置受检边界；失败返回现有 `0` DTB sentinel。
5. 先建立可信 FDT 入口，再推进内存、LIOINTC、MMC/PCIe 等 DTB-first 驱动；不把 QEMU machine 常量复制到实板 profile。

### 完成内容

- [x] `_start.S` 和 `Loongson2K1000LABootArgs` 改为 UEFI flag、command-line PA、system-table PA 的官方语义。
- [x] `wateros_kernel_main` 不再丢弃固件参数，machine-specific `device_tree_phys_addr(a0, a1, a2)` 在平台初始化前执行。
- [x] 新增 64-bit EFI table header、system table、configuration table 和 GUID 的 `repr(C)` 布局。
- [x] 使用官方 `DEVICE_TREE_GUID {b1b621d5-f19c-41a5-830b-d9152c69aae0}` 发现 FDT，允许其位于非首个 configuration-table entry。
- [x] 拒绝非 UEFI-compatible 标志、空或非 64 KiB 对齐 system table、错误签名、超过 4096 项、非零条目数配空指针以及缺失/空 FDT entry。
- [x] unsafe 边界集中在固件表读取函数，并记录 identity-mapped/readable firmware memory 的调用者责任。
- [x] QEMU LoongArch profile 仅适配统一函数签名，仍返回其 machine 固定的 `0x0010_0000`，行为不变。

### 验证证据

- 2K1000LA platform host 单测 3 项通过，其中 EFI 查找覆盖非首位目标项、空目标地址和缺失目标项。
- QEMU LoongArch platform crate `cargo check` 通过，只有既有 `asm_sub_register` warning。
- `make kernel-la`（QEMU LoongArch final profile）整核构建通过，证明接口改动未破坏既有 LA profile。
- `cargo build --release --target loongarch64-unknown-none --no-default-features --features loongson2k1000la,pre,heap-tlsf` 通过，证明 2K1000LA 独立 profile 可交叉编译和链接。
- 全仓 `cargo fmt --all --check` 因仓库及 `vendor/` 既有格式差异失败；受影响 platform crate 已定向格式化，没有批量修改第三方代码。

### 已知限制、未验证与后续测试

- [ ] **EFI 表解析依据官方 ABI 和 UEFI 布局完成，但尚未用目标板固件提供的真实 System Table/FDT 上板验证。**
- [ ] 当前失败仍沿用 `dtb_pa == 0` sentinel，早期控制台尚未输出细分发现错误；真机首启前应加入不依赖 DTB 的诊断码。
- [ ] `a1` command line 仅保留未解析；当前启动流程没有消费内核命令行。
- [ ] 代码假定固件表在早期 identity-map 下可读；切换页表前有效，后续若移动发现时点必须重新审计映射生命周期。
- [ ] 当前 2K1000LA 内存仍使用保守 fallback，driver 仍为 dummy；下一批应使用已发现 FDT 解析官方 DTS 的多段内存，并建立实板专属 DTB 驱动 profile。
- [ ] UART `0x1fe20000`、125 MHz、LIOINTC IRQ 0，MMC `0x1fe2c000`/IRQ 31 以及 PCIe ranges 虽有官方/上游 DTS 依据，均尚未在本批激活，全部保持 UNVERIFIED。

### 参考依据

- 龙芯《CPU 统一系统架构规范（LA 架构嵌入式系列）V1.0》：启动参数、UEFI Configuration Table/FDT GUID 与附录 2K1000LA DTS。
- Linux/LoongArch `loongson-2k1000.dtsi`：UART、LIOINTC、MMC 和 PCIe 资源形态，仅作为后续驱动交叉依据。

### 提交

- `[fix] discover 2K1000LA FDT from UEFI tables`

## 2026-08-10：批次 18——2K1000LA FDT 多段内存发现

### 任务与设计

1. 从批次 17 已发现的固件 FDT 读取 `memory` 节点，不再无条件假设 1 GiB RAM。
2. 官方 2K1000LA DTS 有三个不连续 `reg` 区段；现有 `KernelMemoryLayout` 只能表达单段主 RAM，因此只选择包含内核链接地址 `0x90000000` 的区段。
3. 对地址加法、空区间和页边界做受检处理，不允许通过饱和加法把畸形区段伪装成有效 RAM。
4. 缺少 DTB、FDT 无效或没有包含内核的 memory extent 时，保留 `0x90000000..0xc0000000` 降级窗口。
5. 使用真实 DTS cell 形态经 `dtc` 编译后做端到端 host 验证，并保留纯函数边界测试。

### 完成内容

- [x] 2K1000LA platform 引入既有 `fdt 0.1.5` 解析库，新增 `primary_ram_from_fdt()`。
- [x] 扫描 `memory`/`memory@...` 节点的所有 `reg` entries，并由 FDT 父节点的 address/size cells 解析 64-bit 地址与长度。
- [x] 区段起点向上、终点向下做 4 KiB 对齐；零长度、加法溢出和不包含内核链接地址的区段被忽略。
- [x] 多个候选区段同时包含内核时选择容量较大者，结果仍交由公共 `KernelMemoryLayout::validate()` 校验。
- [x] 官方三段布局中只选择 `0x90000000..0x270000000`，不会把两个低内存段或中间地址空洞交给 frame allocator。
- [x] `physical_ram_end_exclusive()` 和 `kernel_memory_layout()` 统一消费同一个主 RAM 发现结果。
- [x] 新增两个 DTS fixtures、host verifier 和临时 DTB 测试脚本，临时文件在退出时删除。

### 验证证据

- 2K1000LA platform host 单测由 3 项增至 6 项，全部通过。
- 纯单测覆盖 fallback layout、跨页对齐、低内存排除、零长度、`usize` 地址溢出以及多候选择大。
- `official-memory-layout.dts` 经 `dtc` 编译，端到端解析得到 `0x90000000..0x270000000`。
- `no-kernel-memory.dts` 经相同路径解析为 `None`，证明低地址 extent 不会被误选。
- `cargo build --release --target loongarch64-unknown-none --no-default-features --features loongson2k1000la,pre,heap-tlsf` 通过。
- `git diff --check` 通过；测试只生成临时小型 DTB，没有创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] **FDT cell 解析和选择策略已验证，但真实板报告的实际容量、FDT 所在物理地址和固件保留区仍须上板核对。**
- [ ] 公共内存 API 仍只支持一个连续 extent；官方两个低内存区段暂未使用，这是避免跨空洞分配的有意限制。
- [ ] 尚未从 `/reserved-memory` 和 FDT memory reservation block 扣除保留页；在允许整个高内存 extent 进入 frame allocator 前必须解决 DTB/固件表生命周期和保留区问题。
- [ ] 当前发现错误静默降级到 768 MiB fallback；早期诊断日志仍待加入。
- [ ] MMIO 表仍是保守静态范围，没有根据 PCIe ranges 等 DTB 属性细化。
- [ ] 下一批应建立 2K1000LA 专属 driver aggregate，先实现只读 DTB topology，解析 UART、LIOINTC 与 MMC 资源但暂不执行未经上板验证的寄存器写入。

### 提交

- `[feat] discover 2K1000LA RAM from FDT`

## 2026-08-10：批次 19——2K1000LA 独立驱动拓扑

### 任务与设计

1. 为 2K1000LA 新建独立 machine driver crate，顶层 profile 不再选择 `driver/impl-dummy`。
2. 只从 FDT 发现并验证 UART、LIOINTC 和 MMC 资源，本批不执行未经真机验证的寄存器写入。
3. 中断描述保留 parent phandle 和最多 4 个原始 specifier cells，避免被公共单 IRQ 摘要模型截断。
4. MMIO `reg` 必须完整编码、非空、非零长度且地址加法不溢出；设备专属资源数量严格校验。
5. host parser 不依赖目标架构汇编；platform 依赖只在 `target_arch = "loongarch64"` 时启用。

### 完成内容

- [x] 新增 `wateros-driver-impl-loongson2k1000la` 并接入 driver workspace、aggregate feature 和根 `loongson2k1000la` feature。
- [x] `MachineDriver::init_after_boot()` 读取 platform 保存的 FDT、构造 topology 快照并记录发现数量。
- [x] 新增 `BoardTopology`、`UartDescription`、`InterruptControllerDescription`、`MmcDescription` 和 `InterruptSpec`。
- [x] 精确匹配 `ns16550a`、`loongson,liointc-2.0`、`loongson,ls2k1000-mmc` compatible。
- [x] UART 要求单个 MMIO、interrupt parent/specifier 和 `clock-frequency`，可选 `reg-shift`。
- [x] LIOINTC 要求单个 MMIO、phandle 和 1..4 个 interrupt cells；MMC 支持一到两个 MMIO regions 并要求中断描述。
- [x] `status = disabled/reserved/fail/failed` 节点在资源解引用前跳过；未知或非 NUL 终止 status 被拒绝。
- [x] topology 保存进互斥快照供后续激活层读取，但明确输出 `UNVERIFIED_ON_HARDWARE`，没有注册虚假可用设备。
- [x] host 构建时不链接 LoongArch platform，避免测试环境误汇编目标指令。

### 验证证据

- 有效 DTS fixture 经 `dtc` 编译后解析出 1 个 LIOINTC、1 个 UART 和 1 个 MMC。
- 断言 LIOINTC `0x1fe01400`/2 cells、UART `0x1fe20000`/125 MHz/IRQ `<0 4>`、MMC `0x1fe2c000` + `0x1fe00438`/IRQ `<31 4>`。
- fixture 中禁用 UART 带零长度 MMIO 且无中断，仍被先行忽略，最终 UART 数保持 1。
- 缺少 UART `clock-frequency` 的独立 fixture 经相同解析路径返回错误。
- 新 driver crate host test/doc-test 构建通过；fixture runner 两种模式均通过。
- 2K1000LA release 交叉构建通过，构建日志确认编译并链接 `wateros-driver-impl-loongson2k1000la`。
- `git diff --check` 通过；测试只生成两个临时小型 DTB。

### 已知限制、未验证与后续测试

- [ ] **所有资源数值来自规范/上游 DTS 与合成 fixture；真实固件 compatible 列表、phandle 和 interrupt cells 仍须用上板 DTB 对照。**
- [ ] LIOINTC 仅描述未初始化，CPU HWI route、enable/mask、ack 与多核 affinity 全部 UNVERIFIED。
- [ ] UART topology 尚未替换 early console 的固定基址，也没有注册运行期字符设备；UART IRQ 收发未激活。
- [ ] MMC topology 尚未解析 clocks、resets、DMA、bus-width、card-detect 和供电资源，也未绑定块设备驱动。
- [ ] parser 当前要求设备节点显式提供 `interrupt-parent`，尚未实现 Devicetree 规范的祖先继承规则。
- [ ] 尚未发现 PCIe、GMAC、USB、SATA 和 GPIO；后续优先级为 LIOINTC 受检寄存器模型、MMC/clock 资源，再到 PCIe/USB。
- [ ] 下一批应实现 LIOINTC 2.0 的纯寄存器模型与 MMIO backend 分离，先验证 route/mask/ack 算术，不在 machine init 自动 enable。

### 提交

- `[feat] add 2K1000LA driver topology`

## 2026-08-10：批次 20——LIOINTC 2.0 受检寄存器模型

### 任务与设计

1. 依据 Linux 主线 irqchip 和 Devicetree binding 核对 32-source LIOINTC bank 的 route、enable、disable、polarity、edge 和 per-core ISR 布局。
2. 将寄存器算法与访问方式分离为 `RegisterIo`，host model 记录全部读写，目标侧 volatile backend 保持未激活。
3. route byte 使用低 4 位 core mask、高 4 位 parent HWI mask；每个 bank 只接受 IRQ 0..31。
4. `ENABLE`/`DISABLE` 使用专用写寄存器，不对 set/clear 寄存器做 read-modify-write；trigger 的 POL/EDGE 按文档读改写。
5. 修正批次 19 对 LIOINTC 2.0 单 MMIO 的错误假设，按 `reg-names = main,isr0,isr1...` 保存独立 ISR regions。

### 完成内容

- [x] 新增 `liointc` 模块、`LioIntc<I>`、`RegisterIo`、`Route` 和四种 `Trigger`。
- [x] 实现单 IRQ enable、mask/ack、全禁用、route 配置、trigger 配置、per-core pending 和最低位 claim。
- [x] `mask_ack` 明确区分：写 DISABLE 可清锁存 pulse，但 level source 仍必须由源设备清除。
- [x] claim 取 per-core ISR 与 enable-status 的交集，不返回被 mask 的 pending source。
- [x] 构造器限制最多 4 核 ISR，并检查 main/ISR 地址加法溢出；缺失 core 返回 `InvalidParam`。
- [x] 新增 `VolatileMmio`，unsafe 只包围单次 volatile read/write；代码注释标为 `UNVERIFIED_ON_HARDWARE`，没有接入 machine init。
- [x] topology 的 LIOINTC 描述改为 `main_mmio + core_isr[]`，严格校验连续命名 `isr0..isr3`。
- [x] 未被其它节点引用的控制器允许没有 phandle；被设备引用的 interrupt parent 仍必须有有效 phandle。
- [x] fixture 更新为两组官方形态控制器：main `0x1fe01400/1440`，ISR `0x1fe01040/48` 与 `0x1fe01140/48`。

### 验证证据

- LIOINTC host 单测 5 项全部通过。
- route 测试验证 core mask `0b0101` + parent line 2 编码为 `0x45`，并拒绝空 core mask 与 parent line 4。
- 寄存器 trace 验证 IRQ31 route 写 `base+31`，enable 写 `base+0x28`，mask/ack 与 disable-all 写 `base+0x2c`。
- 四种 trigger 组合验证 EDGE/POLARITY 最终位图；pending 测试验证两个 core 的独立 ISR 和最低 enabled bit claim。
- IRQ32 测试最初发现 `then_some` 参数提前求值导致移位 panic；改为范围检查后再移位，现断言错误路径无任何写入。
- 更新后的双 LIOINTC DTS fixture 经 `dtc` 和同一 topology parser 验证通过；缺失 UART clock fixture继续被拒绝。
- 2K1000LA release 交叉构建通过，`git diff --check` 通过。

### 已知限制、未验证与后续测试

- [ ] **volatile MMIO、真实 pending 变化、CPU HWI cascade 和 pulse clear 副作用均未上板验证；machine init 不会构造或启用控制器。**
- [ ] route 模型支持 4 核，但 2K1000LA 实际双核映射和 boot CPU ID 必须从 CPU topology 取得，不能固定为 core0。
- [ ] 尚未解析 `loongson,parent_int_map`、`interrupts` 和 `interrupt-names` 来自动生成每个 source 的 parent route。
- [ ] claim 只返回 bank-local 0..31；第二 LIOINTC 到全局 IRQ 32..63 的 domain 映射尚未接入公共 interrupt API。
- [ ] 没有通用 IRQ handler registry，UART/MMC 也尚未注册 handler，因此现在激活控制器仍没有安全 dispatch 终点。
- [ ] LIOINTC 寄存器并发读改写需要 interrupt-safe lock；当前模型由调用者独占，接入多核前必须加入锁语义。
- [ ] 下一批应扩展 topology 的 parent map/parent IRQ 名称解析，并实现两个 bank 的纯 domain 映射与 dispatch 表，不直接开启硬件。

### 参考依据

- Linux 主线 `drivers/irqchip/irq-loongson-liointc.c`（GPL-2.0）：寄存器偏移、route 编码、mask/ack 和 per-core ISR 行为，仅作为语义参考，没有复制实现代码。
- Linux Devicetree binding `loongson,liointc.yaml`（GPL-2.0-only OR BSD-2-Clause）：LIOINTC 2.0 命名寄存器、2-cell interrupt 和 parent map 契约。
- Linux `loongson-2k1000.dtsi`（GPL-2.0）：两组 LIOINTC 的 main/isr0/isr1 实际地址。

### 提交

- `[feat] model 2K1000LA LIOINTC registers`

## 2026-08-10：批次 21——LIOINTC parent route 与双 bank IRQ domain

### 任务与设计

1. 解析 LIOINTC 自身连接 CPUINTC 的多项 `interrupts`、`interrupt-names` 和四项 `loongson,parent_int_map`。
2. parent name 只接受 `int0..int3`，specifier 数量必须与名称一致，非零 source map 必须有对应 parent interrupt。
3. 四个 source maps 必须互不重叠且合计覆盖完整 32-source bank，拒绝一个 source 同时路由到多个 parent line 的歧义。
4. 建立固定两 bank、每 bank 32 项的全局 IRQ domain，映射公式为 `bank * 32 + local`。
5. handler 表固定 64 项，dispatch 路径不分配内存、不持锁、不隐式执行硬件 ack。

### 完成内容

- [x] topology parser 将单中断解析推广为按 parent `#interrupt-cells` 切分的多 specifier parser。
- [x] `InterruptControllerDescription` 新增四槽 `parent_interrupts` 和 `parent_source_maps`。
- [x] 严格校验 parent name、重复名称、specifier/name 数量、map 长度、重叠 map、未连接 parent 和未覆盖 source。
- [x] 官方形态 fixture 中 LIOINTC0 保存 CPU HWI2/`int0`/`0xffffffff`，LIOINTC1 保存 CPU HWI3/`int1`/`0xffffffff`。
- [x] 新增 `GlobalIrq`、`LioIntcDomain`、`DomainError`、固定 handler table 和 `DispatchReport`。
- [x] 实现注册、拒绝重复注册、注销、bank pending snapshot dispatch，以及 handled/unhandled 64-bit 位图报告。
- [x] dispatch 按 local IRQ 从低到高调用 handler，不重读 pending，不修改 mask，也不声称完成 EOI。
- [x] 新增 overlapping-parent-map fixture，source 0 同时出现在 int0/int1 时明确拒绝。

### 验证证据

- 2K1000LA driver host 单测由 5 项增至 9 项，全部通过。
- domain 边界测试验证 bank0/local0 -> global0、bank1/local31 -> global63，并拒绝 bank2 和 local32。
- dispatch 测试在 bank1 同时提交 local 0/7/31：已注册 0 和 31 被调用并形成 global bits 32/63，未注册 7 报告为 global bit39。
- 注册测试覆盖 duplicate、unregister 和二次 unregister；无效 bank count 0/3 被拒绝。
- 有效 DTS fixture 端到端断言两组 parent specifier 与 maps；缺 UART clock 和重叠 parent map 两个畸形 fixture 均被拒绝。
- 2K1000LA release 交叉构建通过，`git diff --check` 通过；测试只生成三个临时小型 DTB。

### 已知限制、未验证与后续测试

- [ ] **domain 和 dispatch 仅为纯模型，尚未连接 LoongArch CPU HWI trap、真实 LIOINTC pending 或 mask/ack，全部待上板验证。**
- [ ] 固定 64 IRQ 符合 2K1000LA 两 bank；不是面向所有 Loongson SoC 的通用 IRQ domain。
- [ ] handler 目前是 `fn(GlobalIrq)`，不能携带设备实例上下文；UART/MMC 接入前需要确定静态实例或受控 context 方案。
- [ ] register/unregister 要求外部独占且必须在开中断前完成；运行期动态注销需要 interrupt-safe synchronization 和 quiesce 协议。
- [ ] source map 严格要求覆盖 32 bits，符合目标 DTS；若真实固件保留未连接 source，需基于导出 DTB 和 binding 重新评估而非静默放宽。
- [ ] dispatch 遇到 unhandled source 只报告，不自动 mask；未来 trap glue 必须采用有界循环并处理持续电平，防止中断风暴。
- [ ] 下一批应完善 MMC 的 clocks、DMA、bus-width、card-detect 与 non-removable topology，为复用已有 DesignWare/SD 协议层做准备。

### 提交

- `[feat] add 2K1000LA IRQ domain model`

## 2026-08-10：批次 22——2K1000LA MMC clock/DMA/card-detect 拓扑

### 任务与设计

1. 依据上游 2K1000 DTS/参考板 DTS 补齐 MMC 的 APB clock、APB DMA、4-bit bus 和 GPIO card-detect 描述。
2. phandle specifier 根据 provider 的 `#clock-cells`、`#dma-cells`、`#gpio-cells` 动态切分，不写死参数宽度。
3. 命名资源要求 name 数量与 specifier 数量一致；目标 MMC 的 DMA 只接受单个 `rx-tx` channel。
4. `cd-gpios`、`broken-cd`、`non-removable` 三种介质策略互斥；无属性时保存 native detect。
5. supply 只保存并验证 provider phandle，本批不操作 regulator、clock、DMA 或 MMC 寄存器。

### 完成内容

- [x] 新增 `ResourceSpecifier { provider_phandle, args }` 和 `NamedResource`，单 provider 最多接受 8 个参数 cells。
- [x] 通用 parser 逐项读取 phandle、查询 provider cell count，拒绝未知 provider、截断参数、非 cell 对齐和超宽 specifier。
- [x] `MmcDescription` 新增 clocks、可选 DMA、bus width、card-detect、`vmmc-supply` 和 `vqmmc-supply`。
- [x] MMC 要求恰好一个 clock；`clock-names` 可选，但存在时必须与 clock 数量一致。
- [x] `dmas`/`dma-names` 必须同时出现或同时缺失；目标节点只接受一个名为 `rx-tx` 的 DMA specifier。
- [x] `bus-width` 缺省为 1，只接受 1/4/8。
- [x] GPIO card-detect 保存 provider 与 `<22, GPIO_ACTIVE_LOW>` 原始参数；支持 native、broken 和 non-removable 策略。
- [x] boolean 属性必须是零长度 DTB property，拒绝带值的伪 boolean。
- [x] supply property 必须是单 phandle 且 provider 存在。
- [x] 有效 fixture 的 MMC MMIO 大小修正为上游 DTS 的 `0x68`，并增加真实 clock/DMA/GPIO provider 形态。

### 验证证据

- 既有 9 项 LIOINTC/domain host 单测全部通过。
- 有效 fixture 经 `dtc` 后断言 clock args `[0]`、DMA 名称 `rx-tx`/args `[0]`、bus width 4、GPIO args `[22,1]` 及两个 supply phandle。
- 动态 non-removable fixture 走同一 parser 并得到 `CardDetect::NonRemovable`。
- 同时含 `cd-gpios` 与 `broken-cd` 的 fixture 被拒绝。
- DMA provider 声明 `#dma-cells = 1` 而 consumer 缺少 argument 时，`dtc` 发出 warning，WaterOS parser继续明确返回错误。
- 缺 UART clock 与重叠 LIOINTC parent map 的既有畸形 fixtures 继续被拒绝。
- 2K1000LA release 交叉构建通过，`git diff --check` 通过；所有测试 DTB 均为临时小文件。

### 已知限制、未验证与后续测试

- [ ] **资源解析已验证，但 clock enable、APBDMA channel 0 语义、GPIO active-low 电平和卡槽电气行为全部待真机验证。**
- [ ] 上游 `.dtsi` 默认禁用 MMC/APBDMA，参考板 `.dts` 才启用；实际固件 DTB 的 status 与 pinctrl 必须导出核对。
- [ ] 尚未解析 pinctrl、reset、`max-frequency`、write-protect、SD voltage/capability 属性和 DMA coherency constraints。
- [ ] `vmmc/vqmmc` 仅保存 phandle，没有 regulator framework，不能切换供电或信号电压。
- [ ] DMA topology 不等于 DMA driver；在 cache maintenance、descriptor ownership 和中断完成路径验证前必须使用 polling/PIO bring-up。
- [ ] 当前 Loongson 分支尚未拥有 VisionFive 分支中的 DesignWare MMC/SD 协议实现；复用前应抽到独立公共 crate，而不是跨平台复制代码。
- [ ] 下一批应提取跨平台 DesignWare MMC polling/PIO transport 与 SD protocol crate，并让两个平台只提供寄存器资源和 board prerequisites。

### 参考依据

- Linux `loongson-2k1000.dtsi`：MMC `0x1fe2c000/0x68`、辅助区、APB clock、APBDMA1 channel 0 与 `rx-tx`。
- Linux `loongson-2k1000-ref.dts`：MMC enable、4-bit bus、GPIO22 active-low card detect。

### 提交

- `[feat] discover 2K1000LA MMC resources`

## 2026-08-10：批次 23——共享 DesignWare MMC/SD 核心抽取

### 任务与设计

1. 审计 VisionFive 2 已有 DesignWare MMC 与 SD 协议实现的平台耦合。
2. 在 `wateros-driver/driver-block` 下建立独立、`no_std` 的共享核心 crate。
3. 迁入寄存器轮询/PIO、识别时钟、SD 枚举、CSD 容量解析和只读块适配。
4. 平台层继续负责 DTB 资源、外部时钟、复位、pinmux、供电和卡检测。
5. 复用原有 mock 测试，并做 RISC-V 裸机目标编译检查。

本批只建立两个平台可共同依赖的控制器/协议边界，不在缺少真机时启用设备注册。DMA、多块命令和写入路径继续排除在首轮核心之外。

### 完成内容

- [x] 新增 `wateros-driver-block-impl-dw-mmc` workspace crate。
- [x] `RegisterIo` 隔离真实 MMIO 与 host mock 后端。
- [x] 迁入版本化 FIFO、复位、时钟更新/分频、命令执行和单块 PIO 读取。
- [x] 迁入 SD v1/v2 枚举、OCR 有界轮询、寻址选择和 CSD v1/v2 容量计算。
- [x] 保留通用 `BlockDevice` 只读适配、整段越界预检和错误映射。
- [x] 排除 JH7110 clock/reset/syscon 等板级资源结构，避免共享核心依赖具体 SoC。

### 验证证据

- `cargo test -p wateros-driver-block-impl-dw-mmc`：12 项 host 单测全部通过。
- `cargo check -p wateros-driver-block-impl-dw-mmc --target riscv64gc-unknown-none-elf` 通过。
- 覆盖复位/超时/CRC/硬件锁、旧/新 FIFO、识别时钟、SDHC/SDSC、CSD、地址溢出、跨盘尾读取和写入拒绝。

### 已知限制、未验证与后续测试

- [ ] 尚未回接 VisionFive 2 平台 crate；下一批应替换重复实现并做比较验证。
- [ ] 2K1000LA 是否完全兼容该 DesignWare 寄存器布局仍需固件 DTB、手册和真机寄存器确认。
- [ ] CIU/BIU 时钟、reset、pinmux、供电/电压、card-detect 与长响应顺序均为真机未验证。
- [ ] 当前仅轮询、PIO、单块、只读；IRQ、DMA、多块、写入及 cache 一致性尚未实现。
- [ ] host mock 与交叉编译不能替代插卡启动、已知 CSD 对照、盘尾读取和长时间 I/O 压测。

### 提交

- `[ref] extract shared DesignWare MMC core`

## 2026-08-10：批次 25——2K1000LA 共享 SD 核心与延迟激活计划

### 任务与设计

1. 将共享 DesignWare MMC/SD crate 合入 2K1000LA 分支。
2. 核对 Loongson MMC DTB 窗口是否能直接满足共享寄存器后端。
3. 建立只验证资源、不触碰硬件的 deferred bring-up plan。
4. 将寄存器布局、clock、FIFO、供电和 card-detect 缺口编码为显式 blocker。
5. 用合成资源、真实 DTS fixture、共享测试和 LoongArch64 交叉编译验证。

审计发现主窗口仅 `0x68`，另有独立 auxiliary window；共享 `MmioRegisters` 则按控制器版本在偏移 `0x100/0x200` 访问 FIFO。因此当前只能确认共享 SD 协议可复用，不能确认 DesignWare 主机寄存器布局可直接复用。

### 完成内容

- [x] 2K1000LA 分支合入并依赖 `wateros-driver-block-impl-dw-mmc`。
- [x] 新增 `mmc::BringUpPlan`，保存 controller/auxiliary 两段 MMIO 与 bus width。
- [x] 主窗口小于 `0x68`、缺 auxiliary window 或缺 clock 时在任何 MMIO 操作前拒绝。
- [x] 显式列出 split layout、输入时钟、clock control、FIFO depth、供电和 card-detect 六类 blocker。
- [x] `can_activate()` 固定返回 false，代码注释禁止在布局确认前构造真实 `DwMmc`。
- [x] 启动路径只生成并打印 deferred plan，不读取或写入 MMC 寄存器。
- [x] 真实 DTS fixture 断言 plan 保持禁用且含 split-layout blocker。

### 验证证据

- 共享 MMC/SD 核心 12 项 host 单测全部通过。
- 2K1000LA driver 11 项 host 单测全部通过，其中 2 项覆盖 deferred plan 的保留/拒绝行为。
- `tests/verify_topology.sh` 全部通过：真实形态 fixture、non-removable 与 4 个畸形 DTB 场景继续验证。
- `cargo check -p wateros-driver-impl-loongson2k1000la --target loongarch64-unknown-none` 通过。
- 所有 DTB 都在 `mktemp` 小目录生成并由 trap 清理，没有创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] **共享 SD 状态机可复用不等于共享 DesignWare 寄存器后端可用；当前激活被代码强制禁止。**
- [ ] auxiliary `0x1fe00438` 的 FIFO/控制语义和主窗口寄存器布局需查厂商手册并用真机安全读取确认。
- [ ] APB clock 实际频率、enable/reset 顺序、FIFO depth、GPIO22 电平、regulator 与 pinmux 均未知。
- [ ] 尚未实现 2K1000LA split-window `RegisterIo`；确认映射后应先用 mock 后端覆盖 offset routing，再进行只读 PIO bring-up。
- [ ] DMA channel 0、IRQ 完成、cache 一致性、写入和多块传输继续禁用。

### 提交

- `[feat] plan deferred 2K1000LA MMC bring-up`

## 2026-08-10：批次 26——2K1000LA 专属 MMC 命令核心

### 任务与设计

1. 检索 Linux 主线驱动与 DT binding，查明控制器和第二 MMIO 区域语义。
2. 纠正“可能是 split-window DesignWare”假设，明确只复用共享 SD 协议层。
3. 实现 Loongson 专属、可 mock 的时钟分频和非数据命令轮询核心。
4. 数据路径、外部 DMA、IRQ、供电与 card-detect 不完整时继续禁止设备激活。
5. 记录上游来源、许可证和真机验证边界。

上游 `loongson2-mmc` 证明主窗口包含独立的 CTL/PRE/CARG/CCTL/RSP/INT/DATA 寄存器，第二窗口只是 APB DMA routing config。2K1000 使用外部 APB DMA，并非 DesignWare MMC host。

### 完成内容

- [x] deferred plan blocker 改为数据路径、外部 DMA、clock control、供电、card-detect 和 IRQ 六项真实缺口。
- [x] 新增 Loongson `RegisterIo` 与 `Host<R>`，不依赖 DesignWare 寄存器类型。
- [x] 实现输入/目标时钟校验、向上取整且上限 255 的 prescaler 编码和 clock enable RMW。
- [x] 实现 CMD index/argument、短/长响应标志、W1C interrupt、命令完成/timeout/CRC 的有界轮询。
- [x] 响应读取覆盖 RSP0..RSP3；物理 word ordering 在注释中保持 `UNVERIFIED_ON_HARDWARE`。
- [x] 新增上游参考与 GPL-2.0-only 许可证说明；没有将 Linux C 源码 vendoring 进仓库。

### 验证证据

- 2K1000LA driver host 单测由 11 项增至 13 项并全部通过。
- 新测试检查 125 MHz 到 400 kHz 的饱和 prescaler、clock enable 保留位、CMD8 编码、响应读取、轮询超时、命令超时和 CRC 错误。
- 既有 deferred plan 与 LIOINTC/IRQ domain 测试继续通过。
- 参考资料确认主寄存器最大 offset `0x64` 与 DT 主窗口 `0x68` 一致，第二窗口描述为 APB DMA config。

### 已知限制、未验证与后续测试

- [ ] 当前仅非数据命令基础，不实现 `SdTransport`，因此不会误注册不可读写的卡。
- [ ] Linux 上游 2K1000 数据路径使用外部 DMA；WaterOS 尚无 APBDMA driver、descriptor、IRQ completion 或 cache maintenance。
- [ ] clock provider enable/rate、controller reset、vmmc/vqmmc、GPIO22 card detect 与 pinctrl 仍未实现。
- [ ] INT W1C、RSP0..3 顺序和 255 prescaler 的实际低速输出必须用逻辑分析或已知卡在真机对照。
- [ ] 后续应先实现 APBDMA channel 0 的可 mock descriptor/routing 层，再把 host 接成共享 `SdTransport`。

### 参考与许可证

- `docs/references/loongson2-mmc-upstream.md`

### 提交

- `[feat] add 2K1000LA MMC command core`

## 2026-08-10：批次 27——2K1000 APBDMA descriptor 计划

### 任务与设计

1. 审计 WaterOS 的 DMA、物理地址和 cache maintenance 抽象。
2. 对照 Linux `loongson2-apb-dma` 确认 descriptor 与启动寄存器编码。
3. 建立不分配内存、不做地址转换、不访问 MMIO 的纯数据 transfer plan。
4. 显式携带 descriptor clean 和 read buffer invalidate 要求。
5. 用 host 单测覆盖地址、方向、分段、路由和拒绝路径。

仓库没有可供真实 SoC DMA 共用的框架；VirtIO HAL 依赖恒等映射且 cache hooks 为空，不能作为 2K1000 APBDMA 的可用性证据。本批因此只实现可审核的 descriptor 编码，未来 executor 必须提供 DMA-capable 物理内存和架构 cache 操作。

### 完成内容

- [x] 新增 `apbdma` 模块以及 48 字节 `#[repr(C)] HardwareDescriptor`。
- [x] 支持 64 位内存/descriptor 地址、32 位 APB 外设地址和读写方向命令位。
- [x] 按 word 数、burst words 计算 `length_words` 与 `step_times`。
- [x] descriptor 地址要求低 5 位为零，避免与 order register 控制位冲突。
- [x] 生成 `64BIT_EN | START` order 值，并保留 descriptor 物理地址高位。
- [x] `route_sdio_to_dma1()` 只更新 routing bits 17:15，不破坏同一 syscon 的其他位。
- [x] transfer plan 明确要求启动前 clean descriptor；device-to-memory 完成后 invalidate buffer。
- [x] MMC blocker 细化为 `ExternalDmaExecutorUnavailable`，仍禁止激活。

### 验证证据

- 2K1000LA driver host 单测由 13 项增至 16 项并全部通过。
- 512 字节 read fixture 验证 64 位地址拆分、16-word segment、8 steps、DATA `0x1fe2c040` 和 start order。
- write fixture 验证 direction bit、无需 read invalidate，以及 DMA1 route 的 read-modify-write 结果。
- 拒绝零长度、非 4 字节长度、descriptor 非 32 字节对齐、零 burst 和超过 32 位的 APB 地址。
- `HardwareDescriptor` host 断言大小为 48 字节。

### 已知限制、未验证与后续测试

- [ ] transfer plan 不是 DMA executor；尚无 descriptor/buffer 分配、VA→PA、cache clean/invalidate 或内存屏障实现。
- [ ] 尚未解析并驱动 APBDMA controller 自身的 MMIO、clock 和 IRQ topology。
- [ ] descriptor 字段字节序、order register 64 位访问顺序、routing syscon 和 IRQ completion 均待真机验证。
- [ ] 单 descriptor 计划尚未实现 scatter-gather 链、取消、超时回收和并发 channel ownership。
- [ ] 在 cache hooks 可用并经真机确认前，不得把 executor 接入 `SdTransport`。

### 参考与许可证

- `docs/references/loongson2-mmc-upstream.md` 已补充 GPL-2.0-or-later APBDMA 来源。

### 提交

- `[feat] model 2K1000 APBDMA descriptors`

## 2026-08-10：批次 28——APBDMA topology、lease 与 executor 状态机

### 任务与设计

1. 解析 2K1000 APBDMA controller 的 MMIO、IRQ、clock、phandle 和 channel cells。
2. 用 provider/channel lease 防止同一 DMA channel 被重复占用。
3. executor 只接受完成 cache 同步后的不可直接构造 token。
4. mock order register 覆盖 start、busy、IRQ completion 和 stop。
5. DTS fixture 增加缺 clock 与短 MMIO 窗口拒绝测试。

topology、ownership 和 executor 分层：DTB 只描述资源；lease 管理 provider channel 0 的独占；executor 管理单个正在运行的 descriptor。真实 cache/地址转换尚未实现，因此 `PreparedTransfer::after_cache_sync` 是带安全契约的入口，正常安全代码不能绕过准备门槛。

### 完成内容

- [x] `BoardTopology` 新增 `dma_controllers` 与 `DmaControllerDescription`。
- [x] APBDMA 要求单个 8-byte MMIO、单个 clock、合法 IRQ、phandle 和 `#dma-cells = 1`。
- [x] 上游形态 fixture 补齐 APBDMA1 `0x1fe00c10/8`、LIOINTC1 IRQ13 和 APB clock。
- [x] `ChannelLeases` 按 provider phandle 管理 channel 0，拒绝未知 provider、非零 channel 和重复 claim。
- [x] `PreparedTransfer` 只能通过带 descriptor/buffer cache 同步前置条件的 `unsafe` 构造函数创建。
- [x] `Executor` 启动时先写 0 再写 start order，拒绝并发 start；IRQ completion 返回是否需要 invalidate read buffer。
- [x] stop 保留 descriptor address、清除控制低位并编码 64-bit + STOP。
- [x] 启动日志报告 DMA controller 数量，但仍不实例化真实 executor。

### 验证证据

- 2K1000LA driver host 单测由 16 项增至 19 项，全部通过。
- lease 测试覆盖 claim/busy/release/reclaim、错误 provider 和错误 channel。
- mock executor 测试覆盖 start write 序列、重复启动、完成后二次完成拒绝和 stop 编码。
- DTS fixture 脚本通过既有场景，并新增缺 APBDMA clock、4-byte MMIO 两个拒绝场景。
- 有效 fixture 断言 APBDMA MMIO、IRQ `<13,4>`、clock arg `[0]` 和 channel cell count。

### 已知限制、未验证与后续测试

- [ ] executor 仍只有 mock `OrderIo`，没有真实 volatile 64-bit lo/hi MMIO 后端。
- [ ] `unsafe` cache-sync token 是防误用契约，不是 cache maintenance 实现；LoongArch64 cache API 仍缺失。
- [ ] 尚未把 APBDMA IRQ13 接入 LIOINTC domain，也未校验 descriptor status/硬件错误位。
- [ ] lease 当前是平台内存对象，尚未接入全局动态设备 topology 或移除/quiesce 流程。
- [ ] APBDMA controller clock enable、descriptor 内存分配/物理地址、memory barrier 和真机 stop 行为待验证。
- [ ] MMC 仍不实现 `SdTransport`，不会注册块设备。

### 提交

- `[feat] add 2K1000 APBDMA lifecycle model`

## 2026-08-10：批次 29——公共 DMA ownership/cache contract

### 任务与设计

1. 审计公共 frame allocator、地址转换、VirtIO HAL 与两架构 cache 能力。
2. 在稳定 driver API 中定义不假设恒等映射的 DMA region。
3. 用 ownership 状态机强制 CPU→device→CPU 同步顺序。
4. 平台通过 trait 实现 cache maintenance 和 memory ordering，不允许公共层提供空操作。
5. 用 host mock 和双架构裸机目标编译验证契约。

现有 frame allocator只提供物理页，尚无通用连续 DMA 分配与 cache API；多个 VirtIO HAL 的 `share/unshare` 依赖恒等映射并且同步为空，不能外推到物理板。本批只建立平台必须满足的 contract，不伪造硬件实现。

### 完成内容

- [x] `wateros-driver-api-v0` 新增公共 `dma` 模块。
- [x] `DmaRegion` 分别保存 virtual/physical address、长度和 alignment。
- [x] 构造时拒绝零地址/长度、非 2 次幂对齐、两侧地址未对齐、地址溢出和超过设备 address width。
- [x] 定义 `ToDevice`、`FromDevice`、`Bidirectional` 三种方向。
- [x] `DmaCoherency` 要求平台分别实现 `sync_for_device` 与 `sync_for_cpu`。
- [x] `DmaMapping` 初始归 CPU；同步成功后才转移给 device，完成或确认 stop 后才归还 CPU。
- [x] device ownership 期间 `cpu_region()`、重复 prepare 和提前拆分 mapping 均被拒绝。
- [x] 同步失败不会错误转移 ownership。

### 验证证据

- driver API 3 项 host 单测全部通过。
- 测试覆盖不同 VA/PA、64-bit PA、32-bit device address 越界、alignment、完整同步事件序列和失败回滚。
- `cargo check -p wateros-driver-api-v0 --target riscv64gc-unknown-none-elf` 通过。
- `cargo check -p wateros-driver-api-v0 --target loongarch64-unknown-none` 通过。
- `git diff --check` 通过；本批没有创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] contract 不分配内存，也不做 VA→PA；需要建立可证明物理连续、可回收的 DMA allocator。
- [ ] RISC-V/LoongArch64 均未提供真实 `DmaCoherency` backend；cache line 大小、指令可用性与 firmware coherency 必须按平台确认。
- [ ] 现有 VirtIO HAL 尚未迁移到公共 contract，其空 `share/unshare` 行为仍只适用于当前 QEMU coherent/identity 环境。
- [ ] ownership 是单 mapping 状态机，尚未定义 scatter-gather、子区间同步、并发引用或 IOMMU mapping。
- [ ] 下一批应先为 LoongArch64 建立保守的 DMA allocation/translation 边界，再让 2K1000 APBDMA executor 消费 `DmaMapping`。

### 提交

- `[feat] add physical DMA ownership contract`

## 2026-08-10：批次 30——APBDMA 接入公共 DMA ownership

### 任务与设计

1. 将公共 `DmaRegion`/`DmaMapping` contract 合入 2K1000LA 分支。
2. 让 APBDMA plan 与真实 descriptor/payload mapping 做地址、长度和方向一致性校验。
3. 只有两个 mapping 都完成 device-side sync 后才生成 `PreparedTransfer`。
4. busy/MMIO 启动失败返回 token，支持安全取消和 ownership 回滚。
5. IRQ completion 或确认 stop 后同步并归还 CPU ownership。

descriptor 与 payload 分别拥有 mapping；plan 只是期望值，不能替代实际物理内存元数据。completion 和 prepared token 的构造均保持私有，普通安全代码无法跳过 cache contract 或伪造 DMA 已完成。

### 完成内容

- [x] 合入公共物理 DMA ownership contract。
- [x] `TransferPlan` 增加原始 payload PA、byte length 和 direction。
- [x] `prepare_transfer()` 校验 descriptor PA/最小 48-byte、payload PA/精确长度以及两个 mapping 的方向。
- [x] descriptor 固定为 `ToDevice`；payload 根据 DMA 方向要求 `FromDevice` 或 `ToDevice`。
- [x] descriptor sync 成功而 payload sync 失败时，descriptor 自动恢复 CPU ownership。
- [x] 移除公开 unsafe `PreparedTransfer` 构造器，prepared token 只能由校验/同步流程产生。
- [x] `StartFailure` 在 busy 或 order-register 写失败时归还 prepared token。
- [x] `cancel_prepared()` 恢复尚未启动的两个 mapping；`finish_transfer()` 消费 IRQ/stop completion 后归还 ownership。
- [x] completion 字段私有，只能由运行中的 executor 产生。

### 验证证据

- 公共 DMA API 3 项 host 单测全部通过。
- 2K1000LA driver 20 项 host 单测全部通过。
- 新测试覆盖 plan/mapping PA 不一致、payload sync 失败回滚、device ownership 期间 CPU 访问拒绝、busy token 归还、IRQ 完成和 stop 后恢复。
- DTS fixture 全部通过，包括 APBDMA 资源畸形场景。
- `cargo check -p wateros-driver-impl-loongson2k1000la --target loongarch64-unknown-none` 通过。

### 已知限制、未验证与后续测试

- [ ] mock coherency 只证明调用顺序；尚无 LoongArch64 cache clean/invalidate 与 barrier 实现。
- [ ] mapping 元数据由测试构造；尚无真实连续 DMA allocator 和 VA→PA provider。
- [ ] executor 仍未接入 volatile order-register backend、APBDMA IRQ13 或 descriptor status。
- [ ] 启动第二次 64-bit order write 失败时硬件是否可能部分观察，未来真实 backend 必须在回滚前执行 stop/quiesce。
- [ ] MMC 仍未实现数据 command 与共享 `SdTransport`，不会注册块设备。

### 提交

- `[ref] enforce DMA ownership in 2K1000 APBDMA`

## 2026-08-10：批次 31——物理连续 DMA 帧所有权

### 任务与设计

1. 为公共物理帧分配 API 增加连续、按页对齐的区间契约。
2. 保持单页分配和引用计数语义，连续区间分配/释放必须原子失败。
3. 用不可复制的 RAII 对象封装连续物理 RAM，分别暴露物理地址和恒等映射借用。
4. 增加无架构 host 测试缝，默认内核构建仍保留中断屏蔽与 CPU-aware locking。
5. 运行 host 单测及 RISC-V64、LoongArch64 双架构构建。

`FrameSpan` 只描述分配器返回的起始 PPN 和页数；`OwnedPhysFrameSpan` 才持有回收责任。
对齐单位为页且必须为 2 的幂。释放前先检查区间内每页均已分配且引用计数恰为 1，任何
一页不满足时整段保持不变。驱动必须从 `physical_address()` 取得设备地址，不能把恒等
映射 slice 的虚拟指针当作通用 VA→PA 转换。

### 完成内容

- [x] `PhysicalFrameAllocator` 新增带默认 `Unsupported` 的连续分配/释放方法，不破坏其它实现。
- [x] `FrameSpan` 保存起始 frame id 与 frame count，不对外开放字段修改。
- [x] stack allocator 支持非零 2 次幂页对齐，优先从未使用连续高段分配，再扫描回收区间。
- [x] 对齐产生的空洞页进入既有回收栈，不会丢失可用帧。
- [x] 分配成功后一次性标记区间内所有页为 allocated/refcount=1；失败不修改状态。
- [x] 连续释放拒绝越界、零长度、重复释放以及含共享引用的区间，检查完成后才统一回收。
- [x] 新增全局 `frame_alloc_contiguous` / `frame_dealloc_contiguous` 入口。
- [x] `OwnedPhysFrameSpan::alloc_zeroed` 封装清零、物理地址、长度、字节借用和 Drop 整段回收。
- [x] stack crate 增加默认开启的 `kernel-arch` 特性；仅显式关闭时使用 host 测试路径，生产默认行为不变。

### 验证证据

- `cargo test -p wateros-mm-frame-alloctor-impl-stack --no-default-features --features api-v0`：3 项通过。
- host 测试覆盖对齐空洞保留、释放后复用、重复释放、共享引用拒绝、非法参数、OOM 和失败不变性。
- `make check`：RISC-V64 release cargo check 通过。
- `make kernel-rv`：RISC-V64 release kernel 构建通过。
- `make kernel-la`：LoongArch64 release kernel 构建通过。
- 本批 Rust 文件已按仓库 `.rustfmt.toml` 单独格式化；`git diff --check` 通过。
- 全仓 `cargo fmt --all -- --check` 仍会报告 vendor 与其它既有文件的大量格式差异，本批未改动这些文件。

### 已知限制、未验证与后续测试

- [ ] 尚无真机，未验证两块板上的物理 RAM 恒等映射范围、cache coherency、DMA address width 和设备实际读写。
- [ ] `OwnedPhysFrameSpan` 只提供连续 RAM 所有权与明确 PA；尚未直接生成 driver API 的 `DmaRegion`/`DmaMapping`。
- [ ] 当前连续分配为线性扫描，适合 bring-up 和小规模 descriptor/buffer；长期需要更高效的 buddy/extent allocator。
- [ ] 对齐只支持页粒度，不能表达小于 4 KiB 的 descriptor 对齐或大于页且非 2 次幂的设备约束。
- [ ] 连续分配尚未接入 2K1000 APBDMA descriptor/buffer executor，也未迁移现有 VirtIO HAL。
- [ ] 清零和字节借用依赖可分配 RAM 恒等映射；未来若改为高半内核或 IOMMU，需由平台提供显式映射。
- [ ] 后续应增加从 owned span 到 `DmaMapping` 的受控适配，并实现/验证 LoongArch64 cache maintenance backend。

### 提交

- `[feat] allocate owned contiguous physical frames`

## 2026-08-10：批次 32——2K1000LA owned APBDMA resources

### 任务与设计

1. 将公共连续物理帧提交同步到 Loongson 2K1000LA 平台分支。
2. 用 owned allocation 替代 APBDMA 调用方手工拼装的 VA/PA mapping 元数据。
3. descriptor 与 payload 分别持有连续 RAM、精确 DMA region 和 coherency ownership。
4. descriptor 必须写入其真实 DMA allocation，再经过 sync 后才能产生 prepared token。
5. device ownership 下禁止 CPU 字节借用；错误析构不得回收仍可能被硬件访问的物理页。

`OwnedDmaBuffer<C>` 同时持有 `OwnedPhysFrameSpan` 和 `DmaMapping<C>`。当前 2K1000LA
内核采用 RAM 恒等映射，因此 region 的 VA/PA 数值相同，但构造仍分别填写两个字段，且
必须通过设备地址宽度检查。`OwnedTransferResources<D, P>` 自动从实际 descriptor/payload
PA 构建 APBDMA plan，并将 48-byte hardware descriptor 写入 owned descriptor 区域。
真实 cache backend 仍由调用方注入，本批不提供未经验证的空实现。

若资源在 device ownership 下被错误析构，安全兜底会记录错误并保留物理区间，而不是
把可能仍在 DMA 的页交回分配器。正常 IRQ completion 或确认 stop 后恢复 CPU ownership，
仍会按 RAII 正常回收。这一泄漏仅保护错误路径，后续应以 typestate running-transfer API
从类型层禁止该误用。

### 完成内容

- [x] Loongson 分支合入批次 31 的连续帧 API、stack 实现和 `OwnedPhysFrameSpan`。
- [x] 新增 `dma_memory` 模块和 target-specific frame allocator 依赖，host 测试不拉入 ISA backend。
- [x] `allocation_layout` 将任意正字节长度向上取整为物理页数，并保留大于页的 2 次幂对齐。
- [x] `OwnedDmaBuffer::allocate_zeroed` 创建真实连续 allocation、精确 byte-prefix region 和 mapping。
- [x] CPU byte slice 访问先验证 mapping 归 CPU，device ownership 时返回错误。
- [x] 公共 `DmaMapping::is_cpu_owned` 提供只读释放安全判断，并补充 ownership 转换断言。
- [x] device-owned buffer 的 Drop 不回收物理页，避免 DMA use-after-free，并记录 `UNVERIFIED_ON_HARDWARE` 错误。
- [x] `OwnedTransferResources::allocate` 自动分配 descriptor/payload、编码真实 PA 并写入 descriptor。
- [x] resources 封装 prepare/cancel/finish，复用既有 cache sync、失败回滚和 IRQ/stop ownership 流程。
- [x] descriptor 使用 32-byte 对齐/`ToDevice`；payload 使用 4-byte 对齐及与传输方向匹配的 mapping。

### 验证证据

- `wateros-driver-api-v0`：3 项 host 单测全部通过。
- 2K1000LA driver：23 项 host 单测全部通过（本批新增 3 项）。
- 新测试覆盖页数向上取整、大于页的对齐、零长度、非 2 次幂、算术溢出和 32-bit DMA 地址越界。
- 既有 mock 测试继续覆盖 descriptor/payload ownership 顺序、sync 失败回滚、IRQ completion 和 stop。
- `cargo check -p wateros-driver-impl-loongson2k1000la --target loongarch64-unknown-none` 通过，实际编译 owned 生产路径。
- `tests/verify_topology.sh` 全部通过；truncated DMA fixture 的 dtc warning 属于预期畸形输入。
- `make kernel-la` QEMU LoongArch64 release 回归构建通过；仅有仓库既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：尚未验证 2K1000LA 可分配 RAM 的完整恒等映射、cache line 大小和 DMA snoop 行为。
- [ ] 尚无生产 `DmaCoherency` backend，因此真实 APBDMA 路径仍不能构造可安全使用的 resources。
- [ ] owned resources 尚未接入 volatile order-register backend、IRQ13 dispatch、descriptor status/error 检查或 MMC data command。
- [ ] device-owned Drop 的保留策略避免 use-after-free，但会永久泄漏该区间；应以持有 executor/resource 的 typestate running token 替代。
- [ ] descriptor 的 native-endian 内存布局、48-byte 大小和 APBDMA 实际 fetch 行为仍需真机核对。
- [ ] APBDMA 的实际 DMA address width 尚未从 DTB/平台 capability 推导，当前由未来调用方显式传入。
- [ ] payload 只支持单个物理连续区间，尚无 scatter-gather、bounce buffer 或 IOMMU mapping。
- [ ] 下一批优先实现不会伪造 cache coherency 的 LoongArch cache/barrier capability 探测，或先完成 typestate executor ownership。

### 提交

- `[feat] own 2K1000 APBDMA transfer memory`

## 2026-08-10：批次 33——APBDMA typestate transfer lifecycle

### 任务与设计

1. 收紧裸 `PreparedTransfer` 与 owned allocation 生命周期可能分离的接口。
2. 用借用式 typestate 表达 prepared、running、quiesced 三个硬件阶段。
3. running 阶段同时独占借用 executor、descriptor mapping 和 payload mapping。
4. 所有失败状态返回原 session，支持显式取消、stop 或同步重试。
5. 让双 mapping 的 CPU-side sync 在部分成功后保持幂等可恢复。

`PreparedSession` 在两个 mapping 完成 device sync 后持有其可变借用；`start()` 消费它并
产生同时借用 `Executor` 的 `RunningSession`。IRQ completion 或确认 stop 只能产生
`QuiescedSession`，此时硬件已不再运行，但 CPU cache ownership 仍可能尚未完全恢复。
`finish()` 只有在两个 mapping 都归 CPU 后才释放借用。各 session 带 `must_use`，错误
转换通过 `SessionFailure<E, S>` 返回原状态，避免错误路径丢失恢复能力。

底层 token/executor 方法保留为 crate 内原语和 mock 测试工具；owned resources 对外只提供
`prepare_session()`，不再返回与 allocation 无生命周期关系的裸 token。

### 完成内容

- [x] 新增 `PreparedSession`、`RunningSession` 和 `QuiescedSession` 三阶段类型。
- [x] prepared session 的两个 mapping 借用阻止 CPU buffer 访问和 allocation 回收。
- [x] running session 进一步独占 executor，正常安全代码无法并发 start/stop 同一 executor。
- [x] IRQ 与 stop 分别把 running 转换为 quiesced，不能直接跳到 CPU ownership。
- [x] prepared cancel、start busy、running completion/stop 和 quiesced finish 失败均返回原 session。
- [x] `OwnedTransferResources` 删除裸 prepare/cancel/finish 组合，改为单一 `prepare_session()` 入口。
- [x] `prepare_transfer`、`finish_transfer`、`cancel_prepared` 收紧为 crate 内底层原语。
- [x] finish/cancel 对已归 CPU 的 mapping 幂等，只重试仍归 device 的 mapping。
- [x] 若第一次 payload sync 成功、descriptor sync 失败，第二次 finish 可继续 descriptor 而不重复 payload sync。
- [x] session 类型标记 `must_use`；错误调试输出只显示错误，不要求 mapping/backend 实现 `Debug`。

### 验证证据

- `wateros-driver-api-v0`：3 项 host 单测全部通过。
- 2K1000LA driver：27 项 host 单测全部通过（本批新增 4 项）。
- typestate host 测试覆盖 IRQ completion、stop、底层 executor busy 时 prepared session 返还和取消。
- 部分同步测试注入 descriptor 首次 CPU sync 失败，验证 payload 已归 CPU、重试仅恢复 descriptor，最终两者均归 CPU。
- Rust 借用关系在编译期确保 live `RunningSession` 持有 executor 与两个 mappings；测试代码无法同时取得第二个可变借用。
- `cargo check -p wateros-driver-impl-loongson2k1000la --target loongarch64-unknown-none` 通过。
- 2K1000LA DTS topology/畸形 fixture 全部通过；truncated DMA 的 dtc warning 为预期输入。
- `make kernel-la` QEMU LoongArch64 release 回归构建通过；仅有仓库既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：typestate 证明软件生命周期，不证明 APBDMA stop/IRQ 已真正停止总线访问。
- [ ] Rust 允许显式 `mem::forget`；若调用方故意遗忘 session，owned buffer 的 device-owned Drop 保留策略仍是最终防 UAF 兜底。
- [ ] 尚无生产 cache maintenance、memory barrier、volatile order-register backend 或 IRQ13 acknowledgement。
- [ ] completion 尚未读取/验证 descriptor status，无法区分成功、总线错误和短传输。
- [ ] executor 仍只支持一个 descriptor，不支持 scatter-gather、超时状态机或异步 waitqueue。
- [ ] 下一批可实现 APBDMA order-register 的 volatile lo/hi 访问模型及 host mock write-tearing/stop 测试，但真机访问顺序仍需标注待验证。

### 提交

- `[ref] bind 2K1000 DMA resources to executor state`

## 2026-08-10：批次 34——APBDMA order-register MMIO backend

### 任务与设计

1. 核对 2K1000 APBDMA order window 的寄存器宽度与 Linux 上游访问顺序。
2. 将 8-byte order register 建模为 low-then-high 的非原子 64-bit MMIO，而不是假定存在原生 64-bit 总线访问。
3. 在寄存器访问边界加入保守的 compiler/CPU 顺序约束，并保持可注入的 host mock。
4. 由已验证的 DMA topology 构造真实 volatile backend，拒绝零地址、错位和错误窗口大小。
5. 对半写失败保留可观察状态，明确交由上层 stop/quiesce 恢复，而不伪装成原子事务。

Linux `loongson2-apb-dma.c` 使用 `lo_hi_readq`/`lo_hi_writeq` 访问 order register；对应
helper 以两个 32-bit MMIO 操作先低后高完成读写。启动序列先写零，再写 descriptor 地址与
64-bit enable/start 位。本实现只依据这些行为事实建立独立抽象，没有复制上游实现代码；
参考文件同时记录了 APBDMA 驱动 GPL-2.0-or-later 与 helper GPL-2.0 的许可证边界。

`LoHiOrderIo<M>` 把任意 32-bit MMIO backend 适配为现有 executor 的 `OrderIo`。目标平台上的
`VolatileOrderMmio32` 仅接受 topology 已确认的 8-byte、4-byte 对齐、非零 order window，
并只执行 low/high 两次 volatile u32 访问。访问前后使用 `SeqCst` fence 作为当前保守边界；
它不替代未来需在真机确认的 LoongArch device-memory 属性和平台 I/O barrier。

### 完成内容

- [x] 新增 `apbdma_mmio` 模块及可测试的 `OrderMmio32` 接口。
- [x] `LoHiOrderIo` 以 little-endian 低 32 位、高 32 位顺序重组和拆分 64-bit order value。
- [x] read/write 两侧加入 `SeqCst` fence，避免普通内存与寄存器序列被软件重排。
- [x] LoongArch target 新增 raw volatile u32 backend；host 构建不暴露或编译裸 MMIO 构造。
- [x] backend 构造复用 `DmaControllerDescription`，严格检查 8-byte window、非零基址和 4-byte 对齐。
- [x] topology 验证同步拒绝错位 APBDMA order window，并新增 DTS fixture。
- [x] mock 测试断言读写均为 low-then-high，且不会退化为假定原子性的 native u64 访问。
- [x] mock 注入 high-half write 失败，验证错误向上传递且已发生的 low-half 写入保持可观察。
- [x] 本地上游参考补充 order register 访问顺序、启动序列与许可证信息。

### 验证证据

- `cargo test -p wateros-driver-impl-loongson2k1000la`：30 项 host 单测全部通过（本批新增 3 项）。
- 新测试覆盖 low/high read 重组、low/high write 顺序和 high-half 失败后的部分写状态。
- `cargo check -p wateros-driver-impl-loongson2k1000la --target loongarch64-unknown-none` 通过，并实际编译 raw volatile backend。
- `tests/verify_topology.sh` 全部通过，新增错位 DMA MMIO fixture 被正确拒绝；truncated fixture 的 dtc warning 为预期畸形输入。
- `make kernel-la` QEMU LoongArch64 release 回归构建通过；仅有仓库既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：尚未确认 order window 在内核地址空间可直接恒等访问、实际 endian/device-memory 属性及总线行为。
- [ ] `SeqCst` fence 是否足以形成 2K1000LA 所需 I/O ordering 尚未真机验证；未来可能需要架构专用 barrier。
- [ ] low-half 成功而 high-half 失败不是可回滚事务；当前 executor 会返还 prepared session，但真实恢复路径必须先写 stop 并确认 quiesce 后才能回收资源。
- [ ] 尚未验证写零、descriptor/start 写入以及 stop 序列在硬件上的 tearing、完成时机与错误状态。
- [ ] volatile backend 尚未接入 `init_after_boot` 或生产 executor 构造；仍缺真实 clock enable、IRQ13 dispatch/ack 和 cache maintenance backend。
- [ ] 尚未读取 descriptor completion/error status，也未连接 MMC data command，不能宣称真实传输可用。
- [ ] 下一批应先实现可审计的 executor/platform 组装入口和失败后的强制 stop 状态机，同时继续把 cache/IRQ 能力保留为显式依赖。

### 提交

- `[feat] add 2K1000 APBDMA order MMIO`

## 2026-08-10：批次 35——APBDMA partial-start recovery typestate

### 任务与设计

1. 审计 executor、typestate session、order-register backend 与 topology 的生产连接缺口。
2. 区分“寄存器未被触碰”和“可能已有部分写入”的 MMIO 错误效果。
3. 若 start 写可能已到达硬件，禁止返回可直接 cancel 的 prepared session。
4. 用 recovery typestate 独占 executor 与 DMA mappings，强制 stop 成功后才能恢复 CPU ownership。
5. 从 topology 已验证的 DMA controller 描述构造真实 volatile executor，不绕过资源校验。

APBDMA start 位位于 order register 的低 32 位。low-then-high 写入若在 high-half 失败，
低半部可能已带着 start 位到达设备，因此原有“返回 prepared token、允许 cancel”的行为会在
硬件可能仍访问内存时恢复 CPU ownership。`OrderWriteFailure` 现在携带 `WriteEffect`：只有
`Untouched` 才能安全返回 `PreparedSession`；`MayHaveWritten` 会把 executor 标记为占用并返回
`RecoverySession`。recovery session 不提供 cancel 或 IRQ completion，只能反复尝试 stop；
stop 成功后才转为 `QuiescedSession`，再执行 cache ownership 恢复。

`OrderMmio32` 的错误契约明确为单次 32-bit 写失败时该写没有到达设备。因此 low-half 失败可
报告 `Untouched`，high-half 失败必须报告 `MayHaveWritten`。其它 `OrderIo` 实现也必须显式
报告写入效果，不能把未知硬件状态压缩成普通寄存器错误。

### 完成内容

- [x] 新增 `WriteEffect::{Untouched, MayHaveWritten}` 与带效果的 `OrderWriteFailure`。
- [x] `LoHiOrderIo` 精确报告 low-half 失败未触碰、high-half 失败可能已部分写入。
- [x] 新增 `StartSessionFailure::{Prepared, Recovery}`，调用方必须显式处理两类失败状态。
- [x] 新增借用式 `RecoverySession`，同时独占 executor、descriptor mapping 和 payload mapping。
- [x] 可能部分写入时 executor 保留 transfer plan，阻止第二个 transfer 启动。
- [x] recovery stop 失败返回原 recovery session，可重试且不会提前释放 mappings。
- [x] stop 成功才清除 executor 占用状态并产生 quiesced session。
- [x] executor 的裸 start/complete/stop 收紧为 crate 内原语，对外保留 typestate 路径。
- [x] 新增 target-only `PlatformExecutor` 和 `executor_from_controller` 生产组装入口。
- [x] 组装入口复用 topology 校验后的 8-byte volatile MMIO backend，并明确要求外部保持 clock 与 IRQ 能力。

### 验证证据

- `cargo test -p wateros-driver-impl-loongson2k1000la`：33 项 host 单测全部通过（本批新增 3 项）。
- 新测试覆盖未触碰失败可取消、partial-start 必须 stop、stop 首次失败后 recovery 重试成功。
- fault mock 保存可能写入的寄存器值，并断言 clear、start、stop 的完整顺序。
- mappings 在 recovery session 生命周期内保持 device-owned；只有 stop 和 finish 完成后恢复 CPU ownership。
- `cargo check -p wateros-driver-impl-loongson2k1000la --target loongarch64-unknown-none` 通过，并实际编译生产组装入口。
- 2K1000LA topology/畸形 DTS fixtures 全部通过；truncated DMA 的 dtc warning 为预期输入。
- `make kernel-la` QEMU LoongArch64 release 回归构建通过；仅有仓库既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：软件认为 stop 写成功尚不能证明 APBDMA 已停止总线访问；仍需确认 stop bit、自清除/完成状态和必要轮询。
- [ ] 当前 `QuiescedSession` 信任 stop 写成功，尚未读取 order register 或 descriptor status 二次确认 idle。
- [ ] `OrderMmio32` 的失败效果契约由 backend 实现保证；raw volatile backend 当前只有地址校验错误，不能捕获同步总线异常。
- [ ] `executor_from_controller` 只组装 order MMIO；clock enable、DMA route、IRQ13 claim/ack、cache maintenance 仍是缺失的显式能力。
- [ ] 生产 executor 尚未存入平台 driver state，也未接入 `init_after_boot`，避免在能力不完整时误触碰真机寄存器。
- [ ] descriptor completion/error status、超时、短传输以及 MMC data command 仍未实现。
- [ ] 下一批应实现带有有界轮询的 stop-confirmation backend，并让 quiesced 仅由确认 idle 的结果产生；若文档不足则继续保持保守的 `UNVERIFIED_ON_HARDWARE` 标记。

### 提交

- `[fix] recover partial 2K1000 APBDMA starts`

## 2026-08-10：批次 36——APBDMA bounded stop confirmation

### 任务与设计

1. 核对上游 stop 编码以及是否存在可作为 idle 证据的 order/descriptor 位。
2. 将 stop 写入与“硬件已静止”确认拆分，禁止写成功直接产生 quiesced 状态。
3. 用显式有限预算轮询可注入 confirmation 探针，避免无限等待。
4. 超时、探针错误或证据缺失时返回原 running/recovery session，继续隔离 DMA mappings。
5. raw volatile backend 在没有板级证据时保守拒绝确认，不根据未知位伪造 idle。

Linux 上游的 terminate、pause 和 final-IRQ 路径会保留 descriptor 地址位并写入
`64BIT_EN | STOP`，但随后只更新软件状态；没有轮询 START/STOP、自清除位或 descriptor
status 来证明控制器已经停止。因此本批没有把任何寄存器位硬编码为 idle 条件。

`OrderIo::confirm_stopped()` 是显式证据边界。executor 写 STOP 后最多调用配置的 poll limit
次；只有探针返回 `true` 才清除 active plan 并产生 `Completion`。预算耗尽返回
`StopTimeout`，平台无可靠探针返回 `StopUnverified`，I/O 失败保留原错误。所有失败路径
都不清除 executor 状态，typestate session 继续独占 mappings，可安全重试。

### 完成内容

- [x] 抽出 `ORDER_STOP`，新增默认 1024 次的有限 stop poll budget。
- [x] 新增 `Executor::with_stop_poll_limit`，拒绝零预算。
- [x] `ExecutorError` 新增 `InvalidPollLimit`、`StopTimeout` 和 `StopUnverified`。
- [x] `OrderIo` 新增平台 stop-confirmation 契约。
- [x] executor 在 STOP 写成功后有界轮询，确认成功前不清除 active transfer。
- [x] running 与 partial-start recovery 共用相同确认路径；错误均返回原 session。
- [x] `OrderMmio32` 提供保守默认 confirmation：无平台证据时返回 `StopUnverified`。
- [x] `LoHiOrderIo` 转发 confirmation，不把 order register 读值解释成未经证明的 idle 位。
- [x] 本地上游参考补充 Linux stop 路径没有硬件 idle 轮询的事实。

### 验证证据

- `cargo test -p wateros-driver-impl-loongson2k1000la`：38 项 host 单测全部通过（本批新增 5 项）。
- 延迟确认测试在前两次 false、第三次 true 时才恢复 CPU ownership，并断言恰好轮询 3 次。
- timeout 测试在 2 次预算耗尽后返回原 running session，重试确认后才 finish。
- confirmation I/O 错误测试从 recovery session 返回原状态，第二次 stop 成功恢复。
- 零 poll budget 被拒绝；默认 raw/MMIO mock 在无平台证据时返回 `StopUnverified`。
- `cargo check -p wateros-driver-impl-loongson2k1000la --target loongarch64-unknown-none` 通过。
- 2K1000LA topology/畸形 DTS fixtures 全部通过；truncated DMA 的 dtc warning 为预期输入。
- `make kernel-la` QEMU LoongArch64 release 回归构建通过；仅有仓库既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：目前没有已证明可用的 2K1000LA stop-confirmation 探针，因此 raw backend 会返回 `StopUnverified`，不会允许真实 mappings 被回收。
- [ ] Linux 上游写 STOP 后不轮询不是硬件同步完成的证明；WaterOS 有意采用更保守的 ownership 边界。
- [ ] 默认 1024 是调用次数预算，不是时间单位；未来探针需要结合平台 timer/relax 明确最大墙钟时间。
- [ ] IRQ completion 路径仍信任“IRQ 已 claim/ack”的调用前提，尚未检查 descriptor status、错误或短传输。
- [ ] descriptor status 的位定义及 cache visibility 尚无可靠本地资料，不能用于 stop confirmation。
- [ ] clock、DMA route、IRQ13 和 cache maintenance 仍未接入生产 platform state。
- [ ] 真机 bring-up 前必须根据芯片手册或板级实验实现 confirmation：例如 STOP 后可读状态、总线 idle 或平台 reset/clock gate 保证；完成前保持 `StopUnverified`。
- [ ] 下一批可优先实现 APBDMA IRQ13 的 claim/ack 与 descriptor status 读取抽象；即便状态位语义未知，也可先完成可 mock 的内存可见性和错误分类边界。

### 提交

- `[fix] require 2K1000 APBDMA stop confirmation`

## 2026-08-10：批次 37——APBDMA IRQ and descriptor-status evidence

### 任务与设计

1. 审计 LIOINTC claim/ack、APBDMA IRQ completion 与 DMA mapping ownership 顺序。
2. 将 IRQ source 与 executor 显式绑定，错 IRQ 不得完成 active transfer。
3. 在 descriptor status 读取前恢复 descriptor 的 CPU cache visibility。
4. 把 IRQ 到达、descriptor 状态分类和资源回收拆成独立 typestate 阶段。
5. 状态位没有可靠定义时返回 `StatusUnverified`，不把任意数值解释成成功。

`LioIntc::mask_ack_claim()` 在写 ENABLE_CLEAR 后产生线性的 `AcknowledgedIrq`，token 绑定
global bank/local source，且不实现 `Copy`。每个 executor 保存预期 IRQ；只有 token 与绑定
完全相同才会结束 running 状态并产生 `IrqCompletionSession`。错 IRQ 返回原 running session，
不会丢失 stop/retry 能力。

IRQ completion 不再直接等于传输成功。`IrqCompletionSession::inspect_status()` 先对 descriptor
mapping 执行 device-to-CPU sync，再把获得 CPU ownership 的 region 交给 status reader；decoder
随后分类 `Complete`、`HardwareError` 或错误。descriptor 同时包含 CPU 写入命令和设备回写
status，因此本批把其 DMA direction 从错误的 `ToDevice` 修正为 `Bidirectional`。

Linux 上游声明了 descriptor `stats` word，但没有定义位语义，ISR 也不检查它。因此
`UnverifiedStatusDecoder` 一律返回 `StatusUnverified`。无状态证明的 cleanup 入口被标为
`unsafe reclaim_unverified`，调用方必须另有平台证据证明该 IRQ 已停止 DMA 总线访问。

### 完成内容

- [x] 新增非 `Copy` 的 `AcknowledgedIrq`，只能由 crate 内 LIOINTC mask/ack 路径构造。
- [x] 新增 `LioIntc::mask_ack_claim(bank, local)`，无效 bank/local 不执行 MMIO 写。
- [x] executor 构造必须绑定 `GlobalIrq`；生产 factory 同样要求显式 IRQ binding。
- [x] APBDMA completion 拒绝不匹配 source，并返回原 running session。
- [x] 新增 `IrqCompletionSession`，分离 IRQ 静止事件和 descriptor 状态验证。
- [x] 新增 `DescriptorStatusReader`、`DescriptorStatusDecoder` 及 completion/error 分类。
- [x] target-only volatile reader 按 `offset_of!(HardwareDescriptor, status)` 读取 status word。
- [x] status reader 仅在 descriptor CPU sync 成功后调用；sync/read/decode 失败返回原 session。
- [x] 新增保守 `UnverifiedStatusDecoder`，未知平台不宣称成功。
- [x] `reclaim_unverified` 收紧为 unsafe，要求调用方承担硬件停止证明。
- [x] descriptor mapping 全路径改为 `Bidirectional`，覆盖命令发布和 status 回写。
- [x] 本地上游参考补充 stats 未定义/未由 ISR 检查的事实。

### 验证证据

- `cargo test -p wateros-driver-impl-loongson2k1000la`：42 项 host 单测全部通过（本批新增 4 项）。
- LIOINTC 测试断言 bank1/local13 token 与 ENABLE_CLEAR 写序，无效 bank 不写寄存器。
- 错 IRQ 测试断言 `UnexpectedIrq` 并保留 running session，随后正确 IRQ 可继续完成。
- cache fault 测试在 descriptor 首次 CPU sync 失败时断言 status reader 零调用；重试同步后才读取一次。
- fixture decoder 独立覆盖 Complete、HardwareError 和 Unknown；生产默认 decoder 覆盖 `StatusUnverified`。
- 测试中的 unsafe cleanup 均附带 mock IRQ 保证的 SAFETY 注释。
- `cargo check -p wateros-driver-impl-loongson2k1000la --target loongarch64-unknown-none` 通过，并编译 volatile status reader 与生产 executor factory。
- 2K1000LA topology/畸形 DTS fixtures 全部通过；truncated DMA 的 dtc warning 为预期输入。
- `make kernel-la` QEMU LoongArch64 release 回归构建通过；仅有仓库既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：APBDMA IRQ 是否只在总线访问完全结束后触发尚未验证；安全代码不能使用 unverified cleanup。
- [ ] `mask_ack_claim` 只证明 LIOINTC source 被 mask/清 latch；level-triggered APBDMA condition 仍需设备侧 ack/clear 语义。
- [ ] descriptor status 位无公开可靠定义，生产 decoder 当前故意不可用；fixture 位值仅用于测试状态机，不代表硬件。
- [ ] volatile status 读取依赖真实 coherency backend 正确处理 `Bidirectional` descriptor cache line。
- [ ] APBDMA IRQ13 尚未注册到运行时 trap glue，executor 也未存入全局 platform driver state。
- [ ] IRQ token 与 topology interrupt spec 到 global bank 的映射仍由未来组装层显式提供，不能仅凭 phandle 猜测 bank。
- [ ] payload completion、MMC data command、硬件错误传播和 timeout/cancel 仍未连成真实 block I/O。
- [ ] 下一批应实现 topology/LIOINTC bank binding 的可验证解析与 IRQ lifecycle（route、trigger、enable、mask、设备 ack、re-enable）模型，再考虑接入 runtime trap glue。

### 提交

- `[ref] verify 2K1000 APBDMA IRQ completion`

## 2026-08-10：批次 38——LIOINTC topology binding and IRQ lifecycle

### 任务与设计

1. 审计 DTS 中 LIOINTC phandle、bank、`#interrupt-cells` 与 APBDMA IRQ13 的关系。
2. 建立不依赖 DT 节点遍历顺序的 global IRQ 解析规则。
3. 串联 route、trigger、enable、mask/ack、设备侧 clear 与 re-enable 生命周期。
4. 用线性 typestate 阻止未完成设备 ack 时重新开中断线。
5. 将 topology binding 接入 APBDMA 生产 executor factory 与真实 DTS fixture。

global bank 按 LIOINTC main MMIO 基址升序分配，而不是按 discovery vector 顺序：
`0x1fe01400` 为 bank0，`0x1fe01440` 为 bank1。resolver 必须精确匹配 interrupt-parent
phandle，拒绝缺失/重复 phandle、重复 MMIO、非 2-cell spec、非法 trigger 和超出 bank 上限；
因此 fixture 中 APBDMA `<13 4>` 稳定解析为 bank1/local13/level-high。

生命周期为 `InterruptBinding -> ArmedInterrupt -> MaskedInterrupt ->
DeviceAckedInterrupt -> ArmedInterrupt`。claim 产生独立 `AcknowledgedIrq` 证据并消耗 armed
状态；设备 clear 失败会返回原 masked 状态以便重试。只有 `DeviceIrqAck::clear_interrupt()`
成功后才可 rearm，避免 level IRQ 在设备条件未清除时立即重入。

### 完成内容

- [x] 新增 `irq_binding` 模块与稳定 topology resolver。
- [x] resolver 用 provider phandle 确认归属，用 main-MMIO 排序计算 bank，不受节点顺序影响。
- [x] 支持 edge-rising、edge-falling、level-high、level-low 四种 DT trigger 编码。
- [x] `LioIntc` 实例显式绑定 bank，跨 bank 的 mask/ack 与 lifecycle 操作会被拒绝。
- [x] 新增可恢复的 `LifecycleFailure<S>`，任何失败都返还原 typestate。
- [x] 新增显式 `DeviceIrqAck` 契约；未成功 clear 不能获得 re-enable 能力。
- [x] APBDMA target factory 改为接收已解析的 `InterruptBinding`，并复核 provider/local IRQ。
- [x] topology fixture 验证 APBDMA 解析结果固定为 bank1/local13。
- [x] 单测覆盖 controller discovery 顺序交换、歧义/非法 spec、设备 ack 失败重试和 re-enable 写序。

### 验证证据

- `cargo test`（2K1000LA driver crate）：45 项 host 单测全部通过（本批新增 3 项）。
- 顺序稳定性测试用相同 phandle/MMIO 但相反 controller vector 顺序，均解析为 bank1/local13。
- 生命周期测试断言首次 device clear 失败后没有 ENABLE_SET 写；重试成功后仅 rearm 一次。
- `tests/verify_topology.sh` 全部通过；truncated DMA 的 dtc warning 为预期畸形输入。
- `cargo check --target loongarch64-unknown-none` 通过，并编译 binding 驱动的生产 factory。
- `make kernel-la` QEMU LoongArch64 release 回归构建通过；仅有仓库既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：LIOINTC route/trigger/enable 寄存器与 mask/ack 写序尚未在 2K1000LA 真机验证。
- [ ] `UNVERIFIED_ON_HARDWARE`：APBDMA 的设备侧 IRQ clear 寄存器/语义仍未知；本批只定义了必须实现的契约。
- [ ] bank 规则目前依据已审计的两个 2K1000LA LIOINTC main-MMIO 窗口；真机 DTB 必须继续通过 fixture 等价检查。
- [ ] typestate 防止安全 Rust 重复 claim/re-enable，但 runtime trap glue 尚未持有和调度这些状态。
- [ ] `AcknowledgedIrq` 证明 LIOINTC latch 已 mask/ack，不证明 APBDMA 已停止访问 DMA memory。
- [ ] 生产 executor 尚未接入平台全局 driver state；clock、DMA route 与 IRQ handler 注册仍待完成。
- [ ] 下一批应设计 runtime IRQ registration/dispatch glue，使 APBDMA binding、LIOINTC controller 与 executor session 在并发边界上有唯一所有者；在设备 clear 语义获得证据前继续保持硬件路径禁用。

### 提交

- `[ref] bind 2K1000 LIOINTC IRQ lifecycle`

## 2026-08-10：批次 39——IRQ dispatch ownership boundary

### 任务与设计

审计发现组合层 trap 当前只处理 LoongArch64 software/timer interrupt；2K1000LA 外部中断
cause/CSR 路径尚未接入。原 `dispatch_bank(pending)` 仅凭 snapshot 调用 `fn(GlobalIrq)`，会绕过
LIOINTC mask/ack 证据。本批先收紧 crate 内边界：handler 必须消费非 `Copy` 的
`AcknowledgedIrq`；未注册或 inactive-bank 时原 token 必须返还，保持 source masked 供上层诊断。

### 完成内容与验证

- [x] `IrqHandler` 改为 `fn(AcknowledgedIrq)`，删除裸 pending snapshot 分发入口。
- [x] 新增 `UnhandledIrq`，同时返回错误与未消费的线性 token。
- [x] 注册仍以 `GlobalIrq` 为唯一键并拒绝重复 owner。
- [x] 单测覆盖已注册消费、未注册返还 token、inactive bank 返还 token。
- `cargo test`：45 项 host 单测全部通过。
- topology/畸形 DTS fixtures、LoongArch64 target check、`make kernel-la` 全部通过；仅有既有 warning。
- `git diff --check` 通过。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：LoongArch64 外部中断 cause、ESTAT/ECFG 位与 LIOINTC parent line 尚未用真机确认。
- [ ] kernel trap 尚无 external IRQ 分支；在可靠 CSR/手册证据前不猜测编号或清 pending 顺序。
- [ ] function-pointer handler 只建立 token ownership 边界，APBDMA session 的并发容器仍待设计。
- [ ] device-side clear 未知，因此 APBDMA handler 仍不得自动 re-enable。
- [ ] 下一批应从本地 LoongArch64 arch 实现和可核验资料建立 external IRQ cause 抽象及纯 mock trap adapter，再决定 target glue。

### 提交

- `[ref] require acknowledged IRQ dispatch`

## 2026-08-10：批次 40——LoongArch64 HWI decode and LIOINTC parent binding

### 任务与设计

1. 审计 LoongArch64 ESTAT/ECFG、WaterOS `TrapCause` 与 2K1000LA LIOINTC parent IRQ。
2. 修复 HWI0–HWI7 未被架构 trap decoder 识别的问题。
3. 将 CPU hardware line 与 topology LIOINTC bank 做稳定、可拒绝歧义的绑定。
4. 用编译期断言、host 单测和真实 DTS fixture 覆盖无物理机验证。

LoongArch64 ESTAT.IS 的 HWI0–HWI7 位于 bits 2–9；IPI 和 timer 继续保持现有优先级，
随后才把任一 HWI pending 解码为跨架构 `SupervisiorExternel`。平台 parent-line resolver
只接受 HWI 0..7，必须在 topology 中精确匹配一个 controller，并继续按 main-MMIO 升序
计算 bank，避免 discovery 顺序影响。

### 完成内容

- [x] LoongArch64 trap decoder 新增 HWI0–HWI7 mask，外部硬件中断不再误落为 `Unsupported(0)`。
- [x] decoder 拆出 const 纯函数，并编译期验证 HWI0/HWI7 边界及 IPI-over-HWI 优先级。
- [x] 新增 `irq_entry::resolve_parent_line()` 与 `ParentLineBinding`。
- [x] resolver 拒绝非法 HWI、缺失 controller、重复匹配、重复 MMIO 和过多 bank。
- [x] host 单测验证 controller vector 乱序时 HWI3 仍稳定解析为 bank1。
- [x] 真实 topology fixture 验证 HWI2→bank0、HWI3→bank1。

### 验证证据

- `cargo test`（2K1000LA driver crate）：47 项 host 单测全部通过（本批新增 2 项）。
- LoongArch64 decoder 的 HWI bit 边界与优先级由 target 构建必经的 const assertions 验证。
- topology/畸形 DTS fixtures 全部通过；truncated DMA 的 dtc warning 为预期输入。
- `cargo check --target loongarch64-unknown-none` 通过。
- `make kernel-la` QEMU LoongArch64 release 回归构建通过；仅有仓库既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：真实 2K1000LA ESTAT HWI pending、ECFG enable 和 parent-line 电气路由尚未验证。
- [ ] `SupervisiorExternel` 目前只表达“至少一个 HWI pending”，不携带具体 HWI index；kernel handler 仍需读取可验证 snapshot。
- [ ] kernel trap 尚未调用 board IRQ service；当前修改只保证不会把 external IRQ 当成同步异常。
- [ ] 多个 HWI 同时 pending 时需要 snapshot/优先级 adapter，不能仅由语义枚举推断来源。
- [ ] LIOINTC volatile claim、mask/ack 和 device clear/re-enable 仍保持真机未验证。
- [ ] 下一批应在 arch API 增加可读取的 LoongArch interrupt-pending snapshot 契约，并由 2K1000LA machine driver 提供 board IRQ service；QEMU profile 必须保持无行为变化。

### 提交

- `[fix] decode LoongArch hardware interrupts`

## 2026-08-10：批次 41——External IRQ snapshot contract

### 任务与设计

建立 trap-entry snapshot 到 machine driver 的跨层契约，但在 2K1000LA runtime controller
尚未持久化前不接 kernel dispatch。snapshot 必须来自已保存 trap frame，避免 handler 二次读取
ESTAT 引入竞态；其他架构通过默认 `None` 保持行为不变。machine service 默认返回
`Unsupported`，实现方只有在 acknowledge 或 mask 每个消费 source 后才能返回成功。

### 完成内容

- [x] `TrapFrameRead` 新增默认 `external_interrupt_snapshot() -> Option<usize>`。
- [x] LoongArch64 从已保存 ESTAT 提取 HWI0–HWI7 八位 pending snapshot，零 pending 返回 `None`。
- [x] `MachineDriver` 新增默认 fail-closed `handle_external_interrupt(snapshot)`。
- [x] driver 聚合层导出 machine service 路由函数。
- [x] 2K1000LA service 验证 snapshot 范围；runtime 未建立前明确返回 `Unsupported`。
- [x] 未把 kernel external trap 接到失败 service，避免未清 pending 导致中断风暴。

### 验证证据

- `cargo test`（2K1000LA driver crate）：48 项 host 单测全部通过（本批新增 1 项）。
- 测试验证 zero/out-of-range snapshot 为 `InvalidParam`，合法 HWI snapshot 在未初始化时为 `Unsupported`。
- topology/畸形 DTS fixtures 全部通过；truncated DMA dtc warning 为预期输入。
- `cargo check --target loongarch64-unknown-none` 与 `make kernel-la` 通过；仅有既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：trap frame 保存的 ESTAT HWI snapshot 与真实 parent-line 到达时序尚未验证。
- [ ] 2K1000LA machine service 当前故意不可用，kernel 也尚未调用它。
- [ ] topology 与 LIOINTC controller 尚未组成静态 runtime state；init 后发现结果当前会被丢弃。
- [ ] snapshot 只表示 CPU HWI parent line，仍必须读取对应 LIOINTC per-core ISR 才能得到 local source。
- [ ] 下一批应实现一次初始化、不可重复替换的 board IRQ runtime，持有 topology-derived bank descriptors、domain 和 controller MMIO；先用 mock backend 测试 snapshot 多 bit、claim、未注册 mask 保留，再接 kernel trap。

### 提交

- `[ref] add external IRQ snapshot contract`

## 2026-08-10：批次 42——Allocation-free board IRQ runtime core

### 任务与设计

实现固定容量、无热路径分配的 `BoardIrqRuntime<I>`，把 HWI snapshot 展开为 parent line，
映射到稳定 bank，读取该 core 的 enabled-pending snapshot，并对每个 local source 严格执行
mask/ack 后再 dispatch。两个 controller 槽、八条 HWI 映射和 64 项 domain 均固定容量；handler
注册要求初始化期独占 `&mut self`。

service 对每个 bank 只读取一次 pending snapshot。未注册 source 的线性 token 不交给 handler，
但 source 已被 ENABLE_CLEAR mask，报告为 unhandled。后续 parent line 失败时返回累计 report，
不回滚已经发生的硬件写，也不伪装成原子事务。

### 完成内容

- [x] `LioIntc` 新增 `pending_enabled(core)`，统一 pending 与 enable snapshot 读取。
- [x] 新增泛型 `BoardIrqRuntime<I>`、`RuntimeError`、`ServiceReport` 和 `ServiceFailure`。
- [x] runtime 构造验证 controller 槽位与 bank identity、parent map 引用完整性。
- [x] registration 继续复用唯一 `GlobalIrq` domain，拒绝重复 owner。
- [x] snapshot service 支持多 parent line、多 local source 和同 bank 去重。
- [x] 每个 local source 先 `mask_ack_claim`，然后才把非 `Copy` token 交给 handler。
- [x] 未注册 source 保持 masked，并计入诊断报告。
- [x] 后续 source 失败返回 partial report，准确反映不可回滚副作用。

### 验证证据

- `cargo test`：50 项 host 单测全部通过（本批新增 2 项）。
- 多 parent 测试同时服务 HWI2/HWI3，断言 3 次 ENABLE_CLEAR、2 handled、1 unhandled。
- partial-failure 测试先 mask/ack HWI2 local source，再遇到未映射 HWI4，报告保留此前计数。
- topology/畸形 DTS fixtures、LoongArch64 target check、`make kernel-la` 全部通过；仅有既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：runtime 目前仅使用 mock `RegisterIo`，未发布 volatile controller 全局实例。
- [ ] service 未持有设备侧 ack/re-enable 状态；已处理 source 默认继续 masked，这是有意的安全策略。
- [ ] handler 为 function pointer；APBDMA session 的可变 owner 容器尚未接入。
- [ ] 全局 `TOPOLOGY` 仍可被重复覆盖，且不应在 IRQ 上下文持锁。
- [ ] HWI parent map 尚未从 topology 一次性编译成 runtime layout。
- [ ] 下一批应实现 topology→定长 `RuntimeLayout` 编译与一次发布容器，验证重复 init、缺失 bank、重复 parent line；volatile MMIO 发布仍需明确 unsafe 映射前提。

### 提交

- `[ref] add 2K1000 IRQ runtime core`

## 2026-08-10：批次 43——Topology-compiled IRQ runtime layout

### 任务与设计

把动态、可分配的 `BoardTopology` 编译为不借用 DTB 的定长 `RuntimeLayout`。layout 固定保存
两个按 main-MMIO 排序的 controller、每个 controller 最多四个 per-core ISR 地址，以及八条
HWI→bank 映射。编译必须全部成功后才发布；发布槽只能写一次，后续初始化不得覆盖旧状态。

IRQ layout 和诊断 topology 在 `init_after_boot()` 中先局部构造并验证，再同时写入各自容器。
本批仍不在中断热路径获取这些锁，也不创建 volatile MMIO controller。

### 完成内容

- [x] 新增 `ControllerLayout`、`RuntimeLayout`、`LayoutError` 和 `RuntimeLayoutSlot`。
- [x] 严格要求两个 LIOINTC，按 main-MMIO 地址而非 discovery 顺序确定 bank。
- [x] 验证 main MMIO 非零/对齐/尺寸、core ISR 数量/对齐/尺寸。
- [x] 验证每个 controller 至少有一个 CPU parent line，cell 必须是 HWI0–HWI7。
- [x] 拒绝重复 main MMIO、重复 parent line、缺失 controller 与非法资源。
- [x] 一次发布槽拒绝 replacement，并保持原 layout 不变。
- [x] `init_after_boot()` 编译并发布 layout；重复初始化返回 `InvalidParam`，不再覆盖 topology。
- [x] fixture verifier 直接检查编译后的 bank bases 与 HWI2/HWI3 映射。

### 验证证据

- `cargo test`：52 项 host 单测全部通过（本批新增 2 项）。
- 乱序 controller 测试生成完全相同的 layout；单 controller 和重复 parent line 均被拒绝。
- replacement publish 返回 `AlreadyPublished`，`get()` 仍返回第一次发布值。
- topology/畸形 DTS fixtures、LoongArch64 target check、`make kernel-la` 全部通过；仅有既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：layout 中物理地址尚未用于 volatile MMIO，真实映射可访问性未知。
- [ ] topology/layout 容器仍为 spin mutex；只允许初始化/诊断使用，IRQ 热路径不得持锁。
- [ ] 当前发布不是跨两个 mutex 的通用并发事务；boot 初始化按单线程前提调用。
- [ ] runtime layout 已定长，但 `BoardIrqRuntime<VolatileMmio>` 尚未一次构造和发布。
- [ ] 所有设备 source 尚未注册，且 device ack/re-enable 仍未实现。
- [ ] 下一批应实现 target-only unsafe runtime assembler：从 layout 创建 bank-identity 正确的 `LioIntc<VolatileMmio>`，在发布前保持所有 source disabled；用 mock assembler 验证失败不发布、地址逐项传递，再决定全局 runtime 容器。

### 提交

- `[ref] compile 2K1000 IRQ runtime layout`

## 2026-08-10：批次 44——IRQ runtime assembler and initial masking

### 任务与设计

从已验证 `RuntimeLayout` 组装两个 bank identity 固定的 `LioIntc<I>`。assembler 按 bank0、
bank1 调用 I/O factory，构造每个 controller 后立即执行单次 `ENABLE_CLEAR = 0xffffffff`；
只有两者全部成功后才创建 domain 并返回 `BoardIrqRuntime`。任何 factory/controller 失败都不
产出半初始化 runtime。

target-only volatile assembler 被标为 unsafe：调用者必须证明所有 main/core ISR 物理地址已
映射且由该 driver 独占，因为函数会立即写两个 controller。本批没有从启动路径调用它。

### 完成内容

- [x] `LioIntc::mask_all()` 用一次 ENABLE_CLEAR 写屏蔽 32 个 local source。
- [x] 新增 `BoardIrqRuntime::assemble(layout, make_io)` 泛型 assembler。
- [x] core ISR 定长地址按 layout 原样传递，controller bank identity 由数组位置确定。
- [x] 每个 controller 在 runtime 可见前全部 source masked。
- [x] 第二 bank factory 失败时返回错误，不创建 domain/runtime。
- [x] 新增 target-only `unsafe assemble_volatile()` 与完整 Safety/真机未验证说明。
- [x] 源码搜索确认 volatile assembler 当前没有生产调用者。

### 验证证据

- `cargo test`：54 项 host 单测全部通过（本批新增 2 项）。
- assembler 测试断言 factory 顺序为 `(bank0,0x1000)`、`(bank1,0x1040)`。
- 成功 runtime 拆解后，每个 mock controller 恰有一条 `base+0x2c = 0xffffffff` 写。
- 第二 bank 返回 `IoError` 时 assembler 返回对应 error，调用计数为 2 且无 runtime。
- topology/畸形 DTS fixtures、LoongArch64 target check、`make kernel-la` 全部通过；仅有既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：ENABLE_CLEAR 全掩码写及 physical MMIO 可访问性尚未在 2K1000LA 验证。
- [ ] assembler 失败前已对较早 bank 执行 mask-all；这是安全且不可回滚的硬件副作用，不是事务。
- [ ] volatile runtime 尚未全局发布，kernel external trap 仍未调用 machine service。
- [ ] source 注册、route/trigger/enable 必须在开放 CPU HWI 前完成；当前没有启动状态机保证该顺序。
- [ ] device-side ack/re-enable 与 APBDMA session owner 尚未接入。
- [ ] 下一批应实现 `Dormant → Configured → Live` 启动 typestate：Dormant 持有全 masked runtime；Configured 至少注册/arm 已证明 source；只有 Live 才可被 trap handler 访问。无已证明 device ack 的 APBDMA source 继续不得进入 Live。

### 提交

- `[ref] assemble masked 2K1000 IRQ runtime`

## 2026-08-10：批次 45——IRQ runtime startup typestate

### 任务与设计

建立 `DormantRuntime → ConfiguredRuntime → LiveRuntime` 启动状态机。Dormant 来自已全 mask
的 assembler；configure 必须完成唯一 handler 注册、route、trigger 与 local source enable，并
把 source 记录到固定 64-bit ownership mask。activate 根据已配置 source 所在 bank 计算所需
CPU parent HWI mask，只有显式 `CpuParentActivator` 成功后才产生 Live。

当前 handler 只消费 acknowledged token，尚不能返回 device-acked/re-enable 证据。因此 Live
采用安全的一次触发语义：service 会 mask/ack 并 dispatch，但不会自动 re-enable local source。

### 完成内容

- [x] 新增 Dormant、Configured、Live 三种不可互换的 runtime 类型。
- [x] 底层 `BoardIrqRuntime::service` 改为私有，只有 Live 对外提供 service。
- [x] Dormant configure 注册 handler 并执行 binding arm；失败注销 domain owner 并返还 Dormant。
- [x] Configured 可追加 source；重复 `GlobalIrq` 返回 `AlreadyRegistered` 并返还原状态。
- [x] configured source ownership 使用固定 u64，不分配内存。
- [x] activator 只收到已配置 bank 对应的 HWI mask。
- [x] activator 失败通过 `TransitionFailure<ConfiguredRuntime>` 返还完整状态，可安全重试。
- [x] Live 明确保持中断处理后 source masked，不虚构 device ack。

### 验证证据

- `cargo test`：55 项 host 单测全部通过（本批新增 1 项）。
- typestate 测试断言 mask-all 为第一条写，source ENABLE_SET 为配置阶段最后一条写。
- 重复 source 返回 `Domain(AlreadyRegistered)`；状态随后仍可 activate。
- activator 第一次 `IoError`、第二次成功，两次均收到仅 HWI2 的 `0x04` mask。
- topology/畸形 DTS fixtures、LoongArch64 target check、`make kernel-la` 全部通过；仅有既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：route/trigger/source enable 与 CPU parent enable 的真实写序尚未真机验证。
- [ ] `CpuParentActivator` 目前只有 mock；尚无 ECFG target backend，因此生产无法进入 Live。
- [ ] configured ownership 压缩为 bitmask；当前 source 首次 dispatch 后仍保留“已配置”诊断位，但硬件已 masked。
- [ ] handler 不能返回 device ack disposition，故不能安全循环 re-enable。
- [ ] volatile Live runtime 尚未发布，kernel external trap 仍未接入。
- [ ] 下一批应实现 handler disposition typestate：handler 接收 acknowledged/masked state，只能返回 `KeepMasked` 或具备设备 clear 证据的 `Rearm`；runtime 根据返回值决定是否 ENABLE_SET，并测试 ack 失败保持 masked。

### 提交

- `[ref] stage 2K1000 IRQ runtime activation`

## 2026-08-10：批次 46——Device-ack IRQ disposition

### 任务与设计

把 handler 契约升级为 `fn(AcknowledgedIrq) -> IrqDisposition`。handler 可以选择
`KeepMasked`，或返回绑定具体 `GlobalIrq` 的 `DeviceAckedIrq` 证据请求 `Rearm`。证据字段私有，
构造仅 crate 内可见，真实 handler 必须在设备侧 clear 成功后才构造。

runtime 在 handler 返回后核对证据 source identity；只有完全匹配当前 bank/local source 才写
ENABLE_SET。错 source、未注册或 KeepMasked 都不会重新开线。

### 完成内容

- [x] 新增 `DeviceAckedIrq` 与 `IrqDisposition::{KeepMasked, Rearm}`。
- [x] `IrqHandler` 返回 disposition，domain 只负责所有权转交，不自行操作 controller。
- [x] `ServiceReport` 新增 `rearmed_sources`。
- [x] runtime 对 Rearm evidence 重新构造期望 GlobalIrq 并精确核对。
- [x] 匹配证据后执行 ENABLE_SET；错 source 返回 `DispositionMismatch` 和 partial report。
- [x] KeepMasked 与未注册 source 继续保持 masked。
- [x] APBDMA 没有 device clear 证据，生产路径仍不能构造 Rearm。

### 验证证据

- `cargo test`：56 项 host 单测全部通过（本批新增 1 项）。
- KeepMasked 既有测试断言无 ENABLE_SET，rearmed 计数为 0。
- 匹配 evidence 测试断言写序严格为 ENABLE_CLEAR 后 ENABLE_SET，rearmed=1。
- 错 source evidence 测试返回 `DispositionMismatch`，handled=1、rearmed=0，只有 ENABLE_CLEAR 写。
- topology/畸形 DTS fixtures、LoongArch64 target check、`make kernel-la` 全部通过；仅有既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：设备 clear 完成到 LIOINTC ENABLE_SET 的真实同步要求尚未真机验证。
- [ ] 当前 `RegisterIo::write32` 不可失败，无法模拟或传播 ENABLE_SET 总线错误；volatile MMIO fault 会成为同步异常。
- [ ] `after_device_clear` 暂时仅由 mock handler 使用；首个生产 handler 接入前保留 `dead_code` allowance。
- [ ] 证据构造是 crate 内可信边界，不是硬件自动生成的证明；每个生产 handler 仍需独立审计 clear 写序。
- [ ] APBDMA status/clear 位未知，因此必须继续 KeepMasked。
- [ ] 下一批应优先为已知 W1C 语义的 MMC command interrupt 建立 concrete device-ack adapter，验证寄存器 read→W1C clear→Rearm；数据路径仍保持 deferred。

### 提交

- `[ref] gate 2K1000 IRQ rearm on device ack`

## 2026-08-10：批次 47——MMC W1C IRQ acknowledgement adapter

### 任务与设计

为已有明确 W1C 语义的 MMC `REG_INT=0x3c` 建立 source-bound ack adapter。已知 W1C
范围为低 10 位 `0x3ff`。adapter 先核对 acknowledged source，再读取状态；只有至少一个已知
pending 位、没有未知 pending 位、且 W1C 写成功时才生成 `Rearm(DeviceAckedIrq)`。

混合状态会清除已知 W1C 位，但返回 `UnknownPending` 并保持 LIOINTC masked，避免未知 level
condition 导致立即重入。所有失败都返还原 `AcknowledgedIrq`，允许诊断或重试。

### 完成内容

- [x] 新增 `MmcIrqAckError`、`MmcIrqAckFailure` 和 `acknowledge_interrupt()`。
- [x] source mismatch 在任何 register I/O 前失败并返还原 token。
- [x] zero/unknown-only 状态不执行 W1C 写，不产生 rearm evidence。
- [x] known-only 状态严格执行 REG_INT read→known W1C write→Rearm。
- [x] known+unknown 状态只清 known mask，返回 UnknownPending，保持 controller source masked。
- [x] read/write I/O 错误均返还 acknowledged token。
- [x] production MMC activation/IRQ handler 仍 deferred；本批只提供可组合 adapter。

### 验证证据

- `cargo test`：58 项 host 单测全部通过（本批新增 2 项）。
- 成功测试断言事件序列为 `Read(0x3c)`、`Write(0x3c, command_sent|response_crc)`，并核对 evidence IRQ。
- mismatch 测试断言零 I/O；zero 和 read-failure 测试断言只有一次 read。
- mixed 测试断言只写 known command-sent 位并返回未知 bit15；write failure 返回原 token。
- topology/畸形 DTS fixtures、LoongArch64 target check、`make kernel-la` 全部通过；仅有既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：MMC REG_INT 的 W1C 行为和 level deassert 时序尚未在 2K1000LA 验证。
- [ ] adapter 依赖上游已知 `0x3ff` W1C mask，但低 10 位各自完整语义仍未全部建模。
- [ ] W1C write 成功由 RegisterIo 返回值代表；没有 read-back 验证，W1C 寄存器也不保证适合 read-back。
- [ ] 没有生产 MMC owner/global handler，因此 adapter 尚未注册到 Live runtime。
- [ ] MMC clock、power、card-detect、DMA data path 未就绪，不能仅因 IRQ adapter 完成而激活 host。
- [ ] 下一批应把可恢复 ack failure 映射到统一 handler disposition adapter，避免 function-pointer handler 无法携带设备实例；设计固定 slot owner 或静态 context table，并测试并发/重入拒绝。

### 提交

- `[ref] add 2K1000 MMC IRQ ack adapter`

## 2026-08-10：批次 48——Fixed-capacity IRQ owner slots

### 任务与设计

新增安全的 `IrqOwnerTable<O>`，为 64 个 `GlobalIrq` 提供固定槽位。板级代码可令 `O` 为
包含 MMC/APBDMA 等实例的 enum，从而避免裸 `*mut ()` context。`begin(AcknowledgedIrq)` 把
owner 从 Ready 移入线性 `ActiveOwner`，原槽变为 InHandler；只有 `finish(active)` 才能恢复。

每次 register 使用全局单调 generation。active token 同时绑定 IRQ+generation，因此不能错误
归还到另一张 owner table 的同 IRQ 槽。active 被 drop 时 owner 被丢弃且槽永久 busy，这是有意
的 fail-closed 行为，防止同一设备状态被重复消费。

### 完成内容

- [x] 新增 Empty/Ready/InHandler 三态 fixed owner slot。
- [x] register 拒绝重复或 handler 运行中的 slot，并返还未注册 owner。
- [x] begin 只接受匹配 IRQ 的 acknowledged token；未注册/重入失败返还原 token。
- [x] ActiveOwner 提供唯一 `&mut O`，不暴露复制或 clone。
- [x] unregister 在 InHandler 状态被拒绝。
- [x] finish 校验 IRQ 和全局唯一 generation，失败返还完整 active owner。
- [x] token drop 保持槽 busy，后续 begin/unregister 均被拒绝。
- [x] 本批保持既有 function-pointer domain 不变，下一批再进行集成迁移。

### 验证证据

- `cargo test`：61 项 host 单测全部通过（本批新增 3 项）。
- 正常测试覆盖 register→begin→owner mutate→重入拒绝→finish→unregister，值从 10 变为 15。
- drop 测试断言 active 丢失后 slot 永久 InHandler，不能二次 begin 或 unregister。
- 跨表测试同时激活 A/B 同 IRQ，A token 交给 B 返回 InvalidCompletion，两表仍 busy；各自正确 finish 后 owner 值保持 1/2。
- topology/畸形 DTS fixtures、LoongArch64 target check、`make kernel-la` 全部通过；仅有既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] owner table 尚未替换 `LioIntcDomain` function-pointer handler；当前为可独立验证的基础层。
- [ ] generation 为 u64，理论 wrap 未特殊处理；实际 boot/runtime 注册次数不可达该上限。
- [ ] active drop 没有恢复 API，这是安全策略但要求生产 handler 严格避免 panic/提前遗失 token。
- [ ] 中断并发还需由每 CPU interrupt masking 或外部同步保证对 table 的唯一 `&mut` 访问。
- [ ] `UNVERIFIED_ON_HARDWARE`：真实多核同 source 到达与 LIOINTC mask 生效时序未验证。
- [ ] 下一批应让 domain 保存 owner slot identity 而非 function pointer，并由 Live runtime 同时持 domain+owner table；dispatch 流程变为 mask/ack→owner begin→device adapter→owner finish→按 disposition rearm。

### 提交

- `[ref] add 2K1000 IRQ owner slots`

## 2026-08-10：批次 49——Live runtime owner-table integration

### 任务与设计

将 `BoardIrqRuntime` 改为 `BoardIrqRuntime<I, O>` 并直接持有 `IrqOwnerTable<O>`。新增
`IrqOwner::handle(&mut self, AcknowledgedIrq) -> IrqDisposition`；ActiveOwner 单次交付
acknowledged token，handler 返回后先通过 IRQ+generation 校验把 owner 放回 Ready，随后才验证
disposition 并可能 ENABLE_SET。

configure API 现在接收设备 owner，而不是 function pointer。重复注册或 arm 失败通过
`ConfigureFailure<State, Owner>` 同时返还 runtime 状态与 owner，不丢失设备实例。

### 完成内容

- [x] ActiveOwner 保存 `Option<AcknowledgedIrq>`，handle 消费且只能交付一次。
- [x] 新增 `IrqOwner` trait，设备实例以安全 `&mut self` 处理 IRQ。
- [x] runtime 内 function-pointer domain 被 owner table 替代；纯 domain 模块仅保留独立契约测试。
- [x] Dormant/Configured/Live 全部泛型化为 `<I, O>`。
- [x] configure 重复/失败返还原 owner；成功后 owner 固定绑定 GlobalIrq。
- [x] service 顺序固定为 mask/ack→begin→handle→finish→disposition→optional rearm。
- [x] 未注册 owner 计为 unhandled；busy/finish failure 产生 partial failure并保持 source masked。
- [x] owner getter 只在 Ready 时返回引用，便于诊断持久设备状态。
- [x] volatile assembler 泛型化，可为未来板级 owner enum 构造 runtime。

### 验证证据

- `cargo test`：61 项 host 单测全部通过；原 runtime 测试已全部迁移为 `TestOwner`。
- duplicate configure 返回 `Owner(AlreadyRegistered)`，并断言返还 owner 未被修改。
- 连续两次 matching Rearm service 均执行 CLEAR→SET，owner `handled` 从 0 累积到 2。
- mismatch disposition 在 owner 已成功归还 Ready 后被拒绝，rearmed=0。
- 多 parent 测试仍为 3 masked、2 handled、1 unhandled，证明迁移未改变 source 覆盖。
- topology/畸形 DTS fixtures、LoongArch64 target check、`make kernel-la` 全部通过；仅有既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：owner handler 执行期间多核中断屏蔽与同 source 并发到达尚未真机验证。
- [ ] runtime 需要唯一 `&mut` 才能 service；全局发布容器必须避免 IRQ 上下文 spin-lock 死锁。
- [ ] owner handler panic 会在 token 归还前终止内核；slot fail-closed 语义仅适用于可返回控制流。
- [ ] production board owner enum 尚未定义；MMC adapter 仍没有持久 register backend 实例。
- [ ] CPU parent activator 和 global Live runtime 尚未生产接入。
- [ ] 下一批应定义 `BoardIrqOwner` enum，先加入 `MmcCommand` mock/target-deferred variant，把 MMC ack adapter 包装为 `IrqOwner`；APBDMA variant继续 KeepMasked。随后设计 IRQ-safe global runtime publication。

### 提交

- `[ref] bind 2K1000 IRQ runtime owners`

## 2026-08-10：批次 50——Concrete board IRQ owners

### 任务与设计

定义 `BoardIrqOwner<R>`，包含持久 `MmcCommandOwner<R>` 与保守
`DeferredApbDmaOwner`。MMC owner 保存 expected IRQ、register backend、handled 计数和最近
结构化错误；通过已有 W1C adapter 处理，成功才 Rearm，失败记录原因并 KeepMasked。
APBDMA 在 clear/status 语义未知时只记录次数/source，始终 KeepMasked。

同时新增 target-only `mmc::VolatileRegisters`。unsafe 构造验证非零、4-byte 对齐和最小
controller window；每次访问验证 offset 对齐/范围。当前无生产调用者，不会触碰硬件。

### 完成内容

- [x] 新增 `MmcCommandOwner<R>`，持久保存寄存器实例和 IRQ 诊断状态。
- [x] MMC owner 实现 `IrqOwner`，ack 成功清除 last_error，失败 KeepMasked 并保存错误。
- [x] 新增 `DeferredApbDmaOwner`，记录 handled/last_irq，固定 KeepMasked。
- [x] 新增泛型 `BoardIrqOwner<R>` enum 并统一分发两个 variant。
- [x] 新增 LoongArch64-only volatile MMC backend 与 unsafe 映射契约。
- [x] volatile register 地址按 checked offset 验证，不允许未对齐或越界访问。
- [x] 源码搜索确认 volatile backend 当前没有生产构造调用。

### 验证证据

- `cargo test`：63 项 host 单测全部通过（本批新增 2 项）。
- MMC owner 经 table begin→handle→finish 后仍持有同一 backend，handled=1、last_error=None，W1C 写正确。
- MMC zero-status owner 记录 `NoKnownPending` 并 KeepMasked。
- APBDMA deferred owner 处理 IRQ bank1/local13 后 handled=1、last_irq 匹配、disposition=KeepMasked。
- topology/畸形 DTS fixtures、LoongArch64 target check、`make kernel-la` 全部通过；仅有既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：volatile MMC 读写、W1C clear 和 IRQ deassert 尚未真机验证。
- [ ] volatile backend 的 MMIO fault 无 Result 通道，会成为架构同步异常。
- [ ] BoardIrqOwner 尚未由 topology 自动构造并注册到 runtime。
- [ ] MMC owner 只处理 command interrupt ack；clock/power/data path 未完成，不能启动 host。
- [ ] APBDMA owner 有意不 Rearm，直到 device clear/status 证据可用。
- [ ] 下一批应实现纯逻辑 `OwnerPlan`：从 topology 解析 MMC/APBDMA binding、owner variant、route/core/HWI；验证重复 source、缺失设备和 blocker，并让 fixture 检查 plan，仍不创建 volatile backend。

### 提交

- `[ref] add 2K1000 board IRQ owners`

## 2026-08-10：批次 51——Topology-compiled IRQ owner plan

### 任务与设计

新增纯逻辑 `irq_plan::compile()`，从动态 topology 生成固定两项 `BoardOwnerPlan`。每项保存
owner kind、稳定 InterruptBinding、Route、CPU HWI、device MMIO 和 activation policy。
route 不硬编码：根据该 LIOINTC `parent_source_maps` 中唯一覆盖 local source bit 的 slot 推导，
再由对应 parent interrupt spec 得到 HWI。

MMC policy 为 AckOnly，只允许使用 W1C adapter 做中断确认，不代表 host 可激活；APBDMA policy
为 Deferred。两项按 GlobalIrq raw 排序，避免 topology 节点顺序影响启动计划。

### 完成内容

- [x] 新增 `OwnerKind`、`ActivationPolicy`、`OwnerPlan`、`BoardOwnerPlan` 和错误模型。
- [x] 严格要求唯一 MMC 与唯一 APBDMA 描述。
- [x] 复用 RuntimeLayout 与 InterruptBinding 的全部 topology 验证。
- [x] MMC 资源先通过 deferred bring-up plan 验证。
- [x] route 由 source map 唯一覆盖关系推导，拒绝 missing/ambiguous route。
- [x] parent spec 必须是单 cell HWI0–HWI7。
- [x] 拒绝 MMC/APBDMA 解析为同一 GlobalIrq。
- [x] 输出按 global IRQ 排序并携带 boot-core `core_mask=1`。
- [x] fixture verifier 直接断言真实 MMC/DMA owner plan。

### 验证证据

- `cargo test`：65 项 host 单测全部通过（本批新增 2 项）。
- 乱序 controller topology 仍生成 MMC raw31/int0/HWI2/AckOnly 与 DMA raw45/int1/HWI3/Deferred。
- 缺失 MMC 返回 MissingOrDuplicateMmc；重复 GlobalIrq 返回 DuplicateIrq。
- 真实 DTS fixture 验证相同 kind、raw IRQ、route slot、HWI 与 policy。
- topology/畸形 DTS fixtures、LoongArch64 target check、`make kernel-la` 全部通过；仅有既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：parent source map 到真实 CPU HWI 的路由仍需真机确认。
- [ ] core_mask 固定 boot core；SMP affinity、迁移和 per-core ISR 尚未设计。
- [ ] plan 要求恰好一个 MMC/APBDMA，未来多 controller board 需要扩展固定容量与选择策略。
- [ ] AckOnly 不表示 MMC clock/power/data path 可用，生产启动必须继续尊重 blockers。
- [ ] plan 尚未实例化 BoardIrqOwner 或 ConfiguredRuntime。
- [ ] 下一批应实现 mock-first `apply_owner_plan`：按 plan 顺序构造 owner、调用 Dormant.configure；Deferred 项必须保持 local source disabled，AckOnly 项也只在显式 diagnostic mode 下允许配置，默认启动不触碰硬件。

### 提交

- `[ref] compile 2K1000 IRQ owner plan`

## 2026-08-10：批次 52——Safe owner-plan application modes

### 任务与设计

新增 `ApplyMode::{SafeDefault, DiagnosticAckOnly}`。SafeDefault 只返回 Dormant runtime，
不调用 owner factory、不执行 configure，因此不会写 route/trigger/ENABLE_SET。DiagnosticAckOnly
只选择唯一 AckOnly 项；Deferred APBDMA 始终跳过。

应用返回 `AppliedRuntime::{Dormant, Configured}` 和固定计数报告。factory/configure 失败通过
`ApplyFailure` 返还 Dormant runtime、可选 owner 与 partial report；修正问题后可使用相同资源重试。
启动路径只编译并保存 owner plan，不调用应用函数。

### 完成内容

- [x] 新增 ApplyMode、ApplyReport、ApplyError、AppliedRuntime 和 ApplyFailure。
- [x] SafeDefault 对 AckOnly 计 skipped-policy，对 Deferred 计 skipped-deferred，factory 零调用。
- [x] DiagnosticAckOnly 最多选择一个 AckOnly；多个会在硬件动作前失败。
- [x] owner factory error 返还原 Dormant runtime，owner=None。
- [x] configure error 返还 Dormant runtime 与原 owner。
- [x] 修复失败后可用同一 runtime/owner 重试并得到 Configured raw31。
- [x] ConfiguredRuntime 暴露只读 configured source mask 供验证。
- [x] `init_after_boot()` 编译并一次保存 owner plan；重复初始化检查同步覆盖 plan 容器。
- [x] 默认启动没有 apply/volatile backend/controller 配置调用。

### 验证证据

- `cargo test`：67 项 host 单测全部通过（本批新增 2 项）。
- SafeDefault 测试断言 factory calls=0，configured=0、deferred=1、policy=1，结果为 Dormant。
- Diagnostic 测试依次制造 factory IoError、route core_mask=0 configure InvalidParam；两次均返还状态。
- 第三次使用返还 owner 成功，report configured=1/deferred=1，configured mask=`1<<31`。
- topology/畸形 DTS fixtures、LoongArch64 target check、`make kernel-la` 全部通过；仅有既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：DiagnosticAckOnly 尚未用 volatile runtime 执行，所有寄存器行为仍未真机验证。
- [ ] SafeDefault 之前若调用 assembler，assembler 自身会 mask-all；当前生产启动连 assembler 也不调用，完全零 MMIO。
- [ ] 应用器当前支持唯一 AckOnly；多设备需扩展可迭代 Configured 状态和失败恢复列表。
- [ ] owner plan/topology/layout 分别在 spin mutex 中，仅用于 boot/诊断，不得在 IRQ 热路径读取。
- [ ] CPU parent activator 尚无 target backend，Configured 不能生产进入 Live。
- [ ] 下一批应实现 LoongArch ECFG HWI enable/disable backend，先用纯 register model 测试只修改 HWI2/HWI3、保留 timer/IPI/其他位；volatile CSR 写保持 target-only 和未启用。

### 提交

- `[ref] apply 2K1000 IRQ plans safely`

## 2026-08-10：批次 53——LoongArch CPU HWI parent-line control

### 任务与设计

在 arch-api 新增 CPU-local external interrupt line 契约。LoongArch 将抽象 bit0–bit7
映射为 ECFG.LIE 的 HWI0–HWI7（bit2–bit9）；RISC-V 明确返回 Unsupported。CSR read-modify-write
复用一个纯 `update_external_interrupt_lines()` 模型，使 timer bit11、IPI bit12、VS 等其余字段
的保持性可以在无板环境静态验证。

2K1000 驱动新增惰性的 `LoongArchCpuParentActivator`，只在调用 trait 时访问当前 CPU ECFG；
生产 `init_after_boot()` 不构造、不调用该适配器，SafeDefault 行为保持零 CSR/零 MMIO 写。

### 完成内容

- [x] 新增 `ArchExternalInterruptLines` 与 `ArchExternalInterruptControl` 公共契约。
- [x] platform-arch aggregate 暴露 enable/disable external line 调用。
- [x] LoongArch 实现 HWI0–HWI7 到 ECFG bit2–bit9 的 enable/disable。
- [x] 纯寄存器模型覆盖 HWI0/HWI7 边界，并断言 timer、IPI、VS/其他位保持不变。
- [x] RISC-V 实现显式返回 Unsupported，不伪造按线控制能力。
- [x] 新增 2K1000 `CpuParentActivator` target backend；host fallback 返回 Unsupported。
- [x] 适配器与 CSR 写均标记 `UNVERIFIED_ON_HARDWARE`，未接入生产初始化。

### 验证证据

- 2K1000 驱动 `cargo test`：67 项 host 单测全部通过。
- arch-api host test/doc-test 通过；LoongArch 纯模型同时由编译期断言校验。
- `make kernel-la` 通过；仅有仓库既有 warning。
- 2K1000 精确 feature check：`cargo check --no-default-features --features
  loongson2k1000la,heap-tlsf,final_online --target loongarch64-unknown-none` 通过。
- 直接使用默认 feature 做 LoongArch check 会把 `sbi-rt` 带入并报 RISC-V 寄存器错误；这是配置选择问题，
  改为上述 2K1000 精确 feature 后通过。
- topology/畸形 DTS fixtures 与 `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：ECFG.LIE bit2–bit9 映射和 CSR read-modify-write 尚未在 2K1000 实测。
- [ ] 当前适配器只启用当前 CPU 的 HWI，SMP per-core 初始化与 affinity 尚未实现。
- [ ] `CpuParentActivator` 只提供 enable；Live 回滚或 shutdown 所需 disable 流程尚未接入 runtime typestate。
- [ ] production init 仍不 assemble/apply/activate runtime，因此不会意外打开父中断线。
- [ ] 下一批应增加可回滚的 parent activation transaction：启用失败或后续发布失败时关闭本次新增 HWI，
  并用 mock 验证原先已启用的 HWI 不被误关；仍只供显式诊断入口使用。

### 提交

- `[ref] add LoongArch external IRQ line control`

## 2026-08-10：批次 54——Rollback-safe CPU parent activation

### 任务与设计

将 CPU parent activation 改为显式增量事务。调用方提供当前 CPU 已拥有的 HWI snapshot，
runtime 根据 configured source 推导 requested parents，并只对 `requested & !already_enabled`
执行 enable。enable 后的 commit hook 用于未来诊断 runtime 发布；若 commit 失败，只 disable
本事务新增位，不影响其他消费者已有 HWI。

失败对象返还完整 Configured typestate、激活报告、可选 rollback error 和无法撤销的 residual
parent mask。调用方可以把 residual 合并进下一次 `already_enabled` 后重试，避免重复 enable。

### 完成内容

- [x] `CpuParentActivator` 增加对称的 `disable_parent_lines()` 契约。
- [x] 新增 `ParentActivationReport`，记录 requested/already-enabled/newly-enabled。
- [x] 新增 `ActivationFailure`，返还 state、原错误、rollback error 和 residual mask。
- [x] ConfiguredRuntime 增加 `activate_transactional()`；旧 `activate()` 保持兼容并使用零已有位、无失败 commit。
- [x] LiveRuntime 保存并暴露其 parent-line mask。
- [x] LoongArch target adapter 使用上一批 arch API 关闭事务拥有的 ECFG HWI 位。
- [x] host fallback 的 enable/disable 均明确返回 Unsupported。
- [x] production 初始化未调用 assemble/apply/activate，安全默认行为不变。

### 验证证据

- `cargo test`：68 项 host 单测全部通过（本批新增 1 项）。
- mock 验证已有 HWI3 时，本事务只 enable/disable HWI2，未把 HWI3 交给 rollback。
- mock 注入 commit InvalidParam 与 rollback IoError，失败结果保留原错误、rollback error 和 residual HWI2。
- 使用返还的 Configured state 重试，并把 residual 合并进 already-enabled；未重复执行 enable/disable，成功进入 Live。
- 2K1000 精确 feature LoongArch target check 通过；`make kernel-la` 通过，仅有既有 warning。
- topology/畸形 DTS fixture 与 `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：真实 ECFG enable/rollback 以及 LIOINTC 到 CPU HWI delivery 未实测。
- [ ] already-enabled 由未来的单 CPU 诊断状态所有者维护；当前不读取 raw ECFG 来猜测软件 ownership。
- [ ] runtime 尚无全局 Live slot，commit hook 目前只在 mock 中模拟 publication failure。
- [ ] rollback 失败后的 residual 必须由调用方保留；掉电/重启前不能假定该 HWI 已关闭。
- [ ] SMP 每核 parent ownership、CPU offline 和 affinity 迁移尚未实现。
- [ ] 下一批应实现一次发布的 diagnostic runtime slot：SafeDefault 不执行；显式 diagnostic 请求才
  assemble volatile controller、应用 AckOnly plan、事务激活并原子发布，任何阶段失败均保留可诊断状态。

### 提交

- `[ref] make 2K1000 IRQ activation rollback-safe`

## 2026-08-10：批次 55——Masked staging and single-publication runtime slot

### 任务与设计

审查诊断入口时发现原 configure 会立即写 ENABLE_SET；如果后续 CPU parent activation/publish
失败，丢弃 Configured typestate 会遗留已开启 device source。因此先将配置和交付严格分阶段：
configure 只写 route/trigger 且保持 source masked；activate 先启用 CPU parent，再启用 configured
source；commit 失败按 source-mask → 新增 parent-disable 的顺序回滚。

新增 lock-free `DiagnosticRuntimeSlot`。诊断流程必须在任何硬件写前 reserve；reservation drop
自动恢复 Empty，commit 在持有 reservation 时为不可失败写入。IRQ service 使用原子
Live→Servicing 独占访问，避免持有初始化 spin mutex，重入/并发 service fail closed。

### 完成内容

- [x] `InterruptBinding::configure_masked()` 只配置 route/trigger，不执行 ENABLE_SET。
- [x] 既有 `arm()` 复用 masked configuration 后显式 enable，生命周期 API 行为保持兼容。
- [x] Dormant/Configured runtime 配置 owner 时保持所有 device source masked。
- [x] activation 在 parent enable 成功后才逐项 ENABLE_SET。
- [x] commit failure 先逐项 ENABLE_CLEAR，再回滚本事务新增 CPU HWI。
- [x] `ActivationFailure` 分开报告 source rollback 与 parent rollback error。
- [x] 新增 Empty/Reserved/Live/Servicing 原子 slot 状态机。
- [x] reservation 未 commit 时 Drop 可重试；commit 后拒绝二次发布。
- [x] service 重入返回 Busy，不等待自旋锁、不暴露半初始化值。
- [x] production init 尚未创建/调用 slot，默认硬件行为不变。

### 验证证据

- `cargo test`：70 项 host 单测全部通过（本批新增 2 项 slot 测试）。
- rollback 顺序模型断言 LIOINTC 写序列为 ENABLE_SET → ENABLE_CLEAR → retry ENABLE_SET。
- parent mock 断言 commit 失败时只撤销事务新增 HWI2，已有 HWI3 不受影响。
- slot 测试覆盖 reservation 冲突、drop 后重试、单次 commit、Live mutation 和 service 重入 Busy。
- 2K1000 精确 feature LoongArch target check、`make kernel-la`、topology/畸形 DTS fixture 通过；
  仅有既有 warning。
- `git diff --check` 通过；未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：masked route/trigger 配置、parent-first/source-last 顺序仍需示波或日志上板确认。
- [ ] slot 是基础容器，target-only volatile LiveRuntime 尚未声明为全局实例。
- [ ] Servicing 防止同一 slot 并发，但当前未实现 SMP interrupt affinity，预期诊断阶段只路由 boot CPU。
- [ ] commit 后 runtime 不支持卸载；CPU offline/shutdown 需新增 drain/quiesce 状态。
- [ ] source enable 的底层 volatile backend 当前不会报告总线异常；错误模型主要覆盖契约/未来可失败 backend。
- [ ] 下一批可安全实现显式 target-only diagnostic bring-up：先 reserve slot，再 assemble masked runtime、
  应用 AckOnly owner、事务 activate，最后 infallible commit；Machine handler 只服务 Live slot。

### 提交

- `[ref] stage 2K1000 IRQ runtime before publication`

## 2026-08-10：批次 56——Explicit volatile IRQ diagnostic runtime

### 任务与设计

新增仅 LoongArch target 存在的全局诊断 LiveRuntime slot。显式 unsafe activation 严格执行：
reserve slot → 复制已发布 layout/owner plan → assemble volatile LIOINTC 并 mask-all →
只构造 MMC AckOnly owner → masked configure → transaction activate → infallible slot commit。

入口不属于 `init_after_boot()`；安全默认启动仍只保存 topology/layout/plan。聚合 driver 暴露
`activate_loongson2k1000_irq_diagnostic()`，调用者必须显式承担 MMIO 独占、boot CPU 0、无其他
LIOINTC/ECFG owner 等 unsafe 前置条件。

### 完成内容

- [x] 新增 target-only `DiagnosticRuntimeSlot<TargetRuntime>` 全局实例。
- [x] 在任何 MMIO/CSR 写之前 reserve，重复/并发 activation 立即失败。
- [x] volatile assembly 先对两个 LIOINTC bank 执行 mask-all。
- [x] owner factory 仅接受 MmcCommand，APBDMA Deferred 不会被构造或激活。
- [x] MMC volatile register ownership进入 LiveRuntime，并添加受 slot 串行化约束的 Send 证明。
- [x] activation failure 保留 source rollback、parent rollback 和 residual HWI 到错误模型。
- [x] 完整 LiveRuntime 才能 commit；Machine external handler 只服务 slot 中 Live 状态。
- [x] Empty slot 返回 Unsupported；Reserved/Servicing 等竞争状态 fail closed 为 IoError。
- [x] 聚合 driver 提供显式 unsafe 入口，默认 init 路径没有调用。
- [x] host 实现不访问硬件，activation 返回 NotInitialized、service 返回 Unsupported。

### 验证证据

- `cargo test`：71 项 host 单测全部通过（本批新增 1 项 host fail-closed 测试）。
- 既有 70 项模型测试覆盖 assembly mask-all、AckOnly plan、masked configure、parent/source rollback、
  single-publication 和 service reentrancy；target glue 本身由 LoongArch 精确 feature 编译验证。
- `cargo check --no-default-features --features loongson2k1000la,heap-tlsf,final_online
  --target loongarch64-unknown-none` 通过，包含全局 volatile runtime 的完整类型检查。
- `make kernel-la`、topology/畸形 DTS fixture、`git diff --check` 通过；仅有既有 warning。
- 未调用 unsafe activation，未进行任何真实 MMIO/CSR 写，未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：完整 activation 和 interrupt delivery 从未执行；当前只有编译与 model 证据。
- [ ] 诊断入口没有绑定到默认 boot、远程 monitor 或 shell 命令，避免误触；需要板上专用 harness 显式调用。
- [ ] 当前固定 boot CPU/core 0，未支持 SMP affinity、per-CPU ECFG ownership 或 CPU offline。
- [ ] service error 目前折叠为 IoError 并记录日志；后续诊断接口可暴露结构化计数/last failure。
- [ ] Live runtime 只支持一次发布，没有 shutdown/drain/unpublish；重启前无法动态卸载。
- [ ] MMC owner 仅确认 command W1C interrupt，不代表 clock/power/data/card path 可用。
- [ ] 下一批应增加 Live runtime 诊断状态快照与安全 drain 状态机：先 mask device sources、等待
  Servicing 退出、再关闭 transaction-owned parent HWI；用 mock 覆盖 Busy、重复 drain 和失败重试。

### 提交

- `[ref] add explicit 2K1000 IRQ diagnostic runtime`

## 2026-08-10：批次 57——Retryable diagnostic IRQ drain

### 任务与设计

为诊断 runtime 增加 Live→Draining→Empty 状态迁移。slot 通过 CAS 独占 runtime；正在
Servicing 时 drain 立即 Busy，不在中断相关路径自旋。drain 操作失败时 guard 将状态恢复
Live 并保留原 runtime，允许对同一 MMIO/owner 状态重试。

硬件 quiesce 顺序固定为逐项 mask configured LIOINTC source，然后 disable runtime 拥有的
CPU parent HWI。parent disable 失败时 source 已保持 masked，runtime/parent mask 仍被保存；
再次 drain 会幂等 mask source 并重试 parent disable。两步都成功后才 drop runtime 并回到 Empty。

### 完成内容

- [x] DiagnosticRuntimeSlot 新增 Draining 状态和泛型 `drain()`。
- [x] Empty drain 返回 Empty；Reserved/Servicing/Draining 竞争返回 Busy。
- [x] drain operation error 恢复 Live，不移动或销毁 value。
- [x] 成功 drain 原地 drop value并恢复 Empty，允许下一次 reserve/activation。
- [x] LiveRuntime 新增 `quiesce()`、QuiesceReport 和 Source/Parent 分层错误。
- [x] quiesce 先 ENABLE_CLEAR 所有 configured source，再 disable owned parent lines。
- [x] parent disable 失败保持 parent mask，成功后清零 LiveRuntime parent ownership。
- [x] target diagnostic runtime 接入 LoongArch activator drain。
- [x] 聚合 driver 暴露显式 unsafe `drain_loongson2k1000_irq_diagnostic()`。
- [x] host activation/drain/service 全部 fail closed，不访问硬件。

### 验证证据

- `cargo test`：73 项 host 单测全部通过（本批新增 slot drain 与 runtime quiesce 各 1 项）。
- slot 测试覆盖 operation failure恢复 Live、成功回到 Empty、重复 drain Empty、重新 reserve/publish。
- service 闭包内发起 drain 返回 Busy，证明不会与正在执行的 IRQ handler 并发取得可变引用。
- runtime mock 注入第一次 parent disable IoError；source 写序列断言为 ENABLE_SET、ENABLE_CLEAR、
  retry ENABLE_CLEAR，第二次 parent disable 成功且 ownership 清零。
- 2K1000 精确 feature LoongArch target check 与 `make kernel-la` 通过；仅有既有 warning。
- topology/畸形 DTS fixture 和 `git diff --check` 通过；未执行真实 unsafe drain，未创建镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：LIOINTC mask 与 ECFG parent disable 的物理顺序/可见性未上板验证。
- [ ] drain Busy 由调用方稍后重试；当前没有阻塞等待或超时协调器。
- [ ] source mask backend 没有读回验证，volatile write 总是返回成功；真机应增加 ENABLE_STATUS readback。
- [ ] parent disable 失败后 slot 恢复 Live，但 sources 已 masked；service 可进入但不会观察到这些 source。
- [ ] 仅支持 boot CPU 0；跨 CPU drain 必须先实现 affinity/ownership 迁移。
- [ ] runtime drop 不清除 device 侧已有 pending 状态；MMC W1C drain policy需根据真机状态寄存器补充。
- [ ] 下一批应实现 LIOINTC ENABLE_STATUS 有界读回验证，用 model 覆盖延迟生效、timeout、错误 mask，
  并让 activation/quiesce 对无法确认的 enable/disable fail closed。

### 提交

- `[ref] add retryable 2K1000 IRQ runtime drain`

## 2026-08-10：批次 58——Bounded LIOINTC enable-status verification

### 任务与设计

所有关键 LIOINTC source enable/disable 写入后轮询 `ENABLE_STATUS`。单 source 操作只判断目标
bit，允许其他 owner 的无关位保持；mask-all 必须确认整个 status 归零。轮询使用固定 64 次默认
budget，无时钟依赖且不会无限阻塞。零 budget 为 InvalidParam，预算耗尽为 IoError。

assembly mask-all、activation ENABLE_SET、activation rollback、service mask-ack、device-ack rearm
和 drain/quiesce 均改用 verified 操作。任何 status 无法确认的路径不发布、不 rearm 或不销毁
runtime，保持 fail closed。

### 完成内容

- [x] 新增 `enable_verified()` 与目标 bit 的 enabled=true 有界确认。
- [x] 新增 `mask_ack_verified()` 与目标 bit 的 enabled=false 有界确认。
- [x] 新增 `mask_ack_claim_verified()`，只有 mask 状态确认后才生成 AcknowledgedIrq evidence。
- [x] 新增 `mask_all_verified()`，确认完整 ENABLE_STATUS 为零。
- [x] BoardIrqRuntime assembly 在保存 controller 前确认 bank 已全部 mask。
- [x] activation/source rollback/quiesce 使用 verified set/clear。
- [x] IRQ service mask-ack 与 W1C 后 rearm 使用 verified 操作。
- [x] runtime ModelIo 模拟 ENABLE_SET/CLEAR 对 ENABLE_STATUS 的硬件状态变化。
- [x] 状态等待使用 `spin_loop()` hint，不引入睡眠、分配或 timer 依赖。

### 验证证据

- `cargo test`：74 项 host 单测全部通过（本批新增 1 项脚本化 status 测试）。
- delayed model 让状态在两次 read 后生效；budget=3 的 enable 成功，并保持原有无关 bit7。
- delayed mask 在预算内清除目标 bit3，无关 bit7 保持。
- stuck model 对单 bit mask 与 mask-all 均在三次读取后返回 IoError。
- mask-all budget=0 返回 InvalidParam；无无限轮询路径。
- 既有 runtime 测试现通过真实 status 模型覆盖 assembly、activation、rollback、service、rearm、quiesce。
- 2K1000 精确 feature LoongArch target check、`make kernel-la`、topology/畸形 DTS fixture、
  `git diff --check` 通过；仅有既有 warning，未创建镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：ENABLE_STATUS 是否同步反映 SET/CLEAR、需要何种 barrier/延迟仍需真机确认。
- [ ] 固定 64 次是操作次数上界而非时间上界；真机若需要更长 posted-write drain，应基于计时器校准。
- [ ] volatile read/write 当前没有显式架构 I/O fence；需要核对 2K1000 uncached device mapping 与 LoongArch ordering。
- [ ] mask-all 要求 status 全零，因此只适用于 runtime 独占 controller 的 assembly 阶段。
- [ ] service 内最多轮询 64 次，虽有界但可能增加 IRQ latency；真机应记录最大/平均 polls。
- [ ] 错误仍折叠为 DriverError::IoError，尚未暴露 observed status 和 polls-used 供远程诊断。
- [ ] 下一批应增加 LIOINTC 操作诊断报告（期望值、最后 status、poll count）并在 runtime last-failure
  快照中保存，便于无 JTAG 情况下通过串口/远程 monitor 定位寄存器时序问题。

### 提交

- `[ref] verify 2K1000 IRQ enable state`

## 2026-08-10：批次 59——Structured LIOINTC status-poll diagnostics

### 任务与设计

为 verified status polling 增加固定尺寸结构化证据。每次操作报告 operation、expected mask/value、
最后完整 ENABLE_STATUS 和 polls-used；failure 额外保存 DriverError。controller 分别保留最近一次
poll report 与最近一次 failure，后续成功不会擦除历史 failure。

Configured/Live runtime 可汇总两个 bank 的 failure snapshot。显式诊断 activation、drain 和 service
失败路径将 snapshot 写入日志，为无 JTAG 环境下的串口/远程排障提供可复制证据。

### 完成内容

- [x] 新增 `StatusPollOperation::{Enable, Mask, MaskAll}`。
- [x] 新增 `StatusPollReport`：expected mask/value、observed full status、poll count。
- [x] 新增 `StatusPollFailure`：DriverError 加完整 report。
- [x] LioIntc 保存 last report 与 last failure；成功只更新 report，不清除历史 failure。
- [x] zero-budget 在任何 SET/CLEAR 写之前失败，并记录 polls=0 与调用时 observed status。
- [x] ConfiguredRuntime/LiveRuntime 暴露两个 controller 的固定数组 failure snapshot。
- [x] diagnostic activation failure 打印返还 Configured state 的 status snapshots。
- [x] diagnostic drain failure 在 runtime 仍受 slot 独占时打印 snapshots。
- [x] diagnostic service failure 在状态恢复 Live 后读取并打印 snapshots。
- [x] 无堆分配、无新增锁，所有报告均 Copy。

### 验证证据

- `cargo test`：74 项 host 单测全部通过。
- delayed enable 精确报告 operation=Enable、mask/value=bit3、完整 status 含无关 bit7、polls=3。
- delayed mask 报告 polls=2；stuck mask 报告 observed bit4、polls=3、IoError。
- stuck mask-all 报告 expected mask=u32::MAX/value=0、observed bit4、polls=3。
- zero-budget mask-all 在写前报告 observed bit7、polls=0、InvalidParam。
- 2K1000 精确 feature LoongArch target check、`make kernel-la`、topology/畸形 DTS fixture 和
  `git diff --check` 通过；仅有既有 warning，未创建镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：report 字段可用，但真实 observed/status timing 尚无上板样本。
- [ ] failure 目前通过日志输出，尚无稳定的远程 monitor 命令或用户态查询 ABI。
- [ ] service failure 后重新取得 slot 记录日志存在一个短竞争窗口；另一个 CPU 可能先进入 service。
- [ ] 只保存每 bank 最近一次 failure，不保存历史环形缓冲；连续故障会覆盖较旧证据。
- [ ] assembly mask-all 失败时 runtime 尚未形成，controller 内 snapshot 随错误路径丢失；应把 failure
  直接附加到 RuntimeError 或 assembly error report。
- [ ] 下一批应实现板级 `DiagnosticIrqSnapshot`：包含 slot state、configured sources、parent lines、
  两 bank status failures 和 service counters，并通过无硬件访问的 aggregate 查询函数暴露。

### 提交

- `[ref] record 2K1000 IRQ status diagnostics`

## 2026-08-10：批次 60——Read-only diagnostic IRQ snapshots

### 任务与设计

新增固定尺寸、无硬件访问的板级诊断快照。slot 暴露 Empty/Reserved/Live/Servicing/Draining
Acquire snapshot；只有 CAS 取得 Live runtime 独占访问时才复制 runtime 数据。若与 service/drain
竞争，不等待、不重试，返回当时 slot state 和 runtime=None。

LiveRuntime 使用饱和计数累计 service calls/successes/failures 以及 parent/masked/handled/
unhandled/rearmed 数量。成功和失败的 partial ServiceReport 都进入累计统计。runtime snapshot 同时
包含 configured source mask、parent HWI mask 和两个 bank 最近 status-poll failure。

### 完成内容

- [x] 新增公开 `DiagnosticSlotState` 五态枚举和原子 `state()`。
- [x] slot 测试直接观察 reserve、service、drain 期间的 Reserved/Servicing/Draining。
- [x] 新增固定尺寸 `ServiceCounters`，全部使用 saturating add。
- [x] LiveRuntime service 成功/失败均更新 calls 与对应 report 累计值。
- [x] 新增 `RuntimeDiagnosticSnapshot`，汇总 ownership、service counters 和 status failures。
- [x] 新增板级 `DiagnosticIrqSnapshot { slot_state, runtime }`。
- [x] target snapshot 只复制内存，不读取 LIOINTC/MMC/ECFG。
- [x] host snapshot 确定性返回 Empty/None。
- [x] 聚合 driver 暴露 `loongson2k1000_irq_diagnostic_snapshot()`。
- [x] SafeDefault、activation 和 drain 行为均未改变。

### 验证证据

- `cargo test`：74 项 host 单测全部通过。
- slot 测试断言 Empty→Reserved→Live、service 内 Servicing、drain 内 Draining、失败恢复 Live、
  成功恢复 Empty。
- LiveRuntime 测试断言初始 counters 全零；InvalidSnapshot service 后 calls=1/failures=1，
  success 与 report 累计保持零。
- host 板级 snapshot 精确等于 Empty/runtime=None，且 activation/drain/service 仍 fail closed。
- 2K1000 精确 feature LoongArch target check、`make kernel-la`、topology/畸形 DTS fixture、
  `git diff --check` 通过；仅有既有 warning，未创建镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：snapshot 本身无硬件访问，但其中真实计数/status 仍需上板产生。
- [ ] 查询与 service/drain 竞争时 runtime=None；调用方需依据 state 稍后重试。
- [ ] 统计只有累计值，没有时间戳、CPU id、最近 snapshot 或最近 RuntimeError。
- [ ] counters 位于 runtime 内，成功 drain 后随 runtime 销毁；没有跨 activation epoch 累计。
- [ ] aggregate 查询仅在 2K1000 feature 下编译，尚未接入 remote-debug monitor 文本命令。
- [ ] 下一批应将 snapshot 接入已有 development-only remote-debug monitor，增加只读 `ls2k-irq`
  命令；非 2K1000 profile 返回 unsupported，命令不得触发 activation/drain 或硬件读取。

### 提交

- `[ref] expose 2K1000 IRQ diagnostic snapshots`

## 2026-08-10：批次 61——Read-only `ls2k-irq` remote monitor command

### 任务与设计

在既有 development-only、无认证 TCP monitor 中新增只读 `ls2k-irq`。命令解析在所有 profile
一致；2K1000 feature 下只调用内存 snapshot API，其他 profile 返回明确 unsupported。没有新增
activation/drain 或任何 MMIO/CSR 路径。

Live 输出单行固定字段：slot state、configured/parent masks、service counters、bank0/bank1 failure。
每个 failure 编码 operation、expected mask/value、observed full status、poll count；无 failure 为 none。
非 Live/竞争状态输出 state 与 runtime=unavailable，调用端可稍后重试。

### 完成内容

- [x] Command enum/parser 增加 `ls2k-irq`。
- [x] help 文本列出新命令。
- [x] 2K1000 formatter 使用 `loongson2k1000_irq_diagnostic_snapshot()`，无硬件访问。
- [x] Empty/Reserved/Servicing/Draining 等无 runtime 快照状态有稳定响应。
- [x] Live response 输出 ownership、9 项 service 统计和两个 bank failure。
- [x] 非 2K1000 profile 返回 `ERR unsupported`，不引用板级类型。
- [x] 命令响应始终不关闭 session。
- [x] 未增加远程 activation/drain 写命令，安全边界保持只读。
- [x] parser/response 增加 cfg-aware 单元测试。

### 验证证据

- QEMU LoongArch + `remote-debug-monitor` 精确 feature target check 通过，验证 unsupported 分支。
- 2K1000 + `remote-debug-monitor` 精确 feature target check 通过，验证完整 snapshot formatter。
- 2K1000 驱动 74 项 host 单测继续通过。
- remote_debug 单元测试源码覆盖 whitespace parser、`ls2k-irq` 解析、response 不关闭，以及 profile
  对应的 state/unsupported 前缀；内核 no_std target 仅完成编译，未在物理 NIC 上执行。
- `make kernel-la EXTRA_FEATURES=remote-debug-monitor`、topology/畸形 DTS fixture、
  `git diff --check` 通过；仅有既有 warning。
- 未启动 TCP listener、未创建镜像、未执行任何诊断硬件写。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：2K1000 尚无可用 NIC 驱动，monitor 无法在板上建立 TCP 连接。
- [ ] monitor 无认证/加密，只能用于隔离开发网络，不能作为生产 sshd 替代。
- [ ] QEMU LoongArch 本批只编译 remote feature；既有端到端 transport 证据来自 RISC-V QEMU。
- [ ] Live 单行可能较长，当前 send_all 支持分段发送，但客户端脚本需按 CRLF 读取完整行。
- [ ] snapshot 竞争时只返回 unavailable，不自动重试。
- [ ] 未提供 JSON/稳定 ABI；字段名面向人工与简单脚本诊断。
- [ ] 下一批应转向 2K1000 MMC 的最小非数据 bring-up 前置：只读/规划 clock、power、card-detect
  provider，不启动 host；优先补齐 topology provider ownership 和可测试的 blocker 消解状态机。

### 提交

- `[feat] expose 2K1000 IRQ status remotely`

## 2026-08-10：批次 62——2K1000 MMC prerequisite/provider readiness model

### 任务与设计

1. 从 DTB 中识别 MMC clock 与 supply provider，而不执行 provider 操作。
2. 将“资源已描述”和“硬件已就绪”分开，输出 clock、vmmc、vqmmc、card-detect 四项状态。
3. 只有 DT 明确声明无 GPIO 的 fixed regulator 为 `regulator-always-on` 或
   `regulator-boot-on` 时，才分类为 firmware-maintained；其他情况保持 requires-driver。
4. LS2K clock 只保存 topology 验证后的 MMIO 窗口；不读取/写入寄存器。
5. 数据、DMA、clock control、power、card-detect、IRQ 六个 activation blocker 保持不变，
   `can_activate()` 继续固定返回 false。

### 完成内容

- [x] `MmcDescription` 保存 `MmcClockProvider` 和带类型的两个 supply provider。
- [x] LS2K clock provider 验证单个非零、至少 4-byte 的 MMIO region。
- [x] fixed-regulator 保存 always-on、boot-on、GPIO-controlled 三项 DT 属性。
- [x] 非支持的 clock/regulator provider 被保守分类为 `UnsupportedProvider`，不猜测能力。
- [x] 新增 `PrerequisitePlan`，覆盖 ReadyByTopology、FirmwareMaintained、RequiresDriver、
  Missing、UnsupportedProvider 五种状态。
- [x] non-removable card 仅由拓扑即可判定；GPIO/native card detect 仍要求驱动；broken-cd
  作为固件/板级维持策略记录。
- [x] machine discovery 日志打印 prerequisite plan 与既有 blockers。
- [x] 源码注释明确 provider 分类不是硬件状态观测，LS2K clock 语义仍为
  `UNVERIFIED_ON_HARDWARE`。

### 验证证据

- 2K1000 驱动 host 单测 75 项全部通过；新增测试验证 fixed regulator 不会因 compatible
  存在就被误判为已上电，并覆盖 always-on 与 unsupported provider。
- topology fixture 端到端断言 LS2K clock MMIO、fixed-regulator flags，以及当前参考板四项
  prerequisite 均为 requires-driver。
- fixture 新增过短 clock MMIO 和带值的 regulator boolean 两个畸形 DTB，均被拒绝。
- `cargo check --no-default-features --features loongson2k1000la,final_online,heap-tlsf
  --target loongarch64-unknown-none` 通过。
- `make kernel-la` 通过；仅有仓库既有 warning。
- `git diff --check` 通过；fixture 临时 DTB 位于 `mktemp` 并自动删除，未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：`0x1fe00480/0x58` clock provider 的寄存器位域、门控位、
  输入频率和固件初始状态尚未验证。
- [ ] `FirmwareMaintained` 仅表示 DT 合同，不等于本次启动已测得 rail 为高电平。
- [ ] 当前参考 fixture 的两个 fixed regulator 没有 always-on/boot-on，因此仍为
  requires-driver；这符合 fail-closed 策略。
- [ ] GPIO22 provider 类型和 MMIO 尚未进入 card-detect readiness；本批没有读取 GPIO。
- [ ] 未解析 pinctrl、write-protect、voltage ranges 或 clock rate parent tree。
- [ ] 未减少任何 activation blocker；没有执行 MMC command、DMA、IRQ activation 或硬件写。
- [ ] 下一批应建立 LS2K clock 的纯寄存器模型与只读 snapshot：先根据可引用文档确认位域，
  用 mock 覆盖 offset/掩码/频率计算；target volatile backend 不接入 machine init。

### 提交

- `[feat] model 2K1000 MMC prerequisites`

## 2026-08-10：批次 63——LS2K1000 APB clock read-only snapshot model

### 任务与设计

1. 核对 Linux 主线 LS2K clock driver、DT binding、clock ID header 和 MMC binding。
2. 只实现上游明确描述的 2K1000 MMC parent clock 链，不推测不存在的 MMC gate。
3. 使用可 mock `RegisterIo` 固定读取 DC PLL、GMAC divider、APB scale，保存 raw evidence
   并计算各级频率。
4. target-only volatile backend 只读且不接入 machine init；snapshot 不解除 activation blocker。
5. 修正 fixture 中无语义的 clock ID 0，并补齐 binding 要求的 100 MHz reference parent。

上游证明 2K1000 MMC 消费 `LOONGSON2_APB_CLK = 12`。其父链为 100 MHz reference →
DC PLL (`0x20`) → GMAC divider (`0x28`) → APB scale (`0x50`)。2K1000 clock table 没有
MMC/eMMC 专用 gate；ID 31 的 eMMC clock 属于其他 Loongson-2 变体，不能套用于 2K1000。

### 完成内容

- [x] 新增 `clock` 模块及 read-only `ClockSnapshot`，同时返回三个 raw register 和四级 rate。
- [x] DC PLL 按 multiplier `[41:32]`、divisor `[31:26]` 计算并拒绝零字段。
- [x] GMAC divider 按 `[27:22]` 及 Linux one-based/allow-zero 语义计算。
- [x] APB scale 按 `[22:20] + 1`、parent × scale / 8 计算。
- [x] 固定读取顺序和宽度：`0x20/64`、`0x28/32`、`0x50/64`。
- [x] `snapshot_provider()` 从 topology provider 获取 reference rate；unsupported provider 不读取。
- [x] target-only `VolatileRegisters` 检查 base、8-byte alignment、0x58 window 和每次访问范围。
- [x] topology 要求 MMC clock ID 12、单个 `ref_100m` parent、fixed-clock 和非零 frequency。
- [x] fixture 新增 100 MHz fixed-clock，APBDMA/MMC consumer 改用 ID 12。
- [x] 新增 `docs/references/loongson2-clock-upstream.md`，记录来源、SPDX 与实现边界。
- [x] 没有 clock 写接口、enable/disable、PLL lock 轮询或 machine-init 自动 snapshot。

### 验证证据

- 2K1000 驱动 host 单测 79 项全部通过；4 项 clock 测试覆盖 rate chain、固定读取顺序、
  allow-zero bypass、三处 IO failure、零 PLL 字段、topology reference 和 unsupported fail-closed。
- topology fixture 端到端断言 clock ID 12、100 MHz reference 和 0x58 provider window。
- fixture 新增错误 MMC clock ID 与缺 reference parent 两个拒绝场景；既有短 clock window、
  截断 DMA、非法 regulator flag 等场景继续通过。
- `cargo check --no-default-features --features loongson2k1000la,final_online,heap-tlsf
  --target loongarch64-unknown-none` 通过，覆盖 volatile backend。
- `make kernel-la`、`git diff --check` 通过；仅有仓库既有 warning。
- 所有 DTB 仍在 `mktemp` 中生成并自动删除；没有创建镜像或执行硬件访问。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：64-bit volatile read、固件寄存器初值、PLL lock/stability、
  parent ownership 与测得输出频率均未在 2K1000 板上确认。
- [ ] snapshot 是瞬时三次读取，不保证跨寄存器原子一致；clock owner 并发改频时可能混合世代。
- [ ] WaterOS 尚无通用 clock framework，本模块只服务 2K1000 诊断与后续 MMC bring-up。
- [ ] 本批没有 clock enable 写，因为 2K1000 上游表没有 MMC 专用 gate，APB 是否由固件持续
  开启必须通过真实 DT/固件和板上观测确认。
- [ ] `ClockControlUnavailable` blocker 保持不变；snapshot 成功也不会自动激活 MMC。
- [ ] 下一批应实现 LS2K GPIO card-detect 的只读模型：解析 provider MMIO 与 active-low flags，
  用 mock 验证方向/输入采样和极性；volatile snapshot 不接入自动启动。

### 参考与许可证

- `docs/references/loongson2-clock-upstream.md`

### 提交

- `[feat] add LS2K1000 clock diagnostics`

## 2026-08-10：批次 64——LS2K1000 GPIO card-detect read-only model

### 任务与设计

1. 核对 Linux 主线 LS2K GPIO driver、DT binding 和参考板 MMC `cd-gpios` 描述。
2. 在 topology 中保存 GPIO provider MMIO、有效 GPIO 数、pin 与 active-low flag。
3. 建立可 mock 的只读方向/输入 snapshot；GPIO 不是输入时 fail-closed，绝不改方向。
4. 拒绝越界 pin、未知 flags、不足 MMIO window 和不合法 provider 描述。
5. target-only volatile backend 不接入 machine init，不提供方向、输出或中断写接口。
6. 本批只补齐 card-detect 的可诊断证据，不减少 MMC activation blocker。

Linux 主线 LS2K GPIO bank 使用 64-bit 寄存器，方向、输出、输入、中断使能偏移分别为
`0x00`、`0x10`、`0x20`、`0x30`，方向位为 1 表示 input。参考板 GPIO22 的
`GPIO_ACTIVE_LOW` 被保留为 topology 数据，并在纯逻辑层转换为 card-present 状态。

### 完成内容

- [x] `CardDetect::Gpio` 改为带类型的 `GpioLineDescription`，保存原始 specifier、provider、
  pin 与 polarity。
- [x] 识别 `loongson,ls2k1000-gpio` + `loongson,ls2k-gpio` provider，要求单个非零、
  8-byte 对齐且至少 `0x28` 的 MMIO window，以及 `ngpios` 在 1..=64。
- [x] `cd-gpios` 要求恰好两个参数，flags 只接受 active-high 0 或 active-low 1，pin 必须
  小于 provider 的 `ngpios`。
- [x] 新增 `gpio` 模块与 `CardDetectSnapshot`，依次读取 direction `0x00` 和 input
  `0x20`，同时保留 raw register、pin、polarity、level 和 card-present 证据。
- [x] direction 位为 0 时返回 `NotInput`，且不读取 input；unsupported/out-of-range provider
  在任何 MMIO 访问前失败。
- [x] target-only `VolatileRegisters` 检查 base、alignment、window 和每次 64-bit 访问范围；
  模块没有任何写方法。
- [x] topology fixture 补充兼容串与 `ngpios = <64>`，验证器断言 GPIO22、active-low、
  `0x1fe00500/0x38` 和 64-line provider。
- [x] 新增 `docs/references/loongson2-gpio-upstream.md`，记录来源、SPDX 与实现边界。

### 验证证据

- 2K1000 驱动 host 单测 83 项全部通过；4 项 GPIO 测试覆盖 active-low/high、固定读取顺序、
  非 input 提前失败、unsupported/越界零读取和两处 IO failure。
- topology fixture 端到端通过；新增 pin 64 越界、flags 2、GPIO MMIO 仅 `0x20` 三个畸形
  DTB，均被拒绝。截断 DMA fixture 仍仅产生预期的 dtc warning。
- `cargo check --no-default-features --features loongson2k1000la,final_online,heap-tlsf
  --target loongarch64-unknown-none` 通过，覆盖 volatile backend。
- `make kernel-la`、`git diff --check` 通过；仅有仓库既有 warning。
- fixture DTB 位于 `mktemp` 并自动删除；没有创建镜像、访问硬件或执行 GPIO 写。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：64-bit volatile read、GPIO22 mux、方向位语义、输入电平、
  卡座极性和插拔稳定性尚未在 2K1000 板上确认。
- [ ] snapshot 是两次非原子读取；方向或 pinmux 并发变化时可能混合状态，没有 debounce。
- [ ] topology fixture 仅建模 card-detect 所需的只读资源，没有宣称完整验证 Linux binding
  要求的 gpio-ranges、interrupt controller 或 pinctrl 配置。
- [ ] unsupported provider 可被 topology 保留用于诊断，但 snapshot 必定在零 MMIO 访问前拒绝。
- [ ] GPIO 处于 output 时不会尝试纠正；这是避免在未知板级连接上写寄存器的安全边界。
- [ ] `CardDetectControlUnavailable` blocker 保持不变；成功 snapshot 也不会启动 MMC host。
- [ ] 下一批应把 clock 与 GPIO snapshot 接入显式、只读的 MMC prerequisite diagnosis，使用
  mock backend 验证组合状态；仍不接入自动 machine init，也不执行 power/clock/GPIO 写。

### 参考与许可证

- `docs/references/loongson2-gpio-upstream.md`

### 提交

- `[feat] add LS2K1000 card-detect diagnostics`

## 2026-08-10：批次 65——LS2K1000 MMC combined prerequisite diagnosis

### 任务与设计

1. 组合既有 clock 与 GPIO card-detect 只读 snapshot，不复制 provider 或寄存器模型。
2. 诊断结果必须保留完整 `BringUpPlan`，snapshot 成功不能解除任何 activation blocker。
3. clock 与 GPIO 使用两个独立、可注入 backend；错误作为证据保留，不互相短路。
4. non-removable、broken、native 和 GPIO card-detect 返回不同语义，不猜测未观测状态。
5. target-only 显式 volatile 入口只为支持的 provider 构造 MMIO backend，不接入 machine init。
6. 静态 plan 无效时在任何寄存器读取前失败；unsupported provider 必须零读取 fail-closed。

### 完成内容

- [x] 新增 `mmc_diagnostic` 模块、`Diagnosis` 和 `CardDetectDiagnosis`。
- [x] `Diagnosis` 同时保存 `BringUpPlan`、`Result<ClockSnapshot, ClockError>` 与 card-detect
  证据，调用者能区分静态拓扑、瞬时观测和具体读取错误。
- [x] GPIO snapshot 失败不掩盖 clock snapshot，clock 失败也不阻止安全的 GPIO 读取。
- [x] non-removable 返回 topology evidence；broken 返回 firmware-maintained 标记；native
  明确为 unavailable；三者均不读取 GPIO。
- [x] `diagnose_volatile()` 先验证静态 plan，再按 topology 构造只读 clock/GPIO backend。
- [x] target unsupported/unused backend 使用纯软件枚举哨兵，不构造占位物理地址。
- [x] API 注释明确物理读取仍为 `UNVERIFIED_ON_HARDWARE`，machine init 没有调用诊断入口。
- [x] topology 端到端示例使用 mock 寄存器，从 DT 描述计算 250 MHz APB clock、识别
  active-low GPIO22 插卡，同时断言 `can_activate() == false`。

### 验证证据

- 2K1000 驱动 host 单测 88 项全部通过；新增 5 项组合测试覆盖成功证据、独立双错误、
  三种 topology-only card mode、unsupported provider 零读取、invalid plan 零读取。
- topology fixture 与全部畸形 DTB 场景通过；有效 fixture 额外执行组合诊断 mock。
- `cargo check --no-default-features --features loongson2k1000la,final_online,heap-tlsf
  --target loongarch64-unknown-none` 通过，覆盖 target-only backend。
- `make kernel-la`、`git diff --check` 通过；仅有仓库既有 warning。
- 测试没有创建镜像、访问物理 MMIO 或执行 clock/GPIO/MMC 写。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：组合入口未在 2K1000 板执行；clock raw/rate 与 GPIO raw/level
  都只是 host mock 和上游文档所支持的预期。
- [ ] 多个寄存器按顺序读取，不是跨 clock/GPIO 原子 snapshot；结果可能跨越硬件状态变化。
- [ ] power rail 仍只有 topology ownership 分类，没有安全的只读 regulator 电平观测。
- [ ] native card-detect 尚无经上游证明的只读寄存器模型；broken-cd 也不等于实际存在卡。
- [ ] 显式 volatile 入口要求调用者保证设备映射与独占读取，目前没有公开远程命令触发它。
- [ ] 六个 activation blocker 全部保留；本批不表示 MMC data path、DMA、power、IRQ 可用。
- [ ] 下一批应将组合结果接入现有 remote debug monitor 的只读命令与稳定文本 formatter，
  只允许显式触发，并对 unsupported profile、并发 reservation、错误输出做 host 测试。

### 提交

- `[feat] combine LS2K1000 MMC diagnostics`

## 2026-08-10：批次 66——Remote `ls2k-mmc` read-only diagnosis command

### 任务与设计

1. 在 development-only TCP monitor 增加严格无参数的 `ls2k-mmc` 命令。
2. 2K1000 profile 只在收到命令时执行 clock/GPIO 组合 snapshot；其他 profile 零 MMIO 访问。
3. 使用非阻塞 one-shot gate 排除并发物理读取，busy 立即失败而不是等待或重入。
4. 从 topology 锁内只复制唯一 MMC 描述；MMIO 读取、格式化和网络发送均在锁外完成。
5. 成功与部分失败使用固定单行字段顺序和稳定错误码，同时保留 `can_activate=0` 与 blocker 数。
6. 不增加认证、写命令、自动 snapshot 或 MMC activation；真机行为继续标记未验证。

### 完成内容

- [x] monitor parser/help/dispatch 增加 `ls2k-mmc`，命令响应不会关闭会话。
- [x] 新增 `DiagnosticGate`，基于 atomic compare-exchange；guard drop 总能重新开放 gate。
- [x] `diagnose_mmc_once()` 在 gate 内复制 topology 描述后释放锁，再调用显式 volatile diagnosis。
- [x] topology 未初始化、host 数不是 1、busy、invalid plan、clock/GPIO backend 构造失败均映射为
  稳定 facade 错误。
- [x] formatter 输出 clock raw/reference/APB rate、GPIO raw/pin/polarity/level/present、
  `can_activate` 和 blocker 数；clock/GPIO 部分错误独立保留。
- [x] overall 错误使用 `busy`、`topology-unavailable`、`invalid-host-count`、`invalid-plan`、
  `clock-backend`、`gpio-backend`，不暴露 Rust `Debug` 名称。
- [x] QEMU/非 2K1000 profile 固定返回
  `ERR unsupported: ls2k-mmc requires loongson2k1000la`，不会链接物理诊断入口。
- [x] remote debug Python smoke client 现在发送并校验 `ls2k-mmc`，接受成功、诊断错误、
  target unavailable 和 profile unsupported 四类前缀。

### 验证证据

- 2K1000 驱动 host 单测 89 项全部通过；gate 测试覆盖重入 busy 与 drop 后恢复，既有组合测试
  新增成功/双错误响应的逐字节稳定格式断言。
- remote client 与 QEMU launcher 的 13 项 Python host 测试通过，socketpair 会话包含
  `ls2k-mmc` unsupported 响应及后续 `quit`。
- 2K1000 + `remote-debug-monitor` 精确 LoongArch target check 通过，覆盖真实 MMIO 命令分支。
- QEMU LoongArch + `remote-debug-monitor` 精确 target check 通过，覆盖 unsupported 分支。
- `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 通过；仅有仓库既有 warning。
- LoongArch QEMU TCP smoke 端到端通过：使用 1 MiB 临时稀疏 raw 盘和 snapshot 模式，实际收到
  `ping`、`status`、`version`、`ls2k-mmc` unsupported、`quit`；临时盘退出时截断为 0 字节。
- 顶层 host `cargo test` 仍受默认 RISC-V SBI inline-asm/未选择 platform-arch 实现限制；本批把
  formatter 放在可独立 host 测试的 2K1000 crate，并用 target check + QEMU smoke 覆盖顶层协议。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：真实 2K1000 尚无 NIC 驱动，未通过 TCP 执行物理 clock/GPIO 读取。
- [ ] monitor 无认证和加密，只允许在隔离开发网络使用；`remote-debug-monitor` 继续默认关闭。
- [ ] one-shot gate 只排除同一诊断入口；调用者仍须保证其他固件/驱动不会并发改 clock/GPIO。
- [ ] snapshot 跨多个非原子寄存器读取，输出不是一致性事务，也没有 GPIO debounce。
- [ ] 成功输出可能超过输入 `MAX_LINE_LEN`；`send_all` 支持分段发送，Python 客户端按 prompt 聚合。
- [ ] remote 命令只报告证据；六个 MMC activation blocker 保持不变，没有执行任何硬件写。
- [ ] 下一批应重新核对 Linux fixed-regulator 无 GPIO 时的 ownership/enable 语义，建立只读 power
  prerequisite 证据或修正当前保守分类；任何 rail 控制写仍需推迟到可证明板级连接之后。

### 提交

- `[feat] expose LS2K1000 MMC diagnostics remotely`

## 2026-08-10：批次 67——校正 LS2K1000 MMC supply readiness

### 任务与设计

1. 依据 Linux 主线 fixed-regulator binding/driver、MMC regulator core 与 2K1000 reference DTS，核对供电所有权语义。
2. 将“未声明 supply”“显式无控制 fixed rail”“GPIO-controlled fixed rail”“unsupported provider”分开建模。
3. `always-on`/`boot-on` 只作为策略证据保留，不用于猜测 GPIO-controlled rail 已真实开启。
4. fixture 同时覆盖合成 fixed-regulator 与上游参考板省略 supply 的形态，并拒绝歧义控制属性。
5. remote `ls2k-mmc` 稳定输出增加 `vmmc`/`vqmmc` 前置条件，但不解除任何 activation blocker。

### 完成内容

- [x] 新增 `FixedSupplyControl::{None,Gpio}`，把控制能力与 `always_on`/`boot_on` 策略正交表示。
- [x] 未声明 optional supply 分类为 `ImplicitBoardSupply`，与 Linux MMC core 和主线 2K1000 reference DTS 一致。
- [x] 显式无 GPIO 的 fixed regulator 分类为 topology-ready；GPIO-controlled rail 仍为 requires-driver。
- [x] 同时声明 `gpio` 与 `gpios`、带值的 boolean policy 均 fail-closed 为 invalid DTB。
- [x] 合成 fixture 补齐 regulator-name 与固定 3.3 V min/max；注释明确它不是上游 2K1000 板级事实。
- [x] topology 脚本生成 upstream-shaped 无 supply 变体，断言两路均为 implicit board supply 且 host 仍不可激活。
- [x] remote formatter 固定输出 `vmmc=... vqmmc=...`，状态码不依赖 Rust Debug 名称。
- [x] 新增 `docs/references/linux-mmc-power-upstream.md`，记录一手来源与 WaterOS 实现边界。

### 验证证据

- 2K1000 驱动 host 单测 89 项全部通过；供电分类覆盖隐式板级、无控制 fixed、GPIO fixed 与 unsupported。
- topology fixture/畸形 DTB 矩阵通过；新增 upstream-shaped 无 supply 与双 GPIO 属性场景。dtc 仅报告刻意构造输入的预期 warning。
- `cargo check --no-default-features --features loongson2k1000la,final_online,heap-tlsf,remote-debug-monitor --target loongarch64-unknown-none` 通过。
- `make kernel-la EXTRA_FEATURES=remote-debug-monitor`、`git diff --check` 通过；仅有仓库既有 warning。
- 测试没有访问物理 MMIO、执行 regulator/GPIO/MMC 写或创建持久镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：implicit/no-control 只证明软件没有 enable 操作，不证明插槽实际有 3.3 V、rail 稳定或上电时序正确。
- [ ] `PowerSequencingUnavailable` 与其余 MMC activation blocker 全部保留；本批没有启动 host 或数据通路。
- [ ] GPIO-controlled fixed rail 没有安全的状态观测与写路径，即使 DTS 声明 always-on/boot-on 也保持 requires-driver。
- [ ] 主线 2K1000 reference DTS 是参考板证据，不保证两块目标板的原理图完全相同；拿到板级 DTS/原理图后必须重新核对。
- [ ] remote 状态是 topology ownership 证据，不是电压测量；真机验收仍需万用表/示波器或经验证的 PMIC/regulator 状态源。
- [ ] 下一批应审计 MMC pinctrl/pinmux ownership 与主线 2K1000 reference DTS，建立只读/拓扑诊断；任何 pinmux 写继续推迟到板级连接可证明之后。

### 参考与许可证

- `docs/references/linux-mmc-power-upstream.md`

### 提交

- `[fix] correct LS2K1000 MMC supply readiness`

## 2026-08-10：批次 68——LS2K1000 MMC pinctrl topology 与只读诊断

### 任务与设计

1. 依据 Linux 主线 2K1000 reference DTS、pinctrl binding/driver 与 device core 核对 MMC pinmux 所有权。
2. 解析唯一 `default` state，严格验证 `sdio -> sdio` 与 card-detect 所需 `pwm2 -> gpio` 两个映射。
3. 将 pinctrl 作为独立 prerequisite；缺失、受支持 provider、未知 provider 必须有不同状态。
4. 增加只读单寄存器 snapshot，报告 SDIO 与 GPIO22 mux 瞬时状态，但不自动修复或解除 blocker。
5. 扩充 remote 稳定输出、畸形 DTB、host mock、LoongArch target 与整核构建验证。

### 完成内容

- [x] `MmcDescription` 新增 `MmcPinctrlDescription`，保存 state phandle 与 provider/MMIO 证据。
- [x] topology 仅接受 `pinctrl-names = "default"`、单个 `pinctrl-0` 和主线已证明的两个映射；引用不存在、重复/额外映射、禁用 provider、短 MMIO 均 fail-closed。
- [x] `PrerequisitePlan` 新增 `pinctrl`；无 state 为 missing，Loongson provider 为 requires-driver，未知 provider 为 unsupported-provider。
- [x] `BringUpPlan` 新增第七个 `PinControlUnavailable` blocker；`can_activate()` 继续恒为 false。
- [x] 新增 `pinctrl` 模块，以单次只读 32-bit access 解码首个 mux 寄存器 bit20(SDIO) 与 bit14(PWM2/GPIO22)。
- [x] missing/unsupported provider 保证零读取；volatile backend 仅在显式 `ls2k-mmc` 诊断中构造，machine init 不读取或写入 pinctrl。
- [x] remote formatter 新增 topology `pinctrl=...` 与瞬时 `pinmux=ok/error` 字段；backend 构造错误有稳定 `pinctrl-backend` 错误码。
- [x] fixture 对齐主线 reference state；新增缺失 state、未知 provider、错误 card-detect function 和短 pinctrl MMIO 变体。
- [x] 新增 `docs/references/linux-ls2k-pinctrl-upstream.md`，记录一手来源、SPDX 与实现边界。

### 验证证据

- 2K1000 驱动 host 单测 92 项全部通过；新增 3 项覆盖 prerequisite 分类、三组 bit 状态、IO error 与 missing/unsupported 零读取。
- topology fixture/畸形 DTB 矩阵通过；有效 fixture 的组合诊断 mock 读出 raw `0x100000`，确认 SDIO=1、card GPIO=1。
- remote client 与 QEMU launcher 的 13 项 Python host 测试通过。
- `cargo check --no-default-features --features loongson2k1000la,final_online,heap-tlsf,remote-debug-monitor --target loongarch64-unknown-none` 通过。
- `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 与 `git diff --check` 通过；仅有仓库既有 warning。
- 测试没有访问物理 MMIO、执行 pinmux/MMC 写或创建持久磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：`0x1fe00420` 的 endian、bit20/bit14 语义及映射可访问性尚未在 2K1000 板实测。
- [ ] snapshot 是一次瞬时读取，不能证明 boot firmware/其他核心随后不改 mux，也不能证明引脚电气状态正确。
- [ ] snapshot 即使报告 SDIO=1、card_gpio=1，也不会解除 `PinControlUnavailable` 或其他六个 blocker。
- [ ] 本批没有实现 Linux 驱动的 locked read-modify-write；在拿到目标板 DTS/原理图并验证寄存器前禁止自动修复。
- [ ] 两块目标板可能与主线 reference board 的 GPIO22/card-detect 连接不同，必须以各自板级 DTS/原理图复核。
- [ ] remote monitor 仍依赖尚未完成的 2K1000 NIC；真机前只能验证 formatter、facade 和 target 链接路径。
- [ ] 下一批应审计并实现保守的 pinctrl activation typestate：先 snapshot、只允许已满足状态通过；实际 RMW 写路径需独立 feature/policy gate，并继续默认关闭。

### 参考与许可证

- `docs/references/linux-ls2k-pinctrl-upstream.md`

### 提交

- `[feat] add LS2K1000 MMC pinctrl diagnostics`

## 2026-08-10：批次 69——LS2K1000 MMC pinctrl activation typestate

### 任务与设计

1. 在只读 pinmux snapshot 之上建立 `Observed -> Ready/NeedsTransition` 类型状态。
2. 只有同时观测到 SDIO bit20=1、PWM2/GPIO22 bit14=0 才能产生 opaque `Ready` token。
3. 不满足时生成最小纯软件 transition plan，描述 set/clear mask 与期望 raw，但不提供硬件写入口。
4. 防止外部伪造 snapshot；IO、missing、unsupported provider 均不能产生 token。
5. remote 输出增加稳定 `ready=0/1` 证据，所有 activation blocker 保持不变。

### 完成内容

- [x] 新增 `PinctrlState<S>`、`Observed`、`Ready` 与 `NeedsTransition` typestate。
- [x] `PinctrlSnapshot` 字段改为私有，只能通过经过 provider 检查的 snapshot 路径产生；对外提供只读 accessor。
- [x] `classify()` 直接依据 raw bit 分类，不信任重复布尔字段；只有完整满足状态返回 `PinctrlState<Ready>`。
- [x] `NeedsTransition::transition_plan()` 生成 `set_mask`、`clear_mask`、`original_raw`、`desired_raw`，不执行 MMIO。
- [x] transition plan 只允许置 bit20、清 bit14，保留全部无关位。
- [x] `ls2k-mmc` 的 `pinmux=ok` 段新增 `ready=`，错误输出仍保留既有稳定错误码。
- [x] API 注释明确 ready token 是瞬时证据，不授予写权限，也不解除 `PinControlUnavailable`。

### 验证证据

- 2K1000 驱动 host 单测 94 项全部通过；新增 2 项覆盖 opaque ready token、两类不满足状态和无关位保持。
- transition fixture 验证 raw 同时缺失 SDIO/错误选择 PWM2 时，计划精确置 bit20、清 bit14，其他 bit xor 为 0。
- topology fixture/畸形 DTB 矩阵通过；有效 mock 的 remote formatter 精确输出 `ready=1`。
- remote client 与 QEMU launcher 的 13 项 Python host 测试通过。
- `cargo check --no-default-features --features loongson2k1000la,final_online,heap-tlsf,remote-debug-monitor --target loongarch64-unknown-none` 通过。
- `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 与 `git diff --check` 通过；仅有仓库既有 warning。
- 测试未访问物理 MMIO、未执行 pinmux 写、未创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：ready token 只证明一次 volatile read 的解释，真实 endian、bit 语义和引脚电气状态仍未验证。
- [ ] token 不提供时间稳定性；firmware、其他 core 或驱动可能在 snapshot 后修改同一 shared mux register。
- [ ] `TransitionPlan` 只是上游派生的数学计划，尚未连接 write/readback、锁或 rollback，因此不能用于真机激活。
- [ ] 七个 MMC blocker 全部保留，`can_activate()` 仍恒为 false。
- [ ] 两块目标板仍需各自 DTS/原理图确认 SDIO 和 GPIO22 不与其他外设冲突。
- [ ] 下一批应实现显式、默认关闭的 pinctrl transaction abstraction：锁内 read/conditional-RMW/readback，失败时保留可恢复状态；volatile 写后端继续与自动 machine init 隔离。

### 参考与许可证

- `docs/references/linux-ls2k-pinctrl-upstream.md`

### 提交

- `[feat] gate LS2K1000 pinctrl readiness`

## 2026-08-10：批次 70——显式 LS2K1000 pinctrl transaction 与恢复契约

### 任务与设计

1. 为 `NeedsTransition` 增加显式 fresh-read、conditional RMW、readback transaction。
2. 写路径必须同时持有唯一全局 transaction guard 与 unsafe board authority；普通诊断不可达。
3. preflight 使用最新 raw 重新计算 mask，避免陈旧 snapshot 覆盖期间变化的无关位。
4. write failure 一律视为效果未知；preflight/readback IO 与 mismatch 返回分阶段 recovery evidence。
5. recovery 只允许显式只读 revalidate，不猜测写是否生效，也不自动重试写入。

### 完成内容

- [x] 新增 `WriteRegisterIo`，与既有只读 `RegisterIo` 分离；remote diagnosis 继续只持有只读 backend。
- [x] 新增 `TransitionAuthority::assume_board_verified()`；只有 unsafe 调用者确认原理图/DTS、寄存器语义、独占权与恢复流程后才能构造。
- [x] 新增模块内唯一 `TRANSACTION_GATE` 与 `try_begin_transition()`；外部不能构造其他 gate/guard 绕过本地串行化。
- [x] `apply_transition()` 在 guard 内 fresh read；若已 ready 零写入，否则按 fresh raw 生成最小 desired value、写入并 readback。
- [x] 新增 `TransitionStage` 与 `TransitionRecovery`，覆盖 preflight read、write、readback、mismatch、revalidate read/mismatch。
- [x] write error 的 `observed_raw=None`，明确不能把失败解释为未写；成功必须由 readback 重新产生 opaque ready token。
- [x] `TransitionRecovery::revalidate()` 只读一次并重新分类；不会隐式再次写入。
- [x] 新增独立 `VolatileWriteRegisters`，target-only 且标为 `UNVERIFIED_ON_HARDWARE`；machine init、remote monitor 和 facade 均无调用点。
- [x] 上游参考文档补充 transaction 隔离、authority、guard 和 uncertain-write 边界。

### 验证证据

- 2K1000 驱动 host 单测 97 项全部通过；新增 3 项 transaction 测试覆盖 gate busy/drop、already-ready 零写、fresh RMW、write failure、preflight/readback IO、readback mismatch 与 revalidate。
- fresh raw 含无关 bit3/bit27 的 fixture 只改变 bit20/bit14；写值与 readback 顺序逐项断言。
- topology fixture/畸形 DTB 矩阵通过，证明新增写 abstraction 未改变只读 discovery/diagnosis 行为。
- `cargo check --no-default-features --features loongson2k1000la,final_online,heap-tlsf,remote-debug-monitor --target loongarch64-unknown-none` 通过，覆盖隔离的 volatile write backend 编译。
- `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 与 `git diff --check` 通过；仅有仓库既有 warning。
- 所有 transaction 测试均为内存 mock；没有访问物理 MMIO、没有真实 pinmux 写、没有创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：volatile write、readback ordering、device-memory 属性、endian 与 bit20/bit14 语义尚未在 2K1000 板验证。
- [ ] 全局 gate 只排除 WaterOS 内这一 API 的并发事务，无法阻止 boot firmware、管理核心或未遵守 API 的代码修改 shared mux register。
- [ ] authority 是显式 unsafe 契约，不是运行时硬件身份认证；两块目标板必须分别核对 DTS/原理图后才能创建。
- [ ] mismatch 不尝试 rollback，因为恢复原 raw 可能覆盖并发修改；当前策略是保留证据并要求重新观测。
- [ ] transaction 成功只产生 pinctrl ready token，七个 MMC blocker 与 `can_activate()==false` 保持不变。
- [ ] machine init 与远程 monitor 没有写入口；本批不代表 pinctrl 已在物理机激活。
- [ ] 下一批应将 pinctrl ready token 纳入一个聚合的 MMC prerequisite proof，要求 clock/power/card-detect/IRQ 各自 token 全部存在后才能进入 controller activation typestate；仍不启动 data path。

### 参考与许可证

- `docs/references/linux-ls2k-pinctrl-upstream.md`

### 提交

- `[feat] add recoverable LS2K1000 pinctrl transaction`

## 2026-08-10：批次 71——聚合 LS2K1000 MMC prerequisite proof

### 任务与设计

1. 审计 clock、vmmc/vqmmc、pinctrl、card-detect 与 IRQ 的现有证据，区分 topology、瞬时观测和真机验证。
2. 建立六项独立 gate 的聚合报告；任何单项 snapshot 都不能被误当作 controller activation 权限。
3. 用 opaque typestate token 表达经真机验证的 clock、power、pinctrl、card 与 IRQ 证据，只有全部存在才能组装 proof。
4. remote `ls2k-mmc` 输出稳定 gate code 与 `proof=0/1`，同时保留既有 bring-up blocker。
5. 不启动 MMC controller、DMA 或数据通路，也不增加新的硬件写调用点。

### 完成内容

- [x] 新增 `mmc_prerequisite` 模块和 `PrerequisiteReport`，分别报告六项 gate，只有全部为 `Satisfied` 才允许 `can_form_proof()`。
- [x] clock rate snapshot 分类为 `ObservedOnly`；它不证明时钟可控、目标频率已稳定或物理输出正确。
- [x] implicit/fixed supply topology 分类为 `UnverifiedOnHardware`；软件无需 enable 不等于真实 rail 电压及时序已验证。
- [x] pinctrl 只有既有 opaque `PinctrlState<Ready>` 可分类为 satisfied；card-detect 只有 non-removable 或 GPIO input 报告 present 可满足本地 gate。
- [x] diagnostic IRQ runtime 即使已配置 IRQ31 且软件状态干净，也只分类为 `ObservedOnly`，不冒充真实投递、设备 ack、mask/rearm 证明。
- [x] 新增 `ClockReady`、`PowerReady`、`CardReady`、`IrqReady` 与 `ControllerPrerequisiteProof`；需要真机证据的 token 没有普通构造器。
- [x] proof 只代表 controller 前置条件齐备，注释明确它仍不是 data-path token。
- [x] remote facade 合并当前 IRQ software snapshot，稳定输出六项 gate 和 `proof`；当前正常诊断预期仍为 `proof=0`。
- [x] 七个 MMC activation blocker 与 `can_activate()==false` 保持不变；machine init、remote monitor 均未获得 controller 写入口。

### 验证证据

- 2K1000 驱动 host 单测 101 项全部通过；新增 4 项覆盖健康观测仍不能形成 proof、各 gate fault matrix、IRQ observation 降级和完整 typed-token 组装。
- formatter 精确字符串测试覆盖健康与多错误场景，确认稳定 gate code、`proof=0` 和既有 blocker 数量。
- topology fixture/畸形 DTB 矩阵通过；dtc 输出仅为刻意构造畸形输入的预期 warning。
- remote client 与 QEMU launcher 的 13 项 Python host 测试通过。
- `cargo check --no-default-features --features loongson2k1000la,final_online,heap-tlsf,remote-debug-monitor --target loongarch64-unknown-none` 通过。
- `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 与 `git diff --check` 通过；仅有仓库既有 warning。
- 所有新增测试均为 host fixture/software snapshot；没有访问物理 MMIO、执行硬件写或启动 MMC 数据通路。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：两块目标板的 MMC clock 稳定性、vmmc/vqmmc 电压与上电时序、真实 card-detect 电气状态均未验证。
- [ ] live diagnostic IRQ 只能证明 WaterOS 软件 runtime 中配置了 IRQ31；尚未证明真实 MMC 中断投递、W1C ack、mask/rearm 和异常恢复。
- [ ] unsafe `assume_verified` 构造器是显式审计边界，不是硬件身份认证；只能在逐板完成物理验证后调用。
- [ ] pinctrl ready 仍是瞬时寄存器证据；shared mux 的并发修改、endian、bit 语义和引脚电气状态需真机复核。
- [ ] `ControllerPrerequisiteProof` 没有生产调用点，也不解除七个 blocker；当前输出 `proof=0` 是预期安全状态。
- [ ] 下一批优先审计并建立保守的 MMC clock-control transaction/typestate：先证明允许频率、fresh-read/RMW/readback 与恢复边界；真实写入口继续隔离并标记未在硬件验证。

### 提交

- `[feat] aggregate LS2K1000 MMC prerequisites`

## 2026-08-10：批次 72——LS2K1000 MMC parent clock 一致性证据

### 任务与设计

1. 复核 Linux 主线 LS2K clock 与 MMC host 对 parent clock 的 ownership、rate 和 prescaler 处理。
2. 判断是否存在可安全用于 MMC 的 DC PLL/GMAC/APB 写路径；证据不足时必须拒绝共享时钟 RMW。
3. 建立两轮完整 snapshot 的只读一致性事务，保留 first/second read、mismatch 和 IO failure 的恢复证据。
4. 让显式 `ls2k-mmc` 诊断使用一致性事务，拒绝输出混合世代频率。
5. 收紧聚合 proof：clock hardware-ready 的 unsafe 边界至少要求 opaque consistent snapshot，而非任意单次 snapshot。

Linux 主线 MMC host 对 parent clock 调用 `devm_clk_rate_exclusive_get()` 并读取当前 rate，实际调频只在
MMC 控制器内部使用 8-bit prescaler；LS2K clock table 没有 MMC-private gate/rate control。DC PLL、
GMAC divider 和 APB scale 是共享系统时钟链，因此本批不增加 provider 写接口，避免影响其他 APB 设备。

### 完成内容

- [x] 新增 opaque `ConsistentClockSnapshot`；只有连续两轮 DC PLL/GMAC/APB raw 与派生 rate 完全一致才能构造。
- [x] 新增 `ConsistencyStage::{FirstRead,SecondRead,Mismatch}` 与 `ConsistencyRecovery`，错误保留已成功读取的 snapshot，不编造后续证据。
- [x] recovery 提供显式只读 `revalidate()`；不会写时钟、自动重试控制操作或丢弃上一轮证据。
- [x] 新增 topology-aware `snapshot_provider_consistent()`；unsupported provider 保证零 MMIO access。
- [x] `ClockError` 增加稳定 `Inconsistent` 分类；remote formatter 输出 `clock=error:inconsistent`。
- [x] `ls2k-mmc` 显式诊断从三次读取改为两轮共六次固定顺序读取，raw 发生变化时不报告 rate。
- [x] `ClockReady::assume_verified()` 改为接受 opaque consistent snapshot；它仍为 unsafe，连续一致不等于物理稳定。
- [x] clock 模块和参考文档明确禁止为 MMC 修改共享 DC PLL/GMAC/APB 链；未增加 volatile write backend。
- [x] `ClockControlUnavailable`、其余六个 blocker 和 `proof=0` 保持不变，没有启动 MMC 数据通路。

### 验证证据

- 2K1000 驱动 host 单测 105 项全部通过；新增 4 项覆盖双轮固定读取顺序、一致 token、generation mismatch、first/second read failure、revalidate 与远程稳定错误码。
- mismatch fixture 在第二轮改变 GMAC divider，诊断正确返回 `ClockError::Inconsistent`，并保留两代 raw evidence。
- topology fixture/畸形 DTB 矩阵通过；dtc 仅输出刻意构造畸形输入的预期 warning。
- remote client 与 QEMU launcher 的 13 项 Python host 测试通过。
- `cargo check --no-default-features --features loongson2k1000la,final_online,heap-tlsf,remote-debug-monitor --target loongarch64-unknown-none` 通过。
- `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 与 `git diff --check` 通过；仅有仓库既有 warning。
- 测试仅使用内存寄存器模型；没有访问物理 MMIO、写共享时钟或创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：连续两次相同 raw 只能排除采样间可见变化，不能证明没有瞬态 glitch、PLL 已 lock 或物理输出频率正确。
- [ ] 三个寄存器的一轮 snapshot 本身仍非原子；两轮相同不能排除在两轮内部发生并恢复的变化。
- [ ] volatile 64-bit 读取宽度、endian、device-memory ordering 与 100 MHz reference 仍需逐板验证。
- [ ] WaterOS 不持有通用 clock framework ownership；禁止为 MMC 写共享 DC PLL/GMAC/APB 字段。
- [ ] unsafe `ClockReady` 仍要求真机验证稳定性和目标 rate；consistent snapshot 只是最低软件证据，不会自动产生 ready token。
- [ ] 下一批应审计并实现 MMC controller-private `PRE`/`CTL.ENCLK` transaction：验证 parent rate 可表示、prescaler 上取整/255 clamp、fresh read、write/readback、失败恢复及关钟边界；仍不执行命令或 DMA。

### 参考与许可证

- `docs/references/loongson2-clock-upstream.md`

### 提交

- `[feat] validate LS2K1000 MMC parent clock coherence`

## 2026-08-10：批次 73——LS2K1000 MMC controller-private clock transaction

### 任务与设计

1. 审计 Linux 主线 MMC `PRE=0x04`、`CTL=0x00` 的 divider、enable 与写入顺序。
2. 只控制 MMC controller-private prescaler/clock-enable，不修改共享 DC PLL/GMAC/APB parent。
3. 要求上一批的 opaque coherent parent evidence 才能生成 prescaler plan。
4. transaction 必须持有全局 guard 与 unsafe board authority，执行 fresh read、conditional write 和 readback。
5. 任何部分写入、IO error 或 readback mismatch 均返回分阶段 recovery evidence；恢复只读重新验证。

Linux 当前以 `DIV_ROUND_UP(parent, requested)` 计算 divider、clamp 到 255，写入
`PRE.EN | divider`，再只更新 `CTL.ENCLK`。虽然寄存器定义的 divider field 为 `[9:0]`，WaterOS
保守保持上游实际使用的 255 clamp，不推测 256..1023 在两块目标板上的行为。

### 完成内容

- [x] 新增 opaque `ControllerClockPlan`，只能由 `ConsistentClockSnapshot` 构造，并保存 parent、requested、divider 与 actual rate。
- [x] parent rate 超出 `u32` 或 requested rate 为零时 fail-closed；prescaler 继续使用上取整与 255 clamp。
- [x] 新增 `ControllerClockAuthority::assume_board_verified()`，显式承载板级 MMIO、ownership、parent stability 与恢复契约。
- [x] 新增模块内唯一 `CLOCK_TRANSACTION_GATE`；并发调用返回 busy，guard drop 后重新开放。
- [x] `apply_controller_clock()` fresh-read `PRE/CTL`；已满足时零写入，否则先写/读回 PRE，再对 CTL bit0 做保留无关位的 RMW/readback。
- [x] PRE 按上游语义写完整 `PRE.EN | divider`；CTL 只置 `ENCLK`，保留 fresh-read 的其他位。
- [x] 新增 `ControllerClockStage` 与 `ControllerClockRecovery`，覆盖 observe、preflight、两次 write/readback、mismatch 和 revalidate 阶段。
- [x] write error 的对应寄存器观测标为 unknown，不把错误解释为“写入未发生”；revalidate 只读 PRE/CTL。
- [x] 新增 `observe_controller_clock()`，允许对固件已配置状态做零写入 readback 验证。
- [x] 删除原公开 `Host::configure_clock(input_hz, target_hz)` 旁路，调用者不能绕过 parent token、guard、authority 和 readback。
- [x] `mmc_prerequisite::ClockReady` 改为必须接收 opaque `ControllerClockReady`；parent coherence 本身不再足以形成 clock proof。
- [x] machine init、remote diagnosis 没有 authority/guard 调用点；七个 blocker、`proof=0` 和 `can_activate()==false` 保持不变。

### 验证证据

- 2K1000 驱动 host 单测 107 项全部通过；新增 2 个 transaction 测试聚合覆盖 255 clamp、完整写序、CTL 无关位保留、already-ready 零写和 gate 生命周期。
- fault matrix 覆盖四个 read failure 点、PRE/CTL write failure、两处 readback mismatch 与恢复后只读 revalidate。
- 既有 command mock 改为先取得 controller clock token，再执行非数据命令；证明删除旧旁路后组合路径仍可编译测试。
- topology fixture/畸形 DTB 矩阵通过；dtc 仅输出刻意构造畸形输入的预期 warning。
- remote client 与 QEMU launcher 的 13 项 Python host 测试通过。
- `cargo check --no-default-features --features loongson2k1000la,final_online,heap-tlsf,remote-debug-monitor --target loongarch64-unknown-none` 通过。
- `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 与 `git diff --check` 通过；仅有仓库既有 warning。
- 所有新增写测试使用内存 MMIO model；没有访问物理寄存器、启动命令/DMA 或创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：真实 `PRE`/`CTL.ENCLK` 写入、readback、ordering、endian 与输出频率尚未在两块板验证。
- [ ] `ControllerClockReady` 只证明寄存器 readback，不证明时钟波形稳定、占空比正确或 SD 卡实际收到时钟。
- [ ] 255 clamp 会使高 parent rate 下最低可达时钟高于请求值；调用者必须读取 `actual_hz`，真机初始化策略仍需确认卡片容忍度。
- [ ] WaterOS-local gate 无法排除 boot firmware、另一核心或绕过 API 的代码并发修改控制器。
- [ ] 当前没有推测 clock-disable/rollback 顺序；在上游和硬件证据不足时，失败只允许重新观测，不盲目清 ENCLK/PRE.EN。
- [ ] `Host::execute_command()` 尚未在类型系统中强制消费 aggregate prerequisite proof；当前无生产调用点，但下一阶段必须封闭该旁路。
- [ ] 下一批应把 `Host` 重构为 typestate session：只有完整 aggregate proof 才能进入 command-capable 状态，并先实现 reset/idle/interrupt-clear 的有界 preflight；data command 与 DMA 继续禁用。

### 参考与许可证

- `docs/references/loongson2-mmc-upstream.md`
- `docs/references/loongson2-clock-upstream.md`

### 提交

- `[feat] add recoverable LS2K1000 MMC clock transaction`

## 2026-08-10：批次 74——LS2K1000 MMC Host typestate 与 reset preflight

### 任务与设计

1. 审计 Linux 主线在首条命令前的 reset、延时、外部时钟选择与中断清理顺序。
2. 将 Host 建模为 `Uninitialized → Preflighted → ClockConfigured → CommandReady`，封闭未初始化状态直接发命令的旁路。
3. 处理 reset 会覆盖 `CTL.ENCLK` 的顺序约束：先 reset preflight，再配置 controller-private clock，最后消费完整 prerequisite proof。
4. reset 后采用有界 idle 检查；任何 IO、readback 或 timeout 错误都归还可重试的 session，不进入数据命令或 DMA。
5. 保持生产初始化入口关闭，直到两块目标板完成电源、时钟、卡检测和 IRQ 的物理验证。

Linux 上游 power-up 路径写 `CTL.RESET`、等待 10 ms、写 `CTL.EXTCLK`，随后向 `INT` 和 `IEN`
写低 10 位。上游没有把 RESET 当作可轮询的 self-clear 位，因此 WaterOS 不推测该语义；额外读取
`CSTS.ON` 与 `DSTS.RXON/TXON` 做有界 idle 检查，属于 fail-closed 软件策略。

### 完成内容

- [x] 新增 `Uninitialized`、`Preflighted`、`ClockConfigured`、`CommandReady` 四个 Host typestate；只有 `Uninitialized` 暴露普通构造器。
- [x] 新增带安全契约的 `HostPreflightAuthority` 和可注入 `ResetDelay`，明确 10 ms 延时、MMIO ownership 与板级验证责任。
- [x] preflight 严格执行 reset、10 ms delay、EXTCLK、CTL readback、INT clear、IEN enable/readback 与 bounded idle poll。
- [x] 不轮询未经文档证明会 self-clear 的 RESET；CTL/IEN 不匹配与 CSTS/DSTS 超时均 fail-closed。
- [x] `HostPreflightFailure` 保存失败阶段、观测值和 `Host<Uninitialized>`；`retry()` 从完整 reset 序列重新开始。
- [x] controller clock transaction 只能从 `Preflighted` 进入；失败通过 `HostClockFailure` 归还 host 与既有 clock recovery evidence。
- [x] `Host<ClockConfigured>::authorize()` 必须消费完整 `ControllerPrerequisiteProof`，才能产生 `CommandReady`。
- [x] `execute_command()` 只存在于 `Host<CommandReady>`；测试旁路使用 `#[cfg(test)]` fixture，不进入生产构建。
- [x] 仍未提供 data command、DMA、machine init 或 remote monitor 的激活入口；七个 blocker、`can_activate()==false` 与 `proof=0` 保持不变。

### 验证证据

- 2K1000 驱动 host 单测 110 项全部通过；新增 3 项覆盖完整 preflight 顺序、所有 IO/readback fault 与 bounded busy timeout。
- fault matrix 覆盖四处 write failure、四处 read failure、CTL/IEN mismatch，并验证 owned-session retry 必须重新写 RESET。
- aggregate prerequisite 测试走完整生产类型链：reset preflight、controller clock transaction、typed proof 组装和 proof consumption。
- topology fixture/畸形 DTB 矩阵通过；dtc 仅输出刻意构造畸形输入的预期 warning。
- remote client 与 QEMU launcher 的 13 项 Python host 测试通过。
- `cargo check --no-default-features --features loongson2k1000la,final_online,heap-tlsf,remote-debug-monitor --target loongarch64-unknown-none` 通过。
- `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 与 `git diff --check` 通过；仅有仓库既有 warning。
- 所有写操作测试均使用内存寄存器模型；没有访问物理 MMIO、发真实命令、启动 DMA 或创建磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：真实 RESET 行为、10 ms 时序、CTL.EXTCLK readback、INT/IEN 语义以及 reset 后 CSTS/DSTS 状态均未在两块板验证。
- [ ] 生产平台 timer 尚未接入 `ResetDelay`；当前仅由 host fixture 验证调用顺序和延时参数。
- [ ] 完整 proof 没有生产构造路径：电源、卡检测和 IRQ token 仍要求逐板物理证据，因此 production Host 没有 command caller。
- [ ] `CommandReady` 当前只允许非数据命令；响应寄存器顺序、命令完成 IRQ 和错误恢复仍需真机验证。
- [ ] 证据不足时不推测 reset/clock shutdown 或 rollback 顺序；失败只归还 session 和观测证据。
- [ ] 下一批应把单条非数据命令建模为可恢复的 in-flight session：timeout/error 后不能静默回到 ready，必须重新观测或 reset preflight 后才能继续。

### 参考与许可证

- `docs/references/loongson2-mmc-upstream.md`
- `docs/references/loongson2-clock-upstream.md`

### 提交

- `[feat] gate LS2K1000 MMC commands by prerequisites`

## 2026-08-10：批次 75——LS2K1000 MMC 非数据命令 ownership recovery

### 任务与设计

1. 审计 `execute_command()` 在参数错误、MMIO fault、命令超时、CRC 错误和响应读取失败后的 Host ownership。
2. 消除 `&mut Host<CommandReady>` 在命令已下发后自动归还 ready 借用的旁路。
3. 将结果区分为 pre-MMIO rejection、成功完成和必须隔离恢复三类。
4. 仅在 CSTS/DSTS idle、没有未知中断位且已知 W1C 状态清理并 readback 为零时恢复 ready。
5. 任何无法证明安全复用的状态均可降级为 `Uninitialized`，丢弃旧 proof 并要求完整 reset/clock/authorization 链。

状态转移为 `Host<CommandReady> → CommandOutcome::{Completed,Rejected,RecoveryRequired}`。
`Rejected` 只用于零 MMIO 的参数错误；从第一次 W1C write 开始，write 是否生效都可能未知，因此所有错误
均转入 `CommandRecoveryRequired`。恢复对象同时保存不可变 origin fault 和当前 revalidation fault，避免二次
失败覆盖最初诊断证据。

### 完成内容

- [x] `execute_command()` 改为消费 Host ownership，不再以可重复使用的 `&mut self` 执行命令。
- [x] 新增 `CommandOutcome`；成功显式归还 response 与 `Host<CommandReady>`，无 MMIO 参数拒绝归还原 Host。
- [x] 新增不可直接发命令的 `CommandRecoveryRequired` typestate 和 owned `CommandRecovery`。
- [x] 命令路径对 clear INT、写 argument、写 CCTL、poll INT、timeout/CRC、ack completion 和四个 response read 分阶段记录错误。
- [x] recovery 保存 `origin_stage/origin_error`，revalidate 失败时另行更新当前 `stage/error` 和观测值。
- [x] `revalidate()` 读取 CSTS、DSTS、INT，拒绝 command/data busy 和未知位；已知位必须 W1C 后 readback 为零。
- [x] recovery failure 保持 ownership，可再次 revalidate；`into_uninitialized()` 强制丢弃旧 prerequisite proof。
- [x] 上游参考文档补充 INT bits 6/7/8 和 W1C 依据，并明确 owned recovery 是 WaterOS 额外安全策略。
- [x] data command、DMA、machine init 与 remote monitor 激活入口均未开放；七个 blocker 和 `can_activate()==false` 保持不变。

### 验证证据

- 2K1000 驱动 host 单测 113 项全部通过；新增 3 项聚合测试覆盖 ownership 分类、恢复准入和 revalidation fault matrix。
- 参数错误 fixture 验证零 MMIO；成功 fixture 验证 W1C 后显式归还 ready Host 和四字 response。
- command fault matrix 覆盖 4 个 write failure 与 5 个 read failure，全部只返回 recovery ownership。
- timeout/CRC 与 bounded poll 路径均进入隔离；idle + known W1C/readback 可恢复，busy/unknown status 不可恢复。
- recovery fault matrix 覆盖 CSTS、DSTS、INT、W1C write 和 readback IO failure，并验证 origin fault 不被覆盖。
- topology fixture/畸形 DTB 矩阵通过；dtc 仅输出刻意构造畸形输入的预期 warning。
- remote client 与 QEMU launcher 的 13 项 Python host 测试通过。
- `cargo check --no-default-features --features loongson2k1000la,final_online,heap-tlsf,remote-debug-monitor --target loongarch64-unknown-none` 通过。
- `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 与 `git diff --check` 通过；仅有仓库既有 warning。
- 所有命令与恢复测试均使用内存 MMIO model；没有访问物理寄存器、真实卡片或 DMA。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：CSTS/DSTS 是否足以证明失败命令已完全 quiesce，以及 INT W1C 后立即 readback 为零的真实行为尚未逐板验证。
- [ ] 命令完成与 timeout/CRC 的优先级、响应寄存器稳定窗口和长响应 word ordering 仍需逻辑分析仪/真机测试。
- [ ] WaterOS recovery 目前使用一次 bounded snapshot，不证明采样后的固件、另一核心或异常硬件不会再次改变状态。
- [ ] 恢复成功沿用原 aggregate proof；这只适用于未 reset、供电/卡检测/IRQ ownership 未变化的串行 session，生产调用者尚不存在。
- [ ] `into_uninitialized()` 只改变软件 typestate，不自动写 RESET；调用者必须重新执行 preflight，不能把类型转换解释为硬件复位。
- [ ] 当前仍只支持 polling 非数据命令；真实 IRQ-driven completion、data PIO 与 APBDMA binding 均未实现。
- [ ] 下一批应建立非数据命令 descriptor/response contract：按 response type 决定是否读取寄存器，验证 short/long response word mapping，并把 unsupported data flags 在零 MMIO 前拒绝。

### 参考与许可证

- `docs/references/loongson2-mmc-upstream.md`

### 提交

- `[feat] isolate failed LS2K1000 MMC commands`

## 2026-08-10：批次 76——LS2K1000 MMC command descriptor 与 response contract

### 任务与设计

1. 审计 Linux 主线 command flags、CCTL encoding、response register order 和完成后的 controller cleanup。
2. 用预验证 descriptor 取代 index/argument/response bool 散参数，避免非法组合进入 Host MMIO。
3. 将无响应、短响应和 136-bit 长响应建模为不同 contract，只读取 contract 要求的寄存器。
4. 在成功归还 ready ownership 前执行上游同序的 CARG/CCTL cleanup；失败继续进入 owned recovery。
5. recovery 也必须完成 cleanup 和 readback，不能只凭 idle/INT clear 复用旧命令状态。

Linux 将 `MMC_RSP_PRESENT` 映射到 `CCTL.WAIT_RSP`、`MMC_RSP_136` 映射到
`CCTL.LONG_RSP`。其 threaded completion 为统一收尾读取 RSP0..RSP3，然后依次清零 CARG/CCTL。
WaterOS 保留 CCTL encoding、长响应 offset order 和 cleanup order，但按 descriptor 将响应读取缩减为
0/1/4 次；这是额外的最小 MMIO 策略，不声明已经获得真机行为证明。

### 完成内容

- [x] 新增 `CommandDescriptor`、`ResponseType::{None,Short,Long}` 和 `CommandTransfer`。
- [x] descriptor 构造器拒绝 index > 63 和所有 `CommandTransfer::Data`，因此非法/data request 无法调用 Host MMIO 方法。
- [x] 删除执行期 bool 组合和 `CommandOutcome::Rejected`；非法意图在 Host ownership 之外提前失败。
- [x] 新增 typed `CommandResponse::{None,Short,Long}`，不再用 `[u32;4]` 隐含所有响应类型。
- [x] CCTL 对 None 不置 response 位、Short 只置 WAIT_RSP、Long 置 WAIT_RSP|LONG_RSP。
- [x] 成功路径分别读取 0、RSP0、RSP0..RSP3；长响应按递增寄存器 offset 返回。
- [x] 四个 response read 使用独立 `CommandStage::ReadResponse0..3`，保留精确 fault evidence。
- [x] 响应处理后按上游顺序写 CARG=0、CCTL=0；任一写失败均隔离 ownership。
- [x] recovery 在 idle/INT clear 后也写零 CARG/CCTL，并逐个 readback；IO fault 或 mismatch 继续隔离。
- [x] recovery 新增 argument/control 观测值和 cleanup/readback/mismatch 阶段，不会错误返回 ready。
- [x] 数据寄存器、PIO、DMA、machine init 与 remote monitor 激活入口保持关闭；既有 blocker 不变。

### 验证证据

- 2K1000 驱动 host 单测 114 项全部通过；新增 response-contract 测试并扩展 command/recovery fault matrices。
- descriptor fixture 覆盖 index 越界与 data intent，证明没有可传入 Host 的非法 descriptor。
- None/Short/Long fixtures 验证 CCTL 精确位、响应读取次数 0/1/4、长响应 RSP0→RSP3 顺序和 typed payload。
- 成功 fixture 验证 CARG/CCTL 最终均为零；command write fault matrix 从 4 个扩展到 6 个阶段，包含两个 cleanup write。
- long-response read fault matrix 分别注入 RSP0/1/2/3 failure，返回对应 stage 且不归还 ready Host。
- recovery fault matrix 覆盖 CSTS/DSTS/INT、INT readback、CARG/CCTL write/readback，以及两个 cleanup mismatch。
- topology fixture/畸形 DTB 矩阵通过；dtc 仅输出刻意构造畸形输入的预期 warning。
- remote client 与 QEMU launcher 的 13 项 Python host 测试通过。
- LoongArch64 target check 与 `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 通过；仅有仓库既有 warning。
- `git diff --check` 通过；全部 MMIO 测试使用内存模型，没有物理寄存器、卡片或 DMA 访问。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：None/Short 省略未声明 RSP register reads 是否适用于两块目标板，尚未实测。
- [ ] 长响应 `[RSP0,RSP1,RSP2,RSP3]` 只表示寄存器 offset order；协议位序、CRC strip 和 endian mapping 仍需真机对照 CID/CSD。
- [ ] CARG/CCTL 写零及立即 readback 的硬件可靠性、posted-write ordering 和失败恢复仍需逐板验证。
- [ ] Linux 无条件读四个 response words；WaterOS 最小读取是有意差异，若真机发现 latch/read side effect 必须回退并记录证据。
- [ ] 当前 `CommandTransfer::Data` 只作为 fail-closed marker；没有数据 descriptor、PIO 或 APBDMA binding。
- [ ] `ResponseType` 尚未表达 CRC expected、busy response 和 opcode-specific quirk；不能直接视为完整 MMC core flags adapter。
- [ ] 下一批应增加 response policy：区分 short/short+CRC/short+busy/long+CRC，审计 CCTL.CHECK 与 BUSYEND 语义；证据不足的 flag 必须显式拒绝而非猜测。

### 参考与许可证

- `docs/references/loongson2-mmc-upstream.md`

### 提交

- `[feat] define LS2K1000 MMC command responses`

## 2026-08-10：批次 77——LS2K1000 MMC response validation policy

### 任务与设计

1. 审计 Linux MMC core 的 PRESENT/136/CRC/BUSY/OPCODE flags 与 Loongson2 driver 的实际消费范围。
2. 确认 `CCTL.CHECK`、`INT.RESPCRC`、`INT.BUSYEND` 是否存在可复用的上游完成语义。
3. 将 response width 与 protocol validation 分开建模，不能实现的 policy 在 descriptor 构造期拒绝。
4. 明确 polling 错误优先级，禁止 BUSYEND 单独冒充普通命令完成。
5. 保持第 75/76 批 owned recovery、最小 response reads 和 CARG/CCTL cleanup 不变量。

Linux MMC core 明确定义 CRC、card-busy 和 opcode-check flags，但 Loongson2 driver 只消费 PRESENT 与
136-bit。主线虽定义 `CCTL.CHECK`、`INT.RESPCRC`、`INT.BUSYEND`，却不设置 CHECK，也不以
RESPCRC/BUSYEND 驱动命令完成。因此本批不根据寄存器位名推测实现：只开放 `Unchecked`，CRC/busy
policy 必须等待额外上游或逐板证据。

### 完成内容

- [x] 新增 `ResponseValidation::{Unchecked,Crc,CrcAndBusy}`，response width 与 validation policy 正交表达。
- [x] `CommandDescriptor::new()` 增加 validation 参数；所有非 `Unchecked` policy 返回 `ResponsePolicyUnsupported`。
- [x] CRC/busy rejection 发生在 Host ownership/MMIO 之前，不能绕过到 CCTL programming。
- [x] CCTL encoding 仍只设置 WAIT_RSP/LONG_RSP；不会设置证据不足的 bit13 CHECK。
- [x] INT poll 顺序固定为 command timeout、observed RESPCRC anomaly、command sent；错误不会被同时出现的成功位覆盖。
- [x] BUSYEND bit9 单独出现时不形成普通命令完成，最终进入 bounded poll recovery。
- [x] 自发观测到 RESPCRC 仍 fail-closed 为 `CommandStage::ResponseCrc`，但不宣称实现了 requested CRC checking。
- [x] None/Short/Long response payload、cleanup、revalidate 和 typed Host ownership 均保持不变。
- [x] data command、R1/R1b/R2 protocol adapter、IRQ-driven completion 与生产激活入口继续关闭。

### 验证证据

- 2K1000 驱动 host 单测 115 项全部通过；新增 1 项聚合覆盖 interrupt priority 和 BUSYEND fail-closed。
- descriptor matrix 覆盖 Short/Long × Crc/CrcAndBusy，全部在构造期返回稳定错误。
- long-response CCTL fixture 显式验证 bit13 CHECK 为零。
- `CTIMEOUT|RESPCRC|CSENT` 返回 CommandTimeout；`RESPCRC|CSENT` 返回 ResponseCrc；BUSYEND-only 返回 PollTimeout。
- 既有 114 项继续覆盖 response 0/1/4 reads、cleanup、command/recovery fault matrices 和完整 prerequisite chain。
- topology fixture/畸形 DTB 矩阵通过；dtc 仅输出刻意构造畸形输入的预期 warning。
- remote client 与 QEMU launcher 的 13 项 Python host 测试通过。
- LoongArch64 target check 与 `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 通过；仅有仓库既有 warning。
- `git diff --check` 通过；测试全部使用内存寄存器模型，没有访问物理控制器。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：CCTL.CHECK 的真实作用、启用条件以及与 INT.RESPCRC 的关系未知。
- [ ] INT.RESPCRC 在 CHECK 未设置时是否可能置位、是否 W1C、与 CSENT 同时出现时的硬件优先级均未逐板验证。
- [ ] R1b 的 BUSYEND 时序、是否还需观察 DSTS.BUSYFIN、以及 completion/ack 顺序没有可靠证据。
- [ ] opcode echo checking 未建模；主线 Loongson2 driver 同样未显式消费 MMC_RSP_OPCODE。
- [ ] 当前仅能承载低层 unchecked diagnostics，不能声称支持要求 CRC 的标准 R1/R2/R5/R6/R7 初始化流程。
- [ ] 两块板拿到后应先用 CMD0/无响应和只读 diagnostic command 验证基本路径，再受控实验 CHECK/RESPCRC/BUSYEND，禁止直接解除 policy gate。
- [ ] 下一批应建立板级 command validation probe 规格和 software evidence recorder：定义每种 policy 需要采集的 CCTL/INT/CSTS/DSTS trace，使未来上板结果能生成受审计 token，而不是直接调用 unsafe bypass。

### 参考与许可证

- `docs/references/loongson2-mmc-upstream.md`

### 提交

- `[feat] reject unverified LS2K1000 MMC response policies`

## 2026-08-10：批次 78——LS2K1000 MMC command validation evidence recorder

### 任务与设计

1. 为未来逐板验证 CHECK/RESPCRC/BUSYEND 定义固定大小、可审计的 command software trace。
2. trace 只能记录命令路径已经执行的访问，不为采样额外写控制器，也不按 poll_limit 动态分配。
3. 增加固定顺序的 post-command read-only snapshot，保留每个读取失败点之前的 partial evidence。
4. 将 trace 与 snapshot 分类为 ObservedOnly/IncompleteTrace/UnsafeState，但绝不生成 response-policy token。
5. 覆盖容量截断、成功/失败 outcome、post snapshot 顺序、fault matrix 和 assessment 降级。

每条 trace 固定保存最多 8 个 INT poll sample，并额外保存 union 与 dropped count。post snapshot 固定读取
`CARG → CCTL → CSTS → DSTS → INT`。assessment 要求 trace 未截断、命令完成、controller idle、
CARG/CCTL 为零且 INT 无残留，满足全部条件仍只能得到 `ObservedOnly`。

### 完成内容

- [x] 新增 `COMMAND_TRACE_CAPACITY=8`、`CommandTrace` 与 `CommandTraceOutcome`。
- [x] trace 保存 command index/argument、response width、validation policy、实际 programmed CCTL、8 个 INT sample、dropped count 与 INT union。
- [x] trace 保存 response read mask、两个 cleanup write 是否成功，以及 Completed/Failed(stage) outcome。
- [x] `CommandOutcome::Completed` 返回 trace；`CommandRecovery` 同样持有失败 trace，不丢失命令期证据。
- [x] poll 样本超过 8 个时只饱和增加 dropped count，不分配 Vec、不覆盖先前样本。
- [x] 新增 `observe_command_post_state()`，严格以 CARG/CCTL/CSTS/DSTS/INT 顺序只读采样。
- [x] 新增 `CommandPostObservationFailure`，记录失败 stage、IO error 和此前成功的 partial fields。
- [x] 新增 `assess_command_validation()` 和稳定 disposition；trace 截断优先标为 IncompleteTrace。
- [x] 完整、idle、clean、零 INT 的健康 fixture 仅返回 ObservedOnly；任何失败/残留状态返回 UnsafeState。
- [x] assessment 无 token 构造器、无 unsafe bypass，也不改变 Host typestate 或 CRC/busy descriptor gate。

### 验证证据

- 2K1000 驱动 host 单测 117 项全部通过；新增 2 项覆盖 bounded trace/assessment 和 post-observation fault matrix。
- 10 次空 poll fixture 保留前 8 项、报告 dropped=2、union=0 和 Failed(PollTimeout)，没有动态容量增长。
- healthy command fixture 的 trace 记录 response mask/cleanup/outcome；post snapshot 后只得到 ObservedOnly。
- 相同 snapshot 配合截断 trace 降级为 IncompleteTrace；非零 CCTL snapshot 降级为 UnsafeState。
- post observer 验证精确五次读取顺序；5 个 read failure 点分别保留 0/1/2/3/4 个 partial fields。
- 既有 command fault matrices 继续通过，证明 trace 加入后 ownership、cleanup 与 recovery 行为未回退。
- topology fixture/畸形 DTB 矩阵通过；dtc 仅输出刻意构造畸形输入的预期 warning。
- remote client 与 QEMU launcher 的 13 项 Python host 测试通过。
- LoongArch64 target check 与 `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 通过；仅有仓库既有 warning。
- `git diff --check` 通过；全部验证使用内存寄存器模型，没有物理 MMC 访问。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：采样的 INT/CSTS/DSTS/CARG/CCTL 含义、时序关联和 posted-read ordering 均需逐板确认。
- [ ] 固定 8 项 trace 会对长轮询截断；dropped>0 明确不可作为完整证据，未来真机 probe 应使用较小且受控的 poll budget。
- [ ] trace 与 post snapshot 没有硬件 generation counter；调用者必须在同一独占 Host session 中立即采样，assessment 本身不证明二者来自同一时刻/设备。
- [ ] public diagnostic structs 可被软件构造或修改，因此只能用于日志/人工审计，不能承担不可伪造的 capability。
- [ ] ObservedOnly 只证明软件规则下的 snapshot 干净，不证明物理 CRC、busy、时钟波形或卡片协议正确。
- [ ] 仍没有生产 command caller；当前 recorder 只由 host MMIO model 覆盖，未来上板 probe 必须先取得完整 prerequisite proof。
- [ ] 下一批应为 trace/post/assessment 增加固定、无分配的远程文本格式，并把只读 post snapshot 接入显式 `ls2k-mmc` 诊断；不得自动执行命令或解除 policy gate。

### 参考与许可证

- `docs/references/loongson2-mmc-upstream.md`

### 提交

- `[feat] record LS2K1000 MMC command evidence`

## 2026-08-10：批次 79——LS2K1000 MMC 只读 controller 远程诊断

### 任务与设计

1. 审计 `ls2k-mmc` 显式远程诊断的格式、MMIO 范围和错误映射。
2. 为 command post、trace 与 assessment 提供固定字段、无额外分配的文本格式。
3. 只在操作者显式请求 `ls2k-mmc` 时读取 controller post state；默认启动路径保持零新增 MMC MMIO。
4. 诊断不得构造 `Host`、复位/开时钟、执行命令、清中断或执行 cleanup。
5. 没有生产命令 trace 时必须明确输出 `trace=none assessment=unavailable`，不得伪造 assessment。

远程响应复用 monitor 已有的 `String`，新增片段通过 `core::fmt::Write` 写入，因此 formatter
本身不要求 `Vec` 或按 trace 长度分配。controller 观察严格复用上一批的
`CARG → CCTL → CSTS → DSTS → INT` 固定只读顺序；任一读取失败仍输出 stage 和已取得的 partial fields。

### 完成内容

- [x] 新增 `write_command_post()`，稳定输出原始 CARG/CCTL/CSTS/DSTS/INT、idle/clean 和 known/unknown INT。
- [x] controller 读取失败输出稳定 stage code、此前成功字段，并以 `na` 标记尚未取得的字段。
- [x] 新增 `write_command_trace()`，覆盖全部现有 `CommandStage`、response width、sample/drop、INT union、response mask、cleanup 与 outcome。
- [x] 新增 `write_command_assessment()`，稳定输出 disposition 和各判定因子；文本本身不代表授权 token。
- [x] 新增 `VolatileDiagnosis`，把 prerequisite diagnosis 与 controller post observation 作为一次显式请求的结果返回。
- [x] `diagnose_volatile()` 在完成静态规划和 prerequisite reads 后才建立 controller 只读后端，不构造 `Host`。
- [x] driver facade 把 post state 附加到 `ls2k-mmc` 响应，并映射 controller backend 初始化错误。
- [x] 当前远程请求不执行 command，因此固定附加 `trace=none assessment=unavailable`。
- [x] machine init、默认启动、command policy gate、数据路径、DMA 与 blocker 均未改变。

### 验证证据

- 2K1000 驱动 host 单测 119 项全部通过；新增 2 项 formatter 测试。
- 固定缓冲区测试逐字验证 post、trace、assessment 成功格式，并验证容量不足返回 `fmt::Error`。
- failure fixture 验证 `read-dsts` stage、partial CARG/CCTL/CSTS 和 DSTS/INT `na` 均被保留。
- 上一批 post observer 测试继续验证恰好五次读取、固定顺序、零写入，以及五个失败点的 partial evidence。
- topology fixture/畸形 DTB 矩阵通过；dtc 仅输出刻意构造畸形输入的预期 warning。
- remote client 与 QEMU launcher 的 13 项 Python host 测试通过。
- LoongArch64 target check 与 `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 通过；仅有仓库既有 warning。
- `git diff --check` 通过；没有执行 QEMU 无法代表的物理 MMC 访问。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：controller volatile read 的可达性、端序、device ordering 与读取副作用需在两块目标板逐板验证。
- [ ] 显式诊断假定调用期间没有其他 controller owner；当前尚无生产 owner，但未来接入存储栈前必须落实互斥。
- [ ] controller snapshot 没有 generation counter，不能证明 prerequisite 与 controller fields 属于同一硬件瞬间。
- [ ] 当前没有执行 command，因而没有真实 trace 或 assessment；输出已显式标记 unavailable。
- [ ] 外层 monitor 响应仍使用既有 `String`；仅新增 evidence formatter 是无额外分配的。
- [ ] post state 即使显示 idle/clean 也只能作为观察证据，不能解除 CRC/busy policy gate 或生成 capability。
- [ ] 下一批应扩展主机端远程客户端：解析并校验 controller evidence 字段，保存带板卡身份与时间戳的采集记录，为未来上板形成可重复的 evidence capture 流程。

### 参考与许可证

- `docs/references/loongson2-mmc-upstream.md`

### 提交

- `[feat] expose LS2K1000 MMC controller evidence`

## 2026-08-10：批次 80——LS2K1000 MMC 主机端证据采集

### 任务与设计

1. 审计 `remote_debug_client.py` 对 `ls2k-mmc` 的校验范围和未来真机采集入口。
2. 对批次 79 的稳定 controller evidence 文本增加严格、无第三方依赖的结构化解析。
3. 增加显式、不可覆盖的小体积 JSON 归档，保存板卡身份、UTC 时间、原始响应和 SHA-256。
4. 保留未知顶层字段以兼容后续扩展，但拒绝无法正确解释的 command trace/assessment。
5. 不改变 monitor 认证模型、guest MMIO、默认启动或既有 smoke 命令序列。

原始 CRLF 响应是证据主体，结构化字段只是检索索引。客户端校验必需字段、重复字段、二进制值、
十六进制寄存器、prerequisite gate 和 controller success/failure schema。归档固定标记为
`unverified-observation`，不会因 snapshot 看似干净而声称完成硬件验证。

### 完成内容

- [x] 新增 `MmcEvidence` 与 `parse_mmc_evidence()`，解析成功和 partial controller failure。
- [x] 严格要求单行 CRLF、`ls2k-mmc` 前缀、唯一 `key=value` 字段和完整 prerequisite gates。
- [x] `proof`、`can_activate`、`idle`、`clean` 只接受 0/1，controller registers 只接受 `0x...` 或 `na`。
- [x] gate state 只接受当前稳定枚举；未知顶层字段仍保留在原始 fields map 中。
- [x] 当前 capture schema 只接受 `trace=none assessment=unavailable`，防止未来 command evidence 被旧客户端误解析。
- [x] 新增 `write_mmc_evidence()`，以 exclusive-create 模式写紧凑 JSON，不覆盖已有采集结果。
- [x] JSON 保存 schema、板卡 ID、UTC 时间、原始响应、响应 SHA-256、解析索引和硬件验证状态。
- [x] CLI 新增成对启用的 `--mmc-evidence PATH --board-id ID`；不指定时原有 smoke 行为完全不变。
- [x] board ID 在建立网络连接前验证为 1..128 个可打印字符。

### 使用方式

```bash
python3 os/scripts/remote_debug_client.py \
  --host 192.0.2.10 --port 22323 \
  --board-id ls2k1000-board-a \
  --mmc-evidence ls2k1000-board-a-mmc.json
```

monitor 无认证和加密，只能在隔离开发网络使用；示例地址仅为文档保留地址。

### 验证证据

- 全部 47 项 Python host 测试通过；remote client/QEMU launcher 相关用例由 13 项增至 16 项。
- 成功 fixture 验证 controller 数值、IRQ gate 和未知扩展字段保留。
- partial failure fixture 验证 `error:read-dsts` 与 `dsts/int=na`。
- malformed matrix 覆盖非法 proof、负 blocker、重复字段、非法 hex、缺失 gates、未知 gate state、future trace 和错误换行。
- 归档 fixture 验证固定 UTC、schema、板卡 ID、硬件状态、SHA-256 字段、4 KiB 内文件和禁止覆盖。
- `py_compile` 通过；topology fixture/畸形 DTB 矩阵通过，dtc warning 均来自预期畸形输入。
- `git diff --check` 通过。本批只改主机 Python 脚本，不改变 Rust、target feature、内核或物理 MMIO，因此未重复生成大体积内核构建。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：尚未从任一目标板实际连接 monitor 或取得 controller response。
- [ ] 板卡 ID 由操作者提供，当前没有 EEPROM/序列号等不可伪造身份来源。
- [ ] SHA-256 用于发现响应内容变化，不是签名；无认证 monitor 上的记录不能证明远端身份。
- [ ] 当前 schema 明确拒绝 command-bearing trace；未来开放 probe 后必须增加独立 schema 和完整 trace sample 表达。
- [ ] 归档是单次 snapshot，不证明跨时刻稳定性；上板时每块板应在冷启动、热重启和插卡状态分别采集。
- [ ] 成功采集仍只代表只读软件观察，不能解除 CRC/busy policy gate 或 activation blocker。
- [ ] 下一批应增加离线 evidence verifier：重新计算 SHA-256、重解析原始响应、比较 stored parsed index，并生成逐板采集清单/通过失败摘要。

### 参考与许可证

- 本批仅使用 Python 标准库，未引入第三方代码或许可证。

### 提交

- `[feat] capture LS2K1000 MMC remote evidence`

## 2026-08-10：批次 81——LS2K1000 MMC 离线证据审计与覆盖清单

### 任务与设计

1. 对 evidence v1 归档重新计算 SHA-256、重解析原始响应并比较 stored parsed index。
2. 增加版本化 manifest，显式定义两块板及冷启动/热重启/插卡等预期采集场景。
3. 验证每个 `(board_id, scenario)` 唯一、文件身份匹配且路径不能逃逸 manifest 目录。
4. CLI 对完整归档返回成功，对篡改、非法归档和覆盖不完整返回失败。
5. verifier 必须完全离线、只读，不连接 monitor、不访问 MMIO、不修改证据。

原始 `response` 继续是唯一权威数据；JSON 中的 `parsed` 只作为缓存索引。verifier 不信任该索引，
而是调用同一稳定 parser 从 raw response 重建后执行完整结构比较。manifest 只引用独立小文件，
不复制响应，因此两板多场景验证仍保持很小的磁盘占用。

### 完成内容

- [x] 新增 `mmc_evidence_verify.py` 和 `EvidenceVerificationError`。
- [x] 单文件校验严格检查 evidence v1 顶层 schema、command、board ID、UTC 时间和硬件状态标记。
- [x] 重新计算 raw response SHA-256，拒绝 response 与 digest 不一致。
- [x] 重新调用 `parse_mmc_evidence()`，逐项比较 fields/gates/controller，拒绝派生索引篡改。
- [x] 强制 `hardware_validation=unverified-observation`，拒绝手工提升为 verified。
- [x] evidence/manifest 均限制为 64 KiB，读取后按实际 byte length 检查。
- [x] 新增 `wateros-ls2k-mmc-manifest-v1`，支持任意显式板卡/场景矩阵。
- [x] manifest 拒绝重复 board、重复 scenario、重复 evidence、未预期项、身份不匹配、绝对路径和 `..`/symlink 路径逃逸。
- [x] `ManifestSummary` 稳定报告 expected、verified、missing 和 complete。
- [x] CLI 提供互斥的 `--evidence FILE` 与 `--manifest FILE`；JSON 摘要便于自动化处理。
- [x] 完整单文件/manifest 返回 0；manifest 缺项或校验错误返回 1。
- [x] 采集端 board ID 与 manifest 统一收紧为 1..128 个 ASCII 字母、数字、点、下划线或短横线。

### 使用方式

```bash
python3 os/scripts/mmc_evidence_verify.py --evidence board-a-cold.json
python3 os/scripts/mmc_evidence_verify.py --manifest mmc-manifest.json
```

manifest 的 `expected` 定义板卡和场景，`evidence` 中的相对路径必须位于 manifest 同一目录树内。

### 验证证据

- 全部 52 项 Python host 测试通过；新增 verifier 测试 5 项，原 remote/QEMU 16 项继续通过。
- tamper matrix 覆盖 raw response、SHA-256 间接不匹配、parsed controller、hardware validation 和 schema。
- 两板 × `cold-no-card`/`cold-card`/`warm-card` 六场景 fixture 完整通过。
- 删除一个场景后稳定报告 `board-b/warm-card`，CLI 返回 1；补齐后 `complete=true`。
- manifest 负向测试覆盖重复 pair、板卡身份不匹配和目录逃逸。
- CLI subprocess fixture 验证单文件成功 JSON 与不完整 manifest 失败 JSON。
- `py_compile`、topology fixture/畸形 DTB 矩阵和 `git diff --check` 均通过；dtc warning 来自预期畸形输入。
- 本批仅修改离线主机脚本，没有 Rust、内核、guest MMIO 或镜像变更，因此未生成大体积内核/磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：当前测试记录均由软件 fixture 生成，没有两块目标板的真实采集文件。
- [ ] SHA-256 和 parsed-index 一致性只能发现归档内部篡改/损坏，不提供远端身份认证或数字签名。
- [ ] manifest 的 expected matrix 由操作者定义；verifier 不知道板卡资产清单是否完整。
- [ ] 场景名称是受限字符串但语义由团队约定，当前不能自动证明真的执行过断电冷启动或插拔卡片。
- [ ] evidence 时间来自主机时钟，未使用可信时间戳服务。
- [ ] 当前只支持 `trace=none assessment=unavailable` 的 evidence v1；未来 command probe 必须升级 schema/parser/verifier。
- [ ] 下一批应提供受版本控制的两板采集 manifest 模板和操作步骤，并增加重复采样/跨场景 invariant 检查；真实文件到位前所有条目保持 missing。

### 参考与许可证

- 本批仅使用 Python 标准库，未引入第三方代码或许可证。

### 提交

- `[feat] verify LS2K1000 MMC evidence archives`

## 2026-08-10：批次 82——LS2K1000 MMC 两板多样本采集协议

### 任务与设计

1. 保留 manifest v1 单样本兼容，同时增加 v2 的 minimum sample count 与响应字段断言。
2. 同一板卡/场景允许多个独立证据文件，但路径与采集时间不能重复。
3. 场景语义由受版本控制的 `assert_fields` 明确表达，不在 verifier 中硬编码场景名称。
4. 提供两板、三场景、每场景两样本的模板与逐步采集操作文档。
5. 模板在没有物理板时必须明确报告全部 missing，不能用软件 fixture 填充正式 evidence。

v2 允许模板声明 `card=gpio,present=0/1,controller=ok,trace=none` 等跨场景 invariant。
verifier 对每一份 raw response 完整重验证后再执行断言。允许多个样本的响应内容完全相同，因为稳定
controller 可能产生相同寄存器值；独立性以不同文件路径和 UTC `captured_at` 为最低软件证据。

### 完成内容

- [x] `verify_manifest()` 同时支持 `wateros-ls2k-mmc-manifest-v1` 与 v2。
- [x] v2 expected entry 新增 `minimum_samples`，范围限制为 1..16。
- [x] v2 expected entry 新增非空 `assert_fields` string map，对原始响应重建出的 fields 精确比较。
- [x] v2 允许同一 `(board_id, scenario)` 多份记录，summary 的 expected/verified 以样本数统计。
- [x] 缺失样本稳定表示为 `board/scenario#N`；超过最低样本数仍可通过并计入 verified。
- [x] manifest 全局拒绝重复/别名后的相同 evidence path。
- [x] 同一板卡/场景的重复 `captured_at` 被拒绝，避免同一归档伪装为重复采样。
- [x] v1 继续拒绝同一 pair 多份 evidence，旧 manifest 的结果和 missing 格式不变。
- [x] 单文件 CLI 摘要不暴露完整 parsed fields；fields 仅在内部用于 v2 assertion。
- [x] 新增两板 × `cold-no-card`/`cold-card`/`warm-card` 模板，每格要求两份，共 12 份。
- [x] 模板要求 no-card `present=0`、card 场景 `present=1`、controller 正常且保持 read-only trace 状态。
- [x] 新增上板准备、冷启动/热重启定义、采集命令、manifest entry 和离线验收指南。

### 产物

- `docs/tasks/ls2k-mmc-evidence-manifest-v2.json`
- `docs/guides/ls2k-mmc-evidence-capture.md`

### 验证证据

- 全部 53 项 Python host 测试通过；新增 v2 聚合测试 1 项，既有 v1/remote/QEMU 用例继续通过。
- v2 fixture 验证第一份样本报告 `board-a/cold-card#2`，第二份不同时间样本补齐后 complete。
- v2 负向 fixture 覆盖重复路径、重复 captured_at 和 `present` 场景断言失败。
- v1 两板六场景、篡改矩阵、路径逃逸与 CLI 返回码测试继续通过。
- 正式模板由 verifier 成功解析并按预期返回 exit 1：expected=12、verified=0、12 项全部 missing。
- `py_compile`、全部 topology/畸形 DTB fixture 和 `git diff --check` 通过；dtc warning 为预期畸形输入。
- 本批只修改离线脚本与文档，没有 Rust、guest MMIO、内核或镜像变更，未生成大体积内核/磁盘镜像。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：正式模板的 12 个 evidence entry 均为空，必须保持 missing 直到真实板卡采集。
- [ ] `ls2k1000-board-a/b` 是占位 ID，上板前必须替换为实际资产编号并评审提交。
- [ ] 模板按当前 DTB 假设可移除 GPIO card detect；若某板为 non-removable，必须评审修改断言。
- [ ] 不同 captured_at 只证明归档时间字符串不同，不能证明真的断电、热重启或插拔卡片。
- [ ] field assertion 是精确文本比较；formatter/schema 升级时应创建新模板版本，不能静默放宽旧模板。
- [ ] controller=ok 失败记录仍可单文件归档审计，但不能计入当前验收矩阵。
- [ ] 两份样本只能初步发现不稳定状态，不能代替长时间压力测试、协议 trace 或真实块读写。
- [ ] 下一批应回到 MMC 数据路径前置工作：审计最小只读块传输所需的 command/data typestate、buffer ownership 与 PIO/DMA 选择，继续保持生产入口关闭。

### 参考与许可证

- 本批只使用仓库自有格式和 Python 标准库，未引入第三方代码或许可证。

### 提交

- `[feat] define LS2K1000 MMC evidence matrix`

## 2026-08-10：批次 83——LS2K1000 MMC 最小只读数据 preflight

### 任务与设计

1. 审计 Linux 主线 Loongson2 MMC 数据寄存器、block limits、2K1000 DMA backend 和 request 顺序。
2. 定义只能表示 CMD17/CMD18 的只读 block request，不开放写请求或通用 ADTC bypass。
3. 对 block count/size、byte length、block/byte addressing、bus width 和 buffer length 做 checked preflight。
4. 生成固定顺序的 DCTL/BSIZE/TIMER 软件计划，并把 DATA slave 地址绑定到 controller window。
5. 用独占 mutable borrow 保留 CPU buffer ownership；没有 executor 时只能 cancel 归还。
6. 即使软件 evidence 全部为 true，也保留不可移除的 ExecutorUnavailable blocker。

Linux 主线为 2K1000 固定请求外部 `rx-tx` DMA，没有 PIO fallback。数据设置顺序为
`DCTL(BNUM|START|ENDMA|bus-width) → BSIZE → TIMER=U32_MAX`；DMA slave address 是 controller
`DATA+0x40`，读方向为 device-to-memory。WaterOS 只生成这些值的 inert plan，不执行寄存器写、
DMA ownership transfer 或 command。

### 完成内容

- [x] 补充 TIMER/BSIZE/DCTL/DATA offsets 与 12-bit block count、START、ENDMA、4/8-bit bus 常量。
- [x] 新增 `ReadAddressing::{Block,Byte}`；byte addressing 使用 checked `block * block_size`。
- [x] 新增 `ReadBlockRequest`，单块固定 CMD17，多块固定 CMD18。
- [x] request 显式携带 `ResponseType::Short + ResponseValidation::Crc`，不伪装成现有 Unchecked command。
- [x] block count 限制 1..4095；block size 限制 1..4095、且必须四字节整除。
- [x] 使用 checked arithmetic 计算 byte length、command argument 和 DATA MMIO address。
- [x] 新增 `ReadTransport`；LS2K1000 的 PIO 请求稳定返回 `PioUnsupported`，不猜测 FIFO 行为。
- [x] 新增 `DeferredReadPlan`，保存 DCTL→BSIZE→TIMER 固定顺序和 DATA physical address。
- [x] 新增 `ReadPathEvidence` 和五类 blocker：mapping、coherency、data IRQ、response CRC、executor。
- [x] `ExecutorUnavailable` 无条件存在，`DeferredReadPlan::can_execute()` 恒为 false。
- [x] 新增 `DeferredRead<'a>` 独占 `&mut [u8]`；只公开 plan 与 cancel，没有 start/MMIO/device ownership API。
- [x] buffer 必须与 request byte length 完全相等；失败不产生 deferred ownership。
- [x] 默认启动、Host、CommandDescriptor、APBDMA executor、remote monitor 与 activation blockers 均未接线。

### 验证证据

- 2K1000 驱动 host 单测 122 项全部通过；新增 3 项数据 preflight 聚合测试。
- request matrix 覆盖 CMD17/CMD18、block/byte address、zero/4096 blocks、0/510/4096 block size 和 argument overflow。
- plan fixture 精确验证 DCTL→BSIZE→TIMER offsets/value、4-bit bus encoding 和 DATA=0x1fe2c040。
- ownership fixture 验证 cancel 返回同一 buffer address，且 512 字节内容没有改变。
- 缺全部 evidence 时有 5 个 blocker；四项 evidence 全部为 true 时仍保留且仅保留 ExecutorUnavailable。
- 负向 fixture 覆盖 PIO、buffer length mismatch、非法 bus width 和 controller DATA address overflow。
- 既有 command descriptor 继续拒绝 `CommandTransfer::Data`，新 plan 无转换/执行入口。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 通过；dtc warning 为预期畸形输入。
- LoongArch64 精确 target check 与 `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 通过；仅有既有 warning。
- `git diff --check` 通过；所有新测试为内存中的纯值/borrow 测试，没有物理 MMIO、DMA 或卡片访问。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：DCTL BNUM encoding、START/ENDMA、BSIZE/TIMER 写序和 DATA slave address 均未逐板验证。
- [ ] 当前没有 MMC data executor，plan 不能写寄存器、发 CMD17/CMD18 或取得 device ownership。
- [ ] `ReadPathEvidence` 是可构造的软件 observation，不能充当 capability；无条件 executor blocker 防止其被用于激活。
- [ ] 当前 buffer 只是 CPU mutable borrow，不是 physically contiguous `DmaMapping<FromDevice>`；后续必须绑定 `OwnedDmaBuffer`。
- [ ] CRC response policy 仍被现有 command descriptor 拒绝；标准 R1/CMD17/CMD18 不能通过 Unchecked 绕过。
- [ ] data completion 的 DFIN/DTIMEOUT/RXCRC、DMA completion 双重汇合与错误优先级尚未建模。
- [ ] CMD18 stop command/CMD12、partial multi-block transfer 和 recovery 尚未设计；不能声称支持多块读取。
- [ ] cache invalidate、DMA routing、descriptor status 与真实 device-to-memory 可见性需要两板验证。
- [ ] 下一批应建立 MMC data + APBDMA 的双 completion typestate：只在 command response、DFIN 和 DMA completion 全部满足后归还 CPU buffer；任一失败进入可恢复隔离态，仍不接生产入口。

### 参考与许可证

- `docs/references/loongson2-mmc-upstream.md`
- Linux `drivers/mmc/host/loongson2-mmc.c` 为 `GPL-2.0-only`；本批仅参考公开寄存器/流程事实，未复制源码。

### 提交

- `[feat] plan LS2K1000 MMC read transfers`

## 2026-08-10：批次 84——LS2K1000 MMC command/data/DMA 三重完成 typestate

### 任务与设计

1. 为未来只读传输建立 command response、controller DFIN、DMA completion 三份独立证据的汇合状态机。
2. 三份成功证据允许任意顺序到达，但只有全部具备后才能归还 owned buffer。
3. data/command/DMA failure、未知中断与重复 completion 必须进入资源隔离态。
4. 恢复态不得默认 drop 或公开 buffer，因为 DMA 是否停止在失败时可能未知。
5. 当前没有 executor，生产构建不得存在 tracker 构造入口，也不接 MMIO/IRQ runtime。

completion tracker 按值拥有泛型资源 `B`，成功时由 `ReadCompleted::into_buffer()` 归还；失败时将
资源放入 `ManuallyDrop<B>`。生产 API 没有 recovery buffer extractor，避免未来把 device-owned
mapping 当作 CPU-owned 自动释放。仅 `#[cfg(test)]` fixture 可构造 tracker 和回收纯内存资源。

### 完成内容

- [x] 新增 `ReadCompletionEvidence`，分别记录 command response validated、DFIN、DMA finished。
- [x] 新增 `ReadCompletionTracker<B>`、`ReadCompletionProgress`、`ReadCompleted<B>` 和 `ReadCompletionRecovery<B>`。
- [x] success facts 以 consuming transition 更新；第三份独立 evidence 到达前始终返回 Pending。
- [x] completed 类型保存 plan/evidence，只有该类型公开 `into_buffer()`。
- [x] recovery 使用私有 `_buffer: ManuallyDrop<B>`，生产构建没有取回或自动 drop 路径。
- [x] tracker 构造器限定 `#[cfg(test)]`；生产代码无法从 deferred plan 启动 completion 状态机。
- [x] command failure 分类 Timeout/ResponseCrc/Io；DMA failure 分类 Start/Completion/Stop。
- [x] data failure 分类 DTimeout/RxCrc/TxCrc/ProgramError/unknown interrupt。
- [x] controller error priority 固定为 DTimeout→RxCrc→TxCrc→ProgramError→unknown→DFIN；成功位不覆盖错误。
- [x] command、DFIN 或 DMA completion 重复到达分别进入 DuplicateCommand/Data/Dma recovery。
- [x] controller snapshot 可包含其它已知 INT bits；没有 DFIN 时不会错误形成 data completion。
- [x] 没有修改 Host、IRQ owner、APBDMA executor、machine init、remote monitor 或 activation gate。

### 验证证据

- 2K1000 驱动 host 单测 125 项全部通过；新增 3 项 completion typestate 聚合测试。
- 6 种 command/data/DMA 成功排列全部验证：前两步 Pending，第三步 Completed，原 owned value 完整归还。
- data error matrix 将 DFIN、一个已知错误和未知 bit 同时注入，稳定保留已知错误优先级且不设置 data_finished。
- unknown-only snapshot 进入 `UnknownInterrupt` recovery；资源只能经 test-only fixture 回收。
- duplicate matrix 覆盖 command/DFIN/DMA 三类重复 evidence。
- explicit failure matrix 覆盖 3 类 command failure 与 3 类 DMA failure，全部保留资源进入 recovery。
- 组件 production `cargo check` 零新增 warning；tracker 构造器和 recovery extractor 均未编译进生产路径。
- LoongArch64 精确 target check 通过；仅有仓库既有 warning。
- 新状态机不接收 RegisterIo、IRQ runtime 或 APBDMA session，测试没有物理 MMIO/DMA。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：DFIN 与外部 DMA completion 的真实先后、并发和中断合并行为未知。
- [ ] command validated 当前是抽象 evidence；CRC policy 仍无实现，生产构建也没有 tracker 构造入口。
- [ ] recovery 目前只能永久隔离资源；尚无“MMC idle + APBDMA stop + cache sync”组合恢复 token。
- [ ] `ManuallyDrop` 是 fail-closed 所有权策略，未来 executor 必须明确记录泄漏/隔离资源并提供受证明的恢复路径。
- [ ] controller interrupt tracker 不负责 W1C；实际 executor 必须在 IRQ owner 下保存 snapshot、ack 并防止重复 delivery。
- [ ] CMD18 仍缺 CMD12 stop/auto-stop contract，不能用于多块真实读取。
- [ ] 状态机尚未绑定 `OwnedDmaBuffer<FromDevice>` 与 `PreparedSession`，泛型 fixture 不证明 cache coherency。
- [ ] 下一批应定义组合 recovery typestate：同时持有 MMC post-state 和 APBDMA recovery session，只有两侧 quiesce、INT clear/readback 与 sync_for_cpu 都成功后才能取回 buffer。

### 参考与许可证

- `docs/references/loongson2-mmc-upstream.md`
- 本批为 WaterOS 自有 typestate/测试实现，未复制第三方代码。

### 提交

- `[feat] model LS2K1000 MMC read completion`

## 2026-08-10：批次 85——LS2K1000 MMC/APBDMA 组合恢复 typestate

### 任务与设计

1. 审计 APBDMA `RecoverySession→QuiescedSession→finish()` 与 DmaMapping ownership/sync 契约。
2. MMC 恢复要求 cleanup 前后 snapshot：post 必须 command/data idle、CARG/CCTL=0、INT readback=0。
3. DMA 恢复要求独立的 quiesced evidence；token 在生产构建中没有构造器。
4. MMC 与 DMA 两侧均 quiesced 后才允许执行 `sync_for_cpu`。
5. sync 失败必须返回完整 recovery 以便重试，不能 drop 或取回 owned resource。

组合 recovery 按值继承批次 84 的失败资源和原始 completion evidence。资源持续保存在
`ManuallyDrop<B>`；只有 MMC gate、DMA gate 和 synchronizer 三步全部成功，才转换为
`ReadRecovered<B>` 并开放 `into_buffer()`。当前组合 recovery 仅能由 test fixture 构造。

### 完成内容

- [x] 新增无生产构造器的 `ReadDmaQuiescedEvidence`，作为未来 APBDMA QuiescedSession adapter 输出。
- [x] 新增 `ReadCombinedRecovery<B>`、`ReadCombinedRecoveryFailure<B>` 与 `ReadRecovered<B>`。
- [x] 组合 recovery 保留原始 `DeferredReadPlan`、completion failure 和 partial completion evidence。
- [x] `record_mmc_quiesced(before,after)` 验证 after CSTS/DSTS idle。
- [x] before/after 任一含 INT[31:10] 未知位均拒绝，保留具体 unknown mask。
- [x] after INT 非零返回 `MmcInterruptStillPending`，要求 W1C 后 readback 为零。
- [x] after CARG/CCTL 非零返回 `MmcCommandRegistersDirty`。
- [x] `record_dma_quiesced()` 消费独立 token；MMC/DMA evidence 均拒绝重复提交。
- [x] 新增 `ReadRecoverySync<B>`，只在两侧 gate 都满足后调用一次。
- [x] 缺 MMC/DMA evidence 时 sync 调用次数保持零，资源继续隔离。
- [x] sync failure 返回 `SyncForCpu(DriverError)` 和原 recovery；重试成功后才归还资源。
- [x] production 仍无 completion tracker、combined recovery 或 DMA quiesced token 构造入口。
- [x] 没有接入真实 APBDMA session、DmaMapping、RegisterIo、IRQ owner 或 machine init。

### 验证证据

- 2K1000 驱动 host 单测 129 项全部通过；新增 4 项组合恢复聚合测试。
- MMC-first 与 DMA-first 两种 gate 顺序均验证：sync 仅调用一次，buffer 从 40 更新后返回 41。
- MMC invalid matrix 覆盖 CSTS active、DSTS active、post unknown INT、pre unknown INT、pending DFIN、dirty CARG/CCTL。
- 每个 MMC invalid fixture 均保留 recovery，可用后续 clean snapshot 重试成功。
- missing-gate fixture 先后验证 MmcEvidenceMissing/DmaEvidenceMissing，sync call count 始终为零。
- fault-injected sync 第一次返回 IoError，recovery 保留两侧 gate；第二次 sync 成功后返回原资源。
- duplicate matrix 覆盖 MMC 与 DMA quiesce evidence 重复提交。
- production 组件 `cargo check` 无新增 warning；LoongArch64 精确 target check 通过，仅有仓库既有 warning。
- 新代码为纯内存 generic typestate，未执行物理 MMIO、APBDMA stop、cache instruction 或卡片访问。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：MMC idle/cleanup/INT readback 与 APBDMA stop confirmation 能否稳定组合，尚未逐板验证。
- [ ] `ReadDmaQuiescedEvidence` 尚未由真实 `apbdma::QuiescedSession` 产生；生产构建没有 adapter。
- [ ] `ReadRecoverySync` fixture 只模拟失败/重试；没有绑定真实 `DmaMapping<FromDevice>::complete_from_device()`。
- [ ] 两份 MMC snapshot 没有 generation counter；未来 executor 必须在独占 owner 下固定顺序采集。
- [ ] before/after 验证不执行 W1C/cleanup；真实恢复必须保留每个 write/read fault stage。
- [ ] `ManuallyDrop` recovery 会永久隔离未恢复资源；未来需要 board-level quarantine/诊断统计，避免静默内存损失。
- [ ] CMD18 stop/CMD12、card state 和 partial block count 仍未进入 recovery contract。
- [ ] 下一批应在 APBDMA 模块增加一次性、不可伪造的 quiesced handoff token，并用 DmaMapping fixture 将 token、sync 和 CPU ownership真正串起来；仍保持 MMC production executor 关闭。

### 参考与许可证

- `docs/references/loongson2-mmc-upstream.md`
- 本批复用仓库自有 APBDMA/DMA ownership API，未引入第三方代码。

### 提交

- `[feat] isolate LS2K1000 MMC read recovery`

## 2026-08-10：批次 86——APBDMA quiesced handoff 与 MMC recovery adapter

### 任务与设计

1. 让 APBDMA `QuiescedSession` 输出一次性 handoff，而不是 finish 后只返回 `()`。
2. handoff 在 cache sync 成功后转换为持有 CPU-owned mappings 的 typed result。
3. MMC adapter 必须从同一个 handoff 同时生成 DMA-quiesced evidence 与 retryable synchronizer。
4. sync failure 保留原 handoff，不能重新生成 token、丢 mapping borrow 或错误恢复 owner。
5. 用真实 `DmaMapping<FromDevice>` 和 APBDMA stop typestate 串联组合 recovery。

`QuiescedHandoff` 只能由已确认 stop 的 `QuiescedSession::into_handoff()` 产生。其 `finish()`
复用现有 `finish_transfer()`：payload/descriptor 仍为 device owner 时执行 sync_for_cpu，部分成功后
重试只处理仍为 device owner 的 mapping。成功返回 `CpuOwnedHandoff`，消费它才释放两个 mutable borrow。

### 完成内容

- [x] 新增不可 Clone/Copy、字段私有的 `QuiescedHandoff<'a,D,P>`。
- [x] `QuiescedSession::into_handoff()` 消费 quiesced session，保留 completion 与两份 mapping borrow。
- [x] 新增 `CpuOwnedHandoff<'a,D,P>`；只有 handoff finish 成功才能构造。
- [x] `CpuOwnedHandoff::into_mappings()` 返回原 descriptor/payload mutable borrow，此时 owner 均已恢复 CPU。
- [x] handoff finish failure 返回 `SessionFailure<DriverError,QuiescedHandoff>`，同一 handoff 可重试。
- [x] 新增 `ReadApbdmaRecoverySync`，内部持有 `Option<QuiescedHandoff>` 或成功后的 `CpuOwnedHandoff`。
- [x] `ReadDmaQuiescedEvidence::bind_apbdma_handoff()` 一次性、同源地产生 token 与 synchronizer。
- [x] synchronizer 第一次成功后拒绝重复 sync；失败则把原 handoff 放回，保留 retry 能力。
- [x] adapter 实现批次 85 的 `ReadRecoverySync<B>`，由 combined recovery 在 MMC/DMA gates 后调用。
- [x] 增加 test-only combined recovery fixture，生产构建仍无 recovery/tracker 起点。
- [x] 没有新增 machine init、IRQ runtime、MMC executor 或默认数据路径调用。

### 验证证据

- 2K1000 驱动 host 单测 131 项全部通过；新增 2 项 APBDMA handoff/adapter 测试。
- safe stop fixture 依次执行 prepare→start→stop→handoff→finish，最终两份 mapping 均 `is_cpu_owned()`。
- mapping 的 `cpu_region()` 只有在 CpuOwnedHandoff 产生后成功。
- cross-module fixture 从同一 handoff 取得 token/synchronizer，把 token 提交给 MMC combined recovery。
- descriptor cache backend 第一次 sync_for_cpu 注入 IoError：combined recovery 返回失败，handoff 可重试。
- 第二次 sync 成功后同时取得 `ReadRecovered` 与 `CpuOwnedHandoff`，descriptor/payload owner 均为 CPU。
- synchronizer 第三次调用稳定返回 InvalidParam，证明 one-shot sync 不可重复。
- 既有 APBDMA stop timeout/probe error/partial write/recovery tests 全部继续通过。
- production 组件 `cargo check` 无新增 warning；全部测试使用内存 OrderIo/DmaMapping，没有物理 DMA。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：APBDMA `confirm_stopped` 的真实可靠性、cache backend 和 MMC 同步时序仍需两板验证。
- [ ] adapter 尚未由生产 MMC executor 调用；completion/combined recovery 构造器仍限定测试配置。
- [ ] generic combined resource B 与 APBDMA payload mapping 的同一性目前由未来 executor 负责，尚无不可伪造 identity token。
- [ ] CpuOwnedHandoff 证明软件 DmaMapping owner 已切回 CPU，不证明物理 cache line 已按板级规则正确失效。
- [ ] dropping 未完成的 handoff 会释放 Rust borrow，但 mapping 仍保持 Device owner，CPU access API 会拒绝；未来应增加显式 quarantine telemetry。
- [ ] descriptor 与 payload sync 部分成功后的顺序沿用现有 `finish_transfer`，真机 cache failure 的可恢复性未知。
- [ ] MMC post cleanup 仍是抽象 snapshot gate，没有实际 W1C/write/readback executor。
- [ ] 下一批应增加 mapping identity contract：将 deferred read 的 byte length/DATA address 与 APBDMA TransferPlan、payload DmaRegion 做精确匹配，防止错误 buffer/handoff 被组合。

### 参考与许可证

- `docs/references/loongson2-mmc-upstream.md`
- 本批复用仓库自有 APBDMA/DMA API，未引入第三方代码。

### 提交

- `[feat] hand off quiesced LS2K1000 DMA reads`
## 2026-08-10：批次 87——绑定 MMC 读计划与 APBDMA 交接身份

### 本批任务与设计

1. 审计 `TransferPlan`、`Completion`、`QuiescedHandoff` 是否保留同一次传输的完整身份。
2. 将 MMC 读计划与 APBDMA 的方向、长度、DATA 寄存器、descriptor/payload mapping 和 cache 策略逐项绑定。
3. 绑定失败必须归还原始 handoff，禁止用布尔 evidence 丢失 DMA mapping ownership。
4. 在进入 device ownership 前拒绝内部编码与 mapping 不一致的 APBDMA plan。
5. 本批只闭合纯软件身份链，不启用生产 MMC executor 或真实读路径。

### 已完成

- [x] `DmaMapping` 新增只读 identity region/direction；device-owned 时仍不能通过 `cpu_region()` 取得 CPU 访问权。
- [x] `Completion` 保留完整 `TransferPlan`，stop/IRQ completion 不再把身份压缩成单一 invalidate 布尔值。
- [x] `QuiescedHandoff::identity()` 同时暴露 transfer、descriptor mapping 与 payload mapping 的稳定身份。
- [x] `prepare_transfer` 在任何 ownership 转移前校验 descriptor 内存地址、方向 command、长度覆盖、start order、clean/invalidate policy。
- [x] MMC adapter 校验合法读计划、DeviceToMemory、精确字节数、MMC DATA 地址、两份 mapping 的物理区间和方向。
- [x] 新增细分 `ReadDmaIdentityError`；任何 mismatch 都返回原始 `QuiescedHandoff`，可重试或安全 finish。
- [x] 测试覆盖损坏读计划、有效但长度不同的读计划、DATA 地址不同、MemoryToDevice 方向和 mapping/descriptor 编码不一致。
- [x] 没有接入 machine init、IRQ runtime、真实 cache backend 或 MMC activation gate。

### 验证证据

- 2K1000 驱动 host 单测 132 项全部通过；handoff 聚焦 3 项和 mapping mismatch 聚焦 1 项通过。
- 绑定失败链连续复用同一个 handoff：`ReadPlanInvalid`→`ByteLength`→`DataRegisterAddress`→正确绑定，随后 fault-injected cache sync 可重试成功。
- MemoryToDevice handoff 被拒绝后仍可 finish，并归还两份 CPU-owned mapping。
- production 组件 `cargo check`、LoongArch64 精确 feature target check 与 `make kernel-la` 通过；仅有仓库既有 warning。
- 全部 53 项 Python host 测试与 topology/畸形 DTB matrix 通过；dtc warning 均来自预期畸形输入。
- `git diff --check` 通过；全部新增测试使用内存模型，没有物理 MMIO、DMA 或卡片访问。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：两块目标板的 MMC DATA/APBDMA 物理地址、DMA 可见地址与 cache coherency 仍需逐板确认。
- [ ] `UNVERIFIED_ON_HARDWARE`：软件保存的 `TransferPlan` 与 handoff identity 不证明控制器实际取走并执行了同一 descriptor。
- [ ] identity metadata 不授予 CPU 数据访问，但物理 cache line 的 clean/invalidate 正确性仍取决于未来板级 backend。
- [ ] production MMC executor 仍未启用；本批 adapter 只为未来 executor 建立不可丢失 ownership 的绑定边界。
- [ ] CMD18 的 stop/CMD12、卡状态与 partial block count 尚未纳入恢复契约。
- [ ] 下一批应实现测试限定的读事务启动顺序：先绑定合法 read/DMA plan，再 prepare APBDMA，最后才允许发布 MMC command，并覆盖每个部分失败的 rollback/recovery ownership。

### 提交

- 本批计划提交：`[feat] bind LS2K1000 MMC reads to DMA mappings`
## 2026-08-10：批次 88——约束 MMC 读事务启动顺序

### 本批任务与设计

1. 审计 deferred read、APBDMA prepared/running session 与 MMC command 发布之间的现有转换路径。
2. 增加启动前 binding proof，避免只在 DMA stop 后才发现 MMC/DMA 身份不一致。
3. 以 typestate 固定 `bind→prepare DMA→start DMA→publish MMC` 顺序。
4. 每个失败阶段保留精确 ownership：未触及硬件可 cancel，可能写入硬件必须 stop/recovery。
5. publisher 本批保持 test-only，不开启真实 MMC 数据命令或 production activation gate。

### 已完成

- [x] 抽取统一 `validate_read_dma_identity`，启动前 binding 与 stop 后 handoff 共用完全相同的身份规则。
- [x] 新增 `ReadDmaBinding`；在 mapping 仍 CPU-owned 时绑定 read plan、TransferPlan、descriptor/payload identity。
- [x] 新增 `PreparedReadDmaSession`、`RunningReadDmaSession` 和 carrying-plan start failure。
- [x] prepared 状态只能 cancel/start；只有 running 状态在 test 配置下具有 publish 转换。
- [x] publish 成功产生 `PublishedReadDmaSession`；publish 失败原样返回 running session，不能释放 DMA mapping。
- [x] start untouched failure 保留 cancellable prepared session；MayHaveWritten failure 保留 APBDMA recovery session。
- [x] prepare cache-sync failure 验证 descriptor rollback 且两份 mapping 均回到 CPU ownership。
- [x] 没有实现真实 DCTL/CARG/CCTL 写入，没有接入 machine init、IRQ runtime 或 block device registration。

### 验证证据

- 2K1000 驱动 host 单测 135 项全部通过；新增 3 项启动顺序/故障 ownership 聚合测试。
- 正常 fixture 验证 APBDMA `0→start_order` 写入完成后才调用一次 CMD17 publisher，随后 stop 才归还 mapping。
- pre-start 错误 DATA 地址在任何 cache sync/order write 前被拒绝，两份 mapping 保持 CPU-owned。
- prepare fault、untouched start fault、MayHaveWritten start fault 分别走 rollback、cancel、stop/recovery 三条独立路径。
- fault-injected MMC publish 返回 IoError 和原 running session；显式 stop/finish 后才恢复 CPU ownership。
- production 组件 `cargo check`、LoongArch64 精确 feature target check 与 `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 通过；仅有既有 warning。
- 全部 53 项 Python host 测试与 topology/畸形 DTB matrix 通过；dtc warning 来自预期畸形输入。
- `git diff --check` 通过；测试均为内存 order/cache/publisher 模型，没有物理 MMIO、DMA 或卡片访问。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：目标板实际要求 DMA 先启动还是 MMC DCTL/CCTL 先发布，仍需对照硬件 trace 验证。
- [ ] `UNVERIFIED_ON_HARDWARE`：APBDMA start order 写入完成不等于 DMA engine 已经取走 descriptor。
- [ ] test-only publisher 只记录 CMD17，没有执行 DCTL、BSIZE、TIMER、INT、CARG、CCTL 的真实寄存器事务。
- [ ] Rust `must_use` 只能提示 session 被丢弃，未来 production owner 还需要持久 slot/quarantine 防止逻辑泄漏。
- [ ] CMD18/CMD12、卡状态、partial block count 和 IRQ completion 并未进入本批启动状态机。
- [ ] 下一批应实现隔离的 MMC data-command 发布事务：固定 setup/command 写序，覆盖每个 MMIO fault stage，并在任何失败时归还 carrying-running-DMA recovery，而不启用默认读路径。

### 提交

- 本批计划提交：`[feat] order LS2K1000 MMC read startup`
## 2026-08-10：批次 89——隔离 MMC 数据命令发布事务

### 本批任务与设计

1. 审计数据命令寄存器、non-data command transaction 与上游 CRC/CHECK 行为。
2. 实现 one-shot publisher，固定 DCTL→BSIZE→TIMER→INT→CARG→CCTL 写序。
3. publisher 代码参与 production target 编译，但 permit 没有 production constructor，默认路径保持关闭。
4. 每次 write failure 都按“可能已到达硬件”处理，返回精确 stage 和此前成功写数。
5. 与上一批 running-DMA typestate 集成，publisher 失败不得释放 DMA mapping。

### 已完成

- [x] 新增 `ReadDataPublishPermit`；仅 test 配置能构造 activation capability。
- [x] 新增 `ReadDataCommandPublisher<R>`、六阶段 `ReadDataPublishStage`、稳定 failure 与 success receipt。
- [x] 非法 read plan 在任何 MMIO 前拒绝，且修正后仍可首次发布。
- [x] 一旦尝试任意物理 write，无论成功或失败均拒绝第二次发布。
- [x] CMD17/CMD18 均固定写 DCTL、BSIZE、TIMER、INT W1C、CARG、CCTL。
- [x] CCTL 仅设置 command index、HOST、START、WAIT_RESPONSE；LONG_RESPONSE 和未验证的 CHECK(bit13) 均保持零。
- [x] fault matrix 覆盖 DataControl、BlockSize、Timer、ClearInterrupts、CommandArgument、CommandControl 六个阶段。
- [x] running-DMA 集成测试改用真实 publisher；CARG fault 后返回原 running session并显式 stop/finish。
- [x] 没有轮询命令/数据完成，没有创建 production permit，没有接入 machine init 或 block device。

### 验证证据

- 2K1000 驱动 host 单测 138 项全部通过；新增 3 项 publisher 顺序/fault/invalid-plan 测试。
- CMD17 fixture 精确写 offset `[2c,28,24,3c,08,0c]`，argument=7；CMD18 byte-address fixture argument=1024。
- 两种命令均返回 `writes_completed=6`，重复调用返回 AlreadyAttempted 且没有第七次 write。
- 六阶段故障分别保留 0～5 个已确认 write，并记录本次失败 write 的 uncertain stage。
- 非法 byte_length fixture 零 write；同一 publisher 随后接受修正后的合法 plan。
- APBDMA→MMC 集成中，真实六次 MMC write 发生在 DMA start 后；第五次 write fault 后 mapping 仍为 device-owned，stop 后才归还。
- production 组件 check、LoongArch64 精确 feature target check 与 `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 通过；仅有既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：APBDMA start 与 DCTL/CCTL publish 的真实先后要求、posted write 可见性仍需两板 trace。
- [ ] `UNVERIFIED_ON_HARDWARE`：上游没有编程 CCTL.CHECK；本批刻意保持 bit13=0，不能宣称响应 CRC 已由硬件验证。
- [ ] `RegisterIo::write32` 无 WriteEffect，失败阶段只能保守视为可能写入，不能安全重试 publisher。
- [ ] publisher 只负责启动写序；尚未绑定 command response、DFIN、APBDMA IRQ 三类 completion evidence。
- [ ] CMD18 仍缺 CMD12/卡状态/partial block count；不能作为 production multi-block read 使用。
- [ ] 下一批应让 published session 持有 completion tracker，接收 command/data/DMA evidence，并保证任何 error 都携带 published running ownership 进入 stop/recovery。

### 提交

- 本批计划提交：`[feat] stage LS2K1000 MMC read commands`
## 2026-08-10：批次 90——绑定读完成证据与 running DMA ownership

### 本批任务与设计

1. 审计 `PublishedReadDmaSession` 与 `ReadCompletionTracker<B>` 的资源模型。
2. 让真实 published session 本身成为 tracker 的 owned resource，而不是另存可伪造布尔 token。
3. 覆盖 command response、DFIN/data error、DMA completion/failure 的所有成功和失败转换。
4. completed/recovery 都只能取回仍携带 APBDMA running borrow 的 published session。
5. 任一路径均需显式 stop/finish，completion evidence 不得直接恢复 CPU ownership。

### 已完成

- [x] `PublishedReadDmaSession::into_completion_tracker` 将同一 read plan 和 running APBDMA session 原子移入 tracker。
- [x] 新增 published-session 专用 completed/recovery extractor；不会返回裸 buffer 或伪造 quiesce。
- [x] 六种 command/data/DMA 成功排列全部接到真实 publisher+running DMA fixture。
- [x] 五类 data/unknown interrupt error 全部归还携带 published session 的 recovery。
- [x] command Timeout/ResponseCrc/Io 与 DMA Start/Completion/Stop failure 全部保留 running ownership。
- [x] command、DFIN、DMA 三类重复 evidence 全部进入 recovery，并保留此前 evidence。
- [x] 所有 test success/recovery 最终均显式 APBDMA stop/finish 后才恢复两份 mapping。
- [x] 没有把 completion tracker 接入生产 IRQ owner，没有创建 production publish permit 或默认 block device。

### 验证证据

- 2K1000 驱动 host 单测 141 项全部通过；新增 3 项 published completion ownership 聚合测试。
- 6 种成功排列中前两项始终 Pending，第三项才 Completed，evidence 三字段均为 true。
- 每个 Completed 都仍返回 `PublishedReadDmaSession`；stop order write 与 cache finish 后 mapping 才 CPU-owned。
- data error matrix 覆盖 timeout、receive CRC、transmit CRC、program error、unknown bit31。
- explicit failure matrix 覆盖 3 种 command failure 与 3 种 DMA failure；duplicate matrix 覆盖三类 fact。
- 每个 failure/repeat recovery 都可取回原 published session并显式 stop，没有资源或 executor borrow 丢失。
- production 组件 check、LoongArch64 精确 target check 与带 remote-debug-monitor 的 kernel-la 构建通过；仅有既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：command response、DFIN、APBDMA completion 的真实中断先后与合并方式未知。
- [ ] 当前 command_validated 仍由 fixture 提交，不读取 RSP0，也不证明响应 CRC policy。
- [ ] DMA completion fact 仍是软件事件；尚未绑定 acknowledged APBDMA IRQ 和 descriptor status。
- [ ] tracker complete 仅表示三项软件事实齐全，不表示 MMC/DMA 已 quiesced；本批刻意保留显式 stop。
- [ ] CMD18 的 CMD12/card-state/partial-block recovery 尚未进入 completion tracker。
- [ ] 下一批应实现隔离的读命令完成 observer：有界读取 INT/RSP0、固定 W1C/cleanup 顺序、完整 read/write fault stages，并将结果喂给 carrying published tracker。

### 提交

- 本批计划提交：`[feat] retain LS2K1000 DMA through read completion`
## 2026-08-10：批次 91——观察 MMC 读命令完成与合并中断

### 本批任务与设计

1. 审计 non-data command 的 INT polling、W1C、RSP0 与 CARG/CCTL cleanup 顺序。
2. 实现 permit-gated、one-shot、有界的读命令完成 observer。
3. 在 CSENT 成功路径固定 INT read→W1C→RSP0→CARG=0→CCTL=0。
4. 保存所有 poll 的 INT union，将与 CSENT 合并或早先出现的 DFIN/data error 原子送入 tracker。
5. 任意 IO/timeout/CRC failure 都保留精确 stage、poll count、INT union，并携带 published running ownership recovery。

### 已完成

- [x] 新增 `ReadCommandObservePermit`，没有 production constructor，默认读路径继续关闭。
- [x] 新增 `ReadCommandCompletionObserver<R>`、8 类 stage、stable failure 与 success receipt。
- [x] poll limit=0 在任何 MMIO 前拒绝；observer 一旦尝试即禁止重复调用。
- [x] command timeout 优先于 RESPCRC，二者优先于 CSENT；bounded poll exhaustion 返回 command Timeout。
- [x] CSENT 后 W1C 全部已观察 known bits，再读 RSP0并依次清 CARG/CCTL。
- [x] 新增原子 `command_observed(interrupts)`，data error/unknown 优先，command 与 DFIN 在同一次 tracker transition 中提交。
- [x] observer failure bridge 同样先保留已观察 data error/unknown，避免后续 poll timeout 覆盖更具体错误。
- [x] carrying published tracker 集成覆盖 CSENT|DFIN 与 DMA fact 合并完成、首读 IO failure recovery。
- [x] 没有接入生产 IRQ owner、没有启用 response CRC policy、没有自动归还 DMA mapping。

### 验证证据

- 2K1000 驱动 host 单测 145 项全部通过；新增 4 项 observer/bridge 聚合测试。
- delayed fixture 依次观察 0、DFIN、CSENT；receipt polls=3、INT union/ack=`DFIN|CSENT`。
- 成功操作序列精确为 3 次 INT read、INT W1C、RSP0 read、CARG zero、CCTL zero。
- IO fault matrix 覆盖 INT read、W1C write、RSP0 read、CARG cleanup、CCTL cleanup 五个阶段。
- timeout/CRC matrix 覆盖 poll timeout、RESPCRC|CSENT、CTIMEOUT|RESPCRC|CSENT，并验证优先级。
- 先观察 data timeout 后 poll exhaustion 时，tracker recovery 保留 DataTimeout而不是较晚 Command Timeout。
- carrying fixture 中 DMA fact + CSENT|DFIN 原子完成；observer IO error 返回原 published session，均显式 stop/finish 后 CPU-owned。
- production 组件 check 无新增 warning；LoongArch64 target 与带 remote-debug-monitor 的 kernel-la 构建通过。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：INT bits 是否持续到 W1C、CSENT/DFIN 是否合并，以及 RSP0 可读时点仍需逐板 trace。
- [ ] `UNVERIFIED_ON_HARDWARE`：RESPCRC status 只做保守错误分类，CCTL.CHECK 仍未启用。
- [ ] RegisterIo read/write failure 不提供 WriteEffect；W1C/cleanup failure 后不能安全重试 observer。
- [ ] observer 是 bounded polling fixture，尚未绑定 masked/acknowledged MMC IRQ owner 生命周期。
- [ ] DMA fact 仍是软件事件，尚未由 acknowledged APBDMA IRQ 与 descriptor status typestate 产生。
- [ ] CMD18 的 CMD12/card state/partial blocks 仍未实现。
- [ ] 下一批应将 acknowledged APBDMA IRQ、descriptor status inspection 与 tracker DMA fact 绑定，并在类型转换中保留 command/data evidence 和 mapping ownership。

### 提交

- 本批计划提交：`[feat] observe LS2K1000 MMC read commands`

## 2026-08-10：批次 92——以 APBDMA IRQ 与描述符状态驱动读取完成

### 本批任务与设计

1. 审计 APBDMA `RunningSession`→`IrqCompletionSession`→`QuiescedSession` 与 MMC tracker 的资源边界。
2. 只有匹配的 acknowledged APBDMA IRQ 才释放 executor borrow；错误 IRQ 返回原 carrying tracker。
3. IRQ 后必须同步并读取描述符状态；缓存、读取和未知状态失败均返回同一 acknowledged tracker 供重试。
4. `Complete` 才提交 DMA completion fact；`HardwareError(raw)` 进入保留原始状态值的 recovery。
5. 无论 command/data evidence 先到还是 DMA evidence 先到，都保留 read plan、evidence 与两份 mapping ownership。

### 已完成

- [x] 新增 test-only acknowledged/quiesced read-DMA carrying session，把 APBDMA typestate 直接嵌入 tracker 资源类型。
- [x] `acknowledge_dma_irq` 在错误 source 时保留 `PublishedReadDmaSession` 和 executor borrow，正确 source 后转为 acknowledged session。
- [x] `inspect_dma_status` 在 cache/read/decode failure 后保留 `IrqCompletionSession`，可在同一资源上重试。
- [x] 明确完成状态转为 carrying `QuiescedSession` 后提交 DMA fact，不再用独立软件调用模拟 IRQ completion。
- [x] `ReadDmaFailure::Hardware(u32)` 保存描述符原始错误状态，并返回 quiesced recovery 供安全 finish。
- [x] completed/recovery 专用 extractor 只返回 carrying quiesced session；CPU ownership 仍需显式 finish。
- [x] 没有创建 production IRQ/status permit，也没有启用默认 MMC block device。

### 验证证据

- 2K1000 驱动 host 单测 148 项全部通过；新增 3 项 IRQ/status/tracker 聚合测试。
- 正确 IRQ + `Complete` 在 command/DFIN 已到时产生 Completed，三项 evidence 均为 true。
- DMA evidence 先到时保持 Pending，随后 command/DFIN 到齐才 Completed，证明转换顺序无关。
- 错误 IRQ 返回 `UnexpectedIrq`，同一 tracker 随后接受正确 IRQ；资源和 executor 状态没有丢失。
- cache sync 一次失败、descriptor read 失败、Unknown 状态均保留 acknowledged tracker，依次重试后成功；reader call 顺序也被断言。
- `HardwareError(0x80000042)` 精确进入 `Dma(Hardware(0x80000042))` recovery；显式 finish 后 descriptor/payload 才 CPU-owned。
- production 组件 check、RISC-V `make check`、LoongArch64 `make kernel-la` 均通过；仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：descriptor status `0x100` 表示 Complete、最高位表示 HardwareError 目前只是 fixture decoder，不是已确认的 2K1000 APBDMA 位定义。
- [ ] `UNVERIFIED_ON_HARDWARE`：acknowledged DMA IRQ 是否足以证明引擎停止访问 descriptor/payload，仍需两块目标板上的 trace 与 cache 压力测试。
- [ ] 当前 carrying transition 仅在 test 配置可构造；production IRQ owner 尚未把 APBDMA token 路由给 MMC read session。
- [ ] descriptor status 的 authoritative 文档/上游实现尚未确认，因此 production 必须继续 fail-closed，不能使用 fixture decoder。
- [ ] MMC command observer 仍是 polling fixture，尚未绑定 MMC IRQ owner；APBDMA 与 MMC 两路 IRQ 的真实合并/先后仍待真机验证。
- [ ] CMD18 的 CMD12、card state 与 partial-block recovery 尚未实现。
- [ ] 下一批应审计主线/厂商 APBDMA 描述符完成位来源；若仍无法确认，则实现 production-safe `StatusUnverified` 诊断路径和 IRQ owner carrying handoff，但保持数据面禁用。

### 提交

- 本批计划提交：`[feat] bind LS2K1000 DMA IRQ to read completion`

## 2026-08-10：批次 93——保守交接 APBDMA IRQ 且拒绝臆造状态位

### 本批任务与设计

1. 复核 Linux `loongson2-apb-dma` 的 descriptor、final IRQ 和 terminate 路径，寻找 `stats` 位的权威定义。
2. 在没有位定义时固定 production fail-closed policy，不把 host fixture 数值解释带入目标代码。
3. 将 deferred APBDMA board IRQ owner 改造成容量为 1 的 acknowledged-token handoff。
4. runtime 只允许在 owner 不处于 handler transaction 时可变访问并取走 token。
5. 验证 wrong/duplicate IRQ、一次性消费、keep-masked、status unverified 和 carrying resource recovery。

### 已完成

- [x] 上游审计确认 descriptor 有 `stats` word，但驱动没有定义其位含义，ISR 不读取该字段。
- [x] 上游 final IRQ/terminate/pause 都写 `64BIT_EN|STOP`，没有轮询 order bit 或 descriptor status 证明 idle。
- [x] 参考文档明确 `UnverifiedStatusDecoder` 是 production policy；测试 decoder 的 `0x100`/最高位规则仅为 fixture。
- [x] `DeferredApbDmaOwner` 现在绑定 expected IRQ，并保留一个不可复制的 `AcknowledgedIrq` token。
- [x] wrong source 返回 `UnexpectedIrq`；未消费 token 时的重复 IRQ 返回 `PendingNotConsumed`，且不会覆盖首个 token。
- [x] `take_acknowledged()` 一次性移出 token，source 始终保持 masked，不产生伪造的 device-clear/rearm evidence。
- [x] `IrqOwnerTable::get_mut` 与 `BoardIrqRuntime::owner_mut` 提供 handler transaction 之外的 coordinator handoff 点；InHandler 状态继续拒绝访问。
- [x] board owner→published read tracker→`IrqCompletionSession` 集成 fixture 证明 production decoder 对 raw `0x100` 仍返回 `StatusUnverified` 并保留同一 session。
- [x] 默认 diagnostic runtime 仍拒绝构造/激活 APBDMA owner，未开启生产 MMC 数据面。

### 验证证据

- 2K1000 驱动 host 单测 150 项全部通过；新增 owner token 与 fail-closed tracker 集成测试各 1 项。
- wrong IRQ 不生成 pending token；正确 IRQ 生成一个；重复 IRQ 不替换；take 第一次成功、第二次为 None。
- owner 的每种路径均返回 `KeepMasked`，没有产生 `DeviceAckedIrq` 或 rearm disposition。
- carrying fixture 从 owner 取得 token 后释放 executor borrow；volatile-like reader 确实读取 raw `0x100`，production decoder 仍拒绝完成。
- `StatusUnverified` 后同一 acknowledged tracker 可重试；fixture-only decoder 仅用于模型资源清理，最终显式 finish 后 mapping 才 CPU-owned。
- production 组件 check、RISC-V `make check` 与 LoongArch64 `make kernel-la` 全部通过；仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：APBDMA IRQ 的 device-side clear 语义、STOP 是否真正停止总线访问、何时可 rearm 均未知。
- [ ] `UNVERIFIED_ON_HARDWARE`：descriptor `stats` 的任何完成/错误位均未知；当前 production 永远不能由该字段声明成功。
- [ ] owner handoff 已可保存/取出 token，但 production diagnostic runtime 刻意不构造 APBDMA owner，完整板级 coordinator 尚未启用。
- [ ] published MMC read session 和完成 tracker 的构造仍受 test-only permit 限制，因此数据面保持不可达。
- [ ] `StatusUnverified` 后只能保留 session 或由具有平台证据的 unsafe recovery 回收；真机验证前没有自动成功路径。
- [ ] MMC IRQ owner 与 APBDMA owner 尚未由同一 read transaction coordinator 配对，迟到/串线 IRQ 的 transaction identity 仍需加强。
- [ ] 下一批应给 read transaction 增加 generation/cookie，把 MMC 与 APBDMA 两路 owner token 绑定到同一 carrying session；仍保持 activation 和 descriptor success decoding 关闭。

### 提交

- 本批计划提交：`[feat] retain LS2K1000 DMA IRQ evidence`

## 2026-08-10：批次 94——以 generation 配对 MMC 与 APBDMA 读取中断

### 本批任务与设计

1. 为每次读取分配非零且不回绕的 software transaction generation。
2. MMC/APBDMA 两路 owner 必须显式 arm 同一 generation 后才能生成 read receipt。
3. MMC receipt 保留已 W1C 的 known INT snapshot；DMA receipt 保留线性 `AcknowledgedIrq`。
4. 两路 receipt 在交给 carrying tracker 前必须通过 generation pair 校验。
5. 未 arm、重复 arm、未消费 receipt、错代和重复 receipt 均 fail-closed，read source 不提前 rearm。

### 已完成

- [x] 新增 `ReadTransactionId` 与 `ReadTransactionSequence`；0 非法，计数到 `u64::MAX` 后返回 Exhausted 而非回绕。
- [x] `MmcCommandOwner::arm_read` 与 `DeferredApbDmaOwner::arm_read` 绑定同一代际，均保持单 active/single pending。
- [x] 新增 `acknowledge_interrupt_observed`，在保持原 ack API 兼容的同时返回精确 known interrupt snapshot。
- [x] armed MMC IRQ 清 W1C 后生成 `MmcReadIrqReceipt` 并 KeepMasked；普通未 armed command 行为仍按原契约 rearm。
- [x] armed APBDMA IRQ 生成携带原 `AcknowledgedIrq` 的 `ApbDmaReadIrqReceipt`；未 arm IRQ 返回 NotArmed 并 KeepMasked。
- [x] `ReadIrqPair` 接受任意到达顺序，只在 MMC/DMA generation 都匹配且各一份时返回 ready pair。
- [x] mismatch/duplicate submit 返回原 receipt，不吞掉线性 DMA token。
- [x] 集成 fixture 将同代 MMC `CSENT|DFIN` 与 DMA receipt 配对，再依次驱动 APBDMA typestate 和 carrying tracker，最终三项 evidence 全部成立。
- [x] production data path、APBDMA status success decoder 与 IRQ rearm coordinator 仍保持关闭。

### 验证证据

- 2K1000 驱动 host 单测 154 项全部通过；新增 4 项 sequence/owner/pair/carrying 集成测试。
- sequence 从 1 单调分配，0 无法构造；从 `u64::MAX` 分配一次后稳定 Exhausted。
- 两路 owner 均拒绝 AlreadyArmed；存在 pending receipt 时拒绝新 generation，重复 IRQ 不覆盖首个 receipt。
- APBDMA 未 arm IRQ 明确记录 NotArmed；所有 read-mode IRQ disposition 均为 KeepMasked。
- stale generation 被 pair 拒绝并返还 receipt；同源 duplicate 被分类，MMC→DMA 与 DMA 等待 MMC 的顺序均覆盖。
- carrying fixture 中仅 DMA receipt 到达时 tracker 保持 Pending；同代 MMC `CSENT|DFIN` 到达后才 Completed。
- completed evidence 精确为 command/data/DMA 三项 true；显式 quiesced finish 后两份 mapping 才 CPU-owned。
- production 组件 check、RISC-V `make check`、LoongArch64 `make kernel-la` 均通过；仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：cookie 是纯软件 arm identity，硬件 IRQ 不携带 tag；source rearm 后物理迟到 IRQ仍可能被归到新 generation。
- [ ] 当前策略因此在 receipt 消费后仍不提供 read IRQ rearm；真机确认 clear/idle/latency 边界前必须保持 masked。
- [ ] MMC W1C receipt 证明 observed known bits 被清除，不证明 RSP0 已稳定或 APBDMA 已停止访问内存。
- [ ] production diagnostic runtime 仍不构造 APBDMA owner，尚无同时 arm 两个 runtime owner 的板级 coordinator。
- [ ] published carrying tracker 仍由 test-only permit 构造，production 无法提交配对 receipt 到真实块设备请求。
- [ ] transaction abort 还没有对两个 owner 做原子 disarm/drain；任一 arm 后启动失败会需要显式回滚协议。
- [ ] 下一批应实现可恢复的双 owner arm transaction：prepare→arm MMC→arm DMA 的部分失败回滚、abort/drain 与 generation retirement，保证启动错误不遗留 armed/pending owner。

### 提交

- 本批计划提交：`[feat] pair LS2K1000 read IRQ generations`

## 2026-08-10：批次 95——事务化 arm、abort 与读取 IRQ generation 退役

### 本批任务与设计

1. 消除 MMC arm 成功、DMA arm 失败后遗留半个 generation 的窗口。
2. owner 提供按 generation 校验的 binding、disarm 与 pending 状态分类。
3. 双 owner abort 必须先验证两侧，再同时 retire，防止错代操作只清掉一边。
4. 已到达的 receipt 必须原样 drain；尤其 APBDMA `AcknowledgedIrq` 不得丢失或复制。
5. 覆盖无 IRQ 启动失败、单路 IRQ、双路 IRQ、错代、重复 abort 和 owner occupied。

### 已完成

- [x] 新增 `ReadIrqOwnerBinding::{Armed, Pending}`，两种 owner 均可只读检查当前 generation/phase。
- [x] `disarm_read(transaction)` 只撤销匹配 Armed；NotArmed/WrongTransaction/PendingNotConsumed 均不改变状态。
- [x] `arm_read_owners` 先 arm MMC 再 arm DMA；DMA 拒绝时立即回滚本次 MMC arm，保留 DMA 原 binding。
- [x] `drain_read_owners` 先验证 MMC、DMA 两侧均属于目标 generation，再一次性清除 armed/pending。
- [x] `DrainedReadIrqs` 返回可选 MMC/DMA receipt；没有 IRQ 时不会臆造 receipt，单路/双路 pending 均完整返回。
- [x] DMA-side generation mismatch 时 MMC binding 保持不变，证明双侧预检在 mutation 之前完成。
- [x] pending DMA receipt 仍携带原线性 acknowledged token，可交给 APBDMA completion/status/stop recovery。
- [x] retirement 只处理软件 owner 状态，不产生 source rearm、descriptor success 或 hardware-idle evidence。

### 验证证据

- 2K1000 驱动 host 单测 157 项全部通过；新增 3 项 pair arm/drain/retirement 故障测试。
- 预占 DMA generation 时 pair arm 返回 DMA/AlreadyArmed，MMC 回到 None，DMA 原 generation 不变。
- 手工构造 MMC=current、DMA=old 后 drain current 返回 DMA/WrongTransaction，两侧 binding 均保持原值。
- 两侧 Armed 的启动失败模型 drain 后返回 `(None,None)` 并全部退役；重复 drain 返回 MMC/NotArmed。
- DMA-only pending 时单 owner disarm 返回 PendingNotConsumed；错代 drain 不改变 Armed/Pending 状态。
- 正确 drain 返回原 DMA receipt/IRQ；随后新 generation 可重新 arm，并能完整 drain MMC+DMA 两份 receipt。
- production 组件 check、RISC-V `make check` 与 LoongArch64 `make kernel-la` 全部通过；仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：软件 retire 不会清除设备上潜在的迟到 IRQ；retire 后仍不允许自动 rearm。
- [ ] `UNVERIFIED_ON_HARDWARE`：pending DMA receipt 只能证明 LIOINTC mask/ack，不能证明 APBDMA stopped。
- [ ] pair helper 当前操作两个直接 owner 引用；production runtime 中 owner 位于不同 slot，尚无跨 slot 原子借用事务。
- [ ] startup executor 尚未将 prepare→arm owners→start DMA→publish MMC 串成一个 production typestate，因此本批启动失败由 owner model 覆盖。
- [ ] drain 返回 receipt 后由调用者负责送入 completion/recovery；当前没有 must-use transaction guard 防止调用者遗忘 abort。
- [ ] MMC source 的安全 rearm 需要 generation retired、controller condition cleared 与 session completion；APBDMA rearm 还需要未知的 device clear/idle 证据。
- [ ] 下一批应增加 owning `ReadIrqArmGuard`，Drop 保持 fail-closed，显式 commit 后才能进入 started session；并设计 runtime 两 slot 的预约/归还协议。

### 提交

- 本批计划提交：`[feat] rollback LS2K1000 read IRQ arms`

## 2026-08-10：批次 96——以 owning guard 预约双 IRQ owner slot

### 本批任务与设计

1. 为 runtime owner table 增加两个不同 IRQ slot 的原子可变借用，消除板级协调器逐个取 owner 的中间窗口。
2. 双 slot 借用使用 `split_at_mut`，既证明引用不别名，也保持调用者请求的返回顺序。
3. `ReadIrqArmGuard` 独占 MMC/APBDMA 两个 owner 并事务化 arm；guard 存活期间 runtime 无法再次服务或改动这两个 slot。
4. 未提交 guard 的 `Drop` 只回滚仍精确处于本 generation 的软件 Armed 状态；显式 `commit` 后才返回已预约 token。
5. 保持硬件 activation、IRQ rearm 与 descriptor success decoding 关闭，并覆盖 slot/variant/状态错误。

### 已完成

- [x] `IrqOwnerTable::get_pair_mut` 拒绝相同 slot，并用 `split_at_mut` 同时借用两个 Ready owner；反向请求仍按请求顺序返回。
- [x] pair borrow 对 missing、InHandler 和 same-slot 均在交出引用前 fail-closed。
- [x] `BoardIrqRuntime::owners_mut` 将双 slot 借用提供给板级事务协调层。
- [x] 新增 `ReadIrqArmGuard`，内部隐藏两个 owner 引用，构造时通过既有 `arm_read_owners` 原子绑定同一 transaction generation。
- [x] 未 commit 的 guard 在析构时仅当两侧仍为同代 Armed 才同时清除；不会触发 rearm、硬件寄存器写或伪造 completion evidence。
- [x] `commit` 消费 guard 并返回不可复制的 `ArmedReadIrqs`，token 可读取 transaction identity，但不声称 DMA 已启动。
- [x] `reserve_read_irq_owners` 先取得两个 runtime slot，再验证 MMC/APBDMA owner variant；slot 与 variant 错误均不改变 owner 状态。
- [x] production data path、APBDMA hardware start、status success decoder 与 IRQ rearm coordinator 仍保持关闭。

### 验证证据

- 2K1000 驱动 host 单测 160 项全部通过；本批新增双 slot borrow、guard drop/commit 和错误 variant 共 3 项测试。
- 反向 slot 顺序可正确取得并修改对应 owner；same slot、missing slot 与 InHandler slot 均返回精确错误。
- 未 commit guard 离开作用域后两侧 binding 均为 None；commit 后两侧保持同代 Armed，并可由既有 drain 协议完整退役。
- swapped owner variants 被拒绝且无 mutation；guard 独占借用从类型层阻止存活期间 runtime 再借这两个 owner。
- production 组件 check、RISC-V `make check` 与 LoongArch64 `make kernel-la` 全部通过；仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：双 slot 借用与 guard 只证明软件 owner 状态一致，不证明控制器已 idle，也不能排除物理迟到 IRQ。
- [ ] guard 目前只覆盖 arm preparation；production 尚未把它与 DMA start、MMC command publish 串成不可跳步的 typestate。
- [ ] `ArmedReadIrqs` token 还没有绑定真实 `RunningReadDmaSession`，commit 只表示两个 owner 已 armed，不表示硬件已启动。
- [ ] production diagnostic runtime 仍拒绝构造 APBDMA owner，因此真实 runtime 尚未执行本预约入口。
- [ ] 本批仍不提供任何 IRQ source rearm；软件 rollback 不会清除设备侧潜在 pending condition。
- [ ] 若 guard 内部状态因未来代码改动意外不再是同代双 Armed，`Drop` 会保持 fail-closed 而不强行做部分清理；显式恢复仍需 coordinator。
- [ ] 下一批应把 `ArmedReadIrqs` 消费进 DMA start/publish typestate，并确保 start/publish 任一步失败都经 runtime coordinator drain/rollback；在 production permit 开放前继续以 model fixture 验证。

### 提交

- 本批计划提交：`[feat] guard LS2K1000 read IRQ arms`

## 2026-08-10：批次 97——将 IRQ generation 线性绑定到读取 DMA 启动状态

### 本批任务与设计

1. 审计 `ArmedReadIrqs`、`PreparedReadDmaSession`、APBDMA start recovery 与 MMC publish 的状态边界。
2. 已预约的 IRQ generation 必须作为线性 capability 进入 DMA typestate，不能与 Running/Recovery session 任意拆开。
3. 确定未写入硬件的启动失败走 Prepared cancel；可能写入的失败必须先走 Recovery stop，再归还 token。
4. MMC publish 失败时保留 Running DMA 与 generation 的绑定，显式 stop/cache finish 后才允许退役 owner。
5. runtime retire 消费 token，并在两个 slot/variant/generation 全部验证后才同时 drain；失败返回原 token 供重试。

### 已完成

- [x] `ArmedReadIrqs` 标记为 must-use，并新增 `bind_prepared_dma`，把双 owner generation 绑定到真实 `PreparedReadDmaSession`。
- [x] 新增泛型 `IrqArmedReadDmaSession<S>`，线性携带 token 穿过 Prepared、Running、Recovery、test-only Published 与 Quiesced 状态。
- [x] 没有提供通用 session/token 拆分接口；只有 Prepared cancel 或 Quiesced cache finish 成功后才返还 `ArmedReadIrqs`。
- [x] 新增 `IrqArmedReadDmaStartFailure::{Prepared,Recovery}`，精确保留 read plan、错误、对应恢复状态及同一 transaction generation。
- [x] untouched start write 故障返回 IRQ-armed Prepared session；cancel 同步 mapping 后返还 token。
- [x] may-have-written start 故障返回 IRQ-armed Recovery session；必须 stop 并完成 CPU cache ownership 恢复后返还 token。
- [x] test-only MMC publish 成功/失败均保持 token 与 DMA session 绑定；publish 失败不能提前退役 IRQ owners。
- [x] 新增 `retire_read_irq_owners`；slot/variant/drain 任一失败均返回原 `ArmedReadIrqs`，成功才消费 token 并返回两路可选 receipt。
- [x] retire 仅改变软件 owner 状态，不产生 source rearm、APBDMA idle、descriptor success 或物理 IRQ generation evidence。

### 验证证据

- 2K1000 驱动 host 单测 160 项全部通过；本批强化既有启动顺序、两类 start fault、publish fault 与 runtime guard/retire 测试。
- 正常模型链路保持 DMA start 先于六次 MMC publish write；generation 101 从 Prepared 一直保留到 stop/cache finish。
- start 第一次 untouched write 故障被分类为 Prepared，generation 102 只能在 cancel 后取回，两个 mapping 恢复 CPU-owned。
- start 第二次 may-have-written 故障被分类为 Recovery，generation 103 只能在 stop/finish 后取回，不能误走 cancellable path。
- MMC 第五次 publish write 故障保留 generation 104 与 Running session；显式 stop/finish 后才取回 token，mapping 才 CPU-owned。
- runtime 以反向 slot 退役返回 MmcOwnerVariant 和原 token；用正确 slot 重试后同时清除两个同代 Armed owner。
- production 组件 check、RISC-V `make check` 与 LoongArch64 `make kernel-la` 全部通过；仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：线性 token 只证明软件 owner/session 顺序，不会写入硬件，物理 IRQ 本身仍不携带 generation。
- [ ] `UNVERIFIED_ON_HARDWARE`：模型对 untouched/may-have-written 的分类依赖 `OrderIo` effect；真实 volatile MMIO 无法可靠报告总线写是否到达设备。
- [ ] production 的 `ReadDataPublishPermit` 仍无构造入口，Published 状态只在测试中启用；本批没有开放真实 MMC/APBDMA activation。
- [ ] production diagnostic runtime 仍不构造 APBDMA owner，因此尚未从真实 runtime 取得 token 并调用 DMA start。
- [ ] IRQ 到达后的 `MmcReadIrqReceipt`/`ApbDmaReadIrqReceipt` 尚未与 IRQ-armed Running/Published session 合并为一个消费接口；当前 model tracker 仍是旁路测试链。
- [ ] Rust `#[must_use]` 只能产生编译告警，不能禁止调用者显式 `drop` Running/Recovery session；production coordinator 仍需封装并持有整个生命周期。
- [ ] retire 成功仍不 rearm；真机确认 controller condition cleared、APBDMA stopped 与迟到 IRQ 窗口前必须继续 keep-masked。
- [ ] 下一批应让 IRQ-armed Published session 原子消费同代 `ReadIrqPair`/runtime drain 结果，再进入 completion tracker；错误 generation、单路 pending 与 stop recovery 都必须保留 session 和线性 receipt。

### 提交

- 本批计划提交：`[feat] bind LS2K1000 IRQ arms to DMA start`

## 2026-08-10：批次 98——原子取得同代双 IRQ receipt 并进入 carrying tracker

### 本批任务与设计

1. 区分 abort drain 与 completion pair take：单路 pending 时不得退役另一侧 Armed owner。
2. 只有 MMC/APBDMA 两侧都处于同代 Pending 时，才原子取走两份 receipt 并消费 `ArmedReadIrqs`。
3. generation、slot、owner variant 或 pending 状态任一不匹配时，runtime owner、receipt 和 Published session 全部保持不变。
4. APBDMA completion 拒绝错误 IRQ source 时必须返回原 `AcknowledgedIrq`，不得只保留 Running session。
5. test-only Published typestate 消费 runtime pair 后进入 carrying completion tracker；转换失败保留两路 receipt evidence 与 DMA session。

### 已完成

- [x] 新增 must-use `ReadyReadIrqPair`，同时携带同代 `MmcReadIrqReceipt` 与线性 `ApbDmaReadIrqReceipt`。
- [x] 新增 `take_pending_read_irq_pair`，先双 slot/variant/pending/generation 全量预检，再一次性 take 两份 receipt 并清除 owner binding。
- [x] MMC-only 或 DMA-only pending 返回精确 `ReadPendingPairError::*Binding`，不清除已到 receipt，也不撤销另一侧 Armed。
- [x] wrong generation、wrong slot/variant 与内部 receipt invariant 错误均 fail-closed，并通过 `ReadPendingPairFailure` 返回原 armed token。
- [x] 新增 `IrqSessionFailure`；APBDMA executor 在 UnexpectedIrq/Idle 时返还传入的原 `AcknowledgedIrq`，Running session 同时保持可重试/stop。
- [x] `ReadDmaIrqFailure` 继续向上携带 acknowledged token，修复旧接口错误路径可能丢失线性 ack evidence 的缺口。
- [x] test-only `IrqArmedReadDmaSession<Published>::take_pending_pair` 将 runtime pair 与 Published session 原子绑定；失败返回原 session。
- [x] paired session 成功后消费 DMA receipt 进入 `AcknowledgedReadDmaSession` tracker，同时继续携带 MMC receipt 等待 command/data evidence。
- [x] paired DMA source 不匹配时保留 MMC receipt、DMA transaction、原 acknowledged token 与 Published Running session，可显式 stop recovery。
- [x] production publish permit、APBDMA status success decoder、真实 activation 与 IRQ rearm 均保持关闭。

### 验证证据

- 2K1000 驱动 host 单测 162 项全部通过；新增 runtime atomic pair 和 paired failure recovery 2 项，并强化既有端到端 generation 测试。
- DMA-only pending 时 pair take 返回 MMC/Armed(current)；DMA owner 仍为 Pending(current)，Published session 可在 MMC 到达后重试。
- 两侧 Pending(current) 时用 wrong generation 尝试返回 MMC/Pending(current)，两份 receipt 均未被消费；正确 token 随后成功取得 pair。
- 正常模型链路覆盖 runtime reserve→DMA start→MMC publish→DMA IRQ→等待 MMC IRQ→原子 pair→DMA ack/status→MMC snapshot→Completed。
- 完成 evidence 精确为 command response、data finished、DMA finished 三项 true，stop/cache finish 后 mappings 才回到 CPU-owned。
- executor 预期 IRQ 与 runtime DMA IRQ 不一致时，paired failure 精确保留 MMC generation、DMA generation、原 DMA IRQ ack token 和 Running DMA session。
- low-level Executor 在 Idle 与 UnexpectedIrq 两条错误路径均返回原 acknowledged token；正确 IRQ 仍可重试完成或显式 stop。
- production 组件 check、RISC-V `make check` 与 LoongArch64 `make kernel-la` 全部通过；仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：同代 Pending 仍是纯软件归属；硬件 IRQ 不携带 generation，迟到 IRQ 在未来 rearm 后仍可能误归类。
- [ ] `UNVERIFIED_ON_HARDWARE`：APBDMA IRQ ack 不证明 channel stopped；production status decoder 仍拒绝把未知 descriptor status 判为成功。
- [ ] Published/pair/tracker 的整合接口仍受 `#[cfg(test)]` 保护，因为 production `ReadDataPublishPermit` 尚无安全构造入口。
- [ ] pair 成功后 runtime owner 已退役但 source 仍 masked；tracker 后续失败只能 stop/recover，不能重新放回 owner table。
- [ ] `PairedAcknowledgedReadDmaSession` 尚未封装 status inspection 与 MMC snapshot application 的全部失败路径；当前端到端测试会显式拆出 tracker。
- [ ] production diagnostic runtime 仍不构造 APBDMA owner，也没有保存一个跨 trap/worker 生命周期的 read coordinator。
- [ ] abort 路径继续使用允许 Armed/Pending 混合的 `drain_read_owners`；completion 路径必须使用本批新增的 only-both-pending API，调用层尚未统一封装。
- [ ] IRQ source rearm 继续关闭；真机确认 MMC condition clear、APBDMA idle 和迟到 IRQ 窗口前不可启用。
- [ ] 下一批应让 paired acknowledged 状态线性携带 MMC receipt 穿过 descriptor status inspection、status failure retry 和 command snapshot apply，直到 Quiesced/Completed 或显式 stop recovery。

### 提交

- 本批计划提交：`[feat] pair LS2K1000 runtime read IRQ receipts`

## 2026-08-10：批次 99——聚合 MMC 分段完成并线性应用 paired status

### 本批任务与设计

1. 修复 read owner 在首个 `CSENT-only` snapshot 就生成唯一 receipt、导致后续 `DFIN` 永久丢失的问题。
2. 同 generation 累积已 W1C 的 MMC known bits；只有 `CSENT+DFIN` 或任一 command/data error 才形成 terminal receipt。
3. source 保持 masked 时提供显式 serialized recheck，只采样/W1C MMC 状态，不伪造 LIOINTC acknowledgement 或 rearm evidence。
4. MMC receipt 必须线性穿过 APBDMA descriptor status inspection；inspection failure 保留 paired acknowledged session 供重试。
5. DMA status 成功后才一次性应用 terminal MMC snapshot；descriptor hardware error 与 MMC command/data error进入 quiesced recovery。

### 已完成

- [x] 抽出 `clear_masked_interrupt_snapshot`，复用原 read→known W1C→unknown reject 顺序，同时明确不生成 controller ack/rearm token。
- [x] 新增 `read_interrupt_snapshot_terminal`；正常读取要求同时观察 `CSENT|DFIN`，command timeout、response CRC 与四类 data error 可立即终止等待。
- [x] `MmcCommandOwner` 新增 generation-local `read_interrupts` union；非终止 snapshot 保持 Armed，不产生 receipt。
- [x] 新增 `recheck_masked_read` 与 `MmcReadRecheckError`；recheck 失败保留已累积 snapshot，并记录精确 MMC ack error。
- [x] arm、disarm、guard rollback、pair-arm rollback、abort drain 与成功 pair take 均清除 accumulator，旧代 snapshot 不会泄漏到新 generation。
- [x] 新增 `PairedDmaInspectionFailure`，descriptor cache/read/decode 失败返回原 MMC receipt 与 acknowledged DMA tracker，可原状态重试。
- [x] 新增 `PairedDmaStatusProgress`；DMA Complete 进入携带 MMC receipt 的 quiesced pending 状态，HardwareError 保留 receipt 并进入 quiesced recovery。
- [x] `PairedQuiescedReadDmaSession::apply_mmc_receipt` 一次性消费 terminal snapshot，不能在 DMA quiesced 前提前应用。
- [x] 新增 `ReadCompletionTracker::terminal_irq_observed`，command timeout/response CRC 优先于把 CSENT 解释为 validated response。
- [x] production status decoder、MMC publish permit、真实 APBDMA activation 与两路 IRQ rearm 均保持关闭。

### 验证证据

- 2K1000 驱动 host 单测 164 项全部通过；新增 split MMC recheck 与 paired status/error recovery 2 项，并升级 paired 端到端测试。
- 首次 `CSENT-only` 后 owner 保持 Armed、receipt 为 None、source KeepMasked；显式 recheck `DFIN` 后 receipt 精确合并为 `CSENT|DFIN`。
- partial snapshot 后 disarm，再以新 generation 观察 `DFIN-only` 仍保持 Armed，证明旧 `CSENT` 未跨代泄漏。
- paired status 首次使用 production `UnverifiedStatusDecoder` 返回 StatusUnverified，并保留同一 MMC generation；fixture decoder 重试成功。
- 正常链路在 DMA status Complete 后应用 MMC `CSENT|DFIN`，completion evidence 三项全 true，finish 后 mappings 才 CPU-owned。
- descriptor `0x8000_0042` 被 fixture decoder 分类为 HardwareError，MMC receipt 保留在 paired recovery，failure 精确携带硬件 status。
- terminal MMC command-timeout receipt 在 DMA quiesced 后进入 `Command(Timeout)` recovery，不会被误判为 validated response。
- production 组件 check、RISC-V `make check` 与 LoongArch64 `make kernel-la` 全部通过；仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：masked recheck 的寄存器 read/W1C 顺序来自现有 Linux-derived status 语义，尚未在 2K1000LA 实机确认。
- [ ] `UNVERIFIED_ON_HARDWARE`：`CSENT` 与 `DFIN` 的实际分离时延、是否在 source masked 时仍锁存以及 W1C 后新事件的可见性未知。
- [ ] `recheck_masked_read` 目前是单次显式调用；production 尚无带 deadline/poll budget 的 worker coordinator，不能无限轮询。
- [ ] paired status/application wrapper 仍为 `#[cfg(test)]`，因为 production publish permit 与 APBDMA success decoder都未开放。
- [ ] descriptor HardwareError recovery 中保留 MMC receipt 仅用于诊断；当前没有统一错误报告对象把两侧 raw snapshot 一并上报块层。
- [ ] terminal data error可在没有 CSENT 时形成 receipt，并由 `command_observed` 的 data-error priority 正确进入 recovery；尚缺 paired 每一 error bit 的完整 fault matrix。
- [ ] pair take 后 owner 已退役且 source 继续 masked；status/MMC apply 后没有任何 rearm transition。
- [ ] production diagnostic runtime 仍不构造 APBDMA owner，也没有跨 trap/worker 保存 coordinator 状态。
- [ ] 下一批应实现有界的 masked MMC recheck coordinator：明确 Pending/Terminal/Timeout/IO fault 状态，保留 generation 与 Running DMA，并在 timeout 时走 stop→quiesced recovery；继续不启用 rearm。

### 提交

- 本批计划提交：`[feat] carry LS2K1000 paired read completion`

## 2026-08-10：批次 100——有界调度 masked MMC recheck 与超时恢复

### 本批任务与设计

1. 将 masked MMC recheck 建模为可暂停的单步协调器；每步只采样一次寄存器，不在驱动内循环、延时或隐式 rearm。
2. 用非零 poll budget 区分 Pending、Terminal、Timeout 与可恢复 fault，并记录 transaction、剩余预算和已完成采样数。
3. 每次 step 均校验 runtime slot、owner variant 与 generation；错误代次不得读取或改变当前 transaction 的硬件/owner 状态。
4. 空状态与非终止 partial snapshot 消耗一次预算；I/O、未知状态位及 owner 错误不消耗预算，并返还原协调器供调用方决定重试或恢复。
5. Timeout 不撤销 MMC owner、不丢弃 partial snapshot；调用方必须先 stop DMA、完成 cache ownership 回收，再退役可能为 Armed/Pending 混合状态的两路 owner。

### 已完成

- [x] 新增 must-use `BoundedMmcReadRecheck`，以非零 `u16` 预算携带同一 `ReadTransactionId`，并公开 remaining/polls_completed 诊断信息。
- [x] 新增 `BoundedMmcReadRecheckProgress::{Pending,Terminal,Timeout}`；Pending 显式返还下一步 token，避免一次调用无限轮询。
- [x] runtime 中已为同代 Pending 时直接返回 Terminal，不重复读取/W1C；只有同代 Armed owner 才执行一次 masked sample。
- [x] `NoKnownPending` 与 `Ok(false)` 均计为一次有效采样；terminal snapshot 在当步返回 Terminal，预算耗尽则精确返回 Timeout。
- [x] 新增 recoverable `BoundedMmcReadRecheckFailure`；slot/variant/generation、I/O/W1C 与未知位错误均返还未消费的协调器。
- [x] 修正 unknown-only MMC status 被误报为 `NoKnownPending` 的问题；现返回精确 `UnknownPending`，且不会尝试清除未知位。
- [x] 新增模型超时恢复：DMA receipt 已 Pending、MMC 仍 Armed 时，先停止 Published DMA 并 finish cache ownership，再以 abort drain 退役混合 owner 状态。
- [x] production publish permit、APBDMA success decoder、真实 activation 与 IRQ rearm 继续保持关闭。

### 验证证据

- 2K1000 驱动 host 单测 166 项全部通过；新增有界 coordinator 状态矩阵与 timeout stop/drain 集成测试 2 项。
- 三步预算依次观察 empty、`CSENT-only`、`DFIN`，最终 Terminal 且 polls_completed 精确为 3；partial snapshot 合并为 `CSENT|DFIN`。
- 两次 empty sample 在预算 2 时精确 Timeout；MMC owner 仍为同代 Armed，未生成 receipt、未 rearm source。
- wrong generation 返回当前 Armed binding 且协调器仍可恢复；零预算构造被拒绝。
- unknown-only status 返回 `UnknownPending`，不消耗协调器预算，owner 与 generation 保持不变。
- DMA 已 Pending 而 MMC timeout 时，stop/finish 后 mappings 才回到 CPU-owned；混合 drain 返回原 DMA acknowledged receipt，MMC receipt 仍为空。
- production 组件 test/check、RISC-V `make check` 与 LoongArch64 `make kernel-la` 全部通过；仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：实际轮询间隔和合理预算尚无实机时序数据；当前 budget 是 step 次数，不是 wall-clock deadline。
- [ ] `UNVERIFIED_ON_HARDWARE`：MMC source masked 时后续状态的锁存可见性、W1C 与新事件竞争窗口仍需 2K1000LA 实测。
- [ ] I/O/未知位 fault 不消耗预算是为了允许受控重试，但 production 调用方必须另设 fault retry/deadline 策略，避免错误路径无限调度。
- [ ] Timeout 保留 owner accumulator，但当前没有公开的统一 recovery report 把 partial MMC snapshot 与 DMA receipt 一并上报。
- [ ] coordinator 尚未存入 production worker/runtime；当前测试通过显式线性 token 模拟跨调度 step。
- [ ] timeout stop/drain 集成仍为 test-only，因为 production publish permit 与 APBDMA descriptor status decoder 尚未开放。
- [ ] production diagnostic runtime 仍不构造 APBDMA owner，真实 read activation 与 completion worker 入口继续被安全门阻断。
- [ ] IRQ rearm 继续关闭；真机确认 condition clear、DMA idle 和迟到 IRQ generation 风险前不可启用。
- [ ] 下一批应增加 timeout/fault recovery report，线性携带 partial MMC snapshot、poll statistics 与 DMA/owner drain evidence，再考虑接入 production worker storage。

### 提交

- 本批计划提交：`[feat] bound LS2K1000 masked MMC rechecks`

## 2026-08-10：批次 101——线性收集 MMC timeout/fault 恢复证据

### 本批任务与设计

1. 将 bounded recheck 的 Timeout 与 RecheckFault 统一建模为可记录的 recovery cause，保留 polls_completed、remaining 与精确底层错误。
2. 最终 recovery report 必须同时携带 transaction、MMC partial accumulator 和两路 owner drain receipt。
3. 普通 `ArmedReadIrqs` 不能证明 DMA 已停止；新增只能在 quiesced DMA 完成 cache ownership 回收后产生的线性 recovery token。
4. report 生成前先校验 runtime slot、owner variant 与两侧 generation；任一不匹配时不得清 accumulator 或消费 receipt。
5. report 只做软件状态取证/退役，不访问额外寄存器、不解释 completion 成功，也不 rearm 中断源。

### 已完成

- [x] 新增 `ReadRecoveryCause::{Timeout,RecheckFault}`；fault 从 `BoundedMmcReadRecheckFailure::recovery_cause` 推导，错误与轮询统计不会由调用方重新拼接。
- [x] 新增 must-use `QuiescedReadIrqs`，只有 `IrqArmedReadDmaSession<QuiescedSession>::finish_recovery` 能构造。
- [x] `finish_recovery` 先完成 descriptor/payload cache ownership 回收；失败继续返还原 quiesced session，不会伪造 recovery-ready token。
- [x] 新增 must-use `ReadRecoveryReport`，线性携带 transaction、cause、`partial_mmc_interrupts` 与 `DrainedReadIrqs`。
- [x] 新增 `retire_quiesced_read_recovery`：先双 slot/variant/generation 预检，再在 drain 清零前捕获 MMC accumulator。
- [x] 新增 `ReadRecoveryRetireFailure`；失败返还原 cause 与 `QuiescedReadIrqs`，runtime owners 和 receipts 保持可重试。
- [x] timeout 模型升级为 empty→`CSENT-only`，同时模拟 DMA 已 Pending；最终 report 保留 partial `CSENT` 和原 DMA acknowledged receipt。
- [x] production publish permit、APBDMA status decoder、真实 activation 与 IRQ rearm 继续保持关闭。

### 验证证据

- 2K1000 驱动 host 单测 167 项全部通过；新增 wrong-generation recovery report 原子保持测试 1 项，并升级 timeout 集成测试。
- timeout report 只能在 Published DMA stop 后调用 `finish_recovery` 获得 token；此时 descriptor/payload mappings 已回到 CPU-owned。
- 两步预算观察 empty、`CSENT-only` 后 Timeout，report 的 polls_completed 为 2、partial MMC bits 精确为 `1 << 6`。
- 同一 report 返回 MMC receipt None 和已 Pending DMA 的原 acknowledged IRQ receipt，证明 partial MMC 与 DMA evidence 没有互相覆盖。
- wrong generation 的 quiesced token 返回 MMC/WrongTransaction，cause 与 token 均可恢复；当前 generation 的 MMC/DMA owners 仍为 Armed 且随后可正常 drain。
- unknown-only status 与寄存器 read I/O fault 均形成精确 RecheckFault；有效采样数不增加，remaining 分别保持 1 和 3。
- production 组件 test/check、RISC-V `make check` 与 LoongArch64 `make kernel-la` 全部通过；仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：partial accumulator 只证明软件已 read/W1C 的位，不证明 MMC source masked 时没有新状态在 report 捕获后到达。
- [ ] `UNVERIFIED_ON_HARDWARE`：APBDMA stop confirmation 和 cache maintenance 顺序仍只有寄存器模型/typestate 测试，没有 2K1000LA 实机证据。
- [ ] recovery report 没有 wall-clock timestamp/deadline，只保存 bounded step 统计；生产 scheduler 尚未提供时间来源和 backoff 策略。
- [ ] `RecheckFault` 目前覆盖 unknown status 与 read I/O；W1C write I/O 已由底层测试覆盖，但尚缺贯穿完整 stop/report 链路的注入测试。
- [ ] recovery report 当前保留 raw MMC bitmask 和 DMA receipt，不含 descriptor raw status，因为 timeout 路径尚未进入可信 production status decoder。
- [ ] production worker/runtime 尚未保存 coordinator、Published session 或 report；本批只建立可安全组合的线性 API。
- [ ] test-only publish permit 仍是端到端模型入口；真实 MMC/APBDMA 激活、completion worker 与块层错误上报尚未接通。
- [ ] IRQ rearm 继续关闭；真机验证 condition clear、DMA idle 和迟到 IRQ generation 窗口前不可启用。
- [ ] 下一批应为 production runtime 增加不激活硬件的 read coordinator storage/slot，验证 reserve→publish-state→recheck-state→recovery-report 的独占生命周期与重入拒绝。

### 提交

- 本批计划提交：`[feat] report LS2K1000 read recovery evidence`

## 2026-08-10：批次 102——持久化 read coordinator 独占生命周期

### 本批任务与设计

1. 为跨 trap/worker 的 deferred read 增加 production 可编译的 single-publication slot，但不连接真实硬件 activation。
2. 复用已验证的原子 `DiagnosticRuntimeSlot`，以 RAII reservation 防止半初始化状态和并发重复发布。
3. 用 transaction/generation 守卫 Reserved→Published→Rechecking→RecoveryRecorded 转换；迟到 worker 不得修改新事务。
4. slot 只保存无借用的协调元数据和拥有所有权的 recovery report，不保存带 MMIO/DMA 借用的 session。
5. 正常完成/取消必须显式 release；已记录的线性 recovery report 只能 take，普通 release 不得静默丢弃 evidence。

### 已完成

- [x] 新增 production 模块 `read_coordinator` 与 `ReadCoordinatorSlot`，底层复用 EMPTY/RESERVED/LIVE/SERVICING/DRAINING 原子协议。
- [x] 新增 `ReadCoordinatorReservation`；reservation 未 commit 即 drop 会恢复 Empty，commit 后其他 reserve 返回 Reserved/AlreadyLive/Busy。
- [x] 新增 `ReadCoordinatorPhase::{Reserved,Published,Rechecking,RecoveryRecorded}` 与可复制 `ReadCoordinatorSnapshot`。
- [x] `mark_published` 拒绝零 poll budget，并只允许同 transaction 的 Reserved 状态单向转换。
- [x] `record_recheck` 校验 `remaining + polls_completed == poll_budget`，wrong generation/phase/progress 全部保持 live state 不变。
- [x] `record_recovery` 将完整 `ReadRecoveryReport` 移入 slot；失败通过 must-use `RecordRecoveryFailure` 返还原线性 report。
- [x] snapshot 只暴露 cause、partial bits 与 receipt presence，不复制 `AcknowledgedIrq`。
- [x] `take_recovery` 只接受同 generation 的 RecoveryRecorded 状态，成功移动 report 并重新开放 slot。
- [x] `release` 支持发布前取消和正常完成，wrong generation 恢复 Live；RecoveryRecorded 返回 `RecoveryMustBeTaken`，禁止丢弃证据。
- [x] 模块明确不执行 MMC publish、DMA start、MMIO 或 IRQ rearm，真实激活安全门保持关闭。

### 验证证据

- 2K1000 驱动 host 单测 170 项全部通过；新增 reservation/reentry、跨 worker 状态、线性 recovery take 3 项。
- reservation 期间第二次 reserve 精确返回 Reserved；drop 后可重新 reserve，commit 后其他 transaction 返回 AlreadyLive。
- 同代 Published(budget=4) 可记录 Rechecking(remaining=4,polls=0)；budget=3 的不一致 recheck 被拒绝且原 snapshot 保持。
- stale generation 的 publish/release 均返回 expected/actual transaction；失败 drain 自动恢复 Live，正确 generation 随后可 release。
- wrong-generation recovery record 返还原 report；正确 fault report 入 slot 后 snapshot 保留 partial `CSENT` 和 RecheckFault 摘要。
- RecoveryRecorded 不能通过 release 清除；`take_recovery` 移出原 report 后 slot 回到 Empty，并能为下一 generation reserve。
- production 组件 test/check、RISC-V `make check` 与 LoongArch64 `make kernel-la` 全部通过；仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：worker 调度、trap 并发与迟到 IRQ 时序尚未在 2K1000LA 实机验证，slot 不能作为 rearm 许可。
- [ ] slot 当前持久化 recheck 的 generation/budget/progress 元数据，不拥有实际 `BoundedMmcReadRecheck` token；调用层仍需线性保存该 token。
- [ ] 带借用的 Published DMA session 不能放入 `'static` slot；production 尚需定义拥有 DMA mapping/channel 的请求对象或专用 worker ownership。
- [ ] snapshot 的 receipt booleans 仅供诊断，实际 `AcknowledgedIrq` 只在 `take_recovery` 返回的 report 中可用。
- [ ] 当前没有 scheduler wake/backoff/deadline 接口；Published/Rechecking 转换由调用者显式驱动。
- [ ] success completion 尚无同类 report；调用方确认两路 completion 并处理 mappings 后才能调用 release，slot 本身无法验证该事实。
- [ ] production diagnostic runtime 仍未构造 APBDMA owner，真实 MMC publish permit、status decoder、activation 和 block completion 回调均关闭。
- [ ] 下一批应增加 read coordinator 的独占 service lease，使实际 `BoundedMmcReadRecheck` token 能进入/离开 worker service 而不让 slot 暂时 Vacant，并覆盖 service panic/drop/重入恢复。

### 提交

- 本批计划提交：`[feat] persist LS2K1000 read coordinator state`

## 2026-08-10：批次 103——独占 service 实际 masked recheck token

### 本批任务与设计

1. `BoundedMmcReadRecheck` 改为不可 Clone/Copy 的线性 token，避免 slot 与 worker 同时持有可推进副本。
2. 通用 single-publication slot 增加 LIVE→SERVICING 独占 guard；guard 生命周期内绝不短暂进入 Empty。
3. read coordinator 的 Rechecking 状态实际拥有 bounded token，不再只保存 remaining/polls 元数据。
4. 每个 service 只执行一次 caller/runtime 提供的 masked MMC sample，并原子转换 Pending/Terminal/RecoveryPending。
5. runtime/variant/generation 等可重试调度错误保留 token；MMC I/O/unknown fault 与 budget timeout 固化 recovery cause，禁止继续轮询。

### 已完成

- [x] `DiagnosticRuntimeSlot::service` 返回 `RuntimeService<T>`，LIVE→SERVICING CAS 后独占 `DerefMut`，drop 用 Release store 恢复 LIVE。
- [x] `with_live_mut` 改为复用同一 service guard，原有闭包接口与新长期 guard 共用一套 UnsafeCell 安全边界。
- [x] 移除 `BoundedMmcReadRecheck`、progress、failure 的 Clone/Copy；外部 step 仍在 Pending/错误时显式返还唯一 token。
- [x] 抽出 crate-private `step_in_place`；slot 内直接原位推进 token，避免 service unwind 时留下 `Rechecking(None)`。
- [x] `record_recheck` 现在消费实际 token；phase/progress/slot 错误通过 must-use `RecordRecheckFailure` 返还原 token。
- [x] 新增 `ReadRecheckService` 与 `ReadCoordinatorStepProgress`；Pending 原位保留 token，Terminal 进入不可再 service 的 Terminal。
- [x] Timeout 与 `MmcReadRecheckError` 进入 RecoveryPending；release 返回 `RecoveryMustBeRecorded`，必须先形成完整 stop/drain report。
- [x] RecoveryPending 归档 report 时校验 exact cause；poll count/error 不一致返回 `RecoveryCauseMismatch` 和原 report。
- [x] runtime slot、owner variant、binding/generation 错误返回 `ReadCoordinatorStepFailure`，实际 token 与预算原样留在 Rechecking。
- [x] production publish permit、DMA start/status decoder、真实 activation 与 IRQ rearm 继续关闭。

### 验证证据

- 2K1000 驱动 host 单测 175 项全部通过；read coordinator 新增 service/drop、split terminal、timeout/fault、retryable generation、unwind 5 项。
- service guard 存活时 slot 为 Servicing，snapshot、第二次 service 与 drain 均返回 Busy；drop 后恢复 Live/Rechecking。
- 三次跨 service sample 依次 empty、`CSENT-only`、`DFIN`，remaining 2→1，第三步 Terminal(polls=3)，slot 从未暴露 Empty。
- budget=1 的 empty sample 转为 RecoveryPending Timeout(1)；普通 release 被拒绝，cause 不匹配 report 被线性返还，匹配 report 可归档/take。
- unknown-only status 转为 RecheckFault，polls=0、remaining=2，证明 fault 没有误耗预算。
- runtime MMC owner 绑定另一 generation 时 step 返回 Binding(Armed(bound))；snapshot 仍为 current/Rechecking(2,0)，修正 owner 后同 token 可重试 Terminal。
- 注入 RegisterIo panic 并 catch_unwind 后，RuntimeService drop 恢复 Live，in-place token 保持 remaining=2/polls=0；关闭 panic 后可重试 Terminal。
- production 组件 test/check、RISC-V `make check` 与 LoongArch64 `make kernel-la` 全部通过；仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：SERVICING 原子互斥已由 host CAS/reentry 模型验证，但真实 trap/worker 优先级、关中断边界和迟到 IRQ 时序未知。
- [ ] `UNVERIFIED_ON_HARDWARE`：service 内 masked status read/W1C 的真实可见性与 Linux-derived 位语义仍需 2K1000LA 实测。
- [ ] panic/unwind 测试只证明软件 guard/token 恢复；若 panic 发生在真实 W1C 已产生副作用之后，重试仍必须依赖 accumulator，而不能回滚硬件。
- [ ] slot 不拥有 Published DMA session；timeout/fault 后 production 调用层仍需线性执行 stop→cache finish→owner drain→report。
- [ ] `release` 可用于 Rechecking cancellation，但 slot 本身无法证明调用方已 stop DMA；生产整合层必须只在 quiesced/cancelled 证据后调用。
- [ ] service 没有 deadline、wake time 或 backoff；仍由外部 worker 决定何时调用下一步。
- [ ] Terminal 只证明 MMC owner 已形成 terminal receipt，不证明 DMA status 或整笔块请求完成。
- [ ] production runtime 仍不构造 APBDMA owner，真实 publish/status/completion worker 与块层回调尚未接通。
- [ ] 下一批应把本 slot 接入完整模型链路：reserve→publish→service timeout/fault→Published DMA stop/finish→owner recovery report→slot record/take，覆盖每个中间失败保持。

### 提交

- 本批计划提交：`[feat] service LS2K1000 read rechecks exclusively`

## 2026-08-10：批次 104——原子归档 quiesced read recovery report

### 本批任务与设计

1. RecoveryPending 增加独占 service；recovery cause 从 slot 读取，不允许调用方重复拼接 timeout/fault 统计。
2. 在同一个 SERVICING 临界区执行 quiesced generation 校验、runtime owner drain、partial evidence 捕获与 report publication。
3. generation/slot/variant/drain 失败必须同时保留 slot RecoveryPending 和线性 `QuiescedReadIrqs`，可修正后原位重试。
4. owner 已成功 drain 后，API 不提供尚未把 report 移入 slot 的正常返回点。
5. 继续保持 MMC/APBDMA sources masked，不把 stop/quiesce 解释为传输成功或 rearm 许可。

### 已完成

- [x] 新增 `ReadCoordinatorSlot::service_recovery`，只允许同 generation 的 RecoveryPending 状态进入 SERVICING。
- [x] 新增 must-use `ReadRecoveryService`；`retire_and_record` 自动使用 slot 内 exact `ReadRecoveryCause`。
- [x] `retire_and_record` 复用 `retire_quiesced_read_recovery` 捕获 MMC accumulator 与两路 receipts，成功后直接替换为 RecoveryRecorded。
- [x] 新增 `ReadCoordinatorRecoveryError::{WrongTransaction,Retire}` 与 must-use failure，统一返还 cause 和 quiesced token。
- [x] 在任何 owner 访问前比较 slot transaction 与 quiesced transaction，修复“旧代 token/owners 一致但 slot 已换代”可能先 drain 后发现的窗口。
- [x] runtime reversed slot/owner variant 失败包装精确底层 `ReadIrqRetireError`，slot service drop 后恢复 Live/RecoveryPending。
- [x] test-only 扩大 `QuiescedReadIrqs::fixture` 到 crate 可见，用于无物理 DMA 的 fault recovery owner 模型；production 仍只能由 `finish_recovery` 构造。
- [x] APBDMA timeout 集成测试升级为完整 coordinator slot 生命周期并使用真实模型 stop/cache finish token。
- [x] production activation、publish permit、status decoder 与 IRQ rearm 保持关闭。

### 验证证据

- 2K1000 驱动 host 单测 176 项全部通过；新增 fault recovery service 测试 1 项，并升级 timeout stop/drain 集成测试。
- timeout 链路覆盖 owner reserve→DMA start→MMC publish→slot Published/Rechecking→empty/`CSENT`→RecoveryPending Timeout(2)。
- DMA IRQ 已 Pending；Published session stop/finish 后 mappings 才 CPU-owned，并产生真实 `QuiescedReadIrqs`。
- 首次故意交换 MMC/DMA slot，返回 Retire(MmcOwnerVariant)、原 cause 与 quiesced token；slot 仍 RecoveryPending，正确参数重试成功。
- 成功 snapshot 为 RecoveryRecorded，保留 partial `CSENT`、Timeout(2) 和 DMA receipt presence；take 返回原 acknowledged DMA IRQ。
- fault 链路先累积 `CSENT`，再遇到 unknown-only status 进入 RecheckFault；service drain 前捕获 partial `CSENT`，两路 Armed owners 均被退役。
- wrong quiesced generation 在 owner drain 前返回 expected/actual；slot 仍 RecoveryPending，MMC owner 仍为 Armed(current)，正确 token 随后成功。
- production 组件 test/check、RISC-V `make check` 与 LoongArch64 `make kernel-la` 全部通过；仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：真实 DMA stop confirmation、cache maintenance 和 owner drain 的相对时序仍需 2K1000LA 实测。
- [ ] `UNVERIFIED_ON_HARDWARE`：report publication 后 source 仍 masked；真机确认 MMC condition clear 与 APBDMA idle 前不能 rearm。
- [ ] timeout 端到端使用真实模型 Published/stop/finish；fault owner 测试使用 test-only quiesced fixture，没有重复模拟 DMA mapping stop。
- [ ] `retire_and_record` 对 Rust panic 没有事务回滚；底层 prevalidation 后 drain/赋值路径无 fallible 调用，但 production panic=abort 策略仍需保持。
- [ ] slot 仍不拥有 Published DMA session；外部 worker 必须先完成 stop/finish，再把 quiesced token交给 recovery service。
- [ ] 没有 scheduler deadline/backoff/wake API；timeout 仍是 step budget，不是 wall-clock 时间。
- [ ] Terminal success 路径尚未通过 coordinator service 整合 paired MMC/DMA receipt take、descriptor status 与 completion tracker。
- [ ] production runtime 仍不构造 APBDMA owner，真实 command publish、DMA status decoder、block completion callback 与 IRQ rearm 均关闭。
- [ ] 下一批应实现 Terminal 独占 completion service，把 paired receipt take、DMA acknowledge/status inspection、MMC snapshot apply 与 slot release/recovery 转换串成一个模型闭环。

### 提交

- 本批计划提交：`[feat] archive LS2K1000 read recovery atomically`

## 2026-08-10：批次 105——独占认领 Terminal read completion

### 本批任务与设计

1. Terminal coordinator 增加独占 completion service，禁止两个 worker 重复取得同一对 MMC/APBDMA receipts。
2. completion claim 同时校验 slot transaction、Published session 携带的 armed generation 和 runtime 两个 owner generation。
3. 只有两路 owner 都为 Pending 才原子取得 receipt pair，并把 slot 从 Terminal 转为 CompletionClaimed。
4. 错误 slot、owner variant、binding 或 generation 必须返还原 Published session/armed token，slot 仍保持 Terminal 可重试。
5. 成功链严格保持 pair take→DMA acknowledged/status→MMC receipt→cache finish→coordinator release 的顺序。

### 已完成

- [x] 新增 `ReadCoordinatorPhase/State::CompletionClaimed`，snapshot 可观测一次性 completion ownership 已被取得。
- [x] 新增 `ReadCoordinatorSlot::service_terminal`，只允许同 generation 的 Terminal 状态进入 SERVICING。
- [x] 新增 must-use `ReadTerminalService` 与 `ReadTerminalClaimFailure`；session generation 在访问 runtime 前校验，失败返还唯一 armed token。
- [x] terminal claim 复用 `take_pending_read_irq_pair` 的双槽预校验；成功取得同 generation 的 MMC receipt 和 acknowledged DMA IRQ 后才提交 CompletionClaimed。
- [x] test-only Published typestate wrapper 接入 terminal service，失败时重建完整 Published session，成功时进入既有 paired tracker 链。
- [x] 成功集成测试先故意交换 MMC/DMA 槽，确认错误精确、slot 仍 Terminal、同一会话可按正确槽位重试。
- [x] 成功 claim 后第二个 terminal service 被 WrongPhase(CompletionClaimed) 拒绝，避免 receipt 重复消费。
- [x] production activation、真实 status decoder、block completion callback 与 IRQ rearm 继续关闭。

### 验证证据

- 2K1000 驱动 host 单测 176 项全部通过；paired completion 集成测试升级为 coordinator 独占认领闭环。
- 模型链路覆盖 owner reserve→DMA start→MMC publish→slot recheck Terminal→paired claim→DMA ack→descriptor CPU sync/status→MMC apply→cache finish→slot release。
- 交换槽位返回 Pair(MmcOwnerVariant)，Published session 被返还，snapshot 保持 Terminal；正确参数随后成功认领。
- CompletionClaimed 后再次申请 terminal service 返回 WrongPhase，证明 coordinator 不再暴露第二个 completion lease。
- production decoder 的 StatusUnverified 仍返还同一 paired session；fixture decoder 重试后仅 DMA evidence 不会提前完成。
- MMC receipt 应用后 evidence 三项均为 true；descriptor/payload 映射确认 CPU-owned 后才 release，slot 最终为 Empty。
- production 组件 test/check、RISC-V `make check` 与 LoongArch64 `make kernel-la` 全部通过；完整构建仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：真实 MMC terminal IRQ 与 APBDMA IRQ 的先后、合并、迟到及 trap/worker 并发尚未在 2K1000LA 验证。
- [ ] `UNVERIFIED_ON_HARDWARE`：真实 descriptor status 位解码、DMA completion 可见性与 cache maintenance 屏障仍待实机证据。
- [ ] CompletionClaimed 证明 receipt pair 已被唯一取得，但 slot 类型当前不能证明 cache finish 已完成；release 的调用顺序由线性 session 集成层约束并由模型测试覆盖。
- [ ] paired Published/status/MMC tracker 目前仍是 test-only 整合外壳；production worker 尚未拥有可放入长期请求对象的 DMA mappings/session。
- [ ] claim 后若 descriptor status 或 MMC receipt 表示失败，paired tracker 保留 recovery session 和 receipts，但尚未把 CompletionClaimed 原子转换为 RecoveryPending/Recorded。
- [ ] Terminal service 没有 scheduler wake/deadline/backoff 接口；外部 worker 仍需决定何时检查 DMA owner 是否 Pending。
- [ ] source 在成功 release 后仍保持 masked；slot Empty 不是 rearm 许可，必须先验证真实 controller condition clear、DMA idle 和下一 generation arm 顺序。
- [ ] production runtime 仍不构造 APBDMA owner，真实 command publish permit、status decoder、块层完成回调和 rearm 均关闭。
- [ ] 下一批应把 claimed completion 的 status/MMC 失败原子转入 coordinator recovery，并设计 production-owned read request，使 worker 能持久保存 Published/paired session。

### 提交

- 本批计划提交：`[feat] claim LS2K1000 read completion exclusively`

## 2026-08-10：批次 106——固化 claimed read completion failure

### 本批任务与设计

1. 扩展 recovery cause，使 coordinator 能区分 masked recheck 失败与 receipt pair 已认领后的完成失败。
2. CompletionClaimed 增加独占 failure service；错误 generation、错误 phase 和重复归档均不得改变状态。
3. paired DMA descriptor hardware error 与 MMC terminal snapshot error 都必须进入同一精确状态转换。
4. coordinator 只复制 `ReadCompletionFailure` 摘要，不能消费或伪造 quiesced recovery session。
5. RecoveryPending 后普通 release 必须被拒绝，等待后续完整 evidence archive。

### 已完成

- [x] `ReadRecoveryCause` 新增 `CompletionFailure(ReadCompletionFailure)`，保留 DMA hardware status、MMC command/data error 与 duplicate 分类。
- [x] 新增 `ReadCoordinatorSlot::service_claimed_completion`，只允许同 generation 的 CompletionClaimed 状态进入 SERVICING。
- [x] 新增 must-use `ReadClaimedCompletionService`；`record_failure` 在独占 guard 内原子转换为 RecoveryPending。
- [x] paired acknowledged fixture 改为走真实 coordinator 模型链：Published→Rechecking→Terminal→claim→CompletionClaimed，不再绕过 slot 直接 take pair。
- [x] DMA descriptor hardware error `0x8000_0042` 被记录为 `CompletionFailure(Dma(Hardware(...)))`。
- [x] MMC command timeout snapshot 被记录为 `CompletionFailure(Command(Timeout))`。
- [x] wrong-generation service 在变更前返回 expected/actual，snapshot 仍 CompletionClaimed；正确 generation 可随后重试。
- [x] 第二次 failure service 返回 WrongPhase(RecoveryPending)，RecoveryPending 不能通过 release 丢弃。
- [x] 两条错误链的 `ReadCompletionRecovery<QuiescedReadDmaSession>` 均保留到状态提交之后，并完成 cache ownership recovery。

### 验证证据

- 2K1000 驱动 host 单测 176 项全部通过；既有 paired error 测试升级为 coordinator claim 后 failure 分类测试。
- DMA error 链从同代 pair claim、DMA ack、descriptor CPU sync/status 进入 RecoveryPending，snapshot 精确保留 hardware status。
- MMC error 链先确认 DMA success 仍 Pending，再应用 terminal MMC receipt 进入 RecoveryPending，证明不会把单侧成功误判为整笔成功。
- wrong generation 不改变 CompletionClaimed；正确记录后重复 service 被拒绝，证明失败摘要只有一个 coordinator owner。
- 两条链均在 coordinator 状态固化后继续 `finish()`，descriptor/payload 最终为 CPU-owned；状态转换没有消费 recovery session。
- production 组件 test/check、RISC-V `make check` 与 LoongArch64 `make kernel-la` 全部通过；完整构建仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：真实 descriptor error status、MMC terminal error priority、cache barrier 和 DMA idle 时序仍待 2K1000LA 实测。
- [ ] claim 前 timeout/recheck recovery 从 runtime owner table drain receipts；claim 后 receipts 已被 pair 取走，现有 `retire_and_record` 不能复用于 CompletionFailure。
- [ ] 当前 CompletionFailure 会稳定停在 RecoveryPending，普通 release 被拒绝；尚缺 paired recovery 专用的 report archive API，不能宣称错误闭环已完成。
- [ ] DMA acknowledged receipt 当前进入 paired tracker 后没有作为可归档字段公开；后续需要由 production-owned request 持有 generation、MMC/DMA receipts 与 quiesced session。
- [ ] `record_failure` 接受 Copy 摘要，状态机保证唯一 phase，但类型系统尚不能证明摘要一定来自同一个 paired recovery；生产 wrapper 接线时必须用私有构造的证据 token 收紧。
- [ ] success release 仍由调用顺序保证，slot 本身不能证明 cache finish；后续 owned request 应同时解决 success/failure finalize proof。
- [ ] source 继续保持 masked，RecoveryPending 不是 rearm 许可；真实 condition clear、DMA idle 与下一 generation arm 均未验证。
- [ ] production runtime、publish permit、status decoder、block completion callback 与 scheduler worker 仍未接通。
- [ ] 下一批应设计 `ClaimedReadRecoveryEvidence` 线性 token，保留 transaction、两路 receipts、completion failure 和 quiesced session；cache finish 后由专用 coordinator service 原子归档 report。

### 提交

- 本批计划提交：`[feat] classify LS2K1000 claimed read failures`

## 2026-08-10：批次 107——归档 claimed read recovery evidence

### 本批任务与设计

1. 追踪 pair claim 后 MMC receipt、DMA acknowledged token、transaction 与 quiesced session 的线性所有权。
2. 为 claim 后错误建立不可由普通调用方构造的 archive-ready evidence；只有 cache ownership 恢复成功后才能铸造。
3. recovery report 区分 owner-drain 证据与 completion-claimed 证据，不能复制已经被 DMA completion 消费的 `AcknowledgedIrq`。
4. RecoveryPending 独占 service 校验 slot cause、MMC transaction、DMA transaction 与 failure 后原子归档。
5. 任一 cache/代次/cause 校验失败必须返还原线性状态或证据，slot 保持可重试。

### 已完成

- [x] 新增 `ClaimedReadRecoveryEvidence`，私有保存 transaction、MMC terminal interrupts、DMA transaction、completion evidence 与精确 failure。
- [x] evidence 的普通字段不可由 crate 其他模块直接构造；只暴露只读 getter，错误代次注入仅有显式 `cfg(test)` fixture。
- [x] paired acknowledged/quiesced wrappers 持续携带原 DMA transaction；DMA `AcknowledgedIrq` 仍只被 `complete_irq` 消费一次，没有伪造副本。
- [x] `PairedDmaStatusProgress` 与新 `PairedMmcCompletionProgress` 在两类错误中返回 `ClaimedReadRecovery`，绑定 MMC receipt、DMA transaction 和 recovery session。
- [x] 新增 `ClaimedReadCacheRecovery` typestate；cache finish 失败返还可重试 session，成功才返回 archive-ready evidence。
- [x] `ReadRecoveryReport` 新增可选 claimed evidence；既有 owner-drain report 明确填写 `claimed: None`。
- [x] `ReadRecoveryService::archive_claimed` 在 SERVICING 内校验双 transaction 和 exact CompletionFailure，成功直接发布 RecoveryRecorded。
- [x] claimed report snapshot 将两路 receipt presence 标为 true，同时 `drained` 保持空，准确区分“已消费完成证据”和“从 owner table 排空的原 token”。
- [x] DMA hardware error 与 MMC command timeout 都完成 RecoveryPending→cache finish→archive→take 的完整模型闭环。

### 验证证据

- 2K1000 驱动 host 单测 176 项全部通过；paired error 集成测试覆盖两条 claimed recovery 完整链。
- 注入 payload 第一次 CPU sync 失败，`ClaimedReadCacheRecoveryFailure` 返回原 session；slot 保持 RecoveryPending，第二次 finish 成功后才产生 evidence。
- 故意把 archive evidence 的 DMA generation 从 25 改为 125，返回 WrongTransaction 和原 evidence；修正后同一 evidence 成功归档。
- DMA hardware report 保留 `Hardware(0x8000_0042)`、MMC terminal bits、两侧 transaction 与当时 completion evidence。
- MMC timeout report 保留 `Command(Timeout)`；归档后普通 release 返回 RecoveryMustBeTaken，take 后才能清空 slot。
- owner-drain timeout/fault 的既有 report 测试全部通过，证明新增 claimed 字段没有改变原 receipt 所有权语义。
- production 组件 test/check、RISC-V `make check` 与 LoongArch64 `make kernel-la` 全部通过；完整构建仅有仓库既有 warning。
- 全部 53 项 Python host 测试、topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：真实 DMA acknowledged→descriptor status→cache invalidate 的可见性与屏障顺序仍待 2K1000LA 实测。
- [ ] `UNVERIFIED_ON_HARDWARE`：MMC terminal accumulator 的 error priority、W1C 后 condition clear 以及迟到 IRQ 行为仍待实机验证。
- [ ] claimed evidence 保存 DMA transaction 与 completion typestate 事实，而不是已经被合法消费的 `AcknowledgedIrq`；诊断工具需按 evidence source 区分语义。
- [ ] `ClaimedReadRecovery`、paired tracker 和 DMA session 目前仍是 test-only 外壳；production 尚缺拥有 mappings/channel 的长期 read request 对象。
- [ ] cache finish retry 已覆盖 payload failure；descriptor 在 status inspect 阶段的 CPU sync failure 由既有 inspection retry 测试覆盖，尚未组合进同一个 archive 测试。
- [ ] report archive/take 后 sources 仍 masked；证据完整不代表 controller clear、DMA idle 或允许 rearm。
- [ ] success path 仍缺与 claimed failure 对称的 archive-ready completion proof，slot release 依赖调用顺序。
- [ ] production runtime、publish permit、真实 status decoder、block callback、scheduler wake/deadline 与 rearm 均未接通。
- [ ] 下一批应设计 production-owned `ReadRequest` 状态容器，先消除 paired session 的 `cfg(test)` 边界，并用显式 success-finalized token 收紧 CompletionClaimed release。

### 提交

- 本批计划提交：`[feat] archive LS2K1000 claimed read recovery`

## 2026-08-10：批次 108——收紧 claimed read success finalize

### 本批任务与设计

1. 审计成功 pair 在 MMC/DMA tracker、completion evidence 和 cache finish 之间的所有权边界。
2. 引入私有构造的 `ClaimedReadCompletionEvidence`，绑定两侧 transaction、MMC terminal snapshot 和三项完成事实。
3. 成功 wrapper 必须先完成 quiesced cache finish，才能铸造 completion evidence。
4. coordinator 增加 `CompletionFinalized` 状态；未经 finalized proof 的 `CompletionClaimed` 不得 release。
5. 错误 generation、重复 finalize、invalid evidence 和 release-before-finalize 均保持原状态并可重试。

### 已完成

- [x] paired MMC success 不再直接暴露裸 `ReadCompleted`，改为携带 MMC/DMA transaction 的 `ClaimedReadCompletion`。
- [x] 新增 `ClaimedReadCompletionCache` typestate；cache finish 失败返还 completion session，成功才产生私有 completion evidence。
- [x] 新增 `ReadCoordinatorPhase/State::CompletionFinalized`。
- [x] `ReadClaimedCompletionService::finalize` 校验同代双 transaction 和 `command_response_validated/data_finished/dma_finished` 全部为 true。
- [x] `release` 对 `CompletionClaimed` 返回 `CompletionMustBeFinalized`，对 `CompletionFinalized` 才允许清空 slot。
- [x] 成功集成测试覆盖 status decoder 重试、MMC receipt apply、cache finish、错误 DMA generation finalize、release 拒绝和正确 finalize/release。
- [x] finalized evidence 的 DMA completion 语义来自已消费的线性 `AcknowledgedIrq` typestate，不复制第二份 IRQ token。
- [x] owner-drain recovery、claimed failure archive 和既有 `RecoveryRecorded`/`take_recovery` 逻辑保持兼容。

### 验证证据

- 2K1000 驱动 host 单测 176 项全部通过。
- 成功链明确经过 Terminal→CompletionClaimed→cache finish→CompletionFinalized→release；错误 generation 返回 evidence，slot 仍 CompletionClaimed。
- release-before-finalize 返回 CompletionMustBeFinalized；修正 evidence 后进入 CompletionFinalized，slot 最终 Empty。
- completion evidence 三项布尔事实全部来自 tracker；coordinator 不接受调用方单独传入布尔值。
- RISC-V `make check`、LoongArch64 `make kernel-la` 全部通过。
- Python host 测试 53 项全部通过；topology/畸形 DTB matrix 与 `git diff --check` 通过；dtc warning 来自预期畸形输入。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：真实 DMA cache invalidate、MMC terminal snapshot 可见性、IRQ 迟到和 controller clear/rearm 时序仍待 2K1000LA 实机验证。
- [ ] success cache failure 的 retry typestate 已实现，但本批主要集成测试使用成功模型；需要单独组合 descriptor status success 与 payload CPU sync failure 的完整回归。
- [ ] paired completion/session 与 success wrapper 仍为 `cfg(test)`，production 尚缺拥有 DMA mappings/channel 的长期 `ReadRequest`。
- [ ] `CompletionFinalized` 只证明软件 tracker 与 cache finish 顺序；slot 仍不持有硬件 idle/condition-clear 证明，不能授权 rearm。
- [ ] production runtime、publish permit、真实 status decoder、block callback、scheduler wake/deadline 与动态设备注册尚未接通。
- [ ] 下一批应把 `ClaimedReadCompletion`/recovery 两类 test-only wrapper 合并进 production-owned request 状态机，并开始根文件系统/分区镜像与远程 shell 共有基础设施。

### 提交

- 本批计划提交：`[feat] finalize LS2K1000 claimed read success`

## 2026-08-10：批次 109——建立生产态 ReadRequest 拥有边界

### 本批任务与设计

1. 复核根镜像、MBR 分区解析和远程调试基础设施，确认这些能力已在仓库中，避免重复实现。
2. 为 deferred read 增加不依赖 `cfg(test)` 的请求对象，持有稳定 transaction 与已验证的块请求几何。
3. 让请求 reservation commit 后返回 worker-facing handle，由 handle 驱动现有 coordinator 的 publish、snapshot 和 release 生命周期。
4. 保持硬件执行层未接通；不从请求对象推导 DMA mapping、MMC MMIO 或 rearm 权限。

### 已完成

- [x] 新增生产态 `ReadRequest`，绑定 `ReadTransactionId` 与 `ReadBlockRequest`，可跨 worker 保留并提供只读几何访问。
- [x] 新增 `ReadRequestReservation` 与 `ReadRequestHandle`；commit 后 handle 只能通过绑定 slot 的 transaction 调用状态机。
- [x] handle 已覆盖 reserve→publish→snapshot→release 的最小路径，错代 transaction 仍由 coordinator 统一拒绝。
- [x] 未复制或伪造 MMC/DMA receipt；`ClaimedReadCompletion`、recovery session 和硬件执行器仍按既有边界保留。

### 验证证据

- `read_coordinator::tests` 10 项通过，新增测试验证 request geometry、transaction 和 slot 生命周期绑定。
- `git diff --check` 通过；后续完整组件测试、两架构构建和 Python/topology/DTB 回归在提交前执行。

### 已知限制、未验证与后续测试

- [ ] `UNVERIFIED_ON_HARDWARE`：真实 DMA mapping/channel、MMC command publish、cache/barrier、迟到 IRQ 与 rearm 时序仍待 2K1000LA 实机验证。
- [ ] `ReadRequestHandle` 当前是生命周期拥有边界，不持有实际 DMA buffer；下一批应把 buffer/mapping lease 接入同一对象并继续消除 paired session 的 `cfg(test)` 边界。
- [ ] handle 尚未接 scheduler wake/deadline、block callback、动态设备注册和真实 status decoder；这些仍需生产 runtime 接线。
- [ ] 根镜像/MBR 分区 builder、分区 block-device 拓扑和 remote-debug monitor 已存在；物理 SD/eMMC 启动及未认证远程 shell 仍不应宣称完成。

### 提交

- 本批计划提交：`[feat] add LS2K1000 owned read request lifecycle`

## 2026-08-10：批次 110——为 ReadRequest 接入生产态 DMA buffer lease

### 本批任务与设计

1. 审计 `ReadRequest` 与现有 `DmaMapping`/coherency typestate 的连接点。
2. 新增绑定请求几何的生产态 `ReadRequestBuffer<C>`，拒绝长度或方向不匹配的 mapping。
3. 只允许通过显式 `prepare_for_device`/`complete_from_device` 转移 CPU/device 所有权；同步失败必须保留 buffer 以便重试。
4. 用 host mock 验证线性所有权与错配路径，不宣称真实 cache/barrier 已验证。

### 已完成

- [x] `ReadRequest::bind_dma_buffer` 校验 DMA mapping 长度等于请求 byte length，且方向必须为 `FromDevice`。
- [x] `ReadRequestBuffer` 持有 mapping 与 request identity，公开 CPU region、prepare、complete 和 ownership 查询。
- [x] DMA mapping 的现有 typestate 没有被复制；所有 sync 仍由调用方提供的 `DmaCoherency` backend 执行。
- [x] 新增 host 测试覆盖成功 prepare/complete、CPU region 拒绝 device-owned mapping 和错误长度拒绝。

### 验证证据

- 本批 focused coordinator tests 11 项通过；新增 buffer 测试验证 1024-byte 双块请求与 mapping identity 一致。
- `DmaDirection::FromDevice`、长度校验和 ownership transition 均由 API 层实际返回值断言。
- `UNVERIFIED_ON_HARDWARE`：2K1000LA 的实际 cache flush/invalidate、DMA 地址可达性和屏障时序仍没有物理机证据。

### 已知限制、未验证与后续测试

- [ ] 当前 lease 尚未由真实 APBDMA executor 持有 descriptor/channel；下一步需要把 descriptor lease 与 payload lease 合并到同一 production request。
- [ ] 真实硬件上发生 partial start、stop timeout 或迟到 IRQ 时的 buffer 回收仍待 QEMU 模型扩展和实机验证。
- [ ] rootfs 镜像、分区 block device、动态 `/dev` 与 remote-debug monitor 仍按此前批次状态维护，不能将 QEMU/host 结果当作物理板验证。

### 提交

- 本批计划提交：`[feat] bind LS2K1000 read buffer lease`

## 2026-08-10：批次 111——绑定 ReadRequest 的 descriptor/payload DMA lease

### 本批任务与设计

1. 复核既有 `OwnedTransferResources` 与 APBDMA typestate，确认 descriptor 与 payload 仍缺少统一请求代次校验。
2. 新增生产态 `ReadRequestDmaLease<D, P>`，同时持有两路 `DmaMapping` 和不可变 `TransferPlan`。
3. 绑定前校验请求 byte length、descriptor/payload 物理地址、方向和 cache 可见性计划；绑定后只调用已有 `prepare_session`。
4. 用纯 host fixture 验证配对和 cancel，不把 prepare 当成硬件 start 证据。

### 已完成

- [x] `ReadRequestDmaLease::bind` 将 descriptor/payload mapping 与同一个 `ReadRequest`/`TransferPlan` 绑定。
- [x] 拒绝非 DeviceToMemory、错误 descriptor 地址、错误 payload 地址/长度、读请求几何不一致或缺少 invalidate/clean 计划。
- [x] `prepare_session` 复用 APBDMA 现有线性 typestate；prepared session 仍必须显式 cancel/start，lease 不复制 ownership token。
- [x] 新增 host 测试覆盖合法配对、prepare、cancel 和请求 identity 保留。

### 验证证据

- 驱动完整 host 测试 179 项通过；新增配对 lease 测试实际走过 `prepare_transfer` 的 descriptor/status/方向校验。
- 驱动 `cargo check`、RISC-V `make check`、LoongArch64 `make kernel-la` 与 Python host tests 均通过。
- `UNVERIFIED_ON_HARDWARE`：真实 APBDMA channel、descriptor 写入可见性、cache/barrier、stop/rearm 和迟到 IRQ 仍待物理板。

### 已知限制、未验证与后续测试

- [ ] 当前 lease 尚未直接接入 block callback、MMC command publish 和 scheduler worker；下一步应把 request handle、lease 与 coordinator phase 组合成单一 production executor facade。
- [ ] LoongArch `OwnedTransferResources` 的真实 frame allocator 路径仍只在目标架构编译，暂无物理机内存压力/回收证据。
- [ ] QEMU/host 测试没有模拟 LS2K1000 专用 MMC APB routing 的真实寄存器副作用，相关行为必须保持 `UNVERIFIED_ON_HARDWARE`。

### 提交

- 本批计划提交：`[feat] bind LS2K1000 read dma lease`

## 2026-08-10：批次 112——组合 ReadRequest production executor facade

### 本批任务与设计

1. 审计调用方仍需手工维护的 request handle、coordinator phase 和 DMA lease。
2. 新增 `ReadRequestExecutor`，绑定同一请求 identity 的 coordinator handle 与 descriptor/payload lease。
3. facade 仅编排已有状态机：publish、snapshot、prepare/cancel 和 release；不新增任何硬件写入旁路。
4. generation 错配和 release 时的 device-owned mapping 必须保留可恢复对象，避免静默丢失资源。

### 已完成

- [x] `ReadRequestExecutor::bind` 校验 handle 与 lease 的完整 request identity，错代返回两者原对象。
- [x] facade 提供统一 transaction/request/snapshot、publish、prepare DMA session 和 release 入口。
- [x] release 前检查 descriptor/payload 是否都已恢复 CPU ownership；失败返回可恢复 executor。
- [x] 新增 host 测试覆盖组合生命周期与错代返还，现有 APBDMA/MMC typestate 未被绕过。

### 验证证据

- focused coordinator 测试 14 项通过，覆盖 facade reserve→publish→prepare→cancel→release。
- facade 的合法路径实际调用 APBDMA `prepare_transfer`，错代路径验证 handle/lease 均可取回。
- `UNVERIFIED_ON_HARDWARE`：facade 仍未执行真实 MMC command publish、DMA order MMIO、cache/barrier、IRQ/rearm。

### 已知限制、未验证与后续测试

- [ ] 下一步需把 facade 接到真实 worker/scheduler 与 block callback，并在 coordinator 中表达 MMC publish permit。
- [ ] facade 尚未提供完成/恢复 evidence 的统一方法；当前必须继续使用既有低层 completion/recovery services。
- [ ] 物理 2K1000LA 的 cache、APB route、DMA stop 和 SD/eMMC 电气行为仍待实机验证。

### 提交

- 本批计划提交：`[feat] add LS2K1000 read executor facade`

## 2026-08-10：批次 113——为 ReadRequest 增加 MMC publish permit

### 本批任务与设计

1. 审计现有 `DeferredReadPlan`、`ReadDataPublishPermit` 和 coordinator 的 publish 边界。
2. 在 production executor 上增加一次性 `ReadRequestPublishPermit`，校验 Reserved phase、请求几何和 DMA transfer plan。
3. permit commit 只推进 coordinator 到 Published，不直接写 MMC 寄存器；真实 `ReadDataCommandPublisher` 继续由经过硬件验证的独立 capability 控制。
4. 所有失败路径返回可恢复的 executor/permit，避免消费 request 或 DMA lease。

### 已完成

- [x] `issue_publish_permit` 校验 coordinator 当前为 Reserved、MMC request geometry 与 transfer byte length 一致。
- [x] 校验 APBDMA data register、invalidate policy 与请求 plan 匹配。
- [x] `ReadRequestPublishPermit::commit` 一次性推进 Published；coordinator 失败时返回原 permit。
- [x] 新增 host 测试覆盖合法 permit commit，并保持现有 executor/lease ownership。

### 验证证据

- focused coordinator 测试 15 项通过；新增测试实际验证 Reserved→Published 迁移与 plan identity。
- 驱动完整测试、组件 check、RISC-V `make check`、LoongArch64 `make kernel-la` 与 Python host tests 均通过。
- `UNVERIFIED_ON_HARDWARE`：本 permit 不代表 MMC command 已写入；真实寄存器顺序、W1C、DMA start、cache/barrier 与 IRQ 时序仍待实机。

### 已知限制、未验证与后续测试

- [ ] 下一步需让真实 worker 在 permit commit 后调用 MMC publisher，并把 publisher receipt 与 APBDMA running session 绑定。
- [ ] 当前 permit 的 transfer identity 是软件字段校验，尚未通过物理 APB route/SD card loopback 证实。
- [ ] command publish 失败后的“可能已写入”恢复仍由现有 publisher 类型处理，facade 尚未统一收纳该 failure token。

### 提交

- 本批计划提交：`[feat] add LS2K1000 mmc publish permit`

## 2026-08-10：批次 114——绑定 MMC receipt 与 running DMA session

### 本批任务与设计

1. 审计 `RunningReadDmaSession` 及 IRQ-owned wrapper 中仍为 `cfg(test)` 的 publish 边界。
2. 增加生产态 `ReadDataPublisher` trait 和 `PublishedReadDmaReceiptSession`，保存同一 deferred read 的 publisher receipt 与 APBDMA running session。
3. 在 `IrqArmedReadDmaSession` 上提供 receipt publish/stop wrapper，错误时返还原 IRQ generation 与 running session。
4. 用现有 host MMIO model 验证成功写序列和 receipt 字段；不把 model 写成功视为物理硬件证据。

### 已完成

- [x] `ReadDataPublisher` 返回完整 `ReadDataPublishReceipt`，`ReadDataCommandPublisher` 已实现该生产 trait。
- [x] `RunningReadDmaSession::publish_with_receipt` 将 publisher receipt 与 running DMA session 绑定；失败保留原 session。
- [x] 新增 `PublishedReadDmaReceiptSession` 的 receipt 查询、into_parts 和 stop→quiesced 路径。
- [x] IRQ-owned wrapper 同步暴露 publish_with_receipt、receipt、plan 和 stop，保留 generation token。
- [x] 既有 APBDMA/MMC 顺序测试改走新 receipt 路径，确认六次写入顺序与 command index。

### 验证证据

- 驱动完整 host 测试 182 项通过；`mmc_read_start_typestate_orders_dma_before_command_publish` 实际使用新 receipt wrapper。
- 驱动 check、RISC-V `make check`、LoongArch64 `make kernel-la`、Python 53 项测试均通过。
- `UNVERIFIED_ON_HARDWARE`：volatile write 成功、APB route、DMA channel start、cache/barrier、IRQ 迟到与 SD/eMMC 电气行为仍无物理证据。

### 已知限制、未验证与后续测试

- [ ] 下一步要把该 production published token 接入 coordinator completion/recovery，替换目前 success/failure tracker 的 `cfg(test)` 外壳。
- [ ] publisher 失败后的 receipt 不产生，但可能已写入的 MMIO 状态仍需真实 controller readback/clear 证据；当前只保留 session。
- [ ] 目标架构真实 allocator、DMA mapping 和 cache backend 尚未在板上运行。

### 提交

- 本批计划提交：`[feat] bind LS2K1000 mmc receipt to dma session`

## 2026-08-10：批次 115——建立 production published completion tracker

### 本批任务与设计

1. 审计 published receipt token 与旧 `ReadCompletionTracker` 的 `cfg(test)` 分界。
2. 新增 production `PublishedReadCompletionTracker`，持有 published MMC receipt + running DMA session。
3. 以 command/data/DMA 三项事实累加器替代裸布尔调用；重复事实和 data/error interrupt 必须返回原 tracker。
4. 三项事实齐全后铸造 `PublishedReadCompletion`；该 proof 仍不代表 cache CPU ownership、硬件 idle 或可 rearm。

### 已完成

- [x] 新增 production tracker/progress/completion/failure 类型。
- [x] 实现 command response、controller interrupt 和 DMA completion 的多顺序推进及 duplicate/error 拒绝。
- [x] `PublishedReadCompletion::into_session` 保留原 receipt/running session，之后仍须 stop/quiesce。
- [x] IRQ-owned published wrapper 可拆出 generation 与 production completion tracker，避免复制 IRQ token。
- [x] APBDMA/MMC 顺序测试已覆盖新 tracker 的三事实成功路径和最终 quiesce。

### 验证证据

- 驱动完整 host 测试 182 项通过；成功测试断言三项 evidence 全为 true，并在 completion 后显式 stop/finish。
- 驱动 check、RISC-V `make check`、LoongArch64 `make kernel-la`、Python 53 项测试均通过。
- `UNVERIFIED_ON_HARDWARE`：真实 IRQ receipt、DMA descriptor status、cache invalidate、MMC error priority 和 controller clear 仍待实机。

### 已知限制、未验证与后续测试

- [ ] production tracker 当前只管理 running session；需要下一步接入 IRQ acknowledgement/status decoder，才能构造真实 completion/recovery evidence。
- [ ] failure token 尚未统一进入 coordinator RecoveryPending；发生 command/data/DMA failure 后仍需调用既有低层 stop/recovery 服务。
- [ ] completion proof 不授权 buffer CPU 访问；必须继续经过 quiesce 与 cache sync typestate。

### 提交

- 本批计划提交：`[feat] add LS2K1000 published completion tracker`

## 2026-08-10：批次 116——将 completion failure 转入 coordinator recovery

### 本批任务与设计

1. 审计 completion failure 发生于 Published/Rechecking/Terminal 时的 coordinator 状态转移。
2. 新增 `ReadCoordinatorSlot::record_completion_failure`，只接受代次匹配且尚未进入终态 recovery 的请求。
3. 为 production published tracker 增加 stop/quiesce recovery token；失败时保留 tracker，成功时只交出 quiesced DMA session。
4. 继续要求后续 IRQ retire、MMC clear 和 cache sync evidence，不能以 failure 分类代替硬件回收。

### 已完成

- [x] completion failure 可原子进入 `RecoveryPending`，slot 不被清空，普通 release 仍返回 `RecoveryMustBeRecorded`。
- [x] 支持 Published、Rechecking、Terminal phase 的 failure 入口；错代和非法 phase 保持原状态。
- [x] `PublishedReadCompletionFailure::stop` 先执行 DMA stop，生成带 failure/receipt/quiesced session 的 recovery token；stop 失败返回原 tracker。
- [x] 新增 coordinator host 测试覆盖空 slot、成功转移、cause 和 release gate。

### 验证证据

- 驱动完整 host 测试 183 项通过；新增 failure 测试确认 RecoveryPending 保留请求资源。
- 驱动 check、RISC-V `make check`、LoongArch64 `make kernel-la`、Python 53 项测试全部通过。
- `UNVERIFIED_ON_HARDWARE`：真实 stop confirmation、IRQ acknowledgement/clear、MMC controller readback、cache sync 和 SD/eMMC 行为仍待实机。

### 已知限制、未验证与后续测试

- [ ] recovery token 尚未自动构造 `ReadRecoveryReport`，调用方仍需提供 IRQ retire/partial snapshot 后进入 RecoveryRecorded。
- [ ] coordinator failure 入口不拥有底层 DMA session；资源所有权必须由 published tracker 的 stop token 保持，禁止直接丢弃。
- [ ] 下一步应将 quiesced recovery token 与 `ReadRecoveryService::retire_and_record`/`archive_claimed` 统一，形成完整 failure archive 链。

### 提交

- 本批计划提交：`[feat] route LS2K1000 read failures to recovery`

## 2026-08-10：批次 117——归档 published recovery summary

### 本批任务与设计

1. 审计 quiesced published recovery token 的最后一步，确保 DMA 停止后才恢复 CPU ownership。
2. 新增不可变 `PublishedReadCompletionRecovered` 摘要，仅保留 completion failure 与 MMC publish receipt。
3. 新增 `ReadRecoveryService::archive_published`，严格校验 `RecoveryPending` cause 后写入 `RecoveryRecorded`。
4. 通过 host fixture 覆盖 failure → stop/quiesce → finish → archive → take 链路；不伪造 IRQ owner 或硬件 idle 证据。

### 已完成

- [x] `PublishedReadCompletionRecovery::finish` 调用 APBDMA quiesced session 的 cache/ownership finish；失败保留可重试 session。
- [x] 新增 published recovery summary 访问器，归档失败时返回 summary，避免丢失恢复事实。
- [x] `archive_published` 仅接受与 coordinator cause 一致的 completion failure，并记录调用方提供的 partial MMC interrupt snapshot。
- [x] 新增 coordinator host 测试，确认归档后可线性 `take_recovery`，且不产生虚假的 claimed IRQ evidence。

### 验证证据

- 驱动完整 host 测试 184 项通过；published recovery summary archive 测试通过。
- `UNVERIFIED_ON_HARDWARE`：真实 DMA stop/idle、descriptor 与 payload cache backend、MMC interrupt clear/readback、IRQ retire 及 SD/eMMC 电气行为仍待实机验证。

### 已知限制、未验证与后续测试

- [ ] 当前 summary archive 不携带 IRQ owner receipt；真实板上仍需先完成 mask/ack/retire，再把观测到的 partial snapshot 传入归档。
- [ ] `ReadRecoveryReport` 保留 `claimed: None` 是有意的，不能把软件 quiesce 误报成硬件 interrupt 已清除。
- [ ] 目标架构 allocator、DMA mapping/cache 实现及控制器 readback 仍需板上 bring-up 后闭环。

### 提交

- 本批计划提交：`[feat] archive LS2K1000 published recovery`

## 2026-08-10：批次 118——绑定 ReadRequest 与 deferred DMA typestate

### 本批任务与设计

1. 复核公共任务中 rootfs、MBR 分区子设备、动态 `/dev` 和小镜像 builder 的现状；这些能力已由前序公共批次完成，本批不重复实现。
2. 找出当前 2K1000LA production read path 的边界：`ReadRequestExecutor` 原先只能借出裸 `PreparedSession`，会丢失 deferred MMC read 的几何身份。
3. 让 `ReadRequestDmaLease` 通过 `ReadDmaBinding` 校验 request、descriptor、payload、DATA 地址和 cache policy 后生成 `PreparedReadDmaSession`。
4. 保留旧 `prepare_dma_session` 兼容入口，同时新增显式绑定入口；错误返回精确的 `ReadDmaIdentityError`，不转移 DMA ownership。

### 已完成

- [x] 新增 `ReadRequestDmaLease::prepare_read_session`，复用 production `ReadDmaBinding::bind` 与 APBDMA prepare typestate。
- [x] 新增 `ReadRequestExecutor::prepare_bound_dma_session`，为后续 start → publish receipt → completion/recovery 链保留完整 `DeferredReadPlan`。
- [x] 增加 host 测试，验证合法 request 可生成绑定的 prepared session 并 cancel 归还 CPU ownership。
- [x] 维持旧裸 prepared API，避免现有调用方在真实 hardware activation 尚未验证前被强制迁移。

### 验证证据

- 驱动定向测试 `production_request_executor_binds_deferred_read_to_prepared_dma` 通过。
- `UNVERIFIED_ON_HARDWARE`：真实 APB DATA 地址、descriptor fetch、cache clean/invalidate、DMA start/stop 和 MMC command ordering 仍未由物理板确认；本批只证明软件身份校验与 host model 生命周期。

### 已知限制、未验证与后续测试

- [ ] prepared session 尚未从该 executor 直接连到真实 APBDMA executor、MMC publisher 和 IRQ owner；仍需下一批补齐 carrying session 的 start/publish facade。
- [ ] `ReadRequestDmaLeaseError::DmaIdentity` 目前是软件错误分类，不代表 controller 或 DMA 硬件已接受该 plan。
- [ ] 公共分区镜像能力尚未在本 LA 分支接入该 production read request，目标板 SD/eMMC block registration 仍待平台驱动完成。

### 提交

- 本批计划提交：`[feat] bind read request to dma typestate`

## 2026-08-10：批次 119——封装 prepared 到 publish receipt 的 carrying transition

### 本批任务与设计

1. 审计上一批生成的 `PreparedReadDmaSession` 与现有 APBDMA `start`、MMC `publish_with_receipt` 两段接口。
2. 增加单一 `start_and_publish` transition，顺序固定为先启动 DMA、再写 MMC command；不复制 descriptor/payload mapping。
3. 启动失败返回 `ReadDmaStartFailure`，publisher 失败返回仍持有 running session 的 `ReadDataPublishReceiptFailure`，统一由 carrying enum 传回。
4. 将既有 host model 顺序测试迁移到该 facade；不把 model 写入或 stop confirmation 当作真机证据。

### 已完成

- [x] 新增 `ReadDmaStartPublishFailure`，线性区分 DMA start 与 MMC publish 两阶段失败。
- [x] 新增 `PreparedReadDmaSession::start_and_publish`，成功返回带 receipt 的 running session。
- [x] 既有 MMC/DMA 顺序测试改用新 facade，继续覆盖 command/data/DMA completion、stop 和 cache ownership 回收。

### 验证证据

- 定向测试 `mmc_read_start_typestate_orders_dma_before_command_publish` 通过。
- `UNVERIFIED_ON_HARDWARE`：DMA descriptor fetch、APB route、MMC volatile writes、IRQ completion、cache/barrier 和 stop idle confirmation 仍只经过 host model/文档约束。

### 已知限制、未验证与后续测试

- [ ] facade 尚未接入真实 IRQ owner/coordinator slot；下一步需让 carrying published session 与 request transaction/generation 同时存活。
- [ ] 当前 publisher 仍需 caller 提供 capability permit，默认生产路径保持关闭。
- [ ] 目标板 MMC/SD 电气、时钟、DMA channel 和中断行为仍待实机 bring-up。

### 提交

- 本批计划提交：`[feat] carry dma start publish failures`

## 2026-08-10：批次 120——为 published DMA ownership 绑定 request generation

### 本批任务与设计

1. 审计 production `PublishedReadDmaReceiptSession`：它保留 MMC receipt 和 DMA mapping，但此前没有 request transaction/generation。
2. 增加 `ReadRequestPublishedDmaSession` wrapper，由 caller 显式提供已验证的 `ReadTransactionId` 后绑定底层 session。
3. wrapper 只复制 generation 标量，不复制或拆分 receipt/mapping；`into_session` 仍保持底层线性 ownership。
4. 在现有 start→publish→completion→stop host model 测试中加入 generation 和 receipt 断言。

### 已完成

- [x] 新增 generation-bound published session wrapper、`transaction`/`receipt`/`into_session` API。
- [x] 既有 APBDMA/MMC 顺序测试验证 generation 绑定后 receipt 不变，最终仍能 stop/finish 并恢复 CPU ownership。
- [x] 保持底层 MMC session API 向后兼容，避免在硬件 activation 未验证前扩大默认路径。

### 验证证据

- 定向测试 `mmc_read_start_typestate_orders_dma_before_command_publish` 通过。
- `UNVERIFIED_ON_HARDWARE`：generation 与软件 request 关联尚未由真实 IRQ owner/调度 worker 证明；DMA、MMC、cache、IRQ 和 SD/eMMC 电气行为仍待实机。

### 已知限制、未验证与后续测试

- [ ] wrapper 的 `bind` 依赖 caller 先完成 request/session 归属校验；下一步应将它直接接到 `ReadRequestExecutor` 的 worker transition，减少手工绑定面。
- [ ] coordinator 尚未消费该 wrapper 的 generation 来驱动 `RecoveryPending`/`RecoveryRecorded`，目前仍需显式 service 调用。
- [ ] 真实板上仍需验证 transaction cookie 与迟到 IRQ、DMA status 的代次过滤。

### 提交

- 本批计划提交：`[feat] bind published dma generation`

## 2026-08-10：批次 121——接入可复用 NS16550 的 2K1000LA UART 注册路径

### 本批任务与设计

1. 审计 2K1000LA DTB：UART 节点已经被解析，但没有进入 character registry，无法形成 `/dev` 字符设备。
2. 复用仓库已有 `impl-uart-16550`（Byte16550/DW-APB32），不重新引入第三方驱动，避免许可证和实现分叉。
3. 增加 opt-in feature `uart-16550` 及 aggregate forwarding feature `impl-loongson2k1000la-uart`；默认关闭。
4. 根据 DTB `reg-shift` 选择布局，未知值 fail-closed；注册函数要求调用者保证 MMIO device mapping 与独占所有权。

### 已完成

- [x] 新增 LA `uart` 模块，将拓扑 UART 转换为共享 NS16550 character device。
- [x] `init_after_boot` 在显式启用 feature 时注册 DTB UART，后续可由 devfs 枚举字符设备。
- [x] 支持 `reg-shift=0`（Byte16550）和 `reg-shift=2`（DW-APB32），其他布局返回 `InvalidDtb`。
- [x] 复用现有实现，未新增外部驱动代码或许可证负担。

### 验证证据

- LA driver crate 启用 `uart-16550` 的布局单测通过。
- `UNVERIFIED_ON_HARDWARE`：注册函数实际写 IER、UART MMIO 映射、时钟/电气线参数、IRQ 输入和真实 `/dev` 节点仍待目标板验证；host 测试不执行 volatile MMIO。
- aggregate host `cargo check --features impl-loongson2k1000la-uart` 受当前未选择 LoongArch platform-arch backend 的 host 配置限制失败；目标架构 `make check` 仍作为正式编译门禁。

### 已知限制、未验证与后续测试

- [ ] 默认 feature 仍关闭，需在目标板 bring-up 配置中显式启用并确认 MMIO 映射。
- [ ] LA init 尚未完成 UART IRQ owner、接收中断和 console/stdin 路由；当前只注册轮询字符端口。
- [ ] devfs 热插拔/注销尚未覆盖 UART；后续需要与动态 character registry 生命周期对齐。

### 提交

- 本批计划提交：`[feat] register LS2K1000 UART character devices`

## 2026-08-10：批次 122——同步 2K1000LA UART 到 devfs

### 本批任务与设计

1. 审计 UART 注册后的设备可见性：character registry 增加设备不会自动刷新 active devfs 节点表。
2. 复用现有 `fs::devfs::active_impl::refresh`，增加 2K1000LA 启动期同步入口。
3. 同步只重建软件视图，不探测/写硬件；保持幂等，默认仅在 `uart-16550` opt-in feature 下启用。
4. 继续不实现热插拔；设备注销、打开 fd 引用和物理 UART 拔插留到后续生命周期批次。

### 已完成

- [x] 新增 LA `devfs::sync`，在 DTB UART 注册完成后刷新 active devfs。
- [x] `uart-16550` feature 现在同时启用 character driver 与 fs devfs 依赖。
- [x] 同步路径只刷新节点快照，避免重复注册或硬件副作用。

### 验证证据

- UART feature 驱动全量 host 测试 186 项通过。
- `make check`、`make kernel-la` 和 Python 测试通过。
- `UNVERIFIED_ON_HARDWARE`：真实 UART MMIO、IRQ、时钟、电气行为和目标板 `/dev` 挂载路径仍未验证；host 只验证软件构建与既有 devfs 实现。

### 已知限制、未验证与后续测试

- [ ] 当前同步只在启动期追加设备后执行，尚未支持 UART 热移除或动态注销。
- [ ] devfs 刷新不会自动创建用户权限/别名策略；需要后续与 `/dev/console`、PTY 和远程 shell 设计对齐。
- [ ] 2K1000LA 尚未接收 UART IRQ，字符设备仍以轮询接口为主。

### 提交

- 本批计划提交：`[feat] refresh LS2K1000 devfs after UART registration`

## 2026-08-10：批次 123——增加 2K1000LA 设备能力状态快照

### 本批任务与设计

1. 审计现有 LA 驱动目录：仓库已有 QEMU virtio input/network 实现，但没有可直接套用的 2K1000LA GMAC/USB-HID 绑定；不能把 QEMU 驱动状态冒充目标板能力。
2. 在 DTB topology 上增加只读 `BoardCapabilitySnapshot`，分别记录 UART、IRQ、MMC、DMA 的发现数量与 activation 状态。
3. 明确 network/input 为 `Unsupported`，MMC/DMA/IRQ/UART 为 `DeferredActivation`，避免“DTB 发现 = 驱动可用”的误判。
4. 对外提供 `capability_snapshot()`，供远程诊断和后续设备注册流程复用。

### 已完成

- [x] 新增 `CapabilityState` 与 `BoardCapabilitySnapshot`。
- [x] 新增 `BoardTopology::capability_snapshot`，并提供 LA crate 只读查询入口。
- [x] 增加 host 单测，验证发现数量、延迟激活和未支持接口状态彼此区分。
- [x] 记录 QEMU virtio input/network 只能复用在 QEMU profile，目标板 GMAC/USB-HID 仍需独立驱动/许可证审计。

### 验证证据

- `capability_snapshot_separates_discovery_from_activation` 通过。
- `UNVERIFIED_ON_HARDWARE`：快照是软件/DTB 观察，不证明时钟、IRQ、MMIO、DMA、网络 PHY 或 USB 电气状态。

### 已知限制、未验证与后续测试

- [ ] network/input 尚无 2K1000LA 设备解析和硬件实现；下一步应先研究上游 GMAC/USB 控制器驱动及许可证。
- [ ] 快照尚未接入 TCP debug monitor 的 `status` 输出。
- [ ] capability 状态没有热插拔代次；后续动态 `/dev` 需要增加 generation 与注销语义。

### 提交

- 本批计划提交：`[feat] add LS2K1000 capability snapshot`

## 2026-08-10：批次 124——将 2K1000LA 能力快照接入远程诊断

### 本批任务与设计

1. 审计 TCP debug monitor 的命令解析与现有 `ls2k-irq`/`ls2k-mmc` 只读诊断。
2. 增加独立 `capabilities`（别名 `caps`）命令，不修改已有 `status` 协议。
3. 通过 driver aggregate 转发 LA `BoardCapabilitySnapshot`；未初始化返回 `unavailable`，非 LA profile 返回明确 `unsupported`。
4. 命令只读 topology 软件快照，不执行 MMIO、IRQ、网络或设备注册操作；继续保持 monitor 无认证/无加密的开发用途限制。

### 已完成

- [x] remote-debug parser/help/response 增加 `capabilities` 命令。
- [x] driver aggregate 增加 `loongson2k1000_capability_snapshot()` 转发入口。
- [x] 增加 profile-gated host parser/response 测试。

### 验证证据

- 2K1000LA driver capability snapshot 测试及全量 186 项测试通过。
- `UNVERIFIED_ON_HARDWARE`：远程命令只验证软件路径；真实网卡、TCP 接收、UART、IRQ 和 capability topology 仍待目标板。
- host 直接构建 OS monitor 受仓库现有 RISC-V SBI inline-asm 约束影响；应以对应 target 的 `make check`/`make kernel-la` 作为编译门禁。

### 已知限制、未验证与后续测试

- [ ] capabilities 尚未在 QEMU LA/目标板实测交互，只补充了协议单测和 target 编译路径。
- [ ] monitor 仍无认证、加密、PTY 或用户态 shell，不得作为生产远程登录服务。
- [ ] 下一步应将 capability snapshot 与动态设备 generation/devfs 节点列表关联。

### 提交

- 本批计划提交：`[feat] expose LS2K1000 capabilities in debug monitor`

## 2026-08-10：批次 125——为 devfs 软件视图增加刷新代际

### 本批任务与设计

1. 审计 kernel devfs 与简化 devfs 的刷新、注册、注销路径，确认节点快照和设备 topology generation 的边界。
2. 增加独立的软件视图 generation：每次重建节点表或直接登记新路径后递增；不把它解释为硬件热插拔证明。
3. 将 2K1000LA 的 devfs generation 接入 capability 远程诊断；UART/devfs 未启用时明确返回 `None`。
4. 增加重复刷新、设备变化和未初始化路径的 host 编译/单测证据，完成 target 构建与清理。

### 已完成

- [x] kernel devfs、简化 devfs、dummy devfs 统一提供 `generation()` 查询入口。
- [x] 节点重建、块/字符直接注册都会推进软件视图代际；旧句柄不会因代际变化继续被假定有效。
- [x] LA driver aggregate 与 `capabilities` 诊断输出 `devfs_generation`，并保留 UART feature gate。
- [x] 单测覆盖 input 注册/注销和 block 注册/注销后的代际变化。

### 验证证据

- `cargo test`：kernel devfs 1 项通过；简化 devfs 1 项通过。
- `make check EXTRA_FEATURES=remote-debug-monitor` 通过。
- `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 通过。
- `UNVERIFIED_ON_HARDWARE`：generation 只描述内核软件节点快照；真实板卡热插拔、UART IRQ、MMIO 和设备断电/重连行为仍未验证。

### 已知限制、未验证与后续测试

- [ ] 当前 generation 是进程/内核内软件视图代际，不提供跨重启持久性，也不替代设备注销 API。
- [ ] LA 的 generation 只有启用 UART/devfs 集成时才接入；后续应让所有真实设备注册路径共享同一视图代际。
- [ ] 尚未在目标板执行动态注册/注销和远程 `capabilities` 交互测试。

### 提交

- 本批计划提交：`[feat] track devfs software-view generations`

## 2026-08-10：批次 126——将 devfs 节点快照接入远程诊断

### 本批任务与设计

1. 盘点已有根盘镜像、MBR 分区 block device、动态 devfs 和 TCP monitor，避免重复实现已有能力。
2. 增加独立只读 `devfs` 命令，输出软件视图 generation、节点总数、截断标志和最多 32 条路径。
3. 更新 Python monitor smoke client，验证新协议字段；继续拒绝把 monitor 扩展为 SSH 或未授权通用 shell。
4. 通过 host 单测/target 构建验证协议路径，并标注真实网卡、UART 和物理 `/dev` 行为的未验证边界。

### 已完成

- [x] remote-debug parser/help/response 增加 `devfs` 命令。
- [x] monitor 读取 active devfs 的 generation 与节点快照，限制输出规模，避免诊断连接被异常节点表撑爆。
- [x] Python smoke client 增加 devfs 响应检查。

### 验证证据

- remote-debug 解析单测覆盖 `devfs` 命令别名路径。
- `python3 -m unittest discover -s os/scripts/tests -p 'test_remote_debug_client.py'` 通过（协议 mock 路径）。
- `make check EXTRA_FEATURES=remote-debug-monitor` 与 `make kernel-la EXTRA_FEATURES=remote-debug-monitor` 通过。
- `UNVERIFIED_ON_HARDWARE`：真实网卡 TCP 收发、目标板 UART/SD/eMMC 设备节点、动态热插拔和节点权限仍待实机。

### 已知限制、未验证与后续测试

- [ ] monitor 仍无认证、加密、PTY 或用户态 shell，只能作为 loopback/受控 bring-up 诊断通道。
- [ ] devfs 响应只列路径和代际，不提供逐节点权限、major/minor 或事件流；后续应与真实字符/输入设备生命周期结合。
- [ ] 尚未在物理板或 QEMU 真实 TCP 会话中抓取 `devfs` 输出；当前验证依赖编译门禁与协议单测。

### 提交

- 本批计划提交：`[feat] expose devfs snapshot in debug monitor`

## 2026-08-10：批次 127——暴露架构根盘 manifest 与小镜像参数

### 本批任务与设计

1. 审计既有 `root_image.py`、MBR 分区解析和 Make 入口，确认脚本已经支持 manifest/容量，但 Make target 未转发这些参数。
2. 增加 `ROOT_IMAGE_MANIFEST` 与 `ROOT_IMAGE_SIZE_MIB`，让 RISC-V/LoongArch 可以使用各自的用户态文件清单，同时保留默认 32 MiB 行为。
3. 保持镜像生成、ext4 校验、路径逃逸检查和原子替换集中在 Python 工具；Make 只负责参数编排。
4. 使用临时 16 MiB 镜像完成实际 build/verify，避免在仓库或磁盘留下大镜像。

### 已完成

- [x] Makefile 的 build/verify target 均转发自定义 manifest。
- [x] Makefile 支持 `ROOT_IMAGE_SIZE_MIB`，默认值仍为 32 MiB。
- [x] README 补充架构清单和 16 MiB 测试用法。
- [x] 增加 Make 参数静态回归测试。

### 验证证据

- 自定义 manifest + 16 MiB 稀疏镜像实际构建成功：MBR start=2048、30,720 sectors。
- 同一 manifest 的独立 verify 成功，临时镜像已清理。
- root-image Python 单测 5 项通过，`py_compile` 与 `git diff --check` 通过。
- `make check EXTRA_FEATURES=remote-debug-monitor`、`make kernel-la EXTRA_FEATURES=remote-debug-monitor` 使用同一工作树构建门禁均通过。
- `UNVERIFIED_ON_HARDWARE`：架构相关 ELF/动态链接器闭包、真实 SD/eMMC 写入、启动固件和掉电一致性仍待目标板。

### 已知限制、未验证与后续测试

- [ ] 仓库只提供通用最小清单；包含 BusyBox、动态链接器和应用的 rv64/la64 发布清单仍需按工具链产物生成。
- [ ] Make 参数只改变镜像构建输入，不会自动把新镜像注入目标板烧录或 QEMU 启动流程；仍需显式设置 `ROOT_IMAGE`/`WOS_SDCARD`。
- [ ] 当前镜像仍为单 MBR 主分区；GPT、扩展分区和多数据分区尚未实现。

### 提交

- 本批计划提交：`[feat] parameterize physical root image manifests`
