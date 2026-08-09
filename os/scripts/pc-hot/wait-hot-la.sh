#!/usr/bin/env bash
# LoongArch entry point for wait-hot.
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/wait-hot.sh" la "$@"
