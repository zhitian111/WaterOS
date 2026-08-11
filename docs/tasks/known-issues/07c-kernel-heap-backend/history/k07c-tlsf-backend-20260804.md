# K-07C TLSF 内核堆后端验证报告

## 问题与修改

GDB 采样在 BuildStorm 路径中频繁捕获
`linked_list_allocator::allocate_first_fit/deallocate` 和其全局自旋锁。仓库虽已有
TLSF 实现，但 `wateros-runtime` 的依赖默认 feature 使顶层无法可靠做 A/B。

本次将选择权上移到顶层：

- 默认使用 `heap-tlsf`；Makefile 的 pre/final/GDB 目标均显式传递后端。
- `HEAP_ALLOCATOR_FEATURE=heap-linked-list` 保留可构建回退路径。
- `heap-stress` 仅用于 BSP 启动阶段 A/B，不进入普通内核。
- 未修改 task 组件、调度器 API、堆大小或 `.kernel.heap` 链接段契约。

## 碎片压力

RISC-V64/QEMU 8 核，同一套 64 B–16 KiB 非 LIFO 分配/释放，10 万轮：

| 后端 | 结束 used/free | 前段均值 | 后段均值 | 后/前 |
| --- | --- | ---: | ---: | ---: |
| TLSF | `0 / 134217728` | 5 | 5 | 100% |
| linked-list | `0 / 134217728` | 7 | 7 | 100% |

两后端都完全回收资源且无后半段退化。日志：
`/tmp/wateros-k07c-{tlsf,linked}-stress-20260804.log`。

## BuildStorm A/B

linked-list 同 workload 基线为 `6012.56s`。TLSF 三轮均输出完整成功标记：

| 轮次 | `elapsed_s` | 结果 |
| --- | ---: | --- |
| 1 | 1439.05 | CAgent 10/10，BuildStorm 成功 |
| 2 | 1460.34 | CAgent 10/10，BuildStorm 成功 |
| 3 | 1362.31 | CAgent 10/10，BuildStorm 成功 |

中位数 `1439.05s`，相对 linked-list 基线约 **4.18x**（耗时降低 76.1%）；三轮极差为
中位数的 6.8%。三轮均正常编译 `libc 0.2.185/0.2.186`，无 panic、OOM、死锁或
allocator metadata 损坏。

第三轮使用修复 journal 后的官方镜像和 QEMU snapshot 模式，完成后基础镜像 SHA-256
仍为 `83073eb1c5b85def0aba3031300a7c7c3f4594c7a68bfa146ae01d4a076a6abb`，
`e2fsck -fn` 五阶段通过。一次遗漏 snapshot 参数的直写运行已终止并作废，不计入
三轮数据。

## 其他验证

- `make rv_check` / `make la_check`：TLSF 默认均通过。
- 上述两命令增加 `HEAP_ALLOCATOR_FEATURE=heap-linked-list`：回退后端均通过。
- RISC-V pre 5 分钟：LTP 持续推进到 `recvmsg01`，无 allocator 异常。
- LoongArch64 final 120 秒：CAgent 10/10，BuildStorm toolchain/minibuild 通过并进入正式编译。

日志 SHA-256：

- 第 1 轮：`4789cef3d26eb1a9804abd66828ddf22b439ffc136433ecb12a619239bfc60d2`
- 第 2 轮：`de4d987e27f9b627db2d0996cf4af3bd1c17b285fcfd51b7bb166094287d4b3c`
- 第 3 轮：`ed282492e861a2db73fed12e37f51ec12785bab2b723472e12e5cbceb6d9863f`
- RISC-V pre：`7ab38afea91638877845e25e8feddc58311cc2654dac698cf9b4f20bd1df81dc`
- LoongArch64 short final：`e78c729d93be4d7194df2c133e8ec97df14a782e580a4feeb2a2afa2af79ba9a`

## 结论

TLSF 在保持双架构、可回退和资源稳定的前提下，显著降低了 BuildStorm 内核堆热路径
成本，满足 K-07C 切换默认后端的验收条件。该结论不等于所有 final 测试已通过。
