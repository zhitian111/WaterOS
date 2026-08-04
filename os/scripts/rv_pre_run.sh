#!/bin/bash
set -euo pipefail
exec python3 "$(dirname "$0")/qemu_run.py" --arch rv --profile pre
