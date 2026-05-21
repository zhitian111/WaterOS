#!/usr/bin/env bash
# 在 worktree 中提交指定周次的 os/ 快照
# 用法: ./commit_week.sh <week_num> [--with-system-time]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=config.sh
source "${SCRIPT_DIR}/config.sh"

USE_SYSTEM_TIME=false
WEEK_NUM=""

for arg in "$@"; do
  case "$arg" in
    --with-system-time) USE_SYSTEM_TIME=true ;;
    *)
      [[ -z "$WEEK_NUM" ]] && WEEK_NUM="$arg"
      ;;
  esac
done

if [[ -z "$WEEK_NUM" ]]; then
  echo "用法: $0 <week_num> [--with-system-time]" >&2
  exit 1
fi

if ! worktree_ready; then
  echo "错误: worktree 未就绪 (${WORKTREE_DIR})，请先: ./init.sh -y" >&2
  exit 1
fi

entry=""
for row in "${WEEKLY_PLAN[@]}"; do
  wn="${row%%|*}"
  [[ "$wn" == "$WEEK_NUM" ]] && entry="$row" && break
done

[[ -n "$entry" ]] || { echo "错误: 无效周次 ${WEEK_NUM}" >&2; exit 1; }

IFS='|' read -r _wn commit_date tree_tip week_start week_end _base <<< "$entry"
MSG_FILE="${SCRIPT_DIR}/commit_messages/week$(printf '%02d' "${WEEK_NUM}").txt"
[[ -f "$MSG_FILE" ]] || { echo "错误: 缺少 ${MSG_FILE}" >&2; exit 1; }

if ! main_git rev-parse --verify "${tree_tip}^{commit}" >/dev/null 2>&1; then
  echo "错误: 提交 ${tree_tip} 不存在（在主仓库对象库中查找）" >&2
  exit 1
fi

if [[ "$(reorg_git branch --show-current)" != "${REORG_BRANCH}" ]]; then
  echo "错误: worktree 分支应为 ${REORG_BRANCH}" >&2
  exit 1
fi

if $USE_SYSTEM_TIME; then
  if [[ "$(id -u)" -ne 0 ]]; then
    sudo "${SCRIPT_DIR}/set_system_time.sh" "${commit_date}"
  else
    "${SCRIPT_DIR}/set_system_time.sh" "${commit_date}"
  fi
fi

echo "[*] Week ${WEEK_NUM} (${week_start} ~ ${week_end}): 检出 os/ @ ${tree_tip}"
reorg_git checkout "${tree_tip}" -- "${COMMIT_PATH}/"
reorg_git add "${COMMIT_PATH}/"

if reorg_git diff --cached --quiet; then
  echo "[!] 无变更，跳过"
  exit 0
fi

export GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL
export GIT_AUTHOR_DATE="${commit_date}"
export GIT_COMMITTER_DATE="${commit_date}"

echo "[*] 提交 @ worktree (${commit_date})"
reorg_git commit -F "$MSG_FILE"
echo "[+] Week ${WEEK_NUM}: $(reorg_git log -1 --oneline)"
