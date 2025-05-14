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

use core::result;

use crate::io::common::*;
use fdt::{node::FdtNode, Fdt};
// 当架构为riscv64时，VIRT_VIRTIO_BASE为0x10001000，VIRT_VIRTIO_SIZE为0x1000，即virt_memmap[VIRT_VIRTIO]的地址为0x10001000，大小为0x1000。
#[cfg(target_arch = "riscv64")]
const VIRT_VIRTIO_BASE : usize = 0x10001000;
#[cfg(target_arch = "riscv64")]
const VIRT_VIRTIO_SIZE : usize = 0x1000;
// 为了获取mmio设备的Magic Number，需要将VIRT_VIRTIO_BASE加上VIRT_VIRTIO_MAGIC_OFFSET
#[cfg(target_arch = "riscv64")]
const VIRT_VIRTIO_MAGIC_OFFSET : usize = 0x000;
// 为了获取mmio设备的Device ID，需要将VIRT_VIRTIO_BASE加上VIRT_VIRTIO_DEVICE_OFFSET
#[cfg(target_arch = "riscv64")]
const VIRT_VIRTIO_DEVICE_OFFSET : usize = 0x008;
// 为了获取mmio设备的Version Number，需要将VIRT_VIRTIO_BASE加上VIRT_VIRTIO_VERSION_OFFSET
#[cfg(target_arch = "riscv64")]
const VIRT_VIRTIO_VERSION_OFFSET : usize = 0x004;
// 为了获取mmio的Vendoer ID，需要将VIRT_VIRTIO_BASE加上VIRT_VIRTIO_DEVICE_OFFSET
#[cfg(target_arch = "riscv64")]
const VIRT_VIRTIO_VENDOR_OFFSET : usize = 0x00C;
// 为了获取mmio设备的Features，需要将VIRT_VIRTIO_BASE加上VIRT_FEATUES_OFFSET
#[cfg(target_arch = "riscv64")]
const VIRT_FEATUES_OFFSET : usize = 0x010;
// 为了设置mmio设备的status，需要将VIRT_VIRTIO_BASE加上VIRT_VIRTIO_STATUS_OFFSET
#[cfg(target_arch = "riscv64")]
const VIRT_VIRTIO_STATUS_OFFSET : usize = 0x014;

