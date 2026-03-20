#![no_std]
#![no_main]
#[panic_handler]
pub fn panic_handler(_panic_info : &core::panic::PanicInfo) -> ! {
    runtime::panic::panic_handler(_panic_info)
}
#[cfg(feature = "qemu-riscv64-opensbi")]
mod qemu_riscv64_opensbi {
    use core::arch::global_asm;
    use core::include_str;
    global_asm!(include_str!("../components/wateros-platform/platform-impl/\
                              impl-qemu-riscv64-opensbi/src/asm/_start.S"));
    #[unsafe(no_mangle)]
    pub fn kernel_main(boot_arg0 : usize, boot_arg1 : usize) -> ! {
        use platform::boot::{BootArgs, BootContext};
        let _boot_context = BootContext::from(BootArgs::new(boot_arg0, boot_arg1));
        runtime::console::show_logo();
        unreachable!("unreachable code in platform_main")
    }
}
