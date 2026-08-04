# K-01 another-ext4 扩展写掉电一致性（2026-08-04）

## 问题与修复

8 核并发执行扩展写、rename、truncate、unlink 和 sync 时，强制终止 QEMU 后曾得到：

```text
Inode 22460, i_size is 22, should be 8192.
/work/k01-concurrent-fs/worker-2.tmp
```

`another_ext4::Ext4::write()` 先分配并写入 extent/data，最后才更新 inode size。掉电
可能将 extent 持久化在旧 EOF 之外。适配层新增 `write_with_ordered_size()`：扩展写先
用 `setattr` 提交目标长度并形成顺序边界，再调用 vendor `write()`。因此中断最多留下
合法的稀疏范围。写入错误可能保留已扩展的稀疏长度，这是避免结构损坏的明确取舍。
vendor 和公共 FS/VFS API 均未修改。

## 定向验证

- 配置：RISC-V64/OpenSBI，8 vCPU，8 GiB，TLSF，fresh qcow2 overlay。
- 负载：4 worker x 120 轮扩展写、覆盖 rename、truncate、append、copy/unlink，另有
  80 次并发 sync。
- 正常轮：`K01_FS_CONCURRENCY ok workers=4 iterations=120`，79.767 秒，退出 0；4 个
  最终文件均为 8206 字节并包含第 120 轮尾标记。
- 断电轮：负载启动后 15 秒向 QEMU 发送 SIGTERM；合并 overlay 后 `e2fsck -fn`
  五阶段通过，在途 `.tmp/.live` inode 的 size/block 关系合法。
- `make rv_check`、`make la_check`、`make kernel-rv-final`、`make kernel-la-final` 通过。

## Final BuildStorm

官方 final 队列使用 `gdb-debug` 内核运行，CAgent、toolchain、minibuild 和完整编译均
成功：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1286.27 cores=8 bytes=1681000 arch=riscv64
```

`wateros_debug.py watch` 全程为 `stable=0/6 reason=none`。QEMU 正常退出后：

- `qemu-img check`：无错误；
- `e2fsck -fn`：五阶段通过，仅有 extent tree 可收窄的优化提示；
- 产物 inode 23815：size 1,681,000，blockcount 3288，extent 完整。

另一次 release 轮已完成 axbuild（1265.18 秒）但 7 分钟内未从 `cargo xtask` 返回；
该 overlay 的 ext4 同样干净，且 `/work/.build.rc` 尚不存在。随后基于其产物的
`gdb-debug` 增量探针 71.716 秒退出 0，watch 未发现停滞。该现象作为非确定性进程
生命周期竞态保留，不归因于本次 FS 修复。

## 可复核材料

```text
base_commit: 5ac44aa8316745f65e5686434a7949c228726270 + working-tree fix
base_image_sha256: 83073eb1c5b85def0aba3031300a7c7c3f4594c7a68bfa146ae01d4a076a6abb
normal_log: /tmp/wateros-k01-fixed-clean-v3-20260804.log
normal_log_sha256: b26ab137ab19b2d22e5d368071921c6e83ac6eee9ee650d37daa5f3127636a40
powercut_log: /tmp/wateros-k01-fixed-powercut-v2-20260804.log
powercut_log_sha256: 8a26e476ef5db2ab0e2ec4716b51c05a2000279e8340431cff107b455db8140a
final_gdb_log: /tmp/wateros-k01-final-gdb-gate-20260804.log
final_gdb_log_sha256: 235531f381a049a0c3a64904f58671bb98bf9c4626e1be0e77c509fe6fa40ccc
final_gdb_overlay_sha256: 3f67c8df6f7802782764c6d07a7fd4df0cb75058ecaa174b0fa6690eda1193d4
final_gdb_kernel_sha256: 85b1904cc4002419ad39149de2423dca916f3b2ecff4929fde0e14808882af43
```

基础 raw 镜像在全部测试前后 hash 不变；合并生成的临时 raw 在检查后删除。
