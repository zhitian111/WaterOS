
建议先沿着 RISC-V 主链阅读。下面按照实际调用顺序列出文件和关键函数。

## 一、最短阅读路线

如果只想先把整体串起来，按这个顺序看：

1. `build_qemu_launch()`：QEMU 挂载哪些设备。
2. `wateros_kernel_main()`：内核启动入口。
3. `bringup_driver_and_user()`：驱动和用户图形初始化。
4. `scan_device_info()`：识别 VirtIO 设备类型。
5. `probe_virtio_devices()`：创建 GPU、键盘、平板驱动。
6. `VirtioGpuMmioDevice::from_mmio()`：建立 framebuffer。
7. `open_special_device()`：生成 `/dev/fb0` 和 evdev 句柄。
8. `sys_mmap()`：把 framebuffer 映射到 Nano-X。
9. `framebuffer_ioctl()`：查询模式并刷新。
10. `user_graphics_input_worker()`：采集输入事件。
11. Nano-X 的 `fb_open()`、`GsInitialize()`、`GsSelect()`。
12. Nano-X 客户端的 `GrOpen()`。

---

## 二、QEMU 和内核启动

### 1. QEMU 添加虚拟设备

[qemu_run.py:96](/home/kasss/WaterOS/os/scripts/run/qemu_run.py)

关键函数：

- `build_qemu_launch()`
  - RISC-V 添加：
    - `virtio-gpu-device`
    - `virtio-keyboard-device`
    - `virtio-tablet-device`
  - LoongArch 添加对应 PCI 设备。
- `_choose_display_backend()`
  - 选择 GTK、SDL、Cocoa 等 QEMU 图形后端。
- `main()`
  - 执行最终 QEMU 命令。

Feature 定义位于：

[Cargo.toml:70](/home/kasss/WaterOS/os/Cargo.toml)

```text
user-graphics
├── driver/display
├── driver/input
└── vfs/user-graphics
```

### 2. 内核启动图形链路

[main.rs:114](/home/kasss/WaterOS/os/src/main.rs)

关键函数：

- `wateros_kernel_main()`
  - RISC-V 入口在约 258 行。
  - 完成内存、调度器、驱动等初始化。
- `bringup_driver_and_user()`
  - 调用 `driver::machine().init_after_boot()`。
  - 调用 `vfs::initialize_user_graphics_devices()`。
  - 创建 `vfs::user_graphics_input_worker` 内核任务。

核心调用关系：

```text
wateros_kernel_main()
  → bringup_driver_and_user()
    → driver::machine().init_after_boot()
    → initialize_user_graphics_devices()
    → spawn_kernel_task(user_graphics_input_worker)
```

---

## 三、VirtIO 设备识别和注册

### 1. RISC-V 驱动总入口

[impl-qemu-riscv64-virt/src/lib.rs:35](/home/kasss/WaterOS/os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/lib.rs)

关键函数：

- `init_after_boot()`
- `init_after_boot_inner()`

主要负责：

```text
扫描 DTB
→ 枚举设备
→ 初始化 block/network/display/input
→ 注册到各个驱动子系统
```

顶层统一接口：

[wateros-driver/src/lib.rs:43](/home/kasss/WaterOS/os/components/wateros-driver/src/lib.rs)

- `machine()`
- `init_after_boot()`

### 2. 识别 VirtIO 设备类型

[enumerate.rs:20](/home/kasss/WaterOS/os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/enumerate.rs)

关键函数：

- `scan_device_info()`
  - 遍历 DTB 中的 VirtIO-MMIO 节点。
- `probe_virtio_device_type()`
  - 读取 VirtIO header 的 device ID。
- `mmio_read32()`
  - 从 MMIO header 读取寄存器。

当前映射：

```text
device_id = 16 → Display
device_id = 18 → Input
```

### 3. 创建设备实例并注册

[register.rs:38](/home/kasss/WaterOS/os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/register.rs)

关键函数：

- `probe_virtio_devices()`
  - 根据设备类型分别构造 block、network、display、input 驱动。
