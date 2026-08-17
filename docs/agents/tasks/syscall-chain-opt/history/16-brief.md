# 任务 16 简报：munmap 按实际叶 PTE 变化同步

## 实现

- MM API 增加 `PteChange::{None, Changed}`。
- Sv39 与 LoongArch64 的 unmap range 统计实际移除的叶 PTE。
- `munmap`、`munmap_external` 和 SysV SHM detach 通过统一 facade 条件执行本地 flush 与远端 shootdown。
- 删除双架构 munmap 实现内部的重复全量 fence；错误路径仍保守同步。

## 验证

- `make rv_check`
- `make la_check`
- `make kernel-rv-final`
- `make kernel-la-final`
- `git diff --check`

尚未执行 lazy-only、部分驻留和 SMP stale-TLB 的 QEMU 定向回归。
