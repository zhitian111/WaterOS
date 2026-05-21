#!/usr/bin/env bash
# init 完成后一键执行：设置系统时间 + 在 worktree 提交全部 7 周
# 用法: sudo ./run_weekly_commits.sh
#       ./run_weekly_commits.sh --no-system-time
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=config.sh
source "${SCRIPT_DIR}/config.sh"

USE_SYSTEM_TIME=true
[[ "${1:-}" == "--no-system-time" ]] && USE_SYSTEM_TIME=false

if ! worktree_ready; then
  echo "错误: worktree 未就绪，请先: ${SCRIPT_DIR}/init.sh -y" >&2
  exit 1
fi

commit_count="$(reorg_git rev-list --count HEAD 2>/dev/null || echo 0)"
if [[ "$commit_count" -gt 1 ]]; then
  echo "[!] worktree 已有 ${commit_count} 个提交"
  if [[ "${REORG_FORCE:-}" != "1" && "${1:-}" != "-y" ]]; then
    read -r -p "继续追加? 重来请先 init.sh -y [y/N] " ans
    [[ "${ans,,}" == "y" ]] || exit 0
  fi
fi

echo "=========================================="
echo " worktree: ${WORKTREE_DIR}"
echo " 作者: ${GIT_AUTHOR_NAME} <${GIT_AUTHOR_EMAIL}>"
echo " 系统时间: $(${USE_SYSTEM_TIME} && echo '是' || echo '否')"
echo "=========================================="

if $USE_SYSTEM_TIME && [[ "$(id -u)" -ne 0 ]]; then
  exec sudo -E "$(readlink -f "$0")" "$@"
fi

for week in 1 3 4 5 6 7 8; do
  echo ""
  echo ">>>>>>>>>> Week ${week} <<<<<<<<<<"
  if $USE_SYSTEM_TIME; then
    "${SCRIPT_DIR}/commit_week.sh" "$week" --with-system-time
  else
    "${SCRIPT_DIR}/commit_week.sh" "$week"
  fi
done

if $USE_SYSTEM_TIME; then
  echo ""
  "${SCRIPT_DIR}/restore_ntp.sh" || true
fi

echo ""
echo "[+] 完成。worktree 历史:"
reorg_git log --oneline --format='%h %ad %s' --date=short
