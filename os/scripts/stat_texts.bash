#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/source/console.bash"

# 并行线程数（可由 Makefile 传入）
# THREADS="${THREADS:-4}"
THREADS=$(nproc)

info "并行线程数：$THREADS"


# ======================================================
# 默认忽略目录与文件模式
# ======================================================

DEFAULT_IGNORE_DIRS=(
  ".git"
  "node_modules"
  "target"
  ".idea"
)

DEFAULT_IGNORE_FILES=(
  '\.DS_Store$'
  '\.png$'
  '\.jpg$'
  '\.jpeg$'
  '\.gif$'
  '\.zip$'
  '\.tar$'
  '\.gz$'
  '\.exe$'
  '\.dll$'
  '\.so$'
)

IGNORE_DIRS=("${DEFAULT_IGNORE_DIRS[@]}")
IGNORE_REGEX=("${DEFAULT_IGNORE_FILES[@]}")


# ======================================================
# 加载多级 .gitignore
# ======================================================

load_gitignore() {
  while IFS= read -r gitignore; do
    [[ ! -f "$gitignore" ]] && continue

    trace "加载 Gitignore: $gitignore"

    while IFS= read -r rule || [[ -n "$rule" ]]; do
      [[ -z "$rule" || "$rule" =~ ^# ]] && continue

      # 目录规则
      if [[ "$rule" == */ ]]; then
        clean="${rule#/}"
        clean="${clean%/}"
        IGNORE_DIRS+=("$clean")
        continue
      fi

      # 文件模式
      clean="${rule#/}"
      regex=$(printf "%s" "$clean" | sed 's/\./\\./g; s/\*/.*/g; s/\?/./g')
      IGNORE_REGEX+=("$regex")
    done < "$gitignore"

  done < <(find . -type f -name ".gitignore")
}

matches_any() {
  local str="$1"
  shift
  for pat in "$@"; do
    [[ "$str" =~ $pat ]] && return 0
  done
  return 1
}

is_text() {
  file "$1" | grep -q "text"
}


# ======================================================
# 加载忽略规则 / 构建 prune
# ======================================================

load_gitignore

trace "目录忽略规则：${IGNORE_DIRS[*]}"
trace "文件忽略规则：${IGNORE_REGEX[*]}"

PRUNE=()
for d in "${IGNORE_DIRS[@]}"; do
  PRUNE+=(-path "./$d" -prune -o)
done


# ======================================================
# 主逻辑
# ======================================================

declare -A DIR_LINES DIR_CHARS

TOTAL_LINES=0
TOTAL_CHARS=0
CURRENT_DIR=""

# 临时输出目录（保证并行不冲突）
TMPDIR="$(mktemp -d)"
trap "rm -rf '$TMPDIR'" EXIT

# ======================================================
# 1. 单线程扫描文件路径（顺序不乱）
# ======================================================

FILES=()
while IFS= read -r -d '' file; do
  path="${file#./}"

  # 文件级忽略
  if matches_any "$path" "${IGNORE_REGEX[@]}"; then
    trace "忽略文件：$path"
    continue
  fi

  if ! is_text "$file"; then
    continue
  fi

  FILES+=("$path")
done < <(find . "${PRUNE[@]}" -type f -print0 | sort -z)



# ======================================================
# 2. 并行统计每个文件 wc -m/wc -l
# ======================================================

info "开始并行统计（确保输出顺序不乱）"

printf "%s\n" "${FILES[@]}" | \
  xargs -I {} -P "$THREADS" bash -c '
    p="{}"
    file="./$p"
    chars=$(wc -m < "$file" | tr -d "[:space:]")
    lines=$(wc -l < "$file" | tr -d "[:space:]")
    printf "%s:%s:%s\n" "$p" "$chars" "$lines" > "'$TMPDIR'"/"$(echo "$p" | sed "s/\//_/g")"
  '



# ======================================================
# 3. 主线程按顺序读取结果并输出（风格不变）
# ======================================================

for path in "${FILES[@]}"; do
  FILE_DIR=$(dirname "$path")

  # 目录切换 → 输出上一目录的汇总
  if [[ -n "$CURRENT_DIR" && "$CURRENT_DIR" != "$FILE_DIR" ]]; then
    info "--------------------------------------"
    info "目录统计完成：$CURRENT_DIR"
    info " 字符总数：${DIR_CHARS[$CURRENT_DIR]:-0}"
    info " 行数总数：${DIR_LINES[$CURRENT_DIR]:-0}"
    info "--------------------------------------"
  fi
  CURRENT_DIR="$FILE_DIR"

  tmpfile="$TMPDIR/$(echo "$path" | sed "s/\//_/g")"
  IFS=':' read -r p chars lines < "$tmpfile"

  trace "文件: $path"
  debug "字符: $chars 行: $lines"

  DIR_CHARS[$FILE_DIR]=$(( ${DIR_CHARS[$FILE_DIR]:-0} + chars ))
  DIR_LINES[$FILE_DIR]=$(( ${DIR_LINES[$FILE_DIR]:-0} + lines ))

  TOTAL_CHARS=$((TOTAL_CHARS + chars))
  TOTAL_LINES=$((TOTAL_LINES + lines))
done



# ======================================================
# 4. 输出最后目录与总统计
# ======================================================

if [[ -n "$CURRENT_DIR" ]]; then
  info "--------------------------------------"
  info "目录统计完成：$CURRENT_DIR"
  info " 字符总数：${DIR_CHARS[$CURRENT_DIR]:-0}"
  info " 行数总数：${DIR_LINES[$CURRENT_DIR]:-0}"
  info "--------------------------------------"
fi

info "--------------------------------------"
info "最终统计汇总"
info "--------------------------------------"
info "总字符数：$TOTAL_CHARS"
info "总行数：$TOTAL_LINES"
info "--------------------------------------"

info "统计完成"

