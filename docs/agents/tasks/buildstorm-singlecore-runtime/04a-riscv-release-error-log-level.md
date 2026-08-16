# BS-SC-04A：RISC-V 正式构建使用 Error 日志上限

## 任务内容

将 RISC-V64 正式平台 profile 的编译期日志上限从 `Info` 调整为 `Error`，与 LoongArch64
一致。此前 RV BuildStorm 串口持续输出 `[paged_handle]` 热路径 INFO，结果受日志格式化和
串口 I/O 污染，不作为批次 A 的性能证据。

## 实施方案

1. 在顶层 `qemu-riscv64-opensbi` feature 中改为唯一启用 `runtime/impl-error`。
2. 同步根 README、`os/README.md` 与 Makefile 工具文档中的平台日志契约。
3. 重新执行双架构 check 和 Final build；检查 RV ELF 不再包含 paged-handle INFO 格式串。
4. 不在本任务运行 BuildStorm；BS-SC-04 使用重新构建的 ELF 重跑受污染的 RV 性能验收。

## 验收方式

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
make rv_check
make la_check
make kernel-rv-final
make kernel-la-final
! strings kernel-rv-final | rg '\[paged_handle\] seek path='
cd ..
git diff --check
```

功能 probe 使用新 RV ELF 补跑一次，确认编译期裁日志不改变 FPU、signal、timer 和进程状态
语义。此前使用 Info ELF 得到的 `362.05s FAIL` 与 `566.44s OK` 仅保留为无效配置记录。

## 涉及文件

- `os/Cargo.toml`
- `README.md`
- `os/README.md`
- `docs/tools/makefile.md`
- `docs/agents/tasks/buildstorm-singlecore-runtime/README.md`
- `docs/agents/tasks/buildstorm-singlecore-runtime/history/04a-brief.md`

## CodeGraph 查询

```bash
codegraph explore "runtime logging compile-time maximum log level qemu-riscv64-opensbi"
rg -n 'runtime/impl-(trace|debug|info|warn|error)' os/Cargo.toml
```

## 完成后简报

新增 `history/04a-brief.md`，记录旧/新 ELF SHA-256、实际 feature、双架构 check/build、字符串
审计结果，以及 BS-SC-04 必须重新执行的功能和性能项目。
