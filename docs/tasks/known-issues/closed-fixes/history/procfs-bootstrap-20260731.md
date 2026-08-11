# bringup bootstrap mount namespace 修复结果

## 问题

bringup 日志显示 procfs 已挂载且内核 self-test 通过，但初赛 LTP 用户进程读取
`/proc/meminfo`、`/proc/cpuinfo` 和 `/proc/sys/kernel/pid_max` 均得到 `ENOENT`。

根因是 bringup 已进入调度环境，原 `mount_procfs_at()` 只修改当前内核任务的挂载
namespace；后来创建的 runner 和首个用户任务却从空的 bootstrap namespace 初始化。
此外 procfs 虽解析了 `/proc/sys/kernel/*` 叶子，却没有 `sys` 与 `sys/kernel` 中间
目录，`meminfo` 和 `cgroups` 还会输出错误的反斜杠与字母 `t`。

## 修改

- 增加显式 bootstrap procfs 挂载入口，bringup 挂载结果可被后续任务继承。
- 将 bringup 的 `/tmp` tmpfs 同样挂入 bootstrap namespace，避免 benchmark 实际落到
  ext4。
- 首个用户任务复制启动它的内核 runner namespace；fork/clone 既有语义不变。
- 补齐 `/proc/sys`、`/proc/sys/kernel` 目录节点及目录枚举。
- 修正 `/proc/meminfo` 的 `Cached` 行与 `/proc/cgroups` 的制表符格式。
- 未修改 task API、scheduler、process registry 或 task 生命周期架构。

## 短验证

```text
date: 2026-07-31
kernel_base_commit: 21c74f928cbe733523e39c3321dca620115c05e8
user_submodule_commit: 2f470f95fa6bf0401c4b1b7ef3bb8fc7a10b870b
architecture: riscv64, 8 CPUs
qemu_and_firmware: QEMU 11.0.2, OpenSBI 1.7
image_sha256: bf418bf5588cc3cb94c8144493e2faec8149bb21aec04a2aa423b47e8767f558
overlay: /tmp/wateros-procfs-bootstrap-v2.qcow2
raw_log_path: /tmp/wateros-procfs-bootstrap-v2.log
raw_log_sha256: 449fa366c8e0e6412e83522aec1fc6aef9e0d5826233f8b8664d6e7b3ab3f126
```

- `make rv_check`、`make la_check`：通过。
- 90 秒上限的 QEMU 用例实际约 0.36 秒关机，命令退出 0。
- `/proc/meminfo` 输出 `MemTotal`、`MemFree`、`MemAvailable`、`Cached`。
- `/proc/cpuinfo` 枚举 CPU 0 至 7。
- `/proc/sys/kernel/pid_max` 输出 `32768`。
- `/proc/cgroups` 输出合法表头与 12 行 controller，最终出现
  `PROCFS_BOOTSTRAP_OK` 和 `all commands finished`。
- 第二个短用例从用户态 `/proc/mounts` 观察到
  `/tmp /tmp tmpfs rw,relatime 0 0`，随后在 `/tmp` 写入并读回
  `TMPFS_IO_OK`；日志 `/tmp/wateros-bootstrap-mounts.log`，SHA-256
  `5f5988ca3f8d9e16128844a495b0636665de337e3e469e51f48bb01c4bcb80bb`。
- 单独运行 musl LTP `creat01` 后不再出现 `/proc/meminfo: ENOENT`；该用例仍因独立的
  `Main test process might have exit` 返回失败，不能据此宣称 `creat01` 已通过。

## 后续门禁

完整 pre 中的所有 LTP 仍须在夜间授权后重跑，统计修复前后的 `TBROK` 数量。final
BuildStorm 不直接依赖本次节点，但会受益于一致的首进程 mount namespace。
