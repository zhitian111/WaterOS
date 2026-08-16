<div align="center">
  <a href="../README.md">
    <img src="../docs/assert/cover.jpg" height="72" alt="山东大学" />
  </a>
  <h1>WaterOS Userland</h1>
  <p>用户空间 Package 构建与 EXT4 根文件系统生成工具</p>
  <p>
    <a href="../README.md">项目首页</a> ·
    <a href="../os/README.md">内核工程</a> ·
    <a href="../docs/README.md">项目文档</a>
  </p>
</div>

---

`user/` 是 WaterOS 自己维护的用户空间工程。它负责交叉编译 BusyBox、Nano-X 等
package，将其组合成 rootfs staging，然后生成可由 QEMU 直接挂载的无分区表 EXT4
镜像。

它不是运行时包管理器，也不会被根目录的 `make all` 隐式执行。内核与用户镜像可以
独立构建和替换。

## 最快用法

以下命令均在仓库的 `user/` 目录执行：

```bash
cd /path/to/WaterOS/user

# 首次使用：安装目标架构的本地交叉工具链
make setup ARCH=rv
# 或：make setup ARCH=la

# Arch Linux：安装 riscv64-gnu-toolchain-musl-bin 后可直接构建；
# 构建器会自动配对它的 musl GCC 与 GNU binutils。

# 构建当前架构支持的全部 package，并生成 EXT4 镜像
make image ARCH=rv
# 跳过 mGBA；依赖 mGBA 的 waterfm 也会自动跳过
make image ARCH=rv SKIP_PACKAGE=mgba
# 构建指定的Package组合
make image ARCH=rv PACKAGE=graphics
# 启动
cd ../os
make shell ARCH=rv PROFILE=pre \
  SDCARD=../user/build/images/wateros-rv.ext4
```

部分模块构建需要更新子模块

```bash
git submodule update --init --recursive user/vendor/mgba
```

默认产物为：

```text
build/images/wateros-rv.ext4
build/images/wateros-rv.ext4.manifest.json
build/images/wateros-rv.ext4.sha256
```

其中：

- `.ext4` 是可直接挂载的根文件系统镜像；
- `.manifest.json` 记录镜像内各路径的类型、权限和摘要；
- `.sha256` 记录整个镜像的 SHA-256。

## Make 参数

运行 `make help` 可以查看当前入口和默认值。


| 参数            | 默认值                             | 作用                              |
| ----------------- | ------------------------------------ | ----------------------------------- |
| `ARCH`          | `rv`                               | 目标架构，可选`rv`、`la`          |
| `PACKAGE`       | `all`                              | 要组合的 package 预设或自定义列表 |
| `IMAGE_SIZE_MB` | `256`                              | EXT4 镜像容量，单位 MiB           |
| `BLOCK_SIZE`    | `4096`                             | EXT4 块大小                       |
| `INODE_SIZE`    | `256`                              | EXT4 inode 大小                   |
| `JOBS`          | 宿主 CPU 数                        | 并行编译任务数                    |
| `OUTPUT`        | `build/images/wateros-<arch>.ext4` | 自定义输出路径                    |
| `BASE_IMAGE`    | 无                                 | `overlay` 使用的基础镜像          |

`PACKAGE` 提供六个预设：

| 选项 | 实际内容 | 适用场景 |
| --- | --- | --- |
| `all` | 当前架构支持的全部 package | 默认完整用户空间 |
| `minimal` | `base-layout,busybox` | 最小静态 shell/rootfs |
| `operator` | minimal + `operator-tools` | shell 与现场诊断工具 |
| `graphics` | `microwindows` 及其依赖 | 双架构 Nano-X、演示程序与 Doom |
| `jvm` | `openjdk21` 及其依赖 | 双架构 OpenJDK 21 headless 运行时 |
| `minecraft` | `minecraft-server` 及其依赖 | Minecraft Java 服务端与 OpenJDK 21 |

例如：

