# BuildStorm 共享文件映射回写修复记录

## 问题

BuildStorm 链接器通过可写 `MAP_SHARED` 映射生成 ELF。原实现只把文件内容 eager
载入共享物理页，没有保留文件后备，也把 `msync` 实现为无操作。链接完成后文件 inode
具有正确长度但没有数据块，最终由 ELF 解析器报告 `Unknown file magic`。

## 修复

- `DemandPageLoader` 增加页写回和 flush 契约。
- MM 地址空间记录共享文件 VMA 及其后备句柄，并在 fork 时复制该状态。
- `msync`、`munmap`、`MAP_FIXED` 覆盖和地址空间销毁前回写映射页。
- RISC-V Sv39 与 LoongArch64 使用相同语义；首版保守回写全部相关页。

## 验收

- `make check`、`make la_check`：通过。
- 8 核 BuildStorm：`BUILDSTORM_COMPILE mode=multi ok=true`，编译用时 348.58 秒；
  `llvm-objcopy` 成功读取 ELF 并生成 BIN。
- 离线 `debugfs`：ELF 大小 1,681,000 字节，`Blockcount=3304`，extent 数据完整。
- QEMU 日志 `/tmp/wateros-mmap-writeback-probe-20260803-2.log`，SHA-256：
  `a093d7adf8981e7152f55b760fc972958fa82ebf8c169e406fc6335fd2a9aeff`。

