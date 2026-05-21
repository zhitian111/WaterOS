#!/usr/bin/env bash
echo "已弃用：请使用 worktree 方案" >&2
echo "  ./init.sh -y" >&2
echo "  sudo ./run_weekly_commits.sh" >&2
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/init.sh" "$@"