```bash
# 默认完整镜像
make image ARCH=rv

# 最小镜像
make image ARCH=rv PACKAGE=minimal

# Nano-X 与 Doom；依赖会自动补齐
make image ARCH=rv PACKAGE=graphics

# OpenJDK 21；为运行时和后续应用预留空间
make image ARCH=rv PACKAGE=jvm IMAGE_SIZE_MB=512 \
  OUTPUT=build/images/wateros-rv-openjdk21.ext4

# Minecraft 是显式选择项；首次下载前必须先阅读并接受官方 EULA
MINECRAFT_EULA_DOWNLOAD_ACCEPTED=true \
  make image ARCH=rv PACKAGE=minecraft IMAGE_SIZE_MB=1024 \
  OUTPUT=build/images/wateros-rv-minecraft.ext4

# 用户自定义组合，名称用逗号分隔
make image ARCH=rv PACKAGE=base-layout,busybox,operator-tools
```

所有组合默认写入同一个架构产物，例如 `wateros-rv.ext4`。后一次构建会替换前一次
生成的镜像。需要同时保留多个组合时使用 `OUTPUT`：

```bash
make image ARCH=rv PACKAGE=minimal \
  OUTPUT=build/images/wateros-rv-minimal.ext4

make image ARCH=rv PACKAGE=graphics \
  OUTPUT=build/images/wateros-rv-graphics.ext4
```

`PACKAGE=all` 会扫描 `packages/*/package.toml`，只选择声明支持当前 `ARCH` 且默认启用的
package。包含专有许可和显式 EULA 确认的 `minecraft-server` 不会被 `all` 自动选择。
当前 `microwindows` 与 `openjdk21` 均支持 RV 和 LA，因此两个架构的默认完整镜像都包含
Nano-X、演示程序、Doom、`doom1.wad` 和 OpenJDK 21。JVM 运行时较大，制作 `all` 镜像时
建议设置 `IMAGE_SIZE_MB=512`。

## 常用命令

```bash
# 安装并检查交叉工具链
make setup ARCH=rv

# 只检查环境，不构建
make doctor ARCH=rv

# 只生成合并后的 staging，不制作 EXT4
make build ARCH=rv

# 构建 package、合并 staging 并制作 EXT4
make image ARCH=rv

# 查看镜像的 EXT4 信息和内嵌 package 元数据
make inspect ARCH=rv

# 运行宿主单元测试与 EXT4 集成测试
make test

# 删除镜像、staging 和 package 缓存，保留工具链及下载缓存
make clean

# 删除整个 build，包括已安装工具链和下载缓存
make distclean
```

`build` 与 `image` 的区别是：`build` 到 staging 为止，`image` 会继续运行 `mke2fs`、
`e2fsck` 和 `debugfs`，最终生成可挂载镜像。通常直接使用 `make image` 即可。

## 在 WaterOS 中启动

先构建用户镜像：

```bash
cd /path/to/WaterOS/user
make image ARCH=rv
```

再进入内核目录启动 operator shell：

```bash
cd ../os
make shell ARCH=rv PROFILE=pre \
  SDCARD=../user/build/images/wateros-rv.ext4
```

这里的 `PROFILE=pre` 是 `os/Makefile` 的内核构建配置，用于选择内核的 pre/final
feature；它与用户镜像无关。`user/Makefile` 已经不再使用 `PROFILE`。

自有镜像不包含比赛镜像中的 `/glibc`、`/musl` 测试目录，因此不要直接用它替换自动
bringup 所需的比赛测试镜像。如需在比赛镜像中加入自有工具，应使用 `overlay`。

`PACKAGE=operator` 及默认完整镜像会安装目标机 syscall 冒烟程序。进入 shell 后运行：

```sh
wos-syscall-smoke
```

它会直接验证当前内核的 `sendfile`、`copy_file_range`、`splice`、`tee`、
`vmsplice`、`ioprio`、`timerfd`、`recvmmsg` 和 `signalfd4`，不依赖宿主架构或
模拟返回值。RV 与 LA 使用同一份目标机源码，测试真实 VFS、pipe、socket、timer、
signal 和用户内存复制链路。

## Nano-X 与 Doom

RV 与 LA 的默认完整镜像均包含 Nano-X、演示程序、Doom 和 `doom1.wad`。启动时还需要
让内核向用户态暴露 framebuffer 和输入设备。RISC-V：

```bash
cd /path/to/WaterOS/user
make image ARCH=rv

cd ../os
make shell ARCH=rv PROFILE=pre \
  SDCARD=../user/build/images/wateros-rv.ext4 \
  EXTRA_FEATURES=user-graphics
```

