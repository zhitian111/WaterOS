# WaterOS 五分钟决赛展示方案

> 交付内容：完整展示流程、逐页 PPT 内容、逐字讲稿、素材占位、独立素材 GPT Image 提示词与录屏清单。
>
> 建议标题：**WaterOS：从图形桌面看一个双架构 Rust 内核**
> 核心表达：**让成功运行的用户程序替内核说话，再从屏幕上的功能自顶向下揭示支撑它们的系统能力。**

---

## 一、汇报定调

这五分钟不承担“完整介绍全部内核模块”的任务，也不以比赛测试清单作为主线。它要让评委形成一个清晰、可复述的印象：

> WaterOS 是三名学生用 Rust 从零实现的宏内核；它在 RISC-V64 和 LoongArch64 上共享同一套上层语义，既能承载真实 Unix 开发环境，也能运行完整的图形桌面与交互应用。

整场采用“看见 → 拆开 → 下沉 → 展开 → 验证 → 汇合”的叙事：

1. 先看到 Nano-X、文件管理器和 mGBA；
2. 再说明桌面不是内核直接绘制的画面，而是多个用户进程；
3. 沿一次按键、一次窗口刷新向下进入 IPC、输入与显示；
4. 用 APT、Neovim、GCC 证明这些能力并非只为桌面服务；
5. 展开完整内核与双架构边界；
6. 用比赛测试和 BuildStorm 给系统能力加上可量化的刻度；
7. 回到开场画面，让所有技术路径汇合成 WaterOS。

### 评委应记住的三句话

1. WaterOS 是三人用 Rust 从零实现的双架构宏内核。
2. 屏幕上的桌面、终端、开发工具和游戏都运行在同一套通用内核语义之上。
3. WaterOS 不仅能展示，还能使用和测量：它可以 APT 安装 Neovim、本地 GCC 编译程序，并以约 550 秒完成 BuildStorm；同条件 Linux 为 415 秒。

---

## 二、统一视觉规范

### 1. 画面规格

- 比例：16:9。
- 设计基准：1920×1080。
- 建议主标题：40–46 pt；封面标题：52–60 pt；正文：20–24 pt；数据：40–56 pt。
- 每页只保留一个主要画面和一个主要结论，避免卡片墙。
- 真实截图、视频和 GIF 必须作为视觉中心；AI 生成素材只负责背景、包裹框架和抽象装饰，不生成虚假的 WaterOS 界面。

### 2. 色彩

| 用途 | 建议颜色 |
|---|---|
| 主背景 | `#061426` |
| 次级背景 | `#091D35` |
| 主内容色块 | `#0D2B4B` |
| 次级内容色块 | `#123B61` |
| WaterOS 青色 | `#0891B2` |
| 高亮青色 | `#22D3EE` |
| 架构蓝 | `#3B82F6` |
| 架构紫 | `#8B5CF6` |
| 成功绿色 | `#2DD4A7` |
| 数据橙色 | `#F5A742` |
| 主文字 | `#F4F8FC` |
| 次文字 | `#AFC2D8` |

已有 [`wateros-waves.png`](../user/packages/microwindows/assets/wateros-waves.png) 可以作为视觉母题：保留深蓝底和青色流线，但 PPT 中将流线弱化到边缘，中央留给内容。已有 `docs/assert/branding/wateros-wordmark.svg` 可直接作为标题字标。

### 3. 形状语言

- 使用大面积纯色矩形和少量圆角，圆角半径保持一致；不要使用大量小卡片。
- 素材像被“系统容器”包裹：外层深蓝实体框，内层真实视频或截图，外沿一条 1–2 px 青色描边。
- 阴影只用于把真实素材从背景中抬起，透明度低，不做玻璃拟态。
- 架构层使用连续横向色带，而不是几十个孤立节点。
- 两条架构路径分别使用蓝色和紫色；共同层使用青色。
- 每页右上角可以保留小号 `WaterOS` 字标，但封面和结束页除外。

### 4. 动效

- 页面切换以淡入或 Morph 为主。
- 架构图按“应用 → ABI → 内核 → 驱动 → 双架构”逐层出现。
- 每页最多 2–3 次对象动画，避免演示节奏被动画拖慢。
- 视频默认自动播放、静音、隐藏控制栏；必须准备同构图的静态截图作为故障备用。

### 5. 对 GPT Image 的统一要求

不再让 GPT Image 生成完整页面。PPT 的背景、标题、色块、箭头、标签和素材框全部使用 PowerPoint 原生形状制作；GPT Image 只生成少量**独立、无文字、可裁切的解释性素材**。

这样处理有三个好处：

- 架构关系、对齐和文字完全可控；
- 各页能共享同一种视觉语言，而不会出现七张风格略有差异的“海报”；
- 真实截图和视频仍是画面主体，AI 素材只负责解释抽象机制。

所有提示词均可追加以下统一约束：

> Minimalist flat-vector technical illustration, deep navy and cyan color system, large solid-color geometric forms, crisp edges, restrained depth, no gradients unless extremely subtle, no readable text, no letters, no numbers, no logos, no watermark, no fake terminal UI, no glassmorphism, no glossy 3D rendering, no photorealistic people. Isolated composition with generous spacing, suitable for cropping and combining with editable PowerPoint labels. Transparent background if supported; otherwise use one perfectly uniform #061426 background with no texture.

生成时建议：

