//! LoongArch IPI/mailbox 后端。
//!
//! 此后端能为本机 CPU 初始化 IPI，但首期不提供远端 TLB shootdown；上层应把
//! `Unsupported` 当成该架构 SMP 尚未验收，而不是静默继续。

use api_v0::smp::{HartStatus, PlatformSmp, PlatformSmpError, PlatformSmpResult};
use base::cpu::{CpuId, CpuMask};
use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};

/// 该 profile 使用的 IOCSR IPI/mailbox 寄存器；本地 pending 清除在 arch interrupt。
const IOCSR_IPI_EN : usize = 0x1004;
const IOCSR_IPI_SEND : usize = 0x1040;
const IOCSR_MBUF_SEND : usize = 0x1048;
const IOCSR_IPI_SEND_BLOCKING : usize = 1 << 31;
const IOCSR_IPI_SEND_CPU_SHIFT : usize = 16;
const IOCSR_MBUF_SEND_BLOCKING : u64 = 1 << 31;
const IOCSR_MBUF_SEND_BOX_SHIFT : usize = 2;
const IOCSR_MBUF_SEND_CPU_SHIFT : usize = 16;
const IOCSR_MBUF_SEND_BUF_SHIFT : usize = 32;
const IOCSR_MBUF_SEND_H32_MASK : u64 = 0xFFFF_FFFF_0000_0000;
const IPI_BOOT_CPU : usize = 1 << 0;
const IPI_RUNTIME_NOTIFICATION : usize = 1 << 1;

const CPU_STOPPED : u8 = 0;
const CPU_START_PENDING : u8 = 1;
const CPU_STARTED : u8 = 2;
/// profile 对固件启动状态的本地镜像。
///
/// IPI_SYNC: 它仅避免重复向 mailbox 投递启动请求，不等于 task scheduler 的 online
/// mask。AP 必须走完整的 CPU-local/trap/timer 初始化并发布 online 后才能接任务。
static CPU_STATES : [AtomicU8; config::task::MAX_CPUS] =
    [const { AtomicU8::new(CPU_STOPPED) }; config::task::MAX_CPUS];
/// DTB `/cpus` 中实际存在且未禁用的 CPU。解析前只允许 BSP，避免把编译容量误当成
/// QEMU `-smp` 配置。
static CONFIGURED_CPU_MASK : AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuTopologyError {
    /// 未提供 DTB 指针。
    MissingDtb,
    /// DTB 解析失败。
    InvalidDtb,
    /// DTB 缺少 `/cpus` 节点。
    MissingCpuNode,
    /// 没有可用 CPU 节点。
    NoUsableCpu,
}

/// 从 QEMU virt 传入的 DTB 初始化实际 CPU 集合。
///
/// PLATFORM_BOUNDARY: `MAX_CPUS` 只是 WaterOS 的静态容量；QEMU 根据 `-smp` 在
/// `/cpus` 下创建 CPU 节点，二者不能互相替代。
pub fn init_configured_cpu_mask(dtb_pa : usize) -> Result<CpuMask, CpuTopologyError> {
    if dtb_pa == 0 {
        return Err(CpuTopologyError::MissingDtb);
    }
    let fdt = unsafe { fdt::Fdt::from_ptr(dtb_pa as *const u8) }.map_err(|_| {
                                                                    CpuTopologyError::InvalidDtb
                                                                })?;
    let cpus = fdt.find_node("/cpus")
                  .ok_or(CpuTopologyError::MissingCpuNode)?;
    let mut bits = 0u64;
    for node in cpus.children() {
        if node.property("device_type")
               .and_then(|property| property.as_str()) !=
           Some("cpu")
        {
            continue;
        }
        if node.property("status")
               .and_then(|property| property.as_str()) ==
           Some("disabled")
        {
            continue;
        }
        let Some(cpu) = node.property("reg")
                            .and_then(|property| property.as_usize())
        else {
            continue;
        };
        if cpu < config::task::MAX_CPUS && cpu < u64::BITS as usize {
            bits |= 1u64 << cpu;
        }
    }
    if bits == 0 {
        return Err(CpuTopologyError::NoUsableCpu);
    }
    CONFIGURED_CPU_MASK.store(bits, Ordering::Release);
    Ok(CpuMask::from_bits(bits))
}

#[inline]
fn is_configured(cpu : CpuId) -> bool {
    cpu.raw() < u64::BITS as usize &&
    CONFIGURED_CPU_MASK.load(Ordering::Acquire) & (1u64 << cpu.raw()) != 0
}

#[inline]
/// 写 32 位 IOCSR 控制寄存器；调用方必须传入本 profile 定义的合法地址。
fn iocsr_write32(value : u32, address : usize) {
    unsafe {
        core::arch::asm!("iocsrwr.w {value}, {address}", value = in(reg) value,
                         address = in(reg) address, options(nostack));
    }
}

#[inline]
/// 写 64 位 mailbox 数据寄存器。该操作会对目标 CPU 产生机器定义的副作用。
fn iocsr_write64(value : u64, address : usize) {
    unsafe {
        core::arch::asm!("iocsrwr.d {value}, {address}", value = in(reg) value,
                         address = in(reg) address, options(nostack));
    }
}

#[inline]
/// 从 LoongArch CSR 读取当前硬件 CPU 编号，供本 profile 更新启动状态镜像。
fn current_cpu_raw() -> usize {
    let cpu : usize;
    unsafe {
        core::arch::asm!("csrrd {cpu}, 0x20", cpu = out(reg) cpu, options(nomem, nostack));
    }
    cpu
}

