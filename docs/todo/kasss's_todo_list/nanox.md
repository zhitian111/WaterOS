# WaterOS Nano-X 图形系统：从 VirtIO 设备到用户态窗口

## 1. 这套系统做了什么

WaterOS 的 Nano-X 图形方案不是让 GPU 直接“运行桌面”，而是把图形能力分成几层：

1. QEMU 模拟 VirtIO GPU、键盘和平板设备。
2. WaterOS 驱动识别设备，得到 framebuffer 和原始输入事件。
3. 内核通过 Linux 兼容的 `/dev/fb0`、`/dev/input/eventN` 向用户态提供设备。
4. Nano-X server 在用户态管理窗口、绘制控件、分发输入。
5. `nxclock`、`nxedit`、Doom 等客户端通过 AF_UNIX socket 请求 Nano-X 创建窗口；Doom
   还使用 SysV SHM 命令区批量提交一帧请求。

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
| QEMU 参数 | [`qemu_run.py`](../../../os/scripts/run/qemu_run.py) | 挂载 GPU、keyboard、tablet 并打开图形窗口 |
| RISC-V 设备枚举 | [`enumerate.rs`](../../../os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/enumerate.rs) | 从 DTB 枚举 VirtIO-MMIO，设备 ID 16/18 对应 display/input |
| RISC-V 驱动注册 | [`register.rs`](../../../os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/register.rs) | 创建并注册显示与输入设备 |
| 显示公共 API | [`driver-display/.../lib.rs`](../../../os/components/wateros-driver/driver-display/display-api/api-v0/src/lib.rs) | `FramebufferInfo`、`DisplayDevice`、设备注册表 |
| VirtIO GPU 本地扩展 | [`gpu.rs`](../../../os/vendor/virtio-drivers/src/device/gpu.rs) | `flush_region()` 校验区域、计算 backing offset 并发送两条 GPU 命令 |
| RISC-V GPU 驱动 | [`impl-virtio-mmio/src/lib.rs`](../../../os/components/wateros-driver/driver-display/display-impl/impl-virtio-mmio/src/lib.rs) | 初始化 VirtIO GPU、DMA framebuffer、flush |
| 输入公共 API | [`driver-input/.../lib.rs`](../../../os/components/wateros-driver/driver-input/input-api/api-v0/src/lib.rs) | `InputDeviceInfo`、`RawInputEvent`、设备注册表 |
| RISC-V 输入驱动 | [`impl-virtio-mmio/src/lib.rs`](../../../os/components/wateros-driver/driver-input/input-impl/impl-virtio-mmio/src/lib.rs) | 读取 VirtIO input 队列和设备能力 |
| fbdev/evdev VFS | [`user_graphics.rs`](../../../os/components/wateros-vfs/vfs-impl/impl-fd-session/src/user_graphics.rs) | `/dev/fb0`、输入节点、事件队列和 worker |
| Linux ioctl ABI | [`ioctl.rs`](../../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/ioctl.rs) | 翻译 fbdev/evdev ioctl 和用户指针 |
| mmap syscall | [`mmap.rs`](../../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/mmap.rs) | 校验权限并把 VFS 设备映射交给 MM |
| MM 公共接口 | [`mm-api/.../mmap.rs`](../../../os/components/wateros-mm/mm-api/api-v0/src/mmap.rs) | `MmapKind::Device`、`DeviceMapping`、lease |
| Sv39 设备映射 | [`user_heap_mmap.rs`](../../../os/components/wateros-mm/mm-impl/impl-sv39/src/user_heap_mmap.rs) | 将现有 DMA 物理页映射到用户页表 |
| 启动接线 | [`os/src/main.rs`](../../../os/src/main.rs) | 初始化图形设备并启动输入 worker |
| Nano-X 配置 | [`config/wateros`](../../../user/packages/microwindows/config/wateros) | 静态构建、FB 后端、evdev 后端、内置 nanowm |
| WaterOS 适配补丁 | [`patches/`](../../../user/packages/microwindows/patches) | fbdev/evdev、launcher、刷新和 RV64 Doom 修复 |
| 用户包构建 | [`build.py`](../../../user/packages/microwindows/build.py) | 交叉编译、静态 ELF 检查、安装程序和 WAD |
| 启动脚本 | [`start-nanox`](../../../user/packages/microwindows/scripts/start-nanox) | 启动 server、等待 socket、启动默认客户端并清理 |

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
[`qemu_run.py`](../../../os/scripts/run/qemu_run.py) 为 RISC-V 添加：

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
| `FBIOPAN_DISPLAY` | 标准兼容回退：校验零偏移后执行一次全屏 GPU flush |
| `WOSFBIO_FLUSH_RECT` | WaterOS 扩展：校验矩形后只刷新变化区域 |
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
最终直接修改 mmap framebuffer。screen driver 的 `Update` 回调把每次修改合并成最小
包围矩形；framebuffer 打开后还会登记一次全屏脏区，保证第一帧必定显示。

