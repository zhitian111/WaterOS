//! VisionFive 2 PLIC ownership for the kernel's BSP-only external IRQ path.

extern crate alloc;

use alloc::format;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

const UNINITIALIZED : usize = usize::MAX;
static BASE : AtomicUsize = AtomicUsize::new(UNINITIALIZED);
static CONTEXT : AtomicUsize = AtomicUsize::new(UNINITIALIZED);
static MAX_IRQ : AtomicU32 = AtomicU32::new(0);

fn read_be_u32(bytes : &[u8], offset : usize) -> Option<u32> {
    let value = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_be_bytes(value.try_into()
                                 .ok()?))
}

fn property_u32(node : &fdt::node::FdtNode<'_, '_>, name : &str) -> Option<u32> {
    read_be_u32(node.property(name)?
                    .value,
                0)
}

fn is_plic(node : &fdt::node::FdtNode<'_, '_>) -> bool {
    node.property("compatible")
        .is_some_and(|property| {
            property.value
                    .split(|byte| *byte == 0)
                    .any(|value| {
                        core::str::from_utf8(value).is_ok_and(|compatible| {
                                                       matches!(compatible,
                                                                "starfive,jh7110-plic" |
                                                                "riscv,plic0" |
                                                                "sifive,plic-1.0.0")
                                                   })
                    })
        })
}

fn interrupt_controller_for_hart(fdt : &fdt::Fdt<'_>, hart_id : usize) -> Option<u32> {
    for cpu in fdt.all_nodes() {
        let is_cpu = cpu.property("device_type")
                        .and_then(|property| core::str::from_utf8(property.value).ok())
                        .is_some_and(|kind| kind.trim_end_matches('\0') == "cpu");
        if !is_cpu {
            continue;
        }
        let current_hart = cpu.reg()
                              .and_then(|mut regions| regions.next())
                              .map(|region| region.starting_address as usize);
        if current_hart != Some(hart_id) {
            continue;
        }
        let path = format!("/cpus/{}/interrupt-controller",
                           cpu.name);
        let controller = fdt.find_node(&path)?;
        return property_u32(&controller, "phandle").or_else(|| {
                                                       property_u32(&controller, "linux,phandle")
                                                   });
    }
    None
}

fn discover(cpu_id : usize) -> Result<(usize, usize, u32), &'static str> {
    let dtb_pa = crate::dtb::dtb_pa();
    if dtb_pa == 0 {
        return Err("VisionFive 2 DTB is unavailable");
    }
    // SAFETY: the firmware DTB was validated and retained by the platform boot path.
    let fdt =
        unsafe { fdt::Fdt::from_ptr(dtb_pa as *const u8) }.map_err(|_| "invalid VisionFive 2 DTB")?;
    let controller = interrupt_controller_for_hart(&fdt, cpu_id).ok_or("missing CPU \
                                                                        interrupt-controller \
                                                                        phandle")?;
    for node in fdt.all_nodes() {
        if !is_plic(&node) {
            continue;
        }
        let region = node.reg()
                         .and_then(|mut regions| regions.next())
                         .ok_or("PLIC reg is missing")?;
        let size = region.size
                         .ok_or("PLIC reg size is missing")?;
        let base = region.starting_address as usize;
        let sources = property_u32(&node, "riscv,ndev").ok_or("PLIC riscv,ndev is missing")?;
        let contexts = node.property("interrupts-extended")
                           .ok_or("PLIC interrupts-extended is missing")?
                           .value;
        if contexts.len() % 8 != 0 {
            return Err("invalid PLIC interrupts-extended");
        }
        for (context, pair) in contexts.chunks_exact(8)
                                       .enumerate()
        {
            let phandle = read_be_u32(pair, 0).ok_or("invalid PLIC context phandle")?;
            let interrupt = read_be_u32(pair, 4).ok_or("invalid PLIC context interrupt")?;
            if phandle == controller && interrupt == 9 {
                let required = 0x20_0008usize.checked_add(context.checked_mul(0x1000)
                                                                 .ok_or("PLIC context overflow")?)
                                             .ok_or("PLIC context overflow")?;
                if base == 0 || sources == 0 || required > size {
                    return Err("invalid PLIC MMIO range");
                }
                return Ok((base, context, sources));
            }
        }
        return Err("PLIC has no supervisor context for boot hart");
    }
    Err("PLIC node is missing")
}

fn read(offset : usize) -> u32 {
    let base = BASE.load(Ordering::Acquire);
    // SAFETY: init validated the PLIC MMIO range before publishing BASE.
    unsafe { read_volatile((base + offset) as *const u32) }
}

fn write(offset : usize, value : u32) {
    let base = BASE.load(Ordering::Acquire);
    // SAFETY: register offsets are bounded by the validated PLIC layout.
    unsafe { write_volatile((base + offset) as *mut u32, value) }
}

/// Initialize the boot hart's supervisor PLIC context with every source masked.
pub fn init(cpu_id : usize) -> Result<(), &'static str> {
    if CONTEXT.load(Ordering::Acquire) != UNINITIALIZED {
        return Err("PLIC already initialized");
    }
    let (base, context, sources) = discover(cpu_id)?;
    BASE.store(base, Ordering::Release);
    for word in 0..(sources as usize + 32) / 32 {
        write(0x2000 + context * 0x80 + word * 4, 0);
    }
    write(0x20_0000 + context * 0x1000, 0);
    MAX_IRQ.store(sources, Ordering::Release);
    CONTEXT.store(context, Ordering::Release);
    Ok(())
}

pub fn enable(irq : u32) -> bool {
    let context = CONTEXT.load(Ordering::Acquire);
    if context == UNINITIALIZED || irq == 0 || irq > MAX_IRQ.load(Ordering::Acquire) {
        return false;
    }
    write(irq as usize * 4, 1);
    let offset = 0x2000 + context * 0x80 + irq as usize / 32 * 4;
    write(offset, read(offset) | (1 << (irq % 32)));
    true
}

pub fn disable(irq : u32) {
    let context = CONTEXT.load(Ordering::Acquire);
    if context == UNINITIALIZED || irq == 0 || irq > MAX_IRQ.load(Ordering::Acquire) {
        return;
    }
    let offset = 0x2000 + context * 0x80 + irq as usize / 32 * 4;
    write(offset,
          read(offset) & !(1 << (irq % 32)));
}

pub fn claim() -> Option<u32> {
    let context = CONTEXT.load(Ordering::Acquire);
    if context == UNINITIALIZED {
        return None;
    }
    let irq = read(0x20_0004 + context * 0x1000);
    (irq != 0).then_some(irq)
}

pub fn complete(irq : u32) {
    let context = CONTEXT.load(Ordering::Acquire);
    if context == UNINITIALIZED || irq == 0 {
        return;
    }
    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("fence iorw, iorw", options(nostack));
    }
    write(0x20_0004 + context * 0x1000, irq);
}
