qemu-system-riscv64 -machine virt -nographic -kernel ./kernel-rv -serial mon:stdio
# qemu-system-riscv64 -machine virt -kernel {os_file} -m {mem} -nographic -smp {smp} -bios default -drive file={fs},if=none,format=raw,id=x0 \
#                     -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 -no-reboot -device virtio-net-device,netdev=net -netdev user,id=net \
#                     -rtc base=utc \
#                     -drive file=disk.img,if=none,format=raw,id=x1 -device virtio-blk-device,drive=x1,bus=virtio-mmio-bus.1
# 以下是该 QEMU 命令参数的详细解释：
#
# ---
#
# ### **1. 基础配置**
# - **`-machine virt`**  
#   指定模拟的机器类型为 QEMU 的 **RISC-V 虚拟平台**（`virt`），这是一个通用的 RISC-V 虚拟机，支持标准外设（如 UART、VirtIO 设备等）。
#
# - **`-kernel {os_file}`**  
#   加载操作系统内核文件（如 `vmlinux` 或 `Image`），QEMU 会直接将该内核镜像加载到虚拟机内存并运行。
#
# - **`-m {mem}`**  
#   设置虚拟机的内存大小，例如 `-m 2G` 表示分配 2GB 内存，默认单位是 MB（如 `-m 512` 表示 512MB）。
#
# - **`-nographic`**  
#   禁用图形界面，所有输出重定向到当前终端。虚拟机通过串口（UART）与用户交互，适合无图形环境的调试。
#
# - **`-smp {smp}`**  
#   设置虚拟机的 CPU 核心数，例如 `-smp 4` 表示 4 核 CPU，启用对称多处理器（SMP）支持。
#
# ---
#
# ### **2. 固件与启动**
# - **`-bios default`**  
#   使用 QEMU 内置的默认 BIOS（如 **OpenSBI**），负责初始化硬件并引导指定的内核。RISC-V 虚拟机通常不需要传统 BIOS，但需要 OpenSBI 作为监管模式（Supervisor）的中间件。
#
# ---
#
# ### **3. 存储设备**
# - **`-drive file={fs},if=none,format=raw,id=x0`**  
#   定义一个虚拟驱动器：  
#   - `file={fs}`：使用文件 `{fs}`（如 `rootfs.img`）作为存储后端。  
#   - `if=none`：不自动连接总线接口，需手动关联设备。  
#   - `format=raw`：磁盘格式为原始镜像（无额外元数据）。  
#   - `id=x0`：驱动器的唯一标识符，后续设备需通过此 ID 引用。
#
# - **`-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0`**  
#   创建一个 **VirtIO 块设备**，并关联到驱动器 `x0`：  
#   - `bus=virtio-mmio-bus.0`：设备连接到第一个 VirtIO-MMIO 总线（RISC-V 中常用 MMIO 而非 PCI 总线）。
#
# - **`-drive file=disk.img,...,id=x1`** 与 **`-device ...,bus=virtio-mmio-bus.1`**  
#   类似上述配置，添加第二个磁盘 `disk.img`，并连接到第二个 VirtIO-MMIO 总线（`.1`）。虚拟机内将看到两个独立磁盘设备。
#
# ---
#
# ### **4. 网络设备**
# - **`-device virtio-net-device,netdev=net`**  
#   创建一个 **VirtIO 网络设备**，提供高性能半虚拟化网络支持。
#
# - **`-netdev user,id=net`**  
#   定义网络后端为 **用户模式（User Networking）**：  
#   - 虚拟机通过 NAT 共享主机网络，支持访问外部网络，但外部无法直接访问虚拟机。  
#   - 默认启用 DHCP 和 DNS 服务，虚拟机 IP 通常为 `10.0.2.15`。  
#   - 可通过端口转发（如 `hostfwd=tcp::2222-:22`）暴露虚拟机端口到主机。
#
# ---
#
# ### **5. 其他参数**
# - **`-no-reboot`**  
#   虚拟机崩溃时直接退出 QEMU，而不是自动重启。
#
# - **`-rtc base=utc`**  
#   设置虚拟机的实时时钟（RTC）基准为 **UTC 时间**（而非主机本地时间）。
#
# ---
#
# ### **总结**
# 此命令启动一个 RISC-V 虚拟机，具备以下资源：  
# - 多核 CPU、指定内存大小  
# - 两个 VirtIO 块设备（`{fs}` 和 `disk.img`）  
# - 用户模式网络  
# - 无图形界面，通过串口交互  
# - 使用 OpenSBI 作为固件，直接加载自定义内核
#
# 适用于开发或测试 RISC-V 操作系统（如 Linux 或自定义内核），支持多磁盘和网络功能。