1. 每项素材单独生成，不要把多个页面塞进一张图；
2. 同一提示词生成 2–3 个版本，再选择构图最清楚的一张；
3. 如果支持参考图，将最终选定的架构素材作为后续素材的风格参考；
4. 需要准确表达关系的箭头、模块名和数据一律在 PPT 中补画；
5. 不要求模型画“透明发光小字”，也不要求模型复刻 WaterOS 的真实界面。

---

## 三、独立绘制素材提示词库

下面的素材编号会在逐页方案中引用。提示词以英文书写，便于直接交给 GPT Image；每个提示词之后说明 PPT 中需要补充的内容。

### ASSET-A1：用户进程展开图

**用途：** 第 2 页，用来辅助说明“桌面不是内核直接绘制，而是多个用户进程协作”。真实桌面截图仍是主图。

**建议尺寸：** 4:3 或正方形，透明背景。

**提示词：**

> Create an isolated flat-vector exploded-view illustration of a graphical desktop composed of multiple independent user processes. Show one abstract desktop plane at the back, then five clearly separated floating application planes in front of it: a window server plane, a terminal plane, a file-manager plane, a game-emulator plane, and a small shell-process plane. Connect the application planes to the window-server plane with only a few thin cyan communication paths. Each plane must be a simple solid geometric shape with a distinct silhouette, not a realistic application window. Deep navy, cyan, teal and muted blue palette, precise operating-system engineering aesthetic, strong separation between objects, transparent background, no text, no letters, no logos, no terminal content, no recognizable game characters, no watermark.

**PPT 后期补充：** 用原生文字标注 `Nano-X Server`、`nxterm`、`WaterFM`、`mGBA`，并把三条真实窗口连线从截图接到素材对应位置。

### ASSET-A2：一次按键贯穿系统的主链路

**用途：** 第 3 页，是“输入—用户进程—内核—显示”的无文字流程骨架。

**建议尺寸：** 竖向 3:4，透明背景；如果右侧空间较宽，也可生成 4:3 横版。

**提示词：**

> Create an isolated flat-vector end-to-end operating-system pipeline with one unmistakable continuous path and exactly six major visual stations. From top to bottom: a physical input pulse entering an input-device node; an event packet entering a window-server layer; a game-process layer reacting to the event; a kernel-services layer represented by scheduling, IPC and memory tiles; a mapped framebuffer represented by a clean pixel grid with one highlighted dirty rectangle; and a virtual GPU sending the finished frame to a display plane. Group the stations visually into three broad zones: user space, kernel space, and devices, but include no text. Use one bright cyan flow line, large solid navy, blue, teal and violet shapes, very few branches, crisp technical infographic style, transparent background, no labels, no letters, no game artwork, no fake user interface, no watermark.

**PPT 后期补充：** 添加六个节点名称、三条空间分区色带，以及 `VirtIO Input → evdev`、`AF_UNIX / poll`、`/dev/fb0 mmap`、`VirtIO GPU` 等准确标签。动画只沿主线逐段点亮。

### ASSET-A3：完整内核架构骨架（核心素材）

**用途：** 第 5 页的主体。该图只生成视觉骨架，不让模型生成任何模块名称。

**建议尺寸：** 16:9，透明背景；若透明失败，使用完全均匀的 `#061426` 背景。

**提示词：**

> Create a clean 16:9 flat-vector architecture skeleton for a dual-architecture Rust monolithic operating system. The composition must read strictly from top to bottom and contain five visually distinct levels. Level 1: three wide application zones represented by abstract silhouettes for an interactive graphical desktop, a Unix development toolchain, and a heavy build workload. Level 2: one thin continuous compatibility band spanning the full width. Level 3: one broad shared kernel-services band divided only by subtle vertical separators into five areas representing task scheduling, virtual memory, filesystem and cache, IPC and signals, and networking. Level 4: one broad shared device-and-storage band containing abstract motifs for file pages, block storage, network packets, input events, framebuffer pixels and virtual devices. Level 5: the shared foundation splits cleanly into two equal architecture branches: the left branch in blue suggests firmware calls, a three-level page-table grid and memory-mapped devices; the right branch in violet suggests control registers, a different page-table grid and a PCI-style device bus. Use broad continuous horizontal layers rather than floating cards. Use one centered downward flow through the shared layers, then a clean symmetric split only at the bottom. Deep navy base, cyan shared layers, blue left foundation, violet right foundation, sparse connectors, large solid geometric areas, precise systems-engineering infographic, generous room inside every layer for PowerPoint labels, no text, no letters, no numbers, no logos, no processor emblems, no fake code, no watermark.

**负面补充词：**

> Avoid a card dashboard, avoid dense circuit-board decoration, avoid dozens of tiny boxes, avoid isometric buildings, avoid a glowing sci-fi HUD, avoid random arrows, avoid symmetrical duplication above the bottom split.

**PPT 后期补充：**

- 顶层嵌入三张真实缩略图：桌面/mGBA、Neovim/GCC、BuildStorm；
- 第二层写 `Linux generic64 ABI`；
- 第三层写 `Task & Scheduler / MM / VFS & FS / IPC & Signal / Network`；
- 第四层写 `PageCache / ext4 / Block / Input / Display / VirtIO`；
- 底部左侧写 `RISC-V64：OpenSBI · Sv39 · VirtIO-MMIO`；
- 底部右侧写 `LoongArch64：CSR · LA Page Table · VirtIO-PCI`；
- 所有箭头都用 PPT 重画，保证方向和动画顺序准确。

