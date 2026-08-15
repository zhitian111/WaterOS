# Task 04 简报：重新打开 elf-lazy-map 与双架构回归（部分）

## 状态

部分完成。VMA 分支基于最新 main rebase 后，`elf-lazy-map` 保持默认开启。

## 已验证

- `make rv_check` PASS
- `make la_check` PASS
- `make kernel-rv-final` PASS
- `make kernel-la-final` PASS
- LA 12 核完整 BuildStorm PASS：
  `BUILDSTORM_RESULT status=OK rc=0 elapsed_s=555.51 run=OK`
  - 日志：`/tmp/wateros-vma-la-smp12-rebase.log`
  - SHA-256：`220235283f215c0bec45a2b0e9092b5269fd651ced556180fa3496c7fcdcac74`

## 未完成

- RV 8 核完整 BuildStorm 多次在 buildstorm 阶段被外部 SIGTERM 终止，尚未得到
  最终结果标记；
- 待无其他 QEMU 窗口后补跑 RV 8 核，并将完整结果写回本简报。
