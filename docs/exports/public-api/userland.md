# userland — 构建接口

用户空间工程没有提供内核 Rust 公共 API；它的稳定接口是 Make/Python 命令、TOML
元数据和 package 构建 context。

## Make 接口

| 命令 | 作用 |
| --- | --- |
| `make -C user setup ARCH=rv` | 安装并校验仓库锁定的 RV musl 工具链 |
| `make -C user doctor ARCH=<rv|la>` | 检查目标工具链、静态链接及镜像工具 |
| `make -C user build ARCH=... PROFILE=...` | 构建 package 并合并 staging |
| `make -C user image ARCH=... PROFILE=...` | 构建独立 EXT4 |
| `make -C user overlay ... BASE_IMAGE=...` | 创建并叠加基础镜像副本 |
| `make -C user inspect ...` | 显示文件系统和嵌入 package 元数据 |
| `make -C user test` | 运行 Python 单元/EXT4 集成测试 |

公共变量：`ARCH`、`PROFILE`、`JOBS`、`IMAGE_SIZE_MB`、`BLOCK_SIZE`、
`INODE_SIZE`、`BASE_IMAGE`、`OUTPUT`、`TOOLCHAIN_ARCHIVE`、`FORCE`。工具链覆盖变量是
`RV_CROSS_COMPILE`、`LA_CROSS_COMPILE`。

## Package 元数据

`packages/<name>/package.toml` 的 `[package]` 字段：

| 字段 | 含义 |
| --- | --- |
| `name/version` | 身份与输出元数据 |
| `source` | 相对 `user/` 的 vendored 源码；空表示无源码 |
| `architectures` | 支持的 `rv`/`la` 集合 |
| `dependencies` | package 名列表 |
| `build` | 相对 package 目录的 Python 构建入口 |
| `install_prefix` | 记录安装前缀（实际写入仍以 DESTDIR 为根） |
| `allow_overwrite` | staging 合并时允许覆盖的精确逻辑路径 |
| `inputs` | 额外参与缓存摘要的 `user/` 内路径 |

构建入口调用形式为：

```text
python3 packages/<name>/build.py --context <cache>/context.json
```

context 提供 `arch/triple/cross_compile/kernel_arch/cflags/elf_machine/readelf/jobs`，
以及 `user_root/package_dir/source_dir/work_dir/destdir`。构建脚本只能写 `work_dir`
和 `destdir`，不能写 vendor 或联网下载。

## Profile 元数据

`configs/profiles.toml` 为每个 profile 声明：

- `packages`：根 package 列表；依赖自动加入。
- `allow_overwrite`：profile staging 的精确覆盖路径。
- `overlay_replace_prefixes`：基础镜像已有路径允许替换的前缀。

## 镜像产物契约

- 独立镜像：`.ext4`、`.ext4.manifest.json`、`.ext4.sha256`。
- 叠加镜像：`.ext4`、`.ext4.changes.json`、`.ext4.sha256`。
- staging 内 `/var/lib/wateros/packages.json` 记录 profile、架构、工具链和缓存键。
- 独立镜像固定使用 4096 字节块、256 字节 inode，并启用 EXT4 `64bit` 以生成
  内核 `another_ext4` 后端要求的 64 字节块组描述符。

完整使用说明及安全边界见 [`user/README.md`](../../../user/README.md)。

## 修订

| 日期 | 说明 |
| --- | --- |
| 2026-08-09 | 删除旧用户运行时 API 文档，记录新的构建接口 |