- `probe_character_devices()`

显示分支核心调用：

```text
VirtioGpuMmioDevice::from_mmio()
→ register_display_device()
```

输入分支：

```text
VirtioInputMmioDevice::from_mmio()
→ register_input_device()
```

---

## 四、画面输出链路

完整调用链可以概括为：

```text
Nano-X 软件绘制
→ mmap framebuffer
→ FBIOPAN_DISPLAY
→ sys_ioctl()
→ framebuffer_ioctl()
→ FramebufferHandle::flush_device()
→ DisplayDevice::flush()
→ VirtIOGpu::flush()
→ QEMU 窗口
```

### 1. 显示驱动公共接口

[display-api/lib.rs:24](/home/kasss/WaterOS/os/components/wateros-driver/driver-display/display-api/api-v0/src/lib.rs)

重点数据结构：

- `FramebufferInfo`
  - `width`
  - `height`
  - `stride`
  - `byte_len`
  - `phys_base`
  - `mapped_len`
  - `base`
- `FramebufferRegion`
- `SharedDisplayDevice`

重点 trait：

- `DisplayDevice::info()`
- `DisplayDevice::framebuffer()`
- `DisplayDevice::flush()`
- `DisplayDevice::flush_region()`

注册表函数：

- `register_display_device()`
- `first_display_device()`
- `display_device_at()`
- `display_device_count()`

### 2. RISC-V VirtIO GPU 实现

[impl-virtio-mmio/src/lib.rs:92](/home/kasss/WaterOS/os/components/wateros-driver/driver-display/display-impl/impl-virtio-mmio/src/lib.rs)

关键函数：

- `VirtioGpuMmioDevice::from_mmio()`
  - 创建 `MmioTransport`。
  - 初始化 `VirtIOGpu`。
  - 调用 `resolution()`。
  - 调用 `setup_framebuffer()`。
  - 构造 `FramebufferInfo`。
- `VirtioGpuMmioHal::dma_alloc()`
  - 为 GPU 分配连续 DMA 物理页。
- `DisplayDevice::framebuffer()`
  - 返回内核可写 framebuffer 切片。
- `DisplayDevice::flush()`
  - 调用 `VirtIOGpu::flush()` 提交显示。

### 3. LoongArch PCI GPU 实现

[impl-virtio-pci/src/lib.rs:141](/home/kasss/WaterOS/os/components/wateros-driver/driver-display/display-impl/impl-virtio-pci/src/lib.rs)

关键函数：

- `from_pci_root()`
- `probe_first_from_ecam()`
- `assign_memory_bars()`
- `dma_alloc()`
- `framebuffer()`
- `flush()`

上层接口与 RISC-V 完全相同，主要区别是：

```text
RISC-V：VirtIO-MMIO transport
LoongArch：VirtIO-PCI transport
```

---

## 五、`/dev/fb0` 的 VFS 实现

### 1. VFS 公共设备接口

[handle.rs:15](/home/kasss/WaterOS/os/components/wateros-vfs/vfs-api/api-v0/src/handle.rs)

重点数据结构：

- `VfsFramebufferInfo`
- `VfsSpecialDeviceInfo::Framebuffer`
- `VfsDeviceMapping`
- `VfsDeviceMappingLease`

`VfsIoHandle` 中与图形相关的方法：

- `special_device_info()`
- `device_mapping()`
- `flush_device()`
- `read()/write()/seek()`
- `poll_revents()`

这些方法让 syscall 层不需要直接依赖 VirtIO GPU 类型。

### 2. 创建 `/dev/fb0`

[user_graphics.rs:117](/home/kasss/WaterOS/os/components/wateros-vfs/vfs-impl/impl-fd-session/src/user_graphics.rs)

重点类型和函数：

- `FramebufferHandle`
- `FramebufferHandle::new()`
- `FramebufferHandle::read()`
- `FramebufferHandle::write()`
- `FramebufferHandle::seek()`
- `FramebufferHandle::special_device_info()`
- `FramebufferHandle::device_mapping()`
- `FramebufferHandle::flush_device()`

