# RIO-10：`read` 调用族集成与回归

## 任务目标

在所有实现任务合入后，使用同一份差分测试验证 errno、数据消费、offset、record
边界、并发和资源生命周期，并完成双架构、LTP、BuildStorm、CAgent 和文件系统完整性
门禁。

## 前置条件

RIO-01 至 RIO-09 全部完成静态检查和各自组件测试。此任务原则上不设计新 API；若发现
根因属于契约缺陷，退回对应任务修复并单独提交。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/prompts/tasks/run_testsuits_qemu.md`
- `docs/tasks/read-family/README.md`
- `docs/tasks/buildstorm-cargo-index-filesystem-report.md`
- RIO-01 至 RIO-09 的任务文档

按失败子系统阅读：

- `docs/exports/features/wateros-syscall.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/exports/features/wateros-mm.md`
- `docs/exports/features/wateros-ipc.md`
- `docs/exports/features/wateros-driver.md`

## 已知信息与代码证据

- Linux 对照测试已经确认：`read(-1, buf, 0)`、pipe 写端读取和
  `readv(-1, [], 0)` 都返回 `EBADF`，目录读取返回 `EISDIR`。
- 对普通文件或 pipe 使用无效用户指针时，Linux 返回 `EFAULT`，并且不推进文件
  offset、不消费 pipe 数据。
- 当前 WaterOS 的历史实现曾存在 4 MiB 读取硬拒绝、先消费后拷贝、OFD offset
  被 `dup`/`fork` 复制，以及 `readv` 大 iovec 直接失败等问题。
- 各项代码证据和建议接口分别记录在 RIO-01 至 RIO-09；本任务不得重新定义一套
  平行协议。

基准程序应保留如下最小 errno 对照形式，具体断言扩展到后文矩阵：

```c
errno = 0;
assert(read(-1, NULL, 0) == -1);
assert(errno == EBADF);
```

## 涉及文件

- `test_case/basic/user/src/oscomp/read_family_semantics.c`（建议新增）
- `test_case/basic/user/src/oscomp/run-all.sh`
- `os/scripts/guest_read_family_regression.sh`（需要 glibc/socket 扩展时建议新增）
- `os/src/user_bringup_busybox.rs`
- `os/Makefile`
- `os/scripts/run/rv_final_run.sh`
- `os/scripts/run/la_final_run.sh`
- `docs/tasks/read-family/regression-report-YYYYMMDD.md`（测试后新增）
- `docs/tasks/buildstorm-cargo-index-filesystem-report.md`

## 任务内容

1. 使用同一组 LTP 源码定义 Linux/WaterOS 读取调用族语义，并新增可选择、可限时的 guest
   runner；不再重复实现一份平行 C 测试。
2. 覆盖 fd、访问模式、用户内存、OFD、普通文件、pipe、socket、eventfd、设备、
   `readv` 和 `pread*` 的语义矩阵。
3. 在单核及至少 8 核 RISC-V 配置下运行竞态测试，并执行现有 final workload。
4. 记录命令、镜像标识、结果和剩余限制，再回填总索引与兼容性文档。

## 测试程序交付

原计划建议增加一份可同时在 Linux 和 WaterOS guest 运行的 C 回归程序。根据实际执行
约束，当前优先复用初赛镜像已有的 LTP 静态二进制及其开源 C 源码，并增加：

```text
os/scripts/guest_read_family_regression.sh
```

该脚本不得提交测试二进制；通过 `LTP_BIN_DIR` 选择现有 LTP 安装，通过
`READ_FAMILY_CASES` 选择短回归或全矩阵，通过 `READ_FAMILY_CASE_TIMEOUT` 设置逐项上限。
测试必须输出稳定的逐项标记：

```text
READ_FAMILY case=<name> ok=true
READ_FAMILY_RESULT passed=<n> failed=0
```

同一源码先在宿主 Linux 运行并保存结果，再在 WaterOS 运行。不要只把预期值硬编码成
WaterOS 特例。

最小测试骨架应直接检查失败后的 source 状态，而不只检查 errno：

```c
write(file_fd, "xyz", 3);
lseek(file_fd, 0, SEEK_SET);
expect_errno(read(file_fd, invalid_ptr, 3), EFAULT);
expect_eq(lseek(file_fd, 0, SEEK_CUR), 0);
expect_bytes(read(file_fd, valid_buf, 3), "xyz", 3);