/// 将 AP 入口写到目标 CPU 的 mailbox 0。
///
/// BOOT_CONTRACT: 高 32 位必须先写，再写低 32 位；此顺序是目标固件读取入口地址的
/// 协议，而不是普通的 64 位寄存器赋值。
fn send_mailbox0(cpu : CpuId, data : u64) {
    let target = (cpu.raw() as u64) << IOCSR_MBUF_SEND_CPU_SHIFT;
    let high = IOCSR_MBUF_SEND_BLOCKING |
               (1u64 << IOCSR_MBUF_SEND_BOX_SHIFT) |
               target |
               (data & IOCSR_MBUF_SEND_H32_MASK);
    iocsr_write64(high, IOCSR_MBUF_SEND);
    let low = IOCSR_MBUF_SEND_BLOCKING | target | (data << IOCSR_MBUF_SEND_BUF_SHIFT);
    iocsr_write64(low, IOCSR_MBUF_SEND);
}

#[inline]
/// 向一个目标 CPU 发送指定 IOCSR IPI action。
///
/// `action` 只允许本模块定义的启动或运行期通知位；reason 的语义由聚合层保存。
fn send_ipi_action(cpu : CpuId, action : usize) {
    let value = IOCSR_IPI_SEND_BLOCKING | (cpu.raw() << IOCSR_IPI_SEND_CPU_SHIFT) | action;
    iocsr_write32(value as u32, IOCSR_IPI_SEND);
}

/// QEMU LoongArch `virt` 的 mailbox/IPI 运输后端。
///
/// PLATFORM_BOUNDARY: 这里不实现远端 TLB shootdown；调用方收到 `Unsupported` 必须
/// 禁止共享用户地址空间的并发页表回收路径。
pub struct QemuLoongArchSmp;

impl PlatformSmp for QemuLoongArchSmp {
    /// 原子声明启动所有权后投递入口地址和 boot IPI。
    fn start_cpu(cpu : CpuId, start_addr : usize, _ : usize) -> PlatformSmpResult<()> {
        if !cpu.fits_capacity(config::task::MAX_CPUS) || !is_configured(cpu) || start_addr == 0 {
            return Err(PlatformSmpError::InvalidCpu);
        }
        match CPU_STATES[cpu.raw()].compare_exchange(CPU_STOPPED,
                                                     CPU_START_PENDING,
                                                     Ordering::AcqRel,
                                                     Ordering::Acquire)
        {
            Ok(_) => {}
            Err(CPU_START_PENDING) | Err(CPU_STARTED) => {
                return Err(PlatformSmpError::AlreadyAvailable);
            }
            Err(other) => return Err(PlatformSmpError::Firmware(other as usize)),
        }
        send_mailbox0(cpu, start_addr as u64);
        send_ipi_action(cpu, IPI_BOOT_CPU);
        Ok(())
    }

    /// 查询本 profile 的启动状态镜像；不是固件硬件状态的直接读取。
    fn cpu_status(cpu : CpuId) -> PlatformSmpResult<HartStatus> {
        if !cpu.fits_capacity(config::task::MAX_CPUS) || !is_configured(cpu) {
            return Err(PlatformSmpError::InvalidCpu);
        }
        Ok(match CPU_STATES[cpu.raw()].load(Ordering::Acquire) {
               CPU_STOPPED => HartStatus::Stopped,
               CPU_START_PENDING => HartStatus::StartPending,
               CPU_STARTED => HartStatus::Started,
               other => HartStatus::Unknown(other as usize),
           })
    }

    /// 返回 QEMU 实际配置且不超过编译期容量的候选 CPU 集合。
    fn configured_cpu_mask() -> CpuMask {
        // QEMU virt 的 DTB `/cpus` 会如实反映 `-smp`。`init_configured_cpu_mask`
        // 在 BSP 启动阶段解析并缓存，这里只读取该结果；未初始化时仅启动 boot CPU。
        let configured = CONFIGURED_CPU_MASK.load(Ordering::Acquire);
        CpuMask::from_bits(configured)
    }

    /// 为 mask 中每个 CPU 发送运行期通知，pending reason 已由聚合层写入。
    fn send_ipi(mask : CpuMask) -> PlatformSmpResult<()> {
        let configured = CONFIGURED_CPU_MASK.load(Ordering::Acquire);
        if mask.bits() & !configured != 0 {
            return Err(PlatformSmpError::InvalidCpu);
        }
        for cpu in 0..config::task::MAX_CPUS {
            let cpu_id = CpuId::from_raw(cpu);
            if mask.contains(cpu_id) {
                send_ipi_action(cpu_id, IPI_RUNTIME_NOTIFICATION);
            }
        }
        Ok(())
    }

    /// 首期不支持远端 TLB 刷新，禁止上层把此错误降级为本地 flush。
    fn flush_tlb_remote(_ : CpuMask) -> PlatformSmpResult<()> { Err(PlatformSmpError::Unsupported) }

    fn flush_icache_remote(_ : CpuMask) -> PlatformSmpResult<()> {
        Err(PlatformSmpError::Unsupported)
    }

    /// 打开当前 CPU IPI 接收端，并在状态镜像中标为 Started。
    fn init_ipi() -> PlatformSmpResult<()> {
        iocsr_write32(u32::MAX, IOCSR_IPI_EN);
        let cpu = current_cpu_raw();
        if cpu < config::task::MAX_CPUS {
            CPU_STATES[cpu].store(CPU_STARTED, Ordering::Release);
        }
        Ok(())
    }
}

pub use QemuLoongArchSmp as SmpImpl;
