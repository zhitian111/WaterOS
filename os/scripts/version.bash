#!/bin/bash

# 获取脚本目录（用于找到 console.bash）
SCRIPT_DIR=$(dirname "$(readlink -f "$0")")

# 引入控制台颜色与打印函数
source $SCRIPT_DIR/source/console.bash

# ==============================
# 版本号生成逻辑
# ==============================

VERSION_BASE="0.1.0"
STAGE="prototype"
BUILD_NUM=$(git rev-list --count HEAD 2>/dev/null || echo "0")
BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")
DATE=$(date +%Y%m%d)
VERSION="v${COLOR_ANSI_GREEN}${VERSION_BASE}${COLOR_ANSI_CLEAR}-${COLOR_ANSI_PURPLE}${STAGE}${COLOR_ANSI_CLEAR}.${BUILD_NUM}+${COLOR_ANSI_YELLOW}${BRANCH}${COLOR_ANSI_CLEAR}"

# ==============================
# 打印版本信息
# ==============================

info "当前版本信息如下："
echo -e "${COLOR_ANSI_CYAN}WaterOS${COLOR_ANSI_CLEAR}\t${COLOR_ANSI_WHITE}--version${COLOR_ANSI_CLEAR}\t${VERSION}"