write(pipefd[1], "abc", 3);
expect_errno(read(pipefd[0], invalid_ptr, 3), EFAULT);
expect_bytes(read(pipefd[0], valid_buf, 3), "abc", 3);
```

## 必测语义矩阵

### fd、访问模式和 errno

- invalid fd：count 0、NULL、普通 buffer 均为 `EBADF`。
- readable fd + NULL + count 0 返回 0。
- `O_WRONLY` regular file 和 pipe write end 返回 `EBADF`。
- directory fd 返回 `EISDIR`。
- `O_PATH` 返回 `EBADF`。

### 普通文件和 OFD

- 首字节 EFAULT 后 offset 为 0，下一 valid read 得到完整内容。
- 跨页 partial fault：返回值、offset 和下一次读取内容与 Linux 一致。
- dup/fork 共享 offset 和 `O_NONBLOCK/O_APPEND`。
- 两次独立 open 的 offset 独立。
- pread/preadv 不改变顺序 offset。

### pipe 与 Unix socket

- EFAULT 后字节/packet 不丢失。
- partial stream fault 只消费已返回前缀。
- O_DIRECT pipe 保持 packet 边界。
- Unix datagram 短 buffer、EFAULT 和 source address 与 Linux 一致。
- reader/writer/close/signal 与 active lease 并发不死锁。

### inet socket

- TCP/UDP 的 read、recv、recvfrom 共用消费顺序。
- TCP partial fault、UDP EFAULT/截断、loopback 与 virtio-net 路径一致。
- nonblocking、EOF、reset 和 signal errno 正确。

### eventfd 与设备

- eventfd 只在完整 8 字节 copy 后减 counter。
- eventfd semaphore/normal、dup/fork、并发 write 正确。
- UART reservation、zero/null/RTC/urandom 语义符合 RIO-08。

### readv/pread

- iovcnt、地址溢出、NULL zero vector、后段 fault、单段 >4 MiB。
- pipe/socket readv 只执行一个逻辑读取事务。
- 大 pread/preadv 没有巨型内核分配和 panic。

## SMP 竞态

至少在 8 CPU 下循环运行：

1. 一个线程 read，另一线程反复 `munmap/mmap/mprotect` 目标页。
2. 多线程通过 dup fd 读取同一文件，验证无重复/缺失字节。
3. 多 reader/writer 操作同一 pipe，随机注入 invalid/cross-page buffer。
4. socket reader 与 close/shutdown/signal 并发。
5. eventfd reader/writer 与 exit_group 并发。

每项至少运行 1,000 次或持续 60 秒；检查无 reservation 泄漏、任务永久阻塞、
allocator 警告、UAF、重复提交和 spin lock 卡死。测试结束后所有 waiter/lease/OFD
数量回到基线。

## 如何验收

以下构建、运行、性能和资源门禁必须全部满足。任何失败都要保留原始日志和最小复现，
不能只记录最终退出码。

### 构建与运行门禁

```bash
cd os
make rv_check
make la_check
make kernel-rv-final
make kernel-la-final
```

若 Makefile 当前没有 `kernel-la-final`，使用仓库实际 LoongArch final 目标并在报告中
记录，不能绕过 Makefile裸跑 cargo/qemu。

运行门禁：

- [ ] RISC-V64 定向回归全部通过。
- [ ] LoongArch64 定向回归全部通过。
- [ ] LTP `open09`、`pipe03`、readv/preadv/eventfd 相关用例通过。
- [ ] basic/busybox/iozone/lmbench 无新增回归。
- [ ] `cargo metadata --offline` 成功，`web-sys` 索引 hash 不变。
- [ ] 完整输出 `BUILDSTORM_COMPILE mode=multi ok=true`。
- [ ] CAgent 连续三轮 10/10。
- [ ] 测试后 overlay 转 raw，`e2fsck -fn` 五阶段通过。
- [ ] 原始镜像 SHA-256 前后不变。

### 性能与资源检查

- 大 regular read 仍允许一次最多 4 MiB 短读，不恢复原 `EINVAL`。
- 不允许每个 iovec、每页或每次 socket poll 打串口日志。
- 比较修改前后 BuildStorm、iozone 和 pipe throughput；正确性修复造成明显退化时先
  定位锁持有、staging copy 和 reservation wait，不删除语义保证。
- 记录最大 staging allocation、active lease 数和 cancel 次数；测试结束均归零。

### 结果文档

新增：

```text
docs/tasks/read-family/regression-report-YYYYMMDD.md
```

必须包含 commit、两个架构、QEMU 参数、镜像 hash、每条命令、Linux/WaterOS 差分表、
原始日志路径、第一个失败点、性能变化和未解决限制。

更新：

- `docs/tasks/buildstorm-cargo-index-filesystem-report.md`
- `docs/tasks/read-family/README.md` 中的最终状态

日志、内核二进制、overlay、修改后的镜像和测试生成物不得提交。

## 失败处理

- errno/access 失败退回 RIO-01。
- 部分 copied 数错误退回 RIO-02。
- dup/fork offset/status 失败退回 RIO-03。
- offset/lease Drop 失败退回 RIO-04。
- pipe/Unix、inet、eventfd、device 分别退回 RIO-05 至 RIO-08。
- readv/pread 失败退回 RIO-09。

不要在集成任务中加入 source-specific syscall 特判掩盖下层契约错误。

## 搜索范围、并行与回填

测试前用 `rg "READ_FAMILY|sys_read|sys_readv|sys_pread|prepare_read|VfsReadLease"` 确认
新旧入口，检查 syscall 可达句柄没有继续走破坏性 fallback。用 `git diff --check` 和
`git status --short` 审核提交范围。

Linux 与两个 WaterOS 架构可在独立工作目录/overlay 并行运行；同一镜像基线不能被两
个 QEMU 写进程共享。原始日志放 `/tmp/wateros_read_family/`，仓库只提交测试源码和
`regression-report-YYYYMMDD.md`。

全部门禁通过后：

- 在 `docs/tasks/read-family/README.md` 勾选 RIO-10；
- 回填 BuildStorm 报告和阶段 0 文档；
- 记录最终 commit、镜像 hash、命令和日志位置；
- 删除正式代码中的临时 trace/counter，保留 feature-gated 诊断能力。
