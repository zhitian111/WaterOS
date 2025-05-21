extern crate alloc;
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

/**
# 方法简介
## 方法名称
uart_init
## 功能描述
该方法用于初始化串口。
## 处理流程
配置线路控制寄存器 (LCR) 为 8 位数据模式。
## 涉及数据
UART_ADDR：*mut u8：串口地址。
## 链式调用
core::ptr::write_volatile()
## 前置依赖
无
## 是否修改参数
否
# 输入参数
| 参数名 | 类型 | 含义 | 约束条件 | 默认值 |
| ------ | -------- | ------ | ------ | ------ |
| 无 | 无 | 无 | 无 | 无 |
# 输出参数
| 参数名 | 类型 | 含义 | 约束条件 |
| ------ | -------- | ------ | ------ |
| 无 | 无 | 无 | 无 |
# 异常情况
| 异常类型 | 异常原因 | 异常处理方式 |
| ------ | -------- | ------ |
| Panic | 运行时错误 | 打印错误信息，退出程序 |
# 注意事项
在进行串口通信之前，必须先调用该方法进行初始化。
*/
#[unsafe(no_mangle)]
pub fn uart_init() {
    unsafe {
        // 配置线路控制寄存器 (LCR) 为 8 位数据模式
        core::ptr::write_volatile(UART_ADDR.add(3), 0x03u8);
    }
}
/**
# 方法简介
## 方法名称
prints
## 功能描述
该方法用于向串口发送字符串。
## 处理流程
1. 等待发送缓冲区空闲。
2. 发送一个字节。
3. 循环直到字符串发送完毕。
## 涉及数据
UART_ADDR：*mut u8：串口地址。
## 链式调用
core::ptr::write_volatile()
## 前置依赖
water_os::io::stdout::uart_init()
## 是否修改参数
否
# 输入参数
| 参数名 | 类型 | 含义 | 约束条件 | 默认值 |
| ------ | -------- | ------ | ------ | ------ |
| _s | &str | 需要发送的字符串 | 无 | 无 |
# 输出参数
| 参数名 | 类型 | 含义 | 约束条件 |
| ------ | -------- | ------ | ------ |
| 无 | 无 | 无 | 无 |
# 异常情况
| 异常类型 | 异常原因 | 异常处理方式 |
| ------ | -------- | ------ |
| Panic | 运行时错误 | 打印错误信息，退出程序 |
# 注意事项
在进行串口通信之前，必须先调用water_os::io::stdout::uart_init()进行初始化。
*/
#[unsafe(no_mangle)]
pub fn prints(_s : &str) -> () {
    for &byte in _s.as_bytes() {
        unsafe {
            while (core::ptr::read_volatile(UART_ADDR.add(5)) & 0x20) == 0 {} // 等待发送缓冲区空闲
            core::ptr::write_volatile(UART_ADDR, byte); // 发送一个字节
        }
    }
}

/**
# 方法简介
## 方法名称
putc
## 功能描述
该方法用于向串口发送一个字节。
## 处理流程
1. 等待发送缓冲区空闲。
2. 发送一个字节。
## 涉及数据
UART_ADDR：*mut u8：串口地址。
## 链式调用
core::ptr::write_volatile()
## 前置依赖
water_os::io::stdout::uart_init()
## 是否修改参数
否
# 输入参数
| 参数名 | 类型 | 含义 | 约束条件 | 默认值 |
| ------ | -------- | ------ | ------ | ------ |
| byte | u8 | 需要发送的字节 | 无 | 无 |
# 输出参数
| 参数名 | 类型 | 含义 | 约束条件 |
| ------ | -------- | ------ | ------ |
| 无 | 无 | 无 | 无 |
# 异常情况
| 异常类型 | 异常原因 | 异常处理方式 |
| ------ | -------- | ------ |
| Panic | 运行时错误 | 打印错误信息，退出程序 |
# 注意事项
在进行串口通信之前，必须先调用water_os::io::stdout::uart_init()进行初始化。
*/
#[unsafe(no_mangle)]
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
/**
# 方法简介
## 方法名称
print!
## 功能描述
该方法用于向串口输出格式化的字符串。
## 处理流程
1. 格式化字符串。
2. 执行prints()。
## 涉及数据
UART_ADDR：*mut u8：串口地址。
## 链式调用
core::fmt::write!()
water_os::io::stdout::prints()
water_os::io::stdout::putc()
water_os::io::stdout::BufferWriter::new()
water_os::io::stdout::BufferWriter::as_slice()
## 前置依赖
water_os::io::stdout::uart_init()
## 是否修改参数
否
# 输入参数
| 参数名 | 类型 | 含义 | 约束条件 | 默认值 |
| ------ | -------- | ------ | ------ | ------ |
| fmt | &str | 格式化字符串 | 无 | 无 |
| args | 无 | 格式化参数 | 无 | 无 |
# 输出参数
| 参数名 | 类型 | 含义 | 约束条件 |
| ------ | -------- | ------ | ------ |
| 无 | 无 | 无 | 无 |
# 异常情况
| 异常类型 | 异常原因 | 异常处理方式 |
| ------ | -------- | ------ |
| Panic | 运行时错误 | 打印错误信息，退出程序 |
# 注意事项
在进行串口通信之前，必须先调用water_os::io::stdout::uart_init()进行初始化。
格式化字符串的长度应在1024 字节以内。
*/
#[macro_export] // 用于宏导出
macro_rules! print{
    () => {
       return;
    };
    ($($arg:tt)*) =>{
        {
            extern crate alloc;
            let mut buf = alloc::format!($($arg)*);
            water_os::io::stdout::prints(&buf);
        }
    };
}

