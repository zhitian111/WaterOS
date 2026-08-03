# K-10 `access02` 与 `O_TRUNC` 权限修复报告（2026-08-04）

## 现象与定位

LoongArch-musl LTP 基线中 `access02` 的 4 个 `X_OK` 检查失败。LTP 20240524
`access02.c` 用 `0555` 创建 `file_x`，再通过 stdio 写入 `#!/bin/sh`；写入已存在文件
不应修改 inode 权限。

定向探针确认：修复前 `chmod 0555` 后 mode 为 `0555`，shell 以
`O_CREAT|O_TRUNC` 重定向写入后却变成 `0644`。`faccessat` 的判断与返回值正确，错误
来自 VFS truncate fallback。

## 根因与修改

`PagedFileHandle::open()` 在后端不支持 stable-node API 时，用
`replace_file_contents(path, &[])` 实现 `O_TRUNC`。ramfs 的 replace 接口会创建新的
`Node::file`，默认 mode 为 `0644`，同时改变 inode 身份。这违反了 `open(2)` 对已存在
文件只截断内容、保留 mode/owner/inode 的语义。

- `paged_handle.rs`：fallback 改为 `truncate_path(path, 0)`，原地截断后端文件。
- `user_bringup_root_layout.rs`：在既有 BusyBox applet 链接表加入 `sh`，补齐
  `/bin/sh`。否则权限通过后，LTP shebang 会因解释器不存在返回 `ENOENT`。

## 验证

- LoongArch64/QEMU 与 RISC-V64/OpenSBI，均为 8 CPU、musl LTP：`access02` 各
  16 PASS、0 FAIL，退出码 0。
- LoongArch 探针在 truncate+write 前后均为
  `mode=555 inode=2 uid=0 gid=0`，脚本执行成功。
- RISC-V 测试镜像修改后通过 `e2fsck -fn`；两架构均以 snapshot 模式启动。
- `make rv_check`、`make la_check` 和两架构 LTP-musl 内核构建通过，仅有既存 unused
  警告。

日志：`/tmp/wateros-access02-la-final-inode.log`（SHA-256
`09a16814048555fe177d1af008ecb08b4296b911d6c4762df53b2cf70c8045c5`）与
`/tmp/wateros-access02-rv-final.log`（SHA-256
`aec59c6338e467c7c39c524d1c4edac809d1dd54e69cc3813b9da15ed447bffd`）。

该修复关闭 `access02` 回归，但不代表 K-10 全部初赛 LTP 用例已完成。
