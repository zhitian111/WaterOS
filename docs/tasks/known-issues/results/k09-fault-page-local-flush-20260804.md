# K-09 Page Fault 页级本地 TLB 刷新

## 结论

用户 stack、brk 和 lazy file fault 建立的是 invalid -> valid PTE。该变化不需要向所有
曾运行过该地址空间的 CPU 发送 shootdown：未访问该页的 CPU 没有旧有效映射；并发访问
该页的 CPU 会进入 fault handler，在地址空间锁内看到已有 PTE 后刷新自己的当前页。

RISC-V 现在对上述路径执行一次 `sfence.vma va, x0`，不再由统一入口追加一次全本地
TLB 刷新和远端 IPI。LoongArch 复用相同的 `TlbFlushRange::Page` 契约；当前架构后端仍将
它保守实现为本地 `invtlb_all`，但同样消除了不必要的远端 shootdown。

COW 会把有效只读映射改为可写或切换物理页，仍由独立 `handle_cow_fault()` 路径执行
本地全刷和远端 shootdown，本次没有改变其一致性语义。munmap、mprotect、fork 和地址
空间销毁等 valid -> invalid/changed 路径也未改变。

## 修改

- `os/components/wateros-mm/src/lib.rs`
  - 普通用户 page fault 改用 `with_user_aspace_mut()`，去掉无条件全核 flush。
- `os/components/wateros-mm/mm-impl/impl-{sv39,loongarch64}/src/pagetable.rs`
  - lazy file fault 新建页或观察到并发已建页时，仅刷新当前 fault 页。
- `os/components/wateros-mm/mm-impl/impl-{sv39,loongarch64}/src/user_heap_mmap.rs`
  - stack/brk fault 使用相同的当前页刷新规则。

## 验证

- `make check`：通过。
- `make la_check`：通过。
- `python3 -m unittest discover -s scripts/tests -v`：25/25 通过。
- RISC-V final、8 核、snapshot：CAgent 10/10 通过，统计 `user_pf=13482`。
- 同一轮 BuildStorm：`BUILDSTORM_TOOLCHAIN ok`、`BUILDSTORM_MINIBUILD ok`，完成
  `tg-xtask` 构建并进入正式 Cargo release 编译；观察阶段无 panic、重复 fault、错页或
  TLB shootdown 停滞。日志：`/tmp/wateros-rv-k09-fault-page-flush-final.log`。

本轮是白天限时回归，未等待约 100 分钟的完整 BuildStorm，因此不据此宣称端到端耗时
改善。完整性能对比应在夜间门禁中记录三轮总耗时和 TLB shootdown 事件数。

## GDB 诊断接入

`os/components/wateros-debug` 与 `os/scripts/wateros_debug.py` 可记录每 CPU 的 trap、IPI、
TLB shootdown 和锁等待，并用 build ID 校验 guest/ELF。后续异常优先使用
`wateros_debug.py watch`，确认停滞后再保存 snapshot。当前主机的通用 `/usr/bin/gdb`
支持多目标，但工具的 `doctor` 要求名为 `gdb-multiarch` 的命令，因此自动完整报告需先
安装该包；QEMU、RISC-V `nm` 和 `addr2line` 已通过检查。
