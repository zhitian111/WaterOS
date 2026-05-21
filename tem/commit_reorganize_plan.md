# WaterOS Git 提交记录重整计划

> 生成日期: 2026-05-21  
> 目标远程: [https://gitlab.eduxiji.net/T202610422999926/wateros.git](https://gitlab.eduxiji.net/T202610422999926/wateros.git)  
> 作者: **OuterSystems** \<t202610422999926@eduxiji.net\>  
> 提交范围: **仅 `os/` 目录**

---

## 1. 当前仓库状态

| 项目 | 值 |
|------|-----|
| 本地分支 | `main` @ `e86d639` |
| 现有 remote | `github` → `git@github.com:zhitian111/WaterOS.git` |
| 4月1日起原始提交数 | 83（含 merge/docs/非 os） |
| 4月1日起含 `os/` 的提交 | 约 72 |
| 重整后目标提交数 | **7**（每周 1 个，跳过无提交周） |
| 重整分支名 | `reorg/os-weekly` |
| worktree 目录 | `../WaterOS_refactor-reorg`（与主仓库同级） |
| 脚本执行位置 | 始终在 `WaterOS_refactor/tem/`（主仓库） |

### 周次划分规则

以 **2026-04-01** 为第 1 周起点，每 7 天为一周（周一至周日按自然日计算）。

| 周次 | 日期范围 | 原始提交数 | 含 os/ 提交数 | 重整后 |
|------|----------|------------|---------------|--------|
| Week 1 | 04-01 ~ 04-07 | 1 | 1 | **1 个 squash** |
| Week 2 | 04-08 ~ 04-14 | 0 | 0 | **跳过** |
| Week 3 | 04-15 ~ 04-21 | 3 | 1 | **1 个 squash** |
| Week 4 | 04-22 ~ 04-28 | 16 | 16 | **1 个 squash** |
| Week 5 | 04-29 ~ 05-05 | 4 | 3 | **1 个 squash** |
| Week 6 | 05-06 ~ 05-12 | 19 | 17 | **1 个 squash** |
| Week 7 | 05-13 ~ 05-19 | 29 | 25 | **1 个 squash** |
| Week 8 | 05-20 ~ 05-26 | 11 | 11 | **1 个 squash** |

---

## 2. 每周合并方案总览

重整策略：在 `reorg/os-weekly` 分支上，**按周将 `os/` 目录检出到该周结束时的树快照**，做一次提交。  
提交信息见 `tem/commit_messages/weekNN.txt`。

| 重整序号 | 周次 | 建议提交时间 | 树快照 (tip) | 相对上周 diff 规模 |
|----------|------|--------------|--------------|-------------------|
| 1 | Week 1 | 2026-04-05 15:00 +08 | `4db07b6` | 108 files, +2916/-191 |
| 2 | Week 3 | 2026-04-18 15:00 +08 | `2dad975` | 29 files, +1080/-134 |
| 3 | Week 4 | 2026-04-25 15:00 +08 | `60a74d3` | 79 files, +3825/-594 |
| 4 | Week 5 | 2026-05-02 15:00 +08 | `195d24b` | 24 files, +1150/-699 |
| 5 | Week 6 | 2026-05-09 15:00 +08 | `4d6185b` | 234 files, +7732/-1314 |
| 6 | Week 7 | 2026-05-16 15:00 +08 | `3697c83` | 124 files, +5506/-1672 |
| 7 | Week 8 | 2026-05-23 15:00 +08 | `e86d639` | 84 files, +2689/-624 |

---

## 3. 各周需合并的原始提交清单

### Week 1（2026-04-01 ~ 04-07）→ 1 个提交

| 哈希 | 日期 | 作者 | 说明 | 纳入 os |
|------|------|------|------|---------|
| `4db07b6` | 04-02 | zhitian111 | [ref] 项目部分重构后 api 和 impl 同步 | ✅ |

---

### Week 2（2026-04-08 ~ 04-14）

**无提交，跳过。**

---

### Week 3（2026-04-15 ~ 04-21）→ 1 个提交

| 哈希 | 日期 | 说明 | 纳入 os |
|------|------|------|---------|
| `beee6bb` | 04-16 | [docs] 添加大量文档 | ❌ 仅 docs |
| `2dad975` | 04-21 | [feat] 任务和任务调度基础实现 | ✅ |
| `c35ea6c` | 04-21 | [add] gitignore | ❌ 仅根目录 |

**合并目标快照:** `2dad975`

---

### Week 4（2026-04-22 ~ 04-28）→ 1 个提交

合并以下 **16** 个提交（均含 `os/` 变更）：

`792ace6`, `4707c5f`, `8c7bcbd`, `0bd14f9`, `55bf490`, `795af53`, `37be4ec`, `a78d623`, `dad8b4e`, `61c9f9e`, `c86f5cc`, `e3e6ff9`, `d634060`, `9a49279`, `544978f`, `60a74d3`

**合并目标快照:** `60a74d3`

---

### Week 5（2026-04-29 ~ 05-05）→ 1 个提交

| 哈希 | 日期 | 说明 | 纳入 os |
|------|------|------|---------|
| `8ee3a1f` | 04-29 | [feat] task 架构优化 | ✅ |
| `0b5e84e` | 04-29 | [feat] gitignore | ❌ |
| `20a3146` | 04-30 | [feat] 部分代码重构 | ✅ |
| `195d24b` | 05-01 | [feat] 删除冗余链路 | ✅ |

**合并目标快照:** `195d24b`

---

### Week 6（2026-05-06 ~ 05-12）→ 1 个提交

合并以下 **17** 个含 `os/` 的提交：

`d235fec`, `0ae6c9e`, `2a1198a`, `0563eda`, `6944143`, `c07b5dc`, `eae14bf`, `33fefc8`, `87c80f4`, `0b13dd7`, `637e116`, `65a22f8`, `e59a8d9`, `7c7ed10`, `114709e`, `cc5fb35`, `4d6185b`

**排除:** `b010585`（仅 docs）, `6efbe8c`（非 os）

**合并目标快照:** `4d6185b`

---

### Week 7（2026-05-13 ~ 05-19）→ 1 个提交

合并以下 **25** 个含 `os/` 的提交：

`09517cb`, `1c470d5`, `af8030d`, `f86bf93`, `1b47136`, `4df39c7`, `172c126`, `ce6f512`, `8984e4c`, `7990c82`, `8807844`, `264dfe3`, `7720a92`, `c528fa3`, `f68218b`, `1d252ec`, `b2f5862`, `e090d70`, `132bf6b`, `bdd211c`, `b335968`, `c68cd49`, `c7d58c6`, `a7b42fb`, `3697c83`

**排除:** `1cd7bef`, `ae9dd15`（仅 user/）, `16b97db`, `ce315ba`（非 os）

**合并目标快照:** `3697c83`

---

### Week 8（2026-05-20 ~ 05-26）→ 1 个提交

合并以下 **11** 个提交：

`c40bb43`, `c9c5af1`, `0a9fec4`, `689d27e`, `6bd3a1b`, `9f88d44`, `ee4edfe`, `041d1ce`, `fcc1fdc`, `ec8071f`, `e86d639`

**合并目标快照:** `e86d639`（与当前 `main` HEAD 一致）

---

## 4. 不包含在 GitLab 推送中的目录

以下目录/文件在原始仓库中有提交，但按需求 **不进入** 重整历史：

| 路径 | 说明 |
|------|------|
| `docs/` | 文档 |
| `user/` | 用户态程序 |
| `test_case/` | 测试用例 |
| `old/` | 旧版代码 |
| `.gitignore` / `.gitmodules` | 根配置 |
| `scripts/`（仓库根） | 根级脚本 |

---

## 5. 脚本使用说明

所有脚本位于 `tem/` 目录。

### 文件列表

| 脚本 | 作用 |
|------|------|
| `init.sh` | **唯一入口**：配置 remote、创建 orphan 分支、添加 worktree |
| `run_weekly_commits.sh` | init 后一键：系统时间 + 7 周提交（在 worktree 内） |
| `commit_week.sh` | 单周提交 |
| `push_to_gitlab.sh` | 从 worktree 推送到 GitLab |
| `config.sh` | 共享变量与 `reorg_git` / `main_git` 辅助函数 |
| `set_system_time.sh` / `restore_ntp.sh` | 系统时间 |
| `commit_messages/init.txt` | init 提交说明（可编辑） |
| `commit_messages/week*.txt` | 各周功能描述 commit message |
| `README.md` | 快速上手 |

已弃用（转发到 `init.sh`）：`init_reorg_branch.sh`、`init_reorg_worktree.sh`

### 推荐执行顺序

```bash
cd /home/zhitian/project/WaterOS_refactor/tem
chmod +x *.sh _create_reorg_branch.sh

# 1. 创建 worktree（主仓库保持 main，重整在 ../WaterOS_refactor-reorg）
./init.sh -y

# 2. 一键周提交（仍在 tem 目录执行，无需 cd worktree）
sudo ./run_weekly_commits.sh

# 3. 查看 worktree 历史
git -C ../WaterOS_refactor-reorg log --oneline

# 4. 推送 GitLab
./push_to_gitlab.sh
```

init 说明编辑 `commit_messages/init.txt`；各周为功能描述体，见 `commit_messages/week*.txt`。

### 单独提交某一周

```bash
./commit_week.sh 4
sudo ./commit_week.sh 4 --with-system-time
```

---

## 6. 注意事项

1. **worktree 隔离**：重整仅在 `../WaterOS_refactor-reorg` 进行；主仓库 `WaterOS_refactor/` 始终保持 `main`，避免 orphan 分支与未跟踪文件冲突。
2. **脚本位置**：`tem/` 只在主仓库，通过 `git -C worktree` 操作；不必把 `tem/` 复制到 worktree。
3. **GitLab 仓库当前为空**：首次 push 将直接建立 `main` 分支历史。
4. **重复提交**：Week 7 的重复 fix 在 squash 树快照中已消解。
5. **大文件**：`os/` 下可能有 `*.img` 等；推送前确认 `.gitignore` 或 LFS。
6. **身份验证**：push 需 GitLab 凭据。
7. **清理 worktree**：`git -C WaterOS_refactor worktree remove ../WaterOS_refactor-reorg`

---

## 7. 重整后的预期 log 示例

```
e86d639 feat(os): 第8周 — 文件 syscall、mmap、trap 页表切换...  (2026-05-23)
3697c83 feat(os): 第7周 — ELF 加载、syscall 扩展...           (2026-05-16)
4d6185b feat(os): 第6周 — LoongArch 支持、trap/VFS...          (2026-05-09)
195d24b feat(os): 第5周 — task 架构优化...                    (2026-05-02)
60a74d3 feat(os): 第4周 — 用户态链路、系统调用...              (2026-04-25)
2dad975 feat(os): 第3周 — 任务与调度基础实现                  (2026-04-18)
4db07b6 feat(os): 第1周 — API/impl 同步...                    (2026-04-05)
(empty)   chore: 初始化 os 周提交重整分支                      (2026-03-31)
```

---

## 8. 相关原始作者统计（供参考）

4 月 1 日以来主要贡献者：

- `zhitian111 <2367651943@qq.com>`
- `kasss233 <1592858973@qq.com>`

重整后统一为课程/团队账号 **OuterSystems**。