如果 GPT Image 对层级关系控制仍不稳定，不要继续反复抽卡。直接在 PPT 中用五条横向色带搭出架构图，只使用下方的 `ASSET-A4` 作为底部装饰。

### ASSET-A4：双架构底座

**用途：** 第 5 页底部，也可用于结束页左右两条架构色带。

**建议尺寸：** 3:1 超宽横图，透明背景。

**提示词：**

> Create an isolated ultra-wide flat-vector foundation illustration for one operating-system core supporting two processor architectures. A single cyan shared platform enters from the top center and splits only once into two equal low-profile foundations. The left foundation is blue and contains three abstract motifs: a firmware-call ring, a three-level page-table grid, and several memory-mapped device nodes. The right foundation is violet and contains three abstract motifs: a control-register bank, a different hierarchical page-table grid, and a clean PCI-style lane with device endpoints. Keep both branches visually balanced but not identical. Large solid shapes, sparse thin connectors, deep navy details, transparent background, no text, no letters, no processor logos, no circuit-board clutter, no watermark.

**PPT 后期补充：** 分别添加 RISC-V64 与 LoongArch64 的真实文字标签；不要加入未经验证的性能或兼容性暗示。

### ASSET-A5：PageCache 与文件路径

**用途：** 可放在第 4 页右下角，或作为答辩附录中 VFS / PageCache / ext4 页的主视觉。

**建议尺寸：** 16:9 横向条带，透明背景。

**提示词：**

> Create an isolated horizontal flat-vector data path for a Unix file operation inside an operating system. From left to right, show an application request entering a virtual-filesystem gateway, passing through a set of reusable memory-page tiles, then reaching an ext4-like tree-and-block layer and finally a virtual block device. Show one clean fast path where an already-cached page returns directly to the application, and one secondary writeback path where a highlighted dirty page moves toward storage. The cache must be the visual center and largest element. Use solid cyan, teal, blue and muted orange accents on transparent background, crisp edges, very few arrows, no text, no letters, no file names, no disk brand, no watermark.

**PPT 后期补充：** 用 PPT 标注 `read / mmap`、`VFS`、`PageCache`、`ext4`、`VirtIO Block`、`cache hit`、`dirty / writeback`。主讲五分钟内只在需要时作为轻量辅助，不展开算法细节。

### ASSET-A6：安装—编辑—编译—运行

**用途：** 第 4 页视频下方的四阶段视觉标记。视频和真实终端内容不能由 AI 生成。

**建议尺寸：** 4:1 超宽横图，透明背景。

**提示词：**

> Create an isolated ultra-wide four-stage flat-vector software-development workflow. Stage one is an abstract package entering the system; stage two is a clean editor cursor modifying a small code-shaped sheet with no characters; stage three is a compiler mechanism transforming several source tiles into one binary tile; stage four is the binary tile launching as a small successful process pulse. Connect all four stages with one continuous cyan line. Use large solid geometric symbols, deep navy, cyan and blue palette, transparent background, consistent visual weight, no text, no letters, no software logos, no terminal interface, no watermark.

**PPT 后期补充：** 在四个阶段下分别写 `APT`、`Neovim`、`GCC`、`运行 ELF`，随真实视频依次高亮。

### ASSET-A7：测试覆盖的验证带

**用途：** 第 6 页底部，作为测试项目文字的背景装饰。BuildStorm 终端和数字必须使用真实素材与 PPT 文字。

**建议尺寸：** 8:1 超宽横图，透明背景。

**提示词：**

> Create an isolated very wide and shallow flat-vector validation ribbon for a systems benchmark slide. Show a continuous sequence of nine small but clearly separated verification pulses traveling through one shared horizontal rail, ending in a solid completion marker. The ribbon should suggest broad test coverage without resembling a progress bar or a decorative circuit board. Deep navy rail, restrained cyan and teal pulses, one muted orange completion accent, transparent background, no text, no numbers, no checkmark icons, no logos, no watermark.

**PPT 后期补充：** 在验证带上方加入真实测试名称，并用 PPT 原生柱形或大数字呈现 `415 s`、`约 550 s`、`约 1.33×`。

### ASSET-A8：边缘流线与双架构汇合装饰

**用途：** 封面与结束页的轻量装饰。若已有 `wateros-waves.png` 与整体风格匹配，优先复用现有素材，不必重新生成。

**建议尺寸：** 16:9，透明背景，只占画面边缘。

**提示词：**

> Create a restrained transparent edge-overlay for a 16:9 operating-system presentation. Keep the central 70 percent completely empty. From the lower left, one broad blue solid ribbon enters; from the lower right, one broad violet solid ribbon enters; both merge near the lower center into a single cyan water-like data flow that continues subtly along the outer edges. Minimal flat vector, no particles, no glow cloud, no texture, no text, no logos, no watermark. The overlay must frame real screenshots without covering them.

**PPT 后期补充：** 封面叠加真实 WaterOS wordmark、学校标志和视频框；结束页叠加第一页视频的真实最终静帧。

---

## 四、五分钟逐页流程

总页数建议为 **7 页**，总时长控制在 **4分45秒至4分55秒**，预留 5–15 秒给切页、视频启动或现场停顿。

---

## 第 1 页：桌面之下，没有 Linux

**时间：0:00–0:30**

