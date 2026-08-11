# 2026-08-08：user_copy 读路径单次页表 walk

## 背景

`copy_from_user` 每页先 `translate_addr()` 再 `leaf_page_perm()`，同一 VPN 被 walk
两次。BuildStorm 早期 200s 窗口中该符号约 246M 指令。

## 改动

RISC-V Sv39 与 LoongArch64 同步：

- `pagetable` 新增 `translate_addr_with_perm()`，一次 walk 返回 `(PhysAddr, PagePerm)`。
- `copy_from_user_in_aspace()` 使用新方法，权限校验与地址翻译共用一次页表查找。

## pc-hot 同窗口结果

同为 RISC-V Final 早期 200s 窗口，QEMU 8 vCPU / 8 GiB / `-snapshot`，绑定 P-core。

| 指标 | 优化前 | 优化后 |
|---|---:|---:|
| `Sv39UserMemoryOps::copy_from_user` | 245.9M | 236.5M |

目标符号下降约 4%，同时消除了每页一次重复 walk。

## 材料

```text
pcs: /tmp/pcs-rv-usercopy-single-walk-20260808.txt
  sha256: 4a23d4040804d43ed21feebe6c47d4baa50027bf79e63667175663d9147974e3
raw_log: /tmp/usercopy-single-walk-pc-hot.log
  sha256: 070103f22c86ed90fe203f09d5cf1f9c65b0fc44162942e96efd2feefa6c3382
pre_smoke: /tmp/usercopy-single-walk-pre-smoke.log
  sha256: 8f6a29af27a2cbc5fe2281fed9f8f09dceceb6dd51e6300a523f3fe96d7ed23d
```

## 验证

- `make check ARCH=rv PROFILE=final` 通过
- `make check ARCH=la PROFILE=final` 通过
- RISC-V pre smoke 进入 LTP，无 panic/fatal

## 后续

- 完整 BuildStorm 和 read-family 定向回归仍需最终门禁。
- 写路径 `copy_to_user_progress` 仍可继续合并 COW/权限/翻译的 walk，下一轮候选。
