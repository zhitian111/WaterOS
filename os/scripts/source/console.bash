#!/bin/sh
# Shell 脚本共用日志接口。本文件只供其他脚本 source，不单独执行。
#
# 输出格式：
#   [组件][级别] 信息
#
# 可通过 WOS_LOG_COMPONENT 指定组件名，通过 NO_COLOR 禁用颜色。重定向输出时
# 默认不使用 ANSI 颜色，保证日志可以直接被 grep、tee 和测试脚本处理。

COLOR_ANSI_RED="\033[31m"
COLOR_ANSI_GREEN="\033[32m"
COLOR_ANSI_YELLOW="\033[33m"
COLOR_ANSI_BLUE="\033[34m"
COLOR_ANSI_PURPLE="\033[35m"
COLOR_ANSI_CYAN="\033[36m"
COLOR_ANSI_WHITE="\033[37m"
COLOR_ANSI_CLEAR="\033[0m"

_wos_log_component() {
  printf '%s' "${WOS_LOG_COMPONENT:-SCRIPT}"
}

_wos_log_color_enabled() {
  [ -z "${NO_COLOR:-}" ] && [ -t 2 ]
}

_wos_log() {
  level="$1"
  color="$2"
  shift 2
  component="$(_wos_log_component)"

  if _wos_log_color_enabled; then
    printf '%b[%s][%s]%b %s\n' "$color" "$component" "$level" \
      "$COLOR_ANSI_CLEAR" "$*" >&2
  else
    printf '[%s][%s] %s\n' "$component" "$level" "$*" >&2
  fi
}

trace() {
  _wos_log TRACE "$COLOR_ANSI_CYAN" "$@"
}

debug() {
  _wos_log DEBUG "$COLOR_ANSI_BLUE" "$@"
}

info() {
  _wos_log INFO "$COLOR_ANSI_GREEN" "$@"
}

warning() {
  _wos_log WARN "$COLOR_ANSI_YELLOW" "$@"
}

error() {
  message="$1"
  status="${2:-1}"
  _wos_log ERROR "$COLOR_ANSI_RED" "$message exit_code=$status"
  exit "$status"
}
