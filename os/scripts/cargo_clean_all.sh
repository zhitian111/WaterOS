#!/usr/bin/env bash
# 功能：递归找到指定目录下所有 Cargo 项目，只对有 [workspace] 的 workspace 根目录执行 cargo clean
# 用法：cargo_clean_all.sh [起始目录]
#   - 不传参数：从当前目录递归
#   - 例如：cargo_clean_all.sh os           → 只清理 os 下
#   - 例如：cargo_clean_all.sh os/components → 只清理 components 下

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/source/console.bash" || {
    echo "无法加载日志函数：$SCRIPT_DIR/source/console.bash" >&2
    exit 1
}

# 起始递归目录，默认当前目录
ROOT_DIR="${1:-.}"
# 转为绝对路径，便于 find 与 cd
if [[ -n "$ROOT_DIR" ]]; then
    ROOT_DIR="$(cd "$ROOT_DIR" 2>/dev/null && pwd)" || {
        error "无效的起始目录: $1" 1
    }
fi

info "开始执行 cargo clean（仅针对有 workspace 的项目）"
info "递归起始目录：$ROOT_DIR"

success_count=0
fail_count=0
skipped_count=0

find "$ROOT_DIR" -type f -name "Cargo.toml" \
    -not -path "*/target/*"                           \
    -not -path "*/.git/*"                              \
    -not -path "*/.cargo-default-features-backup/*"    \
    -print0 | while IFS= read -r -d '' cargo_toml; do

    project_dir=$(dirname "$cargo_toml")
    rel_path="${project_dir#$ROOT_DIR/}"

    trace "发现 Cargo.toml：$rel_path"

    # 检查是否包含 [workspace] 字段
    if ! grep -q "^[[:space:]]*\[workspace\]" "$cargo_toml"; then
        debug "非 workspace 项目，跳过：$rel_path"
        ((skipped_count++))
        continue
    fi

    info "找到 workspace 根目录，开始清理：$rel_path"

    (
        cd "$project_dir" || {
            warning "无法进入目录 $rel_path，跳过"
            return
        }

        debug "执行 cargo clean → $(pwd)"

        if cargo clean; then
            info "清理成功：$rel_path"
            ((success_count++))
        else
            err_code=$?
            warning "cargo clean 失败：$rel_path （退出码 $err_code）"
            ((fail_count++))
        fi
    )

done

echo ""
info "清理任务总结（仅统计 workspace 项目）"
info "成功清理 workspace 数量：$success_count"
info "跳过的非 workspace 项目数量：$skipped_count"

if [ $fail_count -eq 0 ]; then
    info "所有 workspace 项目清理成功，无失败"
else
    warning "有 $fail_count 个 workspace 项目清理失败"
fi

trace "脚本执行结束"
