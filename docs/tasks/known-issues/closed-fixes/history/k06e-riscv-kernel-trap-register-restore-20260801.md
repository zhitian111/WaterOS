# K-06E RISC-V 内核 trap 寄存器恢复报告（2026-08-01）

## 现象

glibc LTP 全量回归在 `alarm05` 期间由 CPU 0 报告内核态 load page fault：

```text
pc=0x802539e2 fault_addr=0x11 returns_to_user=false
```

符号化后 PC 位于 `wateros_driver_network::stack::poll_socket_events()` 的 smoltcp
socket 查找迭代器。相同位置此前在 `getrusage04` 无限运行后也曾偶发一次，因此本轮确认
它不是单纯的网络数据结构问题。

## 根因

故障指令为 `ld a3, 16(t1)`，`stval=0x11` 表明恢复执行时 `t1==1`。网络全局状态始终
由同一把 `spin::Mutex` 保护；检查 RISC-V `trap.asm` 后发现内核态返回路径存在确定性
寄存器破坏：

1. 先从 TrapContext 恢复 `x5/t0` 和 `x6/t1`；
2. 随后再次使用 `t0` 写 `satp`，使用 `t1` 读取 ASID 开关；
3. 未再次恢复便执行 `sret`。

启用 ASID 时 `t1` 因而固定变成 1，正好解释 `0x11` 故障地址；`t0` 也会被旧栈指针
覆盖。任何在内核态被定时器/IPI 中断的 Rust 代码都可能受影响，并非网络模块专有。

## 修复与验证

返回路径现在先完成 `satp`/ASID 处理，再恢复 `x5`、`x6`，最后直接以
`ld sp, 2*8(sp)` 恢复旧栈并 `sret`。用户态返回路径原本已把 `x5` 放在最后恢复，
无需修改。

- RISC-V LTP glibc 新 overlay：已跨过原 `alarm05` 复现点并推进至第 209 项
  `cpuhotplug04.sh`，无 kernel fault/panic。
- 完整 glibc LTP 重跑继续执行，最终结果另行记录。
