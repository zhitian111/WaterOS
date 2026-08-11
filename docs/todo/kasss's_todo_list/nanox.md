
# WaterOS 支持 Nano-X（Microwindows）方案说明

## 1. 目标与结论

目标：让现成的 **Nano-X / Microwindows** 窗口系统在 WaterOS 上以**用户态**方式跑起来——`nano-X` 服务端进程 + 若干客户端应用（`demo`、`nterm` 等），并通过 UNIX 域套接字通信。

结论：**核心工作量在内核侧**，把显示与输入按 Linux 用户态可见的 ABI 暴露出来；Nano-X 侧基本不改源码，按 Linux 引擎交叉编译即可。

- 必需：`/dev/fb0`（fbdev ABI + `mmap`）+ `sys_mmap` 新增 `MmapKind::Framebuffer`。
- 输入：优先**自写 Nano-X 鼠标/键盘驱动**（避开 `/dev/input/mice` ImPS/2 与 `/dev/tty0` 控制台两套重 ABI）。
- 通信：AF_UNIX 已有，`/tmp` 已挂 tmpfs，基本零成本。

---

## 2. Nano-X 运行模型（它到底访问什么）

```
┌────────────────────────── 用户态 ──────────────────────────┐
│  客户端应用 (libnano-X)          nano-X 服务端             │
│    │  AF_UNIX socket              │ 屏幕引擎 scr_fb.c     │
│    │  (默认 /tmp/.nano-X)         │ 鼠标 / 键盘驱动        │
└────┼──────────────────────────────┼───────────────────────┘
     │ socket                       │ open/ioctl/mmap/read
┌────┴──────────────────────────────┴───────────────────────┐
│  内核: /dev/fb0   /dev/input/...   /dev/tty*   /dev/null  │
└───────────────────────────────────────────────────────────┘
```

来自 `/home/kasss/microwindows/src` 的具体依赖（已核实源码）：


