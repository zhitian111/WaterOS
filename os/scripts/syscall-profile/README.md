# syscall-profile

[项目首页](../../../README.md) · [脚本总览](../README.md) · [工具文档](../../../docs/tools/README.md)

这个 QEMU 插件按 Linux syscall ABI 聚合调用次数、参数、路径复用和返回值，不逐条打印。

后端：

- `backend=auto`：full-system 使用 RISC-V `ecall`，linux-user 使用 QEMU callback；
- `backend=ecall`：读取 `a7` 和 `a0..a5`，适合 WaterOS RISC-V full-system；
- `backend=qemu`：使用 QEMU syscall entry/return callback，适合 linux-user。

构建：

```bash
./scripts/syscall-profile/syscall-profile-rv.sh build
```

运行：

```bash
./scripts/syscall-profile/syscall-profile-rv.sh run /tmp/syscalls.txt \
    backend=auto paths=1 max_path=256 top_paths=200 -- \
    timeout 120 qemu-system-riscv64 -machine virt -kernel ./kernel-rv-final \
    -m 16G -nographic -smp 8 -bios default -snapshot ...
```

结果只在 QEMU 退出时写入。记录类型：

- `S`：syscall 总数及 per-vCPU 数量；
- `A`：参数数量级分桶；
- `V`：flags/op 等枚举参数的精确值；
- `R/E`：native callback 后端的成功、失败及 errno；
- `P/D/PV`：路径读取汇总、复用距离和高频路径；
- `X`：寄存器读取失败与被过滤的内核/SBI ecall。

插件结果属于诊断数据，不作为 BuildStorm 墙钟验收成绩。

生成 Markdown 摘要：

```bash
./scripts/syscall-profile/analyze.py /tmp/syscalls.txt --top 30 > /tmp/syscalls.md
```
