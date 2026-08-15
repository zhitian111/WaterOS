# 任务 04 简报：文档同步与任务简报归档

## 完成情况

- 任务目录 README 更新为“01/02/03/04 均已完成”的状态；
- `os/scripts/README.md` 补充 `regress_ext4_dir_tail.sh` 入口；
- 任务 01/02/03 简报均已归档到 `history/`；
- 提交：`c977a22c [docs] 归档 ext4 目录尾损坏专项简报并同步文档`

## 修改文件

- `docs/agents/tasks/ext4-dir-tail-corruption/README.md`
- `os/scripts/README.md`

## 验收结果

```text
git diff --check        # 通过
相对链接检查            # 无失效链接
git log --oneline       # 见下方提交序列
```

## 专项提交序列（fix/ext4-dir-tail-corruption）

```text
35843422 [docs] 建立 ext4 目录尾损坏专项任务计划
9f4ddd9e [fix] another_ext4 目录项禁止占用 checksum tail
b9e75116 [docs] 记录任务01简报
802ee817 [test] another_ext4 新增目录块 tail 边界 host 回归
18a71ac0 [docs] 记录任务02简报
bbf16a37 [test] 新增 ext4 目录尾损坏的 QEMU 回归脚本（fs 模式）
680f7720 [docs] 记录任务03简报
c977a22c [docs] 归档 ext4 目录尾损坏专项简报并同步文档
12fb068b [docs] 记录任务04简报
d053e312 [docs] 记录 apt 阻断的跨分支核查证据
d6dab555 [docs] 记录 apt 模式回归通过
09684077 [refactor] 移除 another_ext4 新增的 #[test] 边界单元测试
```

## 最终状态

- 根因（`DirBlock` 把 12 字节 checksum tail 当作目录项空间）已修复；
- host 侧单元测试与完整文件系统回归通过；
- QEMU fs 模式通过（guest 构造 tail 边界 + 宿主 e2fsck 干净）；
- 合入远端 `github/main` 的 syscall 修复并 rebase 后，apt 模式也通过：
  `apt-get install neovim-runtime` 返回 0，`syntax/vim/generated.vim` 存在，
  `e2fsck -fn` 干净。
- neovim 可运行：`nvim v0.10.4`，`nvim --headless +q` 退出码 0。
- 按 main 分支要求，未在 `another_ext4` 内保留新增的 `#[test]`；边界回归由
  host 回归与 QEMU 回归覆盖。
