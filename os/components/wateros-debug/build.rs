//! 诊断 crate 的构建脚本：把环境变量转换为可检查的编译期 cfg。

/// 只接受值为 `1` 的环境变量；其它值保持默认关闭，避免把空值或拼写错误误当启用。
fn main() {
    // 环境变量变化必须触发重建，否则 frame-pointer ABI 标记可能与 ELF 不一致。
    println!("cargo:rerun-if-env-changed=WATEROS_DEBUG_FRAME_POINTERS");
    println!("cargo:rustc-check-cfg=cfg(wateros_frame_pointers)");
    if std::env::var_os("WATEROS_DEBUG_FRAME_POINTERS").as_deref() ==
       Some(std::ffi::OsStr::new("1"))
    {
        println!("cargo:rustc-cfg=wateros_frame_pointers");
    }
}