仅写内存不会自动更新 QEMU 窗口。server 在等待下一批事件前只在存在脏区时调用：

```text
WOSFBIO_FLUSH_RECT(x, y, width, height)
  → syscall framebuffer_ioctl()（复制参数并校验边界）
  → VFS FramebufferHandle::flush_device_region()
  → DisplayDevice::flush_region()
  → VirtIOGpu::flush_region()
  → TRANSFER_TO_HOST_2D + RESOURCE_FLUSH
  → QEMU 图形窗口更新
```

如果内核不认识私有 ioctl，Nano-X 只在收到 `ENOTTY` 时回退到标准
`FBIOPAN_DISPLAY` 全屏刷新。提交失败时不会丢弃脏区，而是在下一轮重试。空闲桌面没有
脏区，因此不会持续提交 GPU 命令。

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

- keyboard 后端打开 `/dev/input/keyboard0`，通过显式美式 QWERTY 表把 Linux key code
  转换为 Nano-X key，不能假设 `KEY_A..KEY_Z` 连续；Shift 与 CapsLock 异或决定字母
  大小写，CapsLock 只在首次按下时切换，收到 `SYN_DROPPED` 则清除瞬时修饰键；
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
取出连接；现在 [`unix_sock.rs`](../../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/unix_sock.rs)
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

Doom 是一个 Nano-X 客户端，不直接访问 `/dev/fb0`。它使用 256 项 BGRA 查找表，在一次
循环中把 320×200 调色板画面直接放大到可复用的 32 位缓冲，再调用 Nano-X API 创建窗口
并提交像素；每帧不分配临时缓冲。

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

并默认添加 `-2 -warp 1 1`：二倍放大为 640×400 并直接进入 E1M1，避开当前 WAD 内置
demo 与老端口版本不一致的问题。用户显式传入 `-3` 时仍可使用 960×600。

`GrArea()` 受 Nano-X 单请求大小限制，大画面原本会被拆成几十个 socket 请求。Doom 在
`GrOpen()` 后申请 3 MiB SysV SHM 命令区，把一帧所有 Area 请求批量交给 server，随后用
一次 `GrFlush()` 标记帧边界。server 绘制完所有块后只留下一个合并脏区，因此每个显示帧
最多触发一次 GPU present。SHM 创建失败时自动退回 AF_UNIX 请求路径，程序仍可运行。

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
- 禁用 X11、SDL、NX11 和外部字体/图片依赖，启用 Nano-X SysV SHM 命令批处理；
- 构建 `nxterm`；它通过 WaterOS 的 `/dev/ptmx` 和 `/dev/pts/N` 启动 `/bin/sh`。

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

### 12.3 为什么使用脏矩形和一帧一次 present

