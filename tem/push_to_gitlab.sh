#!/usr/bin/env bash
# 从 worktree 将重整分支推送到 GitLab
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=config.sh
source "${SCRIPT_DIR}/config.sh"

if ! worktree_ready; then
  echo "错误: worktree 未就绪，请先 ./init.sh -y" >&2
  exit 1
fi

# remote 在主仓库配置，worktree 共享
if ! main_git remote get-url "${GITLAB_REMOTE_NAME}" >/dev/null 2>&1; then
  echo "错误: 主仓库未配置 ${GITLAB_REMOTE_NAME}，请先 ./init.sh" >&2
  exit 1
fi

echo "[*] 从 worktree 推送到 ${GITLAB_REMOTE_NAME}/${GITLAB_BRANCH}"
echo "    ${GITLAB_REMOTE_URL}"
reorg_git log --oneline | head -10
echo "    ..."
read -r -p "确认 push? [y/N] " ans
[[ "${ans,,}" == "y" ]] || exit 0

reorg_git push -u "${GITLAB_REMOTE_NAME}" "${REORG_BRANCH}:${GITLAB_BRANCH}"

echo "[+] 推送完成"
