#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=/dev/null
. "${SCRIPT_DIR}/source/console.bash"

OS_DIR=$(CDPATH= cd -- "${SCRIPT_DIR}/.." && pwd)
DEST_DIR="${HOME}/project/WaterOS_gitlab/os"

GIT_ROOT=$(git -C "${OS_DIR}" rev-parse --show-toplevel)
OS_REL="${OS_DIR#"${GIT_ROOT}/"}"

if [[ "${OS_REL}" == "${OS_DIR}" ]]; then
  error "无法确定 os 目录在 git 仓库中的相对路径: ${OS_DIR}" 1
fi

info "源目录: ${OS_DIR}"
info "目标目录: ${DEST_DIR}"
info "导出 git 追踪文件 (前缀: ${OS_REL}/)..."

mkdir -p "${DEST_DIR}"

count=0
skipped=0
while IFS= read -r -d '' tracked; do
  rel="${tracked#"${OS_REL}/"}"
  src="${GIT_ROOT}/${tracked}"
  dst="${DEST_DIR}/${rel}"

  if [[ ! -e "${src}" ]]; then
    warning "跳过缺失文件: ${tracked}"
    skipped=$((skipped + 1))
    continue
  fi

  mkdir -p "$(dirname "${dst}")"
  cp -f "${src}" "${dst}"
  count=$((count + 1))
done < <(git -C "${GIT_ROOT}" ls-files -z -- "${OS_REL}")

info "已覆盖拷贝 ${count} 个文件到 ${DEST_DIR}"
if [[ "${skipped}" -gt 0 ]]; then
  warning "跳过 ${skipped} 个在索引中但工作区不存在的文件"
fi
