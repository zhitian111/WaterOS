# 任务 03：QEMU apt/dpkg 端到端回归

## 状态

待实施。

## 目标

在 RISC-V QEMU 中真实执行 Debian apt/dpkg 的 `neovim-runtime` 解包路径，
验证任务 01 的修复消除了：

```text
syntax/vim/generated.vim.dpkg-new ENOENT
清理时 Directory not empty
find 看到乱码目录项
```

并把回归步骤固化为可复用的脚本。

## 测试镜像

优先使用仓库已解压镜像：

```sh
ls -lh /home/zhitian/project/WaterOS_refactor/os/sdcard-rv.img
ls -lh /home/zhitian/project/WaterOS_refactor/test_case/sdcard-rv.img
```

若确认需要 apt/pub 镜像，再解压：

```sh
gzip -dc ~/Downloads/sdcard-rv-pub.img.gz > /tmp/wateros-apt-rv.img
```

注意：必须先确认目标分区磁盘空间；压缩包约 2.1GB，解压后约 4GB。解压前先
`df -h /tmp`。

## 涉及文件

建议新增一个回归脚本，避免手工命令漂移：

- `os/scripts/regress_ext4_dir_tail.sh`

该脚本负责镜像副本/overlay 准备、QEMU 启动、guest 内 apt/dpkg 命令下发、
日志落盘、退出后宿主 `e2fsck -fn`。脚本必须只读/写副本，绝不修改唯一基准镜像。

## 实施方案

1. 在 `os/scripts/regress_ext4_dir_tail.sh` 中封装：

   - 选择镜像：`RV_IMG` 可覆盖，默认优先 `os/sdcard-rv.img`，其次解压 pub 镜像；
   - 制作副本或 qcow2 overlay，保护基准镜像；
   - 启动 QEMU 时必须带 `-snapshot`，保证即使脚本中途失败也不会写穿基准镜像；
   - 进入 guest 后执行：

     ```sh
     apt-get update
     apt-get install -y neovim-runtime
     # 若安装被既有状态阻断，则：
     dpkg --configure -a
     # 观察 syntax/ 目录枚举
     find /usr/share/vim/vim*/syntax -maxdepth 1 -type d
     ```

   - 收集内核/QEMU 日志到 `tem/`；
   - 关机后对镜像副本执行 `e2fsck -fn`。

   日志策略：不要全量阅读 QEMU 日志或 `make` 构建日志；只用
   `rg -n "error|FAIL|generated.vim|dpkg-new|ENOENT|Directory not empty|illegal characters"`
   和 `tail -n` 定位关键片段，必要时再取上下文。

2. 关键验收点：

   - 无 `generated.vim.dpkg-new ENOENT`；
   - `find` 输出无非法目录项名；
   - `dpkg --configure -a` 返回 0；
   - `e2fsck -fn` 通过。

3. 提交脚本，提交信息：

   ```text
   [test] 新增 ext4 目录尾损坏的 QEMU apt 回归脚本
   ```

## CodeGraph 查询命令

```sh
codegraph explore "sys_execve sys_wait4 getdents64"
codegraph node "os/scripts/run_phase_tests.sh"
codegraph files
```

索引不可用时回退：

```sh
rg -n "rv_final_run|qemu-system-riscv64|sdcard-rv" os/Makefile os/scripts
```

## 验收命令

```sh
cd /tmp/WaterOS_ext4_dir_tail_fix/os
bash scripts/regress_ext4_dir_tail.sh
```

验收标准：

- 脚本退出码为 0；
- 日志中无上述错误文本；
- 宿主 `e2fsck -fn` 五阶段通过；
- 基准镜像未被修改（脚本使用副本/overlay）。

## 完成后简报

写 `history/03-qemu-apt-dpkg-regression-brief.md`，记录镜像路径、QEMU 命令、
guest 输出摘要和 e2fsck 结果。