设备路径相关：

- `special_device_exists()`
- `special_device_metadata()`
- `special_device_paths()`
- `open_special_device()`

打开路径调用链：

```text
sys_openat()
→ FsBridge open()
→ open_special_device("/dev/fb0")
→ FramebufferHandle::new()
```

对应文件：

- [sys_openat():38](/home/kasss/WaterOS/os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/openat.rs)
- [FsBridge open():58](/home/kasss/WaterOS/os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/file_handle.rs)
- [open_special_device():597](/home/kasss/WaterOS/os/components/wateros-vfs/vfs-impl/impl-fd-session/src/user_graphics.rs)

---

## 六、fbdev ioctl 实现

[ioctl.rs:139](/home/kasss/WaterOS/os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/ioctl.rs)

重点函数：

- `sys_ioctl()`
  - ioctl 总入口。
  - 根据 fd 的 `VfsSpecialDeviceInfo` 分发。
- `framebuffer_ioctl()`
  - 实现 fbdev ioctl。
- `fb_var()`
  - 生成 Linux `fb_var_screeninfo`。
- `copy_to_user_struct()/copy_from_user_struct()`
  - 安全复制用户 ABI 结构。

主要 ioctl：

```text
FBIOGET_FSCREENINFO
FBIOGET_VSCREENINFO
FBIOPUT_VSCREENINFO
FBIOPAN_DISPLAY
```

刷新调用链：

```text
Nano-X ioctl(FBIOPAN_DISPLAY)
→ sys_ioctl()
→ framebuffer_ioctl()
→ with_current_io(fd)
→ FramebufferHandle::flush_device()
→ DisplayDevice::flush()
```

---

## 七、framebuffer 设备 mmap

### 1. syscall 层

[mmap.rs:90](/home/kasss/WaterOS/os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/mmap.rs)

核心函数：

- `sys_mmap()`
  - 判断 fd 是否为 framebuffer。
  - 调用 `handle.device_mapping()`。
  - 校验：
    - `MAP_SHARED`
    - 禁止 `MAP_PRIVATE`
    - 禁止 `PROT_EXEC`
    - fd 权限正确
    - framebuffer 物理地址页对齐
  - 构造 `DeviceMapping`。
  - 调用 `MmapOps::mmap_device()`。

### 2. MM 公共接口

[mm-api/mmap.rs:19](/home/kasss/WaterOS/os/components/wateros-mm/mm-api/api-v0/src/mmap.rs)

重点类型：

- `MmapKind::Device { offset }`
- `DeviceMapping`
- `DeviceMappingLease`
- `MmapRequest`

重点函数：

- `MmapOps::mmap_device()`
- `MmapOps::munmap()`
- `MmapOps::mprotect()`
- `MmapOps::mremap()`

### 3. RISC-V Sv39 实现

[user_heap_mmap.rs:99](/home/kasss/WaterOS/os/components/wateros-mm/mm-impl/impl-sv39/src/user_heap_mmap.rs)

重点函数：

- `mmap_device_inner()`
  - 校验映射范围。
  - 计算虚拟地址。
  - 将 framebuffer PPN 映射到用户 VPN。
  - 保存设备 VMA 和 lease。
- `mmap_device()`
  - trait 入口。
- `munmap()`
- `mprotect()`
- `mremap()`

页表实现：

[pagetable.rs:568](/home/kasss/WaterOS/os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs)

重点函数：

- `register_device_vma()`
- `remove_device_vmas()`
- `protect_device_vmas()`
- `device_vma_overlaps()`
- `unmap_mmap_range()`
- `map_page_to_ppn()`
- `unmap_page_to_ppn()`
- `fork_cow()`

设备页与普通内存最重要的区别在这里：

```text
解除设备映射时只删除 PTE
不会把 framebuffer 页交给普通 frame allocator
fork 时共享设备页，不执行 COW
```

---

## 八、输入设备链路

完整链路：

