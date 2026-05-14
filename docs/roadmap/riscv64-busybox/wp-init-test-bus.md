# 工作包：RISC-V64 init/test 总线集成（不含 self_tests）

**所属**：根 crate `wateros`（`os/src/main.rs`）、可选新增 `os/src/bringup_*.rs` 薄模块。  
**并行度**：可与各组件实现并行，但需**尽早冻结总线契约**（日志前缀、调用顺序、失败策略）。

## 要做什么

1. 在 **`qemu_riscv64_opensbi::kernel_main`** 中，在 **`driver` 成功、`fs::init` 之后**，定义一条与现有 **`fs::test()`、`vfs::test()`** 同类的 **用户态 bring-up 总线**（建议新建模块，例如 `os/src/user_bringup_bus.rs`，由 `main.rs` `mod` 引入）。
2. 总线职责：
   - 按**固定顺序**调用各阶段入口（由各工作包在对应 crate 提供 `pub fn test()` 或 `pub fn run_stage_*()`，再由总线聚合调用）。
   - 统一日志标签，例如 `[bringup][stage-02-mm] ...`，便于 CI/人工 grep 验收。
   - **不修改** `os/src/self_tests/`：阶段任务与用户态回归与 `self_tests::task::spawn_all` 解耦。
3. 与现有顺序的约束（见 `os/src/main.rs` 顶层文档）：
   - 任何依赖 **根卷 ext4 与块设备一致视图** 的用户 ELF 加载/校验，须安排在 **`fs::test()` 的 RW 写盘段之前**执行，或与 `fs::test` 的 RO 段顺序一致且**不写盘**；若某阶段必须写盘后再测，须在总线文档与本文件中明确顺序，避免 `from_elf_path` 类路径读到陈旧块缓存。
4. 在 `docs/roadmap/todolist.md` 或本目录 `README.md` 维护「当前已登记的阶段列表」。

## 验收要求

- [ ] `kernel_main` 在 riscv64 路径上存在**单一、可读**的「bring-up 总线」注释块，顺序与 `main.rs` 文档一致。
- [ ] 每个已合并的 bring-up 阶段在失败时行为明确：**`warn` 后继续** vs **`panic`/停机**；默认与 `fs::test` 一致为 **非致命 warn**（除非某阶段契约定义为硬门禁）。
- [ ] 不通过 `self_tests` 即可在日志中看到各 `stage` 的 **BEGIN/END** 或等价成对标记（由各子包实现，总线只负责调用次序）。

## 验证方式

1. **本地**：`make`（或项目既定 riscv64 构建命令）后，按 `os/scripts/test_in_qemu_riscv.sh` 或等价 QEMU 启动，在串口日志中 `grep` 统一前缀（如 `\[bringup\]`），确认阶段顺序与 END 标记。
2. **回归**：新增阶段时，在 PR 描述中粘贴「总线片段日志」；可选后续在 CI 中对固定子串做弱断言（本工作包不要求实现 CI，仅预留约定）。
3. **负例**：人为使某一阶段失败（例如临时改错路径），确认总线策略与文档一致（warn 继续或中止）。

## 依赖与接口

- **被依赖**：所有其它 `wp-*.md` 工作包最终将「验收入口」登记到本总线。
- **依赖**：无（可先做骨架与空阶段）。

## 可并行对象

与任意组件工作包并行；建议**第一个落地**以冻结约定。