// 按行输出宏定义，用于向串口输出格式化的字符串，最大长度为 1024 字节
/**
# 方法简介
## 方法名称
println!
## 功能描述
该方法用于向串口输出格式化的字符串，并自动换行。
## 处理流程
1. 格式化字符串。
2. 执行prints()。
3. 发送换行符。
## 涉及数据
UART_ADDR：*mut u8：串口地址。
## 链式调用
core::fmt::write!()
water_os::io::stdout::prints()
water_os::io::stdout::putc()
water_os::io::stdout::BufferWriter::new()
water_os::io::stdout::BufferWriter::as_slice()
## 前置依赖
water_os::io::stdout::uart_init()
## 是否修改参数
否
# 输入参数
| 参数名 | 类型 | 含义 | 约束条件 | 默认值 |
| ------ | -------- | ------ | ------ | ------ |
| fmt | &str | 格式化字符串 | 无 | 无 |
| args | 无 | 格式化参数 | 无 | 无 |
# 输出参数
| 参数名 | 类型 | 含义 | 约束条件 |
| ------ | -------- | ------ | ------ |
| 无 | 无 | 无 | 无 |
# 异常情况
| 异常类型 | 异常原因 | 异常处理方式 |
| ------ | -------- | ------ |
| Panic | 运行时错误 | 打印错误信息，退出程序 |
# 注意事项
在进行串口通信之前，必须先调用water_os::io::stdout::uart_init()进行初始化。
格式化字符串的长度应在1024 字节以内。
*/
#[macro_export] // 用于宏导出
macro_rules! println{
    () => {
       return;
    };
    ($($arg:tt)*) =>{
        {
            extern crate alloc;
            let mut buf = alloc::format!($($arg)*);
            water_os::io::stdout::prints(&buf);
            water_os::io::stdout::putc(b'\n');
            water_os::io::stdout::putc(b'\r');
        }
    };

}

// 内核日志输出宏定义，用于向串口输出格式化的日志信息字符串，最大长度为 1024 字节
/**
# 方法简介
## 方法名称
kernal_log!
## 功能描述
该方法用于向串口输出格式化的日志信息字符串，并自动换行。
## 处理流程
1. 输出日志前缀。
2. 格式化字符串。
3. 执行prints()。
4. 发送换行符。
## 涉及数据
UART_ADDR：*mut u8：串口地址。
## 链式调用
core::fmt::write!()
water_os::io::stdout::prints()
water_os::io::stdout::putc()
water_os::io::stdout::BufferWriter::new()
water_os::io::stdout::BufferWriter::as_slice()
## 前置依赖
water_os::io::stdout::uart_init()
## 是否修改参数
否
# 输入参数
| 参数名 | 类型 | 含义 | 约束条件 | 默认值 |
| ------ | -------- | ------ | ------ | ------ |
| fmt | &str | 格式化字符串 | 无 | 无 |
| args | 无 | 格式化参数 | 无 | 无 |
# 输出参数
| 参数名 | 类型 | 含义 | 约束条件 |
| ------ | -------- | ------ | ------ |
| 无 | 无 | 无 | 无 |
# 异常情况
| 异常类型 | 异常原因 | 异常处理方式 |
| ------ | -------- | ------ |
| Panic | 运行时错误 | 打印错误信息，退出程序 |
# 注意事项
在进行串口通信之前，必须先调用water_os::io::stdout::uart_init()进行初始化。
格式化字符串的长度应在1024 字节以内。
*/
#[macro_export] // 用于宏导出
macro_rules! kernal_log{
    () => {
       return;
    };
    ($($arg:tt)*) =>{
        {
        water_os::io::stdout::putc(b'[');
        water_os::io::stdout::putc(b' ');
        water_os::io::stdout::prints("\x1B[33m");
        water_os::io::stdout::putc(b'K');
        water_os::io::stdout::putc(b'e');
        water_os::io::stdout::putc(b'r');
        water_os::io::stdout::putc(b'n');
        water_os::io::stdout::putc(b'e');
        water_os::io::stdout::putc(b'l');
        water_os::io::stdout::prints("\x1B[0m");
        water_os::io::stdout::putc(b' ');
        water_os::io::stdout::putc(b']');
        water_os::io::stdout::putc(b'\t');
        extern crate alloc;
        let mut buf = alloc::format!($($arg)*);
        water_os::io::stdout::prints(&buf);
        water_os::io::stdout::putc(b'\n');
        water_os::io::stdout::putc(b'\r');
        }
    };

}

