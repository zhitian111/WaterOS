//! `wateros` 内核 crate 的构建脚本：按启用的 board feature 把平台链接脚本与 `_start.S`
//! 登记为 `rerun-if-changed`，并向链接器传入 `-T.../link.ld`。
//!
//! 与 Rust 侧的对应关系：根 `main.rs` 用 `global_asm!` 编入同路径的 `_start.S`，入口符号
//! 与 `link.ld` 中的 `ENTRY(...)` 必须一致；改脚本或汇编后 Cargo 会重链。

fn main() {
    #[cfg(feature = "qemu-riscv64-opensbi")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-qemu-riscv64-opensbi/src/linker/link.ld");
    #[cfg(feature = "qemu-riscv64-opensbi")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-qemu-riscv64-opensbi/src/asm/_start.S");
    #[cfg(feature = "qemu-riscv64-opensbi")]
    println!("cargo::rustc-link-arg=-T./components/wateros-platform/platform-impl/\
              impl-qemu-riscv64-opensbi/src/linker/link.ld");

    #[cfg(feature = "qemu-loongarch64-virt")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-qemu-loongarch64-virt/src/linker/link.ld");
    #[cfg(feature = "qemu-loongarch64-virt")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-qemu-loongarch64-virt/src/asm/_start.S");
    #[cfg(feature = "qemu-loongarch64-virt")]
    println!("cargo::rustc-link-arg=-T./components/wateros-platform/platform-impl/\
              impl-qemu-loongarch64-virt/src/linker/link.ld");
}
