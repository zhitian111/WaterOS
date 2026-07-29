# 决赛后续任务路线图

## 目的

本文是 2026-07-29 之后的总入口。详细任务按阶段拆分，会议只更新阶段状态和证据，
不再维护一份不断增长的混合清单。

当前已确认：

- CAgent 已连续三轮 10/10 通过。
- Cargo 大文件读取失败已定位为 `read(2)` 对超过 4 MiB 请求返回 `EINVAL`，修复后
  `cargo metadata --offline` 成功，索引文件保持完整。
- 修复后 BuildStorm 已进入 446 个编译单元的实际编译，但尚未取得
  `BUILDSTORM_COMPILE mode=multi ok=true`。
- 运行中仍有 `fsync fd=6 flush failed: Unsupported`，其 fd 类型和 Linux 语义尚未
  闭环。
- 当前可以排除参考镜像损坏和 another-ext4 基本读写不兼容，不能据此宣称全部兼容性
  工作已经完成。

最新问题证据以
[`../tasks/buildstorm-cargo-index-filesystem-report.md`](../tasks/buildstorm-cargo-index-filesystem-report.md)
为准；旧文档中“镜像缺少 web-sys”的结论不再适用。

## 阶段顺序

| 阶段 | 文档 | 核心出口 |
|---|---|---|
| 0 | [`final-phase-0-compatibility-closure.md`](./final-phase-0-compatibility-closure.md) | BuildStorm 完整成功，CAgent/初赛无回归 |
| 1 | [`final-phase-1-buildstorm-measurement.md`](./final-phase-1-buildstorm-measurement.md) | 得到可复现基线和瓶颈排序 |
| 2 | [`final-phase-2-performance-optimization.md`](./final-phase-2-performance-optimization.md) | 有 A/B 数据对比的有效优化 |
| 3 | [`final-phase-3-regression-delivery.md`](./final-phase-3-regression-delivery.md) | 双架构最终验收与提交材料 |

阶段是强制门禁：阶段 0 未完成前，不合入改变调度、缓存策略或页表语义的性能优化；
阶段 1 没有数据前，不按主观判断挑选高风险优化。

## 两周建议排期

| 时间 | 目标 | 会议应做的决定 |
|---|---|---|
| 第 1 周一至周三 | 阶段 0 | 确认首个真实失败点及负责人 |
| 第 1 周五 | 阶段 0 出口、阶段 1 启动 | 决定是否允许进入性能测量 |
| 第 2 周一 | 阶段 1 结论 | 冻结瓶颈 Top 3 和优化分工 |
| 第 2 周三 | 阶段 2 中检 | 只保留有正收益且无回归的改动 |
| 第 2 周五 | 阶段 3 | 冻结候选提交、结果和交付材料 |

## 责任边界

- 成员 A：syscall、VFS/ext4、块与页缓存、测试集成、数据汇总和最终文档。
- 成员 B：task、scheduler、futex、进程生命周期及相关测量。
- 成员 C：`driver/network`、CAgent 网络稳定性和网络性能回归。
- 跨模块修改先冻结接口；每个优化单独提交，不把诊断日志和正式修复混在一个提交。

## 统一证据格式

每轮结果必须记录 commit、架构、QEMU 参数、镜像 SHA-256、overlay 名称、执行命令、
开始/结束时间、成功标记、第一个失败点和日志路径。性能结果至少运行三次，报告中保留
每次原始值和中位数；失败轮不能从统计中静默删除。