LoongArch：

```bash
cd /path/to/WaterOS/user
make setup ARCH=la
make image ARCH=la

cd ../os
make shell ARCH=la PROFILE=pre \
  SDCARD=../user/build/images/wateros-la.ext4 \
  EXTRA_FEATURES=user-graphics
```

进入串口 shell 后启动 Nano-X：

```sh
start-nanox >/tmp/nanox.log 2>&1 &
```

图形窗口和串口 shell 是两个独立界面。`start-nanox` 会检查 `/dev/fb0`、
`/dev/input/keyboard0` 和 `/dev/input/pointer0`，并管理 Nano-X server、客户端及
`/tmp/.nano-X` socket 的生命周期。

默认桌面使用 1280×800 深色 WaterOS 波纹背景和底部居中的应用启动栏。按钮支持正常、
悬停和按下三种状态；终端、编辑器、计算器、时钟与 Doom 始终可用，`Files` 和 `mGBA`
会在对应 package 未安装时自动隐藏。背景原图、图标生成器和启动器主题分别位于：

```text
packages/microwindows/assets/wateros-waves.png
packages/microwindows/tools/prepare_assets.py
packages/microwindows/patches/0006-wateros-launcher-desktop-theme.patch
```

修改这些文件后重新执行 `make image ARCH=rv` 即可；无需向 Nano-X 增加 PNG 库，构建器
会将资源转换成 PPM。默认不再自动弹出 `nxclock` 和 `nxeyes`，需要时从启动栏打开。

图形终端已经作为 `/usr/bin/nxterm` 安装。可以在 `nxlaunch` 点击 `Terminal`，或从串口执行：

```sh
nxterm &
```

内核通过 `/dev/ptmx` 与动态 `/dev/pts/N` 提供 UNIX98 PTY。无需启动 Nano-X 即可先验证：

```sh
pty-smoke
ls -l /dev/ptmx /dev/pts
```

`nxterm` 内默认执行 `/bin/sh`，支持多个独立窗口、canonical/raw 模式、`poll/select`、
Ctrl-C、Ctrl-Z 和 shell 作业控制。

默认完整镜像还会在 `nxlaunch` 提供 `Files` 按钮，用于启动 `/usr/bin/waterfm`。
`waterfm` 会被 `PACKAGE=all` 或 `PACKAGE=waterfm` 选中；精简的 `PACKAGE=graphics`
只包含 Nano-X、Doom 和终端，不包含文件管理器，此时 `Files` 按钮会自动隐藏。

可以在 `nxlaunch` 中点击 Doom，也可以从串口启动：

```sh
start-doom
```

Doom 安装位置：

```text
/usr/bin/doom
/usr/share/games/doom/doom1.wad
```

`start-doom` 默认使用二倍窗口（640×400）并直接进入 E1M1。二倍窗口显著减少软件缩放和
VirtIO GPU 提交的像素量。需要三倍窗口时可执行：

```sh
start-doom -3
```

指定其他地图：

```sh
start-doom -3 -warp 1 2
```

性能统计默认关闭。排查刷新次数或 Doom 帧率时可分别启用：

```sh
NANOX_STATS=1 start-nanox >/tmp/nanox.log 2>&1 &
DOOM_STATS=1 start-doom
```

Nano-X 统计会报告脏区更新、present 次数和提交像素数；Doom 统计会报告 FPS、像素转换和
请求提交耗时。