```text
QEMU keyboard/tablet
→ VirtIOInput::pop_pending_event()
→ InputDevice::pop_event()
→ user_graphics_input_worker()
→ 每个打开者的 EvdevClient 队列
→ read/poll/select
→ Nano-X KBD_Read()/MOU_Read()
→ GsCheckKeyboardEvent()/GsCheckMouseEvent()
→ 当前窗口客户端
```

### 1. 输入公共接口

[input-api/lib.rs:11](/home/kasss/WaterOS/os/components/wateros-driver/driver-input/input-api/api-v0/src/lib.rs)

重点数据结构：

- `InputDeviceKind`
- `AbsoluteAxis`
- `InputDeviceInfo`
- `RawInputEvent`
- `SharedInputDevice`

重点函数：

- `InputDevice::info()`
- `InputDevice::pop_event()`
- `register_input_device()`
- `input_devices()`
- `input_device_at()`

### 2. RISC-V VirtIO input

[impl-virtio-mmio/src/lib.rs:66](/home/kasss/WaterOS/os/components/wateros-driver/driver-input/input-impl/impl-virtio-mmio/src/lib.rs)

关键函数：

- `VirtioInputMmioDevice::from_mmio()`
- `query_info()`
  - 查询设备名称。
  - 查询 EV_REL/EV_ABS。
  - 查询绝对 X/Y 范围。
  - 判断 Keyboard/Pointer。
- `InputDevice::pop_event()`
  - 调用 `pop_pending_event()`。
  - 转换成 `RawInputEvent`。

### 3. 内核 evdev worker

[user_graphics.rs:508](/home/kasss/WaterOS/os/components/wateros-vfs/vfs-impl/impl-fd-session/src/user_graphics.rs)

初始化：

- `initialize_user_graphics_devices()`
  - 建立稳定的 `EvdevSlot`。
- `input_slot_for_path()`
  - 解析 `eventN`、`keyboard0`、`pointer0`。

事件采集：

- `user_graphics_input_worker()`
- `poll_input_once()`
- `LinuxInputEvent::new()`
- `LinuxInputEvent::syn_dropped_like()`
- `LinuxInputEvent::append_bytes()`

读事件：

- `EvdevHandle::open()`
- `EvdevHandle::prepare_read()`
- `EvdevPreparedRead::acquire()`
- `EvdevReadLease::finish()`
- `EvdevHandle::poll_revents()`
- `EvdevHandle::poll_wait_for_ticks()`

用户态 `read` 总入口：

[sys_read():55](/home/kasss/WaterOS/os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs)

### 4. evdev ioctl

[ioctl.rs:223](/home/kasss/WaterOS/os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/ioctl.rs)

重点函数：

- `evdev_ioctl()`
- `copy_evdev_bits()`
- `set_bit()`

支持：

```text
EVIOCGVERSION
EVIOCGID
EVIOCGNAME
EVIOCGBIT
EVIOCGABS
```

---

## 九、Nano-X 用户态适配

注意：WaterOS 的 Nano-X 修改以 patch 为准，构建时应用到 vendored 源码。

### 1. framebuffer 后端

基础源码：

[scr_fb.c:80](/home/kasss/WaterOS/user/vendor/microwindows/src/drivers/scr_fb.c)

重点函数：

- `fb_open()`
  - 打开 framebuffer。
- `open_linuxfb()`
  - 执行 fbdev ioctl。
  - mmap framebuffer。
- `fb_update()`
  - 标记画面变化。
- `fb_preselect()`
  - server 每轮事件循环刷新画面。
- `fb_close()`
- `fb_setpalette()`

WaterOS 强制刷新补丁：

[0005-wateros-input-present-doom-performance.patch](/home/kasss/WaterOS/user/packages/microwindows/patches/0005-wateros-input-present-doom-performance.patch)

重点修改：

```text
fb_preselect()
→ ioctl(FBIOPAN_DISPLAY)
```

### 2. Nano-X 键盘后端

[0001-wateros-fbdev-evdev.patch:22](/home/kasss/WaterOS/user/packages/microwindows/patches/0001-wateros-fbdev-evdev.patch)

