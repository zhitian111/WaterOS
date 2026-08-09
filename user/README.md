# WaterOS 用户空间与 EXT4 镜像工程

`user/` 是 WaterOS 自己维护的用户空间构建工程，不再是 Git 子模块。它负责将固定版本的
BusyBox 和若干可组合 package 构建为双架构静态用户空间，再生成无分区表的 EXT4 rootfs，
或者把这些文件安全地叠加到比赛镜像的副本中。

它只负责构建期组合，不是运行时包管理器，也不会参与根目录 `make all` 的内核构建。

## 快速开始

```bash
# 一键下载、校验并安装仓库锁定的 RISC-V musl 工具链
make setup ARCH=rv

# setup 完成后也可以单独复查工具链及 e2fsprogs
make doctor ARCH=rv

# 构建 staging，随后生成 256 MiB EXT4 镜像
make  build ARCH=rv PROFILE=minimal
make  image ARCH=rv PROFILE=minimal

# LoongArch operator 镜像
make  image ARCH=la PROFILE=operator
```

### Nano-X 图形镜像

Nano-X profile 首期支持 RISC-V，包含静态 `nano-X`、内置 `nanowm` 和演示程序：

```bash
make image ARCH=rv PROFILE=nanox

cd ../os
make shell ARCH=rv PROFILE=pre \
  SDCARD=../user/build/images/wateros-rv-nanox.ext4 \
  EXTRA_FEATURES=user-graphics
```