**本页任务：先建立视觉记忆，不急于解释技术。**

### 页面标题

**WaterOS：从图形桌面看一个双架构 Rust 内核**

副标题只保留一行：

> 三人从零实现 · RISC-V64 / LoongArch64 · 部分兼容 Linux generic64 ABI

### 版式

- 左侧约 38%：标题、副标题、团队成员与学校信息。
- 右侧约 58%：一个大型横向“系统窗口”，内部播放开场视频。
- 视频容器使用深蓝实体外框、青色细描边；不要模拟笔记本电脑或手机样机。
- 左下角放山东大学标志，使用已有真实素材；右上角或标题上方放 WaterOS 字标。

### 素材占位

`[VIDEO-01｜右侧大型窗口｜约 22–25 秒｜16:9 或 4:3 居中裁切]`

视频内容建议连续完成：

1. WaterOS Nano-X 桌面静置 2 秒；
2. 打开文件管理器；
3. 双击 ROM；
4. mGBA 窗口出现；
5. 实际按键并看到画面变化；
6. 最后停在“桌面 + mGBA + 终端”同屏画面。

视频不要出现宿主桌面、QEMU 菜单栏和剪辑软件界面。允许在左下角放一个很小的真实标识：`WaterOS / QEMU`。

### 页面可见文字

除标题和副标题外，不再添加功能列表。

### 逐字讲稿

> 各位评委老师好，我们是 WaterOS 团队。屏幕上运行的是 WaterOS 上的 Nano-X 桌面、文件管理器和 mGBA，它们的下面没有 Linux。WaterOS 是我们三个人用 Rust 从零实现的宏内核，目前运行于 RISC-V64 和 LoongArch64，并部分兼容 Linux generic64 ABI。今天我们想从这张桌面出发，自顶向下介绍它下面真正工作的系统。

### 演示动作

- 开口时视频自动播放。
- 说到“它们的下面没有 Linux”时，视频停在 mGBA 正在运行的一帧，或缓慢循环最后 4 秒。

### 本页素材调用

- 页面背景和视频框直接使用 PPT 纯色块绘制；
- 边缘装饰优先复用现有 `wateros-waves.png`，不匹配时再生成 `ASSET-A8`；
- 不生成任何虚构桌面或整页封面图。

---

## 第 2 页：这不是一张内核画出来的桌面

**时间：0:30–1:05**

**本页任务：把“好看的桌面”拆成真实用户进程和通用 Unix 接口。**

### 页面标题

**桌面上的每个窗口，都是一个真实用户进程**

### 版式

- 左侧 55%：真实 Nano-X 桌面截图，保留 mGBA、文件管理器和 nxterm 三个窗口。
- 右侧 40%：三个上下排列的纯色色块，只写：
  - `Nano-X Server / Window Manager`
  - `WaterFM / nxterm / mGBA`
  - `独立进程 · 通用接口 · 无应用特判`
- 使用三条细线从真实截图中的窗口连到右侧文字块。
- 页面底部以一条青色横线承接下一页，横线上只放：`AF_UNIX · UNIX98 PTY · fork / exec · VFS`。

### 素材占位

`[IMAGE-02A｜左侧主图｜真实 Nano-X 桌面截图｜建议 1440×900 以上]`

`[OPTIONAL-GIF-02B｜主图内部或右下角｜5–7 秒循环｜文件管理器启动应用]`

如果动态图会分散注意，第二页只用静态图，把动态留在第一页和第四页。

### 页面可见文字

主结论：

> **不是内核 GUI，而是运行在 WaterOS 用户空间中的多个程序。**

页脚小字：

> Nano-X 与 mGBA 主体来自开源项目；WaterOS 提供构建适配、用户态前端及其所需的通用内核语义。

### 逐字讲稿

> 这并不是内核直接绘制的一张演示画面。Nano-X server、窗口管理器、终端、文件管理器和 mGBA 分别作为用户进程运行。客户端通过 Unix 域套接字连接 Nano-X，图形终端通过 UNIX98 PTY 连接 shell，文件管理器则通过 fork 和 exec 启动应用。Nano-X 和 mGBA 主体来自开源项目；我们完成的是它们在 WaterOS 上的适配，以及背后所需的通用内核语义，内核中没有针对应用名称的特殊路径。

### 演示动作

- 第一次点击：截图出现。
- 第二次点击：右侧三个色块和连线出现。
- 说完“没有特殊路径”后切到下一页。

### 本页素材调用

- 左侧必须使用真实 Nano-X 截图；
- 右侧可使用 `ASSET-A1` 的用户进程展开图，也可以直接用 PPT 色块和连线完成；
- 模块名称、进程关系和页脚接口名称全部使用 PPT 原生文字。

---

## 第 3 页：一次按键，穿过整个系统

**时间：1:05–1:55**

**本页任务：用一个可理解的动作展示输入、IPC、调度和图形输出的完整链路。**

### 页面标题

**从一次按键到一帧画面，路径贯穿用户态、内核与设备**

### 版式

- 左侧 30%：mGBA 实际运行视频或 GIF，画面中演示一次明显按键操作。
- 右侧 65%：一条从上到下的“单主线”流程，不要画成复杂网络：

```text
键盘 / 鼠标
    ↓
VirtIO Input → evdev
    ↓
Nano-X 事件分发 → mGBA
    ↓
AF_UNIX / 调度 / 内存
    ↓
/dev/fb0 mmap + 脏矩形 ioctl
    ↓
VirtIO GPU → 屏幕
```

