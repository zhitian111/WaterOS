# WaterOS Nano-X 图形系统：从 VirtIO 设备到用户态窗口

## 1. 这套系统做了什么

WaterOS 的 Nano-X 图形方案不是让 GPU 直接“运行桌面”，而是把图形能力分成几层：

1. QEMU 模拟 VirtIO GPU、键盘和平板设备。
2. WaterOS 驱动识别设备，得到 framebuffer 和原始输入事件。
3. 内核通过 Linux 兼容的 `/dev/fb0`、`/dev/input/eventN` 向用户态提供设备。
4. Nano-X server 在用户态管理窗口、绘制控件、分发输入。
5. `nxclock`、`nxedit`、Doom 等客户端通过 AF_UNIX socket 请求 Nano-X 创建窗口。

一句话概括：

> WaterOS 负责把硬件抽象成 Linux 兼容设备，Nano-X 负责把一块像素内存抽象成可交互的窗口系统。

当前方案使用软件渲染。CPU 把像素写入 framebuffer，VirtIO GPU 只负责把 framebuffer
提交给 QEMU 图形窗口，不提供 3D 加速。

## 2. 先理解五个基本概念

### 2.1 GPU 设备

GPU 设备是能够向屏幕提交图像的硬件。在 QEMU 中使用的是 VirtIO GPU。首版只使用它的
2D scanout/framebuffer 能力，没有 DRM、OpenGL 或 3D 加速。

### 2.2 Framebuffer

Framebuffer 可以理解为“一张位于内存中的整屏图片”。例如当前 QEMU 分辨率为
`1280×800`，每个像素 4 字节，则可见区域大约需要：

```text
1280 × 800 × 4 = 4,096,000 字节
```

WaterOS 当前使用 BGRA8888：每个像素依次包含蓝、绿、红和透明度四个 8 位分量。
修改 framebuffer 只是改变内存；还要执行 flush，VirtIO GPU 才会把变化显示出来。

### 2.3 显示驱动

显示驱动负责：

- 初始化 VirtIO GPU；
- 创建 DMA framebuffer；
- 告诉上层分辨率、步长、像素格式和物理地址；
- 在上层请求时刷新画面。

显示驱动并不知道什么是窗口、按钮或 Doom。

### 2.4 窗口系统

Nano-X 是用户态窗口系统。它知道窗口的位置、前后层级、标题栏、输入焦点和客户端，
并用软件算法把所有窗口组合成最终屏幕。内置 `nanowm` 负责窗口装饰、移动和管理。

### 2.5 图形应用

`nxclock`、`nxedit`、`nxlaunch` 和 Doom 都是 Nano-X 客户端。客户端通常不直接打开
`/dev/fb0`，而是连接 Nano-X server，请求 server 创建窗口和绘制内容。

## 3. 总体架构

```text
宿主机/QEMU
├── virtio-gpu-device       显示设备
├── virtio-keyboard-device  键盘
└── virtio-tablet-device    绝对坐标指针
           │
           ▼
WaterOS 驱动层
├── DisplayDevice ── DMA framebuffer ── flush()
└── InputDevice   ── RawInputEvent
           │
           ▼
WaterOS 内核兼容层
├── /dev/fb0
│   ├── ioctl：查询分辨率/像素格式、刷新
│   └── mmap：把 GPU DMA 页映射到用户空间
└── /dev/input/
    ├── event0、event1
    ├── keyboard0
    └── pointer0
           │
           ▼
用户态 Nano-X server
├── FB screen driver：打开并 mmap /dev/fb0
├── evdev keyboard driver：读取 keyboard0
├── evdev pointer driver：读取 pointer0
├── 软件绘制与窗口合成
└── 内置 nanowm
           │ AF_UNIX: /tmp/.nano-X
           ▼
Nano-X 客户端
├── nxlaunch / nxclock / nxeyes
├── nxcalc / nxedit / nxev
└── Doom
```

这里有三条不同的数据通路：

- 画面：应用 → Nano-X → framebuffer → VirtIO GPU → QEMU 窗口；
- 输入：QEMU 键鼠 → VirtIO input → evdev → Nano-X → 应用；
- 控制：应用 ⇄ `/tmp/.nano-X` ⇄ Nano-X server。

## 4. 代码放在哪里

