#!/usr/bin/env bash
# 初始化 GitLab 重整环境（git worktree 方案，主仓库保持 main 不动）
# 用法: ./init.sh [-y]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=config.sh
source "${SCRIPT_DIR}/config.sh"

AUTO_YES=false
[[ "${1:-}" == "-y" || "${REORG_FORCE:-}" == "1" ]] && AUTO_YES=true

echo "=========================================="
echo " WaterOS GitLab 重整 — worktree 初始化"
echo " 主仓库: ${MAIN_REPO_ROOT}"
echo " worktree: ${WORKTREE_DIR}"
echo "=========================================="

# 1. 主仓库配置 gitlab remote
if main_git remote get-url "${GITLAB_REMOTE_NAME}" >/dev/null 2>&1; then
  url="$(main_git remote get-url "${GITLAB_REMOTE_NAME}")"
  [[ "$url" == "$GITLAB_REMOTE_URL" ]] || main_git remote set-url "${GITLAB_REMOTE_NAME}" "${GITLAB_REMOTE_URL}"
else
  echo "[*] 添加 remote ${GITLAB_REMOTE_NAME}"
  main_git remote add "${GITLAB_REMOTE_NAME}" "${GITLAB_REMOTE_URL}"
fi

# 2. 主仓库切回 main
current="$(main_git branch --show-current 2>/dev/null || true)"
if [[ "$current" != "${MAIN_BRANCH}" ]]; then
  if main_git show-ref --verify --quiet "refs/heads/${MAIN_BRANCH}"; then
    echo "[*] 主仓库: ${current} → ${MAIN_BRANCH}"
    main_git checkout -f "${MAIN_BRANCH}"
  fi
fi

# 3. 移除旧 worktree
if main_git worktree list --porcelain 2>/dev/null | grep -qF "worktree ${WORKTREE_DIR}"; then
  echo "[!] 已存在 worktree: ${WORKTREE_DIR}"
  if ! $AUTO_YES; then
    read -r -p "是否删除并重建 worktree? [y/N] " ans
    [[ "${ans,,}" == "y" ]] || exit 0
  fi
  main_git worktree remove --force "${WORKTREE_DIR}"
elif [[ -d "${WORKTREE_DIR}" ]]; then
  echo "[!] 残留目录: ${WORKTREE_DIR}"
  $AUTO_YES || { read -r -p "删除? [y/N] " ans; [[ "${ans,,}" == "y" ]] || exit 1; }
  rm -rf "${WORKTREE_DIR}"
fi

# 4. 重建 reorg 分支
rebuild_branch=true
if main_git show-ref --verify --quiet "refs/heads/${REORG_BRANCH}"; then
  if ! $AUTO_YES; then
    read -r -p "是否重建分支 ${REORG_BRANCH}? [Y/n] " ans
    [[ "${ans,,}" == "n" ]] && rebuild_branch=false
  fi
  $rebuild_branch && main_git branch -D "${REORG_BRANCH}"
fi

if $rebuild_branch || ! main_git show-ref --verify --quiet "refs/heads/${REORG_BRANCH}"; then
  "${SCRIPT_DIR}/_create_reorg_branch.sh"
fi

# 5. 添加 worktree
echo "[*] 添加 worktree → ${WORKTREE_DIR}"
main_git worktree add -B "${REORG_BRANCH}" "${WORKTREE_DIR}" "${REORG_BRANCH}"

main_git checkout -f "${MAIN_BRANCH}" 2>/dev/null || true

cat <<EOF

[+] 初始化完成

  主仓库:  $(main_git branch --show-current) @ ${MAIN_REPO_ROOT}
  worktree: $(reorg_git branch --show-current) @ ${WORKTREE_DIR}

下一步（仍在主仓库 tem 目录）:

  sudo ./run_weekly_commits.sh
  ./push_to_gitlab.sh

EOF
