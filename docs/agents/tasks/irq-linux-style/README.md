# IRQ 驱动化任务（feat/irq-linux-style）

## 目标

把 WaterOS 现有「阻塞 / 轮询」驱动改造成 Linux 风格的 IRQ 驱动化：

- 通用中断子系统：`irqchip` + `irq domain` + DTB `interrupt-parent/interrupts/interrupt-map` 解析，驱动只认统一 `virq`。
- 中断处理模型：top-half 极简（claim/ack/记状态），bottom-half 完成回收与唤醒，ISR 内不 sleep、不拿重锁。
- 块设备：对外保持同步语义，对内 `request_queue + submit/complete + 多请求在途 + completion 唤醒`。
- 网络：virtio-net NAPI（中断触发 → ack + 关中断 → 预算 poll → 重开中断）。
- 输入/显示/GPU：中断唤醒替代轮询空转（第二阶段）。
- 双架构：RISC-V（QEMU virt，PLIC + virtio-MMIO）与 LoongArch（QEMU virt，EIOINTC + virtio-PCI）都需适配。
- 可移植：抽象设计面向后续 VisionFive2（RISC-V PLIC）与 Loongson 2K1000（LIOINTC/EIOINTC）等真机。

## 分支与工作树

- 分支：`feat/irq-linux-style`
- 工作树：`/home/zhitian/project/WaterOS_irq_linux_style`
- 起点：`main @ 59f50c44`

## 历史实验结论（仅作参考）

历史分支已经验证过几条硬约束，本任务沿用这些结论：

1. `virtio-drivers` 的同步 `add_notify_wait_pop` 与异步 `read_blocks_nb/write_blocks_nb` **不能在同一队列混用**。
2. 单纯把忙等换成「IRQ + WFI/睡眠」而没有多请求在途与锁拆分，历史上要么卡死要么 BuildStorm 更慢。
3. 把 FS 外层锁换成读写锁、允许睡眠，不等于所有嵌套 legacy spin lock 都能安全跨 I/O；`ext4/VFS/block-cache` 锁链是真正的雷区。
4. vendor `virtio-drivers 0.12.0` 已提供 `*_nb/complete_*/ack_interrupt/enable_interrupts`，无需改 vendor 即可做异步提交与中断确认。

## 总体架构决策

- 自建极薄 `components/wateros-irq`，**不重新引入 `irq-framework`**。
- 对外 API 保持同步（syscall/FS/VFS 不变），对内异步化。
- 设备契约由 `&mut self` 改为 `&self` + 实现内自持锁；`SharedBlockDevice/SharedNetworkDevice` 从 `Arc<Mutex<Box<dyn ...>>>` 改为 `Arc<dyn ...>`。
- 端口性通过 DTB 驱动 irq domain 保证，不写死中断号。

## 任务列表

| 编号 | 任务 | 关键产出 |
|---|---|---|
| T01 | IRQ 核心抽象 + DTB irq-domain 解析 | `wateros-irq`、`driver-api` DTB 升级 |
| T02 | RISC-V PLIC + SEIE + trap 外部中断分发 | PLIC irqchip、arch external enable、trap 分支 |
| T03 | LoongArch EIOINTC + PCI interrupt-map + ESTAT/ECFG | LoongArch irqchip 与 trap 解码 |
| T04 | bottom-half / softirq + irqaction 线程化 | 延迟执行与唤醒原语 |
| T05 | BlockDevice 契约改 `&self` + 同步回退 | 锁模型重构，行为不变 |
| T06 | 块设备 request_queue + submit/complete + 多请求在途 | RISC-V 块异步完成路径 |
| T07 | ext4/block-cache/VFS 锁拆分 | miss 释放锁、回查、不跨 I/O 持锁 |
| T08 | virtio-net NAPI | 网络中断 + 预算 poll |
| T09 | LoongArch 块异步对等 | EIOINTC/PCI 完成唤醒 |
| T10 | input/display/GPU 中断化 | 轮询改中断唤醒 |
| T11 | 真机适配（VisionFive2 / LS2K1000） | 板级 irqchip + DTB，等板子到手 |
| T12 | 性能验收与归档 | BuildStorm A/B、合并决策 |