| 层次 | 主要位置 | 职责 |
|---|---|---|
| QEMU 参数 | [`qemu_run.py`](../../os/scripts/run/qemu_run.py) | 挂载 GPU、keyboard、tablet 并打开图形窗口 |
| RISC-V 设备枚举 | [`enumerate.rs`](../../os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/enumerate.rs) | 从 DTB 枚举 VirtIO-MMIO，设备 ID 16/18 对应 display/input |
| RISC-V 驱动注册 | [`register.rs`](../../os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/register.rs) | 创建并注册显示与输入设备 |
| 显示公共 API | [`driver-display/.../lib.rs`](../../os/components/wateros-driver/driver-display/display-api/api-v0/src/lib.rs) | `FramebufferInfo`、`DisplayDevice`、设备注册表 |
| RISC-V GPU 驱动 | [`impl-virtio-mmio/src/lib.rs`](../../os/components/wateros-driver/driver-display/display-impl/impl-virtio-mmio/src/lib.rs) | 初始化 VirtIO GPU、DMA framebuffer、flush |
| 输入公共 API | [`driver-input/.../lib.rs`](../../os/components/wateros-driver/driver-input/input-api/api-v0/src/lib.rs) | `InputDeviceInfo`、`RawInputEvent`、设备注册表 |
| RISC-V 输入驱动 | [`impl-virtio-mmio/src/lib.rs`](../../os/components/wateros-driver/driver-input/input-impl/impl-virtio-mmio/src/lib.rs) | 读取 VirtIO input 队列和设备能力 |
| fbdev/evdev VFS | [`user_graphics.rs`](../../os/components/wateros-vfs/vfs-impl/impl-fd-session/src/user_graphics.rs) | `/dev/fb0`、输入节点、事件队列和 worker |
| Linux ioctl ABI | [`ioctl.rs`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/ioctl.rs) | 翻译 fbdev/evdev ioctl 和用户指针 |
| mmap syscall | [`mmap.rs`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/mmap.rs) | 校验权限并把 VFS 设备映射交给 MM |
| MM 公共接口 | [`mm-api/.../mmap.rs`](../../os/components/wateros-mm/mm-api/api-v0/src/mmap.rs) | `MmapKind::Device`、`DeviceMapping`、lease |
| Sv39 设备映射 | [`user_heap_mmap.rs`](../../os/components/wateros-mm/mm-impl/impl-sv39/src/user_heap_mmap.rs) | 将现有 DMA 物理页映射到用户页表 |
| 启动接线 | [`os/src/main.rs`](../../os/src/main.rs) | 初始化图形设备并启动输入 worker |
| Nano-X 配置 | [`config/wateros`](../../user/packages/microwindows/config/wateros) | 静态构建、FB 后端、evdev 后端、内置 nanowm |
| WaterOS 适配补丁 | [`patches/`](../../user/packages/microwindows/patches) | fbdev/evdev、launcher、刷新和 RV64 Doom 修复 |
| 用户包构建 | [`build.py`](../../user/packages/microwindows/build.py) | 交叉编译、静态 ELF 检查、安装程序和 WAD |
| 启动脚本 | [`start-nanox`](../../user/packages/microwindows/scripts/start-nanox) | 启动 server、等待 socket、启动默认客户端并清理 |

## 5. 启动时发生了什么

### 5.1 构建期开关

图形用户态接口由 `user-graphics` feature 控制：

```text
user-graphics
├── driver/display
├── driver/input
└── vfs/user-graphics
```

它与内核 `wateros-gui` 互斥。原因是两者都会访问同一 framebuffer 和输入设备；若同时
启用，两个绘制方可能互相覆盖画面，两个输入消费者也可能争抢事件。

比赛默认构建不启用 `user-graphics`，因此不会增加 GPU 探测、输入 worker 和用户态
图形内存开销。

### 5.2 QEMU 创建设备

运行命令包含 `EXTRA_FEATURES=user-graphics` 时，Make 会启用图形输出，
[`qemu_run.py`](../../os/scripts/run/qemu_run.py) 为 RISC-V 添加：

```text
-device virtio-gpu-device
-device virtio-keyboard-device
-device virtio-tablet-device
```

LoongArch 使用相同设备类型的 PCI transport：

```text
-device virtio-gpu-pci
-device virtio-keyboard-pci
-device virtio-tablet-pci
```

串口仍通过 `-serial stdio` 保留，所以会同时看到两个界面：

- 当前终端：WaterOS 串口 shell 和日志；
- QEMU 图形窗口：Nano-X 桌面。

### 5.3 内核枚举设备

RISC-V QEMU 使用 VirtIO-MMIO。QEMU 把设备节点写进 DTB，WaterOS 遍历节点并读取
VirtIO header 的 device ID：

