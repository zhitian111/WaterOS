# 任务 04：文档同步与任务简报归档

## 状态

待实施。

## 目标

收尾本专项：把前三项任务的验证结论同步到项目文档，归档各任务简报，确保
README 与当前代码事实一致，且不把未验证结论写成已验证。

## 涉及文件

按 AGENTS 第 10 节触发条件核对并更新：

- `docs/agents/tasks/ext4-dir-tail-corruption/history/*.md`
- `docs/tasks/cross-task-reports/reports/` 中与本专项相关的报告（如有）
- 根目录或 `os/README.md` 中描述 ext4 RW 目录持久化/限制的段落
- 如新增了 `os/scripts/regress_ext4_dir_tail.sh`，同步 `os/scripts/README.md`

本任务不产生 vendor 代码改动，只提交文档与简报。

## 实施方案

1. 为任务 01/02/03 各写一份简报到
   `docs/agents/tasks/ext4-dir-tail-corruption/history/`。
2. 检查并修复本专项新增/修改路径的 Markdown 相对链接。
3. 核对 README 中“部分兼容 Linux generic64 ABI / ext4 RW”等能力描述：

   - 已通过 QEMU 验证的才写“已验证”；
   - 未跑 QEMU 或未跑 e2fsck 的项写“未验证/待验证”。

4. 运行：

   ```sh
   git diff --check
   ```

5. 提交，提交信息：

   ```text
   [docs] 归档 ext4 目录尾损坏专项简报并同步文档
   ```

## CodeGraph 查询命令

```sh
codegraph files
codegraph explore "another_ext4 DirBlock DirEntryTail"
```

索引不可用时回退：

```sh
rg -n "another_ext4|ext4|目录|neovim" docs os/README.md README.md
```

## 验收命令

```sh
cd /tmp/WaterOS_ext4_dir_tail_fix
git diff --check
git status --short
git log --oneline -5
```

验收标准：

- 简报齐全，且与各自任务文档的验收结论一致；
- 所有相对链接可解析；
- `git diff --check` 干净；
- 未把未验证结论写成已验证。

## 完成后简报

本任务完成后，在 `history/04-sync-docs-and-briefs-brief.md` 写最终收尾简报，
汇总整个专项的分支、提交序列和最终状态。
