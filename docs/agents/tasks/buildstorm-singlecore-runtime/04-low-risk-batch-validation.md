# BS-SC-04：低风险批次 A 统一验收

## 任务内容

对任务 01--03 的累计结果执行一次双架构功能与性能验收。本任务原则上只新增简报和必要的
验收脚本修正；若发现实现缺陷，应先定位到对应提交并修复或回退，再形成本提交。

## 实施方案

1. 固定任务 03 完成后的候选 HEAD、双架构 ELF 和 CodeGraph 影响面。
2. 先运行静态检查和定向 probe；任一功能失败时停止性能测试。
3. 功能通过后各架构运行一次完整 BuildStorm，按总 README 阈值决定是否补跑。
4. 将结果、日志哈希和保留/回退决定写入简报后形成验收 commit。

## 验收方式

### 功能验收

1. 双架构 check 和 Final build 全部通过。
2. RISC-V FPU probe：固定 FPR 位型跨 getpid/read/timer/yield 后保持，至少 10,000 轮。
3. 双架构 signal/timer probe：SIGKILL、SIGSTOP/SIGCONT、handler+rt_sigreturn、阻塞 syscall
   EINTR、ITIMER_REAL、ITIMER_VIRTUAL/PROF、POSIX timer 各至少一条成功路径。
4. BuildStorm 的 cagent、toolchain、minibuild、正式编译和产物启动全部成功；串口日志在最终
   result marker 前无 panic、SIGSEGV 或 watchdog timeout。

若 pub 镜像中没有现成 probe，使用任务 00 的镜像准备器创建独立功能镜像，将单文件 shell
probe 覆写到 `/glibc/buildstorm_testcode.sh`；不得污染性能母盘。

### 性能验收命令

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
python3 scripts/perf/buildstorm_runner.py --arch rv --kernel ./kernel-rv-final \
  --image tem/perf/buildstorm-singlecore/images/sdcard-rv-pub-prepared.img \
  --run-id bs-sc-04-rv-a1 --timeout 1800
python3 scripts/perf/buildstorm_runner.py --arch la --kernel ./kernel-la-final \
  --image tem/perf/buildstorm-singlecore/images/sdcard-la-pub-prepared.img \
  --run-id bs-sc-04-la-a1 --timeout 1800
```

结果按总 README 的 `<500/500..520/>=520` 规则处理。任何一架构 `>=520s` 都要记录各项
诊断计数；低风险批次可以暂时保留，但不得宣称已通过最终性能验收。

## 涉及文件

- `docs/agents/tasks/buildstorm-singlecore-runtime/history/04-brief.md`
- 必要时 `os/scripts/perf/guest-probes/` 中的批次功能 probe
- 仅当 runner/parser 存在真实缺陷时修改任务 00 的工具文件及测试

## CodeGraph 查询

```bash
codegraph affected os/src/trap_handler.rs \
  os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs \
  os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/asm/trap.asm
codegraph explore "setitimer posix timer rt_sigreturn stop continue"
```

## 完成后简报

新增 `history/04-brief.md`，汇总任务 01--03 的 commit、功能 probe 输出、两个 `result.json`
路径与哈希、guest elapsed、保留/回退决定，以及进入 signal 批次前的当前最佳记录。
