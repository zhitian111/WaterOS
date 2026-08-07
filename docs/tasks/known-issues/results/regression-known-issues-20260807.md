# 已知问题回归汇总（2026-08-07）

## 范围

针对当前已提交最优组合（K-50）运行已知问题回归，不引入新的性能实验。

## 结果

| 项目 | 结果 |
|---|---|
| `make rv_check` | 通过 |
| `make la_check` | 通过 |
| RISC-V Final 构建 | 通过 |
| LoongArch Final 构建 | 通过 |
| RISC-V Final BuildStorm | 通过：`elapsed_s=1728.31`（与 LA 并发运行） |
| RISC-V Pre 60s | 通过：root RW、cyclictest、hackbench 进入执行 |
| BuildStorm 8-crate 并行探针 | 通过：`rc=0 built=8 elapsed_s=43.91` |
| read-family 回归 | 通过：34 passed，12 missing，无真实失败 |
| iozone 回归 | 通过：`iozone test complete` |
| LoongArch Final | 不可验收：构建完成约 5 分钟未打印 `BUILDSTORM_COMPILE`，命中 `cargo xtask` 返回竞态 |
| LoongArch Final 重跑 | 通过：`elapsed_s=1555.69`，CAgent 10/10 |
| LoongArch Pre | 阻断：当前仅有 `sdcard-la-pub.img`，无独立初赛镜像 |

## 可复核材料

```text
commit: 5a27e574 + working tree
rv_final_log: /tmp/reg-rv-final.log
rv_pre_log: /tmp/reg-rv-pre.log
la_final_log: /tmp/reg-la-final.log
la_final_pcore_log: /tmp/reg-la-final-pcore.log
probe_log: /tmp/reg-target.log
read_family_log: /tmp/reg-read-pre.log
iozone_log: /tmp/reg-iozone.log
```

## 尚未闭环的已知问题

- K-02：LoongArch 8 核最终门禁已在本轮重跑通过；偶发 `cargo xtask` 竞态仍需修复。
- K-04/K-05/K-06/K-07/K-08/K-09/K-10：仍需按各自验收清单完成基线、长测与最终交付。
- `cargo xtask` 构建完成后偶发不返回，是当前双架构 Final 的主要阻断。