```text
16 → Display
18 → Input
```

显示设备构造为 `VirtioGpuMmioDevice`，键盘和平板分别构造为
`VirtioInputMmioDevice`，再放入全局设备注册表。上层只依赖 `DisplayDevice` 和
`InputDevice` trait，不依赖 MMIO 细节。

LoongArch 走 VirtIO-PCI，使用相同公共 API，只替换设备发现和 transport 实现。

### 5.4 创建用户图形接口

驱动初始化完成后，`bringup_driver_and_user()` 调用：

```text
initialize_user_graphics_devices()
spawn_kernel_task(user_graphics_input_worker)
```

随后 devfs 可以暴露：

```text
/dev/fb0
/dev/input/event0
/dev/input/event1
/dev/input/keyboard0
/dev/input/pointer0
```

`eventN` 是稳定注册序号；`keyboard0` 和 `pointer0` 根据设备类型建立别名，不依赖键盘
和平板在 QEMU 参数中的先后顺序。

## 6. 一帧画面是怎样显示出来的

### 6.1 驱动创建 DMA framebuffer

GPU 驱动初始化时：

1. 通过 VirtIO transport 与设备协商；
2. 查询 QEMU 的实际分辨率；
3. 调用 `setup_framebuffer()` 分配物理连续的 DMA 页；
4. 清零 framebuffer；
5. 生成 `FramebufferInfo`。

`FramebufferInfo` 中的重要字段：

| 字段 | 含义 |
|---|---|
| `width/height` | 实际分辨率 |
| `stride` | 相邻两行起点之间的字节数，不应假定永远等于 `width × 4` |
| `format` | 当前为 BGRA8888 |
| `byte_len` | 真正可见 framebuffer 的字节数 |
| `phys_base` | DMA 内存物理起点，供设备 mmap 使用 |
| `mapped_len` | 向页大小对齐后的映射长度 |
| `base` | 内核恒等映射地址，仅供内核访问与诊断 |

必须区分 `byte_len` 和 `mapped_len`：最后一页可能只有一部分属于可见画面，但页表映射
必须以完整页为单位。

### 6.2 `/dev/fb0` 提供 Linux fbdev 语义

VFS 中的 `FramebufferHandle` 持有 `SharedDisplayDevice`，实现：

- `read/write/lseek`：便于诊断和简单像素写入；
- `device_mapping()`：返回 framebuffer 的物理地址、长度和生命周期 lease；
- `flush_device()`：转发到 `DisplayDevice::flush()`；
- `special_device_info()`：把驱动信息转换成 VFS 中立结构。

syscall 层实现 Linux fbdev ioctl：

| ioctl | WaterOS 行为 |
|---|---|
| `FBIOGET_FSCREENINFO` | 返回 framebuffer 地址、长度、stride 等固定信息 |
| `FBIOGET_VSCREENINFO` | 返回分辨率、32 bpp 和 BGRA 通道布局 |
| `FBIOPUT_VSCREENINFO` | 首版只接受与当前模式完全相同的参数 |
| `FBIOPAN_DISPLAY` | 校验零偏移后执行一次 GPU flush |
| cmap ioctl | true-color 模式不需要调色板，返回 `ENOTTY` |

Linux ABI 结构使用 `#[repr(C)]`，并在编译期断言 64 位结构大小为 80 和 160 字节。
用户指针的读写只发生在 syscall 层，驱动和 VFS 不直接解引用用户地址。

### 6.3 Nano-X mmap framebuffer

Nano-X 的 framebuffer screen driver 大致执行：

```c
fd = open("/dev/fb0", O_RDWR);
ioctl(fd, FBIOGET_FSCREENINFO, ...);
ioctl(fd, FBIOGET_VSCREENINFO, ...);
fb = mmap(NULL, length, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
```

WaterOS 收到 `mmap` 后进行两层校验：

- syscall 层确认是 framebuffer、`MAP_SHARED`、不可执行、权限与 fd 打开方式一致；
- MM 层确认 offset 和长度按页对齐且不越过 DMA 区域。

通过校验后，MM 将同一批 GPU DMA 物理页映射到 Nano-X 用户页表。这里不新建一份
framebuffer，也不做逐帧 `copy_to_user`，因此避免了内核与用户态之间的大块复制。

需要准确表述：这是“用户态直接共享 framebuffer DMA 页”，但不是 3D GPU 加速；
像素仍由 CPU 软件绘制，VirtIO GPU flush 时仍要向虚拟设备提交显示更新。

