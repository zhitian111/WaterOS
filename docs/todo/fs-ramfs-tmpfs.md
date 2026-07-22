# 待办：ramdisk / ramfs / tmpfs

## 目标

为 WaterOS 补齐内存侧存储能力，分层如下：

| 组件 | 层级 | 职责 |
|------|------|------|
| **ramdisk** | `wateros-driver`（块设备） | 一段可视为磁盘的内存，实现现有 `BlockDevice` |
| **ramfs** | `wateros-fs` | 「数据在 RAM 上」的文件系统；一套实现、两种后端 |
| **tmpfs** | `wateros-vfs`（策略层） | 挂载点 / size 限额 / 默认路径语义；底层复用 ramfs |

设计取舍：

- **不强制** ramfs 走块层；日常 tmpfs 主路径用无 ramdisk 后端。
- ramdisk 路径保留，主要为 **块 API 一致性** 与调试（可与现有 probe/mount 管线对齐）。
- 生态中无可直接对接本仓库 `FsImpl` / `BlockDevice` 的 drop-in 库；`axfs_ramfs` 仅作实现参考，**不依赖外部 FS crate 落地**。

参考：ArceOS [`axfs_ramfs`](https://crates.io/crates/axfs_ramfs) / [`axfs_vfs`](https://crates.io/crates/axfs_vfs)；本仓库已有迷你内存盘雏形 `SampleBlockDevice`（`driver-block/block-api/api-v0`）。

---

## 任务列表

### 阶段 0 — 契约与骨架

- [x] **T0.1** 在 `fs-api` 增加 `FsKind::RamFs`（或 `Other("ramfs")` 过渡方案），并明确与 `DevFs` / 块卷 FS 的登记方式
- [ ] **T0.2** 定义 ramfs **存储后端**抽象（名称待定，例如 `RamBackend`）：至少支持 `Heap` 与 `RamDisk` 两种
- [x] **T0.3** 明确 tmpfs 仅做 vfs 策略：不另实现目录树；挂载时选择 / 包装 ramfs 实例
- [ ] **T0.4**（可选）对照 `docs/guides/filesystem-current.md` / 现有 `wateros-vfs` 挂载表，标出 `/tmp`、`/run` 等目标挂载点

### 阶段 1 — ramdisk（driver）

- [ ] **T1.1** 新增 `driver-block` 实现：固定 `block_size`（建议 512 或与 `BLOCK_SIZE` 一致）+ 可配置块数的内存缓冲
- [ ] **T1.2** 完整实现 `BlockDevice::read_blocks` / `write_blocks`（相对 `SampleBlockDevice` 补齐可写与可扩容）
- [ ] **T1.3** 注册到块设备表；devfs 可见节点（例如 `/dev/ram0` 或内部路径约定）
- [ ] **T1.4** 单元/自测：随机读写、越界拒绝、与 `CachingBlockDevice` 组合冒烟（可选）

### 阶段 2 — ramfs（fs，无 ramdisk 后端优先）

- [x] **T2.1** 新建 `fs-ramfs`（或 `fs-impl/impl-ramfs`）crate，注册进 `wateros-fs` 聚合层
- [x] **T2.2** 实现目录 / 文件最小语义：`mkdir`、lookup、create、read、write、unlink（对齐现有 `FsImpl` / RO·RW trait 能接到的能力）
- [x] **T2.3** 默认后端：堆分配（`Vec`/`BTreeMap` 等；具体结点布局可后定，先保证语义）
- [x] **T2.4** 容量统计钩子（已用字节数），为后续 tmpfs `size=` 预留
- [x] **T2.5** 自测：树操作 + 内容读写 + 超限（若有硬限）返回 `NoSpace`

### 阶段 3 — ramfs + ramdisk 后端

- [ ] **T3.1** 第二种后端：把文件内容/元数据落在 `RamDisk` 块缓冲上（布局可极简，不必做成完整磁盘 FS 格式）
- [ ] **T3.2** 验证「同一套 ramfs API，可切换后端」；文档注明该路径用于 API 对齐/调试，非 `/tmp` 默认
- [ ] **T3.3**（可选）走现有 `probe`/`mount_*_from_block_path` 旁路，确认块管线可挂内存盘

### 阶段 4 — tmpfs（vfs）

- [x] **T4.1** vfs 侧增加 `tmpfs` 挂载类型 / 工厂：创建「无 ramdisk 后端」的 ramfs 实例并挂入命名空间
- [x] **T4.2** 支持 `size`（或等价限额）；写满返回与 `FsError::NoSpace` / errno 对齐的错误
- [x] **T4.3** 启动或 bring-up 默认挂载：至少 `/tmp`（按需再挂 `/run`）
- [ ] **T4.4** 与页缓存 / fd / cwd 路径联调：open-read-write-unlink 冒烟；确认不污染磁盘 rootfs

### 阶段 5 — 收尾与文档

- [ ] **T5.1** feature 开关（如 `ramfs` / `tmpfs`），默认是否启用以启动内存与测试策略为准
- [ ] **T5.2** 更新架构/指南文档中「supported FS」与挂载说明（含命名：本仓库 ramfs ≠ Linux 块层无关 ramfs 的字面等价）
- [ ] **T5.3** QEMU：`rv_check` / `la_check`（或现有等价目标）回归；必要时加一条 busybox/脚本级 `/tmp` 写测

---

## 建议实施顺序

```text
T0.* 契约
  → T1.* ramdisk（可与 T2 并行，但 T3 依赖 T1）
  → T2.* 无块后端 ramfs（主路径）
  → T4.* tmpfs 挂上 /tmp
  → T3.* ramdisk 后端（可后置）
  → T5.* 开关与文档
```

优先打通：**无 ramdisk 的 ramfs + vfs tmpfs → `/tmp`**；ramdisk 与第二后端不阻塞主路径。

---

## 明确不做（本期）

- 引入 `axfs_ramfs` / `bare-vfs` / crates.io `ramdisk` 作为运行时依赖（API 与 `no_std` OS 栈不对齐）
- Linux 式可换出（swap）的 tmpfs
- 单独发明 `varfs`；`/var` 仍走磁盘 root，除非日后显式把某子路径挂到 tmpfs

---

## 状态

| 项 | 状态 |
|----|------|
| 设计讨论结论 | 已定（本文） |
| 代码落地 | 已落地 heap-backed ramfs 与 tmpfs 主路径；ramdisk/第二后端未开始 |
| 更新日期 | 2026-07-15 |
