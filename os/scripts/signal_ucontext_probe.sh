#!/bin/sh

case "$(uname -m 2>/dev/null)" in
    riscv64) probe=/signal-ucontext-probe-rv ;;
    loongarch64) probe=/signal-ucontext-probe-la ;;
    *) echo "SIGNAL_UCONTEXT_RESULT status=FAIL arch=unsupported"; exit 1 ;;
esac

"$probe"