- “用户空间”“WaterOS 内核”“设备”用三块连续背景色带区分。
- 当前讲到的节点变为高亮青色，其余保持低饱和。

### 素材占位

`[VIDEO/GIF-03A｜左侧｜8–10 秒循环｜按键后角色或菜单明显移动]`

`[IMAGE-03B｜备用静帧｜视频无法播放时使用，配两帧前后对比]`

### 页面可见文字

主结论只保留一句：

> **一个“能操作的游戏窗口”，同时验证输入、IPC、调度、内存映射和显示。**

辅助标签：`用户空间`、`WaterOS 内核`、`设备`。

### 逐字讲稿

> 当我们在 mGBA 中按下一个按键时，输入首先由 VirtIO 设备进入 WaterOS，通过 evdev 交给 Nano-X，再分发给 mGBA。mGBA 生成新画面后，Nano-X 合成发生变化的区域，通过映射的 framebuffer 和脏矩形 ioctl 提交，最终由 VirtIO GPU 刷新到屏幕。与此同时，窗口通信依赖 AF_UNIX，事件等待依赖 poll 和调度，程序及 ROM 的读取还会进入 VFS、PageCache 和 ext4。一个看似简单的按键，实际贯穿了 WaterOS 的大部分主干。

### 演示动作

- 流程分三次出现：用户空间 → 内核 → 设备。
- 视频循环播放，不必随讲稿逐帧同步。
- 最后将整条链同时点亮，形成“主干贯通”的视觉结论。

### 本页素材调用

- 左侧使用真实 mGBA 视频或 GIF；
- 右侧使用 `ASSET-A2` 作为无文字主链路骨架；
- 三个空间色带、技术名称、箭头和高亮动画在 PPT 中完成。

---

## 第 4 页：另一个窗口里，它也是开发环境

**时间：1:55–2:40**

**本页任务：证明 WaterOS 并非只为图形演示补接口，而是能够承载通用 Unix 软件。**

### 页面标题

**同一套内核，也能安装、编辑、编译并运行软件**

### 版式

- 页面中央放一条宽大的横向视频带，占页面约 70% 宽、48% 高。
- 视频带下方放四个步骤，采用连续的纯色块而非四张独立卡片：

```text
APT 安装 Neovim  →  Neovim 编辑 C 代码  →  GCC 本地编译  →  运行 Hello World
```

- 右下角用一行较小的底层能力：

`网络 · PTY · shell · fork/exec/wait · VFS/ext4 · mmap · ELF`

### 素材占位

`[VIDEO-04A｜中央横向视频带｜18–22 秒，可加速剪辑]`

建议剪辑为四段连续证据：

1. `apt` 已成功安装 Neovim 的结果页，不必完整展示下载过程；
2. 在 Neovim 中把字符串改为 `Hello from WaterOS`；
3. 执行 `gcc hello.c -o hello`；
4. 执行 `./hello` 并输出修改后的字符串。

`[IMAGE-04B｜静态备用｜四张等高终端截图拼成连续胶片]`

### 页面可见文字

主结论：

> **图形应用和开发工具使用的是同一套通用内核机制。**

避免使用“自举”或“self-hosting”；准确表述为“在 WaterOS 中本地编译并运行用户程序”。

### 逐字讲稿

> WaterOS 并不是只为几个图形程序补齐接口。我们还在系统中使用 APT 安装 Neovim，在 Neovim 中编写 C 文件，再由 GCC 本地编译并立即运行，输出 Hello World。这条路径与 mGBA 完全不同：它会经过网络、PTY、shell、进程创建、大量目录和文件操作、动态库映射以及 ELF 装载。两种应用能够同时成立，说明 WaterOS 提供的是通用 Unix 环境，而不是一组面向演示程序的兼容补丁。

### 演示动作

- 视频自动播放。
- 四个步骤随视频片段依次高亮。
- 输出 `Hello from WaterOS` 时停留 1–2 秒，再进入架构页。

### 本页素材调用

- 中央必须使用真实 APT、Neovim、GCC 与运行结果视频；
- 视频下方可裁切使用 `ASSET-A6`；
- 若希望暗示文件路径，可在右下角小面积使用 `ASSET-A5`，但不要与主视频争夺注意力。

---

## 第 5 页：屏幕之下，是一套双架构内核

**时间：2:40–3:35**

**本页任务：把前面看见的应用汇合到完整架构，并明确两种体系结构的共享边界。**

### 页面标题

**应用路径向下汇合，平台差异留在内核底层**

### 版式

采用“宽层级 + 底部分叉”的单幅架构图，整页只画一个构图：

```text
Nano-X / mGBA / APT / Neovim / GCC / BuildStorm
                         ↓
             Linux generic64 ABI
                         ↓
 Task & Scheduler | MM | VFS & FS | IPC & Signal | Network
                         ↓
 PageCache | ext4 | Block | Input | Display | VirtIO
                    ↙                    ↘
 RISC-V64                                LoongArch64
 OpenSBI · Sv39 · VirtIO-MMIO            LA Page Table · CSR · VirtIO-PCI
```

- 上方应用层放前四页的真实小缩略图，而不是应用 Logo。
- 中间三层使用连续大色带；不要把每个组件画成悬浮卡片。
- 底部从共同驱动层分成蓝色与紫色两条平台路径。
- 左下角可放极小的 RISC-V 字样，右下角放 LoongArch64；使用 PowerPoint 真文字。