// 内核日志输出宏定义，用于向串口输出格式化的日志信息字符串，最大长度为 1024 字节
/**
# 方法简介
## 方法名称
kernal_log!
## 功能描述
该方法用于向串口输出格式化的日志信息字符串。
## 处理流程
1. 输出日志前缀。
2. 格式化字符串。
3. 执行prints()。
## 涉及数据
UART_ADDR：*mut u8：串口地址。
## 链式调用
core::fmt::write!()
water_os::io::stdout::prints()
water_os::io::stdout::putc()
water_os::io::stdout::BufferWriter::new()
water_os::io::stdout::BufferWriter::as_slice()
## 前置依赖
water_os::io::stdout::uart_init()
## 是否修改参数
否
# 输入参数
| 参数名 | 类型 | 含义 | 约束条件 | 默认值 |
| ------ | -------- | ------ | ------ | ------ |
| fmt | &str | 格式化字符串 | 无 | 无 |
| args | 无 | 格式化参数 | 无 | 无 |
# 输出参数
| 参数名 | 类型 | 含义 | 约束条件 |
| ------ | -------- | ------ | ------ |
| 无 | 无 | 无 | 无 |
# 异常情况
| 异常类型 | 异常原因 | 异常处理方式 |
| ------ | -------- | ------ |
| Panic | 运行时错误 | 打印错误信息，退出程序 |
# 注意事项
在进行串口通信之前，必须先调用water_os::io::stdout::uart_init()进行初始化。
格式化字符串的长度应在1024 字节以内。
*/
#[macro_export] // 用于宏导出
macro_rules! kernal_log_no_newline{
    () => {
       return;
    };
    ($($arg:tt)*) =>{
        {
        water_os::io::stdout::putc(b'[');
        water_os::io::stdout::putc(b' ');
        water_os::io::stdout::prints("\x1B[33m");
        water_os::io::stdout::putc(b'K');
        water_os::io::stdout::putc(b'e');
        water_os::io::stdout::putc(b'r');
        water_os::io::stdout::putc(b'n');
        water_os::io::stdout::putc(b'e');
        water_os::io::stdout::putc(b'l');
        water_os::io::stdout::prints("\x1B[0m");
        water_os::io::stdout::putc(b' ');
        water_os::io::stdout::putc(b']');
        water_os::io::stdout::putc(b'\t');
        extern crate alloc;
        let mut buf = alloc::format!($($arg)*);
        water_os::io::stdout::prints(&buf);
        }
    };

}

// 内核日志输出C风格字符串的定义，用于向串口输出字符串。
/**
# 方法简介
## 方法名称
kernel_log_from_c_str
## 功能描述
该方法用于向串口输出C风格字符串。
## 处理流程
1. 输出日志前缀。
2. 输出字符串。
3. 发送换行符。
## 涉及数据
UART_ADDR：*mut u8：串口地址。
## 链式调用
water_os::io::stdout::putc()
## 前置依赖
water_os::io::stdout::uart_init()
## 是否修改参数
否
# 输入参数
| 参数名 | 类型 | 含义 | 约束条件 | 默认值 |
| ------ | -------- | ------ | ------ | ------ |
| s | *const u8 | C风格字符串的地址 | 无 | 无 |
# 输出参数
| 参数名 | 类型 | 含义 | 约束条件 |
| ------ | -------- | ------ | ------ |
| 无 | 无 | 无 | 无 |
# 异常情况
| 异常类型 | 异常原因 | 异常处理方式 |
| ------ | -------- | ------ |
| Panic | 运行时错误 | 打印错误信息，退出程序 |
# 注意事项
在进行串口通信之前，必须先调用water_os::io::stdout::uart_init()进行初始化。
*/
pub fn kernel_log_from_c_str(s : *const u8) {
    putc(b'[');
    putc(b' ');
    prints("\x1B[33m");
    putc(b'K');
    putc(b'e');
    putc(b'r');
    putc(b'n');
    putc(b'e');
    putc(b'l');
    prints("\x1B[0m");
    putc(b' ');
    putc(b']');
    putc(b'\t');
    let mut i = 0;
    while unsafe { *s.add(i) } != 0 {
        putc(unsafe { *s.add(i) });
        i += 1;
    }
    putc(b'\n');
    putc(b'\r');
}