VirtIO framebuffer 是普通内存，设备不知道哪些字节被 CPU 改过。若 server 每处理一个
请求就全屏刷新，960×600 的 Doom 帧被拆成约 77 个请求时，可能重复提交几十次
1280×800 画面。现在所有实际写屏路径通过 `Update` 累积脏矩形，Doom 又用 SHM 批量请求
和 `GrFlush()` 明确帧边界，所以 server 每帧只提交一次合并区域。初始全屏脏区兼顾首次
显示正确性，失败保留脏区则保证设备短暂错误不会永久丢帧。

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

## 13. 键盘与图形刷新优化

这一章集中说明 Nano-X 第一版运行后暴露出的两个问题，以及当前采用的完整优化链路：

- 键盘输入 `asd` 却得到 `abc`；
- Doom 能显示，但帧率低、窗口刷新慢，空闲桌面也在持续提交 GPU。

优化不是单纯把某个循环“调快”，而是同时调整输入翻译、Nano-X 请求传输、脏区管理、
内核 fbdev 接口、VirtIO GPU 提交和 Doom 像素转换。

### 13.1 优化前的问题

#### 键盘映射错误

Linux `evdev` 字母键码按照键盘位置定义，不是按照字母表连续排列。例如：

```text
KEY_A = 30
KEY_S = 31
KEY_D = 32
```

旧代码错误地使用：

```c
'a' + (code - KEY_A)
```

因此 `KEY_S` 被计算为 `b`，`KEY_D` 被计算为 `c`，最终表现为输入 `asd`，Nano-X
应用却收到 `abc`。

#### 全屏刷新过于频繁

旧补丁 `0003-wateros-present-every-loop.patch` 让 Nano-X server 在每轮循环中都执行：

```c
ioctl(fb, FBIOPAN_DISPLAY, &fb_var);
```

这意味着即使没有窗口变化，也会沿着下面的路径提交整屏：

```text
Nano-X server 循环
  → FBIOPAN_DISPLAY
  → VFS flush_device()
  → DisplayDevice::flush()
  → VirtIO GPU 全屏 TRANSFER_TO_HOST_2D
  → VirtIO GPU 全屏 RESOURCE_FLUSH
```

当前 framebuffer 为 `1280×800×4`，一次全屏提交约传输 4 MiB。只修改一个字符、移动一个
小窗口和完全空闲时，付出的代价却相同。

Doom 的旧默认窗口为 3×，即 `960×600×4`，单帧像素约 2.2 MiB。Nano-X 单个协议请求
上限约 30 KiB，`GrArea()` 必须把一帧拆成几十个请求。没有批处理时，这些请求分别经过
AF_UNIX socket，并可能让 server 多次返回事件循环；配合“每轮全屏刷新”，一帧可能触发
很多次 1280×800 提交。

### 13.2 键盘映射如何修复

当前 `kbd_wateros_evdev.c` 使用显式 Linux keycode → ASCII 表：

```c
[KEY_Q] = 'q', [KEY_W] = 'w', ...
[KEY_A] = 'a', [KEY_S] = 's', [KEY_D] = 'd', ...
[KEY_Z] = 'z', [KEY_X] = 'x', ...
```

这样不再假定 `KEY_A..KEY_Z` 与字母表具有相同顺序。当前固定使用美式 QWERTY 布局，并
同时处理：

- 26 个英文字母；
- 数字及其 Shift 符号；
- 退格、Tab、Enter、Escape；
- 方向键、Home、End、Insert、Delete、PageUp、PageDown；
- F1–F12；
- 常用小键盘按键；
- 左右 Shift、Ctrl、Alt 和 Meta。

字母大小写使用：

```text
大写 = Shift 状态 XOR Caps Lock 状态
```

Caps Lock 只在 `event.value == 1`，即首次按下时切换；`value == 2` 的硬件重复事件不会
反复切换。`value == 0/1/2` 分别作为释放、按下和重复处理。

收到 `SYN_DROPPED` 表示 evdev 客户端已经丢失一段事件，此时清除 Shift、Ctrl、Alt、
Meta 等瞬时状态，但保留 Caps Lock，避免漏掉释放事件后出现“按键一直按住”的现象。

