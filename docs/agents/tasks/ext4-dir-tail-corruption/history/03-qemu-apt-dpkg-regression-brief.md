# 任务 03 简报：QEMU 端到端回归

## 完成情况

- 新增 `os/scripts/regress_ext4_dir_tail.sh`，提供 `REGRESS_MODE=fs`（默认）与
  `REGRESS_MODE=apt`（可选）两种模式。
- fs 模式已在 RISC-V QEMU 上通过：guest 创建 360 个 4 字符文件与 `vim` 子目录，
  输出 `REG DIR TAIL FSCK PASS`；overlay 经 `e2fsck -fy` 后 `e2fsck -fn`
  五阶段全部通过。
- apt 模式在合入远端 `github/main` 的 syscall 修复后通过：`apt-get install
  neovim-runtime` 返回 0，`syntax/vim/generated.vim` 存在，`dpkg --configure -a`
  返回 0，overlay `e2fsck -fn` 五阶段干净。
- neovim 运行：`apt-get install -y --no-install-recommends neovim` 返回 0，
  `nvim --version` 输出 `NVIM v0.10.4`，`timeout 15 nvim --headless +q`
  退出码为 0（guest 输出 `NVIM RUN PASS`）。
- 提交：`bbf16a37 [test] 新增 ext4 目录尾损坏的 QEMU 回归脚本（fs 模式）`

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

## apt 模式：已解决

合入远端 `github/main`（提交 `3bf7ee0f`、`d1713ddb`、`39f12e99`、`c0bd9c6b`）
并 rebase 本分支后，apt 模式回归通过：

```text
apt install rc=0
apt neovim install rc=0
nvim headless rc=0
#### NVIM RUN PASS ####
dpkg configure rc=0
#### REGRESS DIR TAIL PASS ####
e2fsck -fn: Pass 1..5 通过
```

## 未验证 / 剩余风险

- 未执行 LoongArch64 端到端回归（RISC-V 已覆盖）。
