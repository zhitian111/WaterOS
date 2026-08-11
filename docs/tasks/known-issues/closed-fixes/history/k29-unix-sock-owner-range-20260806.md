# K-29 `unix_sock` owner range 查询（2026-08-06）

## 问题

AF_UNIX 的全局 `FD_TABLE` 使用 `BTreeMap<(task_id, fd), UnixSockRef>`，fork 继承和
task 退出清理此前用 `iter().filter(|(owner, _)| ...)` 扫描整个表，复杂度随全局
socket 数增长。

## 修改

`copy_fds_from_parent` 和 `drop_task` 改为 `range((owner, 0)..=(owner, usize::MAX))`，
只处理指定 task 的 fd，不遍历无关任务。

## 验证

```text
make rv_check
make la_check
make kernel-rv-final
make kernel-rv-pre
```

Final smoke 两次：一次 9/10（factorial 偶发 reject，脚本退出码 0），重跑 10/10；
`socketpair` self-test 通过，无 panic。

完整 Final：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1690.74 cores=8 bytes=1681000 arch=riscv64
#### OS COMP TEST GROUP END buildstorm-glibc ####
```

Pre 可行性（`sdcard-rv.img`，60 秒）：进入 hackbench/cyclictest，无 panic 和 ext4
读块错误。

说明：该改动消除了全局扫描路径，但完整 BuildStorm 耗时为 1690.74s，与本轮其它完整
运行在 1567-1690s 区间，仍属于宿主噪声范围内，未达到 700-800s 目标。