// TODO: 对于loongarch64架构，需要修改这些值
// 当架构为loongarch64架构时,将这些值设置为与riscv64架构相同的值做占位符
#[cfg(target_arch = "loongarch64")]
const VIRT_VIRTIO_BASE : usize = 0x10001000;
#[cfg(target_arch = "loongarch64")]
const VIRT_VIRTIO_SIZE : usize = 0x1000;
// 为了获取mmio设备的Magic Number，需要将VIRT_VIRTIO_BASE加上VIRT_VIRTIO_MAGIC_OFFSET
#[cfg(target_arch = "loongarch64")]
const VIRT_VIRTIO_MAGIC_OFFSET : usize = 0x000;
// 为了获取mmio设备的Version Number，需要将VIRT_VIRTIO_BASE加上VIRT_VIRTIO_VERSION_OFFSET
#[cfg(target_arch = "loongarch64")]
const VIRT_VIRTIO_VERSION_OFFSET : usize = 0x004;
// 为了获取mmio设备的Device ID，需要将VIRT_VIRTIO_BASE加上VIRT_VIRTIO_DEVICE_OFFSET
#[cfg(target_arch = "loongarch64")]
const VIRT_VIRTIO_DEVICE_OFFSET : usize = 0x008;
// 为了获取mmio的Vendoer ID，需要将VIRT_VIRTIO_BASE加上VIRT_VIRTIO_DEVICE_OFFSET
#[cfg(target_arch = "loongarch64")]
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
static mut DEVICE_TREE : Option<Fdt> = None;
#[derive(Copy, Clone, Debug)]
pub struct VirtMmioBlock {
    pub magic_number : u32,
    pub version : u32,
    pub device_id : u32,
    pub vendor_id : u32,
}
impl VirtMmioBlock {
    /**
    # 方法简介
    ## 方法名称
    VirtMmioBlock::new
    ## 功能描述
    根据基地址初始化VirtMmioBlock结构体。
    ## 处理流程
    1. 初始化结构体，设置magic_number、version、device_id、vendor_id为0。
    2. 获取基地址，并根据基地址初始化magic_number、version、device_id、vendor_id。
    3. 返回结构体。
    ## 涉及数据
    1. VIRT_VIRTIO_MAGIC_OFFSET：const usize，virtio mmio设备的magic number偏移地址。
    2. VIRT_VIRTIO_VERSION_OFFSET：const usize，virtio mmio设备的version number偏移地址。
    3. VIRT_VIRTIO_DEVICE_OFFSET：const usize，virtio mmio设备的device id偏移地址。
    4. VIRT_VIRTIO_VENDOR_OFFSET：const usize，virtio mmio设备的vendor id偏移地址。
    ## 链式调用
    read_value_at_address::<u32>()
    ## 前置依赖
    无
    ## 是否修改参数
    否
    # 输入参数
    | 参数名 | 类型 | 含义 | 约束条件 | 默认值 |
    | ------ | -------- | ------ | ------ | ------ |
    | base_addr | usize | virtio mmio设备的基地址 | 非0 | 无 |
    # 输出参数
    | 参数名 | 类型 | 含义 | 约束条件 |
    | ------ | -------- | ------ | ------ |
    | Self | VirtMmioBlock | 结构体 | 无 |
    # 异常情况
    | 异常类型 | 异常原因 | 异常处理方式 |
    | ------ | -------- | ------ |
    | Panic | 运行时错误 | 打印错误信息，退出程序 |
    # 注意事项
    无
    */
    pub fn new(base_addr : usize) -> Self {
        if base_addr == 0 {
            return VirtMmioBlock { magic_number : 0,
                                   version : 0,
                                   device_id : 0,
                                   vendor_id : 0 };
        }
        let mut virt_mmio_block = VirtMmioBlock { magic_number : 0,
                                                  version : 0,
                                                  device_id : 0,
                                                  vendor_id : 0 };
        virt_mmio_block.magic_number =
            read_value_at_address::<u32>(base_addr, VIRT_VIRTIO_MAGIC_OFFSET);
        virt_mmio_block.version =
            read_value_at_address::<u32>(base_addr, VIRT_VIRTIO_VERSION_OFFSET);
        virt_mmio_block.device_id =
            read_value_at_address::<u32>(base_addr, VIRT_VIRTIO_DEVICE_OFFSET);
        virt_mmio_block.vendor_id =
            read_value_at_address::<u32>(base_addr, VIRT_VIRTIO_VENDOR_OFFSET);
        return virt_mmio_block;
    }
}
#[derive(Copy, Clone, Debug)]
pub struct VirtioMmioDevice {
    pub p_base_addr : usize,
    pub v_base_addr : usize,
    pub reg_phys_addr : usize,
    pub reg_virt_addr : usize,
    pub reg_size : usize,
    pub interrupts : u32,
    pub interrupt_parent : u32,
    pub virt_mmio_block : VirtMmioBlock,
}
impl VirtioMmioDevice {
    pub fn new(device_node : &FdtNode) -> Self {
        let base_addr_str = device_node.name
                                       .split('@')
                                       .nth(1)
                                       .unwrap();
        let base_addr = usize::from_str_radix(base_addr_str, 16).unwrap();
        let mut virtio_mmio_device = VirtioMmioDevice { p_base_addr : base_addr,
                                                        v_base_addr : 0,
                                                        reg_phys_addr : 0,
                                                        reg_virt_addr : 0,
                                                        reg_size : 0,
                                                        interrupts : 0,
                                                        interrupt_parent : 0,
                                                        virt_mmio_block:
                                                            VirtMmioBlock::new(base_addr) };
        let properties = device_node.properties();
        let mut interrupt = 0;
        let mut reg_phys_addr = 0;
        let mut reg_size = 0;

        for property in properties {
            match property.name {
                "reg" => {
                    let reg_value = property.value;
                    let mut reg_phys_addr_bytes : [u8; 8] = [0; 8];
                    reg_phys_addr_bytes[0] = reg_value[0];
                    reg_phys_addr_bytes[1] = reg_value[1];
                    reg_phys_addr_bytes[2] = reg_value[2];
                    reg_phys_addr_bytes[3] = reg_value[3];
                    reg_phys_addr_bytes[4] = reg_value[4];
                    reg_phys_addr_bytes[5] = reg_value[5];
                    reg_phys_addr_bytes[6] = reg_value[6];
                    reg_phys_addr_bytes[7] = reg_value[7];
                    reg_phys_addr = usize::from_be_bytes(reg_phys_addr_bytes);
                    let mut reg_size_bytes : [u8; 8] = [0; 8];
                    reg_size_bytes[0] = reg_value[8];
                    reg_size_bytes[1] = reg_value[9];
                    reg_size_bytes[2] = reg_value[10];
                    reg_size_bytes[3] = reg_value[11];
                    reg_size_bytes[4] = reg_value[12];
                    reg_size_bytes[5] = reg_value[13];
                    reg_size_bytes[6] = reg_value[14];
                    reg_size_bytes[7] = reg_value[15];
                    reg_size = usize::from_be_bytes(reg_size_bytes);
                }
                "interrupts" => {
                    let mut interrupt_bytes : [u8; 4] = [0; 4];
                    interrupt_bytes[0] = property.value[0];
                    interrupt_bytes[1] = property.value[1];
                    interrupt_bytes[2] = property.value[2];
                    interrupt_bytes[3] = property.value[3];
                    interrupt = u32::from_be_bytes(interrupt_bytes);
                }
                "interrupt-parent" => {
                    let mut interrupt_parent_bytes : [u8; 4] = [0; 4];
                    interrupt_parent_bytes[0] = property.value[0];
                    interrupt_parent_bytes[1] = property.value[1];
                    interrupt_parent_bytes[2] = property.value[2];
                    interrupt_parent_bytes[3] = property.value[3];
                    virtio_mmio_device.interrupt_parent =
                        u32::from_be_bytes(interrupt_parent_bytes);
                }
                _ => {}
            }
        }
        virtio_mmio_device.p_base_addr = base_addr;
        virtio_mmio_device.v_base_addr = base_addr;
        virtio_mmio_device.reg_phys_addr = reg_phys_addr;
        virtio_mmio_device.reg_virt_addr = reg_phys_addr;
        virtio_mmio_device.reg_size = reg_size;
        virtio_mmio_device.interrupts = interrupt;

        return virtio_mmio_device;
    }
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
pub fn get_device_tree() -> Fdt<'static> {
    unsafe {
        DEVICE_TREE.unwrap()
                   .clone()
    }
}
pub fn init_dtb_mmio() {
    init_dtb_base_addr();
    init_dtb_header();
    unsafe {
        let dtb_data = core::slice::from_raw_parts(DTB_BASE_ADDR as *const u8,
                                                   DTB_HEADER.total_size as usize);
        match Fdt::new(dtb_data) {
            Ok(dtb) => {
                DEVICE_TREE = Some(dtb);
            }
            Err(_e) => {
                return;
            }
        }
    }
}

pub fn is_virtio_mmio_device_with_ptr(base_addr : usize) -> bool {
    let mut mmio_device_block = VirtMmioBlock { magic_number : 0,
                                                version : 0,
                                                device_id : 0,
                                                vendor_id : 0 };
    mmio_device_block.magic_number =
        read_value_at_address::<u32>(base_addr, VIRT_VIRTIO_MAGIC_OFFSET);
    mmio_device_block.version = read_value_at_address::<u32>(base_addr, VIRT_VIRTIO_VERSION_OFFSET);
    mmio_device_block.device_id =
        read_value_at_address::<u32>(base_addr, VIRT_VIRTIO_DEVICE_OFFSET);
    mmio_device_block.vendor_id =
        read_value_at_address::<u32>(base_addr, VIRT_VIRTIO_VENDOR_OFFSET);
    if mmio_device_block.magic_number == CORRECT_MAGIC_NUMBER {
        return true;
    }
    return false;
}
pub fn init_virtio_mmio_block_device(device : &VirtMmioBlock) {}

pub fn scan_virtio_mmio() {}
