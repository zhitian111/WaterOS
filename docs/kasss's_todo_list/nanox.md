# WaterOS Nano-X 用户态图形支持

## 当前状态

Nano-X 首版链路已经落地，默认比赛构建仍然不启用：

```text
VirtIO GPU ── DisplayDevice ── /dev/fb0 ── mmap ── nano-X
VirtIO input ── InputDevice ── evdev worker ── /dev/input/eventN
                                             ├─ keyboard0
                                             └─ pointer0
nano-X ⇄ /tmp/.nano-X ⇄ nxlaunch/nxclock/nxeyes/nxcalc/nxedit/nxev
```

实现没有新增 `wateros-fbdev` 或 `wateros-input-event` 组件：设备句柄在 VFS
fd-session 中，用户指针 ABI 在 syscall 中，固定物理页映射在 MM 中。这样保留了
“驱动提供能力、VFS 提供文件、syscall 翻译 Linux ABI、MM 管页表”的边界。

## 构建和运行

```bash
# 首次使用先安装 RISC-V musl 工具链
make -C user setup ARCH=rv

# 构建包含静态 Nano-X 的 EXT4 rootfs
make -C user image ARCH=rv PROFILE=nanox

cd os
make shell \
  ARCH=rv PROFILE=pre \
  SDCARD=../user/build/images/wateros-rv-nanox.ext4 \
  EXTRA_FEATURES=user-graphics
```

`user-graphics` 会令 Make 自动打开 QEMU 图形窗口并挂载 VirtIO GPU、keyboard 和
tablet。进入串口 shell 后执行：

```sh
start-nanox
```

脚本检查三个设备节点，清除遗留 socket，启动 server，等待 `/tmp/.nano-X`，再启动
launcher、clock 和 eyes。退出或失败时会清理客户端、server 和 socket。

## 内核实现

### fbdev

`/dev/fb0` 支持：

- `read/write/lseek`，用于诊断；写入不会隐式刷新。
- `FBIOGET_FSCREENINFO`、`FBIOGET_VSCREENINFO`。
- `FBIOPUT_VSCREENINFO`，只接受当前 32-bit BGRA8888 模式。
- `FBIOPAN_DISPLAY`，校验零偏移后执行一次 VirtIO GPU flush。
- `mmap(MAP_SHARED, PROT_READ|PROT_WRITE)`，直接共享 GPU DMA 页。

fbdev 的 64 位结构大小在 Rust 编译期断言为 80/160 字节。ioctl 用户指针只在
syscall 层复制，显示驱动锁不会跨用户复制和页表更新。

### 设备 mmap

MM 公共 API 使用 `MmapKind::Device` 与带生命周期 lease 的 `DeviceMapping`。设备 VMA：

- 不分配、不复制、不回收 framebuffer 物理页；
- fork 后共享同一批页并克隆 lease，不进入 COW；
- munmap、地址空间销毁和 MAP_FIXED 替换只清 PTE；
- 禁止 MAP_PRIVATE、PROT_EXEC、越界、设备 mremap；
- 页表变化继续走 active CPU mask 与 TLB shootdown。

### evdev

驱动探测后建立稳定 `eventN` 索引，键盘和指针别名按设备类型选择，不依赖注册顺序。
低优先级 worker 批量读取原始 VirtIO 事件并广播给每个打开者：

- 事件是 Linux 64 位 24-byte `input_event`，时间戳来自单调 scheduler tick；
- 每个 open description 有独立 256 项队列；溢出时先发 `SYN_DROPPED`；
- read 只交付完整事件，支持阻塞、`O_NONBLOCK/EAGAIN`、信号中断；
- poll/select 只在客户端队列非空时可读；
- 支持 `EVIOCGVERSION/ID/NAME/BIT/ABS`。

### 用户包

`user/vendor/microwindows` 是提交
`2108675308cf69a5c1c54b483e29e3c039f319be` 的干净导出。WaterOS 改动只在
`user/packages/microwindows/patches`：

- Nano-X 每轮 server 事件循环通过 `FBIOPAN_DISPLAY` 全屏刷新；不能只依赖 dirty
  回调，因为部分上游 blit 路径会直接写 mmap framebuffer 而不调用 `Update`；
- keyboard/pointer evdev 后端，tablet 坐标缩放到当前屏幕；
- 静态 musl、内置字体，关闭 X11/SDL/NX11/SysV SHM 和外部图像/字体库；
- 不构建依赖 PTY 的 nxterm。

构建器验证所有安装 ELF 架构正确且无 `PT_INTERP`/`NEEDED`。

## 互斥与兼容

- `user-graphics` 与内核 `gui` 不能同时启用，防止 framebuffer/input 双重消费。
- 不启用 `user-graphics` 时不创建 fbdev/evdev worker，不增加默认比赛构建开销。
- LoongArch 已接通同一内核 ABI并通过 feature 编译检查；Microwindows package 首期只
  声明 RISC-V，待 LoongArch musl 工具链可用后再开放用户态验收。

## 排查

- `missing /dev/fb0`：确认 `EXTRA_FEATURES=user-graphics` 且 QEMU 参数包含
  `virtio-gpu-device`。
- 缺少 keyboard/pointer：确认图形模式挂载了 VirtIO keyboard/tablet，并查看启动时
  `[user-graphics]` 日志。
- 只有鼠标、没有窗口：检查 `ps` 中客户端是否一直停在 `GrOpen()`。Nano-X 依赖
  AF_UNIX 监听 socket 在待连接队列非空时向 `select()` 报告可读；WaterOS 已在
  `unix_sock.rs` 中实现该语义，回归时应确认 `nxlaunch`、`nxclock` 和 `nxeyes`
  启动后画面出现，而不只是进程仍然存活。
- 完全黑屏：直接运行 `nano-X` 检查 fbdev open/ioctl/mmap 错误。服务端启动后至少应
  显示软件鼠标；驱动只在 `FBIOPAN_DISPLAY` 时把 DMA framebuffer 提交给 VirtIO GPU。
- `nano-X did not create /tmp/.nano-X`：直接执行 `nano-X` 查看 open/ioctl/mmap/socket
  错误，并确认 `/tmp` 已挂载。
- 鼠标无响应：用 `cat /dev/input/pointer0` 检查是否有 24 字节事件流；不要把二进制
  输出长期写到串口。
- server 重启失败：删除 `/tmp/.nano-X`，`start-nanox` 会自动完成这一步。