### 6.4 为什么需要 lease

设备 VMA 中保存一个 `DeviceMappingLease`。只要用户映射仍存在，lease 就持有底层显示
设备，防止 DMA framebuffer 被提前析构。

设备页的所有权属于 GPU 驱动，不属于用户地址空间：

- `fork` 后父子进程共享相同物理页和 lease，不触发 COW；
- `munmap` 或进程退出只删除 PTE，不把设备页交给普通 frame allocator；
- `MAP_FIXED` 替换映射时也只移除设备 VMA；
- 禁止设备映射增加执行权限；
- 页表变化继续使用 active CPU mask 和 TLB shootdown。

这避免了“双重释放 DMA 页”和“设备已经销毁但用户仍在访问”的问题。

### 6.5 绘制和刷新

Nano-X server 接受客户端绘制请求，在 CPU 上完成文字、线条、矩形、位图和窗口合成，
最终直接修改 mmap framebuffer。

仅写内存不会自动更新 QEMU 窗口。WaterOS 的 Nano-X patch 在每轮 server 事件循环调用：

```text
FBIOPAN_DISPLAY
  → syscall framebuffer_ioctl()
  → VFS FramebufferHandle::flush_device()
  → DisplayDevice::flush()
  → VirtIOGpu::flush()
  → QEMU 图形窗口更新
```

首版采用全屏刷新。公共 API 已保留 `flush_region()`，以后可基于脏矩形只提交变化区域。

## 7. 鼠标和键盘是怎样到达应用的

### 7.1 驱动读取原始事件

VirtIO input 事件已经采用 Linux evdev 的三元组形式：

```text
type + code + value
```

例如：

```text
EV_KEY + KEY_A    + 1       A 键按下
EV_KEY + KEY_A    + 0       A 键释放
EV_ABS + ABS_X    + 16384   平板 X 坐标变化
EV_KEY + BTN_LEFT + 1       鼠标左键按下
EV_SYN + SYN_REPORT + 0     一组事件结束
```

输入驱动还查询设备名称、支持的事件位图以及绝对 X/Y 范围，由此判断设备是 Keyboard、
Pointer 还是 Unknown。

### 7.2 为什么使用输入 worker

当前 VirtIO input 首版使用一个低优先级内核 worker 轮询，而不是让每个用户进程直接争抢
驱动队列：

1. worker 每次从每个设备最多批量取 64 个事件；
2. 给事件加单调时间戳；
3. 广播到该设备的每个打开者；
4. 唤醒正在 `read/poll/select` 中等待的任务；
5. 有事件时 `yield`，无事件时 sleep 一个 tick，避免忙等占满 CPU。

每次 `open` 拥有独立的 256 项队列，所以两个调试程序可以同时观察同一输入设备，
不会由先读取者独占事件。队列溢出时清空旧事件并插入 `SYN_DROPPED`，通知用户态重新
同步状态。

### 7.3 Linux `input_event` 格式

用户态读取到的是 64 位 Linux 兼容结构：

```c
struct input_event {
    int64_t  tv_sec;
    int64_t  tv_usec;
    uint16_t type;
    uint16_t code;
    int32_t  value;
};
```

固定大小为 24 字节。一次 `read` 只返回完整事件；不足 24 字节的请求被拒绝。

evdev 还支持：

- 阻塞读取；
- `O_NONBLOCK` 下无事件返回 `EAGAIN`；
- 被信号打断返回 `EINTR`；
- 队列非空时 `poll/select` 报告可读；
- `EVIOCGNAME/ID/BIT/ABS` 等能力查询。

### 7.4 Nano-X 输入后端

WaterOS patch 为 Nano-X 增加两个后端：

- keyboard 后端打开 `/dev/input/keyboard0`，把 Linux key code 转换为 Nano-X key，
  同时维护 Shift、Ctrl、Alt、Meta 和 CapsLock；
- pointer 后端打开 `/dev/input/pointer0`，通过 `EVIOCGABS` 查询硬件坐标范围，再按当前
  屏幕宽高缩放绝对坐标，并转换鼠标按键状态。

Nano-X server 再根据窗口位置和输入焦点，把键盘、鼠标事件投递给正确客户端。

## 8. Nano-X server 与客户端如何配合

### 8.1 Server 是唯一的显示管理者

`nano-X` 进程负责：

- 独占打开并 mmap `/dev/fb0`；
- 打开键盘和指针设备；
- 维护窗口树、裁剪区、层级和焦点；
- 软件绘制并刷新 framebuffer；
- 监听客户端连接。