### 13.3 先区分写像素、记录脏区和提交画面

当前刷新路径中有三个不同动作：

1. **绘制**：Nano-X 软件渲染器修改 mmap 得到的 framebuffer 内存；
2. **Update**：记录哪些坐标发生变化，不立即访问 GPU；
3. **present**：将合并后的区域提交给 VirtIO GPU，QEMU 窗口才真正变化。

`fblin32` 的点、水平线和垂直线，以及 fill、convblit、frameblit、窗口移动和 expose
等实际写屏路径，最终都会调用 screen driver 的 `Update`。内存 framebuffer 设备本身
不需要 `Update`，只有最终屏幕 framebuffer 才负责累计脏区。

### 13.4 当前脏矩形合并逻辑

`fb_update()` 不再只保存一个布尔值，而是保存脏区的最小包围矩形：

```text
dirty = [x1, y1, x2, y2)
```

收到新区域时执行：

```text
x1 = min(x1, update.x)
y1 = min(y1, update.y)
x2 = max(x2, update.x + update.width)
y2 = max(y2, update.y + update.height)
```

所有输入都会裁剪到屏幕范围；零尺寸和完全位于屏幕外的区域直接忽略。例如三个绘制请求：

```text
A = (10, 20, 100, 50)
B = (80, 30, 100, 60)
C = (20, 90, 40, 20)
```

最终只产生一个覆盖它们的矩形：

```text
(10, 20) 到 (180, 110)
```

framebuffer 打开后会登记一次全屏脏区，确保桌面第一次显示时不会再次出现“黑屏但鼠标
可见”。初次提交完成后恢复为普通区域刷新。

`fb_preselect()` 只有在存在脏区时才提交。提交成功后清除脏区；失败时保留，下轮重试，
避免一次设备错误造成永久缺帧。因此 Nano-X 完全空闲时不会继续产生 GPU present。

### 13.5 SysV SHM 在 Nano-X 中的作用

这里的共享内存不是 framebuffer，也不是 Doom 直接映射 GPU DMA 页。它共享的是
**Nano-X 客户端发给 server 的协议命令和像素参数**。

Doom 在 `GrOpen()` 后、发送其他绘图请求前调用：

```c
GrReqShmCmds(3 * 1024 * 1024);
```

建立过程如下：

```text
Doom client                         nano-X server
    │ GrNumReqShmCmds(size=3 MiB)        │
    ├──────── AF_UNIX socket ───────────>│
    │                                    ├─ shmget()
    │                                    ├─ shmat()
    │<──────────── SHM key ──────────────┤
    ├─ shmget(key)
    ├─ shmat()
    └─ nxAssignReqbuffer(shared_memory)
```

成功后，客户端原来的堆请求缓冲区被 SHM 替换：

```text
共享命令区
├── nxAreaReq 头 + 像素块 1
├── nxAreaReq 头 + 像素块 2
├── nxAreaReq 头 + 像素块 3
└── ...
```

`GrArea()` 仍会遵守约 30 KiB 的单请求限制，但几十个分块连续写入同一块 SHM，不再逐块
通过 socket 搬运。Doom 每帧最后显式调用 `GrFlush()`，客户端只通过 socket 发送一个很小
的 `nxShmCmdsFlushReq`，告诉 server：

- SHM 前多少字节有效；
- 是否需要完成确认。

socket 此时主要承担唤醒和同步作用，约 1–2.2 MiB 的帧数据留在共享内存中。

server 的 `GrShmCmdsFlushWrapper()` 在一次外层请求中遍历并执行所有内部命令。它会检查：

- flush 长度非零且不超过 SHM 大小；
- 剩余字节至少容纳一个请求头；
- 请求长度非零、正确对齐且不越界；
- 请求编号处于合法范围；
- SHM 中禁止嵌套 SHM 创建和 SHM flush 请求。

