# mount namespace Arc 快照实验结果（2026-08-10）

## 结论

该方案未通过完整 BuildStorm，已回退，不进入 main。

## 方案

将 `MountNamespace.entries` 从 `Vec<MountEntry>` 改为 `Arc<Vec<MountEntry>>`，
使 `mount_namespace_snapshot()` 从深拷贝整张挂载表变成一次 Arc 引用计数。写入路径
使用 `Arc::make_mut`，bootstrap 表使用 `RuntimeOnceCell` 延迟初始化。

## 验证

- 双架构 `make check ARCH={rv,la} PROFILE=final` 通过。
- 180 秒 smoke 通过 toolchain/minibuild 并进入编译。
- 完整 RISC-V BuildStorm 两次尝试：
  - `mountns-arc-full-a1`：在 `Compiling ax-posix-api` 附近长时间无进展，1200s 超时。
  - `mountns-arc-full-a2`：cargo build 命令启动后长时间未输出 `Compiling`，1200s 超时。

两轮均在 BuildStorm 正式编译早期停滞，而同一 host 上无此改动的诊断基线
`tlsf-diag-lowoverhead-full` 能完整输出 `BUILDSTORM_COMPILE ok=true`。

## 可能原因

`Arc::make_mut` 在并发读快照与挂载表写入同时发生时，会在持锁路径上触发整表
复制；BuildStorm 启动/编译阶段可能频繁出现 mount namespace 写入与路径解析交错，
导致不可接受的长时间停顿。另一个可能是 `RuntimeOnceCell` 初始化或 `Arc` 生命周期
与当前 mount 表锁序交互不当。

## 后续

不再重复该方案。mount namespace 优化应改为在调用方持有只读快照时避免并发写入
复制，或先证明 BuildStorm 中 mount 表修改频率，再考虑无锁/epoch 快照。
