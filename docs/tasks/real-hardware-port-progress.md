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
