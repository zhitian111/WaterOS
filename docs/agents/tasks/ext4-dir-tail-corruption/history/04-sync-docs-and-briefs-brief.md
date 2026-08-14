# 任务 04 简报：文档同步与任务简报归档

## 完成情况

- 任务目录 README 更新为“01/02 已完成、03 fs 模式已完成、04 进行中”的状态；
- `os/scripts/README.md` 补充 `regress_ext4_dir_tail.sh` 入口；
- 任务 01/02/03 简报均已归档到 `history/`；
- 提交：`fc11239f [docs] 归档 ext4 目录尾损坏专项简报并同步文档`

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
19d5060d [docs] 建立 ext4 目录尾损坏专项任务计划
096df78a [fix] another_ext4 目录项禁止占用 checksum tail
9485764c [docs] 记录任务01简报
6749a6c0 [test] another_ext4 新增目录块 tail 边界 host 回归
e2598a59 [docs] 记录任务02简报
90b64543 [test] 新增 ext4 目录尾损坏的 QEMU 回归脚本（fs 模式）
4db85ca1 [docs] 记录任务03简报
fc11239f [docs] 归档 ext4 目录尾损坏专项简报并同步文档
```

## 最终状态

- 根因（`DirBlock` 把 12 字节 checksum tail 当作目录项空间）已修复；
- host 侧单元测试与完整文件系统回归通过；
- QEMU fs 模式通过（guest 构造 tail 边界 + 宿主 e2fsck 干净）；
- apt/dpkg 全量路径仍被 main 分支的 `unlockpt`、文件 seek、管道读 syscall 缺口
  阻断，已在任务 03 文档与简报中记录，不属于本专项代码改动。