内置 `nanowm` 被链接进 server，因此不需要额外启动一个窗口管理器进程。

### 8.2 客户端使用 AF_UNIX socket

server 在 `/tmp/.nano-X` 创建 AF_UNIX socket。客户端调用 `GrOpen()` 后连接该 socket，
通过协议发送“创建窗口、画文字、选择事件”等请求，并接收输入或窗口事件。

```text
nxclock ─┐
nxedit  ─┼── AF_UNIX /tmp/.nano-X ── nano-X ── /dev/fb0
Doom    ─┘
```

`/tmp` 必须先由内核挂载为 tmpfs，否则 socket 无法创建。

监听 socket 的 `poll/select` 必须在 accept 队列非空时报告 `POLLIN`。WaterOS 曾出现
“只能看到鼠标、客户端全在运行但没有窗口”的问题，根因就是服务端没有从 accept 队列
取出连接；现在 [`unix_sock.rs`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/unix_sock.rs)
已经实现该语义。

### 8.3 `start-nanox` 做了什么

直接执行 `nano-X` 只会启动 server。推荐执行 `start-nanox`，它会：

1. 检查 `/dev/fb0`、`keyboard0` 和 `pointer0`；
2. 删除上次异常退出残留的 `/tmp/.nano-X`；
3. 后台启动 `nano-X`；
4. 最多等待约 5 秒，直到 socket 创建成功；
5. 启动 `nxlaunch`、`nxclock` 和 `nxeyes`；
6. server 退出时清理所有客户端和 socket。

`nxlaunch` 从 `/etc/wateros/nxlaunch.cnf` 读取菜单，目前可启动 Clock、Eyes、Calculator、
Editor、Events 和 Doom。

## 9. Doom 为什么也能显示

Doom 是一个 Nano-X 客户端，不直接访问 `/dev/fb0`。它把 320×200 的调色板画面转换成
ARGB，再调用 Nano-X API 创建窗口并提交像素。

镜像中的相关文件：

```text
/usr/bin/doom
/usr/bin/start-doom
/usr/share/games/doom/doom1.wad
```

`doom` ELF 不包含游戏数据。直接在 `/usr/bin` 执行 `doom` 时，它默认从当前目录找 WAD，
因此会出现 `W_InitFiles: no files found`。`start-doom` 会设置：

```text
DOOMWADDIR=/usr/share/games/doom
```

并默认添加 `-3 -warp 1 1`：三倍放大且直接进入 E1M1，避开当前 WAD 内置 demo 与老端口
版本不一致的问题。

上游 Doom 还在每帧使用大块 `alloca()`。三倍 ARGB 画面会耗尽 RV64 普通用户栈，曾在
完成初始化后立即段错误。WaterOS patch 将缩放缓冲和 ARGB 缓冲改为可复用堆内存。

## 10. 用户镜像如何提供 Nano-X

Nano-X 不编进内核，而是由 `user/` 用户空间构建系统交叉编译并安装到 EXT4 根文件系统。

默认 `PACKAGE=all` 在 RV 和 LA 上都会选择：

```text
base-layout + busybox + operator-tools + microwindows
```

Microwindows 固定使用 vendored 源码，WaterOS 修改全部位于 package patches，不直接污染
上游源码。构建配置：

- 静态链接（RV 使用 musl，LA 使用 glibc）；
- `SCREEN=FB`；
- `MOUSE/KEYBOARD=WATEROS_EVDEV`；
- `NANOX=Y`、`NANOWM=Y`；
- 使用内置字体；
- 禁用 X11、SDL、NX11、SysV SHM 和外部字体/图片依赖；
- 不构建依赖 PTY 的 `nxterm`。

构建器用目标架构 `readelf` 检查每个 ELF，确保架构正确、没有 `PT_INTERP`，也没有动态
`NEEDED` 库，使镜像不依赖动态链接器。

## 11. 并发、锁与 SMP 安全

图形链路在多核下需要避免“拿着自旋锁睡眠”以及“设备销毁时仍有映射”：

- 显示设备使用 `Arc<Mutex<Box<dyn DisplayDevice>>>` 共享；
- VFS 只在读取元数据、访问 framebuffer 或 flush 时短暂持有显示锁；
- 显示锁不跨用户地址复制、页表修改或任务调度；
- 输入 worker 获取原始事件后再逐客户端入队，唤醒 waitqueue 时不持有设备锁；
- 每个打开者有独立事件锁和队列；
- 设备 VMA 用 lease 保持 DMA 内存生命周期；
- 页表变化复用多核 active CPU mask 和 TLB shootdown。

