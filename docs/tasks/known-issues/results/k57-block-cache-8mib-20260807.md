# K-57 块缓存扩容到 8MiB（2026-08-07）

## 问题

`BLOCK_CACHE_CAPACITY_BLOCKS` 为 `1024`，对应 512B 设备块仅 512KiB。BuildStorm
反复读取 ext4 元数据时，512KiB 热集太小，大量请求穿透到 VirtIO。

## 修改

`os/components/wateros-base/base-config/src/fs.rs`：

```rust
pub const BLOCK_CACHE_CAPACITY_BLOCKS : usize = 16384;
```

16384 × 512B = 8MiB。

## pc-hot A/B（180s 同窗口）

| 符号 | 基线 | K-57 |
|---|---:|---:|
| VirtQueue `add_notify_wait_pop` | 1.24B | 1.09B |
| `read_blocks` | 715M | 574M |
| `write_blocks` | 209M | 195M |

## 完整 RISC-V Final

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1265.83 cores=8 bytes=1681000 arch=riscv64
```

对比 K-53 基线 `elapsed_s=1296.63`，本轮约快 31 秒。

## 验证

- `make rv_check` 通过
- `make la_check` 通过
- RISC-V Final 通过

日志：

```text
/tmp/k57-pchot.log
/tmp/k57-pcs.txt
/tmp/k57-full-rv2.log
```
