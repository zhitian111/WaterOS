#!/usr/bin/env bash
# 功能：递归找到指定目录下所有 Cargo 项目，只对有 [workspace] 的 workspace 根目录执行 cargo clean
# 用法：cargo_clean_all.sh [起始目录]
#   - 不传参数：从当前目录递归
#   - 例如：cargo_clean_all.sh os           → 只清理 os 下
#   - 例如：cargo_clean_all.sh os/components → 只清理 components 下

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WOS_LOG_COMPONENT=CLEAN
source "$SCRIPT_DIR/../source/console.bash" || {
    echo "无法加载日志函数：$SCRIPT_DIR/../source/console.bash" >&2
    exit 1
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    cat <<EOF
用法: ${0##*/} [ROOT_DIR]

递归查找 ROOT_DIR 下的 Cargo workspace 根目录并执行 cargo clean。
ROOT_DIR 默认为当前目录。该操作会删除匹配 workspace 的编译产物。
EOF
    exit 0
fi

# 起始递归目录，默认当前目录
ROOT_DIR="${1:-.}"
# 转为绝对路径，便于 find 与 cd
if [[ -n "$ROOT_DIR" ]]; then
    ROOT_DIR="$(cd "$ROOT_DIR" 2>/dev/null && pwd)" || {
        error "无效的起始目录 path=$1" 1
    }
fi

info "开始清理 Cargo workspaces root=${ROOT_DIR}"

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

    info "开始清理 workspace path=${rel_path}"

    (
        cd "$project_dir" || {
            warning "无法进入 workspace path=${rel_path} action=skip"
            return
        }

        debug "执行 Cargo 清理 directory=$(pwd)"

        if cargo clean; then
            info "workspace 清理完成 path=${rel_path}"
            ((success_count++))
        else
            err_code=$?
            warning "workspace 清理失败 path=${rel_path} exit_code=${err_code}"
            ((fail_count++))
        fi
    )

done

info "清理统计 cleaned=${success_count} skipped=${skipped_count} failed=${fail_count}"

if [ $fail_count -eq 0 ]; then
    info "Cargo workspace 清理完成"
else
    warning "部分 Cargo workspace 清理失败 failed=${fail_count}"
fi

trace "脚本执行结束"