系统只启动一个输入 worker，避免多个 CPU 重复消费同一个 VirtIO 输入队列。worker 本身
可以被调度到任意在线 CPU，功能不依赖 BSP。

## 12. 为什么这样设计

### 12.1 为什么兼容 fbdev/evdev，而不是创造 WaterOS 专用 API

Nano-X 已有 Linux framebuffer 和输入后端。提供 Linux 兼容子集可以复用成熟用户程序，
也能用标准头文件和 ioctl 编译，不必在每个应用中维护 WaterOS 私有移植层。

兼容的是“够用且明确的子集”，不是宣称完整实现 Linux DRM/evdev。

### 12.2 为什么直接 mmap，而不是 write framebuffer

1280×800×4 的一帧约 4 MB。如果每次显示都执行用户态 → 内核临时缓冲 → 驱动缓冲的
复制，CPU 和内存带宽开销很大。直接 mmap 让 Nano-X 写入真正的 DMA framebuffer，
只在刷新时提交设备命令。

`read/write/lseek` 仍保留用于诊断，但不是正常桌面绘制路径。

### 12.3 为什么首版主动全屏 flush

VirtIO framebuffer 是普通内存，设备不知道哪些字节被 CPU 改过。理想方案是 Nano-X
维护脏矩形并调用区域刷新；但上游部分 blit 路径会直接写 mmap 内存而不触发统一的
`Update` 回调。首版每轮事件循环全屏 flush，优先保证正确性，之后再优化传输范围。

### 12.4 为什么不拆成 `wateros-fbdev` 和 `wateros-input-event`

当前实现按既有职责放置：

- 驱动组件描述硬件能力；
- VFS 实现设备文件和每次打开的状态；
- syscall 实现 Linux ABI 与用户指针复制；
- MM 实现设备页映射。

fbdev/evdev 是跨层功能，不是新的硬件子系统。单独创建过细的顶层组件反而会复制 VFS、
syscall 和 MM 能力，使依赖关系更复杂。

### 12.5 为什么 Nano-X 与内核 GUI 互斥

内核 `wateros-gui` 适合启动画面和内核诊断，Nano-X 适合真正的用户程序和进程隔离。
两者都是 framebuffer 的最终所有者，首版不做 compositor 嵌套，因此编译期互斥最安全。

## 13. 构建与运行

### 13.1 首次准备工具链

```bash
make -C user setup ARCH=rv
# LoongArch 使用：make -C user setup ARCH=la
```

### 13.2 生成 Nano-X 根文件系统

```bash
make -C user image ARCH=rv
# LoongArch 使用：make -C user image ARCH=la
```

输出：

```text
user/build/images/wateros-rv.ext4
user/build/images/wateros-la.ext4
```

### 13.3 启动 WaterOS

```bash
cd os
make shell \
  ARCH=rv \
  PROFILE=pre \
  SDCARD=../user/build/images/wateros-rv.ext4 \
  EXTRA_FEATURES=user-graphics
```

LoongArch 对应命令：

```bash
cd os
make shell \
  ARCH=la \
  PROFILE=pre \
  SDCARD=../user/build/images/wateros-la.ext4 \
  EXTRA_FEATURES=user-graphics
```

进入串口 shell 后：

```sh
start-nanox >/tmp/nanox.log 2>&1 &
```

随后在 QEMU 图形窗口中操作。运行 Doom 可以点击 launcher 中的 Doom，或在串口执行：

```sh
start-doom
```

指定缩放和关卡：

```sh
start-doom -2
start-doom -3 -warp 1 2
```

## 14. 现场演示建议

答辩时按以下顺序演示，能同时证明驱动、内存映射、输入和用户态 IPC：

1. 启动后展示设备节点：

   ```sh
   ls -l /dev/fb0 /dev/input/keyboard0 /dev/input/pointer0
   ```

2. 启动 Nano-X：

   ```sh
   start-nanox >/tmp/nanox.log 2>&1 &
   sleep 1
   ps
   cat /tmp/nanox.log
   ```

3. 在图形窗口移动鼠标、拖动窗口，证明输入和窗口管理有效。
4. 打开 Calculator 或 Editor，证明客户端/server 协议和键盘输入有效。
5. 执行 `start-doom`，证明复杂静态用户程序可以复用同一窗口系统。
6. 回到串口说明：图形窗口与 shell 并存，Nano-X 崩溃也不会让内核失去操作入口。

