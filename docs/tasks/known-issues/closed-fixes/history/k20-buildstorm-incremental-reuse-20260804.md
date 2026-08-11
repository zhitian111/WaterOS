# BuildStorm 增量复用与时间戳风险验证

## 验证目标

完整 LoongArch BuildStorm 日志中存在 Cargo global-cache last-use 写入警告，且日志时间为
1970 年。K-01 要求先验证时间戳是否导致 dirty rebuild，再决定是否扩展 FS/VFS metadata
公共契约。本轮不修改评测脚本和持久化实现，只在已完成完整编译的测试镜像中执行增量
探针。

## 方法

使用 K18 完整 BuildStorm 的 qcow2 overlay，经 K19 inode bitmap 修复后启动 8 核
LoongArch final。探针保持 `/work/tgoskits/target`，不执行正式脚本中的：

```sh
rm -rf "target/$AXTGT"
```

随后运行与正式测试相同的命令：

```sh
cargo xtask arceos build -p arceos-helloworld --arch loongarch64
```

该方式同时验证重启后的 target 复用；探针只替换测试镜像内已有脚本的数据块，不修改
仓库或原始主办方镜像。

## 结果

- `tg-xtask` freshness 检查完成于 48.02 秒，没有重新编译；
- 内层 release freshness 检查完成于 1 分 14 秒，没有出现任何 `Compiling`；
- ArceOS 构建步骤报告 `done (172.61s)`；
- 整个增量命令耗时 251.22 秒，退出码为 0，已有 ELF/BIN 产物被正常复用；
- CAgent 同轮 10/10 通过；
- 写后 `e2fsck -fn` 五阶段完成，返回 0。

串口日志 SHA-256：
`98fb08025ffd5014f00066a4ab7c46621ca6d6cc805a1f485d72d330ce46c5a7`。
fsck 日志 SHA-256：
`87064fb2541bead1cdd4e8b11fafa40b80f2f065fbe43fe22ac25ed120987b69`。

## 结论

Cargo 的 `failed to save last-use data` 警告稳定存在，但它只影响 global cache 的自动
清理记录；当前证据不支持它导致 BuildStorm 产物失效或全量重编译。增量构建仍慢，主要
表现为 QEMU LoongArch TCG 下 Cargo/xtask 的大量文件 metadata 扫描和进程启动成本。

依据 K-01 的“有证据再扩展 API”约束，本轮不把 atime/mtime/ctime 加入公共 FS/VFS
契约。持久化时间戳仍是标准兼容性缺口，应作为独立任务处理，但不再作为 BuildStorm
正确性阻塞项。