### 素材占位

`[IMAGE-05A｜应用层缩略图1｜桌面/mGBA真实截帧]`

`[IMAGE-05B｜应用层缩略图2｜Neovim/GCC真实截帧]`

`[OPTIONAL-VIDEO-05C｜底部双架构并列｜两边同时启动同一桌面的短片]`

双架构桌面并列素材如果尚未稳定录制，可以不用视频，以两张真实启动截图替代。不要为了视觉效果暗示未经验证的“完全一致性能”。

### 页面可见文字

建议只保留层名和关键技术词，不放段落：

- `真实用户程序`
- `Linux generic64 ABI`
- `Task / MM / VFS / IPC / Signal / Network`
- `FS / Cache / VirtIO / Input / Display`
- `RISC-V64：OpenSBI · Sv39 · MMIO`
- `LoongArch64：CSR · LA Page Table · PCI`

### 逐字讲稿

> 把前面的程序继续向下展开，就得到 WaterOS 的整体结构。上层程序通过 Linux generic64 ABI 使用任务、内存、VFS、IPC、信号和网络；再由 PageCache、ext4 与 VirtIO 驱动访问设备。两种架构真正不同的部分留在底层：RISC-V 使用 OpenSBI、Sv39 和 VirtIO-MMIO，LoongArch 使用自己的特权寄存器、页表与 VirtIO-PCI。进入 task、MM 公共机制、VFS、IPC 和 syscall 后，两条路径重新汇合。因此我们维护的不是两套内核，而是由两种平台共同检验的一套系统边界。

### 演示动作

- 先出现应用缩略图。
- 讲一句，向下出现一层。
- 最后底部左右分叉，再把共同层同时点亮。

### 本页素材调用

- 首选现成架构图 [`wateros-presentation-architecture.svg`](assert/diagrams/wateros-presentation-architecture.svg)，它已经包含五层结构、关键模块和双架构底座；
- 若需要按演示动画逐层出现，可使用同名 `.mmd` 源文件重绘或在 PPT 中覆盖标签与箭头；
- 如果 `ASSET-A3` 的层级不够准确，直接用 PPT 色带重建五层，只保留 `ASSET-A4` 双架构底座；
- 应用层使用前几页的真实截图缩略图，不使用 AI 生成应用界面。

---

## 第 6 页：可见的系统，也经得起综合负载

**时间：3:35–4:30**

**本页任务：用测试广度和 BuildStorm 数据为前面的系统叙事提供可信度，不把共同赛题冒充独特点。**

### 页面标题

**应用展示回答“能做什么”，综合负载回答“做到什么程度”**

### 版式

- 左侧 48%：BuildStorm 真实终端结果截图或 8–10 秒视频。
- 右侧 46%：只放两个大数字和一条比例：

```text
WaterOS     ≈550 s
Linux       415 s
同机同条件约 1.33×
```

- 页面底部以一条窄色带列出测试覆盖，不做大表格：

`basic · BusyBox · Lua · libctest · LTP · libcbench · lmbench · UnixBench · iozone`

- 数据旁用小字注明：`本地同条件测量；正式版本请替换为三轮中位数和完整环境说明。`

### 素材占位

`[VIDEO/IMAGE-06A｜左侧｜BuildStorm完成构建并由QEMU运行产物的真实终端证据]`

画面必须同时尽可能包含：

- 构建成功标志；
- 生成 ELF；
- 嵌套 QEMU 运行成功；
- 实际耗时。

`[DATA-06B｜右侧｜在PPT中绘制数字，不交给GPT Image生成]`

### 页面可见文字

主结论：

> **测试不是 WaterOS 的差异点，而是证明前述系统能力并非演示特例。**

### 逐字讲稿

> 这些画面说明 WaterOS 能做什么，比赛负载则给它一把可量化的尺子。初赛的基础、BusyBox、Lua、libc、LTP 和性能测试，从不同方向覆盖 Linux 与 POSIX 语义；决赛 BuildStorm 则要求在系统内使用 Cargo 和 rustc 构建 ArceOS HelloWorld，再由 QEMU 启动生成的 ELF。所有队伍都会面对这些测试，因此“跑过测试”本身不是我们的差异。它的意义是证明前面看到的桌面和开发环境，建立在能够承受复杂进程、内存与文件负载的同一套内核上。本地同条件下，Linux 为 415 秒，WaterOS 当前约 550 秒，耗时约为 Linux 的 1.33 倍。

### 演示动作

- 左侧终端证据先出现。
- 随后出现两个数字；不要做柱形图夸大 135 秒差距。
- 底部测试覆盖只快速闪现，不逐项朗读。

### 本页素材调用

- 左侧使用真实 BuildStorm 终端证据；
- 右侧数字与比例全部用 PPT 绘制，不生成 AI 柱形图；
- 底部可以使用 `ASSET-A7` 作为测试覆盖验证带，再叠加真实测试名称。

---

## 第 7 页：让程序替内核说话

**时间：4:30–4:55**

**本页任务：回到开场画面，完成情感和技术上的闭环。**

### 页面标题

**让程序替内核说话**

### 页面中心文字

> 从一个按键、一行代码、一次构建，
>
> 看见同一个 WaterOS。

底部小字：

> 三名学生 · Rust 宏内核 · RISC-V64 / LoongArch64

