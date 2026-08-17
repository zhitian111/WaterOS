# 任务 01：增加低开销热路径计数

## 任务内容与目标

在现有诊断 feature 下统计 mount namespace 快照、FD registry/资源探测、ext4 lookup、
getattr、正缓存清空、`flush_all`、TLB flush/shootdown 和 ELF lazy fault/shared hit。先取得
真实频次，再用同一组指标验收后续任务；默认生产配置不得增加日志或热路径格式化。

## 实施方案

1. 复用 `cache-layer-diagnostics`、syscall profiler 或现有原子诊断设施，不新增全局热锁。
2. 每项只用 relaxed per-CPU/原子计数；低频汇总时再格式化输出。
3. 默认 feature 关闭时编译为空操作，并验证开启/关闭两种配置。
4. 计数名称和单位稳定，后续简报引用相同字段。

## 涉及文件

- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/mount_table.rs`
- `os/components/wateros-vfs/src/fd.rs` 与 `impl-fd-session/src/registry.rs`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/{filesystem,operations,path_lookup}.rs`
- `os/components/wateros-mm/mm-impl/impl-{sv39,loongarch64}/src/user_aspace.rs`
- 现有 diagnostics 汇总模块及相关 `Cargo.toml`

## CodeGraph 查询

```bash
codegraph explore "mount_namespace_snapshot with_registry lookup flush_all request_tlb_shootdown"
codegraph impact "mount_namespace_snapshot"
codegraph callers "request_tlb_shootdown"
```

## 验收方式

```bash
cd os
make rv_check
make la_check
make kernel-rv-final
# 再以 diagnostics feature 构建一次对应 kernel
cd .. && git diff --check
```

用任务 00 runner 做短时采样，确认计数非零、无高频逐事件日志；关闭 diagnostics 后检查
ELF size/反汇编或 feature tree，证明未保留不必要热路径工作。

## Commit 与简报

提交建议：`[perf] 增加 syscall 链路低开销计数`。新增 `history/01-brief.md`，附基线计数和
日志路径；这些数据是任务 02 以后收益判断的前置条件。
