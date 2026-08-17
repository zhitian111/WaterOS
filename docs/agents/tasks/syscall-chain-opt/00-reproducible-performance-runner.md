# 任务 00：固化可复现的 QEMU 9.2.1 性能运行入口

## 任务内容与目标

新增一个薄的 BuildStorm 性能运行脚本，固定 QEMU 9.2.1、8 核、16 GiB、设备参数和
`-snapshot`，同时记录 kernel/image hash 与日志目录。本任务不改内核行为，后续所有 A/B
都使用这一入口，避免命令漂移和误写基准镜像。

## 实施方案

1. 在 `os/scripts/` 新增 runner，只组装并 `exec` README 中的规范命令。
2. QEMU 路径必须是绝对路径，不允许回退到 `PATH`；启动前校验版本为 9.2.1。
3. 强制检查 `-snapshot`，记录完整 argv、git commit、kernel/image SHA-256 和 UTC 时间。
4. 支持 `--dry-run`、`--kernel`、`--log-dir`，但默认设备拓扑和 SMP 不可静默改变。
5. 同步 `os/scripts/README.md` 与必要的 `docs/tools/` 入口。

## 涉及文件

- 新增 `os/scripts/run_syscall_chain_perf.sh`
- `os/scripts/README.md`
- 需要时更新 `docs/tools/README.md`

## CodeGraph / 检索

```bash
codegraph files -p . --filter os/scripts --format tree --no-metadata
rg -n "qemu-system-riscv64|-snapshot|kernel-rv" os/scripts docs/tools
```

## 验收方式

```bash
os/scripts/run_syscall_chain_perf.sh --dry-run
/home/zhitian/qemu_9_2_1/qemu-9.2.1/build/qemu-system-riscv64 --version
bash -n os/scripts/run_syscall_chain_perf.sh
git diff --check
```

dry-run 输出必须与 README 的规范命令等价并包含 `-snapshot`；缺少指定 QEMU、kernel 或
镜像时须明确失败。实际启动一次并确认日志元数据完整，但本任务不要求跑完整 BuildStorm。

## Commit 与简报

提交建议：`[tools] 固化 syscall 链路性能验收入口`。完成后新增
`history/00-brief.md`，记录实际 QEMU 版本、dry-run 和启动检查结果。