// 内核日志输出C风格字符串的定义，用于向串口输出限定长度的字符串。
/**
# 方法简介
## 方法名称
kernel_log_from_c_str_with_len
## 功能描述
该方法用于向串口输出限定长度的C风格字符串。
## 处理流程
1. 输出日志前缀。
2. 输出字符串。
3. 发送换行符。
## 涉及数据
UART_ADDR：*mut u8：串口地址。
## 链式调用
water_os::io::stdout::putc()
## 前置依赖
water_os::io::stdout::uart_init()
## 是否修改参数
否
# 输入参数
| 参数名 | 类型 | 含义 | 约束条件 | 默认值 |
| ------ | -------- | ------ | ------ | ------ |
| s | *const u8 | C风格字符串的地址 | 无 | 无 |
| len | usize | 字符串的长度 | 无 | 无 |
# 输出参数
| 参数名 | 类型 | 含义 | 约束条件 |
| ------ | -------- | ------ | ------ |
| 无 | 无 | 无 | 无 |
# 异常情况
| 异常类型 | 异常原因 | 异常处理方式 |
| ------ | -------- | ------ |
| Panic | 运行时错误 | 打印错误信息，退出程序 |
# 注意事项
在进行串口通信之前，必须先调用water_os::io::stdout::uart_init()进行初始化。
如果在字符串中遇到'\0'，则会自动换行并输出日志前缀，然后继续输出字符串。
*/
pub fn kernel_log_from_c_str_with_len(s : *const u8, len : usize) {
    putc(b'[');
    putc(b' ');
    prints("\x1B[33m");
    putc(b'K');
    putc(b'e');
    putc(b'r');
    putc(b'n');
    putc(b'e');
    putc(b'l');
    prints("\x1B[0m");
    putc(b' ');
    putc(b']');
    putc(b'\t');
    for i in 0..len {
        if unsafe { *s.add(i) } == 0 {
            putc(b'\n');
            putc(b'\r');
            putc(b'[');
            putc(b' ');
            prints("\x1B[33m");
            putc(b'K');
            putc(b'e');
            putc(b'r');
            putc(b'n');
            putc(b'e');
            putc(b'l');
            prints("\x1B[0m");
            putc(b' ');
            putc(b']');
            putc(b'\t');
        }
        putc(unsafe { *s.add(i) });
    }
    putc(b'\n');
    putc(b'\r');
}

/**
# 方法简介
## 方法名称
show_logo
## 功能描述
该方法用于向串口输出水滴wateros的logo。
## 处理流程
1. 输出logo。
## 涉及数据
UART_ADDR：*mut u8：串口地址。
## 链式调用
water_os::io::stdout::prints()
## 前置依赖
water_os::io::stdout::uart_init()
## 是否修改参数
否
# 输入参数
| 参数名 | 类型 | 含义 | 约束条件 | 默认值 |
| ------ | -------- | ------ | ------ | ------ |
| 无 | 无 | 无 | 无 | 无 |
# 输出参数
| 参数名 | 类型 | 含义 | 约束条件 |
| ------ | -------- | ------ | ------ |
| 无 | 无 | 无 | 无 |
# 异常情况
| 异常类型 | 异常原因 | 异常处理方式 |
| ------ | -------- | ------ |
| Panic | 运行时错误 | 打印错误信息，退出程序 |
# 注意事项
在进行串口通信之前，必须先调用water_os::io::stdout::uart_init()进行初始化。
*/
pub fn show_logo() {
    prints("\x1B[36m");
    prints("██╗    ██╗ █████╗ ████████╗███████╗██████╗      ██████╗ ███████╗\n\r");
    prints("██║    ██║██╔══██╗╚══██╔══╝██╔════╝██╔══██╗    ██╔═══██╗██╔════╝\n\r");
    prints("██║ █╗ ██║███████║   ██║   █████╗  ██████╔╝    ██║   ██║███████╗\n\r");
    prints("██║███╗██║██╔══██║   ██║   ██╔══╝  ██╔══██╗    ██║   ██║╚════██║\n\r");
    prints("╚███╔███╔╝██║  ██║   ██║   ███████╗██║  ██║    ╚██████╔╝███████║\n\r");
    prints(" ╚══╝╚══╝ ╚═╝  ╚═╝   ╚═╝   ╚══════╝╚═╝  ╚═╝     ╚═════╝ ╚══════╝\n\r");
    prints("\x1B[0m");
}
