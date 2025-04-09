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

// 当架构为 riscv64 时，串口的地址为 0x1000_0000：
#[cfg(target_arch = "riscv64")]
const UART_ADDR : *mut u8 = 0x1000_0000 as *mut u8; // QEMU的串口地址

// 当架构为 loongarch64 时，串口的地址为 0x1000_0000：
#[cfg(target_arch = "loongarch64")]
const UART_ADDR : *mut u8 = 0x1000_0000 as *mut u8; // QEMU的串口地址

// 当架构为其他架构时，串口的地址为 0x1000_0000：
#[cfg(not(any(target_arch = "riscv64", target_arch = "loongarch64")))]
const UART_ADDR : *mut u8 = 0x1000_0000 as *mut u8; // QEMU的串口地址

/*
* 方法简介：该方法用于初始化串口。
*           调用：core::ptr::write_volatile()
* 输入参数：无。
* 输出参数：无。
* 异常情况：无。
* 注意事项：在进行串口通信之前，必须先调用该方法进行初始化。
*/
pub fn uart_init() {
    unsafe {
        // 配置线路控制寄存器 (LCR) 为 8 位数据模式
        core::ptr::write_volatile(UART_ADDR.add(3), 0x03u8);
    }
}
/*
* 方法简介：该方法用于向串口发送字符串。
*           依赖：water_os::io::stdout::uart_init()
*           调用：core::ptr::write_volatile()
* 输入参数：
*           _s：&str：需要发送的字符串。
* 输出参数：无。
* 异常情况：无。
* 注意事项：在第一次使用该方法之前，必须先调用water_os::io::stdout::uart_init()进行初始化。
*/
pub fn prints(_s : &str) -> () {
    for &byte in _s.as_bytes() {
        unsafe {
            while (core::ptr::read_volatile(UART_ADDR.add(5)) & 0x20) == 0 {} // 等待发送缓冲区空闲
            core::ptr::write_volatile(UART_ADDR, byte); // 发送一个字节
        }
    }
}

/*
* 方法简介：该方法用于向串口发送一个字节。
*           依赖：water_os::io::stdout::uart_init()
*           调用：core::ptr::write_volatile()
* 输入参数：
*           byte：u8：需要发送的字节。
* 输出参数：无。
* 异常情况：无。
* 注意事项：在第一次使用该方法之前，必须先调用water_os::io::stdout::uart_init()进行初始化。
*/
pub fn putc(byte : u8) -> () {
    unsafe {
        while (core::ptr::read_volatile(UART_ADDR.add(5)) & 0x20) == 0 {} // 等待发送缓冲区空闲
        core::ptr::write_volatile(UART_ADDR, byte); // 发送一个字节
    }
}

// 输出缓冲区定义
pub struct BufferWriter<'a> {
    buffer : &'a mut [u8],
    position : usize,
}
impl<'a> BufferWriter<'a> {
    pub fn new(buffer : &'a mut [u8]) -> Self {
        Self { buffer,
               position : 0 }
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.buffer[..self.position]
    }
}
impl<'a> core::fmt::Write for BufferWriter<'a> {
    fn write_str(&mut self, s : &str) -> core::fmt::Result {
        let len = s.len();
        if len >
           self.buffer
               .len() -
           self.position
        {
            return Err(core::fmt::Error);
        }
        self.buffer[self.position..self.position + len].copy_from_slice(s.as_bytes());
        self.position += len;
        Ok(())
    }
}
// 输出宏定义，用于向串口输出格式化的字符串，最大长度为 1024 字节
/*
* 方法简介：该方法用于向串口输出格式化的字符串。
*           依赖：water_os::io::stdout::uart_init()
*           调用：core::fmt::write!()
*                 water_os::io::stdout::BufferWriter::new()
*                 water_os::io::stdout::BufferWriter::as_slice()
*                 water_os::io::stdout::putc()
* 输入参数：
*           fmt：格式化字符串，长度应在1024 字节以内。
* 输出参数：无。
* 异常情况：如果格式化字符串的长度超过 1024 字节，则会截取前 1024 字节。
* 注意事项：在第一次使用该方法之前，必须先调用water_os::io::stdout::uart_init()进行初始化。
*           格式化字符串的长度应在1024 字节以内。
*/
#[macro_export] // 用于宏导出
macro_rules! print{
    () => {
       return;
    };
    ($($arg:tt)*) =>{
        {
            use core::fmt::Write;
            let mut buf = [0u8;1024];
            let mut writer = water_os::io::stdout::BufferWriter::new(&mut buf);
            let _ = write!(&mut writer,$($arg)*).unwrap();
            // let mut _s = format!( $($arg)* );
            for &byte in writer.as_slice() {
                water_os::io::stdout::putc(byte);
            }
        }
    };
}
