# Task 06：隔离单核与 SMP allocator 成本

## 任务目标

判断 slab 退化来自本地快路径常数，还是来自 CPU 迁移、remote-free 和共享缓存线；为后续
单核编译链路优化建立可靠的 allocator 归因。

## 实施方案

1. 使用同一候选内核分别运行 `-smp 1`、标准 RV `-smp 8` 和标准 LA `-smp 12`；只改变
   vCPU 数，镜像、QEMU、脚本和 kernel commit 固定。
2. 每个配置做 TLSF/slab A/B/B/A；记录 BuildStorm 内部 elapsed、工具链/minibuild marker、
   CPU 迁移/remote 诊断和宿主内存状态。
3. 将 `-smp 1` 结果作为判断本地 slab 常数的依据，不把单核结果直接作为线上最终成绩。

## 验收方式

```bash
ps -eo pid,ppid,stat,%cpu,etime,args | rg '[q]emu-system|[b]uildstorm' || true
```

运行命令沿用任务 00，仅替换 `-smp`。每轮必须成功完成全部 marker；任何 panic/OOM/
SIGSEGV/fault 或停滞都判失败。输出表至少包含：架构、smp、allocator、elapsed、remote
ratio、drain max、page high-water。

## 涉及文件与 CodeGraph

本任务原则上只新增结果/分析文档；若需补 CPU runtime counter，修改：
`os/components/wateros-task/**`、`os/components/wateros-runtime/runtime-heap-allocator/**`。

```bash
codegraph explore "current_cpu_id scheduler migration task runtime stats"
codegraph callers "current_cpu_id"
```

## 完成后

新增 `history/06-brief.md`，明确 slab 的退化归因和是否允许进入任务 07/08。

