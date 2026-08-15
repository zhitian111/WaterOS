# 内核待完善清单（LTP + stress-ng）

> 日期：2026-08-15（ARCH=la PROFILE=pre 最新 run.log：内部 TFAIL 8 + TBROK 9；
> stress-ng 0.19.02 实测 RISC-V guest）
> 说明：已修复项在文末对照；本文只记**仍待完善**的问题。测完一批一起修。

---

## 一、LTP 剩余失败（最新 run.log）

### 🔴 P0-CRASH — mmap 类 stressor 整机死机（stress-ng，比下面所有项都严重）

- **现象**：`stress-ng --vm-addr 2 --vma 2 --mmap 2 --mmapfork 2 --timeout=20` →
  `mmapfork: total 7.71G of 7.71G available memory` 后 **QEMU 退出 247（guest 死机）**，
  **无任何内核日志**（无 panic 文本、无 `[heap] high water` warn、无 frame 分配失败）。
- **已验证**：`SMP=1` 单核同样崩 → **排除多核并发竞争**，属资源/内存耗尽类。
- **代码分析**：mmap 私有匿名是 **lazy 按需零页**（`user_heap_mmap.rs` 只登记 lazy VMA）；
  fork 是 **COW**（`fork_table` 叶子页只 `frame_inc_ref` + 打 COW，中间节点才分配页表帧）。
  理论上 7.71G 虚拟映射不直接吃物理页 → 崩溃更可能在：
  ① `fork_cow` 复制 `lazy_file_vmas` Vec（内核堆 256MB，已常驻 ~121MB）；
  ② fork 风暴的 ASID / task-PCB / 页表帧分配耗尽；
  ③ 某锁在低资源下**死锁**（无输出死机更像死锁而非 panic）。
- **定位受阻**：`fork_cow`/`fork_table` 的 `log::trace!("[mm-fork] ...")` 在 RISC-V
  （只开 impl-info+impl-error）**不打**，看不到崩溃前走到哪。
- **✅ 已定位（2026-08-15 晚）**：用 `--forkheavy 2 --pipeherd 2 --context 4` 抓到完整 panic：
  `[heap] high water: used=241MB/256MB` → `[heap] OOM: layout_size=1048576(1MB) used=248MB`
  → `PANIC runtime-heap-allocator/src/lib.rs:65 Heap allocation error`。
  **根因 = 内核堆 `KERNEL_HEAP_SIZE`(256MB, base-config/src/mm.rs) 耗尽 → panic → QEMU 退出 247**。
- **修复权衡**：`qemu_run.py` 里 LA pre 仅 1G RAM（堆 256MB 已占 25%），**盲目增大堆会挤占 LA 用户内存**；
  且 `forkheavy`（反复 fork+reap）堆涨到 248MB 疑有 **fork/reap 后内核结构未回收（泄漏）**，直接加大堆会掩盖问题。
- **修复（已改，待验证）**：`base-config/src/mm.rs` `KERNEL_HEAP_SIZE_BIT_WIDTH` 28→29，
  内核堆 256MB→512MB（final=8G 场景，用户确认"final 能跑就行"；LA pre 1G 会挤占）。
  待 guest 复现 `--forkheavy`/`--mmapfork` 验证不再 OOM/panic。
- **若 512MB 仍崩** → 疑 fork/reap 泄漏（`forkheavy` 反复 fork+reap 堆不回收），
  下一步查 `reap_exited_process`/`drop_user_aspace` 的内核堆释放路径。

### 🔴 P0-FS — ext4 空路径 panic + renameat2 flags（stress-ng `--class filesystem` 触发）

- **现象 1（整机崩）**：`[PANIC] another_ext4/src/ext4/high_level.rs:114`
  `at split index (is 18446744073709551615) should be <= len (is 0)`。
  `generic_remove` 中 `search_path.split_off(search_path.len() - 1)` 在
  `split_path(path)` 返回空 Vec 时 `len()-1` 下溢成 `usize::MAX` → 越界 panic。
  stress-ng `--dir`/`--rename` 传空路径触发。Linux `unlink("")` 应 ENOENT 不 panic。
  **✅ 已修（2026-08-15）**：`generic_remove`/`generic_rename` 在 `split_off(len-1)` 前判空返回 ENOENT（vendor/another_ext4/src/ext4/high_level.rs）。rv_check 通过。
