# Quick Learn：WaterOS 内核组件快速入门

本目录用**统一的方法**介绍 WaterOS 的各个内核组件：先讲**用户怎么用**，再讲**数据结构**，
最后用一个**完整故事（时序图）**串起来，并对应到实际代码。适合"从用户视角反推内核实现"。

## 组件索引

| 组件 | 一句话本质 | 文档 |
|---|---|---|
| futex | 给"等锁"提供睡觉和叫醒服务的内核原语（含 robust 防死锁） | [futex.md](futex.md) |
| signal | 内核给进程的异步通知：记账 → 安全点 → 投递 | [signal.md](signal.md) |
| tty | 终端行规程：把裸字节翻译成"行"和"控制事件" | [tty.md](tty.md) |
| pty | 用 master/slave 软件对模拟终端硬件，供 nxterm/shell 使用 | [pty.md](pty.md) |
| shm | 让多进程看见同一块物理内存，零拷贝 IPC | [shm.md](shm.md) |
| task | 进程/线程/调度器：PCB + TCB + 状态机 + 就绪队列 | [task.md](task.md) |
| mm | 内存管理：页表翻译 + 惰性分配 + COW + TLB shootdown | [mm.md](mm.md) |
| vfs | 统一文件接口：句柄抽象 + fd 表 + 挂载表 + 预约读 | [vfs.md](vfs.md) |
| fs | 文件系统实现层：FsImpl 注册表 + ext4/devfs/procfs/ramfs | [fs.md](fs.md) |
| driver | 设备驱动：DTB 扫描 + 五子系统领域 trait + MachineDriver | [driver.md](driver.md) |
| network | 网络协议栈：socket 状态机 + fd 桥接 + smoltcp 轮询 | [network.md](network.md) |

## 阅读顺序建议

- **入门**：`task`（进程怎么来/怎么跑/怎么没）→ `mm`（内存怎么给）→ `vfs`（文件怎么读）
- **存储链路**：`driver`（硬件）→ `fs`（真文件系统）→ `vfs`（统一文件接口）
- **IPC 三件套**：`futex`（同步）→ `shm`（共享数据）→ `signal`（异步通知）
- **终端链路**：`tty`（真实串口）→ `pty`（虚拟终端）→ 配合 `signal`（Ctrl+C）
- **进阶**：`network`（socket 如何当 fd 用，可与 `vfs` 对照）

## 贯穿全文的三个通用套路

这些组件虽然职责不同，但实现上反复出现三个共同原则，读文档时可以互相印证：

1. **用户态做快事、内核态做慢事**：futex 抢锁在用户态原子完成、只有睡觉/叫醒才进内核。
2. **预约-提交模型**：读数据先"预约"再"复制"，成功才提交、失败回滚（futex/PTY/VFS/socket 通用）。
3. **持锁不干重活**：持锁时禁止做页表映射、信号投递、调度、IPI——重活都在锁外做（所有模块通用）。
