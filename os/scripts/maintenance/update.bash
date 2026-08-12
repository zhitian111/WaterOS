#!/usr/bin/env bash
# 交互式提交和推送辅助工具。该脚本会执行 git add --all，使用前必须检查工作区。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WOS_LOG_COMPONENT=GIT
source "$SCRIPT_DIR/../source/console.bash"

info "开始更新 Git 仓库"
info "输出当前分支"
git -P branch
if ! git status --porcelain | grep -q .; then
    info "工作区没有待提交的更改"
    git status --short
    info "Git 仓库更新结束"
    exit 0
fi

git add --all
info "已暂存工作区中的全部更改"
git status --short
printf '请输入提交信息: '
read -r commit_msg
git commit -m "$commit_msg"
info "Git 提交已创建"

printf '是否推送到远程仓库，输入 y 确认: '
read -r push_choice
if [[ "$push_choice" == y || "$push_choice" == Y ]]; then
    info "开始推送远程仓库"
    git push
    info "远程仓库推送完成"
else
    info "已跳过远程仓库推送"
fi
info "Git 仓库更新结束"
