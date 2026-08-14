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

`PACKAGE` 提供五个预设：

| 选项 | 实际内容 | 适用场景 |
| --- | --- | --- |
| `all` | 当前架构支持的全部 package | 默认完整用户空间 |
| `minimal` | `base-layout,busybox` | 最小静态 shell/rootfs |
| `operator` | minimal + `operator-tools` | shell 与现场诊断工具 |
| `graphics` | `microwindows` 及其依赖 | 双架构 Nano-X、演示程序与 Doom |
| `jvm` | `openjdk21` 及其依赖 | 双架构 OpenJDK 21 headless 运行时 |

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

`PACKAGE=all` 会扫描 `packages/*/package.toml`，只选择声明支持当前 `ARCH` 的 package。
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
`vmsplice` 和 `ioprio`，不依赖宿主架构或模拟返回值。

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

新增用户程序时只需增加 package。只要它在 `package.toml` 中声明支持目标架构，默认的
`PACKAGE=all` 就会自动发现，无需修改镜像生成器或新增组合配置文件。

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
