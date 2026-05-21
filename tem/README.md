# GitLab 提交重整（worktree）

所有重整操作在独立目录进行，**主仓库 `WaterOS_refactor/` 保持 `main` 分支日常开发**。

| 路径 | 用途 |
|------|------|
| `WaterOS_refactor/` | 主仓库，`main`，完整代码 |
| `WaterOS_refactor-reorg/` | worktree，仅 `reorg/os-weekly` 与 `os/` 提交历史 |
| `WaterOS_refactor/tem/` | 脚本（始终在主仓库执行） |

## 快速开始

```bash
cd /path/to/WaterOS_refactor/tem
chmod +x *.sh _create_reorg_branch.sh

# 1. 创建 worktree + orphan 分支
./init.sh -y

# 2. 改系统时间并提交 7 周（仅 add os/）
sudo ./run_weekly_commits.sh

# 3. 推送 GitLab
./push_to_gitlab.sh
```

## 脚本说明

| 脚本 | 作用 |
|------|------|
| `init.sh` | 初始化 worktree（推荐唯一入口） |
| `run_weekly_commits.sh` | 一键周提交 |
| `commit_week.sh` | 单周提交 |
| `push_to_gitlab.sh` | 推送 |
| `set_system_time.sh` / `restore_ntp.sh` | 系统时间 |

详细合并计划见 `commit_reorganize_plan.md`。