新增函数：

- `KBD_Open()`
  - 打开 `/dev/input/keyboard0`。
- `KBD_Read()`
  - 读取 `struct input_event`。
- `special_key()`
  - 转换方向键、功能键等。
- `printable_key()`
  - 转换字母、数字和符号。
- `update_modifier()`
  - 维护 Shift/Ctrl/Alt/CapsLock。
- `KBD_Close()`
- `KBD_GetModifierInfo()`

### 3. Nano-X 指针后端

同一个补丁中的 `mou_wateros_evdev.c`。

重点函数：

- `MOU_Open()`
  - 打开 `/dev/input/pointer0`。
  - 用 `EVIOCGABS` 查询 X/Y 范围。
- `MOU_Read()`
  - 读取坐标与按钮事件。
- `scale_axis()`
  - 将 VirtIO tablet 原始坐标缩放到屏幕坐标。
- `MOU_Close()`
- `MOU_GetButtonInfo()`

---

## 十、Nano-X server

### 1. Server 主循环

[srvmain.c:104](/home/kasss/WaterOS/user/vendor/microwindows/src/nanox/srvmain.c)

重点函数：

- `main()`
  - `nano-X` 进程入口。
- `GsInitialize()`
  - 打开屏幕、键盘、鼠标和 server socket。
- `GsSelect()`
  - 主事件循环。
  - 等待客户端、键盘、鼠标和定时事件。
- `GsAcceptClientFd()`
  - 注册新客户端 fd。
- `GsPumpEvents()`
- `GsTerminate()`

输入分发：

[srvevent.c:113](/home/kasss/WaterOS/user/vendor/microwindows/src/nanox/srvevent.c)

- `GsCheckMouseEvent()`
- `GsCheckKeyboardEvent()`
- `GsHandleMouseStatus()`
- `GsDeliverKeyboardEvent()`

### 2. Server socket

[srvnet.c:1857](/home/kasss/WaterOS/user/vendor/microwindows/src/nanox/srvnet.c)

重点函数：

- `GsOpenSocket()`
  - 创建并监听 `/tmp/.nano-X`。
- `GsAcceptClient()`
- 各类请求 wrapper
  - 把客户端协议请求转到 server 内部绘制函数。

---

## 十一、Nano-X 客户端与 AF_UNIX

### 1. Nano-X 客户端库

[client.c:298](/home/kasss/WaterOS/user/vendor/microwindows/src/nanox/client.c)

重点函数：

- `GrOpen()`
  - 创建 AF_UNIX socket。
  - 连接 `/tmp/.nano-X`。
- `GrClose()`
- `GrFlush()`
- `GrMainLoop()`
- `GrGetNextEvent()`
- `GrCheckNextEvent()`
- `GrSelectEvents()`
- `GrMapWindow()`
- `GrSetFocus()`
- `GrNewGC()`
- `GrSetGCForeground()`

窗口创建和绘制 API 也都在该文件中，例如：

- `GrNewWindow()`
- `GrNewWindowEx()`
- `GrArea()`
- `GrText()`
- `GrFillRect()`

### 2. WaterOS AF_UNIX 实现

[unix_sock.rs:458](/home/kasss/WaterOS/os/components/wateros-syscall/syscall-impl/impl-kernel/src/unix_sock.rs)

重点函数：

- `alloc_unix_socket()`
- `bind()`
- `listen()`
- `connect()`
- `connect_stream()`
- `accept()`
- `UnixSocketHandle::read()`
- `UnixSocketHandle::write()`
- `UnixSocketHandle::poll_revents()`
- `UnixSocketHandle::poll_wait_for_ticks()`

其中 `poll_revents()` 很关键：监听 socket 的 `accept_queue` 非空时必须报告 `POLLIN`，否则 Nano-X 不会 accept 客户端，最终只能看到鼠标而看不到应用窗口。

相应 syscall 入口：

