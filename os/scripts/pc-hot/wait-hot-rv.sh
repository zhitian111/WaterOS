#!/usr/bin/env bash
# RISC-V entry point for wait-hot.
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/wait-hot.sh" rv "$@"
