# QEMU LoongArch64 virt Platform Profile

[Platform 总览](../../README.md) · [LoongArch Arch 实现](../../platform-arch/arch-impl/impl-loongarch64/README.md)

该 profile 负责 QEMU LoongArch `virt` 的固定 DTB 来源、高段 RAM、IOCSR mailbox/IPI、StableCounter timer、16550 console 与 ACPI GED reset。PGDL、CRMD、ECFG/ESTAT、trap 和本地 IPI pending 清除仍属于 arch。

## 启动、DTB 与 RAM

direct-kernel boot 的 DTB PA 固定为 `0x0010_0000`，不是 LoongArch ISA ABI。`BootArgs` 暴露 profile 构造的原始值；平台 bring-up 应尽早 `dtb::store`，再初始化 CPU mask、RAM 与设备。固定地址仍要验证 FDT，不能因为是 QEMU 常量就无条件解引用。

QEMU 1 GiB 布局含低 256 MiB 与从 `0x9000_0000` 开始的高 768 MiB，中间不是可分配 RAM。内核位于高段，`physical_ram_end_exclusive` 只选 base≥`0x8000_0000` 的最大 region end，禁止 frame allocator 跨越低段/MMIO 空洞。

`normalize_qemu8_region` 兼容某些 QEMU 8.x 把 sized-cells 宽度 2 错带进高 32 位的 `0x2_90000000/0x2_30000000` 编码，只在 base 和 size 高半都等于 2 时截断。DTB 不可用时硬编码回退 `0xc000_0000`，只匹配仓库默认 `-m 1G`；改变 QEMU 内存大小而 DTB 解析失败会产生危险上界，生产路径应让失败阻止 MM 初始化。

## CPU topology 和启动状态

`init_configured_cpu_mask(dtb_pa)` 遍历 `/cpus` 子节点，要求 `device_type=cpu`、status 非 disabled、有 reg，并过滤 `MAX_CPUS`/64 位 mask。解析前 mask 只有 BSP bit 0；成功以 Release 发布。CPU reg 若稀疏，mask 也保留洞。

`CPU_STATES` 是本地原子镜像：STOPPED → START_PENDING → STARTED。`start_cpu` 先 CAS 抢启动所有权，再按硬件协议把 AP entry 高 32 位、低 32 位依次写 mailbox 0，最后发 boot IPI。`opaque` 参数当前被忽略，新增 AP 参数不能假装已经传送。

没有硬件 ack 或失败回滚：mailbox/IPI 写之后函数直接 Ok，AP 若未进入，状态会永久停在 StartPending，也无法重试。`init_ipi` 打开当前 CPU 所有 IPI enable 位，并把本地 state 设 Started；这仍早于/不同于 scheduler online。应由 BSP 设置启动超时并联合观察 state 与 online mask。

## 运行期 IPI 与远端 fence 缺口

`send_ipi(mask)` 先验证所有 bits 属于 configured mask，再逐 CPU 发 `IPI_RUNTIME_NOTIFICATION`。软件 reason 由聚合层在此前 Release 写入；arch handler 负责清 IOCSR pending 后 AcqRel 消费。

`flush_tlb_remote` 和 `flush_icache_remote` 当前都明确返回 `Unsupported`。这意味着 LoongArch SMP 尚不能安全承诺共享地址空间页表回收、COW PTE 替换后的远端可见性、ASID reuse 或跨 CPU 修改可执行代码。禁止把错误降级成本地 flush 后继续释放 frame。

正确补齐 shootdown 需要 sequence/generation、目标 mask、每 CPU ack、超时/CPU offline 并发处理：发布请求 → 发 IPI → 每个目标执行本地 fence → Release ack → 发起方 Acquire 等齐，才允许回收。

## Timer、console、reset

`set_timer` 读 `rdtime.d`，用 `deadline.saturating_sub(now).max(1)` 得相对 delta，转换 `usize` 后向 4 tick 上取整，先写 TICLR 再写 `TCFG | ENABLE`。接近 `usize::MAX` 的加 3 会返回 InvalidDeadline。每 CPU 必须分别编程；频率 fallback 与 StableCounter 必须同源。

console 轮询固定 UART `0x1fe001e0`，普通输出做 LF→CRLF，raw 不做；实现没有超时，损坏/未映射 UART 可让 CPU 永久自旋。SMP 输出需由上层锁串行。

reset 向 ACPI GED `0x100e001c` 写 sleep/reset 值。Shutdown 使用 S5 type+SLP_EN，冷/热重启当前都走相同 reset 寄存器；写后若 QEMU 未退出，函数只能报告 Failed，调用者不得假定已复位。

## 回归清单

- 固定 DTB 地址、畸形 FDT、标准/错误 QEMU8 memory cells；
- `-m` 多种大小、高低段/空洞，DTB 失败时不越界分 frame；
- `/cpus` disabled/稀疏/超 MAX/无节点，发布前仅 BSP；
- 两 BSP 竞争启动同 AP、mailbox 高低顺序、忽略 opaque 的可见诊断；
- AP 成功、永久 StartPending 超时、state 与 scheduler online 区别；
- IPI 空/非法/多目标 mask 与 reason 合并；
- shootdown Unsupported 时 MM 拒绝危险回收；完成实现后测 ack/timeout/offline race；
- 过去/极远 deadline、4 tick 对齐、每 CPU timer；
- UART 未就绪、CRLF、SMP 输出，以及 shutdown/cold/warm QEMU 行为。
