# wateros-base — 阶段能力概述

## 当前阶段目标

在内核各子系统之间提供**一套**共享的基础类型与配置常量，减少地址语义混淆和魔法数漂移。

## 已具备

### 类型层（wateros-base）

- 物理/虚拟地址与页号 newtype，供 MM、驱动、引导代码区分语义
- `DTBPA`、`CPUHartID` 等引导/CPU 标识别名
- 单核 `UniprocessorSafeCell`，供全局分配器等场景在 RefCell 规则下可变借用

### 配置层（wateros-base-config）

- Syscall 参数上限（6 槽）
- 内核堆 128MiB 与 QEMU virt 内存/MMIO 缺省布局
- 调度 tick 与时间片、ready 队列维护阈值
- 文件页缓存、块缓存、pipe 容量等 bring-up 尺度
- klog 环缓冲容量常量

## 适用范围

- 全内核 `#![no_std]` 构建
- QEMU virt bring-up；真机需结合 DTB 覆盖部分 `mm` 假设

## 已知限制

- 不含锁、原子或 per-CPU 设施
- 地址类型不做映射有效性检查
- 配置未按板级 feature 分层，QEMU 假设集中在 `mm`
- `FileIoMode::Async` 仅类型占位

## 下一步方向（未承诺）

- SMP 场景下的同步原语是否仍放 base 或下沉 platform
- 板级 feature 拆分 `base-config::mm`
- 视需要扩展地址 newtype 与页表编码辅助
