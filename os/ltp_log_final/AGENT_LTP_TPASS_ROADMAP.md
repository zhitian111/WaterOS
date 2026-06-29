# LTP 官方计分提升路线图（Agent 分发版 v2）

> **v2 变更**：计分指标从 `grep TPASS` 改为 oscomp 官方 judge 口径 —— 每个用例 `Summary:` 块中 `passed` 数字之和（**judge raw**），再经对数映射计入总分。  
> 基于 `os/ltp_log_final/` 23 份含 LTP 的日志（合并 max-per-case），skip 表 **2496** 条。  
> 生成日期：2026-06-29

---

## 1. 必读：官方怎么算 LTP 分

来源：[oscomp/autotest-for-oskernel](https://github.com/oscomp/autotest-for-oskernel) → `kernel/judge/judge_ltp-glibc.py` + `kernel/LTP_SCORING.md`

### 1.1 单用例得分

解析串口日志，每个用例从 `RUN LTP CASE xxx` 到 `FAIL LTP CASE xxx : N`：

- **得分 = 该用例 `Summary:` 里 `passed` 的整数**
- **不是** `grep TPASS` 行数
- **不是**「跑完一个用例算 1 分」

```text
access01.c:245: TPASS: ...
Summary:
passed   199        ← 官方记 199 分
failed   0
FAIL LTP CASE access01 : 0
```

### 1.2 LTP 计入大赛总分

```text
LTP 贡献 = 500 × log10(1 + 9 × raw / 10000)    （raw 封顶 10000，LTP 贡献封顶 500）
```

| judge raw | LTP 计入总分 |
|----------:|-------------:|
| 573 | 90.3 |
| 1031 | **142.5** |
| 1430 | **185.6** |
| 2218 | **255.9** |
| 5000 | 370.2 |
| 10000 | 500.0 |

非 LTP 测试组（basic、busybox、libctest…）**不**做对数压缩，直接加原始分。

### 1.3 三种输出格式（决定能不能计分）

| 格式 | 示例 | 有 Summary? | 官方计分 |
|------|------|:-----------:|:--------:|
| **A. tst_test 新框架** | `access01.c:245: TPASS:` + `Summary: passed 199` | 是 | **计分** |
| **B. PAN 老框架** | `abs01  1  TPASS  :  Test passed` | 否 | **0** |
| **C. cgroup 专用** | `cgroup_xattr  792  TPASS` + `TINFO: summary: PASS` | 否* | **0** |

\* `cgroup_xattr` 等用 `TINFO: All test-cases have been completed, summary: PASS`，**不是** judge 识别的 `Summary:\npassed N`。

**结论**：Wave 0 unskip 批次里的用例几乎全是格式 A，TPASS 与 judge raw 一致；**不要**把 `cgroup_xattr`（792 TPASS）当高 ROI 任务。

### 1.4 本地验收命令（必须用这套）

```bash
cd os

# 单份验证日志 —— 看 judge raw，不是 TPASS
python3 ltp_log_final/.agent/analyze_logs.py ltp_log_final/verify_W0-A.log

# 或看 passed 合计
python3 scripts/ltp_sum_passed.py ltp_log_final/verify_W0-A.log

# 扫描全量 ltp_log_final
python3 ltp_log_final/.agent/analyze_logs.py
```

### 1.5 腐败 Summary 警告

部分日志（如 `rv_local_run_all_0348.log`、`os/rv_local_run_all.log` 尾部）出现：

```text
Summary:
passed   1633771882    ← 垃圾值，内核/用户态内存问题
```

分析脚本对单用例 `passed > 5000` 自动置 0。**若 whitelist 验证出现此现象，先修内核再 unskip。**

---

## 2. 数据摘要（judge raw）

| 指标 | 数值 |
|------|------|
| 合并可计分用例 | **397** |
| 各日志 per-case 取 max 后 raw 总和 | **2218** |
| 最佳单次日志 | `rv_local_run_all_08.log`（**raw=1430**，218 用例） |
| skip 表内已有 Summary 分（tier0） | **177 用例 / raw=1031** |
| tier0 全部 unskip 后 LTP 增量 | **~142.5 分**（对数后） |
| PAN 格式（TPASS>0 但 judge=0） | **169 用例 / 1828 TPASS 行白费** |

**tier0 头部（unskip 零代码回收）**：

| 用例 | judge raw | 备注 |
|------|----------:|------|
| epoll_ctl03 | 255 | 单项最高 |
| pipe11 | 70 | |
| signal03/05/04 | 30/30/28 | |
| ppoll01 | 18 | |
| setitimer01 | 18 | |

**PAN 陷阱（看起来很美，官方 0 分）**：

| 用例 | TPASS 行 | judge raw |
|------|----------|----------:|
| cgroup_xattr | 792 | **0** |
| rt_sigaction01/02/03 | 150×3 | **0** |
| waitpid01 | 126 | **0** |

---

## 3. 推荐推进顺序（按 judge raw ROI）

```
Wave 0  unskip tier0（日志已证明有 Summary passed，零代码）  → raw +1031
Wave 1  nice/pgid 验证、identity、fcntl A、process P0（需写内核）
Wave 2  vfs truncate、sched 扩展
勿做    cgroup_xattr（PAN 不计分）、fcntl14–39、ptrace、fs_bind、inotify 全量
```

### 任务优先级总表

| 优先级 | 任务 ID | 类型 | judge raw | LTP 折算增量 | 工期 |
|--------|---------|------|----------:|-------------:|------|
| **P0** | W0-A-epoll | unskip | **263** | ~46 | 0.5d |
| **P0** | W0-B-signal-pipe | unskip | **179** | ~32 | 0.5d |
| **P0** | W0-H-misc-small | unskip | **203** | ~36 | 1d（分 3 批） |
| **P1** | W0-C-io-vect | unskip | **112** | ~21 | 0.5d |
| **P1** | W0-D-id-set | unskip | **83** | ~16 | 0.5d |
| **P1** | W0-G-stat-sendfile | unskip | **68** | ~13 | 0.5d |
| **P1** | W0-F-select-poll | unskip | **61** | ~12 | 0.5d |
| **P1** | W0-E-sched | unskip | **57** | ~11 | 0.5d |
| **P2** | W1-nice-pgid | 已实现+验证 | **~4** | ~1 | 0.5d |
| **P2** | W1-fcntl-A | 代码+unskip | **~19** | ~4 | 1–2d |
| **P2** | W1-identity | 代码 | **~1** | <1 | 1d |
| **P2** | W1-process-P0 | 代码+unskip | **0→?** | 待实现 | 2–3d |
| **P3** | W1-vfs-truncate | 代码 | **~2** | <1 | 2–3d |
| **降权** | W1-cgroup-xattr | PAN 不计分 | **0** | **0** | 仅内核完善，非冲分 |
| **勿做** | fcntl14–39 / ptrace / fs_bind | 保持 skip | — | — | — |

清单文件：[`ltp_log_final/.agent/unskip_lists/`](.agent/unskip_lists/)  
元数据：[`meta.json`](.agent/unskip_lists/meta.json)（字段 `judge_raw` 已更新）

---

## 4. 机制速查

### 4.1 Skip 表

文件：[`ltp_cgroup_helper.rs`](../components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ltp_cgroup_helper.rs)

- 数组 `LTP_SUBMIT_SKIP_BASENAMES`（**严格字典序**）
- 删除 = unskip；无参 exec 时直接 `exit(0)` → 无输出、0 分

批量删除：

```bash
cd os
python3 <<'PY'
import re
path = "components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ltp_cgroup_helper.rs"
remove = set(open("ltp_log_final/.agent/unskip_lists/W0-A-epoll.txt").read().splitlines())
text = open(path).read()
pat = r"(const LTP_SUBMIT_SKIP_BASENAMES:\s*&\[)(.*?)(\n\];)"
m = re.search(pat, text, re.S)
names = [n for n in re.findall(r'"([^"]+)"', m.group(2)) if n not in remove]
assert names == sorted(names)
new_body = "".join(f'\n    "{n}",' for n in names) + "\n"
text = text[:m.start(2)] + new_body + text[m.end(2):]
open(path, "w").write(text)
print(f"removed {len(remove)}, remaining {len(names)}")
PY
```

### 4.2 debugfs 只跑指定用例

脚本：[`inject_ltp_whitelist.sh`](.agent/inject_ltp_whitelist.sh)

```bash
cd os
chmod +x ltp_log_final/.agent/inject_ltp_whitelist.sh

# 按批次
./ltp_log_final/.agent/inject_ltp_whitelist.sh --file ltp_log_final/.agent/unskip_lists/W0-A-epoll.txt

# 或指定 basename
./ltp_log_final/.agent/inject_ltp_whitelist.sh epoll_ctl02 epoll_ctl03

# musl
LIBC=musl ./ltp_log_final/.agent/inject_ltp_whitelist.sh setpriority01

# 编译 + 跑 + 验收（judge 口径）
make rv
make rv_run 2>&1 | tee ltp_log_final/verify_W0-A.log
python3 ltp_log_final/.agent/analyze_logs.py ltp_log_final/verify_W0-A.log
```

原理：debugfs 覆盖 `sdcard-rv.img` 内 `/glibc/ltp_testcode.sh`，只循环 WHITELIST；**不改** `test_case/` 源树。

### 4.3 验收标准

| 检查项 | 命令 / 期望 |
|--------|-------------|
| judge raw | `analyze_logs.py verify.log` → raw ≥ 批次 `meta.json` 的 90% |
| 无腐败 Summary | 日志中无 `passed` 九位数 |
| 无 hang | whitelist 批次 30min 内结束 |
| 编译 | `make rv_check && make la_check` |
| unskip 后 | 再跑更大集合确认不回归 |

---

## 5. Agent 任务卡片（复制即用）

通用约束：

- 工作目录：`/home/zhitian/project/WaterOS_refactor/os`
- **计分以 `Summary: passed` 为准**，`grep TPASS` 仅辅助
- **不要**改 `test_case/`；LTP 用 debugfs 改 `sdcard-rv.img`
- **不要** commit 除非用户明确要求
- skip 表保持字典序；勿动 hang 前缀（`fs_bind` `ptrace` `nptl01` `memcg_test_` `vma` `vmsplice`）
- **禁止**两 Agent 同时改 `ltp_cgroup_helper.rs`

---

### 任务 W0-A-epoll（P0，unskip，judge raw **263**）

**目标**：从 skip 表删除 `epoll_ctl02`、`epoll_ctl03`（`epoll_ctl01` 日志无 Summary 分，**不删**）。

**清单**：`ltp_log_final/.agent/unskip_lists/W0-A-epoll.txt`

**完整提示词**：

```text
你是 WaterOS LTP 工程师。任务 W0-A-epoll（P0，仅 unskip）。

背景：oscomp 官方 LTP 分数 = 各用例 Summary: passed 之和（judge raw），不是 TPASS 行数。
epoll_ctl03 单项 judge raw=255，是最高 ROI unskip。

步骤：
1. cd os
2. ./ltp_log_final/.agent/inject_ltp_whitelist.sh --file ltp_log_final/.agent/unskip_lists/W0-A-epoll.txt
3. make rv && make rv_run 2>&1 | tee ltp_log_final/verify_W0-A-epoll.log
4. python3 ltp_log_final/.agent/analyze_logs.py ltp_log_final/verify_W0-A-epoll.log
   期望：judge raw ≥ 250；无 passed 九位数腐败值
5. 用 ltp_cgroup_helper.rs 批量删除清单中 basename（保持字典序）
6. make rv_check && make la_check
7. 汇报：删了哪些、verify 的 judge raw、是否 hang

不要：实现新 epoll；不要 unskip epoll_ctl01（无分）；不要一次删整个 epoll_* 前缀。
```

---

### 任务 W0-B-signal-pipe（P0，unskip，judge raw **179**）

**清单**：`ltp_log_final/.agent/unskip_lists/W0-B-signal-pipe.txt`（15 项，含 pipe11=70）

**完整提示词**：

```text
你是 WaterOS LTP 工程师。任务 W0-B-signal-pipe（P0，仅 unskip，judge raw 预期 ~179）。

步骤同 W0-A：inject_ltp_whitelist.sh --file W0-B-signal-pipe.txt → rv_run → analyze_logs.py 验收 judge raw → 删 skip 表 → rv_check/la_check。

注意：pipe11 占 70 分，优先确认；若 hang 则只 unskip 已验证子集。
不要改内核。
```

---

### 任务 W0-H-misc-small（P0，unskip，judge raw **203**）

**清单**：`ltp_log_final/.agent/unskip_lists/W0-H-misc-small.txt`（70 项）

**建议分 3 子批 whitelist**（每批 ~25 basename，字母序）：

- H1：`mmap05` … 前 25 个
- H2：中间 25 个
- H3：剩余

每子批验证 judge raw 累加后再 unskip。

---

### 任务 W0-C-io-vect（P1，unskip，judge raw **112**）

**清单**：`ltp_log_final/.agent/unskip_lists/W0-C-io-vect.txt`（29 项）

流程同 W0-A。与 identity 修复并行不冲突。

---

### 任务 W0-D-id-set（P1，unskip，judge raw **83**）

**清单**：`ltp_log_final/.agent/unskip_lists/W0-D-id-set.txt`（14 项）

---

### 任务 W0-E-sched（P1，unskip，judge raw **57**）

**清单**：`ltp_log_final/.agent/unskip_lists/W0-E-sched.txt`（16 项）

---

### 任务 W0-F-select-poll（P1，unskip，judge raw **61**）

**清单**：`ltp_log_final/.agent/unskip_lists/W0-F-select-poll.txt`（10 项）

---

### 任务 W0-G-stat-sendfile（P1，unskip，judge raw **68**）

**清单**：`ltp_log_final/.agent/unskip_lists/W0-G-stat-sendfile.txt`（19 项）

---

### 任务 W1-nice-pgid（P2，验证，judge raw **~4**）

**目标**：验证 setpriority/getpriority/getpgid/setpgid 已合入。

**测试**：`setpriority01` `setpriority02` `setpgrp02` `getpriority01` `getpriority02` `getpgid01`

**完整提示词**：

```text
你是 WaterOS 内核工程师。任务 W1-nice-pgid（验证，非高 ROI）。

官方计分看 Summary: passed，不是 TPASS。
1. inject_ltp_whitelist.sh setpriority01 setpriority02 setpgrp02 getpriority01 getpriority02 getpgid01
2. make rv && make rv_run 2>&1 | tee ltp_log_final/verify_W1-nice-pgid.log
3. analyze_logs.py 验收：期望 setpriority02 passed≥7，setpgrp02 passed≥2
4. 若在 skip 表则删除对应 basename
5. make rv_check && make la_check

代码位置：sys/priority.rs, sys/task.rs, process.rs
```

---

### 任务 W1-identity（P2，代码，judge raw **~1** 当前）

**目标**：`getresuid(148)` / `getresgid(150)`。

**清单**：`ltp_log_final/.agent/unskip_lists/W1-identity.txt`

---

### 任务 W1-fcntl-A（P2，代码，judge raw **~19**）

**目标**：fcntl01–13；**禁止** unskip fcntl14+。

**清单**：`ltp_log_final/.agent/unskip_lists/W1-fcntl-A.txt`

---

### 任务 W1-process-P0（P2，代码，当前 raw **0**）

**目标**：`waitid(247)` + prctl 扩展。

**清单**：`ltp_log_final/.agent/unskip_lists/W1-process-P0.txt`

注意：`waitpid01` 有 126 TPASS 但是 PAN 格式，**不计分**；不要与 waitid 任务混淆。

---

### 任务 W1-vfs-truncate（P3，代码，judge raw **~2**）

**清单**：`ltp_log_final/.agent/unskip_lists/W1-vfs-truncate.txt`

---

### 任务 W1-cgroup-xattr（降权，官方 **0 分**）

**现状**：792 TPASS 行，但 PAN/cgroup 输出格式，**judge raw=0**。

仅当为了内核 cgroup/xattr 能力完善时做；**不要**作为冲榜 LTP 分的首选。修复 exit code 4 也不会增加官方分数，除非输出格式变为 `Summary: passed N`。

---

## 6. 必须保持 skip

| 类别 | 前缀/示例 | 原因 |
|------|-----------|------|
| mount 传播 | `fs_bind*` | 数周级 |
| 调试 | `ptrace*` | 数周级；PAN 格式 TPASS 也不计分 |
| POSIX 锁 | `fcntl14`–`fcntl39` | 战略 skip |
| hang | `nptl01` `memcg_test_*` `vma*` `vmsplice*` | 无参 sync hang |
| fanotify/inotify | 全量 | 未实现 |
| nice syscall | `nice01`–`nice05` | 缺 nice(2) |

---

## 7. 并行调度

| Agent | 任务 | 改内核 | 改 skip |
|-------|------|--------|---------|
| A | W0-A-epoll | 否 | 2 条 |
| B | W0-B-signal-pipe | 否 | 15 条 |
| C | W0-H-misc H1 | 否 | ~25 条 |
| D | W1-nice-pgid 验证 | 否 | 0–6 条 |

**串行**：所有 skip 表修改（避免 merge 冲突）。

---

## 8. 预期 judge raw 阶梯

| 阶段完成 | 累计 judge raw | LTP 折算分（约） |
|----------|---------------:|-----------------:|
| 当前最佳单次（08.log） | 1430 | 186 |
| + tier0 全 unskip | +1031 → 2461* | 256* |
| + W1 代码任务 | +30~80 | +5~15 |

\* 理论值；实际全跑受 hang/超时影响，需维持 hang skip。

Wave 0 全部完成（7 批 unskip）：**judge raw +1031，LTP 总分 +~142**（对数后）。

---

## 9. 数据来源与再分析

```bash
cd os
python3 ltp_log_final/.agent/analyze_logs.py              # 扫描 ltp_log_final/*.log
python3 ltp_log_final/.agent/analyze_logs.py --tier0      # 打印 177 项 tier0
python3 scripts/ltp_sum_passed.py ltp_log_final/*.log     # passed 合计交叉验证
```

| 文件 | 说明 |
|------|------|
| `rv_local_run_all_08.log` | 最佳单次（raw=1430） |
| `.agent_tier0_unskip.txt` | 177 个 skip 内有 judge 分的 basename |
| `.agent/analysis_judge.json` | 机器可读分析结果 |
| 腐败日志 | `0348` `0403` 等 — 分析时 passed 已置 0 |

---

## 10. FAQ

**Q：为什么 TPASS 很多但分数不涨？**  
A：可能是 PAN 格式（无 `Summary:`）或 skip 表直接 exit(0)。用 `analyze_logs.py` 区分。

**Q：v1 路线图里 cgroup_xattr 144 分？**  
A：v1 误用 TPASS 行数；官方 judge raw=**0**，已降权。

**Q：whitelist 与 unskip？**  
A：whitelist 只改镜像脚本做快速验证；unskip 改内核 skip 表让全跑计分。

**Q：musl？**  
A：`LIBC=musl ./ltp_log_final/.agent/inject_ltp_whitelist.sh ...`；glibc 优先。

---

*v2：每轮全跑后执行 `analyze_logs.py` 更新 tier0 与 `meta.json`。*
