# ext4 目录块校验尾损坏：任务计划

本目录承载 `neovim-runtime` 解包损坏专项的工作计划。该专项使用独立分支
`fix/ext4-dir-tail-corruption` 与独立工作树
`/tmp/WaterOS_ext4_dir_tail_fix`，避免与主工作树及其它并行任务互相污染。

## 目标

修复 `another_ext4` 在目录块末尾 12 字节 `DirEntryTail` 区域写入普通目录项，
导致目录项名被 CRC 覆盖、目录 checksum 失效的问题。该问题表现为：

```text
syntax/vim/generated.vim.dpkg-new ENOENT
清理时 Directory not empty
find 看到 \3232\032、:\t\220 等乱码目录项
```

## 分支与工作树

- 分支：`fix/ext4-dir-tail-corruption`
- 工作树：`/tmp/WaterOS_ext4_dir_tail_fix`
- CodeGraph：已在本工作树执行 `codegraph init`（索引 591 个文件；`vendor/` 与
  `target/` 未进入索引，vendor 定位用 `rg` 回退）

## 任务顺序

每个任务对应一个可回归、可验收的提交。完成后在
`docs/agents/tasks/ext4-dir-tail-corruption/history/` 写对应简报。

1. [01 修复 DirBlock 目录块校验尾被覆盖](./01-fix-dirblock-tail-reserve.md)
2. [02 新增 host 侧目录块边界回归](./02-add-host-dir-tail-regression.md)
3. [03 QEMU apt/dpkg 端到端回归](./03-qemu-apt-dpkg-regression.md)
4. [04 文档同步与任务简报归档](./04-sync-docs-and-briefs.md)

## 测试镜像

优先复用仓库里已经解压的镜像，避免重复解压：

- `/home/zhitian/project/WaterOS_refactor/os/sdcard-rv.img`
- `/home/zhitian/project/WaterOS_refactor/test_case/sdcard-rv.img`
- `/home/zhitian/project/WaterOS_refactor/test_case/sdcard-la.img`

若需要带 apt/dpkg 的 pub 镜像，再解压：

- `~/Downloads/sdcard-rv-pub.img.gz`
- `~/Downloads/sdcard-la-pub.img.gz`

解压前确认目标分区磁盘空间；pub 压缩包约 2.1GB，解压后约 4GB。

## 运行约束

- 运行 QEMU 时一律使用 `-snapshot`（或等价的 qcow2 overlay），禁止直接写穿基准
  镜像；回归脚本也必须自证没有修改基准镜像。
- 除非确有必要，不要全量阅读 QEMU 日志或 `make` 构建日志；优先用
  `rg -n "关键词"`、`tail -n` 和 `grep -C` 取关键片段，避免占用上下文和输出。

## 任务简报

每个任务完成后，必须写一份简报到
`docs/agents/tasks/ext4-dir-tail-corruption/history/<任务序号>-<任务名>-brief.md`，
简要说明当次任务完成情况，至少包含：

- 实际修改的文件与提交 hash；
- 执行的验收命令和结果；
- 未验证项、剩余风险或环境限制。

简报不允许替代任务文档，也不允许把未验证的结论写成已验证。

## 代码草案

任务 01 的核心代码草案已从诊断会话拷入本工作树
`os/vendor/another_ext4/src/ext4_defs/dir.rs`，当前处于未提交状态。开始实施时
先按任务 01 文档核对、验证，再提交；不要直接提交未经验证的草案。