非法命令区会被整批拒绝，避免畸形客户端让 server 越界或死循环。

SHM 协商被 server 拒绝或 `shmget()` 失败时，客户端保持原有 AF_UNIX 请求路径，Doom
仍可运行，只是性能较低。上游客户端对极少见的最后一步 `shmat()` 失败处理仍不够完整，
后续应确保失败值被恢复为 `nxSharedMem = 0` 后再回退。

### 13.6 为什么一帧通常只提交一次

当前 Doom 的一帧按照下面的顺序执行：

```text
GrArea()
  → 将几十个 Area 分块写入 SHM
GrFlush()
  → 通过 socket 发送一次 SHM flush 通知
nano-X server
  → 在一次 GrShmCmdsFlushWrapper 中执行全部 Area
  → 每个绘图操作只合并 Update 脏区
server 返回事件循环
  → fb_preselect() 提交一个合并矩形
```

所以 SHM 解决“几十个协议请求分别传输和唤醒”的问题，脏矩形解决“每个绘图操作都全屏
提交”的问题，显式 `GrFlush()` 则定义帧边界。三者需要配合，单独启用其中一个不能完整
消除原来的刷新放大。

### 13.7 内核区域刷新链路

用户态优先使用 WaterOS 私有 ioctl：

```c
struct wos_fb_rect {
    uint32_t x;
    uint32_t y;
    uint32_t width;
    uint32_t height;
};

ioctl(fb, WOSFBIO_FLUSH_RECT, &rect);
```

内核调用链为：

```text
sys_ioctl
  → 复制 WosFramebufferRegion 用户参数
  → 检查非空、加法溢出和 framebuffer 边界
  → VfsIoHandle::flush_device_region()
  → FramebufferHandle 再次校验固定显示模式
  → DisplayDevice::flush_region()
  ├─ RISC-V：VirtIO-MMIO
  └─ LoongArch：VirtIO-PCI
  → virtio_drivers::VirtIOGpu::flush_region()
  → TRANSFER_TO_HOST_2D(rect, backing_offset)
  → RESOURCE_FLUSH(rect)
```

线性 framebuffer 的 backing offset 为：

```text
offset = ((y × screen_width) + x) × 4
```

底层驱动再次检查零尺寸、坐标加法溢出和越界。用户指针复制、边界校验和固定元数据查询
都不持有 display 锁；只有真正发送 GPU 命令时才获取该锁。

不支持区域 ioctl 的旧内核会返回 `ENOTTY`，Nano-X 随即回退到标准
`FBIOPAN_DISPLAY` 全屏刷新。其他 display 驱动若没有覆盖 `flush_region()`，公共 trait
默认也会安全退化为 `flush()`。

### 13.8 Doom 像素转换优化

旧实现分两步处理每帧：

```text
320×200 的 8 位索引图
  → 放大为 8 位中间缓冲
  → 整帧查调色板转换为 32 位 ARGB
```

当前维护一张 256 项 BGRA 查找表，使用一次循环直接完成缩放和颜色转换：

```text
源索引 → argb_palette[index] → 直接写入复用的 32 位输出缓冲
```

每条源扫描线只转换一次，纵向放大的其他行直接复制第一条结果。输出缓冲在分辨率确定后
只分配一次，不在每帧执行 `malloc/free`。Doom 改变调色板时会同步更新 256 项查找表，
因此受伤红屏、拾取物品等调色效果仍然有效。

`start-doom` 默认从 3× 改为 2×：

```text
默认：640×400，约 1.0 MiB/帧
可选 -3：960×600，约 2.2 MiB/帧
```

2× 同时减少 CPU 软件缩放、SHM 写入量、Nano-X 绘制量和 VirtIO 提交像素数。3× 仍作为
显式选项保留：

```sh
start-doom -3
```

### 13.9 优化前后对比

