
抱歉——附件被摘要器截断了，我只看到开头的构建日志，没看到后面一万多行的 LTP 输出。现在直接检索 `run.log` 里的失败点：

Searched for regex `TFAIL|TBROK|BROK` (`**/os/run.log`), 499 results

No matches found. Your search pattern might be excluded completely by either the search.exclude settings or .*ignore files.
If you believe that it should have results, you can check into the .*ignore files and the exclude setting (here are some excluded patterns for reference:[**/node_modules,**/bower_components,**/*.code-search]).
Then if you want to include those files you can call the tool again by setting "includeIgnoredFiles" to true.

499 处失败标记，先分拣出真实的 `TFAIL`（断言失败）和 `TBROK`（环境性失败）：

Searched for regex `TFAIL:` (`**/os/run.log`), 81 results

Searched for regex `TBROK:` (`**/os/run.log`), 9 results

这次是真读到了 LTP 输出。**81 处 `TFAIL` + 9 处 `TBROK`**（ARCH=la PROFILE=pre 跑的 LTP），按子系统归组如下：

## 失败清单（按子系统分组）

### 🔴 A. setuid/gid 系列（最大组，~60 处，全是 `EPERM` / UID 混乱）


| 测试                     | 错误                                                                                               |
| -------------------------- | ---------------------------------------------------------------------------------------------------- |
| `setreuid01/02/03/05/07` | `SETREUID(-1, euid) failed: EPERM`、`setreuid(nobody,-1) failed`、`Unexpected process UID`         |
| `setresuid01/02/04/05`   | `setresuid(-1,-1,main) failed: EPERM`、`SETRESUID(-1,0,-1) failed: EPERM`、`open(TEMPFILE) EACCES` |
| `setregid03/04`          | `setregid(-1,1) failed: EPERM`、`real gid=1; effective gid=2`                                      |
| `setresgid02`            | `Unexpected process GID after setresgid(-1,-1,-1)`                                                 |

**疑似根因**：非 root 子进程调用 `setreuid/setresuid` 到**自身当前 uid/saved uid** 应成功，但被判 EPERM——很可能与刚做的 caps 感知特权判定/真 KEEPCAPS 冲突（子进程 `setuid(nobody)` 降权后 caps 被清，`uid_privileged` 变 false，而 `plan_set_re_id` 对"改到当前值"的放行逻辑可能没走到）。**这是最值得修的**（直接涉及我们改的 cred）。

### 🟠 B. exec PATH 查找（3 处）

`execlp01`/`execvp01`/`setpgid03` → `Failed to execute ..._child: ENOENT`
**疑似**：`execvp` 的 `PATH` 搜索未实现或子程序找不到。

### 🟠 C. 定时器/时钟精度（2 处）

`tst_timer_test` → `clock_nanosleep() slept for too long`、`futex_wait() slept for too long`
**疑似**：定时器 tick 精度/时钟源不准。

### 🟡 D. capability 语义（2 处）

- `capset03` → `capset() succeeded`（应失败——某个非法 capset 没被拦，可能是我们新增的 word1/bounding 之外的参数校验缺）
- `setgroups03` → `setgroups(65536,...) 期望 EFAULT 得 EINVAL`（参数校验差异）

### 🟡 E. 网络/杂项 socket（4 处）

- `bind04` → `socket(1,5,0) failed: EINVAL`（AF_UNIX 创建失败）
- `getpeername01` → `getpeername() succeeded`（应失败）
- `uname04` → `persona(131072) failed: EINVAL`
- `futex_cmp_requeue01` → `waitpid() EINTR`

### 🟡 F. 其它单点


| 测试             | 错误                        | 疑似                  |
| ------------------ | ----------------------------- | ----------------------- |
| `acct02`         | acct file is empty          | `acct()` 未实现       |
| `getpgid01`      | `getpgid(0)` 返回 1（应 0） | 语义                  |
| `llseek01`       | write 超文件大小限制成功    | `RLIMIT_FSIZE` 未生效 |
| `mmap18`         | child SIGSEGV               | mmap 边界             |
| `settimeofday02` | settimeofday 应失败却成功   | 权限校验缺            |

## 建议处理顺序

1. **A 组（setuid/gid，~60 个失败）**——量最大、且和我们刚做的 cred/caps 直接相关，优先深入（先看 `plan_set_re_id/res` 的非特权放行 + 子进程降权后的 caps 状态）。
2. **B 组（execvp PATH）**——真实软件也依赖，其次。
3. **C 组（时钟精度）**——LTP 定时类多，值得看。
4. D/E/F 按需。

要我先深入 **A 组**吗？我可以读 `setreuid`/`setresuid` 实现 + 一个具体 LTP 用例（如 `setreuid01`）定位为什么"改到当前 uid"会被 EPERM。
