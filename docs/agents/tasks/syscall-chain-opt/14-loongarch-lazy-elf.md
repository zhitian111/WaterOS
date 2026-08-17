# 任务 14：解决 LoongArch 特有约束后启用 lazy ELF

## 任务内容与目标

在任务 13 已稳定的基础上处理 LoongArch 装载后 musl 指令 patch、entry 校验等要求驻留页的
路径，再为 LoongArch profile 显式启用 lazy ELF。不得机械复制 Sv39 feature 开关。

## 实施方案

1. 盘点 `read_mapped_u32`、musl sched stub patch、entry verify 和 signal trampoline 对驻留页
   的要求；只预 fault/私有化确实需要 patch 的页。
2. patch 页不得进入只读共享 cache，修改后正确执行 LoongArch icache/TLB 同步。
3. 其它 PT_LOAD 保持 lazy，继续通过 ExecFile fault；BSS、重叠 segment 与解释器保持正确。
4. 单独启用 LoongArch profile feature，并保存 feature tree。
5. 没有可用 LA QEMU/镜像时不得标记完成，只能记录阻塞简报。

## 涉及文件

- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/kernel_elf.rs`
- LoongArch page fault、icache/TLB 相关实现
- `os/components/wateros-mm/Cargo.toml`、`os/Cargo.toml`
- 平台 feature 文档

## CodeGraph 查询

```bash
codegraph explore "patch_loongarch_musl_sched_stubs read_mapped_u32 verify_mapped_entry elf-lazy-map"
codegraph callers "patch_loongarch_musl_sched_stubs"
codegraph impact "read_mapped_u32"
```

## 验收方式

```bash
cd os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
# 使用项目 LA snapshot runner 跑 exec/musl/BuildStorm 回归
cd .. && git diff --check
```

LA 必须真实启动并通过 musl 动态程序、fork/exec、page fault 与完整 workload；RISC-V 同时回归。
性能 A/B 使用项目固定 LA QEMU 配置，不能用 RV 命令冒充 LA 运行验证。

## Commit 与简报

提交建议：`[perf] LoongArch 安全启用 lazy ELF`。新增 `history/14-brief.md`，明确 patch 页处理、
feature tree 和真实 LA 运行结果。
