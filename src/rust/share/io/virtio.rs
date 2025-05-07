// static const MemMapEntry virt_memmap[] = {
//     [VIRT_DEBUG] =        {        0x0,         0x100 },
//     [VIRT_MROM] =         {     0x1000,        0xf000 },
//     [VIRT_TEST] =         {   0x100000,        0x1000 },
//     [VIRT_RTC] =          {   0x101000,        0x1000 },
//     [VIRT_CLINT] =        {  0x2000000,       0x10000 },
//     [VIRT_ACLINT_SSWI] =  {  0x2F00000,        0x4000 },
//     [VIRT_PCIE_PIO] =     {  0x3000000,       0x10000 },
//     [VIRT_IOMMU_SYS] =    {  0x3010000,        0x1000 },
//     [VIRT_PLATFORM_BUS] = {  0x4000000,     0x2000000 },
//     [VIRT_PLIC] =         {  0xc000000, VIRT_PLIC_SIZE(VIRT_CPUS_MAX * 2) },
//     [VIRT_APLIC_M] =      {  0xc000000, APLIC_SIZE(VIRT_CPUS_MAX) },
//     [VIRT_APLIC_S] =      {  0xd000000, APLIC_SIZE(VIRT_CPUS_MAX) },
//     [VIRT_UART0] =        { 0x10000000,         0x100 },
//     [VIRT_VIRTIO] =       { 0x10001000,        0x1000 },
//     [VIRT_FW_CFG] =       { 0x10100000,          0x18 },
//     [VIRT_FLASH] =        { 0x20000000,     0x4000000 },
//     [VIRT_IMSIC_M] =      { 0x24000000, VIRT_IMSIC_MAX_SIZE },
//     [VIRT_IMSIC_S] =      { 0x28000000, VIRT_IMSIC_MAX_SIZE },
//     [VIRT_PCIE_ECAM] =    { 0x30000000,    0x10000000 },
//     [VIRT_PCIE_MMIO] =    { 0x40000000,    0x40000000 },
//     [VIRT_DRAM] =         { 0x80000000,           0x0 },
// };

use crate::io::common::*;

// 当架构为riscv64时，VIRT_VIRTIO_BASE为0x10001000，VIRT_VIRTIO_SIZE为0x1000，即virt_memmap[VIRT_VIRTIO]的地址为0x10001000，大小为0x1000。
#[cfg(target_arch = "riscv64")]
const VIRT_VIRTIO_BASE : usize = 0x10001000;
#[cfg(target_arch = "riscv64")]
const VIRT_VIRTIO_SIZE : usize = 0x1000;
// 为了获取mmio设备的Magic Number，需要将VIRT_VIRTIO_BASE加上VIRT_VIRTIO_MAGIC_OFFSET
#[cfg(target_arch = "riscv64")]
const VIRT_VIRTIO_MAGIC_OFFSET : usize = 0x000;
// 为了获取mmio设备的Version Number，需要将VIRT_VIRTIO_BASE加上VIRT_VIRTIO_VERSION_OFFSET
#[cfg(target_arch = "riscv64")]
const VIRT_VIRTIO_VERSION_OFFSET : usize = 0x008;
// 为了获取mmio设备的Device ID，需要将VIRT_VIRTIO_BASE加上VIRT_VIRTIO_DEVICE_OFFSET
#[cfg(target_arch = "riscv64")]
const VIRT_VIRTIO_DEVICE_OFFSET : usize = 0x008;
// 为了获取mmio的Vendoer ID，需要将VIRT_VIRTIO_BASE加上VIRT_VIRTIO_DEVICE_OFFSET
#[cfg(target_arch = "riscv64")]
const VIRT_VIRTIO_VENDOR_OFFSET : usize = 0x00C;

// 当架构为其他架构时,将这些值设置为与riscv64架构相同的值做占位符
#[cfg(not(any(target_arch = "riscv64", target_arch = "loongarch64")))]
const VIRT_VIRTIO_BASE : usize = 0x10001000;
#[cfg(not(any(target_arch = "riscv64", target_arch = "loongarch64")))]
const VIRT_VIRTIO_SIZE : usize = 0x1000;
#[cfg(not(any(target_arch = "riscv64", target_arch = "loongarch64")))]
const VIRT_VIRTIO_MAGIC_OFFSET : usize = 0x1000;
#[cfg(not(any(target_arch = "riscv64", target_arch = "loongarch64")))]
const VIRT_VIRTIO_DEVICE_OFFSET : usize = 0x1004;
#[cfg(not(any(target_arch = "riscv64", target_arch = "loongarch64")))]
const VIRT_VIRTIO_VERSION_OFFSET : usize = 0x1004;
#[cfg(not(any(target_arch = "riscv64", target_arch = "loongarch64")))]
const VIRT_VIRTIO_VENDOR_OFFSET : usize = 0x00C;
// 魔术值的正确值
const CORRECT_MAGIC_NUMBER : u32 = 0x74726976;

static mut DTB_BASE_ADDR : usize = 0;
static mut DTB_HEADER : DTBHeader = DTBHeader { ptr : 0,
                                                magic : 0,
                                                total_size : 0,
                                                off_dt_struct : 0,
                                                off_dt_strings : 0,
                                                off_mem_rsvmap : 0,
                                                version : 0,
                                                last_comp_version : 0,
                                                boot_cpuid_phys : 0,
                                                size_dt_strings : 0,
                                                size_dt_struct : 0 };