| 项目 | 优化前 | 当前实现 |
|---|---|---|
| 字母键 | 错误假设 keycode 连续 | 显式 QWERTY 映射 |
| Caps Lock | 重复事件可能反复切换 | 仅首次按下切换 |
| 事件丢失 | 修饰键可能卡住 | `SYN_DROPPED` 清理瞬时状态 |
| Nano-X 空闲 | 每轮全屏刷新 | 无脏区不提交 |
| 小窗口变化 | 提交 1280×800 全屏 | 提交合并脏矩形 |
| Doom 请求 | 多个 AF_UNIX 像素请求 | 3 MiB SysV SHM 批处理 |
| Doom 帧边界 | 不明确 | 每帧显式 `GrFlush()` |
| Doom 默认窗口 | 960×600 | 640×400 |
| 像素转换 | 8 位放大后再转 32 位 | 查表直接生成缩放 BGRA |
| 输出缓冲 | 原实现可能在热路径临时分配 | 分辨率确定后复用 |
| GPU 提交 | 一帧可能触发多次全屏提交 | 通常一帧一次区域 present |
| 兼容回退 | 只有全屏 ioctl | 区域 ioctl 不支持时回退全屏 |

### 13.10 如何观察优化是否生效

统计默认关闭。启动 server 时启用 Nano-X 统计：

```sh
NANOX_STATS=1 start-nanox >/tmp/nanox.log 2>&1 &
```

启动 Doom 时启用帧率和转换统计：

```sh
DOOM_STATS=1 start-doom
```

Nano-X 日志会报告：

- `updates`：收到的 Update 数量；
- `region`：区域 present 成功次数；
- `full`：回退全屏 present 次数；
- `pixels`：累计提交像素数。

Doom 会报告：

- FPS；
- 每帧平均像素转换时间；
- 每帧平均 Nano-X 请求提交时间。

验收时应重点观察：

1. Nano-X 空闲时 present 计数不再持续增长；
2. Doom 运行时 present 增长速度接近 Doom FPS，而不是一帧增长几十次；
3. `full` 正常保持为零，说明使用了区域 ioctl；
4. 提交像素量接近 Doom 窗口及其重绘区域，而不是每次固定 1280×800；
5. `nxedit` 中 `asd`、`qwerty`、Shift、Caps Lock、方向键和退格均正确。

### 13.11 相关实现位置

| 内容 | 文件/位置 |
|---|---|
| 键盘、脏区、SHM 加固和 Doom 优化补丁 | [`0005-wateros-input-present-doom-performance.patch`](../../../user/packages/microwindows/patches/0005-wateros-input-present-doom-performance.patch) |
| Nano-X SHM 构建开关 | [`config/wateros`](../../../user/packages/microwindows/config/wateros) |
| Doom 默认 2× 启动参数 | [`start-doom`](../../../user/packages/microwindows/scripts/start-doom) |
| 私有 framebuffer ioctl | [`ioctl.rs`](../../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/ioctl.rs) |
| VFS 中立区域和接口 | [`handle.rs`](../../../os/components/wateros-vfs/vfs-api/api-v0/src/handle.rs) |
| `/dev/fb0` 区域提交实现 | [`user_graphics.rs`](../../../os/components/wateros-vfs/vfs-impl/impl-fd-session/src/user_graphics.rs) |
| display 公共区域类型和 trait | [`display-api/api-v0/src/lib.rs`](../../../os/components/wateros-driver/driver-display/display-api/api-v0/src/lib.rs) |
| RISC-V MMIO 区域刷新 | [`impl-virtio-mmio/src/lib.rs`](../../../os/components/wateros-driver/driver-display/display-impl/impl-virtio-mmio/src/lib.rs) |
| LoongArch PCI 区域刷新 | [`impl-virtio-pci/src/lib.rs`](../../../os/components/wateros-driver/driver-display/display-impl/impl-virtio-pci/src/lib.rs) |
| VirtIO GPU offset、边界和两条区域命令 | [`gpu.rs`](../../../os/vendor/virtio-drivers/src/device/gpu.rs) |
| 静态优化检查 | [`test_userland.py`](../../../user/tests/test_userland.py) |

