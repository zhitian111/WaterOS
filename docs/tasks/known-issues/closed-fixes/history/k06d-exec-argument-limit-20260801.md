# K-06D exec 参数上限与内核 OOM 修复报告（2026-08-01）

## 问题

glibc LTP 全量回归在第 2,536 个用例 `thp01` 中止。该用例构造 8,192 个、每个
4,096 字节的参数，并二分查找 `execvp()` 可接受的最大参数数量。修复前 WaterOS 返回
`EFAULT`，随后内核堆从约 120 MiB 高水位增长到 132.9 MiB，最终因一次 4,095 字节
分配失败而 panic。测试 overlay 经 `qemu-img check` 验证无错误，排除镜像损坏。

LTP 20240524 的 `testcases/kernel/mem/thp/thp01.c` 第 64-77 行明确要求超限的
`execvp()` 返回 `E2BIG`；该用例用于回归 CVE-2011-0999。

## 根因与修复

`execve::read_string_array()` 原先没有数组数量或总字节上限，并通过路径字符串 helper
为每个参数分配最多 4 KiB。随后 ELF 解析、最终 argv 和用户栈构造还会复制同一批数据，
恶意或探测性参数可在系统调用返回前耗尽 128 MiB 内核堆。

修复在导入用户数据时执行：

- argv 和 envp 共享 2 MiB 总上限，并为固定 2 MiB 用户栈预留 16 KiB argc/auxv 空间；
- 预算计入字符串 NUL 和每个用户指针，使用 checked 地址/长度运算；
- 达到总预算或单参数上限时返回 `E2BIG`；
- 用户栈构造发现 `StackOverflow` 时同样返回 `E2BIG`，保留真实访问错误为 `EFAULT`。

## 验证

- `make rv_check`：通过。
- `make la_check`：通过。
- RISC-V64/OpenSBI/8 CPU，新 qcow2 overlay，镜像原生 glibc LTP `thp01`：按原始
  `ltp_testcode.sh` 入口运行，二分搜索得到 507 个参数可执行、508 个参数返回
  `E2BIG`，最终 `TPASS: system didn't crash.`，用例返回 0。
- 测试未出现 heap high-water、OOM、panic 或内核态 page fault。

本轮仍由 bringup 清理了 1 个遗留用户任务，属于待单独处理的进程生命周期/测试隔离
问题，不影响本项对 `E2BIG` 和 OOM 防护的判定。glibc 全量回归需在本修复后重新执行。