### 版式

- 使用第一页视频的最终静帧作为全页背景，但降低亮度约 35%。
- 画面中央放 WaterOS 字标和两行总结。
- 背景中的桌面、终端和 mGBA 仍应可辨认。
- 左右边缘分别留一条蓝色与紫色实色带，象征双架构；在中央由青色汇合。
- 不单独放“谢谢”大字；如果需要，只在右下角小号写“谢谢各位老师”。

### 素材占位

`[IMAGE-07A｜全页背景｜第一页视频最终静帧：桌面 + mGBA + 终端]`

`[LOGO-07B｜中央｜真实 WaterOS wordmark SVG]`

### 逐字讲稿

> WaterOS 没有发明宏内核、PageCache 或 POSIX，它的许多机制来自我们对 Linux 和 Unix 的学习。我们真正完成的，是由三个人使用 Rust，把这些机制落实为一个跨越 RISC-V 和 LoongArch、能够承载真实开发工具与图形环境的完整系统。从一个按键、一行代码到一次大型构建，屏幕上的每一个结果，都是它下面整个内核共同工作的证明。谢谢各位老师。

### 本页素材调用

- 背景必须使用第一页视频的真实最终静帧；
- 双架构汇合装饰使用 `ASSET-A8`，或裁切复用 `ASSET-A4`；
- WaterOS wordmark、总结语和“谢谢”均使用真实 SVG 与 PPT 文字。

---

## 五、完整连续逐字稿

以下版本可单独打印练习。正常语速下目标为 4分40秒至4分55秒。

> 各位评委老师好，我们是 WaterOS 团队。屏幕上运行的是 WaterOS 上的 Nano-X 桌面、文件管理器和 mGBA，它们的下面没有 Linux。WaterOS 是我们三个人用 Rust 从零实现的宏内核，目前运行于 RISC-V64 和 LoongArch64，并部分兼容 Linux generic64 ABI。今天我们想从这张桌面出发，自顶向下介绍它下面真正工作的系统。
>
> 这并不是内核直接绘制的一张演示画面。Nano-X server、窗口管理器、终端、文件管理器和 mGBA 分别作为用户进程运行。客户端通过 Unix 域套接字连接 Nano-X，图形终端通过 UNIX98 PTY 连接 shell，文件管理器则通过 fork 和 exec 启动应用。Nano-X 和 mGBA 主体来自开源项目；我们完成的是它们在 WaterOS 上的适配，以及背后所需的通用内核语义，内核中没有针对应用名称的特殊路径。
>
> 当我们在 mGBA 中按下一个按键时，输入首先由 VirtIO 设备进入 WaterOS，通过 evdev 交给 Nano-X，再分发给 mGBA。mGBA 生成新画面后，Nano-X 合成发生变化的区域，通过映射的 framebuffer 和脏矩形 ioctl 提交，最终由 VirtIO GPU 刷新到屏幕。与此同时，窗口通信依赖 AF_UNIX，事件等待依赖 poll 和调度，程序及 ROM 的读取还会进入 VFS、PageCache 和 ext4。一个看似简单的按键，实际贯穿了 WaterOS 的大部分主干。
>
> WaterOS 并不是只为几个图形程序补齐接口。我们还在系统中使用 APT 安装 Neovim，在 Neovim 中编写 C 文件，再由 GCC 本地编译并立即运行，输出 Hello World。这条路径与 mGBA 完全不同：它会经过网络、PTY、shell、进程创建、大量目录和文件操作、动态库映射以及 ELF 装载。两种应用能够同时成立，说明 WaterOS 提供的是通用 Unix 环境，而不是一组面向演示程序的兼容补丁。
>
> 把前面的程序继续向下展开，就得到 WaterOS 的整体结构。上层程序通过 Linux generic64 ABI 使用任务、内存、VFS、IPC、信号和网络；再由 PageCache、ext4 与 VirtIO 驱动访问设备。两种架构真正不同的部分留在底层：RISC-V 使用 OpenSBI、Sv39 和 VirtIO-MMIO，LoongArch 使用自己的特权寄存器、页表与 VirtIO-PCI。进入 task、MM 公共机制、VFS、IPC 和 syscall 后，两条路径重新汇合。因此我们维护的不是两套内核，而是由两种平台共同检验的一套系统边界。
>
> 这些画面说明 WaterOS 能做什么，比赛负载则给它一把可量化的尺子。初赛的基础、BusyBox、Lua、libc、LTP 和性能测试，从不同方向覆盖 Linux 与 POSIX 语义；决赛 BuildStorm 则要求在系统内使用 Cargo 和 rustc 构建 ArceOS HelloWorld，再由 QEMU 启动生成的 ELF。所有队伍都会面对这些测试，因此“跑过测试”本身不是我们的差异。它的意义是证明前面看到的桌面和开发环境，建立在能够承受复杂进程、内存与文件负载的同一套内核上。本地同条件下，Linux 为 415 秒，WaterOS 当前约 550 秒，耗时约为 Linux 的 1.33 倍。
>
> WaterOS 没有发明宏内核、PageCache 或 POSIX，它的许多机制来自我们对 Linux 和 Unix 的学习。我们真正完成的，是由三个人使用 Rust，把这些机制落实为一个跨越 RISC-V 和 LoongArch、能够承载真实开发工具与图形环境的完整系统。从一个按键、一行代码到一次大型构建，屏幕上的每一个结果，都是它下面整个内核共同工作的证明。谢谢各位老师。

