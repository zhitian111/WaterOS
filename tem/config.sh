#!/usr/bin/env bash
# 提交重整共享配置 —— source 本文件后使用其中的变量

# Git 作者信息（提交到 GitLab 时使用）
export GIT_AUTHOR_NAME="OuterSystems"
export GIT_AUTHOR_EMAIL="t202610422999926@eduxiji.net"
export GIT_COMMITTER_NAME="${GIT_AUTHOR_NAME}"
export GIT_COMMITTER_EMAIL="${GIT_AUTHOR_EMAIL}"

# 远程仓库（配置在主仓库，推送在 worktree 执行）
export GITLAB_REMOTE_NAME="gitlab"
export GITLAB_REMOTE_URL="https://gitlab.eduxiji.net/T202610422999926/wateros.git"
export GITLAB_BRANCH="main"

# 仅提交此目录
export COMMIT_PATH="os"

# 重整分支名
export REORG_BRANCH="reorg/os-weekly"

# 主仓库（含 main 分支日常开发，脚本所在 WaterOS_refactor/）
export MAIN_REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# 独立 worktree 目录（与主仓库同级，所有重整提交在此进行）
export WORKTREE_DIR="${WORKTREE_DIR:-$(dirname "$MAIN_REPO_ROOT")/WaterOS_refactor-reorg}"

# 主仓库日常开发分支（init 结束后应保持在此分支）
export MAIN_BRANCH="main"

# 4 月 1 日之前的 os 快照（Week 1 diff 基准）
export BASE_REF="0bbbf38"
export ORIGINAL_HEAD="e86d639"

export TZ="Asia/Shanghai"

export WEEKLY_PLAN=(
  "1|2026-04-05T15:00:00+08:00|4db07b6|2026-04-01|2026-04-07|0bbbf38"
  "3|2026-04-18T15:00:00+08:00|2dad975|2026-04-15|2026-04-21|4db07b6"
  "4|2026-04-25T15:00:00+08:00|60a74d3|2026-04-22|2026-04-28|2dad975"
  "5|2026-05-02T15:00:00+08:00|195d24b|2026-04-29|2026-05-05|60a74d3"
  "6|2026-05-09T15:00:00+08:00|4d6185b|2026-05-06|2026-05-12|195d24b"
  "7|2026-05-16T15:00:00+08:00|3697c83|2026-05-13|2026-05-19|4d6185b"
  "8|2026-05-23T15:00:00+08:00|e86d639|2026-05-20|2026-05-26|3697c83"
)

# 在 worktree 目录执行 git
reorg_git() {
  git -C "${WORKTREE_DIR}" "$@"
}

# 在主仓库执行 git
main_git() {
  git -C "${MAIN_REPO_ROOT}" "$@"
}

worktree_ready() {
  [[ -d "${WORKTREE_DIR}/.git" || -f "${WORKTREE_DIR}/.git" ]] \
    && reorg_git rev-parse --is-inside-work-tree &>/dev/null
}