struct MmioDeviceBlock {
    magic_number : u32,
    version : u32,
    device_id : u32,
    vendor_id : u32,
}
#[derive(Copy, Clone, Debug)]
pub struct DTBHeader {
    pub ptr : usize,
    pub magic : u32,
    pub total_size : u32,
    pub off_dt_struct : u32,
    pub off_dt_strings : u32,
    pub off_mem_rsvmap : u32,
    pub version : u32,
    pub last_comp_version : u32,
    pub boot_cpuid_phys : u32,
    pub size_dt_strings : u32,
    pub size_dt_struct : u32,
}
impl DTBHeader {}
fn init_dtb_base_addr() {
    // 从a1寄存器中获取dtb的基地址
    unsafe {
        use core::arch::asm;
        let mut a1 : usize = 0;
        #[cfg(target_arch = "riscv64")]
        asm!("mv {}, a1", out(reg) a1);
        DTB_BASE_ADDR = a1;
    }
}
fn init_dtb_header() {
    unsafe {
        let ptr = DTB_BASE_ADDR;
        DTB_HEADER.ptr = ptr;
        DTB_HEADER.ptr = ptr;
        let mut offset = 0;
        let mut bytes : [u8; 4] = [0; 4];
        bytes[0] = read_value_at_address(ptr, offset);
        bytes[1] = read_value_at_address(ptr, offset + 1);
        bytes[2] = read_value_at_address(ptr, offset + 2);
        bytes[3] = read_value_at_address(ptr, offset + 3);
        offset += 4;
        DTB_HEADER.magic = u32::from_be_bytes(bytes);
        bytes[0] = read_value_at_address(ptr, offset);
        bytes[1] = read_value_at_address(ptr, offset + 1);
        bytes[2] = read_value_at_address(ptr, offset + 2);
        bytes[3] = read_value_at_address(ptr, offset + 3);
        offset += 4;
        DTB_HEADER.total_size = u32::from_be_bytes(bytes);
        bytes[0] = read_value_at_address(ptr, offset);
        bytes[1] = read_value_at_address(ptr, offset + 1);
        bytes[2] = read_value_at_address(ptr, offset + 2);
        bytes[3] = read_value_at_address(ptr, offset + 3);
        offset += 4;
        DTB_HEADER.off_dt_struct = u32::from_be_bytes(bytes);
        bytes[0] = read_value_at_address(ptr, offset);
        bytes[1] = read_value_at_address(ptr, offset + 1);
        bytes[2] = read_value_at_address(ptr, offset + 2);
        bytes[3] = read_value_at_address(ptr, offset + 3);
        offset += 4;
        DTB_HEADER.off_dt_strings = u32::from_be_bytes(bytes);
        bytes[0] = read_value_at_address(ptr, offset);
        bytes[1] = read_value_at_address(ptr, offset + 1);
        bytes[2] = read_value_at_address(ptr, offset + 2);
        bytes[3] = read_value_at_address(ptr, offset + 3);
        offset += 4;
        DTB_HEADER.off_mem_rsvmap = u32::from_be_bytes(bytes);
        bytes[0] = read_value_at_address(ptr, offset);
        bytes[1] = read_value_at_address(ptr, offset + 1);
        bytes[2] = read_value_at_address(ptr, offset + 2);
        bytes[3] = read_value_at_address(ptr, offset + 3);
        offset += 4;
        DTB_HEADER.version = u32::from_be_bytes(bytes);
        bytes[0] = read_value_at_address(ptr, offset);
        bytes[1] = read_value_at_address(ptr, offset + 1);
        bytes[2] = read_value_at_address(ptr, offset + 2);
        bytes[3] = read_value_at_address(ptr, offset + 3);
        offset += 4;
        DTB_HEADER.last_comp_version = u32::from_be_bytes(bytes);
        bytes[0] = read_value_at_address(ptr, offset);
        bytes[1] = read_value_at_address(ptr, offset + 1);
        bytes[2] = read_value_at_address(ptr, offset + 2);
        bytes[3] = read_value_at_address(ptr, offset + 3);
        offset += 4;
        DTB_HEADER.boot_cpuid_phys = u32::from_be_bytes(bytes);
        bytes[0] = read_value_at_address(ptr, offset);
        bytes[1] = read_value_at_address(ptr, offset + 1);
        bytes[2] = read_value_at_address(ptr, offset + 2);
        bytes[3] = read_value_at_address(ptr, offset + 3);
        offset += 4;
        DTB_HEADER.size_dt_strings = u32::from_be_bytes(bytes);
        bytes[0] = read_value_at_address(ptr, offset);
        bytes[1] = read_value_at_address(ptr, offset + 1);
        bytes[2] = read_value_at_address(ptr, offset + 2);
        bytes[3] = read_value_at_address(ptr, offset + 3);
        DTB_HEADER.size_dt_struct = u32::from_be_bytes(bytes);
    }
}

pub fn get_dtb_base_addr() -> usize {
    unsafe { DTB_BASE_ADDR }
}
pub fn get_dtb_header() -> DTBHeader {
    unsafe { DTB_HEADER }
}
pub fn init_dtb_mmio() {
    init_dtb_base_addr();
    init_dtb_header();
}
pub fn init_virtio_mmio() {}

pub fn scan_virtio_mmio() {}
