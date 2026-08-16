# BS-SC-12：只读文件 mmap fault-around

## 执行门禁

仅当完成 trap/syscall/timer 优化后的新画像仍显示用户 page fault 是主要剩余成本，并且只读
文件-backed fault 占有足够比例时执行。先用计数区分 stack/brk/COW/file-read/file-exec，
不得依据旧的总 page-fault 数直接提交预取。

## 任务内容

对只读或可执行的 private file VMA，在处理当前 fault 页时有限地映射相邻顺序页，减少
`.rlib`、object 和 linker 输入的逐页 trap。第一版不对 writable/COW、shared-write、匿名
brk/stack 做 fault-around。

## 实施方案

1. 在 common lazy VMA fault 路径实现固定上限窗口，首选向前 8 页，并允许通过诊断结果调整。
2. 窗口不得跨 VMA 边界、文件映射长度、权限边界或已映射页；当前 fault 页失败仍返回错误，
   邻页预取失败只停止窗口，不把已成功的当前页转为失败。
3. 只对 `!perm.writable()` 使用 shared readonly page cache；保持 content version、frame refcount
   和并发 duplicate-load 语义。
4. 对已经安装的邻页正确释放临时 frame；任何 map 失败都不得泄漏或双重 dealloc。
5. TLB invalidation 只覆盖必要范围；新建不存在的 PTE 若架构不需要 flush，应以架构契约证明，
   不凭经验删除。
6. 增加计数：faults handled、pages mapped、window stop reason、unused prefetched pages；release
   默认关闭重诊断。
7. Sv39/LoongArch 共享机制放 common，架构层只做必要的映射/flush，不复制算法。

## 涉及文件

- `os/components/wateros-mm/mm-impl/common/src/{fault,vma,cache}.rs`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/pagetable.rs`
- mmap loader/page cache 适配层（仅契约需要时）
- `os/scripts/perf/guest-probes/mmap_fault_around.sh`
- `history/12-brief.md`

## CodeGraph 查询

```bash
codegraph explore "handle_lazy_file_fault handle_lazy_page_fault load_shared_page"
codegraph impact "handle_lazy_file_fault"
codegraph explore "MmapReadonlyPageCache load_or_get_readonly_mmap_page frame refcount"
codegraph callers "handle_user_page_fault"
```

## 验收方式

host/self-test 覆盖 VMA 首尾、短文件、非页对齐 EOF、已映射邻页、权限拒绝和中途分配失败。
guest probe 覆盖顺序/随机只读、RX、private writable COW、shared mapping、mprotect/munmap、truncate
或内容版本变化、两线程并发 fault；前后文件内容和 frame/refcount 必须一致。

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
git diff --check
```

再运行双架构完整 BuildStorm。除了 elapsed，还必须证明 `user_pf` 和 file-backed fault 数下降，
内存峰值/预取浪费有界；墙钟不改善则恢复实现并提交拒绝简报。

## 完成后简报

新增 `history/12-brief.md`，记录执行门禁画像、窗口选择、功能矩阵、fault/page/refcount/内存计数、
双架构结果和保留决定。
