# 页缓存 miss 临时缓冲改为栈数组

## 优化思路

`GlobalFilePageCache::install_page()` 每次页缓存 miss 都会创建一个
`Vec<u8>` 临时缓冲：

```rust
let mut page_buf = vec![0u8; FILE_PAGE_SIZE];
```

BuildStorm 会大量读 ELF、脚本和 Cargo 文件，页 miss 高频出现。这个 4KiB
临时缓冲会反复进入 TLSF alloc/dealloc，和 pc-hot 中 TLSF 热点吻合。
`FILE_PAGE_SIZE` 固定为 4096，函数栈空间足够，因此改为栈数组：

```rust
let mut page_buf = [0u8; FILE_PAGE_SIZE];
```

行为完全一致：先读盘到 `page_buf`，再在获得缓存帧后复制到帧数据。收益是消除
每次 miss 的堆分配与释放，只保留一次栈初始化。

## 涉及文件

- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs`

## 验证

- `cargo test --manifest-path os/components/wateros-vfs/vfs-impl/impl-page-cache/Cargo.toml`
  ：13 个页缓存单测通过。
- `make check ARCH=rv PROFILE=pre`
- `make check ARCH=la PROFILE=pre`
- `make check ARCH=rv PROFILE=final`
- `make check ARCH=la PROFILE=final`
- RISC-V pre QEMU 60s smoke：ext4 root RW 挂载成功，进入 busybox bringup，无 panic。

## 后续

该改动是可独立合入的低风险子项。下一步需要做 pc-hot A/B，统计 `install_page`、
TLSF alloc/dealloc 的指令变化；若收益不明显，再继续优化页缓存 miss 的整页拷贝。