进入串口 shell 后执行 `start-nanox`。图形窗口和串口 shell 是两个独立界面；
`start-nanox` 会检查 `/dev/fb0`、keyboard/pointer evdev 节点，并管理 server、客户端和
`/tmp/.nano-X` 的生命周期。详细实现与排查见
[`docs/kasss's_todo_list/nanox.md`](../docs/kasss's_todo_list/nanox.md)。

`nanox` 镜像同时包含静态 Nano-X Doom 和仓库中的 `doom1.wad`。启动桌面后可在
`nxlaunch` 中点击 `Doom`，也可从串口执行：

```sh
start-nanox >/tmp/nanox.log 2>&1 &
start-doom
```

程序安装在 `/usr/bin/doom`，WAD 安装在
`/usr/share/games/doom/doom1.wad`。`start-doom` 默认使用三倍窗口缩放并直接进入
E1M1；可用 `start-doom -2` 改为两倍缩放，或用
`start-doom -3 -warp 1 2` 选择其他地图。

`setup` 把工具链安装在 `user/build/toolchains/rv/`，后续命令会自动发现，不需要
配置 `RV_CROSS_COMPILE`。它是显式的联网安装步骤，不会执行 `sudo`；`build/image`
本身仍然完全离线。已经下载官方归档时可以避免再次联网：

```bash
make  setup ARCH=rv \
  TOOLCHAIN_ARCHIVE=/path/to/riscv64-lp64d--musl--stable-2025.08-1.tar.xz
```

归档会按仓库锁定的 SHA-256 校验。`make clean` 保留下载缓存和工具链；只有
`make distclean` 会连同二者删除。当前自动安装器只锁定了 RISC-V 工具链，
LoongArch 暂时仍需设置 `LA_CROSS_COMPILE`。

输出位于：

```text
user/build/images/wateros-rv-minimal.ext4
user/build/images/wateros-rv-minimal.ext4.manifest.json
user/build/images/wateros-rv-minimal.ext4.sha256
```

在 WaterOS operator 模式中使用自有镜像：

```bash
cd os
make shell ARCH=rv PROFILE=pre \
  SDCARD=../user/build/images/wateros-rv-minimal.ext4
```

LoongArch 将 `ARCH` 和镜像名改为 `la`。minimal/operator 镜像不含比赛的
`/glibc`、`/musl` 测试目录，因此不要把它们用于现有自动 bringup 队列。

## 工具链

默认工具链配置在 `configs/architectures.toml`：


| `ARCH` | 默认前缀                        | 目标/ABI       |
| -------- | --------------------------------- | ---------------- |
| `rv`   | `riscv64-buildroot-linux-musl-` | `rv64gc/lp64d` |
| `la`   | `loongarch64-linux-musl-`       | `lp64d`        |

可以覆盖工具链前缀，不需要修改仓库文件：

```bash
RV_CROSS_COMPILE=/opt/toolchains/rv/bin/riscv64-linux-musl- \
  make  doctor ARCH=rv

LA_CROSS_COMPILE=/opt/toolchains/la/bin/loongarch64-linux-musl- \
  make  image ARCH=la PROFILE=minimal
```

前缀必须能找到 `gcc`、`ar`、`strip`；`doctor` 还会实际编译一个静态程序并用
`readelf` 检查目标架构和 `PT_INTERP`。宿主机需要 Python 3.11+、GNU make、patch，
以及 e2fsprogs 提供的 `mke2fs/debugfs/e2fsck/dumpe2fs`。检查失败只报告缺少的
工具，不联网，也不会执行 `sudo`。

## Profile 与根文件系统


| Profile    | Package                   | 用途                                |
| ------------ | --------------------------- | ------------------------------------- |
| `minimal`  | `base-layout`、`busybox`  | 最小静态 shell/rootfs               |
| `operator` | minimal +`operator-tools` | 增加`wos-help`、`wos-info` 现场脚本 |
| `nanox` | operator +`microwindows` | RISC-V Nano-X server、窗口管理器和演示程序 |

`base-layout` 提供标准目录、账号、网络基础配置、`/etc/profile` 和预留的
`/etc/init.d/rcS`。当前内核 operator supervisor 直接装载 `/bin/sh`，不会把
`rcS` 当作传统 PID 1。`/dev`、`/proc`、`/tmp` 等只在镜像中创建挂载点，运行时
仍由 WaterOS 的 devfs/procfs/tmpfs 初始化。

BusyBox 固定为 1.33.1，并以静态 musl 方式构建。安装完成后构建器检查 ELF 架构、
`PT_INTERP` 和动态 `NEEDED` 项；`/bin/sh` 及其他 applet 是指向 `/bin/busybox`
的符号链接。vendored 源码来源及提交见 `vendor/BUSYBOX_SOURCE.md`，许可证保留在
`vendor/busybox/LICENSE`。

## Package 模型

一个 package 的结构如下：

```text
packages/<name>/
├── package.toml
├── build.py
├── config/
└── patches/
```

`package.toml` 声明名称、版本、架构、依赖、源码目录、构建入口和覆盖权限。
`tools/userland.py` 会：

1. 拓扑排序依赖并拒绝依赖环；
2. 将源码复制到 `build/work`，按文件名顺序应用 patch；
3. 给 `build.py` 传入 JSON context，并要求它只安装到独立 `DESTDIR`；
4. 依据架构、源码、package 配置、patch、额外输入和工具链版本计算缓存键；
5. 合并 profile staging，同一路径冲突默认报错；
6. 写入 `/var/lib/wateros/packages.json` 和宿主侧文件清单。

新增 Vim、Lua 或其他用户项目时，只需新增 package 并把名称加入 profile，无需修改
EXT4 生成器。package 构建不得修改 `vendor/` 源码，也不得从网络下载依赖。

## EXT4 镜像

默认参数可在 Make 命令行覆盖：

```bash
make  image ARCH=rv PROFILE=minimal \
  IMAGE_SIZE_MB=512 BLOCK_SIZE=4096 INODE_SIZE=256 \
  OUTPUT=/tmp/wateros.ext4
```

镜像由 `mke2fs -d` 直接从 staging 生成，不需要 root、loop mount 或分区表。构建器
使用固定 UUID、label 和时间基准，启用 `64bit`，关闭 `metadata_csum`、`dir_index`、`orphan_file`、
`encrypt`、`casefold`，完成后运行只读 `e2fsck -fn`，再用 `debugfs` 检查关键文件。
这里启用 `64bit` 不是为了生成超大镜像，而是为了生成 WaterOS 当前
`another_ext4` 后端要求的 64 字节块组描述符。生成器会同时校验 4096 字节块、
256 字节 inode 和 64 字节描述符，避免产出 Linux 可读但 WaterOS 无法挂载的镜像。
`.manifest.json` 是逐路径内容清单，`.sha256` 是完整镜像摘要。

## 叠加比赛/测试镜像

```bash
make  overlay \
  ARCH=rv PROFILE=operator \
  BASE_IMAGE=../os/sdcard-rv.img
```

默认输出 `user/build/images/sdcard-rv-operator.ext4`。也可显式传 `OUTPUT`。
流程优先使用 reflink，不支持时普通复制；绝不会原地修改 `BASE_IMAGE`。叠加只接受：

- `/bin`、`/sbin`、`/usr/bin`、`/usr/sbin`
- `/etc/wateros`、`/opt/wateros`、`/root`、`/var/lib/wateros`

`/glibc` 和 `/musl` 被硬性保护。目标路径已存在时，只有 profile 的
`overlay_replace_prefixes` 明确允许才会替换；其余冲突立即失败。完成后会生成
`.changes.json`，记录基础镜像摘要、输出摘要和所有写入路径。

## 测试与排查

```bash
make  test
make  inspect ARCH=rv PROFILE=minimal
make  clean
```

测试覆盖 TOML 配置、依赖排序/环、文件冲突、显式覆盖、缓存键、manifest、实际 EXT4
生成、符号链接、权限和基础镜像不变性。若 `doctor` 报缺少交叉编译器，镜像工具的
宿主单元测试仍可运行，但不能把测试中的合成文件当成可启动 BusyBox。

## 目录

```text
user/
├── configs/       # 架构、profile
├── rootfs/base/   # 架构无关基础文件
├── packages/      # package 元数据、配置、patch、构建入口
├── vendor/        # 固定版本源码与许可证
├── tools/         # package 编排与 EXT4 工具
├── tests/         # Python 单元/集成测试
└── build/         # 缓存、work、staging、镜像（不提交）
```
