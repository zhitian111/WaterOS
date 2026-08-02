fn main() {
    println!("cargo:rerun-if-env-changed=WATEROS_DEBUG_FRAME_POINTERS");
    println!("cargo:rustc-check-cfg=cfg(wateros_frame_pointers)");
    if std::env::var_os("WATEROS_DEBUG_FRAME_POINTERS").as_deref() ==
       Some(std::ffi::OsStr::new("1"))
    {
        println!("cargo:rustc-cfg=wateros_frame_pointers");
    }
}
