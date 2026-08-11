# K-07 mmap 游标与 BuildStorm 验证结果

## 问题与定位

RISC-V64 8 核 final BuildStorm 长时间运行时，不能仅凭串口静默判断死锁。使用
`os/scripts/wateros_debug.py snapshot` 对运行中的 QEMU 取样后，确认任务和系统调用
计数持续增长；快照还捕获到 CPU 位于
`find_free_mmap_base_considering_vmas()`。检查代码发现 Sv39 和 LoongArch64 都维护并
更新 `mmap_anon_cursor`/`mmap_file_cursor`，但空闲区查找实际忽略传入游标，每次都从
`mmap_base` 重新逐页扫描。

## 修复

两套页表实现现在以以下三者的最大值作为搜索起点：

- 调用方传入的匿名或文件映射游标；
- 地址空间的 `mmap_base`；
- 当前 `brk` 末端的页边界。

原有 VMA、栈、内核保留区和页表冲突检查保持不变。该修改不涉及 task scheduler、
地址空间公开 API 或 TLB 协议。

涉及文件：

- `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/pagetable.rs`

## 验证

- `make check`：通过。
- `make la_check`：通过。
- RISC-V64/OpenSBI/QEMU，8 核，主办方 final 镜像，snapshot 模式：
  CAgent 10/10 通过；BuildStorm toolchain、minibuild 和完整 compile 均通过。
- 完整标记：
  `BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=6012.56 cores=8 bytes=1681000 arch=riscv64`。
- release 编译耗时 `96m45s`，无 panic、OOM、死锁或测试超时。

日志：`/tmp/wateros-rv-buildstorm-mmap-cursor.log`。

调试快照显示慢阶段可分为单个 `rustc` 的用户态 CPU 计算，以及多个编译任务竞争
page-cache/ext4 写路径；修复后未再采到 mmap 起点线性回扫。修复前基线在 37 分钟时
仍健康运行并被人工终止，因此本轮只能证明修复后的完整正确性，不能给出严格的整轮
A/B 加速比例。后续应把 page-cache/文件系统写入串行化作为独立性能任务处理。
