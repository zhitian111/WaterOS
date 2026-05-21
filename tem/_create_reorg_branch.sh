#!/usr/bin/env bash
# 在主仓库创建/重建 orphan 分支（不切换主仓库当前分支到 reorg 上停留）
# 仅由 init.sh 调用
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=config.sh
source "${SCRIPT_DIR}/config.sh"

INIT_MSG="${SCRIPT_DIR}/commit_messages/init.txt"
[[ -f "$INIT_MSG" ]] || { echo "错误: 缺少 ${INIT_MSG}" >&2; exit 1; }

saved_branch="$(main_git branch --show-current 2>/dev/null || true)"

cleanup() {
  if [[ -n "$saved_branch" ]] && main_git show-ref --verify --quiet "refs/heads/${saved_branch}"; then
    main_git checkout -f "${saved_branch}" 2>/dev/null || true
  elif main_git show-ref --verify --quiet "refs/heads/${MAIN_BRANCH}"; then
    main_git checkout -f "${MAIN_BRANCH}" 2>/dev/null || true
  fi
}
trap cleanup EXIT

if main_git show-ref --verify --quiet "refs/heads/${REORG_BRANCH}"; then
  echo "[*] 删除主仓库中的旧分支 ${REORG_BRANCH}..."
  main_git branch -D "${REORG_BRANCH}"
fi

echo "[*] 在主仓库创建 orphan 分支 ${REORG_BRANCH}..."
main_git checkout --orphan "${REORG_BRANCH}"
main_git rm -rf --cached . 2>/dev/null || true

export GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL
export GIT_AUTHOR_DATE="2026-03-31T12:00:00+08:00"
export GIT_COMMITTER_DATE="${GIT_AUTHOR_DATE}"

main_git commit --allow-empty -F "$INIT_MSG"
echo "[+] orphan 分支已创建: $(main_git rev-parse --short ${REORG_BRANCH})"

# trap 会切回 saved_branch（通常为 main）
