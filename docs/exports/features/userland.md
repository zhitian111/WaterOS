# userland — 已实现功能

事实来源：`user/README.md` 及 `user/` 当前源码。

## 功能表

| 功能 | 状态 | 说明 |
| --- | --- | --- |
| 普通目录用户空间 | 已实现 | 已从 `.gitmodules` 移除旧 `user` 子模块 |
| 双架构配置 | 已实现 | RV64 musl、LoongArch64 musl；前缀可由环境变量覆盖 |
| 环境 doctor | 已实现 | 检查 Python、e2fsprogs、交叉工具、目标和静态链接 |
| 通用 package | 已实现 | 依赖排序、环检测、work 副本、patch、DESTDIR、路径 owner |
| 内容缓存 | 已实现 | 架构、源码、配置、patch、额外输入和工具链共同决定键 |
| base-layout | 已实现 | 标准目录、账号、网络配置、profile、rcS、挂载点 |
| 静态 BusyBox | 已实现 | 1.33.1；ash/job control、vi、文件/进程/网络 applet |
| minimal profile | 已实现 | base-layout + BusyBox |
| operator profile | 已实现 | minimal + `wos-help`、`wos-info` |
| 独立 EXT4 | 已实现 | 无分区表、固定标识、保守 feature、fsck/debugfs 校验 |
| 外部镜像叠加 | 已实现 | reflink/copy、allowlist、冲突权限、变更清单 |
| Nano-X/Vim/Lua package | 预留 | 本阶段未实现 |

## 主要命令

```bash
make -C user doctor ARCH=rv
make -C user build ARCH=rv PROFILE=minimal
make -C user image ARCH=rv PROFILE=minimal
make -C user overlay ARCH=rv PROFILE=operator BASE_IMAGE=../os/sdcard-rv.img
make -C user test
```

默认输出是 `user/build/images/wateros-<arch>-<profile>.ext4`。通过
`os/Makefile` 的 `SDCARD` 传给 QEMU；用户空间 Makefile 不调用内核构建。

## 刻意限制

- 交叉 musl 工具链是宿主依赖，不随仓库提交二进制。
- 构建期不能联网；vendor 必须完整并保留许可证。
- 镜像系统不是运行时包管理器。
- 首版 operator 直接执行 `/bin/sh`，不提供传统 init/service manager。
- overlay 不扫描或解释比赛测试，只保护 `/glibc`、`/musl` 不被修改。
- GUI、Nano-X、GPU、输入设备不在本阶段范围内。

## 修订

| 日期 | 说明 |
| --- | --- |
| 2026-08-09 | 重建为 WaterOS 自有双架构用户空间与 EXT4 系统 |