预期 `nano-X` 日志中会看到类似：

```text
1280x800x32bpp pitch 5120 ...
```

这表明 fbdev ioctl 成功返回实际显示模式。

## 15. 常见问题排查

### 15.1 没有弹出 QEMU 图形窗口

确认启动参数包含：

```text
EXTRA_FEATURES=user-graphics
```

也可以显式指定：

```text
GRAPHICS=1 GRAPHICS_BACKEND=gtk
```

### 15.2 缺少 `/dev/fb0`

检查：

- 是否启用了 `user-graphics`；
- QEMU 命令是否包含 `virtio-gpu-device`；
- 启动日志是否出现 `registered virtio-gpu`；
- devfs 是否在驱动注册后刷新。

### 15.3 缺少键盘或指针节点

检查日志中是否出现：

```text
registered virtio-input ... Keyboard
registered virtio-input ... Pointer
```

还要确认 QEMU 挂载了 keyboard 和 tablet，而不只是 GPU。

### 15.4 黑屏但能看到鼠标

这通常说明：

- Nano-X server 已经 mmap framebuffer 并在运行；
- 输入和软件鼠标绘制基本正常；
- 客户端连接或窗口刷新路径仍有问题。

检查：

```sh
ps
cat /tmp/nanox.log
ls -l /tmp/.nano-X
```

如果客户端存在但没有窗口，重点检查 AF_UNIX listener 的 `POLLIN`/accept queue；如果
窗口内容写入后不出现，重点检查 `FBIOPAN_DISPLAY` 和 VirtIO flush。

### 15.5 `nano-X did not create /tmp/.nano-X`

直接运行 server 查看错误：

```sh
rm -f /tmp/.nano-X
nano-X
```

常见原因是 `/dev/fb0` 不存在、ioctl/mmap 失败或 `/tmp` 未挂载。

### 15.6 输入无响应

可以短暂读取二进制事件验证节点：

```sh
od -An -tx1 -N 48 /dev/input/pointer0
```

移动鼠标后应得到 24 字节整数倍的数据。不要长期用 `cat` 把二进制事件输出到串口。

### 15.7 直接运行 `doom` 找不到 WAD

使用：

```sh
start-doom
```

或显式执行：

```sh
DOOMWADDIR=/usr/share/games/doom doom -3 -warp 1 1
```

### 15.8 server 重启失败

删除残留 socket：

```sh
rm -f /tmp/.nano-X
```

正常情况下 `start-nanox` 会自动清理。

## 16. 当前限制和下一步

当前已经完成：

- RISC-V QEMU VirtIO-MMIO GPU、键盘和平板；
- LoongArch QEMU VirtIO-PCI GPU、键盘和平板；
- Linux fbdev/evdev 兼容子集；
- framebuffer 设备页 mmap；
- Nano-X、内置 nanowm 和多个客户端；
- Doom 图形运行；
- RV/LA 静态 Nano-X 用户包、演示程序和 Doom 均可运行；
- SMP 1/8 下已验证 LA `nano-X`、`nxlaunch`、`nxclock`、`nxeyes` 和 Doom 启动。

当前限制：

- 单显示器、固定启动分辨率；
- CPU 软件渲染，无 3D 加速；
- 首版全屏刷新，性能仍可优化；
- 输入使用低频轮询 worker，尚未改成完整中断驱动；
- 不支持 DRM/KMS、动态 mode setting、多显示器；
- 未实现 PTY，因此不构建 `nxterm`；
- Doom 当前没有完整音频后端，重点验证图形和输入。

建议优化顺序：

1. 实现可靠脏矩形并调用 `flush_region()`；
2. 将 VirtIO input 接入中断，减少轮询延迟和空闲唤醒；
3. 如需统一 libc，再补充可复现的 LoongArch musl 工具链；
4. 支持显示模式变更和更通用的像素格式；
5. 若需要更复杂桌面，再评估 PTY、字体、剪贴板和多进程会话管理。

## 17. 答辩速记

### 17.1 30 秒版本

> 我们在 QEMU 上使用 VirtIO GPU 和 VirtIO input。内核驱动获得 GPU 的 DMA
> framebuffer，并通过 Linux 兼容的 `/dev/fb0` 和 evdev 暴露给用户态。Nano-X server
> 用 `mmap(MAP_SHARED)` 直接映射 framebuffer，在 CPU 上完成软件绘制和窗口合成，之后
> 用 `FBIOPAN_DISPLAY` 触发 VirtIO flush。键盘和鼠标由内核 worker 转成 24 字节 Linux
> `input_event`，Nano-X 读取后按焦点分发给客户端。应用通过 `/tmp/.nano-X` 的 AF_UNIX
> socket 与 server 通信，因此能运行编辑器、计算器和 Doom，同时串口 shell 始终保留。