| 组件            | 文件                        | 访问的接口                                                                                                        |
| ----------------- | ----------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| 屏幕引擎        | `drivers/scr_fb.c`          | `open("/dev/fb0", O_RDWR)`；`ioctl(FBIOGET_FSCREENINFO)`、`ioctl(FBIOGET_VSCREENINFO)`；`mmap(fd, size, PROT_READ |
| 鼠标（现成）    | `drivers/mou_devmice.c`     | `open("/dev/input/mice", O_RDWR                                                                                   |
| 键盘（现成）    | `drivers/kbd_tty.c`         | `open("/dev/tty0                                                                                                  |
| 鼠标/键盘（空） | `mou_null.c` / `kbd_null.c` | 无设备依赖                                                                                                        |
| 客户端通信      | `nanox/`                    | AF_UNIX**pathname** socket（默认 `/tmp/.nano-X`，环境变量 `NANOX_SOCKET` 可覆盖）                                 |

> 注意：本版本没有现成的 evdev 键盘驱动，现成键盘走的是 **Linux tty 控制台 + scancode**；鼠标走的是 `/dev/input/mice` 的 ImPS/2 协议。这两条"现成"路径的内核模拟工作量都不小，因此输入建议走自写驱动路线（见 §4.3）。

---

## 3. WaterOS 现状盘点

### 已具备（可直接复用）


| 能力                                                                  | 位置                                                                                 |
| ----------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| 显示驱动（virtio-gpu，`DisplayDevice` / `FramebufferInfo`）           | `os/components/wateros-driver/driver-display/`                                       |
| 输入驱动（virtio-keyboard/tablet，`RawInputEvent` 兼容 evdev 三元组） | `os/components/wateros-driver/driver-input/`                                         |
| 内核侧 GUI（可复用其刷新任务模式）                                    | `os/components/wateros-gui/`                                                         |
| AF_UNIX（pathname / abstract、stream / dgram）                        | `wateros-syscall/.../impl-kernel/src/unix_sock.rs`                                   |
| TCP/UDP socket                                                        | `wateros-syscall/.../impl-kernel/src/sys/net/`                                       |
| `ioctl`（按 fd 分发 + RTC/TTY fallback）                              | `.../sys/misc/ioctl.rs`                                                              |
| `poll` / `select` / `epoll`                                           | `.../sys/poll/`                                                                      |
| `mmap`（匿名 + file-backed）                                          | `.../sys/mem/mmap.rs`                                                                |
| 字符设备注册 + devfs（`/dev/null`、`/dev/zero`、`/dev/misc/rtc`）     | `wateros-driver/driver-character/`、`wateros-vfs/impl-fd-session/char_dev_handle.rs` |
| `/dev`、`/tmp`(tmpfs)、`/dev/shm` 已建                                | `os/src/user_bringup_root_layout.rs`                                                 |

### 缺口（需要补）

- **A. `/dev/fb0`（fbdev ABI + mmap）** —— 最大缺口，Nano-X 屏幕引擎的入口。
- **B. `MmapKind::Framebuffer`** —— `sys_mmap` 目前只有匿名和 file-backed，无法映射固定物理帧。
- **C. 输入的用户态通道**（鼠标 / 键盘）。
- **D.（可选）`/dev/tty0` 控制台 + termios** —— 仅当走现成 `kbd_tty` 时。
- **E. 显示呈现模型** —— virtio-gpu 不会自动扫描显存，必须解决"用户态画完 → 上屏"。
- **F. 交叉编译 + 打包 + bring-up**。

---

## 4. 分模块要补什么

### 4.1 `/dev/fb0`（fbdev ABI）—— 核心

沿用现有字符设备 + devfs 模式（`CharDevHandle`），新增一个 framebuffer 设备，内部持有 `driver::display::DisplayDevice`。

需要实现的 Linux ABI：

- **`open`/`close`/`read`/`write`/`lseek`/`ioctl`/`mmap`**。
- **`ioctl`**（请求号见 `linux/fb.h`）：
  - `FBIOGET_FSCREENINFO` (0x4602) → `struct fb_fix_screeninfo`
  - `FBIOGET_VSCREENINFO` (0x4600) → `struct fb_var_screeninfo`
  - `FBIOPUT_VSCREENINFO` (0x4601)、`FBIOPAN_DISPLAY` (0x4606)：可先返回 0
  - `FBIOGETCMAP`/`FBIOPUTCMAP`：仅 8bit 调色板需要；32bpp 可跳过
- **结构体布局必须严格按 Linux uapi 头文件**（`repr(C)`），尤其注意 **64 位 `unsigned long` 字段的对齐**（`smem_start`、`mmio_start`）。最稳妥做法：直接从目标工具链的 `linux/fb.h` 拷结构体，不要手写字节偏移。
- **像素格式**：`fb_var_screeninfo.bits_per_pixel=32`，`visual=FB_VISUAL_TRUECOLOR`，注意 **BGRA 内存序 vs RGBX 视图**的字节序差异。
- **`mmap(fd, size, PROT_READ|WRITE, MAP_SHARED, offset=0)`** → 由 §4.2 的 `MmapKind::Framebuffer` 支撑。

### 4.2 `MmapKind::Framebuffer`（`sys_mmap` 扩展）

位置：`wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/mmap.rs`。

- 新增 `MmapKind::Framebuffer`：`DemandPageLoader` 对每个页面**返回同一个物理帧**（共享、非 COW、用户可读写）。
- 需要 mm-impl 提供"把指定物理页映射进用户页表"的入口（参考现有 `map_range_from_backing` / `map_identity_range_user`）。
- `MAP_SHARED`：写入对内核与其它映射方可见（即直接写 framebuffer 物理内存）。

### 4.3 输入（两条路线）

**路线 1（推荐，ABI 工作量最小）：自写 Nano-X 鼠标/键盘驱动**

- 内核侧只需暴露一个简单输入节点（如 `/dev/misc/wos-input`），`read` 返回 `driver-input` 的 `RawInputEvent`（键盘键码 / 平板绝对坐标）。
- Nano-X 侧新增 `drivers/mou_wateros.c`（绝对坐标指针）+ `drivers/kbd_wateros.c`，在 `src/config` 里把 `NAMOUSE` / `NAKBD` 指到它们。
- 优点：避开 ImPS/2 合成和 tty 控制台两套重 ABI，可靠性最高。

**路线 2（最"现成"，内核模拟 Linux ABI，工作量大）**

- `/dev/input/mice`：把 tablet 绝对坐标**合成 ImPS/2 相对位移包**（3 字节：buttons+flags, dx, dy），`O_NONBLOCK` 读。
- `/dev/tty0`：Linux 控制台 + scancode 表 + `TCGETS/TCSETS` raw mode。
- 优点：Nano-X 现成驱动不动；缺点：内核侧两套协议模拟都不小。

**折中**：MVP 阶段鼠标用 `mou_null`（无鼠标）+ 键盘走路线 1 的自写驱动，先把画面和交互链路打通。

### 4.4 显示呈现模型（virtio-gpu 的关键差异，必须解决）

fbdev **没有**标准 "present / flush" 接口，Nano-X 画完只写 mmap 内存，不会主动通知内核；而 virtio-gpu 必须 `flush` 才上屏。三种方案：


| 方案          | 做法                                                                                  | 评价                                      |
| --------------- | --------------------------------------------------------------------------------------- | ------------------------------------------- |
| A（推荐 MVP） | 内核常驻刷新任务按周期`flush_region`（复用 `wateros-gui` 的 `gui_refresh_task` 模式） | 简单、Nano-X 零改动；有固定延迟           |
| B             | 在`FBIOPAN_DISPLAY` / `FBIOPUT_VSCREENINFO` ioctl 里触发 flush                        | 语义自然，但 fbcon 默认不保证每次画完都调 |
| C             | 内核 shadow buffer 对比脏区再 flush                                                   | 延迟最低，代价最高                        |

建议先用 A 打通，再评估 B。

### 4.5 客户端通信

- AF_UNIX **pathname** socket 已支持（`unix_sock.rs`），`/tmp` 已挂 tmpfs，默认路径 `/tmp/.nano-X` 可直接用。
- 若服务端/客户端要跨目录，用 `NANOX_SOCKET` 环境变量。

### 4.6 构建与 bring-up

- **交叉编译**：用 WaterOS 用户态同 ABI 的工具链（riscv64 + musl/glibc）编 Nano-X；`src/config` 打开 `ENGINE=Y`、`NANOX=Y`、`HAVE_FB_SUPPORT=Y`，关闭 `HAVE_PNG/JPEG/TIFF/FREETYPE_SUPPORT`（PNG/Freetype 已关），选择 §4.3 的鼠标/键盘驱动。
- **产物**：`nano-X`、`demo` / `nterm`、PCF 字体 → 放入 sdcard。
- **bring-up**：在 busybox 启动脚本里先后台起 `nano-X`，再起客户端；必要时设 `NANOX_SOCKET`。
- **验证**：`demo` 画出窗口 / `nterm` 可输入 → 全链路（用户态 socket + fbdev mmap + 输入 + 刷新）通过。

---

## 5. 里程碑


| 阶段 | 内容                                                    | 验证                                                  |
| ------ | --------------------------------------------------------- | ------------------------------------------------------- |
| M1   | `/dev/fb0` + `FBIOGET_FSCREENINFO/VSCREENINFO` + `mmap` | 自写 hello：`open("/dev/fb0")` + `mmap` + 画点 + 刷新 |
| M2   | 输入通道（路线 1 自写 kbd/mou，或路线 2 的 mice）       | 用户态程序能读到按键/坐标                             |
| M3   | `nano-X` 服务端出画面（null mouse + 自写键盘即可）      | 服务端无崩溃、窗口上屏                                |
| M4   | `demo` / `nterm` 客户端经 AF_UNIX 连上服务端            | 交互可用                                              |
| M5   | 收尾：刷新方案调优、多客户端、字体                      | 稳定演示                                              |

---

## 6. 风险与注意

1. **fbdev 结构体布局/位宽**必须严格对齐 Linux（64 位 `unsigned long`），直接从 `linux/fb.h` 拷，勿手写偏移。
2. **字节序**：BGRA8888 内存序与 Linux TRUECOLOR 的 RGBX 视图差异要处理好。
3. **virtio-gpu 无自动扫描显存**：呈现模型（§4.4）必须显式解决，否则画面永远空白。
4. **键盘别一开始就走 tty 控制台**（`kbd_tty` 依赖完整 termios + scancode），自写驱动更快更稳。
5. 保持现有内核 `wateros-gui` 独立（feature 开关），`/dev/fb0` 与它互不干扰。
6. 比赛不测 GUI，此项属展示/研究；默认构建不启用，避免影响比赛二进制。
7. 交叉编译选型以 **Nano-X 原生支持 Linux-ELF 目标**为基准，避免引入过多 ABI 适配。