- **现象 2（语义错）**：`[syscall] renameat2(nr=276) unsupported flags=0xffffffff` +
  `rename: renameat2 unexpectedly succeeded on existent dir/file with RENAME_NOREPLACE`。
  **已定位+已修+已验证（2026-08-15）**：
  - `unsupported flags=0xffffffff` warn = **正常路径**：stress-ng `exercise_renameat2()` 第一步故意传
    `(unsigned)~0` 探测非法 flags，内核正确返回 EINVAL（Linux 同样 EINVAL），非 bug，仅 warn 刷屏。
    **已删该 warn**（保留 EINVAL 语义），避免 stress-ng 高频刷屏。
  - **真正 fail**：`renameat2(X, X, RENAME_NOREPLACE)`（同一文件 rename 到自己）时，`renameat2.rs`
    `if old_resolved == new_resolved { return 0 }` 无条件成功，跳过了 NOREPLACE 检查。Linux
    `do_renameat2` 对 old==new + NOREPLACE 返回 **EEXIST**（newpath 已存在）。**已修**：该分支
    加 `flags & RENAME_NOREPLACE != 0 → EEXIST`，普通 rename(X,X) 仍返回 0。
  - **guest 验证**：`stress-ng --rename 2 --timeout=10` → `passed: 2, failed: 0` ✅
- 触发命令：`stress-ng --open 4 --fstat 4 --dir 2 --seek 2 --chmod 2 --chown 2
  --rename 2 --symlink 2 --getdent 2 --timeout=20`（open/fstat/dir 等 passed，rename 失败，ext4 panic）。

### 🔴 P0 — 内存 / procfs 核心

