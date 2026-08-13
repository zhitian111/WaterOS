# WaterOS mGBA 移植

## 目标

在 RISC-V64 WaterOS 的 Nano-X 用户空间中运行 mGBA，支持原始 `.gba` ROM、画面、
键盘输入和稳定帧率。首个可玩版本**不含音频**：mGBA 产生的 PCM 数据由 frontend 丢弃，
不得阻塞模拟主循环。

当前展示目标：

```text
WaterOS → Nano-X → water-mgba ROM.gba → 240×160 游戏窗口 → keyboard input
```

`water-mgba` 默认以最近邻 `2×` 显示（窗口为 480×320），也可在运行时指定：

```sh
water-mgba /games/pokemon_phatom5.0.gba
water-mgba --scale 3 /games/pokemon_phatom5.0.gba
```

支持的整数缩放范围为 `1..4`；mGBA core 的内部 framebuffer 及 frontend 上传缓冲区
始终保持原生 240×160。frontend 把该缓冲区上传至 Nano-X pixmap，再以
`GrStretchArea` 在服务端完成最近邻整数缩放，避免每帧在客户端创建并传输放大的
480×320 缓冲区。

## 现有基础（已完成）

- `user/` 可以交叉构建和封装静态 RISC-V64 musl 用户程序；
- Nano-X、窗口管理器和 AF_UNIX 客户端协议已经在 WaterOS 上运行；
- `/dev/fb0`、VirtIO GPU、键盘 evdev 和 Nano-X 输入分发已经可用；
- Nano-X Doom 已验证软件绘制、文件读取和交互主路径；
- 内核已接入 `openat`、`mmap`、`poll`、`clock_gettime`、`nanosleep` 等入口。

因此本任务不是移植 Qt/SDL 桌面版，而是将 mGBA core 接到一个很薄的 WaterOS/Nano-X
frontend。Nano-X 和 Doom 的用户包实现是构建、安装和运行时集成的直接参考。

## 边界

首版保留：GBA core、raw ROM、内置 BIOS、软件视频、键盘、`.sav` 文件和帧同步。

首版排除：Qt、SDL、OpenGL、shader、JIT、ZIP/7z ROM、联网、录像、调试器 UI、音频、
游戏库和状态管理 UI。

当前仓库包含用户明确提供的压缩回归测试 ROM：
`user/packages/mgba/roms/pokemon_phatom5.0.gba.xz`。mGBA package 在镜像构建时解压它，
安装为 `/games/pokemon_phatom5.0.gba`；压缩包参与 package cache key，因而镜像可复现。
其它 ROM 仍应由运行者本地提供，且不应绕过 package 构建流程手工写入发行镜像。

## 图形文件管理器

`waterfm` 是独立 Nano-X 用户包，提供目录浏览、进入/返回上级目录、启动普通可执行文件、
启动 `.gba` 文件、删除、重命名和新建目录。文本文件双击暂不处理：图形终端依赖尚未实现的
PTY/devpts，不能伪装成可用的 `nano`/`vi` 集成。

## 目录约定

预期新增用户包：

```text
user/packages/mgba/
├── package.toml
├── build.py
├── roms/                    # 已纳入镜像构建的回归测试 ROM
├── patches/
├── wateros/                 # WaterOS 专用 frontend，不污染 core
│   ├── main.c
│   ├── video_nanox.c
│   ├── input_nanox.c
│   ├── time_wateros.c
│   └── deterministic.c
└── PORTING.md
```

上游代码固定在 `user/vendor/mgba/`；构建器复制后再应用补丁，禁止在 `vendor/` 直接修改。

## 里程碑

详见 [plan.md](plan.md)。当前处于 M0：锁定并获取上游源码。

## 验收顺序

1. Linux x86_64 / riscv64：`--frames 1000 --no-video --no-audio` 输出可比较 hash；
2. WaterOS：同一 ROM、同一帧数得到相同 CPU/RAM/VRAM/framebuffer hash；
3. Nano-X：显示正确、按键按下和抬起均生效；
4. 采用 `CLOCK_MONOTONIC` 绝对 deadline 后维持约 59.7 FPS；
5. 保存/重启和长时运行验证。

## 风险与原则

- 优先补通用 POSIX 语义，禁止按可执行文件名为 mGBA 加内核特例；
- `clock_gettime`、`nanosleep` 的实际精度和 wakeup 延迟必须测量，不能只依据 syscall 存在；
- 先完成无视频 deterministic mode，再接窗口，避免混淆 core 与图形问题；
- 对照顺序是 x86_64 Linux → riscv64 Linux → riscv64 WaterOS；
- 涉及存档写入时用镜像副本/overlay，并验证 close/fsync 后的持久性。
