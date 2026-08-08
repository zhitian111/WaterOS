# prctl 错误语义与 PDEATHSIG 实验记录（2026-08-09）

## 问题

`prctl02` 暴露了大量未实现 option 的错误语义；`PR_SET_PDEATHSIG` 的非法信号号
也没有校验。

## 修改

- `os/components/wateros-syscall/.../sys/task/task.rs`：
  - `PR_SET_PDEATHSIG` 校验非法信号号。
  - `PR_SET_DUMPABLE` 只接受 0/1。
  - 补齐 `PR_SET_TIMING`、no_new_privs、THP、CAP_AMBIENT、
    speculation control、securebits 的 `EINVAL/EPERM` 语义。
- `os/components/wateros-syscall/.../sys/cred/cap.rs`：`PR_CAPBSET_DROP`
  不再无权限时静默成功。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

RISC-V LTP：

- `prctl01`：`PR_SET/GET_PDEATHSIG` TPASS。
- `prctl02`：invalid option、PDEATHSIG、dumpable、timing、no_new_privs、
  THP、securebits、CAPBSET_DROP 路径全部 TPASS；seccomp/cap-ambient/
  speculation 按不支持 TCONF。

日志：`/tmp/prctl01-02-fixed2.log`。

## PDEATHSIG 投递实验（已回退）

曾尝试在 `exit_group`/最终 `exit` 路径向子进程投递 `parent_death_signal`，RISC-V
最小程序可收到 `SIGUSR2`。但 LoongArch 完整 Final 连续两轮出现 `SIGSEGV/卡死`，
与之前稳定配置不同；投递路径已回退，保留 `prctl02` 错误语义修复。后续需先在更短
进程退出负载下定位投递/中断时机，再重新合入。