每个任务对应一次可回归、可验收的 commit。

## 验收总则

### 功能 / bug 验收

每任务至少满足：

1. 静态：`make configure && make rv_check && make la_check` 通过。
2. 单测：被改动的 crate 的 host `cargo test`（如涉及）通过。
3. QEMU 功能 smoke：受影响架构启动、根文件系统挂载、关键日志 marker 出现，无 `panic`/`SIGSEGV`/卡死。

全量功能回归（合并前 / T12 前）使用官方 pub 镜像跑双架构，判读 marker：

- `#### OS COMP TEST GROUP START buildstorm-glibc ####`
- `TOOLCHAIN_RESULT status=OK`
- `MINIBUILD_RESULT status=OK`
- `BUILDSTORM_COMPILE mode=multi status=OK rc=0 ... run=OK`
- `#### OS COMP TEST GROUP END buildstorm-glibc ####`

### 性能验收

1. 保留 main 基线内核。当前基线快照：
   - `/home/zhitian/project/WaterOS_refactor/os/.perf-baseline-main/kernel-{rv,la}-final`
   - `/tmp/wateros-irq-baseline-main/`
2. 镜像统一解压到 `~/Downloads/`：
   - `gunzip -k ~/Downloads/sdcard-rv-pub.img.gz`
   - `gunzip -k ~/Downloads/sdcard-la-pub.img.gz`
3. 覆写镜像内 BuildStorm 脚本为 `~/Downloads/buildstorm_testcode.recovered.sh`（用 `debugfs -w`；目标路径以实际镜像为准，候选 `/glibc/buildstorm_testcode.sh`、`/buildstorm_testcode.sh`）。
4. 使用线上同版本 QEMU 9.2.1：`~/qemu_9_2_1/qemu-9.2.1/build/qemu-system-riscv64`、`.../qemu-system-loongarch64`。
5. 跑 BuildStorm，记录 `BUILDSTORM_COMPILE ... elapsed_s=...`。
6. 最终要求：先保证无 bug；性能测试过程中允许暂时劣化，但最终结果必须优于 main 基线。

## QEMU 启动命令（性能测试）

LoongArch：

```bash
~/qemu_9_2_1/qemu-9.2.1/build/qemu-system-loongarch64 \
  -kernel kernel-la -m 36G -nographic -smp 12 \
  -drive file=$HOME/Downloads/sdcard-la-pub.img,if=none,format=raw,id=x0 \
  -device virtio-blk-pci,drive=x0 -no-reboot \
  -device virtio-net-pci,netdev=net0 -netdev user,id=net0 -rtc base=utc
```

RISC-V：

```bash
~/qemu_9_2_1/qemu-9.2.1/build/qemu-system-riscv64 \
  -machine virt -kernel kernel-rv -m 16G -nographic -smp 8 -bios default \
  -drive file=$HOME/Downloads/sdcard-rv-pub.img,if=none,format=raw,id=x0 \
  -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 -no-reboot \
  -device virtio-net-device,netdev=net -netdev user,id=net -rtc base=utc
```

## 协作约定

- 性能测试前，先确认系统中没有正在运行的 QEMU 进程；若有，等待其结束后再跑。
- 功能测试不受此限制。
- 镜像只解压到 `~/Downloads/`，不在仓库内放置大镜像。
- 不提交生成物（`target/`、`kernel-*`、镜像、日志）。

## 任务简报规范

每个任务完成后，在 `history/<NN>-<slug>.md` 写一份简报，至少包含：

- 完成日期与 commit sha；
- 实际改动摘要（与方案差异）；
- 精确验证命令与结果；
- 未验证项 / 风险 / 下一步。

简报用于跨 agent 交接和后续回归审计，不做来源性注释。
