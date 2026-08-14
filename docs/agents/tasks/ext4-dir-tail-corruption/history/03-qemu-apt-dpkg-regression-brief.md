# 任务 03 简报：QEMU 端到端回归

## 完成情况

- 新增 `os/scripts/regress_ext4_dir_tail.sh`，提供 `REGRESS_MODE=fs`（默认）与
  `REGRESS_MODE=apt`（可选）两种模式。
- fs 模式已在 RISC-V QEMU 上通过：guest 创建 360 个 4 字符文件与 `vim` 子目录，
  输出 `REG DIR TAIL FSCK PASS`；overlay 经 `e2fsck -fy` 后 `e2fsck -fn`
  五阶段全部通过。
- 提交：`90b64543 [test] 新增 ext4 目录尾损坏的 QEMU 回归脚本（fs 模式）`

## 修改文件

- `os/scripts/regress_ext4_dir_tail.sh`
- `docs/agents/tasks/ext4-dir-tail-corruption/03-qemu-apt-dpkg-regression.md`

## 验收结果（fs 模式）

```text
guest: #### REG DIR TAIL FSCK PASS ####
e2fsck -fy: Pass 1..5 通过
e2fsck -fn: Pass 1..5 通过
```

说明：pub 工作副本解压后自带 journal 陈旧事务与空闲计数偏差，首次使用前已对
工作副本执行一次 `e2fsck -fy` 清理；guest 写盘使用 qcow2 overlay，基准镜像未被
写穿。

## apt 模式：已知阻断

`REGRESS_MODE=apt` 复现了原始 `apt-get install neovim-runtime` 路径，但 main
（本分支基座 `59f50c44`）缺少三类 syscall 兼容，apt 在解包阶段提前中止：

```text
unlockpt (22: Invalid argument)                    # PTY/TIOCSPTLCK
cannot skip padding ... failed to seek (EOPNOTSUPP) # 普通文件 seek
grep: (standard input): Operation not supported     # 管道读
```

队友声称的 `unlockpt/ptsname` 与 `lseek ESPIPE` 修复不在当前分支，需要先合入
对应 syscall 修复，apt 模式才能作为本任务验收项。

## 未验证 / 剩余风险

- apt/dpkg 端到端仍被上述 syscall 缺口阻断；
- 未执行 LoongArch64 端到端回归（RISC-V 已覆盖）。
