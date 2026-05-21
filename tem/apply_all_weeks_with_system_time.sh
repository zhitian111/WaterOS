#!/usr/bin/env bash
# 已合并到 run_weekly_commits.sh，保留本文件作兼容入口
exec sudo -E "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/run_weekly_commits.sh" "$@"
