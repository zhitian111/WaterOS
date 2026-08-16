# Task 05a 简报：闭合 `exit_group` 与 sibling 入睡竞态

## 完成情况

BuildStorm 偶发在 `[axbuild] ... done` 后永久等待的根因是一次 lost interrupt：退出 CPU 已将
进程发布为 `Exiting`，但 sibling 仍处于 `Running`；旧 `interrupt_task` 当时找不到 wait
容器项，只发送一次重调度。若 sibling 在 IPI 生效前进入 pipe、futex 或 sleep 等等待，它会
在通知之后入队，且没有第二次唤醒，遗留的 pipe writer 使 shell 的 `tee` 永远收不到 EOF。

本次在 TCB 登记仅供 `exit_group` 使用的 sticky wait interrupt。登记、检查和 wait/sleep 入队
都在 scheduler 全局锁内串行化：已经 Blocking/Sleeping 的任务立即从 WaitQueues 摘除并以
`Interrupted` 唤醒；Running 任务收到重调度请求；Ready 或随后尝试阻塞的任务在入队前返回
`Interrupted`。标记持续到 TCB 回收，避免 syscall 内部吞掉一次中断并再次入睡。sibling 仍在
自己的 CPU 上展开 syscall 栈和运行析构，不恢复远端强杀或跨 CPU 提前释放 FD、futex、pipe
lease 等资源。

## 验证结果

最终候选内核：

```text
3e77275c53d5478e1ca63499c7f70fc4acbb703f0f665caabb3fdfef9260a356  kernel-rv-final
707776fb2f72459312e9163a2b56fa71c0cf34ba4247f98631d952d232811498  kernel-la-final
```

静态与构建门禁：

```text
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check          PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check          PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final   PASS
git diff --check                                        PASS
```

独立 host `cargo test` 无法形成有效门禁：不选择 arch feature 时缺少活动实现；显式启用
`arch/impl-riscv64` 后，宿主 x86_64 编译 `sbi-rt` 因 RISC-V 寄存器不可用而失败。真实 RV/LA
目标 check 与 Final build 均已覆盖改动代码。

专项探针使用 8 个 pthread sibling 混合 pipe read、30 秒 nanosleep 和持续 yield；每轮 leader
直接调用 `SYS_exit_group`。最终持续标记版本结果：

```text
EXIT_GROUP_RACE_RESULT status=OK iterations=500
#### OS COMP TEST GROUP END buildstorm-glibc ####
[busybox-bringup] all commands finished
```

日志：`/tmp/wateros-task05a-persistent-race.log`。

最终代码的完整官方 BuildStorm 使用 QEMU 9.2.1、16 GiB、8 vCPU、`-snapshot`，raw 镜像从
`sdcard-rv-pub.img.gz` 重新解压，官方脚本回读 SHA-256 为
`84d631012532e6817565cba02d35d8a2721c5ec7787a1e0519d6d0ae0a4274bb`：

```text
TOOLCHAIN_RESULT status=OK
MINIBUILD_RESULT status=OK
[axbuild] ... done (554.78s)
BUILDSTORM_RESULT mode=multi status=OK rc=0 cores=8 elapsed_s=571.32 ... run=OK
#### OS COMP TEST GROUP END buildstorm-glibc ####
[busybox-bringup] all commands finished
```

日志：`/tmp/wateros-task05a-persistent-official-buildstorm.log`。未出现 WaterOS panic、OOM、
ENOMEM、SIGSEGV、double free、超时或残留 QEMU。宿主测试前已有约 5.4 GiB swap 使用，因此
该轮只作为功能门禁，不纳入最终性能统计。

## 限制与后续

- 未单独运行镜像中的 LTP `exit_group01`；500 轮混合阻塞专项探针覆盖了本次竞态窗口。
- 按既定资源约束未运行 LoongArch 长时间 workload，LoongArch 由 check 与 Final build 验收。
- Task 05 的 slab remote-free 改动仍未提交，将在本提交后独立进行性能验收。
