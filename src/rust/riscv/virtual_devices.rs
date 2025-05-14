use water_os::io::stdout::*;
use water_os::io::virtio::*;
use water_os::kernal_log;
use water_os::kernal_log_no_newline;
use water_os::print;
/**
# 方法简介
## 方法名称
init_virtual_devices
## 功能描述
集中初始化虚拟设备
## 处理流程
目前包括以下流程
## 初始化设备树
## 初始化virtio mmio设备
## 初始化uart设备
## 涉及数据
无
## 链式调用
water_os::io::stdout::uart_init()
water_os::io::virtio::init_dtb_mmio()
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
因为涉及到一些寄存器的读取和初始化，所以需要在进程和文件系统初始化前完成。
*/
pub fn init_virtual_devices() {
    init_dtb_mmio();
    uart_init();
}
/**
# 方法简介
## 方法名称
print_dtb_info
## 功能描述
打印设备树信息
包括详细的节点信息，节点属性信息，以及设备树信息
## 处理流程
- 打印设备树头信息
- 打印设备树节点信息
- 打印设备树节点属性信息
## 涉及数据
无
## 链式调用
无
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
程序输出信息较多，建议仅在调试时使用。
*/
pub fn print_dtb_info() {
    use crate::kernal_log;
    kernal_log!("DTB Header Info: {:?}",
                get_dtb_header());
    // kernal_log!("DTB Base Address: {:#x}", dtb_base_addr);
    // // 魔数
    // let dtb_magic_number : u32 = dtb_header.magic;
    // kernal_log!("DTB magic number: {:#x}",
    //             dtb_magic_number);
    // // DTB 总大小
    // let dtb_total_size : u32 = dtb_header.total_size;
    // kernal_log!("DTB total size: {:#x}", dtb_total_size);
    // // 结构块偏移
    // let dtb_struct_offset : u32 = dtb_header.off_dt_struct;
    // kernal_log!("DTB struct offset: {:#x}",
    //             dtb_struct_offset);
    // // 字符串表偏移
    // let dtb_strings_offset : u32 = dtb_header.off_dt_strings;
    // kernal_log!("DTB strings offset: {:#x}",
    //             dtb_strings_offset);
    // // 内存保留块偏移
    // let dtb_memory_reserve_offset : u32 = dtb_header.off_mem_rsvmap;
    // kernal_log!("DTB memory reserve offset: {:#x}",
    //             dtb_memory_reserve_offset);
    // // DTB 版本
    // let dtb_version : u32 = dtb_header.version;
    // kernal_log!("DTB version: {}", dtb_version);
    // // 最低兼容版本
    // let dtb_lowest_version : u32 = dtb_header.last_comp_version;
    // kernal_log!("DTB lowest version: {}",
    //             dtb_lowest_version);
    // // 启动CPU ID
    // let dtb_boot_cpu_id : u32 = dtb_header.boot_cpuid_phys;
    // kernal_log!("DTB boot cpu id: {}", dtb_boot_cpu_id);
    // // 字符串块大小
    // let dtb_strings_size : u32 = dtb_header.size_dt_strings;
    // kernal_log!("DTB strings size: {}", dtb_strings_size);
    // // 结构块大小
    // let dtb_struct_size : u32 = dtb_header.size_dt_struct;
    // kernal_log!("DTB struct size: {}", dtb_struct_size);
    // let mut dtb_str : [u8; 0x1000] = [0; 0x1000];
    // for i in 0..dtb_strings_size as usize {
    //     dtb_str[i] = read_value_at_address::<u8>(dtb_base_addr as usize +
    //                                              dtb_strings_offset as usize,
    //                                              i);
    // }
    // kernel_log_from_c_str_with_len(dtb_str.as_ptr() as *const u8,
    //                                dtb_strings_size as usize);
    let dt = get_device_tree();
    kernal_log!("DTB Info: {:?}", dt);
    let nodes = dt.all_nodes();
    for node in nodes {
        kernal_log!("Node Name: {:?}", node.name);
        kernal_log!("Node Address: {:#x}",
                    &node as *const _ as usize);
        for prop in node.properties() {
            let name = prop.name;
            let value = prop.value;

            // 打印属性名
            kernal_log!("Property Name: \"{}\"", name);

            // 使用固定大小的数组处理属性值
            let mut buffer : [u8; 256] = [0; 256];
            let value_len = value.len();

            if value_len > 256 {
                // 如果值太长，截断并提示
                buffer[..256].copy_from_slice(&value[..256]);
                kernal_log!("Property Value: <Too long，length: {}>",
                            value_len);
            } else {
                // 将值复制到缓冲区
                buffer[..value_len].copy_from_slice(value);
            }

            kernal_log_no_newline!("Property Value: [");
            for (i, &byte) in buffer[..value_len.min(256)].iter()
                                                          .enumerate()
            {
                if i > 0 {
                    prints(", ");
                }
                print!("0x{:02x}", byte);
            }
            print!("]\n");
            kernal_log!("----------------Device Tree Property Split Line----------------");
        }
        if node.name
               .split('@')
               .nth(0)
               .unwrap() ==
           "virtio_mmio"
        {
            kernal_log!("{:?}", VirtioMmioDevice::new(&node));
        }
        kernal_log!("------------------Device Tree Node Split Line------------------");
    }
    if is_virtio_mmio_device_with_ptr(0x10008000 as usize) {
        kernal_log!("Virtio mmio device found! At 0x10008000");
    }
}