## 14. 构建与运行

### 14.1 首次准备工具链

```bash
make -C user setup ARCH=rv
# LoongArch 使用：make -C user setup ARCH=la
```

### 14.2 生成 Nano-X 根文件系统

```bash
make -C user image ARCH=rv
# LoongArch 使用：make -C user image ARCH=la
```

输出：

```text
user/build/images/wateros-rv.ext4
user/build/images/wateros-la.ext4
```

### 14.3 启动 WaterOS

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

### 14.4 图形终端与 PTY

先在串口验证 PTY 内核接口：

```sh
pty-smoke
ls -l /dev/ptmx /dev/pts
```

启动 Nano-X 后，可以点击 `nxlaunch` 中的 `Terminal`，也可以执行：

```sh
nxterm &
```

`nxterm` 本身只负责绘制字符窗口和把键盘事件变成字节，真正的 shell 是它的子进程
`/bin/sh`。启动链路为：

```text
nxterm
  -> posix_openpt("/dev/ptmx") 创建 PTY pair
  -> TIOCGPTN 得到 N，grantpt/unlockpt 解锁 /dev/pts/N
  -> fork，子进程 setsid 后打开 /dev/pts/N
  -> slave dup2 到 stdin/stdout/stderr
  -> exec /bin/sh
```

输入时，nxterm 写 master，内核 slave 行规程处理 canonical/raw、echo 和控制字符，shell
从 slave 读取；输出时 shell 写 slave，内核执行 `OPOST/ONLCR` 后由 nxterm 从 master 读取并
绘制。Ctrl-C/Ctrl-Z 由内核按 slave 的前台进程组投递，因此不是 nxterm 自己“模拟退出”。

相关实现位置：

| 层 | 位置 | 作用 |
| --- | --- | --- |
| TTY/PT​​Y 核心 | `os/components/wateros-tty/tty-impl/impl-console/src/pty.rs` | pair、行规程、队列、会话和控制事件 |
| VFS 设备 | `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/pty.rs` | `/dev/ptmx`、`/dev/pts/N`、`/dev/tty` 和 fd I/O |
| syscall | `sys/misc/ioctl.rs`、`sys/fs/io.rs` | PTY ioctl、作业控制和信号投递 |
| 用户程序 | `user/vendor/microwindows/src/demos/nanox/nxterm.c` | 窗口、终端字符显示及 shell 子进程 |

## 15. 现场演示建议

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

## 16. 常见问题排查

### 16.1 没有弹出 QEMU 图形窗口

确认启动参数包含：

```text
EXTRA_FEATURES=user-graphics
```

也可以显式指定：

```text
GRAPHICS=1 GRAPHICS_BACKEND=gtk
```

### 16.2 缺少 `/dev/fb0`

检查：

- 是否启用了 `user-graphics`；
- QEMU 命令是否包含 `virtio-gpu-device`；
- 启动日志是否出现 `registered virtio-gpu`；
- devfs 是否在驱动注册后刷新。

### 16.3 缺少键盘或指针节点

检查日志中是否出现：

```text
registered virtio-input ... Keyboard
registered virtio-input ... Pointer
```

还要确认 QEMU 挂载了 keyboard 和 tablet，而不只是 GPU。

### 16.4 黑屏但能看到鼠标

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

### 16.5 `nano-X did not create /tmp/.nano-X`

直接运行 server 查看错误：

```sh
rm -f /tmp/.nano-X
nano-X
```

常见原因是 `/dev/fb0` 不存在、ioctl/mmap 失败或 `/tmp` 未挂载。

### 16.6 输入无响应

可以短暂读取二进制事件验证节点：

```sh
od -An -tx1 -N 48 /dev/input/pointer0
```

