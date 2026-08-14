# Task 08：最终功能/bug 验收

## 任务目标

在最终分支上做完整功能回归，证明两个架构的 slab 化内核没有功能回归和已知 bug。
本任务只允许修复验收中发现的问题；任何修复都必须作为独立 commit 先进入前面
对应任务或一个明确的修复 commit，并更新简报。

## 前置条件

- task 00–07 全部完成，且每个任务简报齐全；
- 工作树干净或只包含本任务预期文件；
- 基线镜像输入 SHA 未变化。

## 验收机制

最终功能验收采用“每架构连续 3 轮全新镜像全量 buildstorm”：

1. 每次测试从 `~/Downloads/*.img.gz` 重新解压新镜像；
2. 覆写 `/glibc/buildstorm_testcode.sh` 为恢复脚本；
3. 使用 QEMU 9.2.1 和线上等价参数；
4. 所有 QEMU 运行均加 `-snapshot`，避免污染镜像；
5. 每轮同时满足：

   ```text
   TOOLCHAIN_RESULT status=OK
   MINIBUILD_RESULT status=OK
   BUILDSTORM_RESULT mode=multi status=OK rc=0 ... run=OK
   WaterOS: all commands finished
   ```

6. 无以下异常：

   ```text
   panic / Panicked at
   [heap] OOM
   ENOMEM / Cannot allocate memory
   SIGSEGV / LoadPageFault / StorePageFault
   invalid TLSF dealloc
   recursive heap allocation detected
   shootdown timeout
   ```

任一架构任一轮失败，验收不通过，回到对应任务修复后重新开始该架构 3 轮。

## 运行命令

与 README 中的 LA/RV QEMU 9.2.1 命令一致，日志命名：

```text
/tmp/wateros-slab-la-final-functional-N.log
/tmp/wateros-slab-rv-final-functional-N.log
```

## 附带静态检查

```bash
cd /tmp/wateros-kernel-heap-slab/os
make rv_check
make la_check
git diff --check
```

## 交付物

- `history/08-brief.md`：6 轮结果、日志 SHA、任何修复 commit 记录。
- 若触发文档同步，一并列出。

## 完成后

新增 `history/08-brief.md`。只有本任务通过后才进入 task 09 性能终验。
