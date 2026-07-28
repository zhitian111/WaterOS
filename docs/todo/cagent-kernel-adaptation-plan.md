# CAgent 内核适配推进计划

本计划以 `cagent-kernel-adaptation-tasks.md` 为需求基线。工作按条目进行：
每条只解决一个根因，完成验证后立即单独 commit，再进入下一条。

## 一、测试与基线

### CAG-BASE-01：建立可重复运行口径

- 使用 `os/sdcard-rv-pub.img` 的 qcow2 overlay，避免修改基准镜像。
- CAgent 必须先于 BuildStorm 运行，单次上限 60 秒。
- 从日志提取十项结果、HTTP `Request:` 数量、GROUP END、panic 和异常 trap。
- 保存当前基线：7/10 pass、17/20 请求，三个失败均接近 timeout。

本条只产生文档和日志，不修改正式测试脚本或镜像。

## 二、网络组件

### CAG-NET-01：兑现 TCP listen backlog

涉及文件：

- `os/components/wateros-driver/driver-network/src/lib.rs`

实施：

1. 将监听槽安全上限提高到能够覆盖 CAgent 的 backlog 10。
2. 保持 `backlog.clamp(1, max)`，避免无界内核内存分配。
3. 若任一附加槽分配失败，移除本次已经创建的附加槽并恢复主 socket 元数据，
   不能留下不可关闭的 listener group。
4. 增加纯状态/辅助函数单元测试；内核运行作为并发语义主验收。

验收：

- `make kernel-rv-final`
- CAgent 连续至少 3 轮，每轮 20 次请求、10/10 pass、GROUP END。
- 无 socket close 警告、panic、超时和持续堆增长。

建议提交：`[fix] honor tcp listen backlog for cagent`

### CAG-NET-02：消除本地 TCP 长尾

仅当 CAG-NET-01 后仍存在请求缺失或接近 timeout 时执行。

涉及文件：

- `os/components/wateros-driver/driver-network/src/lib.rs`
- 必要时 `os/components/wateros-syscall/.../socket_block.rs`

实施优先级：

1. 在 connect/accept/close 的状态转换点执行有上限的 burst poll。
2. 若仍不足，再引入按 socket 状态等待与定向唤醒。
3. 禁止无限 busy loop；每次 burst 必须有固定轮数上限。

验收除 10/10 外，还要求三轮最长单项耗时不接近脚本 timeout。

建议提交：`[fix] drive loopback tcp state transitions promptly`

## 三、Task、信号与进程

### CAG-TASK-01：后台任务与 server 生命周期回归

核对 fork/exec、pipe、SIGCHLD、wait 指定 PID、timeout 的 SIGTERM 和最终 kill。
重点验证：

- 十个后台 shell 都被回收；
- `wait "${TEST_PIDS[@]}"` 不等待 server；
- kill server 能中断 accept，脚本打印 GROUP END；
- 不出现僵尸任务、`sigsuspend` 热循环或 fd 泄漏。

若无失败只记录验证，不产生空提交；若发现问题，修改 task/signal/syscall 对应组件，
每个根因独立提交。

## 四、系统调用与 VFS

### CAG-SYS-01：十条命令逐项验收

分别运行服务端实际生成的命令，确认：

- `uname -r`、`nproc`、`date` 输出有效；
- `ss -tan` 能结束并经管道产生数字；
- create/read/write/mkdir/touch/getdents/find/statfs 成功；
- `utimensat` 支持 glibc 的 `pathname == NULL` fd 模式。

若并发全测失败但单项成功，优先回到网络或 task，不重复修改 VFS。
文件系统根因交对应成员，并提供最小复现、errno 和涉及路径。

## 五、完整验收与初赛回归

### CAG-REG-01：决赛验收

- 使用 8 核、8 GiB、OpenSBI 默认 BIOS 和 final 镜像 overlay。
- 连续 3 轮 CAgent 10/10 pass。
- 每轮有 20 次 HTTP 请求和 GROUP END。
- 日志无 panic、非法 trap、长时间无 syscall 进展或资源持续增长。

### CAG-REG-02：初赛稳定性

- 构建 `make kernel-rv-pre`。
- 使用 `test_case/sdcard-rv.img` 的 overlay 运行初赛命令。
- 至少覆盖 BusyBox、LTP 入口和已有内核 self-test。
- 判定门槛：无 panic、崩溃、trap 循环和永久无进展卡死。

## 六、结果文档

全部条目结束后新增汇总文档，记录：

- 每个问题的根因、改动文件和 commit；
- final/pre 的准确命令、日志和结果；
- 未修改的测试侧缺陷与文件系统成员负责项；
- 已知限制和后续性能优化建议。

建议提交：`[docs] summarize cagent compatibility work`