完整图形链路和排查方式见
[`Nano-X 支持文档`](../docs/todo/kasss's_todo_list/nanox.md)。

## 叠加到比赛或测试镜像

`overlay` 会复制基础镜像，再把选中的 package 写入副本，不会修改原始镜像：

```bash
make overlay \
  ARCH=rv \
  PACKAGE=operator \
  BASE_IMAGE=../os/sdcard-rv.img
```

默认输出：

```text
build/images/sdcard-rv-wateros.ext4
build/images/sdcard-rv-wateros.ext4.changes.json
build/images/sdcard-rv-wateros.ext4.sha256
```

需要自定义输出时：

```bash
make overlay \
  ARCH=rv \
  PACKAGE=operator \
  BASE_IMAGE=../os/sdcard-rv.img \
  OUTPUT=/tmp/sdcard-rv-with-tools.ext4
```

叠加写入范围被限制为：

- `/bin`、`/sbin`、`/usr/bin`、`/usr/sbin`；
- `/etc/wateros`、`/opt/wateros`；
- `/root`、`/var/lib/wateros`。

`/glibc` 和 `/musl` 被硬性保护。基础镜像和输出路径相同也会被拒绝。完成后工具会运行
只读 `e2fsck -fn`，并生成变更清单。

## 工具链

架构配置位于 `configs/architectures.toml`：


| `ARCH` | 默认工具链前缀                  | ABI                       |
| -------- | --------------------------------- | --------------------------- |
| `rv`   | `riscv64-buildroot-linux-musl-` | 静态 musl，`rv64gc/lp64d` |
| `la`   | `loongarch64-linux-gnu-`        | 静态 glibc，`lp64d`       |

双架构都可以一键准备：

```bash
make setup ARCH=rv
make setup ARCH=la
```

工具链分别安装到 `build/toolchains/rv/`、`build/toolchains/la/`，后续构建会自动发现。
`setup` 是显式联网步骤，不会运行 `sudo`；`build` 和 `image` 本身不会下载依赖。

- RV 下载并校验仓库锁定的 Bootlin 静态 musl 工具链归档；
- LA 在 Debian/Ubuntu 上通过 `apt-get download` 下载交叉编译器 deb，再解包到
  `user/build`，不会安装宿主软件包。LA 产物静态链接 glibc，镜像仍不需要动态加载器。

已经持有仓库锁定的工具链归档时可以离线安装：

```bash
make setup ARCH=rv \
  TOOLCHAIN_ARCHIVE=/path/to/riscv64-lp64d--musl--stable-2025.08-1.tar.xz
```

也可以使用环境变量覆盖工具链：

```bash
RV_CROSS_COMPILE=/opt/rv/bin/riscv64-linux-musl- \
  make doctor ARCH=rv

LA_CROSS_COMPILE=/opt/la/bin/loongarch64-linux-gnu- \
  make image ARCH=la
```

前缀必须能找到 `gcc`、`ar` 和 `strip`。`doctor` 会实际静态链接一个目标程序，并用
`readelf` 检查 ELF 架构、`PT_INTERP` 和动态依赖。宿主还需要：

- Python 3.11 或更高版本；
- GNU Make、patch；
- e2fsprogs 提供的 `mke2fs`、`debugfs`、`e2fsck`、`dumpe2fs`。

非 Debian/Ubuntu 宿主无法使用 LA 的 deb 解包后端，此时需自行提供带静态 libc 的
LoongArch 工具链并设置 `LA_CROSS_COMPILE`。

### macOS 与 Docker Desktop

仓库锁定的 RISC-V 工具链是 Linux x86_64 程序，macOS 不能直接执行。请在
Docker Desktop 的 Linux/amd64 容器内构建：

```bash
docker run --rm --platform linux/amd64 \
  -v "$PWD":/workspace -w /workspace \
  python:3.11-slim-bookworm sh -ec '
    sed -i "s|http://deb.debian.org|https://deb.debian.org|g" \
      /etc/apt/sources.list.d/debian.sources
    apt-get -o Acquire::Retries=5 update
    DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=5 install -y \
      --no-install-recommends build-essential e2fsprogs make patch \
      ca-certificates xz-utils file
    make setup ARCH=rv
    make image ARCH=rv JOBS=2
  '
```

当前 `user/` 目录被挂载到 `/workspace`，所以产物仍会出现在宿主的
`build/images/wateros-rv.ext4`。

## Package 模型

每个 package 位于独立目录：

```text
packages/<name>/
├── package.toml
├── build.py
├── config/
├── patches/
└── scripts/
```

目录只需包含实际用到的子项。`package.toml` 声明：

- 名称与版本；
- 支持架构；
- package 依赖；
- vendored 源码位置；
- 构建入口；
- 安装前缀和允许覆盖的路径。

构建器会：

1. 解析依赖、拓扑排序并拒绝依赖环；
2. 将源码复制到 `build/work`，不修改 `vendor/`；
3. 按文件名顺序应用 `patches/*.patch`；
4. 运行 package 的 `build.py`，安装到独立 `DESTDIR`；
5. 根据架构、工具链、源码、patch 和配置计算缓存键；
6. 合并 package 输出，同一路径冲突默认报错；
7. 写入 `/var/lib/wateros/packages.json` 和宿主侧 manifest。

新增用户程序时只需增加 package。只要它在 `package.toml` 中声明支持目标架构且未设置
`default = false`，默认的 `PACKAGE=all` 就会自动发现，无需修改镜像生成器或新增组合
配置文件。大体积或需要显式许可确认的 package 可设为非默认，并通过名称或预设选择。

当前 package：

| 名称 | 作用 | 架构 |
| --- | --- | --- |
| `base-layout` | 目录、账号、环境与基础配置 | RV、LA |
| `busybox` | 静态 shell 与常用 applet | RV、LA |
| `operator-tools` | WaterOS 现场诊断脚本 | RV、LA |
| `microwindows` | Nano-X、演示程序、Doom | RV、LA |
| `mgba` | mGBA 模拟器及示例 ROM | RV |
| `waterfm` | Nano-X 文件管理器 | RV |
| `openjdk21` | OpenJDK 21 headless、zlib 与 JVM 冒烟探针 | RV、LA |
| `minecraft-server` | Minecraft Java 1.21.11 服务端与启动验收脚本 | RV、LA（显式选择） |

`base-layout` 只创建 `/dev`、`/proc`、`/tmp` 等挂载点。运行时实际内容仍由 WaterOS 的
devfs、procfs 和 tmpfs 提供。内核 operator supervisor 直接启动 `/bin/sh`，当前不会
把 `/etc/init.d/rcS` 作为传统 PID 1 执行。

## OpenJDK 21

`openjdk21` 在 RISC-V 使用 Alpine v3.22 的 musl 二进制包，在 LoongArch 使用龙芯发布的
新世界 glibc 2.34+ 版本，并为后者从受管交叉工具链安装匹配的 glibc 运行库、交叉构建
zlib。所有下载文件均校验 SHA-256。当前选择 Java 21 LTS；包内包含 `java`/HotSpot
运行时，不包含 `javac`，适合验证 WaterOS 的 Linux ABI 与运行现有 JAR/class。

```bash
# 在 user/ 下构建 512 MiB JVM 镜像
make image ARCH=rv PACKAGE=jvm IMAGE_SIZE_MB=512 \
  OUTPUT=build/images/wateros-rv-openjdk21.ext4

# 在仓库根目录，让 WaterOS 启动镜像并自动跑完整 JVM 探针后关机
make -C os run ARCH=rv PROFILE=pre MODE=run \
  SCRIPT=/opt/wateros/bin/wos-jvm-smoke \
  SDCARD=../user/build/images/wateros-rv-openjdk21.ext4 SMP=4
```

LoongArch 使用相同流程，把上述命令中的 `ARCH=rv` 和文件名中的 `rv` 改为 `ARCH=la`
与 `la` 即可。双架构冒烟脚本都会关闭 compressed class pointers，避免 HotSpot 默认预留
1 GiB class space；这不影响普通 Java 类加载，只是不再使用该地址压缩优化。

镜像内可直接运行：

```sh
java -version
wos-jvm-smoke version
wos-jvm-smoke hello
wos-jvm-smoke runtime-int
wos-jvm-smoke exception              # 异常展开、catch/finally、跨线程异常投递
wos-jvm-smoke jit
wos-jvm-smoke jit-strict             # 固定 C2 编译目标并核对编译结果
wos-jvm-smoke network                 # 默认测试 example.com
wos-jvm-smoke network your.host.name  # 指定 HTTPS 主机
wos-jvm-smoke application             # JAR/NIO/子进程/Selector
wos-jvm-smoke strict                  # 不含外网的严格完整 JVM 验收
```

冒烟测试固定使用 Serial GC、128 MiB 最大堆，并关闭 compressed ordinary/class pointers，
覆盖版本输出、class 装载、解释执行、
线程、分配/GC 和默认 JIT。联网探针额外覆盖 Java 信任库加载、DNS、TCP 443、TLS
握手和 HTTPS 响应。镜像安装 Alpine `ca-certificates-bundle` 的系统 PEM，并安装由同一
证书集合生成的 JKS 到 `/etc/ssl/certs/java/cacerts`；APK、JKS 和预编译探针均在构建时
校验 SHA-256。

应用探针以可执行 JAR 形式运行，覆盖 JAR manifest、classloader/resource、反射、UTF-8、
临时目录、1 MiB FileChannel 读写、文件锁、mmap/msync、原子 rename、`ProcessBuilder`
的环境变量/stdout/stderr/退出码，以及基于 Selector/epoll 的本机 TCP accept 和 direct
ByteBuffer。

严格验收模式在上述覆盖之外，还要求 GC 日志中实际出现一次回收，验证递归异常栈展开、
`catch/finally` 和跨线程未捕获异常投递，并使用 `-Xbatch`、关闭分层编译、降低编译阈值和
`PrintCompilation`，确认 `JitProbe::hot` 确实被 C2 编译且生成代码的结果与参考实现一致。
成功时最终输出 `WATEROS_JVM_STRICT_OK`。该模式不含依赖外部环境的 DNS/TLS/HTTPS：

```bash
make -C os run ARCH=rv PROFILE=pre MODE=run \
  SCRIPT=/opt/wateros/bin/wos-jvm-strict \
  SDCARD=../user/build/images/wateros-rv-openjdk21.ext4 SMP=4
```

应用探针也可单独运行：

```bash
make -C os run ARCH=rv PROFILE=pre MODE=run \
  SCRIPT=/opt/wateros/bin/wos-jvm-application \
  SDCARD=../user/build/images/wateros-rv-openjdk21.ext4 SMP=4
```

自动跑联网探针并在完成后关机：

```bash
make -C os run ARCH=rv PROFILE=pre MODE=run \
  SCRIPT=/opt/wateros/bin/wos-jvm-network \
  SDCARD=../user/build/images/wateros-rv-openjdk21.ext4 SMP=4
```

## Pacman（RISC-V）

`pacman` 是当前自有 musl 用户镜像的 RISC-V 默认包，会随 `PACKAGE=all` 自动加入；LoongArch
镜像不包含它。构建时会从
公开上游下载并校验 pacman、libarchive、zlib、xz、zstd 和 OpenSSL 的锁定源码版本；下载归档
缓存于 `user/build/downloads/pacman/`，不属于 Git 提交内容。

```bash
make image ARCH=rv PACKAGE=pacman IMAGE_SIZE_MB=512 JOBS=4
```

镜像包含 `/usr/bin/pacman`、`/usr/lib/libalpm.so.15`、匹配的 musl 动态加载器、静态
libcurl/HTTPS 依赖和 `/etc/pacman.conf`。GPGME 与包签名校验仍暂时关闭；本地安装与同步
仓库均可使用：

```sh
pacman --version
pacman -Q
pacman -U /tmp/name-riscv64.pkg.tar.zst
pacman -Sy
pacman -S name
```

默认 `/etc/pacman.d/mirrorlist` 指向 Arch Linux RISC-V 仓库
`https://archriscv.felixc.at/repo/$repo`。阿里云和清华的官方 Arch Linux 镜像地址同时作为
注释示例保留，但它们不提供 `riscv64` 包，不能在 WaterOS 中启用。网络同步（`pacman -Sy`）
已经通过 libcurl/HTTPS 启用；签名验证将在后续引入 GPGME 和 Arch Linux RISC-V keyring 后启用。

Arch Linux RISC-V 仓库包使用 glibc，不能直接覆盖 WaterOS 的 musl/BusyBox 根目录。使用
`archriscv-pacman` 会自动初始化隔离根的 pacman 数据库并安装到 `/opt/archriscv`：

```sh
archriscv-pacman -S neovim
archriscv-run /usr/bin/nvim
```

后一命令通过 `chroot(2)` 在隔离根中执行程序；因此 glibc 动态库、Lua 等通过绝对路径
`dlopen()` 的模块，以及包内脚本都会从 `/opt/archriscv` 解析，而不会混用 WaterOS 的 musl
根目录。由于 `chroot` 会遮住宿主 `/dev`，`archriscv-run` 会先把宿主 `/dev` bind 到
`/opt/archriscv/dev`，而不是使用自指的 `/dev` 符号链接。该命令要求内核已支持
`chroot(2)` 与 bind mount。可通过 `WATER_ARCHRISCV_ROOT=/other/root` 覆盖隔离根路径。

## Arch Linux 的 RISC-V musl 工具链

Arch 的 `riscv64-gnu-toolchain-musl-bin` 通常只以 `riscv64-linux-musl-` 前缀提供 GCC，
而 `ar`、`strip`、`readelf` 位于 `riscv64-linux-gnu-` 前缀。构建器检测到这一组合时会在
`user/build/toolchains/rv/archlinux-compat/bin/` 生成私有兼容前缀；无需手动设置
`RV_CROSS_COMPILE`。

构建当前图形用户空间（mGBA 与 WaterFM，且避开 OpenJDK）使用：

```bash
make image ARCH=rv PACKAGE=waterfm JOBS=4
```

`PACKAGE=waterfm` 会自动包含 `mgba`、Nano-X 和它们的基础依赖。显式设置
`RV_CROSS_COMPILE` 时仍完全由该环境变量接管。

## Minecraft Java 服务端

`minecraft-server` 固定使用 Mojang 官方 Minecraft Java 1.21.11 server.jar，并校验官方
对象 SHA-1。1.21.11 是兼容 Java 21 的最后一个正式版本；Minecraft 26.1 起改用 Java 25，
不能用于验收当前 OpenJDK 21。该 package 依赖 `openjdk21`，不会进入默认 `PACKAGE=all`。
官方说明下载服务端软件即
表示同意 Minecraft EULA 与隐私政策，因此首次联网构建必须显式确认。以下是从构建、
安全验收到正式运行的完整流程：

```bash
# 1. 首次联网准备工具链并构建专用镜像
cd /path/to/WaterOS/user
make setup ARCH=rv
MINECRAFT_EULA_DOWNLOAD_ACCEPTED=true \
  make image ARCH=rv PACKAGE=minecraft IMAGE_SIZE_MB=1024 \
  OUTPUT=build/images/wateros-rv-minecraft.ext4

# 2. 以 snapshot 模式进入 WaterOS；本轮操作不会写回基础镜像
cd ../os
make shell ARCH=rv PROFILE=pre SMP=8 SNAPSHOT=1 \
  SDCARD=../user/build/images/wateros-rv-minecraft.ext4
```

进入 WaterOS shell 后顺序执行：

```sh
# 3. 镜像内验收
wos-minecraft-preflight          # 不接受 EULA，也不创建正式世界
minecraft-server --accept-eula   # 阅读官方 EULA 后显式接受
minecraft-server --check
wos-minecraft-vm-info
wos-minecraft-smoke              # 隔离的 flat 世界：启动、Done、stop、保存

# 4. 本次 snapshot 会话内正式运行（退出 QEMU 后世界不会写回镜像）
minecraft-server
```

下载文件缓存于 `user/build/downloads/minecraft-server/`，此后不设置该变量也能离线重建。
构建确认不等于运行时确认；镜像不会预置 `eula=true`。上面的 `SNAPSHOT=1` 适合无损验收，
但退出 QEMU 后会丢弃 EULA、配置和世界。需要持久保存正式世界时，退出验收会话后使用：

```bash
cd /path/to/WaterOS/os
make shell ARCH=rv PROFILE=pre SMP=8 WRITE_DISK=1 \
  SDCARD=../user/build/images/wateros-rv-minecraft.ext4
```

然后在新的 WaterOS shell 中重新执行 `minecraft-server --accept-eula` 和 `minecraft-server`。
`WRITE_DISK=1` 会直接修改基础镜像，运行前应保留备份，并使用 `stop` 正常关服。

这些验收命令同时提供 `/usr/bin` 入口和 `/opt/wateros/bin` 的自动运行路径。旧镜像若尚未
包含 `/usr/bin` 链接，可直接运行 `/opt/wateros/bin/wos-minecraft-smoke`。

预检只验证 bundler 解包、Java 21 class 装载以及服务端生成 `eula=false` 后正常退出，成功
输出 `WATEROS_MINECRAFT_PREFLIGHT_OK`。严格验收脚本在 `/tmp` 创建低视距、和平、离线、
flat 测试世界，等待服务端输出 `Done`，通过
控制台执行 `stop`，并检查正常退出、`level.dat` 与 region 目录。它同时覆盖真实 JAR
加载、较大堆、GC、后台线程、环回 TCP 监听、文件锁、同步写入、世界保存和
关服路径；成功时输出 `WATEROS_MINECRAFT_SERVER_OK`。`Done` 只统计服务端世界准备阶段，
不包含之前的 JAR 解包和数据加载；QEMU TCG 下整条 smoke 首次运行仍可能持续数分钟，
preflight 和 smoke 脚本都会每 30 秒输出一次当前进度。正式数据默认位于
`/var/lib/minecraft`，可通过 `MINECRAFT_DATA_DIR` 和 `MINECRAFT_JAVA_ARGS` 覆盖。
`wos-minecraft-vm-info` 会列出可能注入 JVM 参数的环境变量，并确认 HotSpot 的
`SelfDestructTimer` 初始值为 0；它用于区分启动参数和运行期破坏导致的 JVM 主动退出。
首次直接运行 `minecraft-server` 且数据目录尚无 `server.properties` 时，启动器会写入
适合 QEMU TCG 的快速首启配置：保留普通地形，关闭结构生成，并把 view/simulation
distance 设为 2。已有配置永远不会被覆盖。RISC-V 默认向 Java 暴露 4 个处理器；仍保留
Serial GC 与 C1 限制以避开尚未解决的 C2 崩溃。

完整服务端初测曾在并发解包阶段打印 `VM self-destructed`。诊断确认 HotSpot 的该选项启动
值为默认的 0，实际缺口是 RISC-V trap 帧没有保存浮点上下文：时钟抢占和线程切换会让
不同 Java 线程串用 `f0`–`f31`/`fcsr`。现在 RISC-V 每次 trap 都在进入 Rust 前保存 32 个
浮点寄存器与 `fcsr`，返回原任务前恢复；信号帧也从该稳定快照读写浮点状态。修复后，
4 核严格 JVM 验收和 Minecraft 的解包、建服、环回监听、世界生成、保存与正常关服均已
通过，服务端输出 `WATEROS_MINECRAFT_SERVER_OK`。

RISC-V 上 Minecraft 当前默认增加 `-XX:TieredStopAtLevel=1`。真实预检先发现 WaterOS
把同步 `SIGILL`/`SIGSEGV` 错误报告成 `SI_USER` 且遗漏 `si_addr`，导致 HotSpot 的陷阱
处理器误判；按 Linux ABI 修复后，C1 已能完成官方 JAR 预检。完整 C2 仍会在复杂的
`ThreadLocal.getCarrierThreadLocal()` 编译路径崩溃，因此只对 RISC-V Minecraft 暂时关闭
C2。LoongArch 保留正常分层 JIT，`wos-jvm-smoke jit-strict` 也继续独立验收基础 JIT。
后续应缩减并修复该 RV C2 用例。
开发者可在可写的临时镜像中运行 `wos-minecraft-jit-diagnostic`，它会启用完整分层 JIT，并把
日志及 `hs_err_pid*.log` 保存在 `/var/lib/minecraft/jit-diagnostic/`，便于提取故障指令；
不要把该命令当作正常启动入口。

## EXT4 生成规则

镜像通过 `mke2fs -d <staging>` 直接生成，不需要 root、loop mount 或分区表。为适配
WaterOS 当前的 `another_ext4` 后端，生成器会：

- 使用固定 UUID、label 和时间基准；
- 使用 4096 字节块和 256 字节 inode；
- 启用 `64bit` 以生成 64 字节块组描述符；
- 关闭 `metadata_csum`、`dir_index`、`orphan_file`、`encrypt` 和 `casefold`；
- 运行 `e2fsck -fn`；
- 使用 `debugfs` 检查 `/bin/busybox`、`/bin/sh`、权限与 package 元数据。

这里的 `64bit` 是 EXT4 布局 feature，不表示 WaterOS 镜像必须大于 2 TiB。

## 目录结构

```text
user/
├── Makefile              # 推荐入口
├── README.md
├── configs/              # 架构和交叉工具链配置
├── rootfs/base/          # 架构无关基础文件
├── packages/             # package 元数据、配置、patch 和构建脚本
├── vendor/               # 固定版本上游源码及许可证
├── tools/                # package 编排、工具链安装和 EXT4 工具
├── tests/                # Python 单元测试和 EXT4 集成测试
└── build/                # 工具链、缓存、staging 和镜像，不提交
```