---

## 六、必须准备的真实素材

### A. 主视频

1. `desktop-mgba-demo.mp4`
   - 20–25 秒；Nano-X → 文件管理器 → 启动 mGBA → 按键操作 → 终端同屏。
   - 用于第 1 页；截取最终帧用于第 7 页。

2. `mgba-input-frame.mp4`
   - 8–10 秒；选一个按键后画面变化明显的场景。
   - 用于第 3 页，可以循环。

3. `apt-nvim-gcc-demo.mp4`
   - 18–22 秒；APT 成功证据 → Neovim 编辑 → GCC 编译 → 执行输出。
   - 用于第 4 页。

4. `buildstorm-result.mp4` 或 `buildstorm-result.png`
   - 8–10 秒或一张清晰终端截图。
   - 同时包含构建成功、产物运行成功和耗时。

5. `dual-arch-desktop.mp4`（可选）
   - RISC-V 与 LoongArch 同屏并列启动相同用户环境。
   - 如果现场版本不够稳定，使用两张独立真实截图，不强行制作同步视频。

### B. 静态素材

- WaterOS 字标：`docs/assert/branding/wateros-wordmark.svg`。
- 蓝色波纹视觉：`user/packages/microwindows/assets/wateros-waves.png`。
- 山东大学标志：`docs/assert/cover.jpg` 中现有素材；制作时最好取得透明背景官方版本。
- Nano-X 桌面高清截图。
- mGBA 运行高清截图。
- APT、Neovim、GCC、Hello World 四张连续截图。
- BuildStorm 完成结果截图。
- 两种架构启动完成截图。

### C. 视频技术建议

- 格式：H.264 MP4，1920×1080，30 fps。
- 保持相同 QEMU 窗口尺寸与缩放比例。
- 关闭鼠标高亮、宿主通知和无关窗口。
- 如果串口字体过小，录制前放大终端字体，不要后期锐化补救。
- 视频全部静音；汇报只保留讲解声音。
- 每段视频都导出一张同构图 poster frame，现场视频失败时可以一键替换。
- 不建议把长视频转 GIF；GIF 色彩和帧率通常更差。只有 5–8 秒的小循环才使用 GIF。

---

## 七、PPT 制作检查表

### 内容准确性

- [ ] 明确说“部分兼容 Linux generic64 ABI”，不说“完全兼容 Linux”。
- [ ] 不把 Nano-X、mGBA、Doom 的上游主体描述为团队原创。
- [ ] 明确团队完成的是 WaterOS 内核、平台/驱动接入、通用语义、构建适配及自有前端/工具。
- [ ] APT、Neovim、GCC 演示使用真实录像；确认 APT 是在 WaterOS 内执行，而非宿主镜像构建工具。
- [ ] 不使用“自举”，除非 WaterOS 已能在自身内部构建完整工具链或内核。
- [ ] BuildStorm 的 415 秒和约 550 秒来自同机同条件；正式展示前换成稳定的三轮中位数。
- [ ] 如果双架构的桌面、BuildStorm或某项能力验证范围不同，在页脚准确标注。

### 视觉与现场

- [ ] 没有使用 AI 生成整页 PPT；AI 素材均为无文字的独立解释性图形。
- [ ] 架构图的标签、箭头和层级由 PPT 原生元素绘制，并已核对语义关系。
- [ ] 所有视频都有静态备用帧。
- [ ] 页面标题在投影环境下保持一行。
- [ ] 正文不小于 18 pt；关键标签不小于 22 pt。
- [ ] 深色背景下，次要文字对比度仍足够。
- [ ] 计时至少完整彩排五次，稳定控制在 4分45秒左右。
- [ ] 设置一个无视频的 PDF 备用版本。
- [ ] 主讲电脑提前测试字体、视频编码、自动播放与页面切换。

---

## 八、建议准备但不进入五分钟的答辩附录

附录建议 7–9 页，评委追问时快速跳转：

1. RISC-V 与 LoongArch 启动、trap、页表、SMP、设备总线对比；
2. `fork / clone / exec / exit / wait` 的任务生命周期；
3. VFS、ext4、PageCache、`mmap` 与写回关系；
4. UNIX98 PTY、termios、poll 与图形终端；
5. AF_UNIX、Nano-X server/client 与 Cargo jobserver；
6. framebuffer、脏矩形 ioctl、VirtIO GPU、evdev 输入路径；
7. BuildStorm 性能演进、测量方法与 Linux baseline；
8. 初赛测试覆盖与两架构验证矩阵；
9. 三名成员分工、代码边界、第三方依赖和许可证。

附录保持同一视觉风格，但允许信息密度略高。主讲部分没有展开的 PageCache、SMP、signal、futex、网络和错误语义，可以在附录中用真实调用链、日志和数据回答追问。

---

## 九、最后的制作原则

1. **不要用“功能很多”证明工作量。** 用一个用户动作穿过多少真实内核层来体现系统规模。
2. **不要用测试清单制造差异。** 测试负责提供可信度，桌面和开发环境负责建立记忆点。
3. **不要让架构图脱离应用。** 每个模块都应能回到前面一个真实画面或动作。
4. **不要让 AI 生成假的产品截图。** WaterOS 的真实界面本身就是最有价值的视觉资产。
5. **整场始终围绕同一句话：让程序替内核说话。**