### 17.2 两分钟版本的讲解顺序

1. **硬件层**：QEMU 模拟 GPU、键盘和平板；RISC-V 走 MMIO，LoongArch 走 PCI。
2. **驱动层**：统一为 `DisplayDevice` 和 `InputDevice`，上层不依赖 transport。
3. **内核兼容层**：fbdev ioctl 提供模式信息，设备 mmap 共享 DMA 页；evdev 提供标准事件。
4. **用户态窗口层**：Nano-X 独占 framebuffer，内置 nanowm，完成软件绘制与事件分发。
5. **应用层**：客户端通过 AF_UNIX socket 请求窗口，不直接操作硬件。
6. **安全和扩展**：lease 防止 DMA 页提前释放，设备映射不参与 COW/普通帧回收；feature
   与内核 GUI 互斥，默认比赛构建不受影响。

### 17.3 常见答辩问题

**问：Nano-X 是驱动吗？**

不是。驱动只提供 framebuffer 和输入事件；Nano-X 是用户态窗口系统。

**问：这是 GPU 加速吗？**

不是 3D 加速。绘制和窗口合成由 CPU 完成，VirtIO GPU 负责显示提交。

**问：为什么用户程序不直接写 `/dev/fb0`？**

多个程序直接写会互相覆盖，也没有窗口层级和输入焦点。Nano-X 作为唯一 server 统一管理。

**问：为什么 mmap 比 write 更合适？**

一帧约 4 MB，逐帧 write 会产生额外大块复制。mmap 让用户态直接写 DMA framebuffer。

**问：直接 mmap 物理页是否危险？**

只允许 framebuffer fd、`MAP_SHARED`、读写权限、页对齐且不越界，禁止执行权限。VMA
还持有 lease，解除映射时只删 PTE，不会错误回收设备页。

**问：为什么需要显式 flush？**

CPU 写普通内存不会自动通知 VirtIO GPU。flush 才会把 framebuffer 更新提交到 scanout。

**问：输入为什么不让每个进程直接读驱动？**

驱动队列只能消费一次。worker 统一读取并向每个 open description 广播，支持调试程序和
Nano-X 同时观察事件。

**问：多核下如何保证安全？**

共享设备用短临界区锁；锁不跨调度、用户复制和页表操作；只有一个输入 worker；设备 VMA
复用 active CPU mask 和 TLB shootdown。

**问：为什么不用 Linux DRM？**

首期目标是小型、可解释、能运行用户窗口程序。fbdev + Nano-X 所需内核接口更少，适合
当前内核成熟度。架构中仍保留 `flush_region` 等扩展点。

**问：为什么还有一个 `wateros-gui`？**

`wateros-gui` 是内核内的软件 GUI，适合启动或诊断；Nano-X 是用户态窗口系统，适合隔离
的应用进程。两者首版互斥，避免竞争同一 framebuffer。

**问：怎样迁移到真实开发板？**

保留 `DisplayDevice/InputDevice` 上层接口，替换设备发现和具体驱动即可。fbdev、evdev、
MM 设备映射、Nano-X 和用户程序不需要跟着重写。

## 18. 术语表

| 术语 | 简单解释 |
|---|---|
| framebuffer | 内存中的整屏像素数组 |
| scanout | GPU 当前拿去显示的图像资源 |
| stride/pitch | framebuffer 一行占用的字节数 |
| DMA | 设备能够访问的物理内存 |
| fbdev | Linux 传统 framebuffer 设备接口 |
| evdev | Linux 通用输入事件接口 |
| ioctl | 对设备 fd 执行查询或控制的系统调用 |
| mmap | 把文件或设备物理页映射进进程虚拟地址空间 |
| PTE | 页表项，描述虚拟页到物理页的映射和权限 |
| lease | 保持设备/内存生命周期的引用令牌 |
| Nano-X | Microwindows 提供的轻量用户态窗口 server/API |
| nanowm | Nano-X 的窗口管理器，当前内置进 server |
| AF_UNIX | 同一系统内进程之间使用的本地 socket |
| software rendering | CPU 计算并写入每一个最终像素 |
| flush | 通知显示设备提交 framebuffer 的变化 |
