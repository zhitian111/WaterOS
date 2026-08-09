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
- [ ] 新增 VisionFive 2 与 2K1000LA 编译型 platform/driver profile。
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
