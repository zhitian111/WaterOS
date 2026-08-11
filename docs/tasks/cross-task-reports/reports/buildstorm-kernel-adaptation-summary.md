# BuildStorm 内核适配工作汇总

## 目标与环境

目标是在不破坏 CAgent 和初赛能力的前提下，使 RISC-V64 多核 WaterOS 能运行
Cargo/rustc 并行构建。验证环境为 QEMU `virt`、8 核、8 GiB、
`os/sdcard-rv-pub.img` 的临时副本；正式镜像未修改。

## 已完成工作

### IPC 与进程生命周期

- `3d1e33c0`：同步清理 `execve` 的 CLOEXEC socket 引用。
- `f4d5bfb1`：支持 Cargo jobserver 使用的 Unix `SOCK_SEQPACKET` I/O。
- `f90fb208`：`exit_group` 先进入 `Exiting`，每个线程在实际退出时单独登记；最后
  一个线程退出前禁止父进程 reap 和销毁地址空间。

### 文件系统与页缓存

- `7bffe932`：修复 another_ext4 同目录 rename 后父目录 link count 错误。
- `e7e0e41d`：目录增加新数据块后持久化 inode size。
- `f7f77481`：rename 前刷回路径脏页，成功后失效新旧缓存；淘汰优先选择干净页，
  避免临时文件回写失败污染共享库缺页；页安装后若被其他 CPU 驱逐则重试，消除
  `expect("page for write")` panic。

### 回归探针

- `5cf76de4`：增加 `os/scripts/guest_buildstorm_parallel_probe.sh`，离线创建 8 个
  crate 并以 `cargo build --workspace -j8` 覆盖 clone/exec/wait/futex、文件 I/O、
  动态库 mmap 和并发页缓存链路。

## 根因链路

并行编译最初出现 rustc 退出码 245、目标文件截断和链接失败。诊断确认共享库按需
缺页读取收到 `VfsError::NotFound`，但库文件本身存在。真正原因是页缓存淘汰旧临时
文件脏页时回写失败，并把该错误返回给无关的共享库读取。修复淘汰策略后又暴露
install/lookup 间的并发驱逐窗口，最终通过重新确认和重试解决。

## 验证结果

- `cargo test --offline -p wateros-vfs-impl-page-cache`：8 项通过。
- `make kernel-rv-final`：通过。
- 并行探针：`BUILDSTORM_PROBE_END rc=0 built=8 elapsed_s=566.82`。
- CAgent：10/10 通过，脚本退出码 0。
- `e2fsck -fn`：五阶段通过，无目录、inode 或引用计数错误。

task 组件 host test 受 `sbi-rt` RISC-V 汇编无法在 x86_64 测试目标编译限制；新增
registry 回归用例已随源码保留，RISC-V 内核构建和完整运行链路通过。

## 剩余工作

当前正确性探针只完成 1/3 轮，仍需两轮稳定性复测。566.82 秒表明性能仍需优化，
应依次测量大 ELF 按需页读、全局 ext4 锁、页缓存索引/淘汰和用户缺页次数。futex
本轮能够正常睡眠、唤醒并完成线程回收，不应在缺少锁竞争数据时先行重构。

官方 `/work/tgoskits` 仍受镜像离线索引阻塞：锁文件需要 `web-sys 0.3.103`，镜像
索引最高为 `0.3.94`。取得依赖一致的镜像后再执行正式 BuildStorm 产物验收。
