#!/bin/bash

# 引入控制台输出函数
source script/source/console.bash

if [ ! -d .git ]; then
    error "当前目录不是 Git 仓库" 1
fi

declare -A lines_count
declare -A chars_count
total_lines=0
total_chars=0
MAX_BAR_WIDTH=40
PROGRESS_WIDTH=20

# 忽略初始化或特殊文件
EXCLUDE_PATTERN="(^|/)\.obsidian(/|$)|(^|/)build(/|$)|(^|/)tmp(/|$)|(^|/)target(/|$)|(^|/)Cargo.lock"

# 获取所有 git 跟踪文件
mapfile -d '' files < <(git ls-files -z)
total_files=${#files[@]}
current_file_index=0

info "开始统计贡献度，总文件数: $total_files"

# 蓝色系颜色数组
colors=(34 36 36 34 36)  # 深蓝→青蓝→青蓝→深蓝→青蓝
color_count=${#colors[@]}

for file in "${files[@]}"; do
    ((current_file_index++))  # 索引先加，保证进度条完整

    # 彩色蓝色系渐变进度条
    progress_len=$(( PROGRESS_WIDTH * current_file_index / total_files ))
    empty_len=$(( PROGRESS_WIDTH - progress_len ))
    progress_bar=""
    for ((i=1; i<=progress_len; i++)); do
        color_index=$(( i * color_count / PROGRESS_WIDTH ))
        progress_bar+=$(printf "\033[1;%sm█\033[0m" "${colors[$color_index]}")
    done
    progress_bar+=$(printf "%${empty_len}s" "")
    debug "progress([$progress_bar${COLOR_ANSI_BLUE}] $current_file_index/$total_files) -> $file"

    # 忽略文件
    if [[ $file =~ $EXCLUDE_PATTERN ]]; then
        debug "忽略文件: $file"
        continue
    fi

    # 文件统计
    if ! git blame --line-porcelain -- "$file" >/dev/null 2>&1; then
        warning "无法统计文件: $file"
        continue
    fi

    while read -r author l c; do
        lines_count["$author"]=$(( lines_count["$author"] + l ))
        chars_count["$author"]=$(( chars_count["$author"] + c ))
        total_lines=$(( total_lines + l ))
        total_chars=$(( total_chars + c ))
    done < <(
        git blame --line-porcelain -- "$file" | awk '
            /^author / {author=$2; getline; line=$0; lines[author]++; chars[author]+=length(line)}
            END {for(a in lines) print a, lines[a], chars[a]}
        '
    )
done

# 最终强制显示 100% 蓝色进度条
progress_bar=$(printf "\033[1;34m█\033[0m%.0s" $(seq 1 $PROGRESS_WIDTH))
debug "progress([$progress_bar${COLOR_ANSI_BLUE}] $total_files/$total_files) -> 完成"

info "统计完成，开始输出表格"

# 输出表格（高亮 + 对齐）
{
  # 表头
  echo -e "\033[1;37m作者\033[0m | \033[1;37m行数\033[0m | \033[1;37m字符数\033[0m | \033[1;37m百分比\033[0m | \033[1;37m可视化\033[0m"

  # 数据行
  for author in "${!lines_count[@]}"; do
      l=${lines_count[$author]}
      c=${chars_count[$author]}
      percent=$(awk -v l="$l" -v t="$total_lines" 'BEGIN{printf "%.2f", l/t*100}')
      bar_length=$(awk -v p="$percent" -v max="$MAX_BAR_WIDTH" 'BEGIN{printf "%d", p*max/100}')
      bar=$(printf '█%.0s' $(seq 1 $bar_length))
      bar=$(printf "%-${MAX_BAR_WIDTH}s" "$bar") # 右侧空格填充
      echo -e "\033[36m$author\033[0m | \033[32m$l\033[0m | \033[34m$c\033[0m | \033[33m$percent%\033[0m | \033[35m$bar\033[0m"
  done | sort -nr -k4
} | column -t -s '|'