移动鼠标后应得到 24 字节整数倍的数据。不要长期用 `cat` 把二进制事件输出到串口。

### 16.7 直接运行 `doom` 找不到 WAD

使用：

```sh
start-doom
```

或显式执行：

```sh
DOOMWADDIR=/usr/share/games/doom doom -3 -warp 1 1
```

### 16.8 server 重启失败

删除残留 socket：

```sh
rm -f /tmp/.nano-X
```

正常情况下 `start-nanox` 会自动清理。

### 16.9 点击 Doom 显示 `Exec format error`

`start-doom` 是 shell 脚本，不是 ELF。WaterOS 当前的 `execve` 不负责解析 shebang；而
`nxlaunch` 使用 `execvp` 直接执行配置中的程序，因此菜单必须写成：

```text
Doom - /bin/sh /usr/bin/start-doom
```

不能写成：

```text
Doom - /usr/bin/start-doom
```

修改用户镜像中的 launcher 配置后需要重新执行 `make image`，仅重新构建内核不会更新
`/etc/wateros/nxlaunch.cnf`。

## 17. 当前限制和下一步

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
- 当前使用包围矩形区域刷新；复杂遮挡可能仍比精确矩形集合多提交一些像素；
- 输入使用低频轮询 worker，尚未改成完整中断驱动；
- 不支持 DRM/KMS、动态 mode setting、多显示器；
- 已实现 UNIX98 PTY 兼容子集并构建 `nxterm`；暂未实现 PTY packet mode 和完整 devpts 挂载选项；
- Doom 当前没有完整音频后端，重点验证图形和输入。

建议优化顺序：

1. 将单个包围矩形扩展为小型矩形集合，减少相距很远窗口同时更新时的过度提交；
2. 将 VirtIO input 接入中断，减少轮询延迟和空闲唤醒；
3. 如需统一 libc，再补充可复现的 LoongArch musl 工具链；
4. 支持显示模式变更和更通用的像素格式；
5. 若需要更复杂桌面，再评估字体、剪贴板、PTY packet mode 和更完整的多进程会话管理。

## 18. 答辩速记

### 18.1 30 秒版本

> 我们在 QEMU 上使用 VirtIO GPU 和 VirtIO input。内核驱动获得 GPU 的 DMA
> framebuffer，并通过 Linux 兼容的 `/dev/fb0` 和 evdev 暴露给用户态。Nano-X server
> 用 `mmap(MAP_SHARED)` 直接映射 framebuffer，在 CPU 上完成软件绘制和窗口合成，之后
> 用 WaterOS 区域 ioctl 触发 VirtIO 的矩形 transfer 和 flush，不支持时才回退全屏刷新；
> Doom 通过 SysV SHM 把一帧命令批量交给 server。键盘和鼠标由内核 worker 转成 24 字节 Linux
> `input_event`，Nano-X 读取后按焦点分发给客户端。应用通过 `/tmp/.nano-X` 的 AF_UNIX
> socket 与 server 通信，因此能运行编辑器、计算器和 Doom，同时串口 shell 始终保留。

### 18.2 两分钟版本的讲解顺序

1. **硬件层**：QEMU 模拟 GPU、键盘和平板；RISC-V 走 MMIO，LoongArch 走 PCI。
2. **驱动层**：统一为 `DisplayDevice` 和 `InputDevice`，上层不依赖 transport。
3. **内核兼容层**：fbdev ioctl 提供模式信息，设备 mmap 共享 DMA 页；evdev 提供标准事件。
4. **用户态窗口层**：Nano-X 独占 framebuffer，内置 nanowm，完成软件绘制与事件分发。
5. **应用层**：客户端通过 AF_UNIX socket 请求窗口，不直接操作硬件。
6. **安全和扩展**：lease 防止 DMA 页提前释放，设备映射不参与 COW/普通帧回收；feature
   与内核 GUI 互斥，默认比赛构建不受影响。

### 18.3 常见答辩问题

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

## 19. 术语表

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
