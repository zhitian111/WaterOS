# LTP fast exit 跳过名单提取任务

## 任务目标

从 LTP 运行日志中，找出 **`ltp/testcases/bin/*` 顶层用例里「至少 RUN 过一次，且在所有指定日志里从未出现 `TPASS:`」** 的 basename，整理成名单文档；用户确认后再写入 `ltp_cgroup_helper.rs` 的 `LTP_SUBMIT_SKIP_BASENAMES`。

**提取方式：Coordinator 派 Subagent 用 Read 读日志，禁止写脚本/regex 批量扫 log 生成名单。**

---

## 判读规则

每个用例是一个 RUN 块：

```text
RUN LTP CASE <basename>
...
<file>:NN: TPASS: ...
Summary:
passed   N
FAIL LTP CASE <basename> : <code>
```

- 块内出现 **`TPASS:`** → 该 basename **不得**入 skip
- 块起点：`RUN LTP CASE <basename>`；终点：下一条 `RUN LTP CASE` 或 EOF
- 同一 basename 在任一日志、任一次 RUN 块里有 TPASS → 全局不得 skip
- 从未出现 `RUN LTP CASE` 的 basename → 不入 skip
- 只统计 `ltp/testcases/bin/` 顶层文件 basename

---

## 输入日志

用户指定；常见例如：

- `os/rv_local_run_all.log`
- `os/bringup_full_run.log`
- `os/ltp_log/rv_local_ltp_*.log`
- 用户附加的 `os/rv_local_run_all_*.log`

**Coordinator 必须读用户点名的每一份 log，不得自行省略。**

---

## 输出

| 文件 | 内容 |
|------|------|
| `os/ltp_log/fast_exit/no_tpass_skip_manifest.md` | 统计 + 表格 + 输入 log 列表 |
| `os/ltp_log/fast_exit/no_tpass_skip_basenames.txt` | 一行一个 basename，**严格字典序** |
| `os/ltp_log/fast_exit/chunks/<log>/chunk_<NNN>.md` | 各 Subagent 分片判读表（中间产物） |

写入 `ltp_cgroup_helper.rs` 须与用户确认；数组与 `.txt` **同序同集、字典序**。

---

## 并行：Subagent 读日志

### Coordinator

1. 对用户指定的每条 log，按 **4000 行** 分片（1-based 行号）
2. 每片派一个 **readonly Subagent**，附上下面模板
3. 收集各片 Markdown 表，**合并** `has_tpass`（任一片 true → 排除）与 `run_count`
4. 生成 skip：`run_count>0` 且从未 `has_tpass` 且存在于 `test_case/mnt/glibc/ltp/testcases/bin`
5. `sorted(set(skip))` 后写入 manifest 与 `.txt`
6. manifest 写明：每份 log 的 RUN 数、末条 `RUN LTP CASE`、skip 条数

### 分片边界

- 片头落在 RUN 块中间 → 从本片第一条 `RUN LTP CASE` 开始
- 片尾 → 越过 `end_line` 读到当前块结束

---

## Subagent 提示词模板

```markdown
你是 LTP 日志分片分析 subagent。只读；不要改仓库；不要写 Python/Shell 解析整份 log。

## 输入
- 日志：{LOG_PATH}
- 行范围（1-based）：{LINE_START}–{LINE_END}
- 分片：{CHUNK_ID}

## 任务
1. 用 Read 读取上述行（块未读完则继续往后读至下一条 RUN LTP CASE 或 EOF）。
2. 按 `docs/prompts/tasks/ltp_fast_exit_skip_list.md` 判读每个 RUN 块。
3. 输出 Markdown 表（禁止只给 JSON/脚本输出）。

## 输出格式

### {CHUNK_ID} — {LOG_PATH} L{LINE_START}-L{LINE_END}

| basename | run_count | has_tpass | 备注 |
|----------|-----------|-----------|------|

### 本分片 has_tpass=true（不得 skip）
- ...

### 本分片候选 skip（run 过且无 TPASS）
- ...
```

---

## Coordinator 合并

- 跨分片、跨 log：`has_tpass` 做 OR，`run_count` 相加
- **有 TPASS 的 basename 一律不进最终 skip**
- 最终名单 **字典序** 写入 `.txt` 与 manifest
- 用户确认后再改 `ltp_cgroup_helper.rs`

---

## 验收（Coordinator 自行完成，不用脚本）

- [ ] 每份输入 log 都派过 Subagent 分片读过
- [ ] 抽样若干 skip 条目：Read 打开源 log 对应 RUN 块，确认无 `TPASS:`
- [ ] 抽样若干有 TPASS 的 basename：确认不在 skip 中
- [ ] `.txt` 字典序、与 manifest 一致
