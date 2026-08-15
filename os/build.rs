//! `wateros` 内核 crate 的构建脚本：按启用的 board feature 把平台链接脚本与
//! 入口 shim 登记为 `rerun-if-changed`，并向链接器传入 `-T.../link.ld`。
//!
//! 平台 impl 用 `global_asm!` 编入 `_start.S`，入口符号必须与
//! `link.ld` 中的 `ENTRY(...)` 一致；架构 impl 则负责通用 boot 汇编。

/// 按启用的 board feature 向 Cargo 声明重链条件并传入链接脚本。
fn main() {
    println!("cargo::rerun-if-changed=./components/wateros-platform/linker/kernel-sections.ld");
    #[cfg(feature = "qemu-riscv64-opensbi")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-qemu-riscv64-opensbi/src/linker/link.ld");
    #[cfg(feature = "qemu-riscv64-opensbi")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-qemu-riscv64-opensbi/src/asm/_start.S");
    #[cfg(feature = "qemu-riscv64-opensbi")]
    println!("cargo::rustc-link-arg=-T./components/wateros-platform/platform-impl/\
              impl-qemu-riscv64-opensbi/src/linker/link.ld");

    #[cfg(feature = "jh7110-visionfive2")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-jh7110-visionfive2/src/linker/link.ld");
    #[cfg(feature = "jh7110-visionfive2")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-jh7110-visionfive2/src/asm/_start.S");
    #[cfg(feature = "jh7110-visionfive2")]
    println!("cargo::rustc-link-arg=-T./components/wateros-platform/platform-impl/\
              impl-jh7110-visionfive2/src/linker/link.ld");

    #[cfg(feature = "qemu-loongarch64-virt")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-qemu-loongarch64-virt/src/linker/link.ld");
    #[cfg(feature = "qemu-loongarch64-virt")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-qemu-loongarch64-virt/src/asm/_start.S");
    #[cfg(feature = "qemu-loongarch64-virt")]
    println!("cargo::rustc-link-arg=-T./components/wateros-platform/platform-impl/\
              impl-qemu-loongarch64-virt/src/linker/link.ld");

    #[cfg(feature = "loongson2k1000la")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-loongson2k1000la/src/linker/link.ld");
    #[cfg(feature = "loongson2k1000la")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-loongson2k1000la/src/asm/_start.S");
    #[cfg(feature = "loongson2k1000la")]
    println!("cargo::rustc-link-arg=-T./components/wateros-platform/platform-impl/\
              impl-loongson2k1000la/src/linker/link.ld");
}