- [sys_socket():18](/home/kasss/WaterOS/os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/socket.rs)
- [sys_bind():26](/home/kasss/WaterOS/os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/bind.rs)
- [sys_listen():11](/home/kasss/WaterOS/os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/listen.rs)
- [sys_connect():29](/home/kasss/WaterOS/os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/connect.rs)
- [sys_accept():38](/home/kasss/WaterOS/os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/accept.rs)
- [sys_poll():10](/home/kasss/WaterOS/os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/poll/poll.rs)
- [sys_select():78](/home/kasss/WaterOS/os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/poll/poll_multiplex.rs)

---

## 十二、Doom 链路

### 1. Doom 进程入口

[i_main.c:35](/home/kasss/WaterOS/user/vendor/microwindows/src/contrib/doom/i_main.c)

调用：

```text
main()
→ D_DoomMain()
→ D_DoomLoop()
```

主要实现：

[d_main.c:795](/home/kasss/WaterOS/user/vendor/microwindows/src/contrib/doom/d_main.c)

- `D_DoomMain()`
- `D_DoomLoop()`
- `D_Display()`
- `D_ProcessEvents()`
- `IdentifyVersion()`
  - 查找 WAD。

### 2. Doom 的 Nano-X 视频后端

[i_video.c:432](/home/kasss/WaterOS/user/vendor/microwindows/src/contrib/doom/i_video.c)

重点函数：

- `I_InitGraphics()`
  - 创建 Nano-X 窗口。
- `I_StartTic()`
  - 获取 Nano-X 输入事件。
- `I_GetEvent()`
  - 转换键盘和鼠标事件。
- `I_FinishUpdate()`
  - 把 Doom 调色板画面转换成 ARGB。
  - 调用 Nano-X `GrArea()` 绘制。
- `I_SetPalette()`
- `I_ShutdownGraphics()`

RV64 framebuffer 栈溢出修复：

[0004-doom-rv64-frame-buffers.patch](/home/kasss/WaterOS/user/packages/microwindows/patches/0004-doom-rv64-frame-buffers.patch)

### 3. Doom 启动包装

[start-doom](/home/kasss/WaterOS/user/packages/microwindows/scripts/start-doom)

负责：

- 检查 `/tmp/.nano-X`。
- 设置 `DOOMWADDIR`。
- 默认添加 `-3 -warp 1 1`。
- 执行 `/usr/bin/doom`。

---

## 十三、用户镜像构建

[build.py:37](/home/kasss/WaterOS/user/packages/microwindows/build.py)

关键函数：

- `main()`
  - 应用 Microwindows 配置。
  - 构建 Nano-X server 和 demo。
  - 构建 Doom。
  - 安装 ELF、脚本、WAD 和 launcher 配置。
- `validate_static()`
  - 检查 ELF 架构。
  - 检查无 `PT_INTERP`。
  - 检查无动态 `NEEDED`。
- `run()`
  - 执行交叉构建命令。

Nano-X 启动脚本：

[start-nanox](/home/kasss/WaterOS/user/packages/microwindows/scripts/start-nanox)

关键 shell 函数：

- `cleanup()`

脚本主流程：

```text
检查设备节点
→ 删除旧 /tmp/.nano-X
→ 启动 nano-X
→ 等待 socket
→ 启动 nxlaunch/nxclock/nxeyes
→ server 退出后清理
```

---

完整画面主链最终可以压缩为：

```text
build_qemu_launch()
→ wateros_kernel_main()
→ bringup_driver_and_user()
→ init_after_boot()
→ scan_device_info()
→ probe_virtio_devices()
→ VirtioGpuMmioDevice::from_mmio()
→ register_display_device()
→ open_special_device("/dev/fb0")
→ FramebufferHandle::device_mapping()
→ sys_mmap()
→ mmap_device_inner()
→ Nano-X fb_open()
→ Nano-X 软件绘制
→ fb_preselect()
→ sys_ioctl(FBIOPAN_DISPLAY)
→ FramebufferHandle::flush_device()
→ VirtioGpuMmioDevice::flush()
→ QEMU 图形窗口
```
