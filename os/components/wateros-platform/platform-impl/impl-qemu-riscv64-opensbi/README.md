# QEMU RISC-V OpenSBI Platform Profile

[Platform 总览](../../README.md) · [RISC-V Arch 实现](../../platform-arch/arch-impl/impl-riscv64/README.md)

该 profile 解释 QEMU `virt` + OpenSBI 机器约定。CSR、trap vector、`satp`、本地 SSIP/STIP 清除属于 arch；这里仅处理入口参数、DTB/RAM、SBI timer/HSM/IPI/RFENCE/reset 与 early UART。

## 启动与内存链

```text
_start.S
 -> Rust entry 构造 BootArgs(a0=hart id, a1=DTB PA)
 -> dtb::store(Release)
 -> 解析 timebase/RAM/设备
 -> arch trap + MM + task 初始化
 -> BSP 请求 HSM hart_start(AP entry, opaque)
 -> AP 完成本地初始化并发布 scheduler online
```

`BootArgs::new` 不验证 hart 或 DTB。`DTB_PA` 是 `AtomicUsize`，store Release/load Acquire 只保证数值发布，不保证任意 PA 安全可解引用；必须验证 FDT header/size，并保证恒等映射覆盖其整个 blob。

`physical_ram_end_exclusive()` 从保存的 DTB 找 memory region 上界，frame allocator 还必须排除内核镜像、DTB、reserved-memory 和 MMIO。仅知道 RAM end 不代表中间没有洞。

## 时间、console 与 reset

profile 默认 timebase 是 QEMU virt 的 10 MHz，聚合层可用 DTB `/cpus/timebase-frequency` 覆盖。`timer::set_timer` 把绝对 `u64` tick 原样交 SBI，不开 STIE、不清 pending；这些由 arch 路径负责。SBI set_timer 失败统一成 `Failure`，未保留原始错误码。

early console 实际直接轮询 16550 MMIO，而不是 SBI legacy console：THRE/TEMT 最多自旋 1,000,000 次；普通 buffer 把 `\n` 展开为 `\r\n`，raw buffer 不转换。多个 CPU 同时写没有本 profile 内部锁，输出可能交错；上层 runtime console 负责串行，panic 早期输出只能 best-effort。

reset 把通用类型映射到 SBI system reset。成功调用按语义不返回；若固件返回错误，必须映射为 Unsupported/Unavailable/Failed，不能继续执行关机后的普通路径。

## SMP 精确语义

SBI 错误映射：-2 Unsupported、-3 InvalidCpu、-6 AlreadyAvailable，其余保留为 `Firmware(raw)`。`start_cpu` 只检查 `cpu < MAX_CPUS`，再调用 HSM；start 成功或 status Started 都不表示 WaterOS online。

当前 `configured_cpu_mask()` 直接返回编译期 `MAX_CPUS` 的低位全集，没有从 DTB `/cpus` 筛选 QEMU 实际 `-smp` 数量。这是已知差距：上层若遍历 configured mask，可能向不存在 hart 发 HSM 并收到错误。修复应在 BSP 解析 enabled CPU reg，缓存实际 mask，并处理 `MAX_CPUS >= 64` 时的位移边界。

`send_ipi` 将 `CpuMask.bits()` 放进 base=0 的 SBI HartMask，没有在 profile 内再筛 configured/online。聚合层必须先发布 reason 并筛目标。目标 trap 清 SSIP 后消费 reason。

`flush_tlb_remote` 调 SBI `remote_sfence_vma(mask, 0, usize::MAX)`，`flush_icache_remote` 调 `remote_fence_i`；成功是同步完成边界。MM 回收 PTE/frame、ASID reuse 和进程级 icache syscall 都依赖错误不被吞掉。`init_ipi()` 为空是因为接收开关由 arch 设置。

## 扩展实例：DTB configured mask

新增一次性初始化函数，遍历 `/cpus` 下 `device_type="cpu"` 且 status 非 disabled 的节点，读取 hart reg，过滤 `< MAX_CPUS` 和 mask 位宽，再 Release 保存。BSP 必须在启动 AP 前调用；解析失败应只保留 BSP 并报告，而非退回全部 MAX_CPUS。

## 回归清单

- a0/a1、零/畸形/越界 DTB、多个/有洞 RAM region；
- DTB timebase 与 10 MHz fallback、长时间 drift、每 hart deadline；
- UART raw/CRLF、stuck THRE/TEMT、SMP 交错；
- `-smp 1/2/8` configured mask 与实际 hart 一致；
- HSM 正常、重复、无 hart、StartPending 到 OS online；
- 空/单/多目标 IPI reason，不给 offline hart 发任务；
- RFENCE 成功与每类 SBI 错误，失败时 MM 不释放相关资源；
- shutdown/cold/warm reset 及固件返回路径。