| # | 测试 | 现象 | 疑似根因 / 位置 |
|---|------|------|----------------|
| 1 | `brk`（stress-ng malloc 触发） | 内核刷 `[brk] rejected requested=0x10009000 ... error=InvalidAddress`，requested **非页对齐**（%0x1000≠0）但 current/start/max 全合法 | **根因已澄清（非页对齐）**：`HeapBrk::brk` 增长时某页与 `lazy_vma`/`kernel_reserved`/stack 重叠（`range_overlaps_*`）→ InvalidAddress。是 **brk 区与 mmap 区边界协调**问题。功能上 glibc fallback mmap 不崩，仅刷屏。位置：impl-sv39/user_heap_mmap.rs `brk` + sys/mem/brk.rs |
| 2 | `getegid01` | `TBROK: Expected 1 conversions got 0 FILE '/proc/self/status'` | **✅ 已修（2026-08-15）**：render.rs format_status 跨行续行 bug（`Gid:\`+换行+`t{}` 导致 `Gid:` 后无 tab、`CapEff`/`VmPeak` 同样）→ 重写为单行正确格式。rv_check 通过，待 guest 验证 |
| 3 | `clock_gettime01` | `Test timeouted ... TBROK: Test killed (timeout?)` | setup 里 `do{ scanf /proc/self/stat 字段14(utime) } while(utime==0)` 死循环——utime 字段读取/格式问题。位置：procfs format_stat 字段 14 |

### 🔴 P0 — SysV IPC 命令集

| # | 测试 | 现象 | 疑似根因 |
|---|------|------|----------|
| 4 | stress-ng `msg`/`sem-sysv` | `msgctl IPC_INFO failed EINVAL`、`semctl IPC_INFO/SEM_INFO failed EINVAL`（stressor 核心仍 passed） | **✅ 已修+已验证（2026-08-15）**：sysv_msg.rs/sysv_sem.rs 补 `IPC_INFO`(3)/`MSG_INFO`(4)/`SEM_INFO`(4) 分支，返回 msginfo/seminfo 结构。**guest 实测**：msgctl IPC_INFO/MSG_INFO ret=0 ✅；semctl IPC_INFO ret=0 ✅；raw syscall(191, cmd=4/0x104) 均正确返回 Seminfo ✅。**SEM_INFO 经 glibc `semctl()` 仍 EINVAL 系 glibc 2.41（Debian 2.41-12+deb13u3）用户态拦截**（`__semctl64` switch 未含 SEM_INFO→default EINVAL，根本不发 syscall），非内核问题，已确认后放弃处理。调试日志已清理 |
| 5 | stress-ng `shm-sysv` | bogo ops = 0（passed 但没真正跑），shmget 7.8ms/次 | 疑似同 4（IPC_INFO 失败影响 shm 探测），需单独确认 `shmget/shmctl IPC_INFO` |

### 🟠 P1 — cred/cap / 信号

| # | 测试 | 现象 | 疑似根因 |
|---|------|------|----------|
| 6 | `settimeofday02` 场景3 | LTP `Dropping CAP_SYS_TIME(25)` 后 settimeofday **仍成功**（期望 EPERM） | 上一轮加的 CAP_SYS_TIME 检查在源码里，但 capset drop 后 effective 仍判定有权——疑 LTP 用 `gettid` 作 capset pid（`sys_gettid` 返回线程 tid）与进程 pid 的映射/写入路径问题。**需 guest 实测 capget→capset drop→settimeofday 定位** |
| 7 | `sigwait` | `TBROK: kill(pid, SIGTERM) failed: EINVAL` | `sys_kill` 对合法信号/目标返回 EINVAL——查信号号或目标 pid 校验 |
| 8 | `futex_cmp_requeue01` | `TBROK: waitpid(...) EINTR` | 父进程 waitpid 被 SIGCHLD（默认忽略）打断——`wait_current_while` 信号打断判定过宽（scheduler） |

### 🟡 P2 — 功能缺口 / 精度

| # | 测试 | 现象 | 疑似根因 |
|---|------|------|----------|
| 9 | `tst_timer_test`（clock_nanosleep/futex_wait） | `slept for too long` | 10ms tick（`SCHED_TIMER_PERIOD_MS`）粒度，`sleep_current_for_ticks` 按 tick 向上取整（impl-multi-class/src/lib.rs:617）。改精度风险大，仅 2 用例 |
| 10 | `mmap18` | child `killed by SIGSEGV`（grow_stack_success 场景） | **`MAP_GROWSDOWN` 未实现**（competition-syscall-division.md 已标 P0 缺口）：栈向下增长自动扩展映射。涉及 mm vma+fault，高风险 |
| 11 | `acct02` | `acct file is empty` | 进程退出不写 acct 记录（未实现） |

### 🟡 环境 / 非内核

| # | 项 | 说明 |
|---|----|------|
| E1 | `execlp01/execvp01/setpgid03` `*_child: ENOENT` | LTP helper 二进制不在 guest PATH（`PATH` 缺 ltp/testcases/bin 之外的 helper 目录）。非内核 bug |
| E2 | `/sys`（sysfs）为空 | `ls /sys` 空，缺 `/sys/devices/system/cpu`（stress-ng `no CPUs found`、lscpu/nproc 依赖）。内核 sysfs 未实现 |
| E3 | 新增 320 个测试未进 guest | 测试集来自 `os/sdcard-la.img`（7/21 静态，testcases/bin 含 477），排除表改动不生效。需宿主构建 LTP 全量后 debugfs 注入 /glibc/ltp/testcases/bin，或重建镜像 |
| E4 | SNAPSHOT=1 | guest apt 装的包（stress-ng 等）重启即失；需 WRITE_DISK=1 持久化 |

---

## 二、stress-ng 实测小结（RISC-V guest, 0.19.02）

- `--cpu 2`：passed 2, failed 0（CPU/计算稳定）
- `--fork 4 --vm 2 --malloc 2 --pipe 2 --signal 2 --futex 2 --msg 2 --sem-sysv 2 --shm-sysv 2`：**passed 20, failed 0**（fork 1011 / vm 3072 / malloc 257484 / pipe 70640 / signal 37790 / futex 31406 / msg 514 / sem-sysv 4000）
- 暴露问题：见上表 #1（brk）、#4（IPC_INFO）、#5（shm-sysv 0 ops）
- 注意：stress-ng 0.19 参数——SysV 消息队列是 `--msg`（非 `--msgq`）、SysV 信号量是 `--sem-sysv`、SysV 共享内存是 `--shm-sysv`；带参选项建议用 `=` 形式（`--timeout=30`）

---

## 三、建议修复顺序

1. **P0-2 `getegid01`（Gid 行 bug）**：小而明确（format 串续行），改完 getegid01 + getsockname 等 /proc 读取受益
2. **P0-3 `clock_gettime01`（utime 字段）**：procfs stat 字段，配合 1 一起看
3. **P0-1 `brk` 对齐**：刷屏最严重，影响面广
4. **P0-4/5 SysV IPC `IPC_INFO` 命令**：补齐 `*ctl` 查询命令，同时服务 LTP `*ctl*` 用例
5. **P1-6 `settimeofday02` CAP_SYS_TIME**：需 guest 实测定位（capset/tid 路径）
6. **P1-7 `sigwait` kill EINVAL**：小而明确
7. **P1-8 futex waitpid EINTR**：调度器信号打断，较大
8. **P2**：MAP_GROWSDOWN、时钟精度、acct02（功能缺口，按需）

---

## 四、已修复（对照基线，勿重复修）

| 修复 | 验证状态 |
|------|----------|
| set\*id -1 哨兵（32 位 uid/gid） | ✅ guest 验证 |
| getpgid01（procfs pgrp/session） | ✅ 新 run.log TFAIL=0 |
| capset03（effective CAP_SETPCAP） | ✅ TFAIL=0 |
| setgroups03（NGROUPS_MAX+EFAULT 探测） | ✅ TFAIL=0 |
| settimeofday02（EINVAL 场景）+ SETTIMEOFDAY syscall | ⚠️ 仅 EINVAL 场景过；场景3 EPERM 未解决（见 P1-6） |
| llseek01（RLIMIT_FSIZE→EFBIG） | ✅ TFAIL=0 |
| getpeername01（addrlen 负值→EINVAL） | ✅ TFAIL=0 |
| bind04（AF_UNIX SOCK_SEQPACKET） | ✅ TFAIL=0 |
| clone SIGHAND 兼容掩码 | ✅ 新 run.log clone02 TFAIL 消失 |
| personality UNAME26 | ✅ guest 验证（uname04 不再 TBROK） |
| ProcessCaps::ROOT + CAP_SYS_TIME | ⚠️ root settimeofday 正常；LTP drop 场景未生效（P1-6） |
