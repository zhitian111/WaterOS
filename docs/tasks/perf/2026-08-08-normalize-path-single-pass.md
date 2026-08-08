# 2026-08-08：绝对路径规范化单遍写入

## 背景

pc-hot 中 `normalize_absolute_path` 约 210M 指令。旧实现先构造 `Vec<&str>` 再拼
`String`，每个路径都产生两次分配和一次二次遍历。

## 改动

`wateros-vfs-api-v0/src/path.rs`：

- 直接写入预分配的 `String`，正常路径不再分配 `Vec<&str>`。
- 只有遇到 `..` 时才回退上一段，保持根路径之上折叠为 `/` 的语义。
- 新增 `.` / `//` / `..` / UTF-8 组件单测。

## pc-hot 同窗口结果

同为 RISC-V Final 早期 200s 窗口，QEMU 8 vCPU / 8 GiB / `-snapshot`，绑定 P-core。

| 指标 | 优化前 | 优化后 |
|---|---:|---:|
| `normalize_absolute_path` | 209.7M | 144.9M |

目标符号下降约 31%。

## 材料

```text
pcs: /tmp/pcs-rv-normalize-path-20260808.txt
  sha256: 1d84ae931f3c8688b96b3978d68a7142099dd714efd58505f5e7921b59a2be7d
raw_log: /tmp/normalize-path-pc-hot.log
  sha256: 5f773b43d5581d8d467507e84dc210c19ec04c30711e05c166a98ceb7249da0e
pre_smoke: /tmp/normalize-path-pre-smoke.log
  sha256: 8f6ae9290aebbabfdd4d8c1bfcd64f4ca6bdea8f54dbceaceb7c275fcdbcdbc0
```

## 验证

- `cargo test -p wateros-vfs-api-v0 --lib` 通过（4 passed）
- `make check ARCH=rv PROFILE=final` 通过
- `make check ARCH=la PROFILE=final` 通过
- RISC-V pre smoke 进入 LTP，无 panic/fatal

## 后续

- 完整 BuildStorm 和 read-family/LTP 路径回归仍需最终门禁。
- 下一个候选：页缓存 BTree key 查找、user_copy 或 `memcpy/memcmp` 来源拆分。
