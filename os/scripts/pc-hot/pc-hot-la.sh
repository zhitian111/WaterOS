#!/usr/bin/env bash
# LoongArch entry point for pc-hot (per-PC instruction counter + symbol analysis).
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/pc-hot.sh" la "$@"
