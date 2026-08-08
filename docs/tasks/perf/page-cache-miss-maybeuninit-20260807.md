# 页缓存 miss 临时页避免整页 memset（已回退）

> 状态：2026-08-07 完整 RISC-V Final 出现内核态 `LoadPageFault`，发生在 trap
> 日志格式化路径；为避免把 `MaybeUninit` 的未定义行为风险带进决赛，本改动已回退。

## 优化思路

页缓存 miss 的 `install_page()` 之前会先 `[0u8; FILE_PAGE_SIZE]` 初始化整个
4KiB 临时页，再从块设备读取：

```rust
let mut page_buf = [0u8; FILE_PAGE_SIZE];
let n = io.read_range(..., &mut page_buf[..to_read])?;
if n < to_read { page_buf[n..to_read].fill(0); }
```

大部分块设备读会填满整页，只有尾部不足一页时才需要补零。改为
`MaybeUninit<[u8; FILE_PAGE_SIZE]>`：

- 只对 `to_read` 区间构造可变 slice 交给 `read_range`。
- 若读不足，只 `memset` 尾部。
- 发布到缓存帧时只复制已初始化字节，再补零缓存帧尾部。

整页命中路径不再先 memset 整个临时页。

## 涉及文件

- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs`

## 验证

- 页缓存单测：13/13 通过。
- `make check ARCH=rv PROFILE=pre`
- `make check ARCH=la PROFILE=pre`
- `make check ARCH=rv PROFILE=final`
- `make check ARCH=la PROFILE=final`
- RISC-V `operator-run` 自动退出 pc-hot：`mmap01`、`epoll_wait01` 通过并正常关机。

## 同 workload pc-hot 数据

| 指标 | 前 | 后 |
|---|---:|---:|
| 总指令 | 326,216,437 | 321,089,048 |
| `memset` | 69,869,190 | 69,656,818 |
| `memcpy` | 94,544,360 | 94,532,611 |

短负载中页缓存 miss 占比不高，收益不明显；完整 BuildStorm 的页 miss 量更高，
但该 unsafe 路径未通过完整轮验收，后续需要先在受控环境中证明内存安全性，再重新
实验。
